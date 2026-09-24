use crate::*;
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, OnceLock, TryLockError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PROTOCOL_VERSION: u32 = 2;
const USER_AGENT: &str = "/QUB Core:1.8.6/"; // HF130 bounded recovery; unchanged wire protocol
const LAN_DISCOVERY_MAGIC: &str = "qub-lan-discovery-v1";
const GLOBAL_PEER_LIVE_SECS: u64 = 900;
const RELAY_REACHABILITY_CACHE_SECS: u64 = 300;
const MINING_GUARD_SYNC_ROUNDS: usize = 20;
const MINING_GUARD_SYNC_SLEEP_MS: u64 = 250;
// HF53/v1.5.2: bounded background catch-up used by mining guard. This is
// intentionally longer than GUI button sync, but it runs only in miner worker
// threads so the UI never freezes.
const MINING_GUARD_CATCHUP_ROUNDS: usize = 8;
const MINING_GUARD_CATCHUP_SLEEP_MS: u64 = 220;
// HF55/v1.5.2: adaptive fork-window sync. When a node is stuck on a local
// branch, do not pull the whole chain from genesis first. Probe overlapping
// windows backwards from the local tip: 2,4,8,16,... blocks. This finds a
// common ancestor quickly and replaces the old "fresh reinstall fixes sync" flow.
const ADAPTIVE_SYNC_WINDOWS: &[u32] = &[2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096];
// HF58/v1.5.2: if incremental/adaptive suffix sync cannot catch up,
// fall back to a checkpoint-anchored direct-peer chain pull. This is the
// emergency path that replaces the old user workaround: uninstall/reinstall.
const FORCE_ANCHOR_SYNC_TIMEOUT_MS: u64 = 60_000;
const FORCE_ANCHOR_SYNC_READ_TIMEOUT_MS: u64 = 12_000;
const FORCE_ANCHOR_SYNC_MAX_PEERS: usize = 4;
// HF60/v1.5.2 fixed3: explicit official-seed snapshot pull used by CLI Sync
// and as the first mining catch-up step. It is deliberately short and does
// not walk the whole stale peer registry.
const OFFICIAL_SNAPSHOT_SYNC_TIMEOUT_MS: u64 = 20_000;
const OFFICIAL_SNAPSHOT_READ_TIMEOUT_MS: u64 = 8_000;
// HF72/v1.5.8: after a successful official seed/tail repair, avoid re-running
// expensive guard syncs for every candidate immediately. HF82 keeps this cache
// short enough to notice a moving mainnet tip, but long enough to avoid the
// HF88 green-light worker pile-up that could starve wallet/snapshot refresh.
const FRESH_TIP_TRUST_WINDOW_SECS: u64 = 45;
// HF80/v1.6.1: keep mining guards fast and bounded. HF88/v1.6.2 keeps the
// proven HF80/HF78 mining path and replaces the HF81 recursive self-heal with
// a single-flight, strictly bounded catch-up ladder.
const HF80_MINING_FAST_GATE_MS: u64 = 12_000;
const HF80_MINING_PARENT_GATE_MS: u64 = 8_000;
const HF80_PEER_STATUS_PROBE_MS: u64 = 260;
const HF80_PEER_STATUS_FETCH_MS: u64 = 320;
const HF82_AUTO_CATCHUP_MS: u64 = 8_000;
const HF82_MINING_CATCHUP_MS: u64 = 14_000;
const HF82_PARENT_CATCHUP_MS: u64 = 10_000;
const HF82_LIGHT_TIP_PROBE_MS: u64 = 320;
// HF107/v1.6.9: mainnet mining must never green-light an old local tip merely
// because a higher advertised tip was hard to fetch. That caused “fake local
// block” branches when users were 10-30+ blocks behind. Quarantine is retained
// for non-mainnet diagnostics only; mainnet waits for official/direct catch-up.
const HF98_UNCATCHABLE_TIP_QUARANTINE_SECS: u64 = 900;
const HF98_UNCATCHABLE_TIP_MAX_GAP: u32 = 512;
const HF125_BLOCK_ACK_TIMEOUT_SECS: u64 = 30;
const HF125_BLOCK_RELAY_TOTAL_SECS: u64 = 40;
const HF125_BLOCK_RELAY_MAX_PEERS: usize = 24;
const HF125_BLOCK_RELAY_RETRY_SECS: u64 = 6;
const HF125_PRIORITY_INBOUND_RESERVE: usize = 16;
const HF125_PRIORITY_MAX_MESSAGES: usize = 96;
const HF125_PRIORITY_MAX_SUBMISSIONS: usize = 2;
const HF125_PENDING_BLOCK_RELAY_VERSION: u32 = 1;
// HF127/v1.8.3: transaction delivery receives the same explicit,
// acknowledgement-based treatment as HF125 block delivery. These values are
// transport policy only; they do not change transaction or block consensus.
const HF127_TX_ACK_TIMEOUT_SECS: u64 = 20;
const HF127_TX_RELAY_TOTAL_SECS: u64 = 30;
const HF127_TX_RELAY_MAX_PEERS: usize = 16;
const HF127_TX_RETRY_IDLE_SECS: u64 = 300;
const HF127_TX_RETRY_MAX_PER_PASS: usize = 8;
const HF127_PRIORITY_MAX_TX_SUBMISSIONS: usize = 2;
const HF127_PRIORITY_HANDSHAKE_SECS: u64 = 3;
const HF127_PRIORITY_SESSION_SECS: u64 = 30;
const HF127_NORMAL_TX_SUBMIT_WINDOW_SECS: u64 = 10;
const HF127_NORMAL_TX_SUBMITS_PER_WINDOW: usize = 16;
const HF127_PEERBOOK_MAX: usize = 512;
const HF127_OUTBOUND_DIALS_PER_TICK: usize = 4;
const HF127_KNOWN_PEER_REFRESH_SECS: u64 = 15;
const HF127_PEER_OBSERVATION_WRITE_SECS: u64 = 30;
const HF127_STATUS_LOG_SECS: u64 = 15;
const HF129_OFFICIAL_TIP_MAX_AGE_SECS: u64 = 3 * 60;
const HF129_OFFICIAL_TIP_FUTURE_SKEW_SECS: u64 = 120;

// HF130: deadlines belong to the operation, not to individual socket reads.
// Nested sync stages inherit the shorter deadline; a scoped probe explicitly
// inherits this context as well. Cancellation never detaches a chain writer.
#[derive(Clone, Default)]
struct Hf130IoContext {
    deadline: Option<Instant>,
    cancel: Option<Arc<AtomicBool>>,
    completion: Option<Arc<AtomicBool>>,
}

thread_local! {
    static HF130_IO_CONTEXT: RefCell<Hf130IoContext> = RefCell::new(Hf130IoContext::default());
}

struct Hf130IoScope(Hf130IoContext);

impl Hf130IoScope {
    fn enter(mut context: Hf130IoContext) -> Self {
        HF130_IO_CONTEXT.with(|slot| {
            let previous = slot.borrow().clone();
            if let Some(parent) = previous.deadline {
                context.deadline = Some(context.deadline.map_or(parent, |d| d.min(parent)));
            }
            if context.cancel.is_none() {
                context.cancel = previous.cancel.clone();
            }
            if context.completion.is_none() {
                context.completion = previous.completion.clone();
            }
            *slot.borrow_mut() = context;
            Self(previous)
        })
    }

    fn deadline(deadline: Instant) -> Self {
        Self::enter(Hf130IoContext { deadline: Some(deadline), cancel: None, completion: None })
    }
}

impl Drop for Hf130IoScope {
    fn drop(&mut self) {
        HF130_IO_CONTEXT.with(|slot| *slot.borrow_mut() = self.0.clone());
    }
}

/// Cooperatively cancel mining guards, including their nested network work.
/// The caller retains its controller until this closure has actually returned.
pub fn hf130_with_mining_cancellation<T>(
    stop: Arc<AtomicBool>,
    operation: impl FnOnce() -> T,
) -> T {
    let _scope = Hf130IoScope::enter(Hf130IoContext { deadline: None, cancel: Some(stop), completion: None });
    operation()
}

fn hf130_io_context() -> Hf130IoContext {
    HF130_IO_CONTEXT.with(|slot| slot.borrow().clone())
}

fn hf130_effective_deadline(deadline: Instant) -> Instant {
    HF130_IO_CONTEXT.with(|slot| slot.borrow().deadline.map_or(deadline, |d| d.min(deadline)))
}

fn hf130_io_remaining(deadline: Instant) -> io::Result<Duration> {
    if HF130_IO_CONTEXT.with(|slot| {
        let context = slot.borrow();
        context.cancel.as_ref().map_or(false, |stop| stop.load(Ordering::Relaxed))
            || context.completion.as_ref().map_or(false, |stop| stop.load(Ordering::Relaxed))
    }) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "mining operation cancelled"));
    }
    let left = hf130_effective_deadline(deadline).saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::Error::new(io::ErrorKind::TimedOut, "operation deadline expired"))
    } else {
        Ok(left)
    }
}


/// Cooperative CPU/storage checkpoint. Socket deadlines only bound network
/// work: expiry during validation must not discard an otherwise valid chunk.
/// Explicit controller cancellation still stops private replay before commit.
pub(crate) fn hf132_check_sync_budget() -> Result<()> {
    let context = hf130_io_context();
    if context.cancel.as_ref().map_or(false, |flag| flag.load(Ordering::Relaxed))
        || context.completion.as_ref().map_or(false, |flag| flag.load(Ordering::Relaxed))
    {
        bail!("mining operation cancelled");
    }
    Ok(())
}

/// Reserve network opportunity for the remaining peers. Validation may finish
/// and persist its accepted chunk after this socket slice expires.
fn hf132_peer_io_deadline(deadline: Instant, peers_left: usize) -> Instant {
    let now = Instant::now();
    let left = hf130_effective_deadline(deadline).saturating_duration_since(now);
    now + left / peers_left.clamp(1, 4) as u32
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct P2PSyncReport {
    pub peers_contacted: usize,
    pub peer_errors: usize,
    pub best_peer_height: u32,
    pub chains_adopted: usize,
    pub blocks_connected: usize,
    pub txs_accepted: usize,
    pub height: u32,
    pub tip_hash: String,
    #[serde(default)]
    pub writer_acquired: bool,
    #[serde(default)]
    pub writer_busy: bool,
    #[serde(default)]
    pub progressed: bool,
    #[serde(default)]
    pub fresh_at_tip: bool,
    #[serde(default)]
    pub canonical_tip_proven: bool,
    #[serde(default)]
    pub terminal: String,
    #[serde(default)]
    pub sync_reason: String,
    #[serde(default)]
    pub before_height: u32,
    #[serde(default)]
    pub before_tip_hash: String,
    #[serde(default)]
    pub best_peer_tip_hash: String,
    // Process-local evidence. Deserializing an old report must never recreate
    // freshness, and this does not change the public report JSON schema.
    #[serde(skip)]
    pub canonical_proof: Option<Hf130CanonicalProof>,
}

#[derive(Debug, Clone, Copy)]
pub enum Hf130CanonicalEvidence {
    OfficialTip,
    OfficialHttp,
    OfficialTcp,
    DirectQuorum,
}

#[derive(Debug, Clone)]
pub struct Hf130CanonicalProof {
    pub height: u32,
    pub tip_hash: String,
    pub observed_at: Instant,
    pub source: Hf130CanonicalEvidence,
}


#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockRelayPeerResult {
    pub peer: String,
    pub official: bool,
    pub transport: String,
    pub status: String,
    pub height: u32,
    pub tip_hash: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BlockRelayReport {
    pub block_hash: String,
    pub height: u32,
    pub attempts: usize,
    pub official_attempts: usize,
    pub connected: usize,
    pub acknowledgements: usize,
    pub official_acknowledgements: usize,
    pub accepted: usize,
    pub already_known: usize,
    pub stale_parent: usize,
    pub rejected: usize,
    pub legacy_sent: usize,
    pub timed_out: usize,
    pub pending_retry: bool,
    pub peer_results: Vec<BlockRelayPeerResult>,
}

impl BlockRelayReport {
    pub fn acknowledged(&self) -> usize {
        self.accepted.saturating_add(self.already_known)
    }

    fn distinct_positive_ack_peers(&self, official: Option<bool>) -> usize {
        self.peer_results
            .iter()
            .filter(|result| {
                official.map(|expected| result.official == expected).unwrap_or(true)
                    && matches!(result.transport.as_str(), "ack" | "ack_after_suffix" | "ack_after_overlap")
                    && matches!(result.status.as_str(), "accepted" | "already_known")
            })
            .map(|result| normalize_peer_addr(&result.peer))
            .filter(|peer| !peer.is_empty())
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn delivered(&self) -> bool {
        // HF127/v1.8.3: official seeds remain the preferred low-latency proof,
        // but they are not a liveness single point of failure. Delivery is
        // proven by one positive official ACK, by a quorum of two distinct
        // non-official ACK peers, or by one explicit ACK in isolated/regtest
        // deployments where no official destination was attempted.
        if self.official_acknowledgements > 0 {
            return true;
        }
        let direct = self.distinct_positive_ack_peers(Some(false));
        if direct >= 2 {
            return true;
        }
        self.official_attempts == 0 && self.distinct_positive_ack_peers(None) >= 1
    }

    pub fn summary(&self) -> String {
        format!(
            "acknowledged={} official_acknowledged={} direct_ack_peers={} accepted={} already_known={} stale_parent={} rejected={} legacy_sent={} timed_out={} attempts={} official_attempts={}",
            self.acknowledged(),
            self.official_acknowledgements,
            self.distinct_positive_ack_peers(Some(false)),
            self.accepted,
            self.already_known,
            self.stale_parent,
            self.rejected,
            self.legacy_sent,
            self.timed_out,
            self.attempts,
            self.official_attempts,
        )
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TxRelayPeerResult {
    pub peer: String,
    pub official: bool,
    pub transport: String,
    pub status: String,
    pub height: u32,
    pub tip_hash: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TxRelayReport {
    pub txid: String,
    pub attempts: usize,
    pub official_attempts: usize,
    pub connected: usize,
    pub acknowledgements: usize,
    pub official_acknowledgements: usize,
    pub accepted: usize,
    pub already_known: usize,
    pub already_confirmed: usize,
    pub rejected: usize,
    pub legacy_sent: usize,
    pub timed_out: usize,
    pub pending_retry: bool,
    pub peer_results: Vec<TxRelayPeerResult>,
}

impl TxRelayReport {
    pub fn acknowledged(&self) -> usize {
        self.accepted
            .saturating_add(self.already_known)
            .saturating_add(self.already_confirmed)
    }

    fn distinct_positive_ack_peers(&self, official: Option<bool>) -> usize {
        self.peer_results
            .iter()
            .filter(|result| {
                official.map(|expected| result.official == expected).unwrap_or(true)
                    && result.transport == "ack"
                    && matches!(
                        result.status.as_str(),
                        "accepted" | "already_known" | "already_confirmed"
                    )
            })
            .map(|result| normalize_peer_addr(&result.peer))
            .filter(|peer| !peer.is_empty())
            .collect::<HashSet<_>>()
            .len()
    }

    pub fn delivered(&self) -> bool {
        // HF127/v1.8.3: prefer official acknowledgement without requiring a
        // seed to be online forever. Two distinct direct peers are a durable
        // network-escape quorum. In isolated/regtest deployments with no
        // official attempts, one explicit positive ACK is sufficient.
        if self.official_acknowledgements > 0 {
            return true;
        }
        let direct = self.distinct_positive_ack_peers(Some(false));
        if direct >= 2 {
            return true;
        }
        self.official_attempts == 0 && self.distinct_positive_ack_peers(None) >= 1
    }

    pub fn summary(&self) -> String {
        format!(
            "acknowledged={} official_acknowledged={} direct_ack_peers={} accepted={} already_known={} already_confirmed={} rejected={} legacy_sent={} timed_out={} attempts={} official_attempts={}",
            self.acknowledged(),
            self.official_acknowledgements,
            self.distinct_positive_ack_peers(Some(false)),
            self.accepted,
            self.already_known,
            self.already_confirmed,
            self.rejected,
            self.legacy_sent,
            self.timed_out,
            self.attempts,
            self.official_attempts,
        )
    }
}

#[derive(Debug, Clone)]
struct TxAcceptance {
    status: String,
    txid: String,
    height: u32,
    tip_hash: String,
    reason: String,
    changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PendingBlockRelay {
    version: u32,
    network: String,
    block_hash: String,
    height: u32,
    block: Block,
    created_unix: u64,
    last_attempt_unix: u64,
    attempts: u64,
    last_acknowledgements: usize,
    last_error: String,
    #[serde(default)]
    last_stale_parent_height: u32,
    #[serde(default)]
    last_stale_parent_tip: String,
    #[serde(default)]
    last_stale_parent_reports: usize,
    #[serde(default)]
    last_official_stale_parent_reports: usize,
}

#[derive(Debug, Clone)]
struct BlockAcceptance {
    status: String,
    block_hash: String,
    height: u32,
    tip_hash: String,
    reason: String,
    changed: bool,
}

static HF125_AUTO_CATCHUP_RUNNING: AtomicBool = AtomicBool::new(false);
static HF125_BLOCK_RETRY_RUNNING: AtomicBool = AtomicBool::new(false);

struct Hf125AutoCatchupGuard;

impl Drop for Hf125AutoCatchupGuard {
    fn drop(&mut self) {
        HF125_AUTO_CATCHUP_RUNNING.store(false, Ordering::Release);
    }
}

struct Hf125BlockRetryGuard;

impl Drop for Hf125BlockRetryGuard {
    fn drop(&mut self) {
        HF125_BLOCK_RETRY_RUNNING.store(false, Ordering::Release);
    }
}

fn hf125_block_relay_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn hf127_tx_acceptance_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn load_chain_for_hf90_catchup(settings: &Settings) -> Result<ChainState> {
    // HF107/v1.6.9: catch-up workers can be invoked repeatedly while the GUI is
    // rendering. Replaying the whole chain at every tiny probe is slow enough to
    // let the public tip move away again. Use the persisted chain/UTXO snapshot
    // with checkpoint + recent-link checks for the read side, then consensus-
    // validate every newly received block through connect_block / from_blocks.
    // A busy live owner is retryable; falling back here replayed the full chain
    // while the existing writer was still working.
    load_or_init_chain_for_ui_fast(settings)
}

// Snapshot append validation needs a consensus-validated UTXO baseline. A
// structural UI snapshot alone is insufficient for the validated-candidate API.
fn load_chain_for_hf132_validated_catchup(settings: &Settings) -> Result<ChainState> {
    if let Some(local) = try_live_chain_snapshot(settings)? { return Ok(local); }
    load_or_init_chain(settings)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct P2PNetworkSnapshot {
    pub enabled: bool,
    pub known_peers: usize,
    /// Direct TCP-reachable peers from this node.
    pub reachable_peers: usize,
    /// Direct TCP-reachable peers from this node. Kept explicit so the GUI can
    /// separate direct connectivity from globally-live miner telemetry.
    pub direct_reachable_peers: usize,
    /// Peers seen recently through the network/seed registry, including peers
    /// behind NAT that are mining and syncing but cannot be dialed directly.
    pub globally_live_peers: usize,
    /// This node appears suitable as a public relay candidate. This is a
    /// conservative local heuristic: only public, non-private advertised
    /// addresses are marked relay-capable. No node is promoted to an official
    /// seed automatically.
    pub relay_capable: bool,
    /// This node is probably behind NAT/private networking and should not
    /// advertise itself as a reachable relay endpoint.
    pub nat_private: bool,
    /// Short warning displayed by the GUI when peer telemetry suggests a stale
    /// or split view. Registry-only high tips are not trusted for mining.
    pub stale_warning: String,
    pub peers: Vec<P2PPeerStatus>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct P2PPeerStatus {
    pub addr: String,
    /// Direct TCP probe succeeded from this local node.
    pub reachable: bool,
    /// Seen recently by the network/seed registry. This can be true even when
    /// direct TCP probe fails because the remote miner is behind NAT/firewall.
    pub global_live: bool,
    pub height: Option<u32>,
    pub tip_hash: Option<String>,
    pub user_agent: Option<String>,
    pub error: Option<String>,
    pub node_id: Option<String>,
    pub observed_addr: Option<String>,
    pub listen_addr: Option<String>,
    pub role: Option<String>,
    pub miner_address: Option<String>,
    pub last_seen_unix: Option<u64>,
    pub seen_age_secs: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct P2PObservedPeer {
    pub node_id: String,
    pub observed_addr: String,
    pub listen_addr: String,
    pub height: u32,
    pub tip_hash: String,
    pub user_agent: String,
    pub role: String,
    pub miner_address: String,
    pub last_seen_unix: u64,
}

#[derive(Debug, Clone, Default)]
struct P2PProbeInfo {
    height: u32,
    tip_hash: String,
    user_agent: String,
    node_id: String,
    listen_addr: String,
    role: String,
    miner_address: String,
}

#[derive(Debug, Default)]
struct FreshTipTrustCache {
    network: String,
    height: u32,
    tip_hash: String,
    at: Option<Instant>,
}

static FRESH_TIP_TRUST_CACHE: OnceLock<Mutex<FreshTipTrustCache>> = OnceLock::new();

fn fresh_tip_cache() -> &'static Mutex<FreshTipTrustCache> {
    FRESH_TIP_TRUST_CACHE.get_or_init(|| Mutex::new(FreshTipTrustCache::default()))
}

fn mark_fresh_tip_trusted(settings: &Settings, chain: &ChainState) {
    if !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return;
    }
    if let Ok(mut cache) = fresh_tip_cache().lock() {
        cache.network = settings.network.name.clone();
        cache.height = chain.height();
        cache.tip_hash = chain.tip_hash().to_string();
        cache.at = Some(Instant::now());
    }
}

fn hf129_mark_canonical_tip_proven(report: &mut P2PSyncReport) {
    // Legacy call sites also accept lower ancestors and unresolved PoW ties.
    // Those remain mineable under the existing rules, but are not exact-tip
    // acknowledgement. Only the evidence-bearing helper can create a proof.
    report.canonical_tip_proven = report.canonical_proof.is_some();
}

fn hf130_mark_exact_tip_proven(
    report: &mut P2PSyncReport,
    local: &ChainState,
    source: Hf130CanonicalEvidence,
) {
    report.canonical_proof = Some(Hf130CanonicalProof {
        height: local.height(),
        tip_hash: local.tip_hash().to_string(),
        observed_at: Instant::now(),
        source,
    });
    report.canonical_tip_proven = true;
}

fn hf130_proof_matches_final(
    report: &P2PSyncReport,
    height: u32,
    tip_hash: &str,
    pending_relay: bool,
) -> bool {
    !pending_relay && report.canonical_proof.as_ref().map_or(false, |proof| {
        proof.height == height
            && proof.tip_hash == tip_hash
            && proof.observed_at.elapsed() <= Duration::from_secs(FRESH_TIP_TRUST_WINDOW_SECS)
    })
}

/// Keep an advertised height and its hash as one observation. In particular,
/// a higher height with no hash must clear a lower height's hash rather than
/// manufacture a mismatched `(height, hash)` pair.
fn hf129_observe_tip(report: &mut P2PSyncReport, height: u32, tip_hash: &str) {
    if report.canonical_proof.as_ref().map_or(false, |proof| {
        height > proof.height || (height == proof.height && !tip_hash.trim().is_empty() && tip_hash != proof.tip_hash)
    }) {
        report.canonical_proof = None;
        report.canonical_tip_proven = false;
    }
    if height == 0 {
        return;
    }
    if height > report.best_peer_height {
        report.best_peer_height = height;
        report.best_peer_tip_hash = tip_hash.to_string();
    } else if height == report.best_peer_height && !tip_hash.trim().is_empty() {
        report.best_peer_tip_hash = tip_hash.to_string();
    }
}

fn fresh_tip_is_trusted(settings: &Settings, chain: &ChainState) -> bool {
    let Ok(cache) = fresh_tip_cache().lock() else {
        return false;
    };
    let Some(at) = cache.at else {
        return false;
    };
    cache.network == settings.network.name
        && cache.height == chain.height()
        && cache.tip_hash == chain.tip_hash().to_string()
        && at.elapsed() <= Duration::from_secs(FRESH_TIP_TRUST_WINDOW_SECS)
}

#[derive(Debug, Default)]
struct Hf97UncatchableTipCache {
    network: String,
    local_height: u32,
    local_tip_hash: String,
    advertised_height: u32,
    at: Option<Instant>,
}

static HF98_UNCATCHABLE_TIP_CACHE: OnceLock<Mutex<Hf97UncatchableTipCache>> = OnceLock::new();

fn hf97_uncatchable_tip_cache() -> &'static Mutex<Hf97UncatchableTipCache> {
    HF98_UNCATCHABLE_TIP_CACHE.get_or_init(|| Mutex::new(Hf97UncatchableTipCache::default()))
}

fn mark_hf97_uncatchable_tip(settings: &Settings, local: &ChainState, advertised_height: u32) {
    if settings.network.name == "mainnet" {
        return;
    }
    if !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return;
    }
    if advertised_height <= local.height() {
        return;
    }
    if let Ok(mut cache) = hf97_uncatchable_tip_cache().lock() {
        cache.network = settings.network.name.clone();
        cache.local_height = local.height();
        cache.local_tip_hash = local.tip_hash().to_string();
        cache.advertised_height = advertised_height;
        cache.at = Some(Instant::now());
    }
}

fn hf97_uncatchable_tip_quarantined(
    settings: &Settings,
    local: &ChainState,
    advertised_height: u32,
) -> bool {
    if settings.network.name == "mainnet" {
        return false;
    }
    if advertised_height <= local.height() {
        return false;
    }
    let Ok(cache) = hf97_uncatchable_tip_cache().lock() else {
        return false;
    };
    let Some(at) = cache.at else {
        return false;
    };
    cache.network == settings.network.name
        && cache.local_height == local.height()
        && cache.local_tip_hash == local.tip_hash().to_string()
        && cache.advertised_height >= advertised_height
        && at.elapsed() <= Duration::from_secs(HF98_UNCATCHABLE_TIP_QUARANTINE_SECS)
}

fn hf97_greenlight_local_tip_after_uncatchable_height(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    advertised_height: u32,
    context: &str,
) -> Result<bool> {
    if settings.network.name == "mainnet" {
        return Ok(false);
    }
    if !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return Ok(false);
    }
    if advertised_height <= local.height() {
        return Ok(false);
    }
    if settings.network.name == "mainnet" && local.height() < MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT
    {
        return Ok(false);
    }
    let gap = advertised_height.saturating_sub(local.height());
    if gap > HF98_UNCATCHABLE_TIP_MAX_GAP {
        return Ok(false);
    }

    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    let local_tip = local.tip_hash().to_string();
    let (contacted, best_direct_height, conflicts) = direct_parent_view(
        settings,
        local.height(),
        &local_tip,
        settings.p2p.max_outbound_peers.max(6).min(12),
        260,
    )?;
    report.peers_contacted = report.peers_contacted.max(contacted);
    if !conflicts.is_empty() {
        return Ok(false);
    }

    mark_hf97_uncatchable_tip(settings, local, advertised_height);
    mark_fresh_tip_trusted(settings, local);
    report.best_peer_height = local.height();
    report.best_peer_tip_hash.clear();
    report.height = local.height();
    report.tip_hash = local_tip;
    eprintln!(
        "HF98 liveness recovery: green-light local validated #{} while reported #{} remains uncatchable (gap {}, best_direct {}, contacted {}, context {}).",
        local.height(), advertised_height, gap, best_direct_height, contacted, context
    );
    Ok(true)
}

fn hf97_suppress_quarantined_best_height(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
) {
    if settings.network.name == "mainnet" {
        return;
    }
    if report.best_peer_height > local.height()
        && hf97_uncatchable_tip_quarantined(settings, local, report.best_peer_height)
    {
        report.best_peer_height = local.height();
        report.best_peer_tip_hash.clear();
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();
    }
}

#[derive(Debug, Default, Clone)]
struct PeerSession {
    height: u32,
    tip_hash: String,
    user_agent: String,
    node_id: String,
    listen_addr: String,
    role: String,
    miner_address: String,
    sync_peer_height: u32,
    sync_peer_tip: String,
    sync_local_height: u32,
    sync_local_tip: String,
    sync_from_height: u32,
    sync_requested_at: Option<Instant>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct PeerRegistry {
    peers: Vec<P2PObservedPeer>,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct RuntimeIdentity {
    miner_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LanDiscoveryBeacon {
    marker: String,
    protocol: u32,
    network: String,
    magic: String,
    genesis_hash: String,
    listen_addr: String,
    node_id: String,
    miner_address: String,
    height: u32,
    tip_hash: String,
    user_agent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexedHeader {
    height: u32,
    hash: String,
    header: BlockHeader,
}

pub fn release_bootnodes(settings: &Settings) -> Vec<String> {
    let mut out = settings
        .p2p
        .bootnodes
        .iter()
        .map(|b| normalize_peer_addr(b))
        .filter(|b| !b.trim().is_empty())
        .collect::<Vec<_>>();
    for seed in default_bootnodes_for_network(&settings.network.name) {
        let seed = normalize_peer_addr(&seed);
        if !seed.is_empty() && !out.iter().any(|existing| existing == &seed) {
            out.push(seed);
        }
    }
    out
}

pub fn default_bootnodes_for_network(network: &str) -> Vec<String> {
    match network {
        "mainnet" => vec![
            "seed.qubit-coin.io:17444".to_string(),
            "seed-ams3.qubit-coin.io:17444".to_string(),
            "seed-nyc3.qubit-coin.io:17444".to_string(),
        ],
        "testnet" => vec![
            "seed.qubit-coin.io:18444".to_string(),
            "seed-ams3.qubit-coin.io:18444".to_string(),
        ],
        _ => Vec::new(),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WireMessage {
    Version {
        protocol: u32,
        network: String,
        magic: String,
        user_agent: String,
        height: u32,
        tip_hash: String,
        work: String,
        genesis_hash: String,
        listen_addr: String,
        node_id: String,
        role: String,
        miner_address: String,
    },
    Inv {
        height: u32,
        tip_hash: String,
        work: String,
    },
    GetHeaders {
        from_height: u32,
    },
    Headers {
        headers: Vec<IndexedHeader>,
    },
    GetChain {
        from_height: u32,
    },
    Chain {
        start_height: u32,
        blocks: Vec<Block>,
    },
    Block {
        block: Block,
    },
    SubmitBlock {
        request_id: u64,
        block: Block,
    },
    BlockAck {
        request_id: u64,
        block_hash: String,
        status: String,
        height: u32,
        tip_hash: String,
        reason: String,
    },
    Tx {
        tx: Transaction,
    },
    SubmitTx {
        request_id: u64,
        tx: Transaction,
    },
    TxAck {
        request_id: u64,
        txid: String,
        status: String,
        height: u32,
        tip_hash: String,
        reason: String,
    },
    GetMempool,
    Mempool {
        txs: Vec<Transaction>,
    },
    GetAddr,
    Addr {
        addrs: Vec<String>,
    },
    GetPeerList,
    PeerList {
        peers: Vec<P2PObservedPeer>,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
    Reject {
        reason: String,
    },
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct PeerBook {
    peers: Vec<String>,
}

#[derive(Debug)]
struct Hf127OutboundDialEntry {
    in_flight: bool,
    failures: u32,
    next_attempt: Instant,
}

fn hf127_outbound_backoff(failures: u32) -> Duration {
    let exponent = failures.min(5);
    Duration::from_secs(2u64.saturating_pow(exponent).min(60))
}

/// Start a best-effort detached propagation task without allowing an operating-
/// system thread-allocation failure to panic the active peer/session worker.
/// Durable acceptance/ACK semantics never depend on this helper succeeding;
/// persistent peer heartbeats and retry workers remain the fallback.
fn hf127_spawn_detached<F>(name: &str, task: F)
where
    F: FnOnce() + Send + 'static,
{
    if let Err(err) = thread::Builder::new().name(name.to_string()).spawn(task) {
        eprintln!("{name} worker allocation failed: {err}");
    }
}

fn start_hf127_pending_tx_delivery_worker(
    settings: Settings,
    chain: Arc<Mutex<ChainState>>,
    mempool_dirty: Arc<AtomicBool>,
) -> Result<()> {
    thread::Builder::new()
        .name("qub-tx-delivery".to_string())
        .spawn(move || {
            let mut last_reconcile = Instant::now() - Duration::from_secs(30);
            let mut last_outbox_error_log = Instant::now() - Duration::from_secs(60);
            loop {
                // Reconcile on a copy so wallet-file I/O and contextual
                // transaction checks never hold the canonical chain mutex. If
                // transactions are reaccepted, save_chain performs the same-tip
                // mempool merge/publish after the lock has been released.
                if last_reconcile.elapsed() >= Duration::from_secs(15) {
                    last_reconcile = Instant::now();
                    let snapshot = chain.lock().ok().map(|local| local.clone());
                    if let Some(mut local) = snapshot {
                        match reconcile_pending_txs(&settings, &mut local) {
                            Ok(report) if report.reaccepted > 0 => {
                                match save_chain(&settings, &local) {
                                    Ok(()) => mempool_dirty.store(true, Ordering::Release),
                                    Err(err) => eprintln!(
                                        "HF127 pending transaction reconciliation persistence failed: {err:#}"
                                    ),
                                }
                            }
                            Ok(_) => {}
                            Err(err) => {
                                if last_outbox_error_log.elapsed() >= Duration::from_secs(30) {
                                    last_outbox_error_log = Instant::now();
                                    eprintln!(
                                        "HF127 durable transaction outbox reconciliation failed closed: {err:#}"
                                    );
                                }
                            }
                        }
                    }
                }

                let now = hf125_now_secs();
                let records = match pending_tx_delivery_records(&settings) {
                    Ok(records) => records,
                    Err(err) => {
                        if last_outbox_error_log.elapsed() >= Duration::from_secs(30) {
                            last_outbox_error_log = Instant::now();
                            eprintln!(
                                "HF127 durable transaction outbox read failed closed: {err:#}"
                            );
                        }
                        thread::sleep(Duration::from_secs(2));
                        continue;
                    }
                };
                for record in records
                    .into_iter()
                    .filter(|record| {
                        !is_pool_share_transaction(&record.tx)
                            && record.next_delivery_retry_unix <= now
                    })
                    .take(HF127_TX_RETRY_MAX_PER_PASS)
                {
                    let report = broadcast_tx_confirmed_internal(
                        &settings,
                        &record.tx,
                        None,
                        HF127_TX_RELAY_MAX_PEERS,
                        850,
                        true,
                    );
                    match report {
                        Ok(report) => {
                            if report.delivered() {
                                eprintln!(
                                    "HF127 transaction delivery acknowledged {}: {}",
                                    record.txid,
                                    report.summary()
                                );
                            } else if report.pending_retry {
                                eprintln!(
                                    "HF127 transaction delivery pending {}: {}",
                                    record.txid,
                                    report.summary()
                                );
                            }
                        }
                        Err(err) => {
                            let fallback = TxRelayReport {
                                txid: record.txid.to_string(),
                                attempts: 1,
                                pending_retry: true,
                                ..TxRelayReport::default()
                            };
                            let retry_at = hf125_now_secs().saturating_add(
                                hf127_tx_retry_delay_secs(record.delivery_attempts, &fallback),
                            );
                            if let Err(update_err) = update_pending_tx_delivery(
                                &settings,
                                record.txid,
                                1,
                                0,
                                0,
                                "delivery_error",
                                0,
                                "",
                                &err.to_string(),
                                false,
                                retry_at,
                            ) {
                                eprintln!(
                                    "HF127 transaction delivery error {}: {err:#}; additionally failed to persist retry state: {update_err:#}",
                                    record.txid
                                );
                            } else {
                                eprintln!(
                                    "HF127 transaction delivery error {}: {err:#}; retry scheduled",
                                    record.txid
                                );
                            }
                        }
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                thread::sleep(Duration::from_secs(2));
            }
        })
        .context("failed to start the durable transaction-delivery worker")?;
    Ok(())
}

pub fn run_node(settings: Settings) -> Result<()> {
    if !settings.p2p.enabled {
        bail!("p2p.enabled=false in config; set it true for node mode");
    }

    let mut initial_chain = load_or_init_chain(&settings)?;
    if !initial_chain.mempool.is_empty() {
        // HF124/v1.8.1 startup policy migration. Older versions could persist
        // stale, malformed or over-policy pool-share queues. Rebuild once with
        // the bounded batch-aware admission path before exposing the canonical
        // owner, then persist only when the deterministic queue changed.
        let before = initial_chain
            .mempool
            .iter()
            .map(Transaction::txid)
            .collect::<Vec<_>>();
        let retained = initial_chain.revalidate_mempool(&settings);
        let after = initial_chain
            .mempool
            .iter()
            .map(Transaction::txid)
            .collect::<Vec<_>>();
        if before != after {
            eprintln!(
                "HF124 startup mempool policy retained {retained}/{} transaction(s)",
                before.len()
            );
            save_chain(&settings, &initial_chain)?;
        }
    }
    let chain = Arc::new(Mutex::new(initial_chain));
    let mempool_dirty = Arc::new(AtomicBool::new(false));
    register_live_chain(&settings, &chain);
    if settings.rpc.enabled {
        crate::rpc::start_embedded(settings.clone(), chain.clone())?;
    }
    let peers = Arc::new(Mutex::new(load_peer_set(&settings)?));
    {
        let mut p = peers.lock().expect("peer mutex poisoned");
        for bootnode in release_bootnodes(&settings) {
            if !is_self_or_empty_addr(&settings, &bootnode) {
                p.insert(normalize_peer_addr(&bootnode));
            }
        }
        // Public networks use baked-in DNS seed domains plus any config-provided nodes.
        // Regtest-LAN uses UDP discovery, so users do not need to type LAN IPs.
    }
    save_peer_set(&settings, &peers.lock().expect("peer mutex poisoned"))?;

    // HF88/v1.6.2: inbound listening is useful but must never be a single point
    // of failure for outbound sync. On Windows/laptops the port may be occupied
    // by another instance or blocked by security software; QUB Core should still
    // connect out, learn peers, relay mempool, and catch up.
    let listener = match TcpListener::bind(&settings.p2p.bind) {
        Ok(listener) => {
            listener.set_nonblocking(true)?;
            Some(listener)
        }
        Err(err) => {
            eprintln!(
                "p2p inbound listener disabled on {}: {err}. Continuing outbound-only.",
                settings.p2p.bind
            );
            None
        }
    };
    let inbound_count = Arc::new(AtomicUsize::new(0));
    let outbound_count = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(Mutex::new(HashSet::<String>::new()));

    if lan_discovery_enabled(&settings) {
        start_lan_discovery(settings.clone(), peers.clone(), chain.clone());
    }

    if let Some(listener) = listener {
        let accept_settings = settings.clone();
        let accept_chain = chain.clone();
        let accept_peers = peers.clone();
        let accept_active = active.clone();
        let accept_inbound = inbound_count.clone();
        let accept_mempool_dirty = mempool_dirty.clone();
        thread::Builder::new()
            .name("qub-p2p-accept".to_string())
            .spawn(move || loop {
            match listener.accept() {
                Ok((stream, addr)) => {
                    // HF125/v1.8.2 reserves a small short-lived inbound lane
                    // for block submission/acknowledgement connections. HF124
                    // silently dropped every new socket once the normal peer cap
                    // was full, so a valid found block could never reach a seed.
                    let normal_limit = accept_settings.p2p.max_inbound_peers;
                    let inbound_limit = normal_limit.saturating_add(HF125_PRIORITY_INBOUND_RESERVE);
                    let current_inbound = accept_inbound.load(Ordering::Relaxed);
                    if current_inbound >= inbound_limit {
                        continue;
                    }
                    let priority_only = current_inbound >= normal_limit;
                    accept_inbound.fetch_add(1, Ordering::Relaxed);
                    let s = accept_settings.clone();
                    let c = accept_chain.clone();
                    let p = accept_peers.clone();
                    let a = accept_active.clone();
                    let inbound = accept_inbound.clone();
                    let dirty = accept_mempool_dirty.clone();
                    let peer = addr.to_string();
                    let thread_name = format!("qub-p2p-in-{}", addr.ip());
                    if let Err(err) = thread::Builder::new().name(thread_name).spawn(move || {
                        if let Err(err) = handle_peer(
                            stream,
                            peer.clone(),
                            false,
                            priority_only,
                            s,
                            c,
                            p,
                            a,
                            dirty,
                        ) {
                            if !is_benign_disconnect(&err) {
                                eprintln!("p2p inbound {peer} ended: {err:#}");
                            }
                        }
                        inbound.fetch_sub(1, Ordering::Relaxed);
                    }) {
                        accept_inbound.fetch_sub(1, Ordering::Relaxed);
                        eprintln!("p2p inbound worker allocation failed for {addr}: {err}");
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100))
                }
                Err(err) => {
                    eprintln!("p2p accept error: {err}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        })
        .context("failed to start the P2P inbound accept worker")?;
        println!("QUB P2P node listening on {}", settings.p2p.bind);
    } else {
        println!(
            "QUB P2P node running outbound-only; inbound bind {} unavailable",
            settings.p2p.bind
        );
    }
    println!(
        "Network={} advertise_addr={} automatic_discovery={} public_seed_domains={}",
        settings.network.name,
        effective_advertise_addr(&settings),
        if lan_discovery_enabled(&settings) {
            "lan"
        } else {
            "dns"
        },
        release_bootnodes(&settings).len()
    );

    let mut last_mempool_relay = Instant::now() - Duration::from_secs(30);
    let mut last_mempool_persist = Instant::now();
    let mut last_light_catchup = Instant::now() - Duration::from_secs(60);
    let mut last_block_relay_retry = Instant::now() - Duration::from_secs(30);
    let dial_state = Arc::new(Mutex::new(HashMap::<String, Hf127OutboundDialEntry>::new()));
    let mut known_peer_cache = known_peers(&settings).unwrap_or_default();
    let mut last_known_peer_refresh = Instant::now();
    let mut last_status_log = Instant::now() - Duration::from_secs(HF127_STATUS_LOG_SECS);
    start_hf127_pending_tx_delivery_worker(
        settings.clone(),
        chain.clone(),
        mempool_dirty.clone(),
    )?;

    loop {
        // HF127/v1.8.3: peer dialing is detached and bounded. Failed/stale
        // peerbook rows can no longer block mempool merge, transaction retry,
        // block retry or tip heartbeat for N x connect-timeout seconds.
        if last_known_peer_refresh.elapsed() >= Duration::from_secs(HF127_KNOWN_PEER_REFRESH_SECS)
            || known_peer_cache.is_empty()
        {
            last_known_peer_refresh = Instant::now();
            known_peer_cache = known_peers(&settings).unwrap_or_else(|_| {
                peers
                    .lock()
                    .map(|p| p.iter().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            });
        }
        let known = known_peer_cache.clone();
        // Inbound sessions must not consume the outbound budget. A busy seed
        // can have hundreds of inbound peers while still requiring persistent
        // outbound connections to the other official seeds.
        let current_outbound = outbound_count.load(Ordering::Acquire);
        let available = settings
            .p2p
            .max_outbound_peers
            .saturating_sub(current_outbound)
            .min(HF127_OUTBOUND_DIALS_PER_TICK);
        let mut scheduled = 0usize;
        for addr in known {
            if scheduled >= available {
                break;
            }
            if should_skip_outbound(&settings, &addr) {
                continue;
            }
            let already_active = active
                .lock()
                .map(|active| active.contains(&addr))
                .unwrap_or(false);
            if already_active {
                continue;
            }
            let due = {
                let mut state = dial_state.lock().expect("dial state mutex poisoned");
                let entry = state.entry(addr.clone()).or_insert_with(|| Hf127OutboundDialEntry {
                    in_flight: false,
                    failures: 0,
                    next_attempt: Instant::now(),
                });
                if entry.in_flight || Instant::now() < entry.next_attempt {
                    false
                } else {
                    entry.in_flight = true;
                    true
                }
            };
            if !due {
                continue;
            }
            scheduled = scheduled.saturating_add(1);
            let s = settings.clone();
            let c = chain.clone();
            let p = peers.clone();
            let a = active.clone();
            let dirty = mempool_dirty.clone();
            let dial = dial_state.clone();
            let outbound = outbound_count.clone();
            outbound.fetch_add(1, Ordering::AcqRel);
            let dial_addr = addr.clone();
            let dial_for_failure = dial_state.clone();
            let outbound_for_failure = outbound_count.clone();
            let spawn_result = thread::Builder::new()
                .name("qub-p2p-outbound".to_string())
                .spawn(move || {
                let started = Instant::now();
                let mut connected = false;
                match connect_peer(&addr, Duration::from_millis(900)) {
                    Ok(stream) => {
                        connected = true;
                        if let Err(err) = handle_peer(
                            stream,
                            addr.clone(),
                            true,
                            false,
                            s,
                            c,
                            p,
                            a,
                            dirty,
                        ) {
                            if !is_benign_disconnect(&err) {
                                eprintln!("p2p outbound {addr} ended: {err:#}");
                            }
                        }
                    }
                    Err(_) => {}
                }
                if let Ok(mut state) = dial.lock() {
                    if let Some(entry) = state.get_mut(&addr) {
                        entry.in_flight = false;
                        if connected && started.elapsed() >= Duration::from_secs(5) {
                            entry.failures = 0;
                            entry.next_attempt = Instant::now() + Duration::from_secs(2);
                        } else {
                            entry.failures = entry.failures.saturating_add(1);
                            entry.next_attempt =
                                Instant::now() + hf127_outbound_backoff(entry.failures);
                        }
                    }
                }
                outbound.fetch_sub(1, Ordering::AcqRel);
            });
            if let Err(err) = spawn_result {
                outbound_for_failure.fetch_sub(1, Ordering::AcqRel);
                if let Ok(mut state) = dial_for_failure.lock() {
                    if let Some(entry) = state.get_mut(&dial_addr) {
                        entry.in_flight = false;
                        entry.failures = entry.failures.saturating_add(1);
                        entry.next_attempt =
                            Instant::now() + hf127_outbound_backoff(entry.failures);
                    }
                }
                eprintln!("p2p outbound worker allocation failed for {dial_addr}: {err}");
            }
        }

        // HF123/v1.8.0 Fast Chain Engine: chain.json is no longer a thread
        // communication bus. The canonical Arc is authoritative. A lightweight
        // pointer comparison detects state committed by another process and only
        // then loads one immutable Fast Chain Engine generation.
        let committed_identity = fast_storage_identity(&settings).ok().flatten();
        let memory_identity = live_chain_identity(&settings);
        let committed_changed = match (committed_identity.as_ref(), memory_identity.as_ref()) {
            (Some(committed), Some(memory)) => {
                committed.committed_height != memory.committed_height
                    || committed.tip_hash != memory.tip_hash
                    || committed.mempool_digest != memory.mempool_digest
            }
            (Some(_), None) => true,
            _ => false,
        };

        let mut relay_after_merge = Vec::<Transaction>::new();
        if committed_changed {
            match load_committed_chain_with_identity(&settings, false) {
                Ok((disk_chain, captured_identity)) => {
                    let disk_mempool_txids = disk_chain.mempool_txids();
                    let mut local = chain.lock().expect("chain mutex poisoned");
                    // The generation may have changed while waiting for the
                    // live owner. Never publish a stale captured disk view.
                    let current_identity = fast_storage_identity(&settings).ok().flatten();
                    if hf132_committed_capture_is_current(captured_identity.as_ref(), current_identity.as_ref()) {
                        match hf132_reconcile_committed_view(&mut local, disk_chain, &settings) {
                            Ok(relayed) => {
                                relay_after_merge.extend(relayed);
                                // Retain the union of concurrent mempool updates.
                                if local.mempool_txids() != disk_mempool_txids {
                                    mempool_dirty.store(true, Ordering::Release);
                                }
                            }
                            Err(err) => eprintln!("HF132 committed-state reconcile skipped: {err:#}"),
                        }
                    }
                }
                Err(err) => {
                    eprintln!("HF123 committed-state reconcile skipped: {err:#}");
                }
            }
        }

        let mempool_for_periodic = if last_mempool_relay.elapsed() >= Duration::from_secs(6) {
            last_mempool_relay = Instant::now();
            let local = chain.lock().expect("chain mutex poisoned");
            hf124_fair_mempool_relay_batch(&settings, &local.mempool, 96)
        } else {
            Vec::new()
        };

        // HF124/v1.8.1: coalesce mempool-only Fast Chain Engine commits. Clone
        // the copy-on-write canonical state under the mutex, then perform the
        // state-file commit after releasing it so inbound blocks and mining are
        // not paused by disk I/O. save_chain()/publish_live_chain() protects
        // same-tip mempool unions; a post-save identity check schedules another
        // commit if the canonical owner moved while this snapshot was written.
        if last_mempool_persist.elapsed() >= Duration::from_secs(5)
            && mempool_dirty.swap(false, Ordering::AcqRel)
        {
            last_mempool_persist = Instant::now();
            let snapshot = match chain.try_lock() {
                Ok(local) => Some(local.clone()),
                Err(_) => {
                    mempool_dirty.store(true, Ordering::Release);
                    None
                }
            };
            if let Some(snapshot) = snapshot {
                if let Err(err) = save_chain(&settings, &snapshot) {
                    mempool_dirty.store(true, Ordering::Release);
                    eprintln!("HF124 coalesced mempool persistence failed: {err:#}");
                } else {
                    match chain.try_lock() {
                        Ok(current) => {
                            if current.tip_hash() != snapshot.tip_hash()
                                || current.mempool_txids() != snapshot.mempool_txids()
                            {
                                mempool_dirty.store(true, Ordering::Release);
                            }
                        }
                        Err(_) => mempool_dirty.store(true, Ordering::Release),
                    }
                }
            }
        }

        // HF124/v1.8.1: relay the heartbeat as one bounded Mempool message per
        // peer, not one fresh TCP connection per transaction. A share backlog
        // can contain hundreds of entries; per-tx fanout would otherwise create
        // another self-amplifying connection storm even after validation and
        // persistence were fixed.
        let mut relay_seen = HashSet::<Hash256>::new();
        let relay_candidates = relay_after_merge
            .into_iter()
            .chain(mempool_for_periodic.into_iter())
            .filter(|tx| relay_seen.insert(tx.txid()))
            .collect::<Vec<_>>();
        let relay_batch = hf124_fair_mempool_relay_batch(&settings, &relay_candidates, 96);
        if !relay_batch.is_empty() {
            let _ = relay_mempool_batch_to_known_peers(
                &settings,
                &relay_batch,
                None,
                settings.p2p.max_outbound_peers.max(4).min(8),
            );
        }
        // HF125/v1.8.2: a locally accepted block remains durably queued until
        // at least one updated peer explicitly acknowledges it. Retry delivery
        // independently of mempool traffic and GUI state.
        if last_block_relay_retry.elapsed() >= Duration::from_secs(3) {
            last_block_relay_retry = Instant::now();
            if !HF125_BLOCK_RETRY_RUNNING.swap(true, Ordering::AcqRel) {
                let relay_settings = settings.clone();
                let relay_chain = chain.clone();
                if let Err(err) = thread::Builder::new()
                    .name("qub-block-retry".to_string())
                    .spawn(move || {
                        let _reset = Hf125BlockRetryGuard;
                        if let Ok(Some(report)) =
                            retry_pending_block_relay(&relay_settings, &relay_chain)
                        {
                            if report.delivered() {
                                eprintln!(
                                    "HF125 pending block delivery acknowledged: {}",
                                    report.summary()
                                );
                            } else if report.pending_retry {
                                eprintln!(
                                    "HF125 pending block delivery retry: {}",
                                    report.summary()
                                );
                            }
                        }
                    })
                {
                    HF125_BLOCK_RETRY_RUNNING.store(false, Ordering::Release);
                    eprintln!("pending block retry worker allocation failed: {err}");
                }
            }
        }

        // HF125/v1.8.2: the heavy repair ladder no longer runs synchronously in
        // the outbound/relay coordinator. One detached worker at a time keeps
        // peer spawning, mempool relay and pending-block delivery responsive.
        if last_light_catchup.elapsed() >= Duration::from_secs(18) {
            last_light_catchup = Instant::now();
            if !HF125_AUTO_CATCHUP_RUNNING.swap(true, Ordering::AcqRel) {
                let catchup_settings = settings.clone();
                if let Err(err) = thread::Builder::new()
                    .name("qub-auto-catchup".to_string())
                    .spawn(move || {
                        let _reset = Hf125AutoCatchupGuard;
                        let _ = hf90_auto_catchup(&catchup_settings, 120_000);
                    })
                {
                    HF125_AUTO_CATCHUP_RUNNING.store(false, Ordering::Release);
                    eprintln!("auto-catchup worker allocation failed: {err}");
                }
            }
        }

        if last_status_log.elapsed() >= Duration::from_secs(HF127_STATUS_LOG_SECS) {
            last_status_log = Instant::now();
            // Snapshot each mutex independently. Diagnostic logging must never
            // introduce a chain -> peer -> active lock ordering that can oppose
            // connection workers and stall the coordinator.
            let (height, tip, mempool_len) = {
                let local = chain.lock().expect("chain mutex poisoned");
                (local.height(), local.tip_hash(), local.mempool.len())
            };
            let peer_count = peers.lock().expect("peer mutex poisoned").len();
            let active_count = active.lock().expect("active peer mutex poisoned").len();
            println!(
                "p2p status height={} tip={} mempool={} peers={} active={}",
                height, tip, mempool_len, peer_count, active_count
            );
        }
        let sleep_secs = settings.p2p.connect_interval_secs.max(1).min(3);
        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

fn hf132_committed_capture_is_current(
    captured: Option<&FastStorageIdentity>, current: Option<&FastStorageIdentity>,
) -> bool {
    matches!((captured, current), (Some(a), Some(b)) if a == b)
}

fn hf132_reconcile_committed_view(
    local: &mut ChainState, disk: ChainState, settings: &Settings,
) -> Result<Vec<Transaction>> {
    // Committed equal-work reanchors remain authoritative at the exact CURRENT
    // identity. Height alone must never replace a stronger live branch.
    let different_tip = disk.tip_hash() != local.tip_hash();
    let can_adopt = if different_tip {
        let disk_work = chain_work_for_blocks(&disk.blocks)?;
        let local_work = chain_work_for_blocks(&local.blocks)?;
        disk_work > local_work || (disk_work == local_work && disk.height() >= local.height())
    } else { false };
    let mut relay = merge_mempool_from_chain(local, &disk, settings);
    if can_adopt {
        let mut adopted = disk;
        let keep_mempool = local.reorg_mempool_candidates_for(&adopted);
        adopted.rebuild_mempool_from(keep_mempool, settings);
        relay.extend(adopted.mempool.iter().take(64).cloned());
        *local = adopted;
    }
    Ok(relay)
}

pub fn sync_chain_once(settings: &Settings) -> Result<P2PSyncReport> {
    let _scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(120));
    // HF74/v1.5.8 fixed2: keep CLI callers on the tiered official sync path.
    // If the local chain is on a valid-but-noncanonical branch at the same/near
    // height, tail/suffix sync cannot append. In that case, allow canonical
    // snapshot repair instead of falling into long peer loops.
    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "sync-chain-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, workflow_report),
    };
    let mut report = hf90_manual_catchup(settings, 120_000).unwrap_or(workflow_report);
    let before_tail = load_chain_for_hf90_catchup(settings).ok();
    let before_h = before_tail.as_ref().map(|c| c.height()).unwrap_or(0);
    let before_tip = before_tail
        .as_ref()
        .map(|c| c.tip_hash().to_string())
        .unwrap_or_default();
    let official = official_http_tip(settings, 3_000).ok().flatten();
    let official_h = official
        .as_ref()
        .map(|(h, _)| *h)
        .unwrap_or(report.best_peer_height);
    let official_hash = official
        .as_ref()
        .map(|(_, h)| h.clone())
        .unwrap_or_default();

    let mut tail_failed = false;
    if official_h >= before_h {
        match sync_official_http_tail(settings, 20_000) {
            Ok(tail) => merge_sync_reports(&mut report, tail),
            Err(_) => tail_failed = true,
        }
    }

    let after_tail = load_chain_for_hf90_catchup(settings).ok();
    let after_h = after_tail.as_ref().map(|c| c.height()).unwrap_or(before_h);
    let after_tip = after_tail
        .as_ref()
        .map(|c| c.tip_hash().to_string())
        .unwrap_or(before_tip.clone());
    let same_height_wrong_tip =
        official_h == after_h && !official_hash.trim().is_empty() && official_hash != after_tip;
    let still_behind = official_h > after_h;
    let no_tail_progress = after_h == before_h && after_tip == before_tip;

    if same_height_wrong_tip || (still_behind && (tail_failed || no_tail_progress)) {
        if let Ok(full) = sync_official_http_snapshot(settings, 90_000) {
            merge_sync_reports(&mut report, full);
            return finish_report(settings, report);
        }
    }

    if report.height == 0 && report.tip_hash.trim().is_empty() {
        if let Ok(local) = load_chain_for_hf90_catchup(settings) {
            report.height = local.height();
            report.tip_hash = local.tip_hash().to_string();
        }
    }
    finish_report(settings, report)
}

pub fn sync_once(settings: &Settings) -> Result<P2PSyncReport> {
    let _scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(15));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        15_000,
        false,
        "legacy-sync-once",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };
    let peers = known_peers(settings)?;
    for addr in peers
        .into_iter()
        .take(settings.p2p.max_outbound_peers.max(1))
    {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let Ok(mut stream) = connect_peer(&addr, Duration::from_secs(3)) else {
            continue;
        };
        report.peers_contacted += 1;
        stream.set_read_timeout(Some(Duration::from_secs(8)))?;
        stream.set_write_timeout(Some(Duration::from_secs(8)))?;
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut client_session = PeerSession::default();
        let local = load_chain_for_hf90_catchup(settings)?;
        let from_height = local.height().saturating_add(1);
        send_version(&mut stream, settings, &local)?;
        send_wire(&mut stream, &WireMessage::GetAddr)?;
        send_wire(&mut stream, &WireMessage::GetPeerList)?;
        // Pull remote mempools during every sync round. This makes wallet sends
        // propagate to miners even if the original sender is not the next block finder.
        send_wire(&mut stream, &WireMessage::GetMempool)?;
        // HF53: ask for only the missing suffix first. If the peer is on a
        // different branch, block processing will request a full chain for
        // deterministic fork-choice. This makes fresh installs and lagging
        // miners catch up much faster than always requesting from genesis.
        send_wire(&mut stream, &WireMessage::GetHeaders { from_height })?;
        send_wire(&mut stream, &WireMessage::GetChain { from_height })?;
        let deadline = Instant::now() + Duration::from_secs(10);
        let deadline = hf130_effective_deadline(deadline);
        let _hf130_io_scope = Hf130IoScope::deadline(deadline);
        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(msg) => process_client_message(settings, &addr, msg, &mut stream, &mut report, &mut client_session)?,
                Err(err) if is_timeout(&err) => break,
                Err(err) => {
                    report.peer_errors = report.peer_errors.saturating_add(1);
                    if !is_benign_io(&err) {
                        break;
                    }
                }
            }
        }
    }
    finish_report(settings, report)
}

pub fn sync_quick(
    settings: &Settings,
    max_peers: usize,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "quick-sync",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(1_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let peer_budget = max_peers.max(1).saturating_mul(2).min(32);
    let mut peers = hf131_direct_candidates(settings, peer_budget.min(12));
    for addr in prioritized_outbound_peers(settings, peer_budget)
        .unwrap_or_else(|_| known_peers(settings).unwrap_or_default()) {
        if !peers.contains(&addr) { peers.push(addr); }
    }
    peers.truncate(peer_budget);
    if let Ok(version) = hf131_probe_version(settings) {
        let probe_deadline = deadline.min(Instant::now() + Duration::from_millis(800));
        let reachable = hf131_probe_batch(&peers, probe_deadline,
            |addr, left| probe_peer_with_version(settings, addr, left, &version).ok())
            .into_iter().map(|(addr, _)| addr).collect::<HashSet<_>>();
        hf131_prefer_reachable(&mut peers, &reachable);
    }
    let candidate_count = peers.len().min(peer_budget);
    for (index, addr) in peers.into_iter().take(peer_budget).enumerate() {
        if Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let peer_deadline = hf132_peer_io_deadline(deadline, candidate_count.saturating_sub(index));
        match sync_peer_adaptive_session(settings, &addr, peer_deadline, &mut report) {
            Ok(()) => {}
            Err(err) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                if !is_benign_io_error(&err) {
                    // Keep trying other peers. A single stale or slow peer must not
                    // make the GUI/miner require a reinstall.
                }
            }
        }
        let local = load_chain_for_hf90_catchup(settings)?;
        // HF82: if we are at the same height as a peer but possibly on a
        // different tip, keep trying other peers instead of declaring success by
        // height alone. This is important for same-height fork repair.
        if report.best_peer_height > 0
            && (local.height() > report.best_peer_height
                || (local.height() == report.best_peer_height
                    && (report.chains_adopted > 0 || report.blocks_connected > 0)))
        {
            break;
        }
    }

    // HF60/v1.5.2 fixed: forced checkpoint sync must be part of the caller's
    // timeout budget, not an extra timeout appended after sync_quick already
    // spent its full deadline. The old code could make `qubd sync` take longer
    // than the UI/CLI watchdog even though every individual phase was bounded.
    let local_after = load_chain_for_hf90_catchup(settings)?;
    if report.best_peer_height > local_after.height() && total_timeout_ms >= 6_000 {
        let remaining_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(FORCE_ANCHOR_SYNC_TIMEOUT_MS as u128) as u64;
        if remaining_ms >= 1_500 {
            let forced =
                sync_force_anchor_to_best_direct(settings, report.best_peer_height, remaining_ms)?;
            merge_sync_reports(&mut report, forced);
        }
    }

    finish_report(settings, report)
}

fn hf131_prefer_reachable(peers: &mut [String], reachable: &HashSet<String>) {
    // Stable ordering keeps discovered direct peers ahead of official aliases
    // within the responsive group; height advertisements are not fork choice.
    peers.sort_by_key(|addr| !reachable.contains(addr));
}

// Give ordinary peers a real opportunity before long official snapshot paths.
// Probing is concurrent, but block validation/adoption stays under the existing
// single canonical writer. A peer advertisement only selects a transport.
fn hf131_sync_direct_before_snapshots(settings: &Settings, report: &mut P2PSyncReport, budget_ms: u64) {
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(budget_ms));
    let _scope = Hf130IoScope::deadline(deadline);
    let peers = hf131_direct_candidates(settings, 12);
    if peers.is_empty() { return; }
    let Ok(version) = hf131_probe_version(settings) else { return; };
    let probe_deadline = deadline.min(Instant::now() + Duration::from_millis(800));
    let mut observations = hf131_unique_observations(hf131_probe_batch(&peers, probe_deadline,
        |addr, left| probe_peer_with_version(settings, addr, left, &version).ok()));
    observations.sort_by(|a, b| b.1.height.cmp(&a.1.height));
    let candidate_count = observations.len().min(4);
    for (index, (addr, _)) in observations.into_iter().take(4).enumerate() {
        if hf130_io_remaining(deadline).is_err() { break; }
        let mut step = P2PSyncReport::default();
        let peer_deadline = hf132_peer_io_deadline(deadline, candidate_count.saturating_sub(index));
        if sync_peer_adaptive_session(settings, &addr, peer_deadline, &mut step).is_err() {
            step.peer_errors = step.peer_errors.saturating_add(1);
        }
        merge_sync_reports(report, step);
    }
}

fn sync_peer_adaptive_session(
    settings: &Settings,
    addr: &str,
    deadline: Instant,
    report: &mut P2PSyncReport,
) -> Result<()> {
    let _hf130_entry_scope = Hf130IoScope::deadline(deadline);
    let connect_left = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(1800));
    if connect_left.is_zero() {
        return Ok(());
    }
    let mut stream = connect_peer(addr, connect_left)?;
    report.peers_contacted = report.peers_contacted.saturating_add(1);
    stream.set_read_timeout(Some(Duration::from_millis(900)))?;
    stream.set_write_timeout(Some(Duration::from_millis(2500)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut client_session = PeerSession::default();

    let local = load_chain_for_hf90_catchup(settings)?;
    let _ = send_version(&mut stream, settings, &local);
    let _ = send_wire(&mut stream, &WireMessage::GetAddr);
    let _ = send_wire(&mut stream, &WireMessage::GetPeerList);
    let _ = send_wire(&mut stream, &WireMessage::GetMempool);

    // HF56/v1.5.2: do not blast many overlapping windows all at once. Send the
    // normal suffix request first, learn the peer tip, then probe exponentially
    // larger fork windows until a common ancestor is found. This avoids the
    // "Sync takes forever / reinstall fixes it" path.
    let from_height = local.height().saturating_add(1);
    let _ = send_wire(&mut stream, &WireMessage::GetHeaders { from_height });
    request_peer_chain_from(&mut stream, &local, 0, "", from_height, &mut client_session)?;

    let mut peer_height = 0u32;
    let mut peer_tip = String::new();
    let mut sent_windows = HashSet::<u32>::new();
    sent_windows.insert(from_height);
    let mut next_window_idx = 0usize;
    let mut last_progress_at = Instant::now();
    let mut last_local_height = local.height();
    let mut last_local_tip = local.tip_hash().to_string();

    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes) {
            Ok(msg) => {
                match &msg {
                    WireMessage::Version {
                        height, tip_hash, ..
                    }
                    | WireMessage::Inv {
                        height, tip_hash, ..
                    } => {
                        peer_height = peer_height.max(*height);
                        if *height >= peer_height {
                            peer_tip = (*tip_hash).clone();
                        }
                        hf129_observe_tip(report, *height, tip_hash);
                    }
                    _ => {}
                }
                process_client_message(settings, addr, msg, &mut stream, report, &mut client_session)?;

                let current = load_chain_for_hf90_catchup(settings)?;
                if current.height() != last_local_height
                    || current.tip_hash().to_string() != last_local_tip
                {
                    last_local_height = current.height();
                    last_local_tip = current.tip_hash().to_string();
                    last_progress_at = Instant::now();
                }
                if peer_height > 0 && current.height() >= peer_height {
                    if peer_tip.trim().is_empty()
                        || current.tip_hash().to_string() == peer_tip
                        || current.height() > peer_height
                    {
                        break;
                    }
                }
            }
            Err(err) if is_timeout(&err) => {
                let current = load_chain_for_hf90_catchup(settings)?;
                let effective_peer_height = peer_height
                    .max(report.best_peer_height)
                    .max(current.height());
                let peer_tip_differs = peer_height == current.height()
                    && !peer_tip.trim().is_empty()
                    && peer_tip != current.tip_hash().to_string();
                let windows = adaptive_from_heights(
                    current.height(),
                    effective_peer_height,
                    peer_tip_differs,
                );
                let mut sent = false;
                while next_window_idx < windows.len() {
                    let from = windows[next_window_idx];
                    next_window_idx += 1;
                    if sent_windows.insert(from) {
                        let _ =
                            send_wire(&mut stream, &WireMessage::GetHeaders { from_height: from });
                        let _ =
                            send_wire(&mut stream, &WireMessage::GetChain { from_height: from });
                        sent = true;
                        break;
                    }
                }
                if !sent {
                    if last_progress_at.elapsed() >= Duration::from_secs(3) {
                        break;
                    }
                }
            }
            Err(err) => {
                if is_benign_io(&err) {
                    break;
                }
                return Err(err.into());
            }
        }
    }
    Ok(())
}

fn is_benign_io_error(err: &anyhow::Error) -> bool {
    let s = format!("{err:#}");
    s.contains("timed out")
        || s.contains("WouldBlock")
        || s.contains("peer closed")
        || s.contains("Resource temporarily unavailable")
        || s.contains("connection reset")
        || s.contains("Connection reset")
}

fn force_anchor_from_height(settings: &Settings, local: &ChainState) -> u32 {
    // Mainnet has a hard checkpoint. Pulling from checkpoint+1 is much faster
    // than genesis and replaces all post-checkpoint local fork blocks.
    if settings.network.name == "mainnet" && local.height() >= MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT
    {
        MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT.saturating_add(1)
    } else {
        0
    }
}

fn official_snapshot_peers(settings: &Settings) -> Vec<String> {
    let mut out = Vec::<String>::new();
    let push = |addr: String, out: &mut Vec<String>| {
        let normalized = normalize_peer_addr(&addr);
        if !normalized.trim().is_empty()
            && !is_self_or_empty_addr(settings, &normalized)
            && !out.iter().any(|p| p == &normalized)
        {
            out.push(normalized);
        }
    };

    // Prefer explicit live regional seeds first. AMS3 is the canonical EU seed;
    // NYC3 is now verified live for US users. The generic seed is included too.
    // No peerbook/registry rows are used here.
    let port = if settings.network.name == "testnet" {
        18444
    } else {
        17444
    };
    if matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        push(format!("seed-ams3.qubit-coin.io:{port}"), &mut out);
        if settings.network.name == "mainnet" {
            push(format!("seed-nyc3.qubit-coin.io:{port}"), &mut out);
        }
        push(format!("seed.qubit-coin.io:{port}"), &mut out);
    }
    for bootnode in release_bootnodes(settings) {
        push(bootnode, &mut out);
    }
    out
}

static HF131_ACTIVE_PROBES: AtomicUsize = AtomicUsize::new(0);

struct Hf131ProbePermit;

impl Hf131ProbePermit {
    fn acquire() -> Option<Self> {
        HF131_ACTIVE_PROBES.fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
            if active < 32 { Some(active + 1) } else { None }
        }).ok().map(|_| Self)
    }
}

impl Drop for Hf131ProbePermit {
    fn drop(&mut self) { HF131_ACTIVE_PROBES.fetch_sub(1, Ordering::AcqRel); }
}

// Probe readers may run concurrently; canonical chain writers never do. Every
// worker inherits the same absolute deadline and cancellation and is joined.
// A silent first seed therefore cannot hide another regional or ordinary peer.
fn hf131_probe_batch<T: Send, F>(peers: &[String], deadline: Instant, probe: F) -> Vec<(String, T)>
where F: Fn(&str, Duration) -> Option<T> + Sync {
    let deadline = hf130_effective_deadline(deadline);
    let inherited = hf130_io_context();
    thread::scope(|scope| {
        let mut workers = Vec::new();
        let probe = &probe;
        for addr in peers.iter().take(16) {
            if hf130_io_remaining(deadline).is_err() { break; }
            let Some(permit) = Hf131ProbePermit::acquire() else { continue; };
            let inherited = inherited.clone();
            if let Ok(worker) = thread::Builder::new().name("qub-tip-probe".into()).spawn_scoped(scope, move || {
                let _permit = permit;
                let _scope = Hf130IoScope::enter(inherited);
                let _deadline = Hf130IoScope::deadline(deadline);
                let left = hf130_io_remaining(deadline).ok()?;
                probe(addr, left).map(|info| (addr.clone(), info))
            }) {
                workers.push(worker);
            }
        }
        workers.into_iter().filter_map(|worker| worker.join().ok().flatten()).collect()
    })
}

fn hf131_unique_observations(observations: Vec<(String, P2PProbeInfo)>) -> Vec<(String, P2PProbeInfo)> {
    let mut nodes = HashSet::new();
    observations.into_iter().filter(|(_, info)| {
        let id = info.node_id.trim();
        !id.is_empty() && nodes.insert(id.to_string())
    }).collect()
}

fn hf131_direct_candidates(settings: &Settings, max_peers: usize) -> Vec<String> {
    // Do not requeue timed-out official aliases in the ordinary-peer lane.
    let official = official_snapshot_peers(settings).into_iter()
        .map(|addr| normalize_peer_addr(&addr)).collect::<HashSet<_>>();
    known_peers(settings).unwrap_or_default().into_iter()
        .filter(|addr| !official.contains(&normalize_peer_addr(addr)) && !should_skip_outbound(settings, addr))
        .take(max_peers.clamp(1, 16)).collect()
}

fn hf131_direct_observations(settings: &Settings, max_peers: usize, deadline: Instant) -> Vec<(String, P2PProbeInfo)> {
    let peers = hf131_direct_candidates(settings, max_peers);
    if peers.is_empty() { return Vec::new(); }
    let Ok(version) = hf131_probe_version(settings) else { return Vec::new(); };
    hf131_unique_observations(hf131_probe_batch(&peers, deadline,
        |addr, left| probe_peer_with_version(settings, addr, left, &version).ok()))
}

#[derive(Default)]
struct Hf131ParentView {
    contacted: usize,
    exact: usize,
    ahead_height: u32,
    raw_ahead_height: u32,
    conflicts: Vec<String>,
    raw_conflicts: Vec<String>,
}

fn hf131_parent_view(parent_height: u32, parent_hash: &str, observations: Vec<(String, P2PProbeInfo)>) -> Hf131ParentView {
    let mut view = Hf131ParentView::default();
    let mut ahead = HashMap::<(u32, String), usize>::new();
    let mut conflicts = HashMap::<String, usize>::new();
    for (_, info) in hf131_unique_observations(observations) {
        if info.height == 0 || info.tip_hash.trim().is_empty() { continue; }
        view.contacted += 1;
        if info.height > parent_height {
            view.raw_ahead_height = view.raw_ahead_height.max(info.height);
            *ahead.entry((info.height, info.tip_hash)).or_default() += 1;
        } else if info.height == parent_height {
            if info.tip_hash == parent_hash { view.exact += 1; }
            else { *conflicts.entry(info.tip_hash).or_default() += 1; }
        }
    }
    view.ahead_height = ahead.into_iter().filter(|(_, count)| *count >= 2)
        .map(|((height, _), _)| height).max().unwrap_or(0);
    for (hash, count) in conflicts {
        view.raw_conflicts.push(hash.clone());
        if count >= 2 { view.conflicts.push(hash); }
    }
    view
}

fn hf131_observe_parent(settings: &Settings, height: u32, hash: &str, max_peers: usize, timeout_ms: u64)
    -> (Option<(usize, u32, String)>, Option<(u32, String)>, Hf131ParentView)
{
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let inherited = hf130_io_context();
    thread::scope(|scope| {
        let official = scope.spawn(|| {
            let _scope = Hf130IoScope::enter(inherited.clone());
            let _deadline = Hf130IoScope::deadline(deadline);
            best_official_peer_tip(settings, timeout_ms).ok()
        });
        let http = scope.spawn(|| {
            let _scope = Hf130IoScope::enter(inherited.clone());
            let _deadline = Hf130IoScope::deadline(deadline);
            official_http_tip(settings, timeout_ms).ok().flatten()
        });
        let direct = hf131_direct_observations(settings, max_peers, deadline);
        (official.join().ok().flatten(), http.join().ok().flatten(), hf131_parent_view(height, hash, direct))
    })
}

fn official_snapshot_peer_candidates(
    settings: &Settings,
    timeout_ms: u64,
) -> Vec<(String, u32, String, u128, String)> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let mut out: Vec<(String, u32, String, u128, String)> = Vec::new();
    let mut seen_node_ids = HashMap::<String, String>::new();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(150));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let peers = official_snapshot_peers(settings).into_iter().filter(|addr| !should_skip_outbound(settings, addr)).take(4).collect::<Vec<_>>();
    let Ok(version) = hf131_probe_version(settings) else { return out; };
    for (addr, (result, elapsed)) in hf131_probe_batch(&peers, deadline, |addr, left| {
        let started = Instant::now();
        Some((probe_peer_with_version(settings, addr, left, &version), started.elapsed().as_millis()))
    }) {
        match result {
            Ok(info) => {
                let identity = info.node_id.trim().to_string();
                if !identity.is_empty() {
                    if let Some(first_addr) = seen_node_ids.get(&identity) {
                        eprintln!(
                            "HF128 seed identity dedupe: {} and {} report the same node_id {}; counting one independent source",
                            first_addr, addr, identity
                        );
                        continue;
                    }
                    seen_node_ids.insert(identity.clone(), addr.clone());
                }
                out.push((
                    addr,
                    info.height,
                    info.tip_hash,
                    elapsed,
                    identity,
                ));
            }
            Err(_) => out.push((
                addr,
                0,
                String::new(),
                elapsed,
                String::new(),
            )),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.3.cmp(&b.3)).then(a.0.cmp(&b.0)));
    out
}

fn best_official_peer_tip(settings: &Settings, timeout_ms: u64) -> Result<(usize, u32, String)> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let candidates = official_snapshot_peer_candidates(settings, timeout_ms);
    let contacted = candidates
        .iter()
        .filter(|(_, height, _, _, node_id)| *height > 0 && !node_id.trim().is_empty())
        .count();
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    for (_, height, tip, _, node_id) in candidates {
        if height == 0 || tip.trim().is_empty() || node_id.trim().is_empty() {
            continue;
        }
        if height > best_height {
            best_height = height;
            best_tip = tip;
        }
    }
    Ok((contacted, best_height, best_tip))
}

fn local_chain_contains_tip(local: &ChainState, height: u32, tip_hash: &str) -> bool {
    if tip_hash.trim().is_empty() {
        return false;
    }
    local
        .blocks
        .get(height as usize)
        .map(|block| block.block_hash().to_string() == tip_hash)
        .unwrap_or(false)
}

fn official_tip_compatible_with_local(
    local: &ChainState,
    official_height: u32,
    official_tip: &str,
) -> bool {
    if official_height == 0 {
        return false;
    }
    if official_height > local.height() {
        return false;
    }
    if official_tip.trim().is_empty() {
        return official_height <= local.height();
    }
    local_chain_contains_tip(local, official_height, official_tip)
}

/// HF114/v1.7.2: mainnet mining needs an exact live acknowledgement of the
/// current local tip. A local chain that merely contains the official tip as an
/// older ancestor is not canonical; it is a private/self-mined suffix and must be
/// paused or re-anchored before any new candidate is built.
fn hf114_official_tip_acknowledges_local(
    settings: &Settings,
    local: &ChainState,
    official_height: u32,
    official_tip: &str,
) -> bool {
    if settings.network.name != "mainnet" {
        return official_tip_compatible_with_local(local, official_height, official_tip);
    }
    if official_height == 0 || official_height != local.height() {
        return false;
    }
    let official_tip = official_tip.trim();
    !official_tip.is_empty() && official_tip == local.tip_hash().to_string()
}

fn hf114_official_tip_is_local_ancestor(
    settings: &Settings,
    local: &ChainState,
    official_height: u32,
    official_tip: &str,
) -> bool {
    if settings.network.name != "mainnet" {
        return false;
    }
    if official_height == 0 || official_height >= local.height() {
        return false;
    }
    let official_tip = official_tip.trim();
    official_tip.is_empty() || local_chain_contains_tip(local, official_height, official_tip)
}

/// HF116/v1.7.4: liveness-preserving exact HTTP acknowledgement. HF114 made
/// live mainnet mining depend on exact TCP seed/direct peer acknowledgement. If
/// seed TCP is unavailable or stale while the official published snapshot tip is
/// exactly our local tip, all honest GUI miners can deadlock at the public tip.
/// HTTP is not allowed to green-light a private local suffix; it is exact-tip only
/// and still loses to a two-peer direct quorum that proves a different/ahead tip.
fn hf115_http_tip_acknowledges_local(
    settings: &Settings,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if settings.network.name != "mainnet" {
        return false;
    }
    let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.clamp(100, 2_500))
    else {
        return false;
    };
    hf114_official_tip_acknowledges_local(settings, local, height, &tip)
}

fn hf115_http_tip_acknowledges_parent(
    settings: &Settings,
    parent_height: u32,
    parent_hash: &str,
    timeout_ms: u64,
) -> bool {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if settings.network.name != "mainnet" {
        return false;
    }
    let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.clamp(100, 2_500))
    else {
        return false;
    };
    height == parent_height && !tip.trim().is_empty() && tip == parent_hash
}

fn hf129_preobserved_http_exact_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    height: u32,
    tip: &str,
    timeout_ms: u64,
) -> bool {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if !hf114_official_tip_acknowledges_local(settings, local, height, tip) {
        return false;
    }
    let local_tip = local.tip_hash().to_string();
    let (contacted, quorum_ahead_height, quorum_conflicts) = direct_parent_view(
        settings,
        local.height(),
        &local_tip,
        settings.p2p.max_outbound_peers.max(6).min(12),
        timeout_ms.clamp(100, 700),
    )
    .unwrap_or((0, 0, Vec::new()));
    report.peers_contacted = report.peers_contacted.max(contacted);
    hf129_observe_tip(report, quorum_ahead_height, "");
    if quorum_ahead_height > local.height() || !quorum_conflicts.is_empty() {
        return false;
    }
    mark_fresh_tip_trusted(settings, local);
    hf130_mark_exact_tip_proven(report, local, Hf130CanonicalEvidence::OfficialHttp);
    true
}

fn official_tip_summary(
    settings: &Settings,
    report: &mut P2PSyncReport,
    timeout_ms: u64,
) -> (usize, u32, String) {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let (contacted, height, tip) =
        best_official_peer_tip(settings, timeout_ms).unwrap_or((0, 0, String::new()));
    report.peers_contacted = report.peers_contacted.max(contacted);
    hf129_observe_tip(report, height, &tip);
    (contacted, height, tip)
}

fn official_tip_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let (contacted, official_height, official_tip) =
        official_tip_summary(settings, report, timeout_ms);
    if contacted == 0 {
        return false;
    }
    let ok = if settings.network.name == "mainnet" {
        hf114_official_tip_acknowledges_local(settings, local, official_height, &official_tip)
    } else {
        official_tip_compatible_with_local(local, official_height, &official_tip)
    };
    if ok {
        mark_fresh_tip_trusted(settings, local);
        if official_height == local.height() && official_tip == local.tip_hash().to_string() {
            hf130_mark_exact_tip_proven(report, local, Hf130CanonicalEvidence::OfficialTcp);
        }
        return true;
    }
    false
}

/// HF107/v1.6.9: mainnet mining green-light must be based on the
/// freshest official/direct source as a set, not on whichever source happens
/// to return a compatible older tip first. If any official source we can see
/// is ahead of the local validated chain, mining stays paused until catch-up.
fn hf104_canonical_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    let _scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let local_tip = local.tip_hash().to_string();
    // All three lanes receive the same budget. Official outages cannot consume
    // time reserved for direct peers or hide a responsive second seed.
    let (official, http, direct) = hf131_observe_parent(
        settings, local.height(), &local_tip, 12, timeout_ms.clamp(100, 3_000),
    );
    report.canonical_proof = None;
    report.canonical_tip_proven = false;
    let mut best_height = 0;
    let mut best_tip = String::new();
    let mut contacted = direct.contacted;
    if let Some((count, height, tip)) = official.as_ref() {
        contacted += *count;
        if *count > 0 { best_height = *height; best_tip = tip.clone(); }
    }
    if let Some((height, tip)) = http.as_ref() {
        contacted += 1;
        if *height >= best_height { best_height = *height; best_tip = tip.clone(); }
    }
    report.peers_contacted = report.peers_contacted.max(contacted);
    hf129_observe_tip(report, best_height, &best_tip);
    hf129_observe_tip(report, direct.ahead_height, "");

    if settings.network.name != "mainnet" {
        if official.as_ref().map_or(false, |(count, height, tip)| {
            *count > 0 && official_tip_compatible_with_local(local, *height, tip)
        }) || http.as_ref().map_or(false, |(height, tip)| official_tip_compatible_with_local(local, *height, tip)) {
            mark_fresh_tip_trusted(settings, local);
            return true;
        }
        return false;
    }

    // A previously observed newer source cannot be overwritten by an older
    // exact HTTP publication. A single untrusted high peer remains telemetry.
    if hf131_parent_is_behind(local.height(), best_height, direct.ahead_height) { return false; }
    let pending = pending_block_relay_waiting(settings, local);
    if !direct.conflicts.is_empty() {
        let verdict = hf130_quorum_verdict(direct.exact, false, true, !pending);
        let allowed = hf130_apply_quorum_verdict(report, verdict, local.height(), &local_tip);
        if allowed { mark_fresh_tip_trusted(settings, local); }
        return allowed;
    }

    let official_exact = official.as_ref().map_or(false, |(count, height, tip)| {
        *count > 0 && hf114_official_tip_acknowledges_local(settings, local, *height, tip)
    });
    let http_exact = http.as_ref().map_or(false, |(height, tip)| {
        hf114_official_tip_acknowledges_local(settings, local, *height, tip)
    });
    if official_exact || http_exact {
        mark_fresh_tip_trusted(settings, local);
        hf130_mark_exact_tip_proven(report, local, if official_exact {
            Hf130CanonicalEvidence::OfficialTcp
        } else { Hf130CanonicalEvidence::OfficialHttp });
        return true;
    }
    let verdict = hf130_quorum_verdict(direct.exact, false, false, !pending);
    if hf130_apply_quorum_verdict(report, verdict, local.height(), &local_tip) {
        mark_fresh_tip_trusted(settings, local);
        return true;
    }
    // Preserve HF125/HF126 permission for an acknowledged higher local suffix
    // or an ordinary equal-height PoW tie. Neither is an exact freshness proof.
    if !pending && ((best_height > 0 && best_height < local.height()
        && local_chain_contains_tip(local, best_height, &best_tip))
        || hf126_is_same_height_competing_tip(local.height(), &local_tip, best_height, &best_tip)) {
        mark_fresh_tip_trusted(settings, local);
        return true;
    }
    false
}

fn hf131_parent_is_behind(parent_height: u32, official_height: u32, direct_quorum_height: u32) -> bool {
    official_height > parent_height || direct_quorum_height > parent_height
}

fn local_is_at_or_past_tip(local: &ChainState, height: u32, tip_hash: &str) -> bool {
    if height == 0 {
        return true;
    }
    if height > local.height() {
        return false;
    }
    if tip_hash.trim().is_empty() {
        return true;
    }
    local_chain_contains_tip(local, height, tip_hash)
}

fn fresh_tip_still_matches_light_network(
    settings: &Settings,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if !fresh_tip_is_trusted(settings, local) {
        return false;
    }

    // HF107/v1.6.9: on mainnet, a cached fresh-tip trust entry is never enough
    // to green-light mining if official/direct sources cannot be sampled right
    // now. This prevents the "local fake branch" case where the UI already knows
    // an official/direct tip is ahead but the miner reuses a short-lived local
    // trust cache and hashes an old parent.
    if settings.network.name == "mainnet" {
        let mut report = P2PSyncReport::default();
        return hf104_canonical_greenlight(
            settings,
            &mut report,
            local,
            timeout_ms.clamp(100, 1_200),
        );
    }

    let mut best_height = 0u32;
    let mut best_tip = String::new();
    if let Ok((contacted, height, tip)) = best_official_peer_tip(settings, timeout_ms.max(120)) {
        if contacted > 0 {
            best_height = height;
            best_tip = tip;
        }
    }
    if let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(700)) {
        if height > best_height {
            best_height = height;
            best_tip = tip;
        }
    }
    if let Ok((contacted, height, tip)) = best_reachable_peer_tip(settings, 4, timeout_ms.max(120))
    {
        if contacted > 0 && height > best_height {
            best_height = height;
            best_tip = tip;
        }
    }
    if best_height > local.height()
        && hf97_uncatchable_tip_quarantined(settings, local, best_height)
    {
        return true;
    }
    best_height == 0 || local_is_at_or_past_tip(local, best_height, &best_tip)
}

fn best_reachable_peer_tip(
    settings: &Settings,
    max_peers: usize,
    timeout_ms: u64,
) -> Result<(usize, u32, String)> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let mut contacted = 0usize;
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    // `timeout_ms` is an end-to-end probe deadline, not a fresh allowance for
    // each registry row. Multiplying it by peer count made a light tip check
    // occupy the sync/mining worker for several seconds.
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.clamp(100, 4_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for addr in prioritized_outbound_peers(settings, max_peers.max(1))? {
        if Instant::now() >= deadline {
            break;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match probe_peer(
            settings,
            &addr,
            left.min(Duration::from_millis(700)),
        ) {
            Ok(info) => {
                contacted = contacted.saturating_add(1);
                if info.height > best_height
                    || (info.height == best_height && !info.tip_hash.trim().is_empty())
                {
                    best_height = info.height;
                    best_tip = info.tip_hash;
                }
            }
            Err(_) => {}
        }
    }
    Ok((contacted, best_height, best_tip))
}

/// HF113/v1.7.1: if official seeds are temporarily unavailable, do not shut
/// the whole network down. Allow mining only when directly reachable peers give
/// a conservative quorum view that is compatible with the local validated tip.
/// A single random future/ghost peer is telemetry only and cannot green-light or
/// pause mining by itself.
fn hf113_peer_quorum_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    max_peers: usize,
    timeout_ms: u64,
) -> bool {
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let observations = hf131_direct_observations(settings, max_peers, deadline);
    let tip = local.tip_hash().to_string();
    let view = hf131_parent_view(local.height(), &tip, observations);
    report.peers_contacted = report.peers_contacted.max(view.contacted);
    hf129_observe_tip(report, view.ahead_height, "");
    let verdict = hf130_quorum_verdict(view.exact, view.ahead_height > local.height(),
        !view.conflicts.is_empty(), settings.network.name == "mainnet" && !pending_block_relay_waiting(settings, local));
    let allowed = hf130_apply_quorum_verdict(report, verdict, local.height(), &tip);
    if allowed { mark_fresh_tip_trusted(settings, local); }
    allowed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Hf130QuorumVerdict { Rejected, Exact, MineableTie }

fn hf130_quorum_verdict(
    exact_matches: usize,
    ahead_quorum: bool,
    conflicting_quorum: bool,
    tie_mineable: bool,
) -> Hf130QuorumVerdict {
    if ahead_quorum {
        Hf130QuorumVerdict::Rejected
    } else if conflicting_quorum {
        if tie_mineable { Hf130QuorumVerdict::MineableTie } else { Hf130QuorumVerdict::Rejected }
    } else if exact_matches >= 2 {
        Hf130QuorumVerdict::Exact
    } else {
        Hf130QuorumVerdict::Rejected
    }
}

fn hf130_apply_quorum_verdict(
    report: &mut P2PSyncReport,
    verdict: Hf130QuorumVerdict,
    local_height: u32,
    local_tip: &str,
) -> bool {
    // A newer tie/veto cannot retain an exact proof from an earlier pass.
    report.canonical_proof = if verdict == Hf130QuorumVerdict::Exact {
        Some(Hf130CanonicalProof {
            height: local_height,
            tip_hash: local_tip.to_string(),
            observed_at: Instant::now(),
            source: Hf130CanonicalEvidence::DirectQuorum,
        })
    } else { None };
    report.canonical_tip_proven = report.canonical_proof.is_some();
    verdict != Hf130QuorumVerdict::Rejected
}

fn hf115_direct_parent_ack_quorum(
    settings: &Settings,
    parent_height: u32,
    parent_hash: &str,
    max_peers: usize,
    timeout_ms: u64,
) -> bool {
    if settings.network.name != "mainnet" { return false; }
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let observations = hf131_direct_observations(settings, max_peers, deadline);
    let view = hf131_parent_view(parent_height, parent_hash, observations);
    view.exact >= 2 && view.ahead_height == 0 && view.conflicts.is_empty()
}

/// HF113/v1.7.1: lightweight active-mining pause probe. This is intentionally
/// fast and non-repairing: it can stop workers within a few seconds when the
/// official/direct chain or a peer quorum has moved, without making the miner
/// continue hashing while a heavy catch-up function is blocked.
pub fn hf113_live_tip_pause_reason(
    settings: &Settings,
    parent_height: u32,
    parent_hash: Hash256,
    timeout_ms: u64,
) -> Option<String> {
    let _scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan") {
        return None;
    }
    let parent_hash_s = parent_hash.to_string();
    let (official, http, direct) = hf131_observe_parent(settings, parent_height, &parent_hash_s, 12, timeout_ms);
    let official_height = official.as_ref().filter(|(count, _, _)| *count > 0)
        .map(|(_, height, _)| *height).unwrap_or(0);
    let http_height = http.as_ref().map(|(height, _)| *height).unwrap_or(0);
    if hf131_parent_is_behind(parent_height, official_height.max(http_height), direct.ahead_height) {
        return Some(format!("live network view advanced to #{} while candidate parent is #{}",
            official_height.max(http_height).max(direct.ahead_height), parent_height));
    }
    if settings.network.name != "mainnet" && !direct.conflicts.is_empty() {
        return Some(format!("direct peer quorum disagrees at candidate parent #{}", parent_height));
    }
    if let Some((count, height, tip)) = official {
        if count > 0 && settings.network.name == "mainnet" && (height < parent_height || tip.trim().is_empty()) {
            let http_exact = http.as_ref().map_or(false, |(h, hash)| *h == parent_height && hash == &parent_hash_s);
            if (http_exact || direct.exact >= 2) && direct.conflicts.is_empty() { return None; }
            // An older seed is not authority to stop an already acknowledged
            // local suffix. Verify ancestry against the exact active parent.
            if let Ok(local) = load_chain_for_hf90_catchup(settings) {
                if local.height() == parent_height && local.tip_hash() == parent_hash
                    && local_chain_contains_tip(&local, height, &tip)
                    && !pending_block_relay_waiting(settings, &local) { return None; }
            }
            return Some(format!("candidate parent #{} awaits live acknowledgement; sampled seed is #{}", parent_height, height));
        }
    }
    // Absence of observations does not invalidate an already running parent.
    // The next candidate still passes the strict canonical/relay gates. Normal
    // same-height PoW competition retains HF126's mining permission.
    None
}

fn best_known_live_tip(
    settings: &Settings,
    report: &mut P2PSyncReport,
    timeout_ms: u64,
) -> (usize, u32, String) {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let (official_contacted, official_height, official_tip) =
        official_tip_summary(settings, report, timeout_ms.max(150));
    if settings.network.name == "mainnet" {
        // HF107: canonical progress on mainnet is official-only. Random/pool/private
        // peers are displayed as telemetry elsewhere, but they must not advance the
        // blue catch-up target or mining/reward confidence.
        return (official_contacted, official_height, official_tip);
    }
    let (direct_contacted, direct_height, direct_tip) =
        best_reachable_peer_tip(settings, 8, timeout_ms.max(150)).unwrap_or((0, 0, String::new()));
    report.peers_contacted = report.peers_contacted.saturating_add(direct_contacted);
    hf129_observe_tip(report, direct_height, &direct_tip);
    if direct_height > official_height {
        (
            official_contacted.saturating_add(direct_contacted),
            direct_height,
            direct_tip,
        )
    } else {
        (
            official_contacted.saturating_add(direct_contacted),
            official_height,
            official_tip,
        )
    }
}

fn hf97_visible_tip(settings: &Settings, timeout_ms: u64) -> (usize, u32, String) {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let total_budget = timeout_ms.clamp(100, 3_000);
    let deadline = Instant::now() + Duration::from_millis(total_budget);
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let probe_budget = total_budget.saturating_sub(100).max(100);
    let inherited = hf130_io_context();
    let (direct_observation, http_observation) = thread::scope(|scope| {
        let direct = scope.spawn(|| {
            let _scope = Hf130IoScope::enter(inherited.clone());
            best_official_peer_tip(settings, probe_budget).ok()
        });
        let http = scope.spawn(|| {
            let _scope = Hf130IoScope::enter(inherited.clone());
            official_http_tip(settings, probe_budget).ok().flatten()
        });
        (
            direct.join().ok().flatten(),
            http.join().ok().flatten(),
        )
    });
    let mut contacted = 0usize;
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    if let Some((c, h, tip)) = direct_observation {
        contacted = contacted.saturating_add(c);
        if h > best_height || (h == best_height && !tip.trim().is_empty()) {
            best_height = h;
            best_tip = tip;
        }
    }
    if let Some((h, tip)) = http_observation {
        contacted = contacted.saturating_add(1);
        if h > best_height || (h == best_height && !tip.trim().is_empty()) {
            best_height = h;
            best_tip = tip;
        }
    }
    // HF107: arbitrary reachable peers are useful telemetry, but never canonical
    // on mainnet. If official sources are unavailable, mainnet reports no trusted
    // visible tip instead of chasing ghost/private/pool branches. Test/regtest can
    // still use generic reachable peers for developer diagnostics.
    if best_height == 0 && settings.network.name != "mainnet" {
        let peer_budget = remaining_ms(deadline);
        if peer_budget >= 100 {
            if let Ok((c, h, tip)) = best_reachable_peer_tip(settings, 8, peer_budget) {
                contacted = contacted.saturating_add(c);
                if h > best_height || (h == best_height && !tip.trim().is_empty()) {
                    best_height = h;
                    best_tip = tip;
                }
            }
        }
    }
    (contacted, best_height, best_tip)
}

fn hf82_catchup_gate() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

static HF129_PRIORITY_CATCHUP_WAITERS: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// A complete public sync/mining workflow may call the coordinated ladder
    /// internally. Keep the process-wide mutex re-entrant only for that same
    /// thread so one permit can cover the whole mutation workflow without
    /// deadlocking its nested repair stages.
    static HF129_CATCHUP_WRITER_DEPTH: Cell<u32> = const { Cell::new(0) };
    static HF132_DEFERRED_CHAIN_RELAY: RefCell<Option<(Settings, Option<String>)>> = const { RefCell::new(None) };
}

struct Hf129CatchupWriter {
    _guard: Option<MutexGuard<'static, ()>>,
}

impl Hf129CatchupWriter {
    fn nested() -> Option<Self> {
        HF129_CATCHUP_WRITER_DEPTH.with(|depth| {
            let current = depth.get();
            if current == 0 {
                None
            } else {
                depth.set(current.saturating_add(1));
                Some(Self { _guard: None })
            }
        })
    }

    fn outer(guard: MutexGuard<'static, ()>) -> Self {
        HF129_CATCHUP_WRITER_DEPTH.with(|depth| {
            debug_assert_eq!(depth.get(), 0);
            depth.set(1);
        });
        Self {
            _guard: Some(guard),
        }
    }
}

impl Drop for Hf129CatchupWriter {
    fn drop(&mut self) {
        let outer_finished = HF129_CATCHUP_WRITER_DEPTH.with(|depth| {
            let current = depth.get();
            debug_assert!(current > 0);
            depth.set(current.saturating_sub(1));
            current == 1
        });
        // Release the writer before scheduling any outbound diffusion. During
        // bulk sync all accepted chunks coalesce into this one final relay.
        if outer_finished {
            drop(self._guard.take());
            let pending = HF132_DEFERRED_CHAIN_RELAY.with(|slot| slot.borrow_mut().take());
            if let Some((settings, source)) = pending {
                hf132_spawn_chain_relay(settings, source);
            }
        }
    }
}

fn hf132_spawn_chain_relay(settings: Settings, source: Option<String>) {
    hf127_spawn_detached("qub-sync-chain-diffusion", move || {
        let _scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(6));
        let _ = relay_chain_to_known_peers(&settings, source.as_deref());
    });
}

fn hf132_defer_chain_relay(settings: &Settings, source: &str) {
    let pending = (settings.clone(), Some(source.to_string()));
    if HF129_CATCHUP_WRITER_DEPTH.with(|depth| depth.get() > 0) {
        HF132_DEFERRED_CHAIN_RELAY.with(|slot| *slot.borrow_mut() = Some(pending));
    } else {
        hf132_spawn_chain_relay(pending.0, pending.1);
    }
}

struct Hf129PriorityWaiter;

impl Hf129PriorityWaiter {
    fn begin() -> Self {
        HF129_PRIORITY_CATCHUP_WAITERS.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for Hf129PriorityWaiter {
    fn drop(&mut self) {
        HF129_PRIORITY_CATCHUP_WAITERS.fetch_sub(1, Ordering::SeqCst);
    }
}

/// HF129/v1.8.5 process-wide catch-up coordinator. Background pulses coalesce
/// immediately, but manual/mining repair waits for the real writer instead of
/// consuming a UI request and returning a false-success local report.
fn hf129_acquire_catchup_writer(
    priority: bool,
    deadline: Instant,
) -> Result<Option<Hf129CatchupWriter>> {
    let _hf130_entry_scope = Hf130IoScope::deadline(deadline);
    match hf130_io_remaining(deadline) {
        Ok(_) => {}
        Err(err) if err.kind() == io::ErrorKind::TimedOut => return Ok(None),
        Err(err) => return Err(err.into()),
    }
    if let Some(nested) = Hf129CatchupWriter::nested() {
        return Ok(Some(nested));
    }
    if !priority {
        if HF129_PRIORITY_CATCHUP_WAITERS.load(Ordering::SeqCst) > 0 {
            return Ok(None);
        }
        return match hf82_catchup_gate().try_lock() {
            Ok(guard) => Ok(Some(Hf129CatchupWriter::outer(guard))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Poisoned(_)) => bail!("catch-up writer mutex poisoned"),
        };
    }

    let _waiter = Hf129PriorityWaiter::begin();
    loop {
        match hf130_io_remaining(deadline) {
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::TimedOut => return Ok(None),
            Err(err) => return Err(err.into()),
        }
        match hf82_catchup_gate().try_lock() {
            Ok(guard) => return Ok(Some(Hf129CatchupWriter::outer(guard))),
            Err(TryLockError::Poisoned(_)) => bail!("catch-up writer mutex poisoned"),
            Err(TryLockError::WouldBlock) => {}
        }
        let left = match hf130_io_remaining(deadline) {
            Ok(left) => left,
            Err(err) if err.kind() == io::ErrorKind::TimedOut => return Ok(None),
            Err(err) => return Err(err.into()),
        };
        thread::sleep(left.min(Duration::from_millis(50)));
    }
}

fn hf129_mark_writer_busy(report: &mut P2PSyncReport, reason: &str) {
    report.writer_busy = true;
    report.sync_reason = reason.to_string();
    report.terminal = "busy".to_string();
}

fn hf129_mark_writer_acquired(
    report: &mut P2PSyncReport,
    reason: &str,
    before: &ChainState,
) {
    report.writer_acquired = true;
    report.sync_reason = reason.to_string();
    report.before_height = before.height();
    report.before_tip_hash = before.tip_hash().to_string();
}

fn hf129_note_canonical_greenlight(
    report: &mut P2PSyncReport,
    green: bool,
) -> bool {
    if green {
        hf129_mark_canonical_tip_proven(report);
    }
    green
}

fn hf129_acquire_sync_step(
    settings: &Settings,
    report: &mut P2PSyncReport,
    wait_timeout_ms: u64,
    priority: bool,
    reason: &str,
) -> Result<Option<Hf129CatchupWriter>> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(wait_timeout_ms));
    let deadline = Instant::now()
        + Duration::from_millis(wait_timeout_ms.clamp(1_000, 30_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let Some(guard) = hf129_acquire_catchup_writer(priority, deadline)? else {
        hf129_mark_writer_busy(report, reason);
        return Ok(None);
    };
    let before = load_chain_for_hf90_catchup(settings)?;
    hf129_mark_writer_acquired(report, reason, &before);
    Ok(Some(guard))
}

fn remaining_ms(deadline: Instant) -> u64 {
    hf130_io_remaining(deadline)
        .unwrap_or(Duration::ZERO)
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn hf82_catchup_impl(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_full_snapshot: bool,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let priority = allow_full_snapshot;
    let overall_deadline = Instant::now()
        + Duration::from_millis(total_timeout_ms.clamp(1_500, 90_000));
    let overall_deadline = hf130_effective_deadline(overall_deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(overall_deadline);
    let _guard = match hf129_acquire_catchup_writer(priority, overall_deadline)? {
        Some(guard) => guard,
        None => {
            hf129_mark_writer_busy(
                &mut report,
                if priority { "legacy-priority" } else { "legacy-auto" },
            );
            return finish_report(settings, report);
        }
    };
    if priority && remaining_ms(overall_deadline) < 1_200 {
        drop(_guard);
        hf129_mark_writer_busy(&mut report, "legacy-priority-deadline");
        return finish_report(settings, report);
    }

    let deadline = overall_deadline;
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let mut local = load_chain_for_hf90_catchup(settings)?;
    hf129_mark_writer_acquired(
        &mut report,
        if priority { "legacy-priority" } else { "legacy-auto" },
        &local,
    );
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    report.height = local.height();
    report.tip_hash = local.tip_hash().to_string();

    let direct_budget = remaining_ms(deadline).min(4_000);
    if direct_budget >= 1_200 {
        hf131_sync_direct_before_snapshots(settings, &mut report, direct_budget);
    }

    // 1) Fast official missing-suffix path. This is the common behind-by-1..N case
    // and should never touch full snapshots.
    let left = remaining_ms(deadline);
    if left >= 1_200 {
        if let Ok(suffix) = sync_official_suffix(settings, left.min(5_500).max(1_500)) {
            merge_sync_reports(&mut report, suffix);
        }
    }
    local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if fresh_tip_still_matches_light_network(settings, &local, HF82_LIGHT_TIP_PROBE_MS) {
        mark_fresh_tip_trusted(settings, &local);
        hf129_mark_canonical_tip_proven(&mut report);
        return finish_report(settings, report);
    }

    // 2) Official HTTP tip/tail. Tails are deterministic, consensus-validated and
    // excellent for users who are a few blocks behind while the seed snapshot is live.
    let left = remaining_ms(deadline);
    if left >= 2_000 {
        let local_tip = local.tip_hash().to_string();
        if let Ok(Some((http_h, http_tip))) =
            official_http_tip(settings, left.min(2_200).max(1_500))
        {
            hf129_observe_tip(&mut report, http_h, &http_tip);
            let wrong_same_height =
                http_h == local.height() && !http_tip.trim().is_empty() && http_tip != local_tip;
            if http_h > local.height() || wrong_same_height {
                if let Ok(tail) =
                    sync_official_http_tail(settings, remaining_ms(deadline).min(8_500).max(2_000))
                {
                    merge_sync_reports(&mut report, tail);
                }
            }
        }
    }
    local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if fresh_tip_still_matches_light_network(settings, &local, HF82_LIGHT_TIP_PROBE_MS) {
        mark_fresh_tip_trusted(settings, &local);
        hf129_mark_canonical_tip_proven(&mut report);
        return finish_report(settings, report);
    }

    // 3) Direct peer adaptive sync. Keep it short for auto/mining. Manual Sync can
    // pass a larger budget and may continue to full official snapshot below.
    let left = remaining_ms(deadline);
    if left >= 2_000 {
        if let Ok(quick) = sync_quick(
            settings,
            settings.p2p.max_outbound_peers.max(6).min(12),
            left.min(7_500).max(2_000),
        ) {
            merge_sync_reports(&mut report, quick);
        }
    }
    local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if fresh_tip_still_matches_light_network(settings, &local, HF82_LIGHT_TIP_PROBE_MS) {
        mark_fresh_tip_trusted(settings, &local);
        hf129_mark_canonical_tip_proven(&mut report);
        return finish_report(settings, report);
    }

    // 4) Manual/explicit repair only: full official snapshot fallback. This is not
    // used by normal GUI background refresh, so users never lose wallet/balance UI
    // for minutes while a heavy repair is running.
    if allow_full_snapshot {
        let left = remaining_ms(deadline);
        if left >= 10_000 {
            if let Ok(full) = sync_official_http_snapshot(settings, left.min(45_000).max(10_000)) {
                merge_sync_reports(&mut report, full);
            }
        }
    }

    finish_report(settings, report)
}

pub fn hf82_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf82_catchup_impl(
        settings,
        total_timeout_ms.min(HF82_AUTO_CATCHUP_MS).max(1_500),
        false,
    )
}

pub fn hf82_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf82_catchup_impl(
        settings,
        total_timeout_ms.min(HF82_MINING_CATCHUP_MS).max(2_500),
        false,
    )
}

pub fn hf82_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf82_catchup_impl(settings, total_timeout_ms.max(8_000), true)
}

fn hf85_catchup_pulse(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_full_snapshot: bool,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.clamp(1_500, 60_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);

    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        remaining_ms(deadline),
        allow_full_snapshot,
        if allow_full_snapshot {
            "hf85-priority-workflow"
        } else {
            "hf85-auto-workflow"
        },
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    // HF88/v1.6.2: no recursive self-heal and no global P2P gate here. This is
    // a one-shot pulse intended for detached GUI workers and mining green-light.
    // Each step is independently bounded and errors fall through to the next step.
    let left = remaining_ms(deadline);
    if left >= 1_200 {
        if let Ok(suffix) = sync_official_suffix(settings, left.min(4_500).max(1_500)) {
            merge_sync_reports(&mut report, suffix);
        }
    }

    let local_h = load_chain_for_hf90_catchup(settings)
        .map(|c| c.height())
        .unwrap_or(0);
    let official_tip = if remaining_ms(deadline) >= 800 {
        official_http_tip(settings, remaining_ms(deadline).min(1_600).max(700))
            .ok()
            .flatten()
    } else {
        None
    };

    if let Some((official_h, _)) = official_tip.clone() {
        if official_h > local_h || allow_full_snapshot {
            let left = remaining_ms(deadline);
            if left >= 1_500 {
                if let Ok(tail) = sync_official_http_tail(settings, left.min(6_000).max(1_500)) {
                    merge_sync_reports(&mut report, tail);
                }
            }
        }
    }

    let local_h_after_tail = load_chain_for_hf90_catchup(settings)
        .map(|c| c.height())
        .unwrap_or(local_h);
    if allow_full_snapshot {
        if let Some((official_h, _)) = official_tip {
            if official_h > local_h_after_tail && remaining_ms(deadline) >= 8_000 {
                if let Ok(full) = sync_official_http_snapshot(
                    settings,
                    remaining_ms(deadline).min(24_000).max(8_000),
                ) {
                    merge_sync_reports(&mut report, full);
                }
            }
        }
    }

    let left = remaining_ms(deadline);
    if left >= 1_200 {
        if let Ok(quick) = sync_quick(
            settings,
            settings.p2p.max_outbound_peers.max(6).min(10),
            left.min(2_500).max(1_200),
        ) {
            merge_sync_reports(&mut report, quick);
        }
    }

    finish_report(settings, report)
}

pub fn hf85_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    // HF88: background auto pulses are intentionally append/tail only. Full
    // snapshot repair is reserved for manual Sync/Repair or mining workers so
    // the GUI cannot starve its first local wallet/block snapshot.
    hf85_catchup_pulse(settings, total_timeout_ms.min(10_000).max(1_500), false)
}

pub fn hf85_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    // Worker-thread mining green-light can also use the canonical HTTP snapshot
    // fallback to avoid infinite one/few-block-behind loops.
    hf85_catchup_pulse(settings, total_timeout_ms.min(16_000).max(2_500), true)
}

pub fn hf85_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf85_catchup_pulse(settings, total_timeout_ms.max(8_000), true)
}

/// HF88/v1.6.2: detached GUI catch-up is still bounded, but it may use the
/// official HTTP snapshot fallback when suffix/tail sync does not move a stale
/// local chain. This runs off the UI thread and is single-purpose: make nodes
/// that were left open overnight catch up again without manual babysitting.
pub fn hf86_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        total_timeout_ms,
        false,
        "hf86-auto-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, workflow_report),
    };
    let mut report = workflow_report;
    let pulse = hf85_catchup_pulse(settings, total_timeout_ms.min(12_000).max(2_000), false)?;
    merge_sync_reports(&mut report, pulse);
    let local_now = load_chain_for_hf90_catchup(settings).ok();
    let local_h = local_now
        .as_ref()
        .map(|c| c.height())
        .unwrap_or(report.height);
    let local_tip = local_now
        .as_ref()
        .map(|c| c.tip_hash().to_string())
        .unwrap_or_default();
    if let Ok(Some((official_h, official_tip))) = official_http_tip(settings, 2_500) {
        hf129_observe_tip(&mut report, official_h, &official_tip);
        let same_height_wrong_tip =
            official_h == local_h && !official_tip.trim().is_empty() && official_tip != local_tip;
        if official_h > local_h || same_height_wrong_tip {
            if let Ok(full) =
                sync_official_http_snapshot(settings, total_timeout_ms.min(24_000).max(10_000))
            {
                merge_sync_reports(&mut report, full);
            }
        }
    }
    finish_report(settings, report)
}

pub fn hf86_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "hf86-manual-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, workflow_report),
    };
    let mut report = workflow_report;
    let pulse = hf85_manual_catchup(settings, total_timeout_ms.max(12_000))?;
    merge_sync_reports(&mut report, pulse);
    let local_now = load_chain_for_hf90_catchup(settings).ok();
    let local_h = local_now
        .as_ref()
        .map(|c| c.height())
        .unwrap_or(report.height);
    let local_tip = local_now
        .as_ref()
        .map(|c| c.tip_hash().to_string())
        .unwrap_or_default();
    if let Ok(Some((official_h, official_tip))) = official_http_tip(settings, 3_000) {
        hf129_observe_tip(&mut report, official_h, &official_tip);
        let same_height_wrong_tip =
            official_h == local_h && !official_tip.trim().is_empty() && official_tip != local_tip;
        if official_h > local_h || same_height_wrong_tip {
            if let Ok(full) =
                sync_official_http_snapshot(settings, total_timeout_ms.min(45_000).max(12_000))
            {
                merge_sync_reports(&mut report, full);
            }
        }
    }
    finish_report(settings, report)
}

pub fn hf86_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "hf86-mining-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, workflow_report),
    };
    let mut report = workflow_report;
    let pulse = hf85_mining_catchup(settings, total_timeout_ms.max(8_000))?;
    merge_sync_reports(&mut report, pulse);
    let local_now = load_chain_for_hf90_catchup(settings).ok();
    let local_h = local_now
        .as_ref()
        .map(|c| c.height())
        .unwrap_or(report.height);
    let local_tip = local_now
        .as_ref()
        .map(|c| c.tip_hash().to_string())
        .unwrap_or_default();
    if let Ok(Some((official_h, official_tip))) = official_http_tip(settings, 2_500) {
        hf129_observe_tip(&mut report, official_h, &official_tip);
        let same_height_wrong_tip =
            official_h == local_h && !official_tip.trim().is_empty() && official_tip != local_tip;
        if official_h > local_h || same_height_wrong_tip {
            if let Ok(full) =
                sync_official_http_snapshot(settings, total_timeout_ms.min(26_000).max(12_000))
            {
                merge_sync_reports(&mut report, full);
            }
        }
    }
    finish_report(settings, report)
}

fn hf88_chain_moved(settings: &Settings, before_h: u32, before_tip: &str) -> bool {
    load_chain_for_hf90_catchup(settings)
        .map(|c| c.height() > before_h || c.tip_hash().to_string() != before_tip)
        .unwrap_or(false)
}

fn hf88_best_tip(
    settings: &Settings,
    report: &mut P2PSyncReport,
    timeout_ms: u64,
) -> (u32, String) {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let mut best_h = 0u32;
    let mut best_tip = String::new();
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(100));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let http_budget = remaining_ms(deadline).min(2_000);
    if http_budget >= 100 {
        if let Ok(Some((h, tip))) = official_http_tip(settings, http_budget) {
            report.peers_contacted = report.peers_contacted.saturating_add(1);
            hf129_observe_tip(report, h, &tip);
            best_h = h;
            best_tip = tip;
        }
    }
    let tcp_budget = remaining_ms(deadline).min(900);
    if tcp_budget >= 100 {
        if let Ok((contacted, h, tip)) = best_official_peer_tip(settings, tcp_budget) {
            report.peers_contacted = report.peers_contacted.saturating_add(contacted);
            hf129_observe_tip(report, h, &tip);
            if h > best_h || (h == best_h && !tip.trim().is_empty()) {
                best_h = h;
                best_tip = tip;
            }
        }
    }
    // HF107: for canonical catch-up/mining, do not let a random reachable peer
    // outrank official seed/HTTP tips. On mainnet, if official sources are absent,
    // return no canonical target and let mining wait; peer tips remain telemetry.
    if best_h == 0 && settings.network.name != "mainnet" {
        let peer_budget = remaining_ms(deadline).min(700);
        if peer_budget >= 100 {
            if let Ok((contacted, h, tip)) =
                best_reachable_peer_tip(settings, 6, peer_budget)
            {
                report.peers_contacted = report.peers_contacted.saturating_add(contacted);
                hf129_observe_tip(report, h, &tip);
                if h > best_h || (h == best_h && !tip.trim().is_empty()) {
                    best_h = h;
                    best_tip = tip;
                }
            }
        }
    }
    (best_h, best_tip)
}

fn hf88_catchup_ladder(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_snapshot: bool,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    // HF107/v1.6.9: real catch-up, not another cosmetic pulse. HF88/HF89 made the
    // UI transparent, which exposed the exact failure mode: local #N could see
    // official/direct #N+k forever while short suffix/tail pulses kept rechecking
    // the same gap. HF98 gives the detached writer enough time and prioritizes
    // canonical official P2P repair before generic peer probing. The UI remains
    // read-only/live because this runs behind the GUI single-flight writer gate.
    let budget = if allow_snapshot {
        total_timeout_ms.clamp(90_000, 180_000)
    } else {
        total_timeout_ms.clamp(90_000, 150_000)
    };
    let deadline = Instant::now() + Duration::from_millis(budget);
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let _guard = match hf129_acquire_catchup_writer(allow_snapshot, deadline)? {
        Some(guard) => guard,
        None => {
            hf129_mark_writer_busy(
                &mut report,
                if allow_snapshot { "hf88-priority" } else { "hf88-auto" },
            );
            return finish_report(settings, report);
        }
    };
    if allow_snapshot && remaining_ms(deadline) < 2_000 {
        drop(_guard);
        hf129_mark_writer_busy(&mut report, "hf88-priority-deadline");
        return finish_report(settings, report);
    }

    let mut before = load_chain_for_hf90_catchup(settings)?;
    hf129_mark_writer_acquired(
        &mut report,
        if allow_snapshot { "hf88-priority" } else { "hf88-auto" },
        &before,
    );
    validate_chain_consensus_checkpoints(settings, &before.blocks)?;
    let mut local_h = before.height();
    let mut local_tip = before.tip_hash().to_string();
    report.height = local_h;
    report.tip_hash = local_tip.clone();

    if remaining_ms(deadline) >= 2_000 {
        hf131_sync_direct_before_snapshots(settings, &mut report, remaining_ms(deadline).min(18_000));
        before = load_chain_for_hf90_catchup(settings)?;
        local_h = before.height();
        local_tip = before.tip_hash().to_string();
        report.height = local_h;
        report.tip_hash = local_tip.clone();
    }
    let (mut best_h, mut best_tip) = hf88_best_tip(settings, &mut report, 2_500);
    let same_height_wrong_tip =
        best_h == local_h && !best_tip.trim().is_empty() && best_tip != local_tip;
    let should_catch = best_h == 0
        || best_h > local_h
        || same_height_wrong_tip
        || !fresh_tip_still_matches_light_network(settings, &before, 1_200);
    if !should_catch {
        mark_fresh_tip_trusted(settings, &before);
        hf129_mark_canonical_tip_proven(&mut report);
        return finish_report(settings, report);
    }

    let mut no_progress_steps = 0usize;
    let mut round = 0usize;
    while remaining_ms(deadline) >= 2_000 && round < 4 {
        round = round.saturating_add(1);
        let (fresh_h, fresh_tip) = hf88_best_tip(settings, &mut report, 1_600);
        if fresh_h > best_h || (fresh_h == best_h && !fresh_tip.trim().is_empty()) {
            best_h = fresh_h;
            best_tip = fresh_tip;
        }

        // HF102 order matters. If direct official seeds say we are behind and the
        // short missing-suffix append does not move, go to official canonical P2P
        // repair immediately. Generic peer probing can waste the whole budget on
        // stale registry nodes; it now runs after official repair attempts.
        let steps: &[(&str, u64)] = &[
            ("official-suffix", 18_000),
            ("http-tail", 20_000),
            ("official-p2p-snapshot", 75_000),
            ("http-full-snapshot", 90_000),
            ("quick-peers", 18_000),
        ];

        for (step, max_ms) in steps {
            let left = remaining_ms(deadline);
            if left < 2_000 {
                break;
            }

            let current = load_chain_for_hf90_catchup(settings)?;
            local_h = current.height();
            local_tip = current.tip_hash().to_string();
            report.height = local_h;
            report.tip_hash = local_tip.clone();

            if best_h > 0 && local_is_at_or_past_tip(&current, best_h, &best_tip) {
                mark_fresh_tip_trusted(settings, &current);
                hf129_mark_canonical_tip_proven(&mut report);
                return finish_report(settings, report);
            }

            let gap = best_h.saturating_sub(local_h);
            let wrong_tip =
                best_h == local_h && !best_tip.trim().is_empty() && best_tip != local_tip;
            let snapshot_step = *step == "official-p2p-snapshot" || *step == "http-full-snapshot";
            let full_http_step = *step == "http-full-snapshot";
            let should_run = match *step {
                "official-suffix" | "http-tail" => true,
                "official-p2p-snapshot" => {
                    allow_snapshot || gap > 0 || wrong_tip || no_progress_steps >= 1
                }
                "http-full-snapshot" => {
                    allow_snapshot || gap > 0 || wrong_tip || no_progress_steps >= 2 || round >= 2
                }
                "quick-peers" => allow_snapshot || gap > 0 || wrong_tip || no_progress_steps >= 1,
                _ => false,
            };
            if !should_run {
                continue;
            }
            let minimum_budget = if full_http_step {
                16_000
            } else if snapshot_step {
                12_000
            } else {
                3_000
            };
            if left < minimum_budget {
                continue;
            }
            // The remaining overall deadline is authoritative. Raising a short
            // remainder to a per-step minimum recreated the very post-deadline
            // 1-3 minute rescue work HF129 is removing.
            let step_budget = left.min(*max_ms);
            let before_step_h = local_h;
            let before_step_tip = local_tip.clone();
            let res = match *step {
                "official-suffix" => sync_official_suffix(settings, step_budget),
                "http-tail" => sync_official_http_tail(settings, step_budget),
                "official-p2p-snapshot" => sync_official_snapshot(settings, step_budget),
                "http-full-snapshot" => sync_official_http_snapshot(settings, step_budget),
                "quick-peers" => sync_quick(
                    settings,
                    settings.p2p.max_outbound_peers.max(8).min(20),
                    step_budget,
                ),
                _ => Ok(P2PSyncReport::default()),
            };
            match res {
                Ok(r) => merge_sync_reports(&mut report, r),
                Err(_) => report.peer_errors = report.peer_errors.saturating_add(1),
            }

            let after = load_chain_for_hf90_catchup(settings)?;
            report.height = after.height();
            report.tip_hash = after.tip_hash().to_string();
            if after.height() > before_step_h || after.tip_hash().to_string() != before_step_tip {
                no_progress_steps = 0;
            } else {
                no_progress_steps = no_progress_steps.saturating_add(1);
            }

            let refresh_budget = remaining_ms(deadline).min(1_100);
            if refresh_budget >= 150 {
                let (new_best_h, new_best_tip) =
                    hf88_best_tip(settings, &mut report, refresh_budget);
                if new_best_h > best_h
                    || (new_best_h == best_h && !new_best_tip.trim().is_empty())
                {
                    best_h = new_best_h;
                    best_tip = new_best_tip;
                }
            }
            if best_h > 0 && local_is_at_or_past_tip(&after, best_h, &best_tip) {
                mark_fresh_tip_trusted(settings, &after);
                hf129_mark_canonical_tip_proven(&mut report);
                return finish_report(settings, report);
            }
        }

        // Do not stop after a cosmetic no-progress loop unless we have already
        // given both official P2P and HTTP canonical paths a chance in at least
        // two rounds. The common stuck case is exactly "gap visible, no progress".
        if no_progress_steps >= 12 && round >= 3 {
            break;
        }
    }

    // HF107 final rescue: if the UI can still see a higher official/direct tip
    // after the ladder, force one canonical HTTP snapshot attempt before the
    // pulse ends. This addresses the observed stuck state where the visible gap
    // remained frozen at 4-7 blocks until a restart. The snapshot is off-UI and
    // must pass consensus/checkpoint validation before save_chain() is called.
    if best_h > 0 {
        if let Ok(cur) = load_chain_for_hf90_catchup(settings) {
            if !local_is_at_or_past_tip(&cur, best_h, &best_tip) {
                let left = remaining_ms(deadline);
                if left >= 2_000 {
                    if let Ok(full) = sync_official_http_snapshot(settings, left) {
                        merge_sync_reports(&mut report, full);
                    }
                }
            }
        }
    }

    if let Ok(final_chain) = load_chain_for_hf90_catchup(settings) {
        report.height = final_chain.height();
        report.tip_hash = final_chain.tip_hash().to_string();
        let canonical_green =
            hf104_canonical_greenlight(settings, &mut report, &final_chain, 1_200);
        if hf129_note_canonical_greenlight(&mut report, canonical_green)
            || (settings.network.name != "mainnet"
                && best_h > 0
                && local_is_at_or_past_tip(&final_chain, best_h, &best_tip))
        {
            mark_fresh_tip_trusted(settings, &final_chain);
        }
    }
    finish_report(settings, report)
}

pub fn hf88_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(150_000), false)
}

pub fn hf88_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(180_000), true)
}

pub fn hf88_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(180_000), true)
}

pub fn hf90_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf90_catchup_ladder(
        settings,
        total_timeout_ms.clamp(60_000, 600_000),
        true,
        "auto",
    )
}

pub fn hf90_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf90_catchup_ladder(
        settings,
        total_timeout_ms.clamp(90_000, 720_000),
        true,
        "manual",
    )
}

pub fn hf90_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    hf90_catchup_ladder(
        settings,
        total_timeout_ms.clamp(60_000, 600_000),
        true,
        "mining",
    )
}

pub fn hf110_deep_official_repair(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let deadline = Instant::now()
        + Duration::from_millis(total_timeout_ms.clamp(180_000, 900_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);

    // HF111/v1.7.1: this is the heavy but safe repair used by manual Repair and
    // GUI auto-heal when a miner has been behind for too long. It is official-
    // source only: first the fresh HTTP tail/snapshot, then official direct full
    // chain, then suffix/overlap repair. All adopted blocks are consensus-
    // validated by the lower-level repair functions before chain.json is saved.
    let _guard = match hf129_acquire_catchup_writer(true, deadline)? {
        Some(guard) => guard,
        None => {
            hf129_mark_writer_busy(&mut report, "manual-deep-repair");
            return finish_report(settings, report);
        }
    };
    if remaining_ms(deadline) < 5_000 {
        drop(_guard);
        hf129_mark_writer_busy(&mut report, "manual-deep-repair-deadline");
        return finish_report(settings, report);
    }
    let before = load_chain_for_hf90_catchup(settings)?;
    hf129_mark_writer_acquired(&mut report, "manual-deep-repair", &before);
    if remaining_ms(deadline) >= 2_000 {
        hf131_sync_direct_before_snapshots(settings, &mut report, remaining_ms(deadline).min(18_000));
    }

    for round in 0..3usize {
        if remaining_ms(deadline) < 5_000 {
            break;
        }
        let local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();

        let green = hf104_canonical_greenlight(settings, &mut report, &local, 1_500);
        if hf129_note_canonical_greenlight(&mut report, green) {
            return finish_report(settings, report);
        }

        let before_h = local.height();
        let before_tip = local.tip_hash().to_string();

        let steps: &[(&str, u64)] = &[
            ("official-http-tail-deep", 120_000),
            ("official-http-full-snapshot-deep", 220_000),
            ("official-direct-full-chain-deep", 260_000),
            ("official-suffix-deep", 90_000),
            ("official-p2p-overlap-snapshot-deep", 180_000),
        ];

        for (step, max_ms) in steps {
            let left = remaining_ms(deadline);
            if left < 5_000 {
                break;
            }
            let budget = left.min(*max_ms);
            let res = match *step {
                "official-http-tail-deep" => sync_official_http_tail(settings, budget),
                "official-http-full-snapshot-deep" => sync_official_http_snapshot(settings, budget),
                "official-direct-full-chain-deep" => {
                    sync_official_direct_full_chain(settings, budget)
                }
                "official-suffix-deep" => sync_official_suffix(settings, budget),
                "official-p2p-overlap-snapshot-deep" => sync_official_snapshot(settings, budget),
                _ => Ok(P2PSyncReport::default()),
            };
            match res {
                Ok(r) => merge_sync_reports(&mut report, r),
                Err(_) => report.peer_errors = report.peer_errors.saturating_add(1),
            }

            let after = load_chain_for_hf90_catchup(settings)?;
            validate_chain_consensus_checkpoints(settings, &after.blocks)?;
            report.height = after.height();
            report.tip_hash = after.tip_hash().to_string();
            let green = hf104_canonical_greenlight(settings, &mut report, &after, 1_500);
            if hf129_note_canonical_greenlight(&mut report, green) {
                return finish_report(settings, report);
            }
        }

        let after_round = load_chain_for_hf90_catchup(settings)?;
        if after_round.height() == before_h
            && after_round.tip_hash().to_string() == before_tip
            && round >= 1
        {
            break;
        }
    }

    finish_report(settings, report)
}

/// HF107/v1.6.9: strong single-flight catch-up for the real mainnet gap case.
/// The GUI can show local #N and direct/official #N+k; this worker must keep
/// trying long enough to actually move the validated chain, not merely rediscover
/// the same gap on each pulse. It remains off the UI thread and all received
/// blocks/snapshots are consensus-validated before saving.
fn hf90_catchup_ladder(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_full_snapshot: bool,
    reason: &str,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(30_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let priority = reason != "auto";
    let _guard = match hf129_acquire_catchup_writer(priority, deadline)? {
        Some(guard) => guard,
        None => {
            hf129_mark_writer_busy(&mut report, reason);
            return finish_report(settings, report);
        }
    };
    if priority && remaining_ms(deadline) < 3_000 {
        drop(_guard);
        hf129_mark_writer_busy(&mut report, "priority-deadline");
        return finish_report(settings, report);
    }
    let before = load_chain_for_hf90_catchup(settings)?;
    hf129_mark_writer_acquired(&mut report, reason, &before);
    if remaining_ms(deadline) >= 2_000 {
        hf131_sync_direct_before_snapshots(settings, &mut report, remaining_ms(deadline).min(18_000));
    }
    if report.blocks_connected > 0 || report.chains_adopted > 0 {
        let current = load_chain_for_hf90_catchup(settings)?;
        if hf104_canonical_greenlight(settings, &mut report, &current, remaining_ms(deadline).min(1_200))
            && hf130_proof_matches_final(&report, current.height(), &current.tip_hash().to_string(),
                pending_block_relay_waiting(settings, &current)) {
            return finish_report(settings, report);
        }
    }
    let mut no_progress_rounds = 0usize;

    for round in 0..4usize {
        if remaining_ms(deadline) < 3_000 {
            break;
        }
        let current = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &current.blocks)?;
        report.height = current.height();
        report.tip_hash = current.tip_hash().to_string();

        let probe_budget = remaining_ms(deadline).min(2_800);
        if probe_budget < 150 {
            break;
        }
        let (mut best_h, mut best_tip) =
            hf88_best_tip(settings, &mut report, probe_budget);
        let official_budget = remaining_ms(deadline).min(2_800);
        if official_budget >= 150 {
            if let Ok((contacted, h, tip)) = best_official_peer_tip(settings, official_budget) {
                report.peers_contacted = report.peers_contacted.saturating_add(contacted);
                hf129_observe_tip(&mut report, h, &tip);
                if h > best_h || (h == best_h && !tip.trim().is_empty()) {
                    best_h = h;
                    best_tip = tip;
                }
            }
        }
        hf129_observe_tip(&mut report, best_h, &best_tip);

        let wrong_tip = best_h == current.height()
            && !best_tip.trim().is_empty()
            && best_tip != current.tip_hash().to_string();

        // HF126/v1.8.2: resolve a locally mined, unacknowledged equal-height
        // sibling before entering the expensive generic ladder. The dedicated
        // path requires multi-source evidence, full replay validation and the
        // explicit equal-work Fast Chain Engine commit policy.
        if wrong_tip && pending_block_relay_waiting(settings, &current) {
            let budget = remaining_ms(deadline).min(35_000);
            if let Ok(reanchor) = sync_official_http_tail_reanchor_hf114(settings, budget) {
                merge_sync_reports(&mut report, reanchor);
                let repaired = load_chain_for_hf90_catchup(settings)?;
                validate_chain_consensus_checkpoints(settings, &repaired.blocks)?;
                if repaired.tip_hash() != current.tip_hash() {
                    report.height = repaired.height();
                    report.tip_hash = repaired.tip_hash().to_string();
                    mark_fresh_tip_trusted(settings, &repaired);
                    hf129_mark_canonical_tip_proven(&mut report);
                    return finish_report(settings, report);
                }
            }
        }

        if best_h > 0
            && !best_tip.trim().is_empty()
            && local_is_at_or_past_tip(&current, best_h, &best_tip)
            && !wrong_tip
            && !pending_block_relay_waiting(settings, &current)
        {
            mark_fresh_tip_trusted(settings, &current);
            if best_h == current.height() && best_tip == current.tip_hash().to_string() {
                hf130_mark_exact_tip_proven(&mut report, &current, Hf130CanonicalEvidence::OfficialTip);
            }
            return finish_report(settings, report);
        }
        if best_h == 0 {
            let light_budget = remaining_ms(deadline).min(900);
            if light_budget >= 150
                && fresh_tip_still_matches_light_network(settings, &current, light_budget)
            {
                mark_fresh_tip_trusted(settings, &current);
                hf129_mark_canonical_tip_proven(&mut report);
                return finish_report(settings, report);
            }
        }

        let gap = best_h.saturating_sub(current.height());
        let need_strong = gap > 0 || wrong_tip || no_progress_rounds > 0 || round > 0;
        let need_emergency = gap >= 4 || wrong_tip || no_progress_rounds > 0;
        let steps: &[(&str, u64)] = if need_emergency {
            &[
                ("official-http-tail-strong", 45_000),
                ("official-http-full-snapshot", 130_000),
                ("official-direct-full-chain", 160_000),
                ("official-suffix-strong", 35_000),
                ("official-p2p-overlap-snapshot", 120_000),
                ("direct-force-anchor", 90_000),
                ("adaptive-direct-peers", 45_000),
            ]
        } else if need_strong {
            &[
                ("official-suffix-strong", 35_000),
                ("official-http-tail-strong", 32_000),
                ("official-direct-full-chain", 80_000),
                ("official-p2p-overlap-snapshot", 90_000),
                ("adaptive-direct-peers", 35_000),
            ]
        } else {
            &[
                ("official-suffix-strong", 25_000),
                ("official-http-tail-strong", 24_000),
                ("adaptive-direct-peers", 24_000),
            ]
        };

        let before_round = load_chain_for_hf90_catchup(settings)?;
        let before_h = before_round.height();
        let before_tip = before_round.tip_hash().to_string();

        for (step, max_ms) in steps {
            let left = remaining_ms(deadline);
            if left < 2_500 {
                break;
            }
            let step_budget = left.min(*max_ms);
            let before_step = load_chain_for_hf90_catchup(settings)?;
            let before_step_h = before_step.height();
            let before_step_tip = before_step.tip_hash().to_string();

            let res = match *step {
                "official-suffix-strong" => sync_official_suffix(settings, step_budget),
                "official-http-tail-strong" => sync_official_http_tail(settings, step_budget),
                "official-p2p-overlap-snapshot" => sync_official_snapshot(settings, step_budget),
                "official-direct-full-chain" => {
                    sync_official_direct_full_chain(settings, step_budget)
                }
                "direct-force-anchor" => {
                    sync_force_anchor_to_best_direct(settings, best_h, step_budget)
                }
                "adaptive-direct-peers" => sync_quick(
                    settings,
                    settings.p2p.max_outbound_peers.max(12).min(24),
                    step_budget,
                ),
                "official-http-full-snapshot" => {
                    if allow_full_snapshot {
                        sync_official_http_snapshot(settings, step_budget)
                    } else {
                        Ok(P2PSyncReport::default())
                    }
                }
                _ => Ok(P2PSyncReport::default()),
            };
            match res {
                Ok(r) => merge_sync_reports(&mut report, r),
                Err(_) => report.peer_errors = report.peer_errors.saturating_add(1),
            }

            let after_step = load_chain_for_hf90_catchup(settings)?;
            report.height = after_step.height();
            report.tip_hash = after_step.tip_hash().to_string();
            if after_step.height() > before_step_h
                || after_step.tip_hash().to_string() != before_step_tip
            {
                no_progress_rounds = 0;
                let fresh_budget = remaining_ms(deadline).min(1_800);
                let (fresh_h, fresh_tip) = if fresh_budget >= 150 {
                    hf88_best_tip(settings, &mut report, fresh_budget)
                } else {
                    (0, String::new())
                };
                let target_h = fresh_h.max(best_h);
                let target_tip = if fresh_h >= best_h && !fresh_tip.trim().is_empty() {
                    fresh_tip
                } else {
                    best_tip.clone()
                };
                if target_h == 0 || local_is_at_or_past_tip(&after_step, target_h, &target_tip) {
                    mark_fresh_tip_trusted(settings, &after_step);
                    hf129_mark_canonical_tip_proven(&mut report);
                    return finish_report(settings, report);
                }
                // A moving remote tip after a successful suffix is normal.
                // Continue the next round from the committed height before
                // considering expensive snapshot/fork repair again.
                break;
            }
        }

        let after_round = load_chain_for_hf90_catchup(settings)?;
        report.height = after_round.height();
        report.tip_hash = after_round.tip_hash().to_string();
        if after_round.height() == before_h && after_round.tip_hash().to_string() == before_tip {
            no_progress_rounds = no_progress_rounds.saturating_add(1);
        } else {
            no_progress_rounds = 0;
        }
        if no_progress_rounds >= 3 {
            break;
        }
    }

    let probe_budget = remaining_ms(deadline).min(1_200);
    let (_seen, visible_h, visible_tip) = if probe_budget >= 150 {
        hf97_visible_tip(settings, probe_budget)
    } else {
        (0, report.best_peer_height, report.best_peer_tip_hash.clone())
    };
    if visible_h > 0 {
        if let Ok(cur) = load_chain_for_hf90_catchup(settings) {
            if !local_is_at_or_past_tip(&cur, visible_h, &visible_tip) {
                let left = remaining_ms(deadline);
                if left >= 2_500 {
                    if let Ok(full) = sync_official_http_snapshot(settings, left) {
                        merge_sync_reports(&mut report, full);
                    }
                }
                if let Ok(cur_after_http) = load_chain_for_hf90_catchup(settings) {
                    let left = remaining_ms(deadline);
                    if left >= 2_500
                        && !local_is_at_or_past_tip(&cur_after_http, visible_h, &visible_tip)
                    {
                        if let Ok(full_direct) = sync_official_direct_full_chain(settings, left) {
                            merge_sync_reports(&mut report, full_direct);
                        }
                    }
                }
            }
        }
    }

    let final_chain = load_chain_for_hf90_catchup(settings)?;
    report.height = final_chain.height();
    report.tip_hash = final_chain.tip_hash().to_string();
    hf129_observe_tip(&mut report, visible_h, &visible_tip);
    if visible_h > final_chain.height()
        && !local_is_at_or_past_tip(&final_chain, visible_h, &visible_tip)
    {
        mark_hf97_uncatchable_tip(settings, &final_chain, visible_h);
    }
    let final_light_budget = remaining_ms(deadline).min(1_200);
    if final_light_budget >= 150
        && fresh_tip_still_matches_light_network(settings, &final_chain, final_light_budget)
    {
        mark_fresh_tip_trusted(settings, &final_chain);
        hf129_mark_canonical_tip_proven(&mut report);
    }
    finish_report(settings, report)
}

fn hf80_fast_official_catchup(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &mut ChainState,
    total_timeout_ms: u64,
) -> Result<()> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    if !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let mut last_height = local.height();
    let mut last_tip = local.tip_hash().to_string();

    for _ in 0..3 {
        if Instant::now() >= deadline {
            break;
        }
        if official_tip_greenlight(settings, report, local, 360) {
            return Ok(());
        }

        let (_, official_height, official_tip) = official_tip_summary(settings, report, 360);
        let same_height_wrong_tip = official_height == local.height()
            && !official_tip.trim().is_empty()
            && official_tip != local.tip_hash().to_string();
        let official_ahead = official_height > local.height();
        if !official_ahead && !same_height_wrong_tip {
            return Ok(());
        }

        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        if left_ms < 900 {
            break;
        }
        if let Ok(suffix) = sync_official_suffix(settings, left_ms.min(4_500).max(1_500)) {
            merge_sync_reports(report, suffix);
            *local = load_chain_for_hf90_catchup(settings)?;
            validate_chain_consensus_checkpoints(settings, &local.blocks)?;
            if official_tip_greenlight(settings, report, local, 300) {
                return Ok(());
            }
        }

        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        if left_ms < 1_200 {
            break;
        }
        if let Ok(tail) = sync_official_http_tail(settings, left_ms.min(5_500).max(2_000)) {
            merge_sync_reports(report, tail);
            *local = load_chain_for_hf90_catchup(settings)?;
            validate_chain_consensus_checkpoints(settings, &local.blocks)?;
            if official_tip_greenlight(settings, report, local, 300) {
                return Ok(());
            }
        }

        let now_tip = local.tip_hash().to_string();
        if local.height() == last_height && now_tip == last_tip {
            break;
        }
        last_height = local.height();
        last_tip = now_tip;
    }
    Ok(())
}

/// HF60/v1.5.2 fixed3: explicit snapshot-style catch-up from the official
/// bootstrap seed path. This avoids the old loop where normal peer sync kept
/// trying stale/local suffixes and never replaced the post-checkpoint fork.
/// It asks only official bootnodes for the chain suffix from the hard anchor,
/// never stale registry rows, and it always returns within the supplied budget.
fn best_common_height_from_headers(local: &ChainState, headers: &[IndexedHeader]) -> Option<u32> {
    for header in headers.iter().rev() {
        let idx = header.height as usize;
        if idx < local.blocks.len() && local.blocks[idx].block_hash().to_string() == header.hash {
            return Some(header.height);
        }
    }
    None
}

/// HF68/v1.5.2 fixed2: very small official-suffix sync. This is deliberately
/// simpler than the full checkpoint repair path: it only asks official seeds for
/// the missing suffix starting at local_height + 1 and directly connects those
/// blocks if they extend the current tip. It is used before mining/manual sync
/// so the common one-block-behind case cannot fall into long repair loops.
pub fn sync_official_suffix(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "official-suffix",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(1_500));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for addr in official_snapshot_peers(settings).into_iter().take(3) {
        if Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }

        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let Ok(mut stream) = connect_peer(&addr, left.min(Duration::from_millis(3_000))) else {
            report.peer_errors = report.peer_errors.saturating_add(1);
            continue;
        };
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let io_left = deadline.saturating_duration_since(Instant::now());
        if io_left.is_zero() {
            break;
        }
        let _ = stream.set_read_timeout(Some(io_left.min(Duration::from_millis(3_200))));
        let _ = stream.set_write_timeout(Some(io_left.min(Duration::from_millis(3_500))));
        let mut reader = BufReader::new(stream.try_clone()?);

        let local_before = load_chain_for_hf90_catchup(settings)?;
        let before_height = local_before.height();
        let before_tip = local_before.tip_hash().to_string();
        let from_height = before_height.saturating_add(1);

        let _ = send_version(&mut stream, settings, &local_before);
        let _ = send_wire(&mut stream, &WireMessage::GetHeaders { from_height });
        let _ = send_wire(&mut stream, &WireMessage::GetChain { from_height });

        let peer_deadline = Instant::now() + left.min(Duration::from_millis(16_000));
        let peer_deadline = hf130_effective_deadline(peer_deadline);
        let _hf130_io_scope = Hf130IoScope::deadline(peer_deadline);
        let mut peer_tip = String::new();
        let mut last_request_at = Instant::now();
        let mut last_progress_at = Instant::now();
        let mut last_seen_height = before_height;
        while Instant::now() < deadline && Instant::now() < peer_deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(WireMessage::Version {
                    height, tip_hash, ..
                })
                | Ok(WireMessage::Inv {
                    height, tip_hash, ..
                }) => {
                    hf129_observe_tip(&mut report, height, "");
                    if height >= report.best_peer_height {
                        peer_tip = tip_hash.clone();
                    }
                    if hf126_is_same_height_competing_tip(
                        before_height,
                        &before_tip,
                        height,
                        &tip_hash,
                    ) {
                        let _ = send_adaptive_chain_requests(
                            &mut stream,
                            &local_before,
                            height,
                            &tip_hash,
                        );
                        last_request_at = Instant::now();
                    }
                }
                Ok(WireMessage::Headers { headers }) => {
                    if let Some(last) = headers.last() {
                        hf129_observe_tip(&mut report, last.height, &last.hash);
                        peer_tip = last.hash.clone();
                    }
                }
                Ok(WireMessage::Chain {
                    start_height,
                    blocks,
                }) => {
                    if blocks.is_empty() {
                        break;
                    }
                    if blocks.len() > settings.p2p.max_blocks_per_message {
                        bail!("too many blocks in chain message");
                    }
                    let mut local = load_chain_for_hf90_catchup(settings)?;
                    let local_height_before = local.height();
                    let changed = if start_height == local.height().saturating_add(1) {
                        let mut candidate = local.clone();
                        let mut ok = true;
                        for block in blocks {
                            hf132_check_sync_budget()?;
                            if let Err(_) = candidate.connect_block(block, settings) {
                                ok = false;
                                break;
                            }
                        }
                        if ok && candidate.height() > local.height() {
                            local = candidate;
                            true
                        } else {
                            false
                        }
                    } else {
                        try_adopt_overlapping_blocks(
                            &mut local,
                            start_height,
                            blocks,
                            settings,
                            settings.network.name != "mainnet",
                        )
                        .unwrap_or(false)
                    };
                    if changed {
                        save_chain(settings, &local)?;
                        if local.height() > last_seen_height {
                            last_seen_height = local.height();
                            last_progress_at = Instant::now();
                        }
                        report.chains_adopted = report.chains_adopted.saturating_add(1);
                        report.blocks_connected = report.blocks_connected.saturating_add(
                            local.height().saturating_sub(local_height_before) as usize,
                        );
                        report.height = local.height();
                        report.tip_hash = local.tip_hash().to_string();
                        let target = report.best_peer_height;
                        if target == 0
                            || local.height() >= target
                            || !peer_tip.trim().is_empty()
                                && local.tip_hash().to_string() == peer_tip
                        {
                            return finish_report(settings, report);
                        }
                        let next_from = local.height().saturating_add(1);
                        let _ = send_wire(
                            &mut stream,
                            &WireMessage::GetChain {
                                from_height: next_from,
                            },
                        );
                        last_request_at = Instant::now();
                    } else {
                        if last_progress_at.elapsed() >= Duration::from_millis(2_500) {
                            break;
                        }
                    }
                }
                Ok(_) => {}
                Err(err) if is_timeout(&err) => {
                    let cur = load_chain_for_hf90_catchup(settings)?;
                    if cur.height() > last_seen_height {
                        last_seen_height = cur.height();
                        last_progress_at = Instant::now();
                    }
                    if last_request_at.elapsed() >= Duration::from_millis(1_500) {
                        let next_from = cur.height().saturating_add(1);
                        let _ = send_wire(
                            &mut stream,
                            &WireMessage::GetHeaders {
                                from_height: next_from,
                            },
                        );
                        let _ = send_wire(
                            &mut stream,
                            &WireMessage::GetChain {
                                from_height: next_from,
                            },
                        );
                        last_request_at = Instant::now();
                    }
                    if last_progress_at.elapsed() >= Duration::from_millis(8_000) {
                        break;
                    }
                }
                Err(err) => {
                    if !is_benign_io(&err) {
                        report.peer_errors = report.peer_errors.saturating_add(1);
                    }
                    break;
                }
            }
        }

        let after = load_chain_for_hf90_catchup(settings)?;
        report.height = after.height();
        report.tip_hash = after.tip_hash().to_string();
        if after.height() > before_height || after.tip_hash().to_string() != before_tip {
            break;
        }
    }

    finish_report(settings, report)
}

/// HF60/v1.5.2 fixed3 + HF65: explicit snapshot-style catch-up from the
/// official bootstrap seed path. HF65 makes this path robust for the exact
/// stuck case where probes see seeds ahead but the local chain never advances:
/// it first tries the tiny missing suffix, then asks for recent headers to find
/// the real common ancestor, and only then falls back to checkpoint repair.
/// This avoids huge checkpoint-chain messages for +1..+20 lag and prevents the
/// miner from declaring safety OK while official seeds are ahead.

fn official_http_snapshot_urls(settings: &Settings) -> Vec<String> {
    match settings.network.name.as_str() {
        "mainnet" => vec![
            "https://download.qubit-coin.io/mainnet/snapshots/chain.json".to_string(),
            "https://download.qubit-coin.io/mainnet/canonical-chain.json".to_string(),
        ],
        "testnet" => vec![
            "https://download.qubit-coin.io/testnet/snapshots/chain.json".to_string(),
            "https://download.qubit-coin.io/testnet/canonical-chain.json".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn official_http_tip_urls(settings: &Settings) -> Vec<String> {
    match settings.network.name.as_str() {
        "mainnet" => vec!["https://download.qubit-coin.io/mainnet/snapshots/tip.json".to_string()],
        "testnet" => vec!["https://download.qubit-coin.io/testnet/snapshots/tip.json".to_string()],
        _ => Vec::new(),
    }
}

fn official_http_tail_urls(settings: &Settings) -> Vec<String> {
    match settings.network.name.as_str() {
        "mainnet" => vec![
            "https://download.qubit-coin.io/mainnet/snapshots/tail-64.json".to_string(),
            "https://download.qubit-coin.io/mainnet/snapshots/tail-256.json".to_string(),
            "https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json".to_string(),
            "https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json".to_string(),
            "https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json".to_string(),
        ],
        "testnet" => vec![
            "https://download.qubit-coin.io/testnet/snapshots/tail-64.json".to_string(),
            "https://download.qubit-coin.io/testnet/snapshots/tail-256.json".to_string(),
            "https://download.qubit-coin.io/testnet/snapshots/tail-1024.json".to_string(),
            "https://download.qubit-coin.io/testnet/snapshots/tail-2048.json".to_string(),
            "https://download.qubit-coin.io/testnet/snapshots/tail-4096.json".to_string(),
        ],
        _ => Vec::new(),
    }
}

fn official_tail_window_from_url(url: &str) -> u32 {
    for part in url.split('/') {
        if let Some(raw) = part
            .strip_prefix("tail-")
            .and_then(|s| s.strip_suffix(".json"))
        {
            if let Ok(n) = raw.parse::<u32>() {
                return n;
            }
        }
    }
    u32::MAX
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OfficialTipSnapshot {
    network: String,
    height: u32,
    tip_hash: String,
    chain_sha256: Option<String>,
    published_at_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OfficialTailSnapshot {
    network: String,
    start_height: u32,
    tip_height: u32,
    tip_hash: String,
    blocks: Vec<Block>,
}

fn hf129_official_tip_timestamp_is_fresh(published_at: Option<u64>, now: u64) -> bool {
    let Some(published_at) = published_at else {
        return false;
    };
    published_at <= now.saturating_add(HF129_OFFICIAL_TIP_FUTURE_SKEW_SECS)
        && now.saturating_sub(published_at) <= HF129_OFFICIAL_TIP_MAX_AGE_SECS
}

fn curl_binary() -> &'static str {
    if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    }
}

#[cfg(target_os = "windows")]
fn hide_snapshot_command_window(command: &mut Command) {
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(target_os = "windows"))]
fn hide_snapshot_command_window(_command: &mut Command) {}

fn download_http_snapshot_to(path: &PathBuf, url: &str, timeout_ms: u64) -> Result<()> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let bounded_ms = hf130_io_remaining(deadline)?.as_millis().max(1) as u64;
    let timeout_secs = format!("{:.3}", bounded_ms as f64 / 1_000.0);
    let connect_secs = format!("{:.3}", bounded_ms.min(5_000) as f64 / 1_000.0);
    let mut command = Command::new(curl_binary());
    hide_snapshot_command_window(&mut command);
    let mut child = command
        .arg("-fsSL")
        .arg("--connect-timeout")
        .arg(connect_secs)
        .arg("--max-time")
        .arg(timeout_secs)
        .arg("--output")
        .arg(path)
        .arg(url)
        .spawn()
        .with_context(|| format!("failed to start curl for {url}"))?;
    let status = loop {
        // Reap before returning: a cancelled download must not keep writing to
        // a path that the next canonical writer will use or remove.
        if let Err(err) = hf130_io_remaining(deadline) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err.into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => thread::sleep(Duration::from_millis(20)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(err.into());
            }
        }
    };
    if !status.success() {
        bail!("snapshot download failed from {url} with status {status}");
    }
    Ok(())
}

fn download_http_text(url: &str, timeout_ms: u64, label: &str) -> Result<String> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let mut tmp = std::env::temp_dir();
    let safe = label
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect::<String>();
    let mut rng = rand::thread_rng();
    let nonce = rng.next_u64();
    tmp.push(format!(
        "qub-{}-{}-{}.json",
        safe,
        std::process::id(),
        nonce
    ));
    let result = (|| -> Result<String> {
        download_http_snapshot_to(&tmp, url, timeout_ms)?;
        fs::read_to_string(&tmp)
            .with_context(|| format!("failed reading downloaded http body from {url}"))
    })();
    let _ = fs::remove_file(&tmp);
    result
}

fn hf130_snapshot_identity_matches(tip: &OfficialTipSnapshot, height: u32, hash: &str) -> bool {
    tip.height == height && !hash.trim().is_empty() && tip.tip_hash == hash
}

/// Existing URLs remain compatible with HF128. A client may cross a publisher
/// pointer switch between requests, so bind the payload to the sampled tip and
/// retry twice within the SAME operation budget on a generation mismatch.
fn hf130_download_bound_snapshot<T>(
    settings: &Settings,
    url: &str,
    timeout_ms: u64,
    decode: impl Fn(&str, &OfficialTipSnapshot) -> Result<T>,
) -> Result<T> {
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let _scope = Hf130IoScope::deadline(deadline);
    let tip_url = official_http_tip_urls(settings).into_iter().next()
        .context("no official snapshot tip URL")?;
    let mut last_error = None;
    for _ in 0..2 {
        hf130_io_remaining(deadline)?;
        let attempt = (|| -> Result<T> {
            let metadata = download_http_text(&tip_url, remaining_ms(deadline).min(1_500), "generation-tip")?;
            let tip: OfficialTipSnapshot = serde_json::from_str(&metadata)?;
            if tip.network != settings.network.name || tip.tip_hash.trim().is_empty() {
                bail!("invalid snapshot generation metadata");
            }
            // Bootstrap data may be old; only official_http_tip treats the
            // publication timestamp as live mining evidence.
            let raw = download_http_text(url, remaining_ms(deadline), "bound-snapshot")?;
            hf130_io_remaining(deadline)?;
            decode(&raw, &tip)
        })();
        match attempt {
            Ok(value) => return Ok(value),
            Err(err) => last_error = Some(err),
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("snapshot generation unavailable")))
}

fn hf130_decode_tail_generation(raw: &str, tip: &OfficialTipSnapshot) -> Result<OfficialTailSnapshot> {
    let tail: OfficialTailSnapshot = serde_json::from_str(raw)?;
    if tail.network != tip.network
        || !hf130_snapshot_identity_matches(tip, tail.tip_height, &tail.tip_hash)
    {
        bail!("snapshot generation changed during tail download");
    }
    Ok(tail)
}

fn hf130_download_tail(settings: &Settings, url: &str, timeout_ms: u64) -> Result<OfficialTailSnapshot> {
    hf130_download_bound_snapshot(settings, url, timeout_ms, hf130_decode_tail_generation)
}

fn hf130_download_full(settings: &Settings, url: &str, timeout_ms: u64) -> Result<PersistedChainState> {
    hf130_download_bound_snapshot(settings, url, timeout_ms, |raw, tip| {
        let expected = tip.chain_sha256.as_deref().context("snapshot generation lacks checksum")?;
        let actual = hex::encode(Sha256::digest(raw.as_bytes()));
        if !actual.eq_ignore_ascii_case(expected) {
            bail!("snapshot generation changed during full download");
        }
        let persisted: PersistedChainState = serde_json::from_str(raw)?;
        let height = persisted.blocks.len().saturating_sub(1) as u32;
        let hash = persisted.blocks.last().map(|block| block.block_hash().to_string()).unwrap_or_default();
        if persisted.network != tip.network || !hf130_snapshot_identity_matches(tip, height, &hash) {
            bail!("snapshot generation payload identity mismatch");
        }
        Ok(persisted)
    })
}

pub fn official_http_tip(settings: &Settings, timeout_ms: u64) -> Result<Option<(u32, String)>> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(100));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for url in official_http_tip_urls(settings) {
        let left = remaining_ms(deadline).min(6_000);
        if left < 100 {
            break;
        }
        let raw = match download_http_text(&url, left, "official-tip") {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let tip: OfficialTipSnapshot = serde_json::from_str(&raw)
            .with_context(|| format!("invalid official tip json from {url}"))?;
        if tip.network != settings.network.name {
            continue;
        }
        // HF129: tip.json is liveness evidence, not merely bootstrap data. A
        // frozen publisher must never remain an HTTP-only mining green-light or
        // suppress newer-tip discovery indefinitely. Tails/full snapshots stay
        // available as consensus-validated bootstrap transports elsewhere.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        if !hf129_official_tip_timestamp_is_fresh(tip.published_at_unix, now) {
            continue;
        }
        if tip.height > 0 && !tip.tip_hash.trim().is_empty() {
            return Ok(Some((tip.height, tip.tip_hash)));
        }
    }
    Ok(None)
}

fn apply_official_snapshot_fast_path(
    settings: &Settings,
    local_before: &ChainState,
    persisted: PersistedChainState,
) -> Result<(ChainState, usize)> {
    if persisted.network != settings.network.name {
        bail!("network mismatch");
    }
    if persisted.blocks.is_empty() {
        bail!("empty snapshot");
    }
    if persisted.blocks.first() != Some(&genesis_block(settings)?) {
        bail!("genesis mismatch");
    }

    let local_height = local_before.height();
    let snapshot_height = persisted.blocks.len().saturating_sub(1) as u32;
    if snapshot_height <= local_height {
        return ChainState::from_persisted(persisted, settings).map(|candidate| (candidate, 0));
    }

    if persisted
        .blocks
        .get(local_height as usize)
        .map(|b| b.block_hash())
        == Some(local_before.tip_hash())
    {
        let mut repaired = local_before.clone();
        Arc::make_mut(&mut repaired.mempool).clear();
        let mut connected = 0usize;
        for block in persisted.blocks.into_iter().skip(local_height as usize + 1) {
            hf132_check_sync_budget()?;
            repaired.connect_block(block, settings)?;
            connected = connected.saturating_add(1);
        }
        validate_chain_consensus_checkpoints(settings, &repaired.blocks)?;
        return Ok((repaired, connected));
    }

    let candidate = ChainState::from_persisted(persisted, settings)?;
    validate_chain_consensus_checkpoints(settings, &candidate.blocks)?;
    let connected = candidate.height().saturating_sub(local_height) as usize;
    Ok((candidate, connected))
}

fn apply_official_tail_snapshot(
    settings: &Settings,
    local_before: &ChainState,
    tail: OfficialTailSnapshot,
) -> Result<(ChainState, usize)> {
    if tail.network != settings.network.name {
        bail!("tail network mismatch");
    }
    if tail.blocks.is_empty() {
        bail!("empty tail snapshot");
    }
    let local_height = local_before.height();
    let local_tip = local_before.tip_hash().to_string();

    // If the published tail is older than local, it cannot repair us. If it is
    // exactly the same height but the hash differs, do not return early: that is
    // precisely a local-fork repair case and the tail may contain the canonical
    // replacement suffix from a common ancestor.
    if tail.tip_height < local_height {
        return Ok((local_before.clone(), 0));
    }
    if tail.tip_height == local_height && tail.tip_hash == local_tip {
        return Ok((local_before.clone(), 0));
    }
    if tail.start_height > local_height.saturating_add(1) {
        bail!(
            "tail starts at #{} but local next height is #{}",
            tail.start_height,
            local_height.saturating_add(1)
        );
    }

    // HF74/v1.5.8 fixed2: tails must repair both normal behind-by-N nodes and
    // same/similar-height local forks. The old implementation rejected a tail as
    // soon as any overlapping local block differed, so a node stuck at a valid
    // but non-canonical #10700 could not be repaired by official tails and Sync
    // would spin until timeout. Find the highest common ancestor covered by the
    // official tail, then rebuild from local prefix + official tail suffix.
    let mut anchor_height: Option<u32> = None;
    for (idx, block) in tail.blocks.iter().enumerate() {
        let height = tail.start_height.saturating_add(idx as u32);
        if height > local_height {
            break;
        }
        if local_before
            .blocks
            .get(height as usize)
            .map(|b| b.block_hash())
            == Some(block.block_hash())
        {
            anchor_height = Some(height);
        }
    }

    // Common append-only case: tail starts exactly at local+1, so the local tip
    // itself is the implicit ancestor even though it is not inside the tail file.
    if anchor_height.is_none() && tail.start_height == local_height.saturating_add(1) {
        anchor_height = Some(local_height);
    }

    let anchor = anchor_height.context("official tail has no common ancestor with local chain")?;

    // HF78/v1.6.0: the normal case is append-only catch-up from the current
    // local tip. Do not rebuild/revalidate the entire chain for each short tail
    // sync; connect only the missing suffix and verify the resulting official
    // tip. Fork repairs still use the full prefix+suffix rebuild path below.
    if anchor == local_height {
        let mut repaired = local_before.clone();
        Arc::make_mut(&mut repaired.mempool).clear();
        let mut appended_from_tail = 0usize;
        for (idx, block) in tail.blocks.into_iter().enumerate() {
            let height = tail.start_height.saturating_add(idx as u32);
            if height <= anchor {
                continue;
            }
            if height != repaired.blocks.len() as u32 {
                bail!(
                    "tail has a height gap: got #{}, expected #{}",
                    height,
                    repaired.blocks.len()
                );
            }
            hf132_check_sync_budget()?;
            repaired.connect_block(block, settings)?;
            appended_from_tail = appended_from_tail.saturating_add(1);
        }
        if repaired.height() != tail.tip_height {
            bail!(
                "tail metadata height #{} does not match repaired height #{}",
                tail.tip_height,
                repaired.height()
            );
        }
        if repaired.tip_hash().to_string() != tail.tip_hash {
            bail!(
                "tail metadata hash {} does not match repaired tip {}",
                tail.tip_hash,
                repaired.tip_hash()
            );
        }
        validate_chain_consensus_checkpoints(settings, &repaired.blocks)?;
        return Ok((repaired, appended_from_tail));
    }

    let mut candidate_blocks = local_before.blocks[..=anchor as usize].to_vec();
    let mut appended_from_tail = 0usize;
    for (idx, block) in tail.blocks.into_iter().enumerate() {
        let height = tail.start_height.saturating_add(idx as u32);
        if height <= anchor {
            continue;
        }
        if height != candidate_blocks.len() as u32 {
            bail!(
                "tail has a height gap: got #{}, expected #{}",
                height,
                candidate_blocks.len()
            );
        }
        candidate_blocks.push(block);
        appended_from_tail = appended_from_tail.saturating_add(1);
    }

    let repaired = ChainState::from_blocks(candidate_blocks, settings)?;
    if repaired.height() != tail.tip_height {
        bail!(
            "tail metadata height #{} does not match repaired height #{}",
            tail.tip_height,
            repaired.height()
        );
    }
    if repaired.tip_hash().to_string() != tail.tip_hash {
        bail!(
            "tail metadata hash {} does not match repaired tip {}",
            tail.tip_hash,
            repaired.tip_hash()
        );
    }
    validate_chain_consensus_checkpoints(settings, &repaired.blocks)?;
    Ok((repaired, appended_from_tail))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Hf126EqualHeightTipEvidence {
    height: u32,
    tip_hash: String,
    http_matches: usize,
    official_tcp_matches: usize,
    direct_matches: usize,
}

impl Hf126EqualHeightTipEvidence {
    fn confirmed(&self) -> bool {
        self.official_tcp_matches >= 2
            || (self.http_matches > 0 && self.direct_matches >= 2)
    }
}

fn hf126_is_same_height_competing_tip(
    local_height: u32,
    local_tip: &str,
    remote_height: u32,
    remote_tip: &str,
) -> bool {
    remote_height == local_height
        && !remote_tip.trim().is_empty()
        && remote_tip.trim() != local_tip
}

/// HF126/v1.8.2: an equal-work sibling can only replace a locally mined tip
/// through the explicit re-anchor path when the durable relay record proves the
/// local tip is unacknowledged and independent official/direct sources agree on
/// the exact competing hash. Ordinary equal-height forks remain normal PoW ties.
fn hf126_verified_equal_height_competing_tip(
    settings: &Settings,
    local: &ChainState,
    timeout_ms: u64,
) -> Option<Hf126EqualHeightTipEvidence> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    if settings.network.name != "mainnet" || !pending_block_relay_waiting(settings, local) {
        return None;
    }

    let local_height = local.height();
    let local_tip = local.tip_hash().to_string();
    let mut evidence: HashMap<String, Hf126EqualHeightTipEvidence> = HashMap::new();

    // Reuse durable HF125 stale-parent acknowledgements as one evidence source.
    // They came from explicit SubmitBlock/BlockAck exchanges for this exact
    // unacknowledged local tip, not from a UI heuristic or cached height alone.
    if let Ok(Some(pending)) = load_pending_block_relay(settings) {
        if pending.height == local_height
            && pending.block_hash == local_tip
            && hf126_is_same_height_competing_tip(
                local_height,
                &local_tip,
                pending.last_stale_parent_height,
                &pending.last_stale_parent_tip,
            )
        {
            let tip = pending.last_stale_parent_tip.clone();
            let entry = evidence.entry(tip.clone()).or_insert_with(|| {
                Hf126EqualHeightTipEvidence {
                    height: pending.last_stale_parent_height,
                    tip_hash: tip.clone(),
                    ..Hf126EqualHeightTipEvidence::default()
                }
            });
            entry.official_tcp_matches = entry
                .official_tcp_matches
                .saturating_add(pending.last_official_stale_parent_reports);
            entry.direct_matches = entry.direct_matches.saturating_add(
                pending
                    .last_stale_parent_reports
                    .saturating_sub(pending.last_official_stale_parent_reports),
            );
        }
    }

    if let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(700).min(2_500)) {
        if hf126_is_same_height_competing_tip(local_height, &local_tip, height, &tip) {
            let entry = evidence.entry(tip.clone()).or_insert_with(|| {
                Hf126EqualHeightTipEvidence {
                    height,
                    tip_hash: tip.clone(),
                    ..Hf126EqualHeightTipEvidence::default()
                }
            });
            entry.http_matches = entry.http_matches.saturating_add(1);
        }
    }

    for (_, height, tip, _, _) in
        official_snapshot_peer_candidates(settings, timeout_ms.max(220).min(900))
    {
        if hf126_is_same_height_competing_tip(local_height, &local_tip, height, &tip) {
            let entry = evidence.entry(tip.clone()).or_insert_with(|| {
                Hf126EqualHeightTipEvidence {
                    height,
                    tip_hash: tip.clone(),
                    ..Hf126EqualHeightTipEvidence::default()
                }
            });
            entry.official_tcp_matches = entry.official_tcp_matches.saturating_add(1);
        }
    }

    if let Ok(peers) = prioritized_outbound_peers(settings, 12) {
        let per_peer = Duration::from_millis(timeout_ms.max(180).min(650));
        for addr in peers {
            if should_skip_outbound(settings, &addr) {
                continue;
            }
            let Ok(info) = probe_peer(settings, &addr, per_peer) else {
                continue;
            };
            if hf126_is_same_height_competing_tip(
                local_height,
                &local_tip,
                info.height,
                &info.tip_hash,
            ) {
                let tip = info.tip_hash;
                let entry = evidence.entry(tip.clone()).or_insert_with(|| {
                    Hf126EqualHeightTipEvidence {
                        height: info.height,
                        tip_hash: tip.clone(),
                        ..Hf126EqualHeightTipEvidence::default()
                    }
                });
                entry.direct_matches = entry.direct_matches.saturating_add(1);
            }
        }
    }

    evidence
        .into_values()
        .filter(Hf126EqualHeightTipEvidence::confirmed)
        .max_by_key(|item| {
            item.official_tcp_matches
                .saturating_mul(100)
                .saturating_add(item.http_matches.saturating_mul(25))
                .saturating_add(item.direct_matches)
        })
}

/// HF114/v1.7.2: official-tail re-anchor for local-ahead/self-mined stale
/// branches. The normal tail sync is append/reorg-forward only; this path is
/// deliberately separate because it is allowed to replace a longer local suffix
/// with the shorter official canonical prefix/tail after full consensus replay.
fn apply_official_tail_snapshot_hf114_reanchor(
    settings: &Settings,
    local_before: &ChainState,
    tail: OfficialTailSnapshot,
) -> Result<(ChainState, usize)> {
    if settings.network.name != "mainnet" {
        bail!("HF114 re-anchor is mainnet-only");
    }
    if tail.network != settings.network.name {
        bail!("tail network mismatch");
    }
    if tail.blocks.is_empty() {
        bail!("empty tail snapshot");
    }
    let local_height = local_before.height();
    if tail.tip_height > local_height.saturating_add(1) {
        bail!(
            "tail tip #{} is ahead of local #{}; use normal catch-up",
            tail.tip_height,
            local_height
        );
    }

    let mut anchor_height: Option<u32> = None;
    for (idx, block) in tail.blocks.iter().enumerate() {
        let height = tail.start_height.saturating_add(idx as u32);
        if height > local_height {
            break;
        }
        if local_before
            .blocks
            .get(height as usize)
            .map(|b| b.block_hash())
            == Some(block.block_hash())
        {
            anchor_height = Some(height);
        }
    }
    let anchor = anchor_height
        .context("official tail has no common ancestor with local chain for HF114 re-anchor")?;

    let mut candidate_blocks = local_before.blocks[..=anchor as usize].to_vec();
    let mut appended_from_tail = 0usize;
    for (idx, block) in tail.blocks.into_iter().enumerate() {
        let height = tail.start_height.saturating_add(idx as u32);
        if height <= anchor {
            continue;
        }
        if height != candidate_blocks.len() as u32 {
            bail!(
                "tail has a height gap during HF114 re-anchor: got #{}, expected #{}",
                height,
                candidate_blocks.len()
            );
        }
        candidate_blocks.push(block);
        appended_from_tail = appended_from_tail.saturating_add(1);
    }

    let mut candidate = ChainState::from_blocks(candidate_blocks, settings)?;
    if candidate.height() != tail.tip_height {
        bail!(
            "tail metadata height #{} does not match HF114 re-anchor height #{}",
            tail.tip_height,
            candidate.height()
        );
    }
    if candidate.tip_hash().to_string() != tail.tip_hash {
        bail!(
            "tail metadata hash {} does not match HF114 re-anchor tip {}",
            tail.tip_hash,
            candidate.tip_hash()
        );
    }
    validate_chain_consensus_checkpoints(settings, &candidate.blocks)?;

    // HF117: rebuild with disconnected local suffix txs too, so QUB sends
    // mined in a losing local branch return to the mempool after re-anchor.
    let keep_mempool = local_before.reorg_mempool_candidates_for(&candidate);
    candidate.rebuild_mempool_from(keep_mempool, settings);
    let rolled_back = local_height.saturating_sub(candidate.height()) as usize;
    Ok((candidate, rolled_back.saturating_add(appended_from_tail)))
}

/// HF114/v1.7.2: when the local node is ahead only because it kept mining a
/// private stale suffix, re-anchor it to the official HTTP tail immediately. This
/// removes the old user workaround of stopping mining and waiting for canonical
/// height to overtake the local height.
fn sync_official_http_tail_reanchor_hf114(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if settings.network.name != "mainnet" {
        return finish_report(settings, report);
    }
    let mut urls = official_http_tail_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }

    let local_before = load_chain_for_hf90_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();

    let Some(evidence) =
        hf126_verified_equal_height_competing_tip(settings, &local_before, 900)
    else {
        report.height = local_height_before;
        report.tip_hash = local_tip_before;
        return finish_report(settings, report);
    };
    let official_height = evidence.height;
    let official_tip = evidence.tip_hash.clone();
    report.peers_contacted = report.peers_contacted.max(
        evidence
            .official_tcp_matches
            .saturating_add(evidence.direct_matches)
            .saturating_add(evidence.http_matches),
    );
    hf129_observe_tip(&mut report, official_height, &official_tip);
    eprintln!(
        "HF126 verified equal-height re-anchor evidence at #{} {}: http={} official_tcp={} direct={}",
        official_height,
        official_tip,
        evidence.http_matches,
        evidence.official_tcp_matches,
        evidence.direct_matches,
    );

    urls.sort_by_key(|url| official_tail_window_from_url(url));
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for url in urls {
        if Instant::now() >= deadline {
            break;
        }
        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let per_url_timeout = left_ms.min(8_000);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let tail = match hf130_download_tail(settings, &url, per_url_timeout) {
            Ok(tail) => tail,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        hf129_observe_tip(&mut report, tail.tip_height, &tail.tip_hash);
        if official_height > 0 && tail.tip_height != official_height {
            continue;
        }
        if !official_tip.trim().is_empty() && tail.tip_hash != official_tip {
            continue;
        }

        let (candidate, replaced_hint) =
            match apply_official_tail_snapshot_hf114_reanchor(settings, &local_before, tail) {
                Ok(v) => v,
                Err(_) => {
                    report.peer_errors = report.peer_errors.saturating_add(1);
                    continue;
                }
            };
        if candidate.height() == local_height_before
            && candidate.tip_hash().to_string() != local_tip_before
        {
            let candidate_work = chain_work_for_blocks(&candidate.blocks)?;
            let local_work = chain_work_for_blocks(&local_before.blocks)?;
            if candidate_work < local_work {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
            if candidate_work == local_work {
                save_chain_verified_equal_work_reanchor(settings, &candidate)?;
            } else {
                save_chain(settings, &candidate)?;
            }
            clear_pending_block_relay(settings);
            mark_fresh_tip_trusted(settings, &candidate);
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report.blocks_connected.saturating_add(replaced_hint);
            report.height = candidate.height();
            report.tip_hash = candidate.tip_hash().to_string();
            return finish_report(settings, report);
        }
    }

    // If the stale private suffix is longer than the published tail windows, use
    // the full official snapshot as a last re-anchor path. This is intentionally
    // reached only for local-ahead/same-height-conflict repair, not normal mining.
    let snapshot_budget = total_timeout_ms.max(30_000).min(90_000);
    if let Ok(snapshot) = sync_official_http_snapshot_reanchor_hf114(
        settings,
        official_height,
        &official_tip,
        snapshot_budget,
    ) {
        let adopted = snapshot.chains_adopted > 0;
        merge_sync_reports(&mut report, snapshot);
        if adopted {
            return finish_report(settings, report);
        }
    }

    report.height = local_height_before;
    report.tip_hash = local_tip_before;
    finish_report(settings, report)
}

/// HF114/v1.7.2: full official HTTP snapshot re-anchor fallback. This is used
/// only when mainnet local state is ahead/same-height-conflicting and the shorter
/// tail windows do not include the fork ancestor. It is slower than tail repair but
/// removes the old requirement to wait for the canonical chain to overtake a long
/// private self-mined suffix.
fn sync_official_http_snapshot_reanchor_hf114(
    settings: &Settings,
    official_height_hint: u32,
    official_tip_hint: &str,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if settings.network.name != "mainnet" {
        return finish_report(settings, report);
    }
    let urls = official_http_snapshot_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }

    let local_before = load_chain_for_hf90_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    let mut tmp = std::env::temp_dir();
    tmp.push(format!(
        "qub-{}-official-snapshot-hf114-reanchor-{}.json",
        settings.network.name,
        std::process::id()
    ));

    for url in urls {
        let _ = fs::remove_file(&tmp);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let persisted = match hf130_download_full(settings, &url, remaining_ms(hf130_effective_deadline(Instant::now() + Duration::from_millis(total_timeout_ms)))) {
            Ok(persisted) => persisted,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        let (mut candidate, connected_hint) =
            apply_official_snapshot_fast_path(settings, &local_before, persisted).with_context(
                || format!("HF114 re-anchor snapshot failed consensus replay from {url}"),
            )?;

        validate_chain_consensus_checkpoints(settings, &candidate.blocks)?;
        hf129_observe_tip(&mut report, candidate.height(), &candidate.tip_hash().to_string());
        if official_height_hint > 0 && candidate.height() < official_height_hint {
            report.peer_errors = report.peer_errors.saturating_add(1);
            continue;
        }
        if official_height_hint > 0
            && candidate.height() == official_height_hint
            && !official_tip_hint.trim().is_empty()
            && candidate.tip_hash().to_string() != official_tip_hint.trim()
        {
            report.peer_errors = report.peer_errors.saturating_add(1);
            continue;
        }

        if candidate.height() < local_height_before
            || candidate.height() > local_height_before
            || (candidate.height() == local_height_before
                && candidate.tip_hash().to_string() != local_tip_before)
        {
            let keep_mempool = local_before.reorg_mempool_candidates_for(&candidate);
            candidate.rebuild_mempool_from(keep_mempool, settings);
            if candidate.height() == local_height_before
                && candidate.tip_hash().to_string() != local_tip_before
            {
                let candidate_work = chain_work_for_blocks(&candidate.blocks)?;
                let local_work = chain_work_for_blocks(&local_before.blocks)?;
                if candidate_work < local_work {
                    report.peer_errors = report.peer_errors.saturating_add(1);
                    continue;
                }
                if candidate_work == local_work {
                    save_chain_verified_equal_work_reanchor(settings, &candidate)?;
                } else {
                    save_chain(settings, &candidate)?;
                }
                clear_pending_block_relay(settings);
            } else {
                save_chain(settings, &candidate)?;
            }
            mark_fresh_tip_trusted(settings, &candidate);
            let height_delta = if candidate.height() >= local_height_before {
                candidate.height().saturating_sub(local_height_before) as usize
            } else {
                local_height_before.saturating_sub(candidate.height()) as usize
            };
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report
                .blocks_connected
                .saturating_add(connected_hint.max(height_delta));
            report.height = candidate.height();
            report.tip_hash = candidate.tip_hash().to_string();
            let _ = fs::remove_file(&tmp);
            return finish_report(settings, report);
        }

        report.height = local_height_before;
        report.tip_hash = local_tip_before.clone();
        let _ = fs::remove_file(&tmp);
        return finish_report(settings, report);
    }

    let _ = fs::remove_file(&tmp);
    finish_report(settings, report)
}

/// HF70/v1.5.8: static HTTP tail snapshot. This is much smaller than full
/// chain.json and covers the common case where a miner is 1..1024 blocks behind.
/// The tail is not trusted blindly: every block is connected with full consensus
/// validation against the existing local tip/prefix.
pub fn sync_official_http_tail(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    let mut urls = official_http_tail_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "official-http-tail",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let local_before = load_chain_for_hf132_validated_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    if let Ok(Some((official_h, _))) = official_http_tip(settings, 700) {
        let gap = official_h.saturating_sub(local_height_before).max(1);
        urls.sort_by_key(|url| {
            let window = official_tail_window_from_url(url);
            (window < gap, window)
        });
    }
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(1_500));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);

    for url in urls {
        if Instant::now() >= deadline {
            break;
        }
        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let per_url_timeout = left_ms.min(8_000);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let tail = match hf130_download_tail(settings, &url, per_url_timeout) {
            Ok(tail) => tail,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        hf129_observe_tip(&mut report, tail.tip_height, &tail.tip_hash);
        let (candidate, connected_hint) =
            match apply_official_tail_snapshot(settings, &local_before, tail) {
                Ok(v) => v,
                Err(_) => {
                    report.peer_errors = report.peer_errors.saturating_add(1);
                    continue;
                }
            };
        let mut selected = local_before.clone();
        let adopted = selected.try_adopt_validated_peer_chain(candidate, settings, false)?;
        if adopted {
            // HF125/v1.8.2: HTTP snapshots are bootstrap transports, not an
            // equal-work chain-selection authority. Only normal cumulative-work
            // fork choice may replace the current canonical state.
            save_chain(settings, &selected)?;
            mark_fresh_tip_trusted(settings, &selected);
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report.blocks_connected.saturating_add(
                connected_hint.max(selected.height().saturating_sub(local_height_before) as usize),
            );
            report.height = selected.height();
            report.tip_hash = selected.tip_hash().to_string();
            return finish_report(settings, report);
        }
        report.height = local_height_before;
        report.tip_hash = local_tip_before.clone();
    }
    finish_report(settings, report)
}

/// HF68/v1.5.2: HTTP canonical snapshot fallback. This is deliberately outside
/// the P2P retry path. If the GUI/CLI can see official seeds ahead but the P2P
/// suffix/adoption path still fails to append, download a full canonical
/// chain.json snapshot from the official download host, replay-validate it with
/// normal consensus rules/checkpoints, and replace the local post-stale state.
/// This is the automatic version of the old manual fresh reinstall/snapshot
/// repair flow. The snapshot is not trusted blindly: invalid consensus or wrong
/// checkpoint data is rejected by ChainState::from_persisted/validate_all.
pub fn sync_official_http_snapshot(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    let urls = official_http_snapshot_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "official-http-snapshot",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let local_before = load_chain_for_hf132_validated_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    let mut tmp = std::env::temp_dir();
    tmp.push(format!(
        "qub-{}-official-snapshot-{}.json",
        settings.network.name,
        std::process::id()
    ));

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for url in urls {
        let left = remaining_ms(deadline);
        if left < 2_000 {
            break;
        }
        let _ = fs::remove_file(&tmp);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let persisted = match hf130_download_full(settings, &url, remaining_ms(hf130_effective_deadline(Instant::now() + Duration::from_millis(total_timeout_ms)))) {
            Ok(persisted) => persisted,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        let (candidate, connected_hint) =
            apply_official_snapshot_fast_path(settings, &local_before, persisted)
                .with_context(|| format!("snapshot failed consensus repair from {url}"))?;

        hf129_observe_tip(&mut report, candidate.height(), &candidate.tip_hash().to_string());
        let mut selected = local_before.clone();
        let adopted = selected.try_adopt_validated_peer_chain(candidate, settings, false)?;
        if adopted {
            // HF125/v1.8.2: a published full snapshot must win by validated
            // cumulative work, never by hostname or publication status alone.
            save_chain(settings, &selected)?;
            mark_fresh_tip_trusted(settings, &selected);
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report.blocks_connected.saturating_add(
                connected_hint.max(selected.height().saturating_sub(local_height_before) as usize),
            );
            report.height = selected.height();
            report.tip_hash = selected.tip_hash().to_string();
            let _ = fs::remove_file(&tmp);
            return finish_report(settings, report);
        }

        report.height = local_height_before;
        report.tip_hash = local_tip_before.clone();
        let _ = fs::remove_file(&tmp);
        return finish_report(settings, report);
    }

    let _ = fs::remove_file(&tmp);
    finish_report(settings, report)
}

/// HF107/v1.6.9: emergency canonical full-chain pull from official direct seeds.
fn sync_official_direct_full_chain(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "official-direct-full-chain",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let local_before = load_chain_for_hf90_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);

    let mut candidates = official_snapshot_peer_candidates(settings, 1_500);
    if candidates.is_empty() {
        candidates = official_snapshot_peers(settings)
            .into_iter()
            .map(|addr| (addr, 0, String::new(), 0, String::new()))
            .collect();
    }

    for (addr, candidate_height, candidate_tip, _, _) in candidates.into_iter().take(4) {
        if Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }

        let Ok(mut stream) = connect_peer(&addr, left.min(Duration::from_millis(7_500))) else {
            report.peer_errors = report.peer_errors.saturating_add(1);
            continue;
        };
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        hf129_observe_tip(&mut report, candidate_height, &candidate_tip);
        let io_left = deadline.saturating_duration_since(Instant::now());
        if io_left.is_zero() {
            break;
        }
        let _ = stream.set_read_timeout(Some(io_left.min(Duration::from_millis(18_000))));
        let _ = stream.set_write_timeout(Some(io_left.min(Duration::from_millis(5_000))));
        let mut reader = BufReader::new(stream.try_clone()?);

        let local_now = load_chain_for_hf90_catchup(settings)?;
        let _ = send_version(&mut stream, settings, &local_now);
        let _ = send_wire(&mut stream, &WireMessage::GetChain { from_height: 0 });
        let _ = send_wire(
            &mut stream,
            &WireMessage::GetHeaders {
                from_height: local_now.height().saturating_sub(256),
            },
        );

        let mut assembled: Vec<Block> = Vec::new();
        let mut expected_from: u32 = 0;
        let mut peer_height = candidate_height;
        let mut peer_tip = candidate_tip.clone();
        let mut last_request = Instant::now();
        let mut last_rx = Instant::now();

        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(WireMessage::Version {
                    height, tip_hash, ..
                })
                | Ok(WireMessage::Inv {
                    height, tip_hash, ..
                }) => {
                    if height >= peer_height {
                        peer_height = height;
                        if !tip_hash.trim().is_empty() {
                            peer_tip = tip_hash;
                        }
                    }
                    hf129_observe_tip(&mut report, height, "");
                    last_rx = Instant::now();
                }
                Ok(WireMessage::Headers { headers }) => {
                    if let Some(last) = headers.last() {
                        if last.height >= peer_height {
                            peer_height = last.height;
                            peer_tip = last.hash.clone();
                        }
                        hf129_observe_tip(&mut report, last.height, &last.hash);
                    }
                    last_rx = Instant::now();
                }
                Ok(WireMessage::Chain {
                    start_height,
                    blocks,
                }) => {
                    last_rx = Instant::now();
                    if blocks.is_empty() {
                        continue;
                    }
                    if blocks.len() > settings.p2p.max_blocks_per_message {
                        bail!("too many blocks in full official chain message");
                    }
                    if start_height == 0 {
                        assembled.clear();
                        expected_from = 0;
                    }
                    if start_height != expected_from {
                        assembled.clear();
                        expected_from = 0;
                        let _ = send_wire(&mut stream, &WireMessage::GetChain { from_height: 0 });
                        last_request = Instant::now();
                        continue;
                    }
                    let more_expected = blocks.len() == settings.p2p.max_blocks_per_message;
                    expected_from = start_height.saturating_add(blocks.len() as u32);
                    assembled.extend(blocks);
                    let assembled_height = assembled.len().saturating_sub(1) as u32;
                    if more_expected && (peer_height == 0 || assembled_height < peer_height) {
                        let _ = send_wire(
                            &mut stream,
                            &WireMessage::GetChain {
                                from_height: expected_from,
                            },
                        );
                        last_request = Instant::now();
                        continue;
                    }
                    if !assembled.is_empty() {
                        let mut candidate = ChainState::from_blocks(assembled.clone(), settings)
                            .with_context(|| format!("official direct full chain from {addr} failed consensus replay"))?;
                        validate_chain_consensus_checkpoints(settings, &candidate.blocks)?;
                        let candidate_height_now = candidate.height();
                        let candidate_tip_now = candidate.tip_hash().to_string();
                        hf129_observe_tip(&mut report, candidate_height_now, &candidate_tip_now);
                        hf129_observe_tip(&mut report, peer_height, &peer_tip);
                        let repairs_same_height = candidate_height_now == local_height_before
                            && candidate_tip_now != local_tip_before;
                        let matches_peer_tip = peer_tip.trim().is_empty()
                            || candidate_height_now >= peer_height
                            || candidate_tip_now == peer_tip;
                        if matches_peer_tip
                            && (candidate_height_now > local_height_before || repairs_same_height)
                        {
                            let keep_mempool =
                                local_before.reorg_mempool_candidates_for(&candidate);
                            candidate.rebuild_mempool_from(keep_mempool, settings);
                            save_chain(settings, &candidate)?;
                            mark_fresh_tip_trusted(settings, &candidate);
                            report.chains_adopted = report.chains_adopted.saturating_add(1);
                            report.blocks_connected = report.blocks_connected.saturating_add(
                                candidate_height_now.saturating_sub(local_height_before) as usize,
                            );
                            report.height = candidate_height_now;
                            report.tip_hash = candidate.tip_hash().to_string();
                            return finish_report(settings, report);
                        }
                    }
                    break;
                }
                Ok(_) => {}
                Err(err) if is_timeout(&err) => {
                    if last_request.elapsed() >= Duration::from_secs(5) {
                        let from = if assembled.is_empty() {
                            0
                        } else {
                            expected_from
                        };
                        let _ =
                            send_wire(&mut stream, &WireMessage::GetChain { from_height: from });
                        last_request = Instant::now();
                    }
                    if last_rx.elapsed() >= Duration::from_secs(24) {
                        break;
                    }
                }
                Err(err) => {
                    if !is_benign_io(&err) {
                        report.peer_errors = report.peer_errors.saturating_add(1);
                    }
                    break;
                }
            }
        }
    }

    finish_report(settings, report)
}

pub fn sync_official_snapshot(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "official-p2p-snapshot",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(4_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    for (addr, candidate_height, candidate_tip, _latency_ms, _node_id) in
        official_snapshot_peer_candidates(settings, 650)
            .into_iter()
            .take(4)
    {
        if Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }

        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        let connect_timeout = left.min(Duration::from_millis(6_000));
        let Ok(mut stream) = connect_peer(&addr, connect_timeout) else {
            report.peer_errors = report.peer_errors.saturating_add(1);
            continue;
        };
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        hf129_observe_tip(&mut report, candidate_height, &candidate_tip);
        let io_left = deadline.saturating_duration_since(Instant::now());
        if io_left.is_zero() {
            break;
        }
        let _ = stream.set_read_timeout(Some(
            io_left.min(Duration::from_millis(OFFICIAL_SNAPSHOT_READ_TIMEOUT_MS)),
        ));
        let _ = stream.set_write_timeout(Some(io_left.min(Duration::from_millis(2_000))));
        let mut reader = BufReader::new(stream.try_clone()?);

        let local = load_chain_for_hf90_catchup(settings)?;
        let local_before_height = local.height();
        let local_before_tip = local.tip_hash().to_string();
        let anchor_from = force_anchor_from_height(settings, &local);
        let fast_from = local.height().saturating_add(1);
        let recent_header_from = local
            .height()
            .saturating_sub(512)
            .max(anchor_from.min(local.height()));
        let _ = send_version(&mut stream, settings, &local);

        // Fast path: most users are only one/few blocks behind. Ask for the tiny
        // suffix first, not the whole post-checkpoint suffix. Also ask for recent
        // headers so if the local tip is stale/forked we can find the true common
        // ancestor and request a small suffix instead of a giant checkpoint pull.
        let mut requested_from = if fast_from > anchor_from {
            fast_from
        } else {
            anchor_from
        };
        let mut anchor_requested = requested_from == anchor_from;
        let mut recent_headers_requested = true;
        let _ = send_wire(
            &mut stream,
            &WireMessage::GetChain {
                from_height: requested_from,
            },
        );
        let _ = send_wire(
            &mut stream,
            &WireMessage::GetHeaders {
                from_height: recent_header_from,
            },
        );

        let mut peer_height = candidate_height;
        let mut peer_tip = String::new();
        let mut last_rx = Instant::now();
        let mut last_request = Instant::now();

        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(msg) => {
                    last_rx = Instant::now();
                    match msg {
                        WireMessage::Version {
                            height, tip_hash, ..
                        }
                        | WireMessage::Inv {
                            height, tip_hash, ..
                        } => {
                            if height >= peer_height {
                                peer_height = height;
                                peer_tip = tip_hash;
                            }
                            hf129_observe_tip(&mut report, height, "");
                            let cur = load_chain_for_hf90_catchup(settings)?;
                            if height == cur.height()
                                && !peer_tip.trim().is_empty()
                                && peer_tip != cur.tip_hash().to_string()
                            {
                                let from = cur
                                    .height()
                                    .saturating_sub(512)
                                    .max(anchor_from.min(cur.height()));
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::GetHeaders { from_height: from },
                                );
                                last_request = Instant::now();
                            }
                        }
                        WireMessage::Headers { headers } => {
                            if let Some(last) = headers.last() {
                                hf129_observe_tip(&mut report, last.height, &last.hash);
                            }
                            let cur = load_chain_for_hf90_catchup(settings)?;
                            if let Some(common) = best_common_height_from_headers(&cur, &headers) {
                                let from = common.saturating_add(1);
                                if from
                                    <= report
                                        .best_peer_height
                                        .max(peer_height)
                                        .max(cur.height().saturating_add(1))
                                {
                                    requested_from = from;
                                    anchor_requested =
                                        anchor_requested || requested_from == anchor_from;
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::GetChain {
                                            from_height: requested_from,
                                        },
                                    );
                                    last_request = Instant::now();
                                }
                            } else if !anchor_requested {
                                requested_from = anchor_from;
                                anchor_requested = true;
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::GetChain {
                                        from_height: requested_from,
                                    },
                                );
                                last_request = Instant::now();
                            }
                        }
                        WireMessage::Chain {
                            start_height,
                            blocks,
                        } => {
                            if blocks.len() > settings.p2p.max_blocks_per_message {
                                bail!("too many blocks in chain message");
                            }
                            if blocks.is_empty() {
                                continue;
                            }
                            let more_expected =
                                blocks.len() == settings.p2p.max_blocks_per_message;
                            let next_from = start_height.saturating_add(blocks.len() as u32);
                            let mut chain = load_chain_for_hf90_catchup(settings)?;
                            let before_height = chain.height();
                            let before_tip = chain.tip_hash().to_string();

                            // HF102: never let an optimistic +1 suffix response abort the
                            // official repair session. If the local tip is a stale/fork tip,
                            // the peer may send #local+1 first; connect_block will reject it
                            // because the prev-hash does not match. Older HF87-HF89 code used
                            // `?` here, so the whole official P2P repair ended before the
                            // already-requested headers could reveal the common ancestor.
                            let changed = match try_adopt_overlapping_blocks(
                                &mut chain,
                                start_height,
                                blocks,
                                settings,
                                true,
                            ) {
                                Ok(v) => v,
                                Err(_) => {
                                    report.peer_errors = report.peer_errors.saturating_add(1);
                                    let cur = load_chain_for_hf90_catchup(settings)?;
                                    let overlap_from = cur
                                        .height()
                                        .saturating_sub(2048)
                                        .max(anchor_from.min(cur.height()));
                                    requested_from = overlap_from;
                                    anchor_requested =
                                        anchor_requested || requested_from == anchor_from;
                                    recent_headers_requested = true;
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::GetHeaders {
                                            from_height: overlap_from,
                                        },
                                    );
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::GetChain {
                                            from_height: overlap_from,
                                        },
                                    );
                                    last_request = Instant::now();
                                    continue;
                                }
                            };
                            if changed {
                                save_chain(settings, &chain)?;
                                report.chains_adopted = report.chains_adopted.saturating_add(1);
                                report.blocks_connected = report.blocks_connected.saturating_add(
                                    chain.height().saturating_sub(before_height) as usize,
                                );
                            } else {
                                let cur = load_chain_for_hf90_catchup(settings)?;
                                // HF102: no progress from the requested window. Escalate the
                                // overlap window immediately instead of waiting for multiple
                                // timeout cycles while the public tip moves farther ahead.
                                let from = if start_height >= before_height.saturating_add(1) {
                                    cur.height()
                                        .saturating_sub(512)
                                        .max(anchor_from.min(cur.height()))
                                } else if !anchor_requested {
                                    anchor_from
                                } else {
                                    cur.height()
                                        .saturating_sub(4096)
                                        .max(anchor_from.min(cur.height()))
                                };
                                if from != requested_from || !recent_headers_requested {
                                    requested_from = from;
                                    anchor_requested =
                                        anchor_requested || requested_from == anchor_from;
                                    recent_headers_requested = true;
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::GetHeaders {
                                            from_height: requested_from,
                                        },
                                    );
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::GetChain {
                                            from_height: requested_from,
                                        },
                                    );
                                    last_request = Instant::now();
                                    continue;
                                }
                            }
                            if more_expected && Instant::now() < deadline {
                                requested_from = next_from;
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::GetChain {
                                        from_height: requested_from,
                                    },
                                );
                                last_request = Instant::now();
                            }
                            let current = load_chain_for_hf90_catchup(settings)?;
                            let target = peer_height.max(report.best_peer_height);
                            if target > 0 && current.height() >= target {
                                if peer_tip.trim().is_empty()
                                    || current.tip_hash().to_string() == peer_tip
                                    || current.height() > target
                                {
                                    break;
                                }
                            }
                            if current.height() > local_before_height
                                || current.tip_hash().to_string() != local_before_tip
                            {
                                if peer_height == 0
                                    || current.height() >= peer_height
                                    || last_rx.elapsed() > Duration::from_millis(500)
                                {
                                    break;
                                }
                            }
                            let _ = before_tip;
                        }
                        WireMessage::Mempool { txs } => {
                            let mut chain = load_chain_for_hf90_catchup(settings)?;
                            for tx in txs.into_iter().take(hf115_mempool_inbound_limit(settings)) {
                                if chain.accept_transaction_to_mempool(tx, settings).is_ok() {
                                    report.txs_accepted = report.txs_accepted.saturating_add(1);
                                }
                            }
                            let _ = save_chain(settings, &chain);
                        }
                        _ => {}
                    }
                }
                Err(err) if is_timeout(&err) => {
                    let cur = load_chain_for_hf90_catchup(settings)?;
                    if last_request.elapsed() >= Duration::from_secs(2) {
                        if !recent_headers_requested {
                            let from = cur
                                .height()
                                .saturating_sub(512)
                                .max(anchor_from.min(cur.height()));
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::GetHeaders { from_height: from },
                            );
                            recent_headers_requested = true;
                            last_request = Instant::now();
                            continue;
                        }
                        if !anchor_requested {
                            requested_from = anchor_from;
                            anchor_requested = true;
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::GetChain {
                                    from_height: requested_from,
                                },
                            );
                            last_request = Instant::now();
                            continue;
                        }
                        if last_rx.elapsed() >= Duration::from_secs(10) {
                            break;
                        }
                    }
                }
                Err(err) => {
                    if !is_benign_io(&err) {
                        report.peer_errors = report.peer_errors.saturating_add(1);
                    }
                    break;
                }
            }
        }

        let after = load_chain_for_hf90_catchup(settings)?;
        report.height = after.height();
        report.tip_hash = after.tip_hash().to_string();
        if after.height() > local_before_height || after.tip_hash().to_string() != local_before_tip
        {
            break;
        }
    }

    finish_report(settings, report)
}

fn sync_peer_force_anchor_session(
    settings: &Settings,
    addr: &str,
    min_peer_height: u32,
    deadline: Instant,
    report: &mut P2PSyncReport,
) -> Result<()> {
    let _hf130_entry_scope = Hf130IoScope::deadline(deadline);
    let connect_left = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(3_500));
    if connect_left.is_zero() {
        return Ok(());
    }

    let mut stream = connect_peer(addr, connect_left)?;
    report.peers_contacted = report.peers_contacted.saturating_add(1);
    let io_left = deadline.saturating_duration_since(Instant::now());
    if io_left.is_zero() {
        return Ok(());
    }
    stream.set_read_timeout(Some(
        io_left.min(Duration::from_millis(FORCE_ANCHOR_SYNC_READ_TIMEOUT_MS)),
    ))?;
    stream.set_write_timeout(Some(io_left.min(Duration::from_millis(5_000))))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut client_session = PeerSession::default();

    let local = load_chain_for_hf90_catchup(settings)?;
    let anchor_from = force_anchor_from_height(settings, &local);
    let local_start_height = local.height();
    let local_start_tip = local.tip_hash().to_string();

    let _ = send_version(&mut stream, settings, &local);
    let _ = send_wire(&mut stream, &WireMessage::GetAddr);
    let _ = send_wire(&mut stream, &WireMessage::GetPeerList);
    let _ = send_wire(&mut stream, &WireMessage::GetMempool);

    // The important line: ask from the last trusted anchor, not from the local
    // stale tip. If local is on a post-checkpoint fork, suffix-from-tip can never
    // connect; this anchor pull replaces the fork suffix deterministically.
    let _ = send_wire(
        &mut stream,
        &WireMessage::GetHeaders {
            from_height: anchor_from,
        },
    );
    let _ = send_wire(
        &mut stream,
        &WireMessage::GetChain {
            from_height: anchor_from,
        },
    );

    client_session.sync_local_height = local.height();
    client_session.sync_local_tip = local.tip_hash().to_string();
    client_session.sync_from_height = anchor_from;
    client_session.sync_requested_at = Some(Instant::now());

    let mut peer_height = 0u32;
    let mut peer_tip = String::new();
    let mut last_request_at = Instant::now();
    let mut last_progress_at = Instant::now();
    let mut last_local_height = local_start_height;
    let mut last_local_tip = local_start_tip;

    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes) {
            Ok(msg) => {
                match &msg {
                    WireMessage::Version {
                        height, tip_hash, ..
                    }
                    | WireMessage::Inv {
                        height, tip_hash, ..
                    } => {
                        peer_height = peer_height.max(*height);
                        if *height >= peer_height {
                            peer_tip = tip_hash.clone();
                        }
                        hf129_observe_tip(report, *height, tip_hash);
                    }
                    _ => {}
                }
                process_client_message(settings, addr, msg, &mut stream, report, &mut client_session)?;

                let current = load_chain_for_hf90_catchup(settings)?;
                if current.height() != last_local_height
                    || current.tip_hash().to_string() != last_local_tip
                {
                    last_local_height = current.height();
                    last_local_tip = current.tip_hash().to_string();
                    last_progress_at = Instant::now();
                }
                let wanted_height = if peer_height > 0 {
                    peer_height
                } else {
                    min_peer_height
                };
                if wanted_height > 0 && current.height() >= wanted_height {
                    if peer_tip.trim().is_empty()
                        || current.height() > peer_height
                        || current.tip_hash().to_string() == peer_tip
                    {
                        break;
                    }
                }
            }
            Err(err) if is_timeout(&err) => {
                // Re-ask from the anchor if the peer was slow or the previous message
                // was dropped. This makes the path robust for home connections.
                if last_request_at.elapsed() >= Duration::from_secs(4) {
                    let _ = send_wire(
                        &mut stream,
                        &WireMessage::GetHeaders {
                            from_height: anchor_from,
                        },
                    );
                    let _ = send_wire(
                        &mut stream,
                        &WireMessage::GetChain {
                            from_height: anchor_from,
                        },
                    );
                    last_request_at = Instant::now();
                }
                if last_progress_at.elapsed() >= Duration::from_secs(18) {
                    break;
                }
            }
            Err(err) => {
                if is_benign_io(&err) {
                    break;
                }
                return Err(err.into());
            }
        }
    }
    Ok(())
}

fn sync_force_anchor_to_best_direct(
    settings: &Settings,
    min_peer_height: u32,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(total_timeout_ms));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let _guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        total_timeout_ms,
        false,
        "direct-force-anchor",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(3_000));
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let peers =
        prioritized_outbound_peers(settings, settings.p2p.max_outbound_peers.max(8).min(24))
            .unwrap_or_else(|_| release_bootnodes(settings));
    let mut tried = 0usize;

    for addr in peers.into_iter() {
        if tried >= FORCE_ANCHOR_SYNC_MAX_PEERS || Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }

        // HF60/v1.5.2: always try official bootnodes during forced anchor sync.
        // A stale/forked direct peer can report a higher height than AMS3, but AMS3
        // is the operational canonical bootstrap while extra seeds are paused. Do
        // not skip AMS3 just because a rogue/unreachable peer claimed a higher tip.
        let normalized_addr = normalize_peer_addr(&addr);
        let is_release_bootnode = release_bootnodes(settings)
            .into_iter()
            .map(|b| normalize_peer_addr(&b))
            .any(|b| b == normalized_addr);
        let mut should_try = is_release_bootnode;
        if let Ok(info) = probe_peer(settings, &addr, Duration::from_millis(900)) {
            hf129_observe_tip(&mut report, info.height, &info.tip_hash);
            should_try = should_try || min_peer_height == 0 || info.height >= min_peer_height;
        }
        if !should_try {
            continue;
        }

        tried = tried.saturating_add(1);
        let before = load_chain_for_hf90_catchup(settings).ok();
        let before_height = before.as_ref().map(|c| c.height()).unwrap_or(0);
        let before_tip = before
            .as_ref()
            .map(|c| c.tip_hash().to_string())
            .unwrap_or_default();

        match sync_peer_force_anchor_session(
            settings,
            &addr,
            min_peer_height,
            deadline,
            &mut report,
        ) {
            Ok(()) => {}
            Err(err) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                if !is_benign_io_error(&err) {
                    // Try next direct peer. Forced anchor sync is best-effort.
                }
            }
        }

        let after = load_chain_for_hf90_catchup(settings)?;
        if after.height() > before_height || after.tip_hash().to_string() != before_tip {
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report
                .blocks_connected
                .saturating_add(after.height().saturating_sub(before_height) as usize);
        }
        if min_peer_height == 0 || after.height() >= min_peer_height {
            break;
        }
    }

    finish_report(settings, report)
}

fn catch_up_to_direct_height(
    settings: &Settings,
    min_peer_height: u32,
    normal_rounds: usize,
    sleep_ms: u64,
) -> Result<P2PSyncReport> {
    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "direct-height-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, workflow_report),
    };
    let mut merged = sync_until_converged(settings, normal_rounds, sleep_ms)?;
    let local = load_chain_for_hf90_catchup(settings)?;
    if min_peer_height > local.height() {
        let forced = sync_force_anchor_to_best_direct(
            settings,
            min_peer_height,
            FORCE_ANCHOR_SYNC_TIMEOUT_MS,
        )?;
        merge_sync_reports(&mut merged, forced);
    }
    finish_report(settings, merged)
}

pub fn sync_until_converged(
    settings: &Settings,
    rounds: usize,
    sleep_ms: u64,
) -> Result<P2PSyncReport> {
    let mut merged = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut merged,
        30_000,
        true,
        "converged-sync-workflow",
    )? {
        Some(guard) => guard,
        None => return finish_report(settings, merged),
    };
    let rounds = rounds.max(1);
    let mut stable_rounds = 0usize;
    let mut last_tip = load_chain_for_hf90_catchup(settings)
        .map(|c| c.tip_hash().to_string())
        .unwrap_or_default();

    // HF68/v1.5.2 fixed2: first try the tiny official missing-suffix path.
    // The full checkpoint repair is heavier and should not be needed when the
    // official seeds are only one/few blocks ahead of local.
    if matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        if let Ok(suffix_report) = sync_official_suffix(settings, 8_000) {
            merge_sync_reports(&mut merged, suffix_report);
            last_tip = load_chain_for_hf90_catchup(settings)
                .map(|c| c.tip_hash().to_string())
                .unwrap_or(last_tip);
        }
        let local_after_suffix = load_chain_for_hf90_catchup(settings).ok();
        let local_h = local_after_suffix.as_ref().map(|c| c.height()).unwrap_or(0);
        let local_tip = local_after_suffix
            .as_ref()
            .map(|c| c.tip_hash().to_string())
            .unwrap_or_default();
        let official = official_http_tip(settings, 2_500).ok().flatten();
        let official_h = official.as_ref().map(|(h, _)| *h).unwrap_or_else(|| {
            best_official_peer_tip(settings, 900)
                .map(|(_, h, _)| h)
                .unwrap_or(0)
        });
        let official_tip = official
            .as_ref()
            .map(|(_, h)| h.clone())
            .unwrap_or_default();
        let official_same_height_differs =
            official_h == local_h && !official_tip.trim().is_empty() && official_tip != local_tip;
        if official_h > local_h || official_same_height_differs {
            let before_tail_h = local_h;
            let before_tail_tip = local_tip.clone();
            let mut tail_failed = false;
            if let Ok(tail_report) = sync_official_http_tail(settings, 12_000) {
                merge_sync_reports(&mut merged, tail_report);
                last_tip = load_chain_for_hf90_catchup(settings)
                    .map(|c| c.tip_hash().to_string())
                    .unwrap_or(last_tip);
            } else {
                tail_failed = true;
            }
            let local_after_tail = load_chain_for_hf90_catchup(settings).ok();
            let local_tail_h = local_after_tail
                .as_ref()
                .map(|c| c.height())
                .unwrap_or(local_h);
            let local_tail_tip = local_after_tail
                .as_ref()
                .map(|c| c.tip_hash().to_string())
                .unwrap_or(local_tip);
            let still_differs = official_h == local_tail_h
                && !official_tip.trim().is_empty()
                && official_tip != local_tail_tip;
            let still_behind = official_h > local_tail_h;
            let no_tail_progress =
                local_tail_h == before_tail_h && local_tail_tip == before_tail_tip;
            let gap = official_h.saturating_sub(local_tail_h);
            if still_differs || (still_behind && (tail_failed || no_tail_progress || gap > 4096)) {
                if let Ok(http_report) = sync_official_http_snapshot(settings, 90_000) {
                    merge_sync_reports(&mut merged, http_report);
                    last_tip = load_chain_for_hf90_catchup(settings)
                        .map(|c| c.tip_hash().to_string())
                        .unwrap_or(last_tip);
                }
            }
        }
    }

    for _ in 0..rounds {
        let before_chain = load_chain_for_hf90_catchup(settings).ok();
        let before_tip = before_chain
            .as_ref()
            .map(|c| c.tip_hash().to_string())
            .unwrap_or_default();
        let before_height = before_chain.as_ref().map(|c| c.height()).unwrap_or(0);
        let report = sync_quick(
            settings,
            settings.p2p.max_outbound_peers.max(8).min(16),
            12_000,
        )?;
        merged.peers_contacted = merged
            .peers_contacted
            .saturating_add(report.peers_contacted);
        merged.peer_errors = merged.peer_errors.saturating_add(report.peer_errors);
        hf129_observe_tip(&mut merged, report.best_peer_height, &report.best_peer_tip_hash);
        merged.chains_adopted = merged.chains_adopted.saturating_add(report.chains_adopted);
        merged.blocks_connected = merged
            .blocks_connected
            .saturating_add(report.blocks_connected);
        merged.txs_accepted = merged.txs_accepted.saturating_add(report.txs_accepted);
        merged.height = report.height;
        merged.tip_hash = report.tip_hash.clone();

        let after_tip = merged.tip_hash.clone();
        let after_height = load_chain_for_hf90_catchup(settings)
            .map(|c| c.height())
            .unwrap_or(before_height);
        // Do not declare sync stable while a directly contacted peer is still ahead.
        if report.best_peer_height > after_height {
            stable_rounds = 0;
        } else if after_tip == before_tip
            && after_tip == last_tip
            && report.chains_adopted == 0
            && report.blocks_connected == 0
        {
            stable_rounds = stable_rounds.saturating_add(1);
            if stable_rounds >= 2 {
                break;
            }
        } else {
            stable_rounds = 0;
        }
        last_tip = after_tip;
        if sleep_ms > 0 {
            thread::sleep(Duration::from_millis(sleep_ms));
        }
    }

    finish_report(settings, merged)
}

fn merge_sync_reports(into: &mut P2PSyncReport, other: P2PSyncReport) {
    into.peers_contacted = into.peers_contacted.saturating_add(other.peers_contacted);
    into.peer_errors = into.peer_errors.saturating_add(other.peer_errors);
    // Keep height and hash as one observation. Updating the max first made the
    // comparison below unable to recognize a genuinely higher target and could
    // retain a hash belonging to an older, lower-height peer.
    hf129_observe_tip(into, other.best_peer_height, &other.best_peer_tip_hash);
    if let Some(proof) = other.canonical_proof.as_ref() {
        if into.canonical_proof.as_ref().map_or(true, |old| proof.observed_at > old.observed_at) {
            into.canonical_proof = Some(proof.clone());
        }
    }
    into.canonical_tip_proven = into.canonical_proof.is_some();
    into.chains_adopted = into.chains_adopted.saturating_add(other.chains_adopted);
    into.blocks_connected = into.blocks_connected.saturating_add(other.blocks_connected);
    into.txs_accepted = into.txs_accepted.saturating_add(other.txs_accepted);
    if other.height > into.height
        || (other.height == into.height && !other.tip_hash.trim().is_empty())
    {
        into.height = other.height;
        into.tip_hash = other.tip_hash;
    }
}

fn adaptive_from_heights(local_height: u32, peer_height: u32, peer_tip_differs: bool) -> Vec<u32> {
    let mut out = Vec::<u32>::new();
    let mut seen = HashSet::<u32>::new();
    let push = |h: u32, out: &mut Vec<u32>, seen: &mut HashSet<u32>| {
        if seen.insert(h) {
            out.push(h);
        }
    };

    // Always try the normal missing suffix first.
    push(local_height.saturating_add(1), &mut out, &mut seen);

    if peer_height > local_height || peer_tip_differs {
        for window in ADAPTIVE_SYNC_WINDOWS.iter().copied() {
            let from = local_height.saturating_sub(window);
            push(from, &mut out, &mut seen);
            if from == 0 {
                break;
            }
        }
    }
    out
}

fn send_adaptive_chain_requests(
    stream: &mut TcpStream,
    local: &ChainState,
    peer_height: u32,
    peer_tip_hash: &str,
) -> Result<()> {
    let peer_tip_differs =
        peer_height == local.height() && peer_tip_hash != local.tip_hash().to_string();
    let from_height = if peer_height > local.height() {
        local.height().saturating_add(1)
    } else if peer_tip_differs {
        local.height().saturating_sub(8)
    } else {
        return Ok(());
    };
    send_wire(stream, &WireMessage::GetChain { from_height })?;
    Ok(())
}

fn request_peer_chain_if_needed(
    stream: &mut TcpStream,
    local: &ChainState,
    peer_height: u32,
    peer_tip: &str,
    session: &mut PeerSession,
) -> Result<bool> {
    let local_tip = local.tip_hash().to_string();
    let peer_differs = peer_height == local.height() && peer_tip != local_tip;
    let from_height = if peer_height > local.height() {
        local.height().saturating_add(1)
    } else if peer_differs {
        local.height().saturating_sub(8)
    } else {
        return Ok(false);
    };
    // An advancing advertisement does not invalidate the suffix already in
    // flight from this exact local parent.
    let same_view = session.sync_local_height == local.height()
        && session.sync_local_tip == local_tip
        && session.sync_from_height <= from_height;
    if same_view
        && session
            .sync_requested_at
            .map(|at| at.elapsed() < Duration::from_secs(30))
            .unwrap_or(false)
    {
        return Ok(false);
    }
    request_peer_chain_from(
        stream,
        local,
        peer_height,
        peer_tip,
        from_height,
        session,
    )?;
    Ok(true)
}

fn request_peer_chain_from(
    stream: &mut TcpStream,
    local: &ChainState,
    peer_height: u32,
    peer_tip: &str,
    from_height: u32,
    session: &mut PeerSession,
) -> Result<()> {
    session.sync_peer_height = peer_height;
    session.sync_peer_tip = peer_tip.to_string();
    session.sync_local_height = local.height();
    session.sync_local_tip = local.tip_hash().to_string();
    session.sync_from_height = from_height;
    session.sync_requested_at = Some(Instant::now());
    send_wire(stream, &WireMessage::GetChain { from_height })
}

fn request_earlier_peer_chain(
    stream: &mut TcpStream,
    local: &ChainState,
    session: &mut PeerSession,
    previous_from: u32,
) -> Result<bool> {
    if previous_from == 0 {
        return Ok(false);
    }
    let overlap = local.height().saturating_sub(previous_from).max(2);
    let next_window = overlap.saturating_mul(4).min(local.height().max(1));
    let mut next_from = local.height().saturating_sub(next_window);
    if next_from >= previous_from {
        next_from = previous_from.saturating_sub(1);
    }
    let peer_height = session.height;
    let peer_tip = session.tip_hash.clone();
    request_peer_chain_from(
        stream,
        local,
        peer_height,
        &peer_tip,
        next_from,
        session,
    )?;
    Ok(true)
}

fn hf132_chunk_already_known(local: &ChainState, start_height: u32, blocks: &[Block]) -> bool {
    let start = start_height as usize;
    start.checked_add(blocks.len()).and_then(|end| local.blocks.get(start..end))
        .map_or(false, |known| known.iter().zip(blocks)
            .all(|(a, b)| a.block_hash() == b.block_hash()))
}

fn try_adopt_overlapping_blocks(
    local: &mut ChainState,
    start_height: u32,
    blocks: Vec<Block>,
    settings: &Settings,
    prefer_peer_on_equal_work: bool,
) -> Result<bool> {
    if blocks.is_empty() {
        return Ok(false);
    }
    if start_height == 0 {
        return local.try_adopt_peer_chain(blocks, settings, prefer_peer_on_equal_work);
    }
    let start = start_height as usize;
    if start > local.blocks.len() {
        return Ok(false);
    }

    // A byte-identical duplicate is not a fork and needs no replay. A true
    // overlap extension still takes normal from_blocks validation below.
    if hf132_chunk_already_known(local, start_height, &blocks) { return Ok(false); }
    // A matching overlap followed by new blocks is still an append. Retain
    // the already validated local prefix and validate EVERY new block. Full
    // Block equality also checks the supplied body, not only its header hash.
    let overlap = local.blocks.len() - start;
    if overlap < blocks.len()
        && local.blocks[start..].iter().zip(&blocks).all(|(known, incoming)| known == incoming)
    {
        let mut candidate = local.clone();
        for block in blocks.into_iter().skip(overlap) {
            hf132_check_sync_budget()?;
            candidate.connect_block(block, settings)?;
        }
        *local = candidate;
        return Ok(true);
    }

    let mut candidate_blocks = local.blocks[..start].to_vec();
    candidate_blocks.extend(blocks);

    let local_height_before = local.height();
    // Replay a real competing branch once. Retain the validated candidate for
    // the existing checkpoint repair policy instead of cloning/replaying all
    // historical blocks a second time after a normal work comparison fails.
    let candidate = ChainState::from_blocks(candidate_blocks, settings)?;
    let normal = local.try_adopt_validated_peer_chain(
        candidate.clone(),
        settings,
        prefer_peer_on_equal_work,
    )?;
    if normal {
        return Ok(true);
    }

    // HF60/v1.5.2: forced checkpoint-anchored repair. After DAA #10500 a stale
    // local suffix can keep enough accumulated work to block the normal adoption
    // rule, even though the official direct seed is clearly ahead. This is why
    // users saw Sync loop forever and fresh reinstall fixed it. If the peer sent
    // a chain suffix that starts at/before the hard checkpoint boundary, and the
    // candidate validates and is ahead, replace the local post-checkpoint suffix.
    if settings.network.name == "mainnet"
        && start_height <= MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT.saturating_add(1)
    {
        let cp = MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT as usize;
        let checkpoint_matches = candidate.blocks.len() > cp
            && local.blocks.len() > cp
            && candidate.blocks[cp].block_hash() == local.blocks[cp].block_hash();
        if checkpoint_matches && candidate.height() > local_height_before {
            let mut candidate = candidate;
            let keep_mempool = local.reorg_mempool_candidates_for(&candidate);
            candidate.rebuild_mempool_from(keep_mempool, settings);
            *local = candidate;
            return Ok(true);
        }
    }

    Ok(false)
}

fn request_earlier_fork_window(
    stream: &mut TcpStream,
    local_height: u32,
    start_height: u32,
) -> Result<()> {
    // If a returned overlap still cannot connect/adopt, widen the fork window.
    let delta = local_height.saturating_sub(start_height).max(2);
    let next_window = delta.saturating_mul(4).min(8192);
    let next_from = local_height.saturating_sub(next_window);
    send_wire(
        stream,
        &WireMessage::GetHeaders {
            from_height: next_from,
        },
    )?;
    send_wire(
        stream,
        &WireMessage::GetChain {
            from_height: next_from,
        },
    )?;
    Ok(())
}

fn direct_parent_view(
    settings: &Settings,
    parent_height: u32,
    expected_parent_hash: &str,
    max_peers: usize,
    timeout_ms: u64,
) -> Result<(usize, u32, Vec<String>)> {
    let deadline = hf130_effective_deadline(Instant::now() + Duration::from_millis(timeout_ms));
    // This view includes configured peers as before, but no silent first row
    // can consume the entire observation deadline and aliases count once.
    let peers = known_peers(settings)?.into_iter().take(max_peers.clamp(1, 16)).collect::<Vec<_>>();
    let version = hf131_probe_version(settings)?;
    let observations = hf131_probe_batch(&peers, deadline, |addr, left| probe_peer_with_version(settings, addr, left, &version).ok());
    let view = hf131_parent_view(parent_height, expected_parent_hash, observations);
    if settings.network.name == "mainnet" {
        Ok((view.contacted, view.ahead_height, view.conflicts))
    } else {
        Ok((view.contacted, view.raw_ahead_height, view.raw_conflicts))
    }
}

fn force_official_tip_if_ahead(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &mut ChainState,
    timeout_ms: u64,
) -> Result<bool> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_millis(timeout_ms));
    let (_, mut official_height, mut official_tip) = official_tip_summary(settings, report, 420);
    if let Ok(Some((http_height, http_tip))) = official_http_tip(settings, 900) {
        report.peers_contacted = report.peers_contacted.max(1);
        hf129_observe_tip(report, http_height, &http_tip);
        if http_height >= official_height || official_tip.trim().is_empty() {
            official_height = http_height;
            official_tip = http_tip;
        }
    }

    if hf114_official_tip_acknowledges_local(settings, local, official_height, &official_tip) {
        mark_fresh_tip_trusted(settings, local);
        return Ok(false);
    }

    // A published lower ancestor does not erase a validated higher-work suffix.
    // A locally mined suffix without acknowledgement still waits for HF125 relay
    // confirmation, exactly as before HF126.
    let local_ahead = hf114_official_tip_is_local_ancestor(
        settings,
        local,
        official_height,
        &official_tip,
    );
    if settings.network.name == "mainnet" && local_ahead {
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();
        if pending_block_relay_waiting(settings, local) {
            bail!(
                "mining paused: valid local tip #{} {} is awaiting explicit peer acknowledgement; HF125 automatic block-delivery retry is active",
                local.height(),
                local.tip_hash()
            );
        }
        mark_fresh_tip_trusted(settings, local);
        return Ok(false);
    }

    if official_height > local.height()
        && hf97_uncatchable_tip_quarantined(settings, local, official_height)
    {
        report.best_peer_height = local.height();
        report.best_peer_tip_hash.clear();
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();
        mark_fresh_tip_trusted(settings, local);
        return Ok(false);
    }

    // HF126/v1.8.2: durable stale-parent acknowledgements may know the
    // competing tip even while one official probe is temporarily unavailable.
    // Try the strict multi-source/full-replay re-anchor before classifying the
    // remaining state as an ordinary mineable tie.
    if pending_block_relay_waiting(settings, local)
        && (official_height == 0 || official_height == local.height())
    {
        let before_tip = local.tip_hash();
        let repair = sync_official_http_tail_reanchor_hf114(
            settings,
            timeout_ms.max(HF82_PARENT_CATCHUP_MS).min(45_000),
        )?;
        merge_sync_reports(report, repair);
        *local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        if local.tip_hash() != before_tip {
            mark_fresh_tip_trusted(settings, local);
            return Ok(false);
        }
    }

    let same_height_conflict = hf126_is_same_height_competing_tip(
        local.height(),
        &local.tip_hash().to_string(),
        official_height,
        &official_tip,
    );

    // If strict re-anchor evidence or transport is temporarily unavailable,
    // equal-height competition remains a normal PoW tie and must not deadlock
    // every miner.
    if same_height_conflict {
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();
        eprintln!(
            "HF126 equal-height PoW tie at #{}: local={} observed={}; continuing on the active validated tip until cumulative work or verified re-anchor resolves it",
            local.height(),
            local.tip_hash(),
            official_tip,
        );
        mark_fresh_tip_trusted(settings, local);
        return Ok(true);
    }

    if official_height > local.height() {
        let heal = hf90_mining_catchup(settings, timeout_ms.max(HF82_PARENT_CATCHUP_MS))?;
        merge_sync_reports(report, heal);
        *local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;

        if hf104_canonical_greenlight(settings, report, local, HF82_LIGHT_TIP_PROBE_MS.max(700)) {
            return Ok(false);
        }

        let (_, mut latest_height, mut latest_tip) =
            official_tip_summary(settings, report, 350);
        if let Ok(Some((http_height, http_tip))) = official_http_tip(settings, 900) {
            report.peers_contacted = report.peers_contacted.max(1);
            hf129_observe_tip(report, http_height, &http_tip);
            if http_height >= latest_height || latest_tip.trim().is_empty() {
                latest_height = http_height;
                latest_tip = http_tip;
            }
        }

        if hf114_official_tip_acknowledges_local(settings, local, latest_height, &latest_tip) {
            mark_fresh_tip_trusted(settings, local);
            return Ok(false);
        }

        let latest_local_ahead = hf114_official_tip_is_local_ancestor(
            settings,
            local,
            latest_height,
            &latest_tip,
        );
        if latest_local_ahead {
            if pending_block_relay_waiting(settings, local) {
                bail!(
                    "mining paused: valid local tip #{} {} is awaiting explicit peer acknowledgement; HF125 automatic block-delivery retry is active",
                    local.height(),
                    local.tip_hash()
                );
            }
            mark_fresh_tip_trusted(settings, local);
            return Ok(false);
        }

        if latest_height > local.height() {
            if hf97_greenlight_local_tip_after_uncatchable_height(
                settings,
                report,
                local,
                latest_height,
                "mining-greenlight-official-ahead",
            )? {
                return Ok(false);
            }
            bail!(
                "mining paused: reachable chain height {} is {} block(s) ahead of local height {}; detached catch-up remains active",
                latest_height,
                latest_height.saturating_sub(local.height()),
                local.height()
            );
        }

        if hf126_is_same_height_competing_tip(
            local.height(),
            &local.tip_hash().to_string(),
            latest_height,
            &latest_tip,
        ) {
            mark_fresh_tip_trusted(settings, local);
            return Ok(true);
        }
    }

    Ok(false)
}

/// HF127/v1.8.3 transaction construction guard. Transactions are branch-
/// sensitive because their inputs may differ across competing tips. Unlike
/// mining, an equal-height sibling is not safe for signing: require an exact
/// current-tip acknowledgement from an official source or a two-peer direct
/// quorum, and run the bounded repair ladder only when the quick view disagrees.
pub fn hf127_transaction_parent_guard(
    settings: &Settings,
    repair_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled
        || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
        return finish_report(settings, report);
    }

    let inspect = |local: &ChainState, report: &mut P2PSyncReport| -> (bool, bool, Vec<String>) {
        let local_height = local.height();
        let local_tip = local.tip_hash().to_string();
        let mut exact = false;
        let mut needs_repair = false;
        let mut conflicts = Vec::<String>::new();

        if let Ok((contacted, height, tip)) = best_official_peer_tip(settings, 900) {
            report.peers_contacted = report.peers_contacted.max(contacted);
            hf129_observe_tip(report, height, "");
            if contacted > 0 {
                if height > local_height {
                    needs_repair = true;
                    conflicts.push(format!(
                        "official TCP tip #{} is ahead of local #{}",
                        height, local_height
                    ));
                } else if height == local_height && !tip.trim().is_empty() {
                    if tip == local_tip {
                        exact = true;
                    } else {
                        needs_repair = true;
                        conflicts.push(format!(
                            "official TCP reports sibling {} at #{}",
                            tip, height
                        ));
                    }
                }
            }
        }

        if let Ok(Some((height, tip))) = official_http_tip(settings, 1_800) {
            report.peers_contacted = report.peers_contacted.max(1);
            hf129_observe_tip(report, height, "");
            if height > local_height {
                needs_repair = true;
                conflicts.push(format!(
                    "official HTTP tip #{} is ahead of local #{}",
                    height, local_height
                ));
            } else if height == local_height && !tip.trim().is_empty() {
                if tip == local_tip {
                    exact = true;
                } else {
                    needs_repair = true;
                    conflicts.push(format!(
                        "official HTTP reports sibling {} at #{}",
                        tip, height
                    ));
                }
            }
        }

        if !needs_repair
            && !exact
            && hf115_direct_parent_ack_quorum(
                settings,
                local_height,
                &local_tip,
                12,
                650,
            )
        {
            exact = true;
        }

        (exact, needs_repair, conflicts)
    };

    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    let (exact, needs_repair, first_conflicts) = inspect(&local, &mut report);
    if exact && !needs_repair {
        mark_fresh_tip_trusted(settings, &local);
        return finish_report(settings, report);
    }

    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut report,
        30_000,
        true,
        "transaction-repair-workflow",
    )? {
        Some(guard) => guard,
        None => bail!("transaction creation paused: canonical sync writer is busy"),
    };

    let repaired = hf90_auto_catchup(settings, repair_timeout_ms.max(60_000))?;
    merge_sync_reports(&mut report, repaired);
    local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    let (exact_after, needs_repair_after, second_conflicts) = inspect(&local, &mut report);
    if exact_after && !needs_repair_after {
        mark_fresh_tip_trusted(settings, &local);
        return finish_report(settings, report);
    }

    let detail = first_conflicts
        .into_iter()
        .chain(second_conflicts)
        .take(6)
        .collect::<Vec<_>>()
        .join("; ");
    bail!(
        "transaction creation paused: local tip #{} {} is not exactly acknowledged by the current official/direct network view{}",
        local.height(),
        local.tip_hash(),
        if detail.is_empty() {
            String::new()
        } else {
            format!(" ({detail})")
        }
    )
}

/// HF54/v1.5.2 hard mining template gate. It runs immediately before
/// building a candidate and again before submitting a found block. It fails
/// closed on a genuinely higher validated tip, while HF126 keeps ordinary
/// equal-height PoW sibling ties mineable. Registry-only telemetry is not used
/// as a fork-choice input.
pub fn mining_parent_guard(
    settings: &Settings,
    parent_height: u32,
    expected_parent_hash: Hash256,
) -> Result<P2PSyncReport> {
    let _hf130_guard_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(30));
    let expected_parent_hash_s = expected_parent_hash.to_string();

    // HF71/v1.5.8: if this exact local tip was just validated/repaired from
    // official seeds, skip the expensive guard cascade for a short window.
    if settings.p2p.enabled && matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        if let Ok(local) = load_chain_for_hf90_catchup(settings) {
            if local.height() == parent_height && local.tip_hash() == expected_parent_hash {
                let mut report = P2PSyncReport::default();
                if hf104_canonical_greenlight(
                    settings,
                    &mut report,
                    &local,
                    HF82_LIGHT_TIP_PROBE_MS,
                ) {
                    report.peers_contacted = report.peers_contacted.max(1);
                    report.height = local.height();
                    report.tip_hash = local.tip_hash().to_string();
                    hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
                    return finish_report(settings, report);
                }
            }
        }
    }

    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
        let local = load_chain_for_hf90_catchup(settings)?;
        if local.height() != parent_height || local.tip_hash() != expected_parent_hash {
            bail!(
                "mining candidate stale before submit: local tip is #{} {}, candidate parent is #{} {}",
                local.height(),
                local.tip_hash(),
                parent_height,
                expected_parent_hash_s
            );
        }
        hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
        return finish_report(settings, P2PSyncReport::default());
    }

    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "mining-parent-workflow",
    )? {
        Some(guard) => guard,
        None => bail!("mining candidate paused: canonical sync writer is busy"),
    };

    let mut report = hf90_mining_catchup(settings, 90_000).unwrap_or(workflow_report);
    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if (hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS))
        && local.height() == parent_height
        && local.tip_hash() == expected_parent_hash
    {
        hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
        return finish_report(settings, report);
    }
    let quick = sync_quick(
        settings,
        settings.p2p.max_outbound_peers.max(6).min(12),
        3_500,
    )?;
    merge_sync_reports(&mut report, quick);
    local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    let equal_height_tie = force_official_tip_if_ahead(
        settings,
        &mut report,
        &mut local,
        HF80_MINING_PARENT_GATE_MS,
    )?;
    if settings.network.name == "mainnet"
        && !equal_height_tie
        && !hf104_canonical_greenlight(
            settings,
            &mut report,
            &local,
            HF82_LIGHT_TIP_PROBE_MS.max(700),
        )
    {
        bail!("mining candidate paused by HF116: local parent #{} is not acknowledged by official/direct canonical view or exact peer quorum.", local.height());
    }

    if local.height() != parent_height || local.tip_hash() != expected_parent_hash {
        bail!(
            "mining candidate stale after sync: local tip is #{} {}, candidate parent is #{} {}. Rebuilding from fresh tip.",
            local.height(),
            local.tip_hash(),
            parent_height,
            expected_parent_hash_s
        );
    }

    if report.peers_contacted == 0
        && !hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS)
    {
        bail!("mining paused: no official/direct TCP seed reachable during candidate guard. Keep QUB Core open; it will keep discovering seeds and retrying.");
    }

    hf97_suppress_quarantined_best_height(settings, &mut report, &local);
    if report.best_peer_height > parent_height {
        let retry = catch_up_to_direct_height(
            settings,
            report.best_peer_height.max(parent_height.saturating_add(1)),
            MINING_GUARD_CATCHUP_ROUNDS.saturating_add(2),
            MINING_GUARD_CATCHUP_SLEEP_MS,
        )?;
        merge_sync_reports(&mut report, retry);
        local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        if local.height() != parent_height || local.tip_hash() != expected_parent_hash {
            bail!(
                "network advanced to #{} {}; candidate parent #{} {} was discarded before submit.",
                local.height(),
                local.tip_hash(),
                parent_height,
                expected_parent_hash_s
            );
        }
        if report.best_peer_height > parent_height {
            if hf97_uncatchable_tip_quarantined(settings, &local, report.best_peer_height) {
                report.best_peer_height = parent_height;
                report.best_peer_tip_hash.clear();
            } else {
                bail!(
                    "mining paused: direct peer height {} is ahead of candidate parent #{}. Waiting for local chain catch-up before building/submitting blocks.",
                    report.best_peer_height,
                    parent_height
                );
            }
        }
    }

    let (direct_contacted, best_direct_height, conflicts) = direct_parent_view(
        settings,
        parent_height,
        &expected_parent_hash_s,
        settings.p2p.max_outbound_peers.max(8).min(16),
        450,
    )?;
    report.peers_contacted = report.peers_contacted.max(direct_contacted);
    hf129_observe_tip(&mut report, best_direct_height, "");

    if best_direct_height > parent_height
        && !hf97_uncatchable_tip_quarantined(settings, &local, best_direct_height)
    {
        let retry = catch_up_to_direct_height(
            settings,
            best_direct_height.max(parent_height.saturating_add(1)),
            MINING_GUARD_CATCHUP_ROUNDS.saturating_add(6),
            MINING_GUARD_CATCHUP_SLEEP_MS,
        )?;
        merge_sync_reports(&mut report, retry);
        local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        if local.height() != parent_height || local.tip_hash() != expected_parent_hash {
            bail!(
                "network advanced to #{} {}; candidate parent #{} {} was discarded. Rebuilding from synced canonical tip.",
                local.height(),
                local.tip_hash(),
                parent_height,
                expected_parent_hash_s
            );
        }
        let (retry_contacted, retry_best_height, retry_conflicts) = direct_parent_view(
            settings,
            parent_height,
            &expected_parent_hash_s,
            settings.p2p.max_outbound_peers.max(8).min(16),
            650,
        )?;
        report.peers_contacted = report.peers_contacted.max(retry_contacted);
        hf129_observe_tip(&mut report, retry_best_height, "");
        if retry_best_height > parent_height {
            if hf97_uncatchable_tip_quarantined(settings, &local, retry_best_height) {
                report.best_peer_height = parent_height;
                report.best_peer_tip_hash.clear();
            } else {
                bail!(
                    "mining paused: direct peer height {} is ahead of candidate parent #{}. Adaptive sync is still catching up; candidate will be rebuilt automatically.",
                    retry_best_height,
                    parent_height
                );
            }
        }
        if !retry_conflicts.is_empty() && settings.network.name != "mainnet" {
            bail!(
                "mining paused: direct peer(s) disagree at candidate parent #{}: {}. Candidate discarded; sync/repair first.",
                parent_height,
                retry_conflicts.join(", ")
            );
        }
    }

    if !conflicts.is_empty() && settings.network.name != "mainnet" {
        bail!(
            "mining paused: direct peer(s) disagree at candidate parent #{}: {}. Candidate discarded; sync/repair first.",
            parent_height,
            conflicts.join(", ")
        );
    }

    if let Ok(local) = load_chain_for_hf90_catchup(settings) {
        if local.height() == parent_height && local.tip_hash() == expected_parent_hash {
            mark_fresh_tip_trusted(settings, &local);
        }
    }
    hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
    finish_report(settings, report)
}

/// HF113/v1.7.1: fast submit guard used after a nonce was found. Do not run a
/// heavy repair here; every second spent verifying before relay increases stale
/// risk, especially for pool blocks with multiple transactions. The outer miner
/// loop will perform deep repair/rebuild after this rejects a stale candidate.
pub fn mining_parent_submit_guard(
    settings: &Settings,
    parent_height: u32,
    expected_parent_hash: Hash256,
) -> Result<P2PSyncReport> {
    let _hf130_guard_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(15));
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
        hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
        return finish_report(settings, report);
    }
    let local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if local.height() != parent_height || local.tip_hash() != expected_parent_hash {
        bail!(
            "mining submit stale: local tip is #{} {}, candidate parent is #{} {}",
            local.height(),
            local.tip_hash(),
            parent_height,
            expected_parent_hash
        );
    }
    if let Some(reason) =
        hf113_live_tip_pause_reason(settings, parent_height, expected_parent_hash, 520)
    {
        bail!("mining submit paused by fast canonical guard: {reason}");
    }
    let (direct_contacted, best_direct_height, conflicts) = direct_parent_view(
        settings,
        parent_height,
        &expected_parent_hash.to_string(),
        settings.p2p.max_outbound_peers.max(6).min(10),
        360,
    )?;
    report.peers_contacted = report.peers_contacted.max(direct_contacted);
    hf129_observe_tip(&mut report, best_direct_height, "");
    if !conflicts.is_empty() && settings.network.name != "mainnet" {
        bail!(
            "mining submit paused: direct peer(s) disagree at candidate parent #{}: {}",
            parent_height,
            conflicts.join(", ")
        );
    }
    if best_direct_height > parent_height {
        bail!(
            "mining submit paused: direct peer height {} is ahead of candidate parent #{}",
            best_direct_height,
            parent_height
        );
    }
    mark_fresh_tip_trusted(settings, &local);
    hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
    finish_report(settings, report)
}

/// HF49 fork-safety guard used by CLI/GUI mining entrypoints. It deliberately
/// trusts only locally validated chain data and direct TCP peers. Unreachable
/// registry-only high-tip reports are useful telemetry, but they are not enough
/// to make a node mine on a different branch.
pub fn mining_safety_check(settings: &Settings) -> Result<P2PSyncReport> {
    let _hf130_guard_scope = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(60));
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
        hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
        return finish_report(settings, P2PSyncReport::default());
    }

    // HF71/v1.5.8: when the chain was just repaired/validated from official
    // seeds, do not immediately redo the expensive mining guard. This is the
    // main source of the Ready->Syncing->Ready oscillation users saw before
    // actual hashing started.
    if let Ok(local) = load_chain_for_hf90_catchup(settings) {
        let mut report = P2PSyncReport::default();
        if hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS) {
            report.peers_contacted = report.peers_contacted.max(1);
            report.height = local.height();
            report.tip_hash = local.tip_hash().to_string();
            hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
            return finish_report(settings, report);
        }
    }

    let mut workflow_report = P2PSyncReport::default();
    let _workflow_guard = match hf129_acquire_sync_step(
        settings,
        &mut workflow_report,
        30_000,
        true,
        "mining-safety-workflow",
    )? {
        Some(guard) => guard,
        None => bail!("mining paused: canonical sync writer is busy"),
    };

    let mut report = hf90_mining_catchup(settings, 120_000).unwrap_or(workflow_report);
    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS) {
        hf130_io_remaining(Instant::now() + Duration::from_secs(1))?;
        return finish_report(settings, report);
    }
    let quick = sync_quick(
        settings,
        settings.p2p.max_outbound_peers.max(8).min(16),
        4_500,
    )?;
    merge_sync_reports(&mut report, quick);
    local = load_chain_for_hf90_catchup(settings)?;

    // HF53: if a fresh install or lagging miner is still behind the checkpoint,
    // do a longer worker-thread catch-up before pausing. This avoids asking users
    // to reinstall just to obtain the already-known canonical chain suffix.
    if settings.network.name == "mainnet" && local.height() < MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT
    {
        let catchup = catch_up_to_direct_height(
            settings,
            MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT,
            MINING_GUARD_CATCHUP_ROUNDS,
            MINING_GUARD_CATCHUP_SLEEP_MS,
        )?;
        report.peers_contacted = report
            .peers_contacted
            .saturating_add(catchup.peers_contacted);
        report.peer_errors = report.peer_errors.saturating_add(catchup.peer_errors);
        hf129_observe_tip(&mut report, catchup.best_peer_height, &catchup.best_peer_tip_hash);
        report.chains_adopted = report.chains_adopted.saturating_add(catchup.chains_adopted);
        report.blocks_connected = report
            .blocks_connected
            .saturating_add(catchup.blocks_connected);
        report.txs_accepted = report.txs_accepted.saturating_add(catchup.txs_accepted);
        report.height = catchup.height;
        report.tip_hash = catchup.tip_hash;
        local = load_chain_for_hf90_catchup(settings)?;
    }

    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    let equal_height_tie =
        force_official_tip_if_ahead(settings, &mut report, &mut local, HF80_MINING_FAST_GATE_MS)?;
    if settings.network.name == "mainnet"
        && !equal_height_tie
        && !hf104_canonical_greenlight(
            settings,
            &mut report,
            &local,
            HF82_LIGHT_TIP_PROBE_MS.max(700),
        )
    {
        bail!("mining paused by HF116: local tip #{} is not acknowledged by official/direct canonical view or exact peer quorum. This prevents all-self-mined local stale branches.", local.height());
    }
    if hf97_uncatchable_tip_quarantined(settings, &local, report.best_peer_height) {
        report.best_peer_height = local.height();
        report.best_peer_tip_hash.clear();
    }
    mark_fresh_tip_trusted(settings, &local);

    if settings.network.name == "mainnet" && local.height() < MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT
    {
        bail!(
            "mining paused: catching up to mainnet checkpoint #{} ({}). Current height #{}. QUB Core will keep syncing automatically; only use chain-only repair if this does not progress.",
            MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT,
            MAINNET_FORK_SAFETY_CHECKPOINT_HASH,
            local.height()
        );
    }

    if report.peers_contacted == 0
        && !hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS)
    {
        bail!("mining paused: no official/direct TCP seed reachable. Keep QUB Core open; it will keep discovering seeds and retrying.");
    }

    let mut local_height = local.height();
    let mut local_tip = local.tip_hash().to_string();
    if report.best_peer_height > local_height
        && !hf97_uncatchable_tip_quarantined(settings, &local, report.best_peer_height)
    {
        let retry = catch_up_to_direct_height(
            settings,
            report
                .best_peer_height
                .max(local.height().saturating_add(1)),
            MINING_GUARD_CATCHUP_ROUNDS,
            MINING_GUARD_CATCHUP_SLEEP_MS,
        )?;
        report.peers_contacted = report.peers_contacted.saturating_add(retry.peers_contacted);
        report.peer_errors = report.peer_errors.saturating_add(retry.peer_errors);
        hf129_observe_tip(&mut report, retry.best_peer_height, &retry.best_peer_tip_hash);
        report.chains_adopted = report.chains_adopted.saturating_add(retry.chains_adopted);
        report.blocks_connected = report
            .blocks_connected
            .saturating_add(retry.blocks_connected);
        report.txs_accepted = report.txs_accepted.saturating_add(retry.txs_accepted);
        report.height = retry.height;
        report.tip_hash = retry.tip_hash;
        let retry_local = load_chain_for_hf90_catchup(settings)?;
        if retry.best_peer_height > retry_local.height() {
            if hf97_uncatchable_tip_quarantined(settings, &retry_local, retry.best_peer_height) {
                report.best_peer_height = retry_local.height();
                report.best_peer_tip_hash.clear();
            } else {
                bail!(
                    "mining paused: direct peer height {} is still ahead of local height {} after background catch-up. QUB Core will keep retrying automatically; keep it open unless this stays unchanged for a long time.",
                    retry.best_peer_height,
                    retry_local.height()
                );
            }
        }
        local_height = retry_local.height();
        local_tip = retry_local.tip_hash().to_string();
    }

    // HF88: never call the full peer_status() GUI probe inside the mining gate;
    // it can block green-light startup on NAT-heavy networks. A bounded direct
    // parent check is enough to reject same-height conflicts while allowing
    // official-HTTP-greenlit miners to hash.
    let (direct_contacted, best_direct_height, conflicting_direct) = direct_parent_view(
        settings,
        local_height,
        &local_tip,
        settings.p2p.max_outbound_peers.max(4).min(8),
        350,
    )?;
    report.peers_contacted = report.peers_contacted.max(direct_contacted);
    hf129_observe_tip(&mut report, best_direct_height, "");
    if !conflicting_direct.is_empty() && settings.network.name != "mainnet" {
        bail!(
            "mining paused: direct peer(s) report a different hash at local height #{}: {}. Do not mine until branches converge.",
            local_height,
            conflicting_direct.join(", ")
        );
    }
    if best_direct_height > local_height {
        if hf97_uncatchable_tip_quarantined(settings, &local, best_direct_height) {
            report.best_peer_height = report.best_peer_height.min(local_height);
            report.best_peer_tip_hash.clear();
        } else {
            bail!(
                "mining paused: direct peer height {} is ahead of local height {}. HF98 catch-up will refresh before hashing.",
                best_direct_height,
                local_height
            );
        }
    }

    Ok(report)
}

fn prioritized_outbound_peers(settings: &Settings, max_count: usize) -> Result<Vec<String>> {
    let mut out = Vec::<String>::new();
    let push = |addr: String, out: &mut Vec<String>| {
        let normalized = normalize_peer_addr(&addr);
        if !normalized.trim().is_empty()
            && !is_self_or_empty_addr(settings, &normalized)
            && !should_skip_outbound(settings, &normalized)
            && !out.iter().any(|p| p == &normalized)
        {
            out.push(normalized);
        }
    };
    // Always relay to official seeds first so mempool actions are picked up by
    // the network even when the stale peer registry is noisy.
    for seed in official_snapshot_peers(settings) {
        push(seed, &mut out);
    }
    for seed in release_bootnodes(settings) {
        push(seed, &mut out);
    }
    for peer in known_peers(settings)? {
        push(peer, &mut out);
    }
    out.truncate(max_count.max(1));
    Ok(out)
}

const HF116_MEMPOOL_RELAY_BATCH_TXS: usize = 512;
const HF116_MEMPOOL_INBOUND_BATCH_TXS: usize = 2_048;

fn hf115_mempool_inbound_limit(settings: &Settings) -> usize {
    effective_mempool_max_transactions(settings)
        .min(HF116_MEMPOOL_INBOUND_BATCH_TXS)
        .max(1)
}

fn hf124_fair_mempool_relay_batch(
    settings: &Settings,
    mempool: &[Transaction],
    max_count: usize,
) -> Vec<Transaction> {
    let max_count = max_count.max(1);
    let mut shares = Vec::new();
    let mut ordinary = Vec::new();
    for tx in mempool
        .iter()
        .filter(|tx| hf106_jin_sale_standardness_policy(tx, settings).is_ok())
    {
        if is_pool_share_transaction(tx) {
            shares.push(tx);
        } else {
            ordinary.push(tx);
        }
    }
    // Relay the oldest still-confirmable shares first so a multi-parent
    // backlog drains before earlier shares expire. Mempool admission already
    // guarantees these markers are structurally valid.
    shares.sort_by_cached_key(|tx| {
        (
            parse_pool_share_tx(tx)
                .map(|share| share.parent_height)
                .unwrap_or(u32::MAX),
            tx.txid().to_string(),
        )
    });
    ordinary.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });

    // HF124/v1.8.1: reserve relay space for ordinary transactions so a pool-
    // share burst cannot starve QUB/JIN/Library traffic. Shares are also capped
    // to one block's consensus capacity per relay batch.
    let ordinary_reserve = if ordinary.is_empty() {
        0
    } else {
        ordinary.len().min((max_count / 4).max(1))
    };
    let share_quota = settings
        .pools
        .max_share_txs_per_block
        .min(max_count.saturating_sub(ordinary_reserve));

    let mut out = Vec::with_capacity(max_count);
    out.extend(shares.iter().take(share_quota).map(|tx| (*tx).clone()));
    out.extend(
        ordinary
            .iter()
            .take(max_count.saturating_sub(out.len()))
            .map(|tx| (*tx).clone()),
    );
    out
}

fn hf115_mempool_relay_batch(settings: &Settings, mempool: &[Transaction]) -> Vec<Transaction> {
    hf124_fair_mempool_relay_batch(
        settings,
        mempool,
        hf115_mempool_inbound_limit(settings).min(HF116_MEMPOOL_RELAY_BATCH_TXS),
    )
}

fn merge_mempool_from_chain(
    local: &mut ChainState,
    source: &ChainState,
    settings: &Settings,
) -> Vec<Transaction> {
    // HF124/v1.8.1: merge by txid with the same bounded batch-aware admission
    // used for peer mempool messages. A GUI/CLI snapshot carrying a share burst
    // must not rebuild confirmed pool context once per transaction while the
    // canonical owner is being reconciled.
    let known = local.mempool_txids();
    let candidates = source
        .mempool
        .iter()
        .filter(|tx| !known.contains(&tx.txid()))
        .take(effective_mempool_max_transactions(settings))
        .cloned()
        .collect::<Vec<_>>();
    local
        .accept_transactions_to_mempool_batch(candidates, settings)
        .unwrap_or_default()
}

fn relay_mempool_batch_to_known_peers(
    settings: &Settings,
    txs: &[Transaction],
    source_peer: Option<&str>,
    max_peers: usize,
) -> Result<usize> {
    if !settings.p2p.enabled || txs.is_empty() {
        return Ok(0);
    }
    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let chain = load_chain_for_hf90_catchup(settings)?;
    let message = WireMessage::Mempool {
        txs: txs
            .iter()
            .take(HF116_MEMPOOL_RELAY_BATCH_TXS)
            .cloned()
            .collect(),
    };
    let mut sent = 0usize;
    for addr in prioritized_outbound_peers(settings, max_peers.max(1).min(16))? {
        let normalized = normalize_peer_addr(&addr);
        if normalized.is_empty() || (!source.is_empty() && normalized == source) {
            continue;
        }
        let Ok(mut stream) = connect_peer(&normalized, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        if send_version(&mut stream, settings, &chain).is_err() {
            continue;
        }
        if send_wire(&mut stream, &message).is_ok() {
            sent = sent.saturating_add(1);
        }
    }
    Ok(sent)
}

pub fn rebroadcast_local_mempool(settings: &Settings, max_txs: usize) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let txs = hf124_fair_mempool_relay_batch(
        settings,
        &chain.mempool,
        max_txs.max(1).min(HF116_MEMPOOL_RELAY_BATCH_TXS),
    );
    let peers = relay_mempool_batch_to_known_peers(
        settings,
        &txs,
        None,
        settings.p2p.max_outbound_peers.max(4).min(8),
    )?;
    Ok(peers.saturating_mul(txs.len()))
}

fn hf125_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn pending_block_relay_path(settings: &Settings) -> PathBuf {
    PathBuf::from(&settings.node.data_dir).join("pending-block-relay.json")
}

fn write_pending_block_relay(settings: &Settings, pending: &PendingBlockRelay) -> Result<()> {
    let path = pending_block_relay_path(settings);
    crate::write_text_replace(&path, &serde_json::to_string_pretty(pending)?)
}

fn load_pending_block_relay(settings: &Settings) -> Result<Option<PendingBlockRelay>> {
    let path = pending_block_relay_path(settings);
    if !path.exists() {
        return Ok(None);
    }
    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let pending: PendingBlockRelay = match serde_json::from_str(&raw) {
        Ok(pending) => pending,
        Err(err) => {
            let corrupt = path.with_file_name(format!(
                "pending-block-relay.corrupt.{}.json",
                hf125_now_secs()
            ));
            let _ = fs::rename(&path, &corrupt);
            return Err(err.into());
        }
    };
    if pending.version != HF125_PENDING_BLOCK_RELAY_VERSION
        || pending.network != settings.network.name
    {
        let _ = fs::remove_file(path);
        return Ok(None);
    }
    Ok(Some(pending))
}

fn clear_pending_block_relay(settings: &Settings) {
    let _ = fs::remove_file(pending_block_relay_path(settings));
}

fn schedule_pending_block_relay(settings: &Settings, block: &Block, height: u32) -> Result<()> {
    let block_hash = block.block_hash().to_string();
    let now = hf125_now_secs();
    let existing = load_pending_block_relay(settings).ok().flatten();
    let pending = match existing {
        Some(mut existing) if existing.block_hash == block_hash => {
            existing.block = block.clone();
            existing.height = height;
            existing
        }
        _ => PendingBlockRelay {
            version: HF125_PENDING_BLOCK_RELAY_VERSION,
            network: settings.network.name.clone(),
            block_hash,
            height,
            block: block.clone(),
            created_unix: now,
            last_attempt_unix: 0,
            attempts: 0,
            last_acknowledgements: 0,
            last_error: String::new(),
            last_stale_parent_height: 0,
            last_stale_parent_tip: String::new(),
            last_stale_parent_reports: 0,
            last_official_stale_parent_reports: 0,
        },
    };
    write_pending_block_relay(settings, &pending)
}

fn pending_block_relay_waiting(settings: &Settings, local: &ChainState) -> bool {
    let Ok(Some(pending)) = load_pending_block_relay(settings) else {
        return false;
    };
    pending.height == local.height()
        && pending.block_hash == local.tip_hash().to_string()
}

fn block_acceptance_from_chain(
    settings: &Settings,
    local: &mut ChainState,
    block: Block,
) -> BlockAcceptance {
    let block_hash = block.block_hash().to_string();
    if let Some((height, _)) = local
        .blocks
        .iter()
        .enumerate()
        .find(|(_, existing)| existing.block_hash().to_string() == block_hash)
    {
        return BlockAcceptance {
            status: "already_known".to_string(),
            block_hash,
            height: height as u32,
            tip_hash: local.tip_hash().to_string(),
            reason: String::new(),
            changed: false,
        };
    }

    if block.header.prev_block_hash != local.tip_hash() {
        return BlockAcceptance {
            status: "stale_parent".to_string(),
            block_hash,
            height: local.height(),
            tip_hash: local.tip_hash().to_string(),
            reason: format!(
                "candidate parent {} does not match current tip {}",
                block.header.prev_block_hash,
                local.tip_hash()
            ),
            changed: false,
        };
    }

    match connect_block_persist_atomic(settings, local, block) {
        Ok(_) => {
            mark_fresh_tip_trusted(settings, local);
            BlockAcceptance {
                status: "accepted".to_string(),
                block_hash,
                height: local.height(),
                tip_hash: local.tip_hash().to_string(),
                reason: String::new(),
                changed: true,
            }
        }
        Err(err) => BlockAcceptance {
            status: "rejected".to_string(),
            block_hash,
            height: local.height(),
            tip_hash: local.tip_hash().to_string(),
            reason: err.to_string(),
            changed: false,
        },
    }
}

fn accept_submitted_block_live(
    settings: &Settings,
    chain: &Arc<Mutex<ChainState>>,
    mempool_dirty: &Arc<AtomicBool>,
    block: Block,
) -> BlockAcceptance {
    let mut local = match chain.lock() {
        Ok(local) => local,
        Err(_) => {
            return BlockAcceptance {
                status: "rejected".to_string(),
                block_hash: block.block_hash().to_string(),
                height: 0,
                tip_hash: String::new(),
                reason: "canonical chain mutex poisoned".to_string(),
                changed: false,
            }
        }
    };
    let accepted = block_acceptance_from_chain(settings, &mut local, block);
    if accepted.changed {
        mempool_dirty.store(false, Ordering::Release);
    }
    accepted
}

fn send_block_ack(
    stream: &mut TcpStream,
    request_id: u64,
    accepted: &BlockAcceptance,
) -> Result<()> {
    send_wire(
        stream,
        &WireMessage::BlockAck {
            request_id,
            block_hash: accepted.block_hash.clone(),
            status: accepted.status.clone(),
            height: accepted.height,
            tip_hash: accepted.tip_hash.clone(),
            reason: accepted.reason.clone(),
        },
    )
}

fn is_official_block_relay_peer(settings: &Settings, addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    official_snapshot_peers(settings)
        .into_iter()
        .chain(release_bootnodes(settings))
        .map(|peer| normalize_peer_addr(&peer))
        .any(|peer| peer == normalized)
}

fn send_block_delivery_suffix(
    stream: &mut TcpStream,
    settings: &Settings,
    chain: &ChainState,
    from_height: u32,
) -> Result<()> {
    let mut from = from_height;
    let max_blocks = settings.p2p.max_blocks_per_message.max(1) as u32;
    let final_height = chain.height();
    let floor = final_height.saturating_sub(4_096);
    if from < floor {
        from = floor;
    }
    while from <= final_height {
        send_headers(stream, settings, chain, from)?;
        send_chain(stream, settings, chain, from)?;
        let next = from.saturating_add(max_blocks);
        if next <= from {
            break;
        }
        from = next;
    }
    Ok(())
}

fn respond_during_block_submit(
    stream: &mut TcpStream,
    settings: &Settings,
    chain: &ChainState,
    message: WireMessage,
) -> Result<bool> {
    match message {
        WireMessage::GetHeaders { from_height } => {
            send_headers(stream, settings, chain, from_height)?;
            Ok(true)
        }
        WireMessage::GetChain { from_height } => {
            send_chain(stream, settings, chain, from_height)?;
            Ok(true)
        }
        WireMessage::GetMempool => {
            let txs = hf115_mempool_relay_batch(settings, &chain.mempool);
            send_wire(stream, &WireMessage::Mempool { txs })?;
            Ok(true)
        }
        WireMessage::GetAddr => {
            send_wire(
                stream,
                &WireMessage::Addr {
                    addrs: release_bootnodes(settings),
                },
            )?;
            Ok(true)
        }
        WireMessage::GetPeerList => {
            send_wire(
                stream,
                &WireMessage::PeerList {
                    peers: load_peer_registry(settings).unwrap_or_default().peers,
                },
            )?;
            Ok(true)
        }
        WireMessage::Ping { nonce } => {
            send_wire(stream, &WireMessage::Pong { nonce })?;
            Ok(true)
        }
        _ => Ok(false),
    }
}

fn legacy_send_block_to_peer(
    settings: &Settings,
    addr: &str,
    block: &Block,
    chain: &ChainState,
) -> BlockRelayPeerResult {
    let mut result = BlockRelayPeerResult {
        peer: addr.to_string(),
        official: is_official_block_relay_peer(settings, addr),
        transport: "legacy".to_string(),
        ..BlockRelayPeerResult::default()
    };
    let attempt = (|| -> Result<()> {
        let mut stream = connect_peer(addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(3)))?;
        stream.set_write_timeout(Some(Duration::from_secs(4)))?;
        send_version_with_role(&mut stream, settings, chain, "block-submit")?;
        send_wire(
            &mut stream,
            &WireMessage::Block {
                block: block.clone(),
            },
        )?;
        send_inv(&mut stream, chain)?;
        let tail_from = chain.height().saturating_sub(128);
        send_headers(&mut stream, settings, chain, tail_from)?;
        send_chain(&mut stream, settings, chain, tail_from)?;
        Ok(())
    })();
    match attempt {
        Ok(()) => {
            result.status = "legacy_sent".to_string();
            result.height = chain.height();
            result.tip_hash = chain.tip_hash().to_string();
        }
        Err(err) => {
            result.status = "transport_error".to_string();
            result.reason = err.to_string();
        }
    }
    result
}

fn submit_block_to_peer(
    settings: &Settings,
    addr: &str,
    block: &Block,
    chain: &ChainState,
) -> BlockRelayPeerResult {
    let mut result = BlockRelayPeerResult {
        peer: addr.to_string(),
        official: is_official_block_relay_peer(settings, addr),
        transport: "ack".to_string(),
        ..BlockRelayPeerResult::default()
    };
    let request_id = rand::thread_rng().next_u64();
    let attempt = (|| -> Result<Option<BlockRelayPeerResult>> {
        let mut stream = connect_peer(addr, Duration::from_secs(3))?;
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;
        send_version_with_role(&mut stream, settings, chain, "block-submit")?;
        send_wire(
            &mut stream,
            &WireMessage::SubmitBlock {
                request_id,
                block: block.clone(),
            },
        )?;

        let mut reader = BufReader::new(stream.try_clone()?);
        let mut deadline = Instant::now() + Duration::from_secs(HF125_BLOCK_ACK_TIMEOUT_SECS);
        let mut repair_mode: Option<&'static str> = None;
        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(WireMessage::BlockAck {
                    request_id: ack_id,
                    block_hash,
                    status,
                    height,
                    tip_hash,
                    reason,
                }) if ack_id == request_id => {
                    if block_hash != block.block_hash().to_string() {
                        return Ok(Some(BlockRelayPeerResult {
                            peer: addr.to_string(),
                            official: is_official_block_relay_peer(settings, addr),
                            transport: "ack".to_string(),
                            status: "rejected".to_string(),
                            height,
                            tip_hash,
                            reason: format!(
                                "acknowledgement hash mismatch: {block_hash}; {reason}"
                            ),
                        }));
                    }

                    // HF125/HF126: repair the receiver on the same connection
                    // before retrying the exact block. If its tip is our ancestor,
                    // send only the missing suffix. If it reports an equal-height
                    // sibling, send a bounded overlap window including the common
                    // ancestor and our higher-work suffix. This is required after
                    // HF126 keeps equal-height PoW ties mineable: the winning next
                    // block must be able to carry its parent branch to a seed whose
                    // current tip is the competing sibling.
                    if status == "stale_parent" && repair_mode.is_none() && height < chain.height()
                    {
                        let peer_is_ancestor = chain
                            .blocks
                            .get(height as usize)
                            .map(|candidate| candidate.block_hash().to_string())
                            .as_deref()
                            == Some(tip_hash.as_str());
                        let (from_height, mode) = if peer_is_ancestor {
                            (height.saturating_add(1), "ack_after_suffix")
                        } else {
                            (height.saturating_sub(32), "ack_after_overlap")
                        };
                        send_block_delivery_suffix(&mut stream, settings, chain, from_height)?;
                        send_wire(
                            &mut stream,
                            &WireMessage::SubmitBlock {
                                request_id,
                                block: block.clone(),
                            },
                        )?;
                        repair_mode = Some(mode);
                        // Give the receiver a fresh bounded window to validate the
                        // overlap/suffix before it acknowledges the exact block.
                        deadline = Instant::now() + Duration::from_secs(25);
                        continue;
                    }

                    return Ok(Some(BlockRelayPeerResult {
                        peer: addr.to_string(),
                        official: is_official_block_relay_peer(settings, addr),
                        transport: repair_mode.unwrap_or("ack").to_string(),
                        status,
                        height,
                        tip_hash,
                        reason,
                    }));
                }
                Ok(WireMessage::Version {
                    protocol,
                    network,
                    magic,
                    genesis_hash,
                    ..
                }) => {
                    if protocol != PROTOCOL_VERSION {
                        bail!("unsupported protocol {protocol}");
                    }
                    validate_remote_network(settings, &network, &magic, &genesis_hash)?;
                }
                Ok(WireMessage::Reject { reason }) => {
                    result.reason = reason;
                    return Ok(None);
                }
                Ok(message) => {
                    let _ = respond_during_block_submit(
                        &mut stream,
                        settings,
                        chain,
                        message,
                    )?;
                }
                Err(err) if is_timeout(&err) => continue,
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(err) => return Err(err.into()),
            }
        }
        Ok(None)
    })();

    match attempt {
        Ok(Some(ack)) => ack,
        Ok(None) => legacy_send_block_to_peer(settings, addr, block, chain),
        Err(err) => {
            result.status = if is_timeout_text(&err.to_string()) {
                "timeout".to_string()
            } else {
                "transport_error".to_string()
            };
            result.reason = err.to_string();
            result
        }
    }
}

fn is_timeout_text(text: &str) -> bool {
    let text = text.to_ascii_lowercase();
    text.contains("timed out")
        || text.contains("timeout")
        || text.contains("would block")
}

fn update_block_relay_report(report: &mut BlockRelayReport, result: BlockRelayPeerResult) {
    if result.status != "transport_error" && result.status != "timeout" {
        report.connected = report.connected.saturating_add(1);
    }
    match result.status.as_str() {
        "accepted" => {
            report.acknowledgements = report.acknowledgements.saturating_add(1);
            if result.official {
                report.official_acknowledgements =
                    report.official_acknowledgements.saturating_add(1);
            }
            report.accepted = report.accepted.saturating_add(1);
        }
        "already_known" => {
            report.acknowledgements = report.acknowledgements.saturating_add(1);
            if result.official {
                report.official_acknowledgements =
                    report.official_acknowledgements.saturating_add(1);
            }
            report.already_known = report.already_known.saturating_add(1);
        }
        "stale_parent" => report.stale_parent = report.stale_parent.saturating_add(1),
        "rejected" => report.rejected = report.rejected.saturating_add(1),
        "legacy_sent" => report.legacy_sent = report.legacy_sent.saturating_add(1),
        "timeout" => report.timed_out = report.timed_out.saturating_add(1),
        _ => {}
    }
    report.peer_results.push(result);
}

fn broadcast_block_confirmed_internal(
    settings: &Settings,
    block: &Block,
    source_peer: Option<&str>,
    track_pending: bool,
) -> Result<BlockRelayReport> {
    let deadline = Instant::now() + Duration::from_secs(HF125_BLOCK_RELAY_TOTAL_SECS);
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let _relay_guard = loop {
        let left = hf130_io_remaining(deadline)?;
        match hf125_block_relay_lock().try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::Poisoned(_)) => bail!("HF125 block-relay mutex poisoned"),
            Err(TryLockError::WouldBlock) => thread::sleep(left.min(Duration::from_millis(20))),
        }
    };
    let block_hash = block.block_hash().to_string();
    let chain = if let Some(live) = live_chain_snapshot(settings) {
        live
    } else if let Ok(committed) = load_committed_chain(settings, false) {
        committed
    } else {
        load_chain_for_hf90_catchup(settings)?
    };
    let height = chain
        .blocks
        .iter()
        .position(|candidate| candidate.block_hash().to_string() == block_hash)
        .map(|height| height as u32)
        .unwrap_or_else(|| chain.height().saturating_add(1));

    if track_pending {
        schedule_pending_block_relay(settings, block, height)?;
    }

    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let peers = prioritized_outbound_peers(
        settings,
        HF125_BLOCK_RELAY_MAX_PEERS
            .max(settings.p2p.max_outbound_peers)
            .min(48),
    )?
    .into_iter()
    .filter(|addr| source.is_empty() || normalize_peer_addr(addr) != source)
    .take(HF125_BLOCK_RELAY_MAX_PEERS)
    .collect::<Vec<_>>();

    let official_attempts = peers
        .iter()
        .filter(|addr| is_official_block_relay_peer(settings, addr))
        .count();
    let mut report = BlockRelayReport {
        block_hash,
        height,
        attempts: peers.len(),
        official_attempts,
        ..BlockRelayReport::default()
    };
    if peers.is_empty() {
        report.pending_retry = track_pending;
        return Ok(report);
    }

    let inherited = hf130_io_context();
    let controller_owned = inherited.cancel.is_some();
    let relay_done = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();
    let (sender, receiver) = mpsc::channel();
    for addr in peers {
        let mut worker_context = inherited.clone();
        if controller_owned { worker_context.completion = Some(relay_done.clone()); }
        let sender = sender.clone();
        let error_sender = sender.clone();
        let settings = settings.clone();
        let block = block.clone();
        let chain = chain.clone();
        let peer = addr.clone();
        let official = is_official_block_relay_peer(&settings, &addr);
        if let Err(err) = thread::Builder::new()
            .name("qub-block-submit".to_string())
            .spawn(move || {
                let _scope = Hf130IoScope::enter(worker_context);
                let _ = sender.send(submit_block_to_peer(&settings, &addr, &block, &chain));
            })
            .map(|handle| { if controller_owned { workers.push(handle); } })
        {
            let _ = error_sender.send(BlockRelayPeerResult {
                peer,
                official,
                transport: "spawn_error".to_string(),
                status: "transport_error".to_string(),
                reason: format!("could not allocate block-submit worker: {err}"),
                ..BlockRelayPeerResult::default()
            });
        }
    }
    drop(sender);

    while report.peer_results.len() < report.attempts && Instant::now() < deadline {
        let wait = match hf130_io_remaining(deadline) {
            Ok(left) => left,
            Err(_) => break,
        };
        match receiver.recv_timeout(wait.min(Duration::from_millis(200))) {
            Ok(result) => {
                update_block_relay_report(&mut report, result);
                if report.delivered() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // A mining controller may retire only after its network workers return.
    // Completion cancels redundant attempts without setting the user's Stop.
    relay_done.store(true, Ordering::Relaxed);
    for worker in workers { let _ = worker.join(); }
    while let Ok(result) = receiver.try_recv() {
        update_block_relay_report(&mut report, result);
    }
    if !report.delivered() {
        report.timed_out = report
            .timed_out
            .saturating_add(report.attempts.saturating_sub(report.peer_results.len()));
    }

    if report.delivered() {
        clear_pending_block_relay(settings);
        report.pending_retry = false;
    } else if track_pending {
        let now = hf125_now_secs();
        if let Ok(Some(mut pending)) = load_pending_block_relay(settings) {
            if pending.block_hash == report.block_hash {
                pending.last_attempt_unix = now;
                pending.attempts = pending.attempts.saturating_add(report.attempts as u64);
                pending.last_acknowledgements = report.acknowledged();
                pending.last_error = report
                    .peer_results
                    .iter()
                    .filter(|result| !result.reason.trim().is_empty())
                    .map(|result| format!("{}: {}", result.peer, result.reason))
                    .take(8)
                    .collect::<Vec<_>>()
                    .join(" | ");

                let mut stale_views: HashMap<(u32, String), (usize, usize)> = HashMap::new();
                for result in report
                    .peer_results
                    .iter()
                    .filter(|result| result.status == "stale_parent")
                {
                    if result.height == 0 || result.tip_hash.trim().is_empty() {
                        continue;
                    }
                    let entry = stale_views
                        .entry((result.height, result.tip_hash.clone()))
                        .or_insert((0, 0));
                    entry.0 = entry.0.saturating_add(1);
                    if result.official {
                        entry.1 = entry.1.saturating_add(1);
                    }
                }
                if let Some(((height, tip), (count, official_count))) = stale_views
                    .into_iter()
                    .max_by_key(|(_, (count, official_count))| {
                        official_count.saturating_mul(100).saturating_add(*count)
                    })
                {
                    pending.last_stale_parent_height = height;
                    pending.last_stale_parent_tip = tip;
                    pending.last_stale_parent_reports = count;
                    pending.last_official_stale_parent_reports = official_count;
                }
                // Keep earlier explicit stale-parent evidence across a later
                // transport-only retry. The record is scoped to this exact local
                // block hash and is still combined with fresh HTTP/TCP/direct
                // evidence before HF126 can re-anchor.
                let _ = write_pending_block_relay(settings, &pending);
            }
        }
        report.pending_retry = true;
    }

    Ok(report)
}

pub fn broadcast_block_confirmed(settings: &Settings, block: &Block) -> Result<BlockRelayReport> {
    if !settings.p2p.enabled {
        return Ok(BlockRelayReport {
            block_hash: block.block_hash().to_string(),
            ..BlockRelayReport::default()
        });
    }
    broadcast_block_confirmed_internal(settings, block, None, true)
}


pub fn block_relay_status(settings: &Settings) -> Result<serde_json::Value> {
    let pending = load_pending_block_relay(settings)?;
    let local = load_chain_for_hf90_catchup(settings)?;
    Ok(match pending {
        Some(pending) => serde_json::json!({
            "ok": true,
            "pending": true,
            "network": settings.network.name,
            "local_height": local.height(),
            "local_tip": local.tip_hash().to_string(),
            "block_height": pending.height,
            "block_hash": pending.block_hash,
            "created_unix": pending.created_unix,
            "last_attempt_unix": pending.last_attempt_unix,
            "attempts": pending.attempts,
            "last_acknowledgements": pending.last_acknowledgements,
            "last_error": pending.last_error,
            "competing_tip_height": pending.last_stale_parent_height,
            "competing_tip_hash": pending.last_stale_parent_tip,
            "competing_tip_reports": pending.last_stale_parent_reports,
            "official_competing_tip_reports": pending.last_official_stale_parent_reports,
            "hf125_reliable_block_delivery": true,
            "hf126_equal_height_recovery": true,
            "delivery_rule": "one official ACK, or two distinct direct-peer ACKs, or one explicit ACK when no official destination is configured",
        }),
        None => serde_json::json!({
            "ok": true,
            "pending": false,
            "network": settings.network.name,
            "local_height": local.height(),
            "local_tip": local.tip_hash().to_string(),
            "hf125_reliable_block_delivery": true,
            "hf126_equal_height_recovery": true,
            "delivery_rule": "one official ACK, or two distinct direct-peer ACKs, or one explicit ACK when no official destination is configured",
        }),
    })
}

pub fn relay_pending_block_now(settings: &Settings) -> Result<Option<BlockRelayReport>> {
    let Some(pending) = load_pending_block_relay(settings)? else {
        return Ok(None);
    };
    let local = load_chain_for_hf90_catchup(settings)?;
    if local
        .blocks
        .get(pending.height as usize)
        .map(|block| block.block_hash().to_string())
        != Some(pending.block_hash.clone())
    {
        clear_pending_block_relay(settings);
        return Ok(None);
    }
    broadcast_block_confirmed_internal(settings, &pending.block, None, true).map(Some)
}


pub fn tx_delivery_status(
    settings: &Settings,
    txid: Option<Hash256>,
) -> Result<serde_json::Value> {
    pending_tx_delivery_json(settings, txid)
}

pub fn relay_pending_txs_now(settings: &Settings) -> Result<Vec<TxRelayReport>> {
    let now = hf125_now_secs();
    let mut reports = Vec::new();
    for record in pending_tx_delivery_records(settings)?
        .into_iter()
        .filter(|record| !is_pool_share_transaction(&record.tx))
        .take(HF127_TX_RETRY_MAX_PER_PASS.max(1))
    {
        let report = broadcast_tx_confirmed_internal(
            settings,
            &record.tx,
            None,
            HF127_TX_RELAY_MAX_PEERS,
            850,
            true,
        )?;
        reports.push(report);
        if hf125_now_secs().saturating_sub(now) >= HF127_TX_RELAY_TOTAL_SECS {
            break;
        }
    }
    Ok(reports)
}

fn retry_pending_block_relay(
    settings: &Settings,
    chain: &Arc<Mutex<ChainState>>,
) -> Result<Option<BlockRelayReport>> {
    let Some(pending) = load_pending_block_relay(settings)? else {
        return Ok(None);
    };
    let (local_height, local_at_height) = {
        let snapshot = chain
            .lock()
            .map_err(|_| anyhow::anyhow!("chain mutex poisoned"))?;
        (
            snapshot.height(),
            snapshot
                .blocks
                .get(pending.height as usize)
                .map(Block::block_hash),
        )
    };
    match local_at_height {
        Some(hash) if hash.to_string() == pending.block_hash => {
            if local_height > pending.height {
                clear_pending_block_relay(settings);
                return Ok(None);
            }
        }
        _ => {
            clear_pending_block_relay(settings);
            return Ok(None);
        }
    }
    if pending.last_attempt_unix > 0
        && hf125_now_secs().saturating_sub(pending.last_attempt_unix)
            < HF125_BLOCK_RELAY_RETRY_SECS
    {
        return Ok(None);
    }
    let report = broadcast_block_confirmed_internal(settings, &pending.block, None, true)?;
    Ok(Some(report))
}

pub fn broadcast_block(settings: &Settings, block: &Block) -> Result<usize> {
    let report = broadcast_block_confirmed(settings, block)?;
    Ok(report.acknowledged().saturating_add(report.legacy_sent))
}

fn relay_chain_to_known_peers(settings: &Settings, source_peer: Option<&str>) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let mut sent = 0usize;
    let tail_from = chain.height().saturating_sub(128);
    for addr in
        prioritized_outbound_peers(settings, settings.p2p.max_outbound_peers.max(16).min(48))?
    {
        let normalized = normalize_peer_addr(&addr);
        if normalized.is_empty() || (!source.is_empty() && normalized == source) {
            continue;
        }
        let Ok(mut stream) = connect_peer(&normalized, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(4)))?;
        if send_version(&mut stream, settings, &chain).is_err() {
            continue;
        }
        if send_inv(&mut stream, &chain).is_err() {
            continue;
        }
        if send_headers(&mut stream, settings, &chain, tail_from).is_err() {
            continue;
        }
        if send_chain(&mut stream, settings, &chain, tail_from).is_ok() {
            sent += 1;
        }
    }
    Ok(sent)
}

fn tx_acceptance_from_chain(
    settings: &Settings,
    local: &mut ChainState,
    tx: Transaction,
) -> TxAcceptance {
    let txid_hash = tx.txid();
    let txid = txid_hash.to_string();
    if let Some((height, _confirmations)) = local.tx_confirmations(txid_hash) {
        return TxAcceptance {
            status: "already_confirmed".to_string(),
            txid,
            height,
            tip_hash: local.tip_hash().to_string(),
            reason: String::new(),
            changed: false,
        };
    }
    if local.tx_in_mempool(txid_hash) {
        return TxAcceptance {
            status: "already_known".to_string(),
            txid,
            height: local.height(),
            tip_hash: local.tip_hash().to_string(),
            reason: String::new(),
            changed: false,
        };
    }
    if is_pool_share_transaction(&tx) {
        return TxAcceptance {
            status: "rejected".to_string(),
            txid,
            height: local.height(),
            tip_hash: local.tip_hash().to_string(),
            reason: "pool shares use the bounded mempool relay path, not tx-submit".to_string(),
            changed: false,
        };
    }

    // HF127 acknowledgement means durable acceptance, not merely a mutation of
    // a process-local mempool. Validate on a copy, commit QUB-FCE-1, and only
    // then replace the caller state and return accepted.
    let mut candidate = local.clone();
    match candidate.accept_transaction_to_mempool(tx, settings) {
        Ok(_) => match save_chain(settings, &candidate) {
            Ok(()) => {
                *local = candidate;
                TxAcceptance {
                    status: "accepted".to_string(),
                    txid,
                    height: local.height(),
                    tip_hash: local.tip_hash().to_string(),
                    reason: String::new(),
                    changed: true,
                }
            }
            Err(err) => TxAcceptance {
                status: "rejected".to_string(),
                txid,
                height: local.height(),
                tip_hash: local.tip_hash().to_string(),
                reason: format!("durable mempool commit failed: {err:#}"),
                changed: false,
            },
        },
        Err(err) => TxAcceptance {
            status: "rejected".to_string(),
            txid,
            height: local.height(),
            tip_hash: local.tip_hash().to_string(),
            reason: err.to_string(),
            changed: false,
        },
    }
}

fn hf127_chain_mempool_digest(chain: &ChainState) -> Hash256 {
    let mut txids = chain.mempool.iter().map(Transaction::txid).collect::<Vec<_>>();
    txids.sort();
    let mut raw = Vec::with_capacity(txids.len().saturating_mul(32));
    for txid in txids {
        raw.extend_from_slice(txid.as_bytes());
    }
    Hash256::double_sha256(&raw)
}

fn hf127_committed_state_matches(settings: &Settings, chain: &ChainState) -> bool {
    fast_storage_identity(settings)
        .ok()
        .flatten()
        .map(|identity| {
            identity.committed_height == chain.height()
                && identity.tip_hash == chain.tip_hash()
                && identity.mempool_digest == hf127_chain_mempool_digest(chain)
        })
        .unwrap_or(false)
}

fn accept_submitted_tx_live(
    settings: &Settings,
    chain: &Arc<Mutex<ChainState>>,
    _mempool_dirty: &Arc<AtomicBool>,
    tx: Transaction,
) -> TxAcceptance {
    // Serialize durable transaction admission, but never hold the canonical
    // chain mutex during Fast Chain Engine disk I/O. Blocks retain priority and
    // can be accepted while an ordinary transaction state snapshot is written.
    let _accept_guard = match hf127_tx_acceptance_lock().lock() {
        Ok(guard) => guard,
        Err(_) => {
            return TxAcceptance {
                status: "rejected".to_string(),
                txid: tx.txid().to_string(),
                height: 0,
                tip_hash: String::new(),
                reason: "transaction acceptance mutex poisoned".to_string(),
                changed: false,
            };
        }
    };

    let txid_hash = tx.txid();
    let txid = txid_hash.to_string();
    for _attempt in 0..2 {
        let base = match chain.lock() {
            Ok(local) => local.clone(),
            Err(_) => {
                return TxAcceptance {
                    status: "rejected".to_string(),
                    txid,
                    height: 0,
                    tip_hash: String::new(),
                    reason: "canonical chain mutex poisoned".to_string(),
                    changed: false,
                };
            }
        };

        if let Some((height, _confirmations)) = base.tx_confirmations(txid_hash) {
            return TxAcceptance {
                status: "already_confirmed".to_string(),
                txid,
                height,
                tip_hash: base.tip_hash().to_string(),
                reason: String::new(),
                changed: false,
            };
        }
        if base.tx_in_mempool(txid_hash) {
            // An explicit positive ACK must prove that the transaction is not
            // merely present in a process-local dirty mempool. If the committed
            // Fast Chain identity already matches this exact state, no write is
            // needed. Otherwise persist the existing mempool snapshot before
            // returning already_known.
            if hf127_committed_state_matches(settings, &base) {
                return TxAcceptance {
                    status: "already_known".to_string(),
                    txid,
                    height: base.height(),
                    tip_hash: base.tip_hash().to_string(),
                    reason: String::new(),
                    changed: false,
                };
            }

            match save_chain(settings, &base) {
                Ok(()) => {
                    let current = match chain.lock() {
                        Ok(local) => local.clone(),
                        Err(_) => base.clone(),
                    };
                    if let Some((height, _confirmations)) =
                        current.tx_confirmations(txid_hash)
                    {
                        return TxAcceptance {
                            status: "already_confirmed".to_string(),
                            txid,
                            height,
                            tip_hash: current.tip_hash().to_string(),
                            reason: String::new(),
                            changed: false,
                        };
                    }
                    if current.tx_in_mempool(txid_hash)
                        && hf127_committed_state_matches(settings, &current)
                    {
                        return TxAcceptance {
                            status: "already_known".to_string(),
                            txid,
                            height: current.height(),
                            tip_hash: current.tip_hash().to_string(),
                            reason: String::new(),
                            changed: false,
                        };
                    }
                    // A block or competing mempool commit moved the live owner
                    // while the snapshot was written. Retry once against it.
                }
                Err(err) => {
                    let moved = chain
                        .lock()
                        .map(|current| {
                            current.height() != base.height()
                                || current.tip_hash() != base.tip_hash()
                        })
                        .unwrap_or(false);
                    if !moved {
                        return TxAcceptance {
                            status: "rejected".to_string(),
                            txid,
                            height: base.height(),
                            tip_hash: base.tip_hash().to_string(),
                            reason: format!(
                                "durable existing-mempool commit failed: {err:#}"
                            ),
                            changed: false,
                        };
                    }
                }
            }
            continue;
        }
        if is_pool_share_transaction(&tx) {
            return TxAcceptance {
                status: "rejected".to_string(),
                txid,
                height: base.height(),
                tip_hash: base.tip_hash().to_string(),
                reason: "pool shares use the bounded mempool relay path, not tx-submit"
                    .to_string(),
                changed: false,
            };
        }

        let mut candidate = base.clone();
        if let Err(err) = candidate.accept_transaction_to_mempool(tx.clone(), settings) {
            return TxAcceptance {
                status: "rejected".to_string(),
                txid,
                height: base.height(),
                tip_hash: base.tip_hash().to_string(),
                reason: err.to_string(),
                changed: false,
            };
        }

        match save_chain(settings, &candidate) {
            Ok(()) => {
                // save_chain publishes/merges the candidate into the registered
                // live owner. Confirm that the live view now knows the exact tx
                // before acknowledging durable acceptance.
                let current = match chain.lock() {
                    Ok(local) => local.clone(),
                    Err(_) => candidate,
                };
                if let Some((height, _confirmations)) = current.tx_confirmations(txid_hash) {
                    return TxAcceptance {
                        status: "already_confirmed".to_string(),
                        txid,
                        height,
                        tip_hash: current.tip_hash().to_string(),
                        reason: String::new(),
                        changed: true,
                    };
                }
                if current.tx_in_mempool(txid_hash) {
                    return TxAcceptance {
                        status: "accepted".to_string(),
                        txid,
                        height: current.height(),
                        tip_hash: current.tip_hash().to_string(),
                        reason: String::new(),
                        changed: true,
                    };
                }
                // The canonical tip changed during the commit. Retry exactly
                // once against the new live state before returning a rejection.
            }
            Err(err) => {
                let moved = chain
                    .lock()
                    .map(|current| {
                        current.height() != base.height()
                            || current.tip_hash() != base.tip_hash()
                    })
                    .unwrap_or(false);
                if !moved {
                    return TxAcceptance {
                        status: "rejected".to_string(),
                        txid,
                        height: base.height(),
                        tip_hash: base.tip_hash().to_string(),
                        reason: format!("durable mempool commit failed: {err:#}"),
                        changed: false,
                    };
                }
            }
        }
    }

    let current = chain.lock().ok();
    TxAcceptance {
        status: "rejected".to_string(),
        txid,
        height: current.as_ref().map(|chain| chain.height()).unwrap_or(0),
        tip_hash: current
            .as_ref()
            .map(|chain| chain.tip_hash().to_string())
            .unwrap_or_default(),
        reason: "canonical tip changed during transaction admission; retry delivery on the current tip"
            .to_string(),
        changed: false,
    }
}

fn send_tx_ack(
    stream: &mut TcpStream,
    request_id: u64,
    accepted: &TxAcceptance,
) -> Result<()> {
    send_wire(
        stream,
        &WireMessage::TxAck {
            request_id,
            txid: accepted.txid.clone(),
            status: accepted.status.clone(),
            height: accepted.height,
            tip_hash: accepted.tip_hash.clone(),
            reason: accepted.reason.clone(),
        },
    )
}

fn is_official_tx_relay_peer(settings: &Settings, addr: &str) -> bool {
    is_official_block_relay_peer(settings, addr)
}

fn legacy_send_tx_to_peer(
    settings: &Settings,
    addr: &str,
    tx: &Transaction,
    chain: &ChainState,
    timeout: Duration,
    prior_reason: &str,
) -> TxRelayPeerResult {
    let mut result = TxRelayPeerResult {
        peer: addr.to_string(),
        official: is_official_tx_relay_peer(settings, addr),
        transport: "legacy".to_string(),
        ..TxRelayPeerResult::default()
    };
    let attempt = (|| -> Result<()> {
        let mut stream = connect_peer(addr, timeout)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        send_version(&mut stream, settings, chain)?;
        send_wire(&mut stream, &WireMessage::Tx { tx: tx.clone() })?;
        Ok(())
    })();
    match attempt {
        Ok(()) => {
            result.status = "legacy_sent".to_string();
            result.height = chain.height();
            result.tip_hash = chain.tip_hash().to_string();
            result.reason = prior_reason.to_string();
        }
        Err(err) => {
            result.status = "transport_error".to_string();
            result.reason = if prior_reason.is_empty() {
                err.to_string()
            } else {
                format!("{prior_reason}; legacy fallback: {err:#}")
            };
        }
    }
    result
}

fn respond_during_tx_submit(
    stream: &mut TcpStream,
    settings: &Settings,
    chain: &ChainState,
    message: WireMessage,
) -> Result<bool> {
    match message {
        WireMessage::Version { .. }
        | WireMessage::Inv { .. }
        | WireMessage::Headers { .. }
        | WireMessage::Chain { .. }
        | WireMessage::Mempool { .. }
        | WireMessage::Addr { .. }
        | WireMessage::PeerList { .. }
        | WireMessage::Pong { .. }
        | WireMessage::BlockAck { .. }
        | WireMessage::TxAck { .. } => Ok(true),
        other => respond_during_block_submit(stream, settings, chain, other),
    }
}

fn submit_tx_to_peer(
    settings: &Settings,
    addr: &str,
    tx: &Transaction,
    chain: &ChainState,
    timeout: Duration,
) -> TxRelayPeerResult {
    let official = is_official_tx_relay_peer(settings, addr);
    let request_id = rand::thread_rng().next_u64();
    let expected_txid = tx.txid().to_string();
    let attempt = (|| -> Result<TxRelayPeerResult> {
        let mut stream = connect_peer(addr, timeout)?;
        stream.set_read_timeout(Some(Duration::from_secs(HF127_TX_ACK_TIMEOUT_SECS)))?;
        stream.set_write_timeout(Some(timeout.max(Duration::from_secs(2))))?;
        send_version_with_role(&mut stream, settings, chain, "tx-submit")?;
        send_wire(
            &mut stream,
            &WireMessage::SubmitTx {
                request_id,
                tx: tx.clone(),
            },
        )?;

        let mut reader = BufReader::new(stream.try_clone()?);
        let deadline = Instant::now() + Duration::from_secs(HF127_TX_ACK_TIMEOUT_SECS);
        let deadline = hf130_effective_deadline(deadline);
        let _hf130_io_scope = Hf130IoScope::deadline(deadline);
        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(WireMessage::TxAck {
                    request_id: ack_id,
                    txid,
                    status,
                    height,
                    tip_hash,
                    reason,
                }) if ack_id == request_id => {
                    if txid != expected_txid {
                        return Ok(TxRelayPeerResult {
                            peer: addr.to_string(),
                            official,
                            transport: "ack".to_string(),
                            status: "rejected".to_string(),
                            height,
                            tip_hash,
                            reason: format!(
                                "transaction acknowledgement id mismatch: {txid}; {reason}"
                            ),
                        });
                    }
                    return Ok(TxRelayPeerResult {
                        peer: addr.to_string(),
                        official,
                        transport: "ack".to_string(),
                        status,
                        height,
                        tip_hash,
                        reason,
                    });
                }
                Ok(WireMessage::Reject { reason }) => {
                    bail!("peer rejected tx-submit session: {reason}");
                }
                Ok(message) => {
                    let _ = respond_during_tx_submit(
                        &mut stream,
                        settings,
                        chain,
                        message,
                    )?;
                }
                Err(err) if is_timeout(&err) => continue,
                Err(err) => return Err(err.into()),
            }
        }
        bail!("transaction acknowledgement timeout")
    })();

    match attempt {
        Ok(result) => result,
        Err(err) => legacy_send_tx_to_peer(
            settings,
            addr,
            tx,
            chain,
            timeout,
            &err.to_string(),
        ),
    }
}

fn update_tx_relay_report(report: &mut TxRelayReport, result: TxRelayPeerResult) {
    if result.status != "transport_error" {
        report.connected = report.connected.saturating_add(1);
    }
    let acknowledged = matches!(
        result.status.as_str(),
        "accepted" | "already_known" | "already_confirmed"
    );
    if acknowledged {
        report.acknowledgements = report.acknowledgements.saturating_add(1);
        if result.official {
            report.official_acknowledgements =
                report.official_acknowledgements.saturating_add(1);
        }
    }
    match result.status.as_str() {
        "accepted" => report.accepted = report.accepted.saturating_add(1),
        "already_known" => report.already_known = report.already_known.saturating_add(1),
        "already_confirmed" => {
            report.already_confirmed = report.already_confirmed.saturating_add(1)
        }
        "rejected" => report.rejected = report.rejected.saturating_add(1),
        "legacy_sent" => report.legacy_sent = report.legacy_sent.saturating_add(1),
        _ => {}
    }
    report.peer_results.push(result);
}

fn hf127_tx_retry_delay_secs(previous_attempts: u64, report: &TxRelayReport) -> u64 {
    if report.delivered() {
        return HF127_TX_RETRY_IDLE_SECS;
    }
    let width = report.attempts.max(1) as u64;
    let rounds = previous_attempts.saturating_div(width).min(8) as u32;
    2u64
        .saturating_pow(rounds)
        .clamp(2, HF127_TX_RETRY_IDLE_SECS)
}

fn persist_tx_delivery_report(
    settings: &Settings,
    txid: Hash256,
    previous_attempts: u64,
    fallback_height: u32,
    fallback_tip: &str,
    report: &TxRelayReport,
) -> Result<()> {
    let best = report
        .peer_results
        .iter()
        .find(|result| {
            result.official
                && matches!(
                    result.status.as_str(),
                    "accepted" | "already_known" | "already_confirmed"
                )
        })
        .or_else(|| {
            report.peer_results.iter().find(|result| {
                matches!(
                    result.status.as_str(),
                    "accepted" | "already_known" | "already_confirmed"
                )
            })
        })
        .or_else(|| {
            report
                .peer_results
                .iter()
                .find(|result| result.official && result.status == "rejected")
        })
        .or_else(|| {
            report
                .peer_results
                .iter()
                .find(|result| result.status == "rejected")
        });
    let status = best
        .map(|result| result.status.as_str())
        .unwrap_or("pending_retry");
    let height = best.map(|result| result.height).unwrap_or(fallback_height);
    let tip_hash = best
        .map(|result| result.tip_hash.as_str())
        .unwrap_or(fallback_tip);
    let reason = report
        .peer_results
        .iter()
        .filter(|result| !result.reason.trim().is_empty())
        .map(|result| format!("{}: {}", result.peer, result.reason))
        .take(8)
        .collect::<Vec<_>>()
        .join(" | ");
    let retry_at = hf125_now_secs().saturating_add(hf127_tx_retry_delay_secs(
        previous_attempts,
        report,
    ));
    let updated = update_pending_tx_delivery(
        settings,
        txid,
        report.attempts,
        report.acknowledgements,
        report.official_acknowledgements,
        status,
        height,
        tip_hash,
        &reason,
        report.delivered(),
        retry_at,
    )?;
    if !updated {
        bail!(
            "durable transaction delivery record disappeared before report persistence: {}",
            txid
        );
    }
    Ok(())
}

fn broadcast_tx_confirmed_internal(
    settings: &Settings,
    tx: &Transaction,
    source_peer: Option<&str>,
    max_peers: usize,
    timeout_ms: u64,
    track_pending: bool,
) -> Result<TxRelayReport> {
    let deadline = Instant::now() + Duration::from_secs(HF127_TX_RELAY_TOTAL_SECS);
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let txid_hash = tx.txid();
    let txid = txid_hash.to_string();
    let previous_delivery_attempts = if track_pending {
        let records = pending_tx_delivery_records(settings)?;
        records
            .into_iter()
            .find(|record| record.txid == txid_hash)
            .map(|record| record.delivery_attempts)
            .with_context(|| {
                format!(
                    "durable transaction delivery record missing before relay: {}",
                    txid_hash
                )
            })?
    } else {
        0
    };
    if !settings.p2p.enabled || is_pool_share_transaction(tx) {
        let mut report = TxRelayReport {
            txid,
            ..TxRelayReport::default()
        };
        report.pending_retry = track_pending && !is_pool_share_transaction(tx);
        if track_pending && !is_pool_share_transaction(tx) {
            persist_tx_delivery_report(
                settings,
                txid_hash,
                previous_delivery_attempts,
                0,
                "",
                &report,
            )?;
        }
        return Ok(report);
    }
    if let Err(err) = hf106_jin_sale_standardness_policy(tx, settings) {
        let mut report = TxRelayReport {
            txid,
            rejected: 1,
            ..TxRelayReport::default()
        };
        report.peer_results.push(TxRelayPeerResult {
            status: "rejected".to_string(),
            reason: err.to_string(),
            ..TxRelayPeerResult::default()
        });
        report.pending_retry = track_pending;
        if track_pending {
            persist_tx_delivery_report(
                settings,
                txid_hash,
                previous_delivery_attempts,
                0,
                "",
                &report,
            )?;
        }
        return Ok(report);
    }

    let chain = if let Some(live) = live_chain_snapshot(settings) {
        live
    } else {
        load_chain_for_hf90_catchup(settings)?
    };
    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let peers = prioritized_outbound_peers(
        settings,
        max_peers.max(1).min(HF127_TX_RELAY_MAX_PEERS),
    )?
    .into_iter()
    .filter(|addr| source.is_empty() || normalize_peer_addr(addr) != source)
    .collect::<Vec<_>>();
    let official_attempts = peers
        .iter()
        .filter(|addr| is_official_tx_relay_peer(settings, addr))
        .count();
    let mut report = TxRelayReport {
        txid: txid.clone(),
        attempts: peers.len(),
        official_attempts,
        ..TxRelayReport::default()
    };
    if peers.is_empty() {
        report.pending_retry = track_pending;
        if track_pending {
            persist_tx_delivery_report(
                settings,
                txid_hash,
                previous_delivery_attempts,
                chain.height(),
                &chain.tip_hash().to_string(),
                &report,
            )?;
        }
        return Ok(report);
    }

    let per_peer = Duration::from_millis(timeout_ms.max(300).min(3_000));
    let inherited = hf130_io_context();
    let controller_owned = inherited.cancel.is_some();
    let relay_done = Arc::new(AtomicBool::new(false));
    let mut workers = Vec::new();
    let (sender, receiver) = mpsc::channel();
    for addr in peers {
        let mut worker_context = inherited.clone();
        if controller_owned { worker_context.completion = Some(relay_done.clone()); }
        let sender = sender.clone();
        let error_sender = sender.clone();
        let settings = settings.clone();
        let tx = tx.clone();
        let chain = chain.clone();
        let peer = addr.clone();
        let official = is_official_tx_relay_peer(&settings, &addr);
        if let Err(err) = thread::Builder::new()
            .name("qub-tx-submit".to_string())
            .spawn(move || {
                let _scope = Hf130IoScope::enter(worker_context);
                let _ = sender.send(submit_tx_to_peer(
                    &settings,
                    &addr,
                    &tx,
                    &chain,
                    per_peer,
                ));
            })
            .map(|handle| { if controller_owned { workers.push(handle); } })
        {
            let _ = error_sender.send(TxRelayPeerResult {
                peer,
                official,
                transport: "spawn_error".to_string(),
                status: "transport_error".to_string(),
                reason: format!("could not allocate tx-submit worker: {err}"),
                ..TxRelayPeerResult::default()
            });
        }
    }
    drop(sender);

    while report.peer_results.len() < report.attempts && Instant::now() < deadline {
        let wait = match hf130_io_remaining(deadline) {
            Ok(left) => left,
            Err(_) => break,
        };
        match receiver.recv_timeout(wait.min(Duration::from_millis(200))) {
            Ok(result) => {
                update_tx_relay_report(&mut report, result);
                // An official acceptance is conclusive. When there are no
                // official destinations at all, one explicit peer acceptance is
                // sufficient. Do not keep the GUI waiting for unrelated peer
                // timeouts after delivery is already proven.
                if report.delivered() {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // A mining controller may retire only after its network workers return.
    // Completion cancels redundant attempts without setting the user's Stop.
    relay_done.store(true, Ordering::Relaxed);
    for worker in workers { let _ = worker.join(); }
    while let Ok(result) = receiver.try_recv() {
        update_tx_relay_report(&mut report, result);
    }
    if !report.delivered() {
        report.timed_out = report
            .timed_out
            .saturating_add(report.attempts.saturating_sub(report.peer_results.len()));
    }
    report.pending_retry = track_pending && !report.delivered();

    if track_pending {
        persist_tx_delivery_report(
            settings,
            txid_hash,
            previous_delivery_attempts,
            chain.height(),
            &chain.tip_hash().to_string(),
            &report,
        )?;
    }

    Ok(report)
}

pub fn broadcast_tx_confirmed(settings: &Settings, tx: &Transaction) -> Result<TxRelayReport> {
    broadcast_tx_confirmed_internal(
        settings,
        tx,
        None,
        HF127_TX_RELAY_MAX_PEERS,
        850,
        true,
    )
}

pub fn broadcast_tx(settings: &Settings, tx: &Transaction) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    if is_pool_share_transaction(tx) {
        return broadcast(settings, WireMessage::Tx { tx: tx.clone() });
    }
    Ok(broadcast_tx_confirmed(settings, tx)?.acknowledged())
}

/// Exact, acknowledgement-based transaction relay. The legacy WireMessage::Tx
/// fallback remains available for older peers, but a socket write is no longer
/// reported to the GUI as confirmed network delivery.
pub fn broadcast_tx_limited(
    settings: &Settings,
    tx: &Transaction,
    max_peers: usize,
    timeout_ms: u64,
) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    if is_pool_share_transaction(tx) {
        if hf106_jin_sale_standardness_policy(tx, settings).is_err() {
            return Ok(0);
        }
        let chain = load_chain_for_hf90_catchup(settings)?;
        let per_peer = Duration::from_millis(timeout_ms.max(150).min(2_000));
        let mut sent = 0usize;
        for addr in prioritized_outbound_peers(settings, max_peers.max(1).min(8))? {
            if should_skip_outbound(settings, &addr) {
                continue;
            }
            let Ok(mut stream) = connect_peer(&addr, per_peer) else {
                continue;
            };
            let _ = stream.set_write_timeout(Some(per_peer));
            let _ = send_version(&mut stream, settings, &chain);
            if send_wire(&mut stream, &WireMessage::Tx { tx: tx.clone() }).is_ok() {
                sent = sent.saturating_add(1);
            }
        }
        return Ok(sent);
    }
    Ok(broadcast_tx_confirmed_internal(
        settings,
        tx,
        None,
        max_peers,
        timeout_ms,
        true,
    )?
    .acknowledged())
}

pub fn rebroadcast_txid_limited(
    settings: &Settings,
    txid: &Hash256,
    max_peers: usize,
    timeout_ms: u64,
) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let tx = chain
        .mempool
        .iter()
        .find(|tx| tx.txid() == *txid)
        .cloned()
        .or_else(|| pending_tx_raw(settings, *txid).ok().flatten());
    let Some(tx) = tx else {
        return Ok(0);
    };
    broadcast_tx_limited(settings, &tx, max_peers, timeout_ms)
}

fn relay_tx_to_known_peers(
    settings: &Settings,
    tx: &Transaction,
    source_peer: Option<&str>,
) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    // HF124/v1.8.1: capture one immutable Fast Chain Engine snapshot for the
    // whole relay fanout. The old loop reloaded canonical state once per peer,
    // multiplying every accepted share into several unnecessary storage reads.
    let chain = load_chain_for_hf90_catchup(settings)?;
    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let max_peers = if is_pool_share_transaction(tx) {
        // HF124/v1.8.1: prevent share storms from opening up to 48 fresh TCP
        // connections per accepted marker. Periodic fair batches provide
        // continued propagation, while eight official-first peers are enough
        // for immediate network diffusion.
        settings.p2p.max_outbound_peers.max(4).min(8)
    } else {
        settings.p2p.max_outbound_peers.max(16).min(48)
    };
    let mut sent = 0usize;
    for addr in prioritized_outbound_peers(settings, max_peers)? {
        let normalized = normalize_peer_addr(&addr);
        if normalized.is_empty() || (!source.is_empty() && normalized == source) {
            continue;
        }
        let Ok(mut stream) = connect_peer(&normalized, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        if send_version(&mut stream, settings, &chain).is_err() {
            continue;
        }
        if send_wire(&mut stream, &WireMessage::Tx { tx: tx.clone() }).is_ok() {
            sent += 1;
        }
    }
    Ok(sent)
}

fn broadcast(settings: &Settings, message: WireMessage) -> Result<usize> {
    // Capture one state snapshot for the full fanout; do not reload Fast Chain
    // Engine state for every destination socket.
    let chain = load_chain_for_hf90_catchup(settings)?;
    let mut sent = 0usize;
    for addr in
        prioritized_outbound_peers(settings, settings.p2p.max_outbound_peers.max(16).min(48))?
    {
        let Ok(mut stream) = connect_peer(&addr, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let _ = send_version(&mut stream, settings, &chain);
        if send_wire(&mut stream, &message).is_ok() {
            sent += 1;
        }
    }
    Ok(sent)
}

fn hf127_push_missing_suffix(
    stream: &mut TcpStream,
    settings: &Settings,
    local: &ChainState,
    session: &PeerSession,
) -> Result<bool> {
    if session.tip_hash.trim().is_empty() || session.height >= local.height() {
        return Ok(false);
    }
    let Some(parent) = local.blocks.get(session.height as usize) else {
        return Ok(false);
    };
    if parent.block_hash().to_string() != session.tip_hash {
        return Ok(false);
    }
    let from_height = session.height.saturating_add(1);
    if local.height().saturating_sub(session.height) == 1 {
        if let Some(block) = local.blocks.last().cloned() {
            send_wire(stream, &WireMessage::Block { block })?;
            return Ok(true);
        }
    }
    send_chain(stream, settings, local, from_height)?;
    Ok(true)
}

fn hf127_announce_live_tip(
    stream: &mut TcpStream,
    settings: &Settings,
    local: &ChainState,
    session: &PeerSession,
    allow_direct_block: bool,
) -> Result<()> {
    send_version(stream, settings, local)?;
    if allow_direct_block {
        let _ = hf127_push_missing_suffix(stream, settings, local, session)?;
    }
    send_inv(stream, local)
}

fn handle_peer(
    mut stream: TcpStream,
    peer_addr: String,
    outbound: bool,
    priority_only: bool,
    settings: Settings,
    chain: Arc<Mutex<ChainState>>,
    peers: Arc<Mutex<HashSet<String>>>,
    active: Arc<Mutex<HashSet<String>>>,
    mempool_dirty: Arc<AtomicBool>,
) -> Result<()> {
    // HF127/v1.8.3: disable Nagle on latency-sensitive peer sessions and
    // use one-second read ticks so a newly accepted tip is announced promptly.
    let _ = stream.set_nodelay(true);
    stream.set_read_timeout(Some(Duration::from_secs(1)))?;
    stream.set_write_timeout(Some(Duration::from_secs(3)))?;
    let peer_key = normalize_peer_addr(&peer_addr);
    {
        let mut a = active.lock().expect("active peer mutex poisoned");
        if !a.insert(peer_key.clone()) {
            return Ok(());
        }
    }

    let result = (|| -> Result<()> {
        let local_chain = chain.lock().expect("chain mutex poisoned").clone();
        send_version(&mut stream, &settings, &local_chain)?;
        // A reserve-lane connection is a short-lived delivery channel. Do not
        // front-load address, mempool or full-chain requests before the submitter
        // can deliver its block. Normal inbound/outbound sessions retain the
        // existing discovery and synchronization handshake.
        if !priority_only {
            send_wire(&mut stream, &WireMessage::GetAddr)?;
            send_wire(&mut stream, &WireMessage::GetPeerList)?;
            send_wire(&mut stream, &WireMessage::GetMempool)?;
            // HF128 waits for the remote Version and requests one bounded
            // missing suffix/overlap. HF127 requested the entire chain from
            // genesis on every outbound connection before knowing peer state.
        }
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut peer_errors = 0u32;
        let mut last_inv = Instant::now();
        let mut last_announced_height = local_chain.height();
        let mut last_announced_tip = local_chain.tip_hash();
        // Persistent sessions must not pin the handshake's chain/UTXO/mempool
        // generation for their entire lifetime. Heartbeats capture fresh views.
        drop(local_chain);
        let mut session = PeerSession::default();
        let priority_handshake_deadline =
            Instant::now() + Duration::from_secs(HF127_PRIORITY_HANDSHAKE_SECS);
        let priority_deadline =
            Instant::now() + Duration::from_secs(HF127_PRIORITY_SESSION_SECS);
        let mut priority_verified = !priority_only;
        let mut priority_role = String::new();
        let mut priority_messages = 0usize;
        let mut priority_block_submissions = 0usize;
        let mut priority_tx_submissions = 0usize;
        let mut normal_tx_submit_window_started = Instant::now();
        let mut normal_tx_submissions = 0usize;

        loop {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(message) => {
                    if priority_only {
                        priority_messages = priority_messages.saturating_add(1);
                        if priority_messages > HF125_PRIORITY_MAX_MESSAGES {
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::Reject {
                                    reason: "priority block-submit session exceeded its message budget"
                                        .to_string(),
                                },
                            );
                            break;
                        }

                        if !priority_verified {
                            match &message {
                                WireMessage::Version { role, .. }
                                    if role == "block-submit" || role == "tx-submit" =>
                                {
                                    priority_verified = true;
                                    priority_role = role.clone();
                                }
                                _ => {
                                    let _ = send_wire(
                                        &mut stream,
                                        &WireMessage::Reject {
                                            reason: "priority inbound slot requires block-submit or tx-submit Version as the first message"
                                                .to_string(),
                                        },
                                    );
                                    break;
                                }
                            }
                        }

                        let priority_message_allowed = match priority_role.as_str() {
                            "block-submit" => matches!(
                                &message,
                                WireMessage::Version { .. }
                                    | WireMessage::SubmitBlock { .. }
                                    | WireMessage::Block { .. }
                                    | WireMessage::Inv { .. }
                                    | WireMessage::Headers { .. }
                                    | WireMessage::Chain { .. }
                                    | WireMessage::GetHeaders { .. }
                                    | WireMessage::GetChain { .. }
                                    | WireMessage::Ping { .. }
                                    | WireMessage::Pong { .. }
                            ),
                            "tx-submit" => matches!(
                                &message,
                                WireMessage::Version { .. }
                                    | WireMessage::SubmitTx { .. }
                                    | WireMessage::Ping { .. }
                                    | WireMessage::Pong { .. }
                            ),
                            _ => false,
                        };
                        if !priority_message_allowed {
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::Reject {
                                    reason: format!(
                                        "priority {} session received a disallowed message",
                                        priority_role
                                    ),
                                },
                            );
                            break;
                        }

                        if matches!(&message, WireMessage::SubmitBlock { .. }) {
                            if priority_role != "block-submit" {
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::Reject {
                                        reason: "tx-submit reserve session cannot submit blocks"
                                            .to_string(),
                                    },
                                );
                                break;
                            }
                            priority_block_submissions =
                                priority_block_submissions.saturating_add(1);
                            if priority_block_submissions > HF125_PRIORITY_MAX_SUBMISSIONS {
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::Reject {
                                        reason: "priority block-submit session exceeded its submission budget"
                                            .to_string(),
                                    },
                                );
                                break;
                            }
                        }

                        if matches!(&message, WireMessage::SubmitTx { .. }) {
                            if priority_role != "tx-submit" {
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::Reject {
                                        reason: "block-submit reserve session cannot submit transactions"
                                            .to_string(),
                                    },
                                );
                                break;
                            }
                            priority_tx_submissions = priority_tx_submissions.saturating_add(1);
                            if priority_tx_submissions > HF127_PRIORITY_MAX_TX_SUBMISSIONS {
                                let _ = send_wire(
                                    &mut stream,
                                    &WireMessage::Reject {
                                        reason: "priority tx-submit session exceeded its submission budget"
                                            .to_string(),
                                    },
                                );
                                break;
                            }
                        }
                    } else if matches!(&message, WireMessage::SubmitTx { .. }) {
                        // SubmitTx performs durable validation/persistence and is
                        // intentionally more expensive than the legacy batched
                        // mempool path. Bound it per persistent session so a
                        // single authenticated network peer cannot force an
                        // unbounded stream of Fast Chain Engine commits.
                        if normal_tx_submit_window_started.elapsed()
                            >= Duration::from_secs(HF127_NORMAL_TX_SUBMIT_WINDOW_SECS)
                        {
                            normal_tx_submit_window_started = Instant::now();
                            normal_tx_submissions = 0;
                        }
                        normal_tx_submissions = normal_tx_submissions.saturating_add(1);
                        if normal_tx_submissions > HF127_NORMAL_TX_SUBMITS_PER_WINDOW {
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::Reject {
                                    reason: "tx-submit session rate limit exceeded; retry after the current delivery window"
                                        .to_string(),
                                },
                            );
                            continue;
                        }
                    }
                    if let WireMessage::Version { node_id: remote_node_id, .. } = &message {
                        if remote_node_id_is_self(&settings, remote_node_id) {
                            if outbound {
                                hf128_quarantine_self_alias(&settings, &peer_key);
                                if let Ok(mut known) = peers.lock() {
                                    known.remove(&peer_key);
                                }
                            }
                            let _ = send_wire(
                                &mut stream,
                                &WireMessage::Reject {
                                    reason: "self connection rejected by node identity".to_string(),
                                },
                            );
                            bail!("self connection rejected for peer alias {peer_key}");
                        }
                    }
                    if let Err(err) = process_peer_message(
                        &settings,
                        &peer_key,
                        message,
                        &mut stream,
                        &chain,
                        &peers,
                        &mempool_dirty,
                        &mut session,
                        outbound,
                    ) {
                        peer_errors = peer_errors.saturating_add(1);
                        let _ = send_wire(
                            &mut stream,
                            &WireMessage::Reject {
                                reason: err.to_string(),
                            },
                        );
                        if peer_errors >= settings.p2p.max_peer_errors.max(1) {
                            bail!("peer exceeded invalid-message score");
                        }
                    }
                    // HF127/v1.8.3: a changed canonical tip is announced on the
                    // next one-second coordination tick even under continuous
                    // traffic. The four-second heartbeat remains as a fallback.
                    let local = chain.lock().expect("chain mutex poisoned").clone();
                    let tip_changed = local.height() != last_announced_height
                        || local.tip_hash() != last_announced_tip;
                    if tip_changed || last_inv.elapsed() >= Duration::from_secs(4) {
                        hf127_announce_live_tip(
                            &mut stream,
                            &settings,
                            &local,
                            &session,
                            !priority_only,
                        )?;
                        last_announced_height = local.height();
                        last_announced_tip = local.tip_hash();
                        last_inv = Instant::now();
                    }
                }
                Err(err) if is_timeout(&err) => {
                    if priority_only
                        && ((!priority_verified
                            && Instant::now() >= priority_handshake_deadline)
                            || Instant::now() >= priority_deadline)
                    {
                        break;
                    }
                    let local = chain.lock().expect("chain mutex poisoned").clone();
                    let tip_changed = local.height() != last_announced_height
                        || local.tip_hash() != last_announced_tip;
                    if tip_changed || last_inv.elapsed() >= Duration::from_secs(4) {
                        // Heartbeat identity/status too, not just inv. Priority
                        // delivery sessions remain narrow and never request peer
                        // discovery or a full mempool.
                        hf127_announce_live_tip(
                            &mut stream,
                            &settings,
                            &local,
                            &session,
                            !priority_only,
                        )?;
                        if !priority_only {
                            send_wire(&mut stream, &WireMessage::GetAddr)?;
                            send_wire(&mut stream, &WireMessage::GetPeerList)?;
                            send_wire(&mut stream, &WireMessage::GetMempool)?;
                        }
                        last_announced_height = local.height();
                        last_announced_tip = local.tip_hash();
                        last_inv = Instant::now();
                    }
                }
                Err(err) if err.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    })();

    active
        .lock()
        .expect("active peer mutex poisoned")
        .remove(&peer_key);
    result
}

fn hf127_is_submit_role(role: &str) -> bool {
    matches!(role.trim(), "block-submit" | "tx-submit")
}

fn process_peer_message(
    settings: &Settings,
    peer_addr: &str,
    message: WireMessage,
    stream: &mut TcpStream,
    chain: &Arc<Mutex<ChainState>>,
    peers: &Arc<Mutex<HashSet<String>>>,
    mempool_dirty: &Arc<AtomicBool>,
    session: &mut PeerSession,
    outbound_session: bool,
) -> Result<()> {
    match message {
        WireMessage::Version {
            protocol,
            network,
            magic,
            height,
            tip_hash,
            listen_addr,
            genesis_hash,
            user_agent,
            node_id: remote_node_id,
            role,
            miner_address,
            ..
        } => {
            if protocol != PROTOCOL_VERSION {
                bail!("unsupported protocol {protocol}");
            }
            validate_remote_network(settings, &network, &magic, &genesis_hash)?;
            session.height = height;
            session.tip_hash = tip_hash.clone();
            session.user_agent = user_agent.clone();
            session.node_id = remote_node_id.clone();
            session.listen_addr = listen_addr.clone();
            session.role = role.clone();
            session.miner_address = miner_address.clone();
            // Short-lived block/transaction submit sessions use ephemeral
            // source ports. Never persist those observed endpoints into the
            // peerbook; doing so polluted future dial rounds with unreachable
            // addresses and delayed both block and transaction propagation.
            if !hf127_is_submit_role(&role) {
                record_peer_observation(
                    settings,
                    peer_addr,
                    &remote_node_id,
                    &listen_addr,
                    height,
                    &tip_hash,
                    &user_agent,
                    &role,
                    &miner_address,
                )?;
                // The observed remote TCP endpoint is dialable only for an
                // outbound session. On inbound sessions it is normally an
                // ephemeral source port and must not poison future dial rounds.
                if outbound_session {
                    add_peer(settings, peers, peer_addr)?;
                }
                if !listen_addr.trim().is_empty() {
                    add_peer(settings, peers, &listen_addr)?;
                }
            }
            let local_snapshot = chain.lock().expect("chain mutex poisoned").clone();
            let local_height = local_snapshot.height();
            let local_tip = local_snapshot.tip_hash().to_string();
            if height > local_height || (height == local_height && tip_hash != local_tip) {
                let _ = request_peer_chain_if_needed(
                    stream,
                    &local_snapshot,
                    height,
                    &tip_hash,
                    session,
                )?;
            } else if height < local_height {
                let _ = hf127_push_missing_suffix(stream, settings, &local_snapshot, session)?;
            }
            send_inv(stream, &local_snapshot)?;
        }
        WireMessage::Inv {
            height, tip_hash, ..
        } => {
            session.height = height;
            session.tip_hash = tip_hash.clone();
            let local = chain.lock().expect("chain mutex poisoned");
            let local_height = local.height();
            let local_tip = local.tip_hash().to_string();
            drop(local);
            if height > local_height || (height == local_height && tip_hash != local_tip) {
                let local_snapshot = chain.lock().expect("chain mutex poisoned").clone();
                let _ = request_peer_chain_if_needed(
                    stream,
                    &local_snapshot,
                    height,
                    &tip_hash,
                    session,
                )?;
            }
        }
        WireMessage::GetHeaders { from_height } => {
            let local = chain.lock().expect("chain mutex poisoned").clone();
            send_headers(stream, settings, &local, from_height)?;
        }
        WireMessage::Headers { headers } => {
            let local = chain.lock().expect("chain mutex poisoned").clone();
            if let Some(last) = headers.last() {
                if last.height > local.height() {
                    let _ = request_peer_chain_if_needed(
                        stream,
                        &local,
                        last.height,
                        &last.hash,
                        session,
                    )?;
                }
            }
        }
        WireMessage::GetChain { from_height } => {
            let local = chain.lock().expect("chain mutex poisoned").clone();
            send_chain(stream, settings, &local, from_height)?;
        }
        WireMessage::Chain {
            start_height,
            blocks,
        } => {
            if blocks.len() > settings.p2p.max_blocks_per_message {
                bail!("too many blocks in chain message");
            }
            // Incoming peer sessions share the existing manual/background sync
            // writer. Never queue peer threads with full candidate histories.
            // A busy response is discarded before taking a chain snapshot;
            // normal Version/Inv request expiry retries it on a later tick.
            let Some(_writer) = hf129_acquire_catchup_writer(
                false, Instant::now() + Duration::from_millis(50),
            )? else {
                return Ok(());
            };
            if blocks.is_empty() {
                let local = chain.lock().expect("chain mutex poisoned").clone();
                let _ = request_earlier_peer_chain(stream, &local, session, start_height)?;
                return Ok(());
            }
            let next_from = start_height.saturating_add(blocks.len() as u32);
            let base = chain.lock().expect("chain mutex poisoned").clone();
            if hf132_chunk_already_known(&base, start_height, &blocks) {
                request_peer_chain_if_needed(stream, &base, session.height, &session.tip_hash.clone(), session)?;
                return Ok(());
            }
            let mut candidate = base.clone();
            let mut changed = false;
            if start_height <= candidate.height().saturating_add(1) {
                match try_adopt_overlapping_blocks(
                    &mut candidate,
                    start_height,
                    blocks,
                    settings,
                    prefer_peer_tip_on_equal_work(settings, peer_addr),
                ) {
                    Ok(true) => {
                        // HF125/v1.8.2 transactional chain adoption: persist the
                        // isolated candidate before publishing it to the live
                        // owner. A failed disk commit cannot advance memory.
                        save_chain(settings, &candidate)?;
                        let mut live = chain.lock().expect("chain mutex poisoned");
                        if live.tip_hash() == base.tip_hash() {
                            *live = candidate;
                            changed = true;
                        } else {
                            // Candidate validation already ran on the private
                            // snapshot. Never replay history under the live lock.
                            changed = live.try_adopt_validated_peer_chain(
                                candidate,
                                settings,
                                prefer_peer_tip_on_equal_work(settings, peer_addr),
                            )?;
                        }
                        if changed {
                            mempool_dirty.store(false, Ordering::Release);
                        }
                    }
                    Ok(false) => {
                        let _ = request_earlier_peer_chain(
                            stream,
                            &base,
                            session,
                            start_height,
                        )?;
                    }
                    Err(_) => {
                        let _ = request_earlier_peer_chain(
                            stream,
                            &base,
                            session,
                            start_height,
                        )?;
                    }
                }
            } else {
                let peer_height = session.height;
                let peer_tip = session.tip_hash.clone();
                request_peer_chain_from(
                    stream,
                    &base,
                    peer_height,
                    &peer_tip,
                    base.height().saturating_add(1),
                    session,
                )?;
            }
            if changed {
                hf132_defer_chain_relay(settings, peer_addr);
            }
            if changed && next_from <= session.height {
                let local = chain.lock().expect("chain mutex poisoned").clone();
                let peer_height = session.height;
                let peer_tip = session.tip_hash.clone();
                request_peer_chain_from(
                    stream,
                    &local,
                    peer_height,
                    &peer_tip,
                    next_from,
                    session,
                )?;
            }
        }
        WireMessage::Block { block } => {
            let relay_block = block.clone();
            let accepted = accept_submitted_block_live(settings, chain, mempool_dirty, block);
            if accepted.status == "stale_parent" {
                let h = chain.lock().expect("chain mutex poisoned").height();
                request_earlier_fork_window(stream, h, h.saturating_add(1))?;
            }
            if accepted.changed {
                let relay_settings = settings.clone();
                let source = peer_addr.to_string();
                hf127_spawn_detached("qub-block-diffusion", move || {
                    let _ = broadcast_block_confirmed_internal(
                        &relay_settings,
                        &relay_block,
                        Some(&source),
                        false,
                    );
                });
            }
        }
        WireMessage::SubmitBlock { request_id, block } => {
            let relay_block = block.clone();
            let accepted = accept_submitted_block_live(settings, chain, mempool_dirty, block);
            if accepted.changed {
                // Start outward diffusion before acknowledging the producer.
                // The durable commit already succeeded; spawning first removes
                // the producer-only propagation head start without delaying ACK.
                let relay_settings = settings.clone();
                let source = peer_addr.to_string();
                hf127_spawn_detached("qub-block-diffusion", move || {
                    let _ = broadcast_block_confirmed_internal(
                        &relay_settings,
                        &relay_block,
                        Some(&source),
                        false,
                    );
                });
            }
            send_block_ack(stream, request_id, &accepted)?;
        }
        WireMessage::BlockAck { .. } => {}
        WireMessage::SubmitTx { request_id, tx } => {
            let relay_tx = tx.clone();
            let accepted = accept_submitted_tx_live(settings, chain, mempool_dirty, tx);
            if accepted.changed {
                let relay_settings = settings.clone();
                let source = peer_addr.to_string();
                hf127_spawn_detached("qub-tx-diffusion", move || {
                    let _ = broadcast_tx_confirmed_internal(
                        &relay_settings,
                        &relay_tx,
                        Some(&source),
                        8,
                        850,
                        false,
                    );
                });
            }
            send_tx_ack(stream, request_id, &accepted)?;
        }
        WireMessage::TxAck { .. } => {}
        WireMessage::Tx { tx } => {
            let (accepted, mempool_changed) = {
                let mut local = chain.lock().expect("chain mutex poisoned");
                let before_len = local.mempool.len();
                let accepted = local
                    .accept_transaction_to_mempool(tx.clone(), settings)
                    .is_ok();
                (accepted, accepted || local.mempool.len() != before_len)
            };
            if mempool_changed {
                // Admission may also prune structurally stale pool shares before
                // rejecting a new transaction. Persist that real local policy
                // change even when the incoming transaction itself was rejected.
                mempool_dirty.store(true, Ordering::Release);
            }
            if accepted && !is_pool_share_transaction(&tx) {
                let relay_settings = settings.clone();
                let relay_tx = tx.clone();
                let source = peer_addr.to_string();
                hf127_spawn_detached("qub-tx-diffusion", move || {
                    let _ = broadcast_tx_confirmed_internal(
                        &relay_settings,
                        &relay_tx,
                        Some(&source),
                        8,
                        850,
                        false,
                    );
                });
            }
            // Inbound pool shares are propagated by the six-second fair batch
            // heartbeat. Deferring their fanout prevents legacy peers that send
            // shares one-by-one from forcing a new outbound socket burst here.
        }
        WireMessage::GetMempool => {
            let txs = {
                let local = chain.lock().expect("chain mutex poisoned");
                hf115_mempool_relay_batch(settings, &local.mempool)
            };
            send_wire(stream, &WireMessage::Mempool { txs })?;
        }
        WireMessage::Mempool { txs } => {
            let (accepted_txs, mempool_changed) = {
                let mut local = chain.lock().expect("chain mutex poisoned");
                let before_len = local.mempool.len();
                let accepted = local.accept_transactions_to_mempool_batch(
                    txs.into_iter()
                        .take(hf115_mempool_inbound_limit(settings)),
                    settings,
                )?;
                let changed = !accepted.is_empty() || local.mempool.len() != before_len;
                (accepted, changed)
            };
            if mempool_changed {
                mempool_dirty.store(true, Ordering::Release);
            }
            if !accepted_txs.is_empty() {
                let relay_batch = hf124_fair_mempool_relay_batch(
                    settings,
                    &accepted_txs,
                    HF116_MEMPOOL_RELAY_BATCH_TXS,
                );
                let _ = relay_mempool_batch_to_known_peers(
                    settings,
                    &relay_batch,
                    Some(peer_addr),
                    settings.p2p.max_outbound_peers.max(4).min(8),
                );
            }
        }
        WireMessage::GetAddr => {
            let mut addrs = peers
                .lock()
                .expect("peer mutex poisoned")
                .iter()
                .take(96)
                .cloned()
                .collect::<Vec<_>>();
            // Include recent registry listen/observed addresses too. This makes peer
            // exchange useful even when the seed is not reachable later.
            if let Ok(registry) = load_peer_registry(settings) {
                for observed in registry.peers.into_iter().take(96) {
                    let listen = normalize_peer_addr(&observed.listen_addr);
                    let seen = normalize_peer_addr(&observed.observed_addr);
                    if !listen.is_empty() {
                        addrs.push(listen);
                    }
                    if !seen.is_empty() {
                        addrs.push(seen);
                    }
                }
            }
            addrs.sort();
            addrs.dedup();
            addrs.retain(|addr| !is_self_or_empty_addr(settings, addr));
            addrs.truncate(128);
            send_wire(stream, &WireMessage::Addr { addrs })?;
        }
        WireMessage::Addr { addrs } => {
            for addr in addrs.into_iter().take(128) {
                add_peer(settings, peers, &addr)?;
            }
        }
        WireMessage::GetPeerList => {
            send_wire(
                stream,
                &WireMessage::PeerList {
                    peers: load_peer_registry(settings)?.peers,
                },
            )?;
        }
        WireMessage::PeerList { peers: observed } => {
            merge_peer_registry(settings, observed)?;
        }
        WireMessage::Ping { nonce } => send_wire(stream, &WireMessage::Pong { nonce })?,
        WireMessage::Pong { .. } => {}
        WireMessage::Reject { reason } => eprintln!("peer {peer_addr} reject: {reason}"),
    }
    refresh_session_observation(settings, peer_addr, session);
    Ok(())
}

fn refresh_session_observation(settings: &Settings, peer_addr: &str, session: &PeerSession) {
    if session.node_id.trim().is_empty() || hf127_is_submit_role(&session.role) {
        return;
    }
    let _ = record_peer_observation(
        settings,
        peer_addr,
        &session.node_id,
        &session.listen_addr,
        session.height,
        &session.tip_hash,
        &session.user_agent,
        &session.role,
        &session.miner_address,
    );
}

fn process_client_message(
    settings: &Settings,
    addr: &str,
    message: WireMessage,
    stream: &mut TcpStream,
    report: &mut P2PSyncReport,
    session: &mut PeerSession,
) -> Result<()> {
    match message {
        WireMessage::Version {
            protocol,
            network,
            magic,
            genesis_hash,
            height,
            tip_hash,
            user_agent,
            listen_addr,
            node_id: remote_node_id,
            role,
            miner_address,
            ..
        } => {
            if protocol != PROTOCOL_VERSION {
                bail!("unsupported protocol {protocol}");
            }
            validate_remote_network(settings, &network, &magic, &genesis_hash)?;
            if remote_node_id_is_self(settings, &remote_node_id) {
                hf128_quarantine_self_alias(settings, addr);
                bail!("self connection rejected for {addr}");
            }
            if !hf127_is_submit_role(&role) {
                record_peer_observation(
                    settings,
                    addr,
                    &remote_node_id,
                    &listen_addr,
                    height,
                    &tip_hash,
                    &user_agent,
                    &role,
                    &miner_address,
                )?;
            }
            session.height = height;
            session.tip_hash = tip_hash.clone();
            hf129_observe_tip(report, height, &tip_hash);
            let local = load_chain_for_hf90_catchup(settings)?;
            if height > local.height()
                || (height == local.height() && tip_hash != local.tip_hash().to_string())
            {
                request_peer_chain_if_needed(stream, &local, height, &tip_hash, session)?;
            }
        }
        WireMessage::Chain {
            start_height,
            blocks,
        } => {
            if blocks.len() > settings.p2p.max_blocks_per_message {
                bail!("too many blocks in chain message");
            }
            if blocks.is_empty() {
                return Ok(());
            }
            let more_expected = blocks.len() == settings.p2p.max_blocks_per_message;
            let next_from = start_height.saturating_add(blocks.len() as u32);
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let mut changed = false;
            let already_known = hf132_chunk_already_known(&local, start_height, &blocks);
            if !already_known && start_height <= local.height().saturating_add(1) {
                let local_height_before = local.height();
                match try_adopt_overlapping_blocks(
                    &mut local,
                    start_height,
                    blocks,
                    settings,
                    prefer_peer_tip_on_equal_work(settings, addr),
                ) {
                    Ok(true) => {
                        save_chain(settings, &local)?;
                        report.chains_adopted = report.chains_adopted.saturating_add(1);
                        report.blocks_connected = report.blocks_connected.saturating_add(
                            local.height().saturating_sub(local_height_before) as usize,
                        );
                        changed = true;
                    }
                    Ok(false) => {
                        if start_height == local_height_before.saturating_add(1) {
                            request_earlier_fork_window(stream, local_height_before, start_height)?;
                        }
                    }
                    Err(_) => {
                        request_earlier_fork_window(stream, local_height_before, start_height)?;
                    }
                }
            } else if !already_known {
                request_peer_chain_if_needed(stream, &local, session.height, &session.tip_hash.clone(), session)?;
            }
            if changed {
                hf132_defer_chain_relay(settings, addr);
            }
            if session.height > local.height()
                || (more_expected && next_from > local.height())
            {
                // Follow the latest advertisement from the committed parent.
                // Duplicate/overlapping frames must never restart old windows.
                request_peer_chain_if_needed(stream, &local,
                    session.height.max(next_from), &session.tip_hash.clone(), session)?;
            }
        }
        WireMessage::Block { block } => {
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let mut changed = false;
            if block.header.prev_block_hash == local.tip_hash() {
                connect_block_persist_atomic(settings, &mut local, block)?;
                report.blocks_connected = report.blocks_connected.saturating_add(1);
                changed = true;
            } else {
                let h = local.height();
                request_earlier_fork_window(stream, h, h.saturating_add(1))?;
            }
            if changed {
                hf132_defer_chain_relay(settings, addr);
            }
        }
        WireMessage::SubmitBlock { request_id, block } => {
            let relay_block = block.clone();
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let accepted = block_acceptance_from_chain(settings, &mut local, block);
            if accepted.changed {
                report.blocks_connected = report.blocks_connected.saturating_add(1);
                let relay_settings = settings.clone();
                let source = addr.to_string();
                hf127_spawn_detached("qub-block-chain-diffusion", move || {
                    let _ = broadcast_block_confirmed_internal(
                        &relay_settings,
                        &relay_block,
                        Some(&source),
                        false,
                    );
                    let _ = relay_chain_to_known_peers(&relay_settings, Some(&source));
                });
            }
            send_block_ack(stream, request_id, &accepted)?;
        }
        WireMessage::BlockAck { .. } => {}
        WireMessage::SubmitTx { request_id, tx } => {
            let relay_tx = tx.clone();
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let accepted = tx_acceptance_from_chain(settings, &mut local, tx);
            if accepted.changed {
                report.txs_accepted = report.txs_accepted.saturating_add(1);
                let relay_settings = settings.clone();
                let source = addr.to_string();
                hf127_spawn_detached("qub-tx-diffusion", move || {
                    let _ = broadcast_tx_confirmed_internal(
                        &relay_settings,
                        &relay_tx,
                        Some(&source),
                        8,
                        850,
                        false,
                    );
                });
            }
            send_tx_ack(stream, request_id, &accepted)?;
        }
        WireMessage::TxAck { .. } => {}
        WireMessage::Tx { tx } => {
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let before_len = local.mempool.len();
            let accepted = local
                .accept_transaction_to_mempool(tx.clone(), settings)
                .is_ok();
            if accepted || local.mempool.len() != before_len {
                // Even a rejected transaction may trigger deterministic cleanup
                // of stale/unconfirmable pool shares. Persist that real policy
                // change in the standalone sync/client path as well.
                save_chain(settings, &local)?;
            }
            if accepted {
                report.txs_accepted += 1;
                if !is_pool_share_transaction(&tx) {
                    let relay_settings = settings.clone();
                    let relay_tx = tx.clone();
                    let source = addr.to_string();
                    hf127_spawn_detached("qub-tx-diffusion", move || {
                        let _ = broadcast_tx_confirmed_internal(
                            &relay_settings,
                            &relay_tx,
                            Some(&source),
                            8,
                            850,
                            false,
                        );
                    });
                }
            }
        }
        WireMessage::Addr { addrs } => {
            let mut set = load_peer_set(settings)?;
            for addr in addrs.into_iter().take(128) {
                set.insert(normalize_peer_addr(&addr));
            }
            save_peer_set(settings, &set)?;
        }
        WireMessage::PeerList { peers } => {
            merge_peer_registry(settings, peers)?;
        }
        WireMessage::GetHeaders { from_height } => send_headers(
            stream,
            settings,
            &load_chain_for_hf90_catchup(settings)?,
            from_height,
        )?,
        WireMessage::Headers { headers } => {
            let local = load_chain_for_hf90_catchup(settings)?;
            if let Some(last) = headers.last() {
                if last.height > session.height {
                    session.height = last.height;
                    session.tip_hash = last.hash.clone();
                }
                request_peer_chain_if_needed(stream, &local, session.height, &session.tip_hash.clone(), session)?;
            }
        }
        WireMessage::GetChain { from_height } => send_chain(
            stream,
            settings,
            &load_chain_for_hf90_catchup(settings)?,
            from_height,
        )?,
        WireMessage::GetMempool => {
            let local = load_chain_for_hf90_catchup(settings)?;
            let txs = hf115_mempool_relay_batch(settings, &local.mempool);
            send_wire(stream, &WireMessage::Mempool { txs })?;
        }
        WireMessage::Mempool { txs } => {
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let before_len = local.mempool.len();
            let accepted_txs = local.accept_transactions_to_mempool_batch(
                txs.into_iter()
                    .take(hf115_mempool_inbound_limit(settings)),
                settings,
            )?;
            if !accepted_txs.is_empty() || local.mempool.len() != before_len {
                save_chain(settings, &local)?;
            }
            if !accepted_txs.is_empty() {
                report.txs_accepted = report.txs_accepted.saturating_add(accepted_txs.len());
            }
            if !accepted_txs.is_empty() {
                let relay_batch = hf124_fair_mempool_relay_batch(
                    settings,
                    &accepted_txs,
                    HF116_MEMPOOL_RELAY_BATCH_TXS,
                );
                let _ = relay_mempool_batch_to_known_peers(
                    settings,
                    &relay_batch,
                    Some(addr),
                    settings.p2p.max_outbound_peers.max(4).min(8),
                );
            }
        }
        WireMessage::Ping { nonce } => send_wire(stream, &WireMessage::Pong { nonce })?,
        WireMessage::Inv {
            height, tip_hash, ..
        } => {
            session.height = height;
            session.tip_hash = tip_hash.clone();
            hf129_observe_tip(report, height, &tip_hash);
            let local = load_chain_for_hf90_catchup(settings)?;
            if height > local.height()
                || (height == local.height() && tip_hash != local.tip_hash().to_string())
            {
                request_peer_chain_if_needed(stream, &local, height, &tip_hash, session)?;
            }
        }
        WireMessage::GetAddr
        | WireMessage::GetPeerList
        | WireMessage::Pong { .. }
        | WireMessage::Reject { .. } => {}
    }
    Ok(())
}

fn version_message_with_role(settings: &Settings, chain: &ChainState, role: &str) -> Result<WireMessage> {
    Ok(WireMessage::Version {
        protocol: PROTOCOL_VERSION,
        network: settings.network.name.clone(),
        magic: settings.network.magic.clone(),
        user_agent: USER_AGENT.to_string(),
        height: chain.height(),
        tip_hash: chain.tip_hash().to_string(),
        work: chain.total_work_hex().unwrap_or_else(|_| "0".to_string()),
        genesis_hash: genesis_block(settings)?.block_hash().to_string(),
        listen_addr: effective_advertise_addr(settings),
        node_id: node_id(settings)?,
        role: role.to_string(),
        miner_address: runtime_miner_address(settings)
            .unwrap_or_else(|| settings.mining.miner_address.clone()),
    })
}

fn send_version_with_role(stream: &mut TcpStream, settings: &Settings, chain: &ChainState, role: &str) -> Result<()> {
    send_wire(stream, &version_message_with_role(settings, chain, role)?)
}

fn send_version(stream: &mut TcpStream, settings: &Settings, chain: &ChainState) -> Result<()> {
    let role = if settings.mining.enabled {
        "miner"
    } else {
        "node"
    };
    send_version_with_role(stream, settings, chain, role)
}

fn send_inv(stream: &mut TcpStream, chain: &ChainState) -> Result<()> {
    send_wire(
        stream,
        &WireMessage::Inv {
            height: chain.height(),
            tip_hash: chain.tip_hash().to_string(),
            work: chain.total_work_hex().unwrap_or_else(|_| "0".to_string()),
        },
    )
}

fn send_headers(
    stream: &mut TcpStream,
    settings: &Settings,
    chain: &ChainState,
    from_height: u32,
) -> Result<()> {
    let start = from_height as usize;
    let end = chain
        .blocks
        .len()
        .min(start.saturating_add(settings.p2p.max_blocks_per_message));
    let headers = if start < chain.blocks.len() {
        chain.blocks[start..end]
            .iter()
            .enumerate()
            .map(|(offset, block)| IndexedHeader {
                height: from_height.saturating_add(offset as u32),
                hash: block.block_hash().to_string(),
                header: block.header.clone(),
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    send_wire(stream, &WireMessage::Headers { headers })
}

#[derive(Serialize)]
struct Hf132BorrowedChain<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    start_height: u32,
    blocks: &'a [Block],
}

fn send_chain(
    stream: &mut TcpStream,
    settings: &Settings,
    chain: &ChainState,
    from_height: u32,
) -> Result<()> {
    let start = from_height as usize;
    let end = chain
        .blocks
        .len()
        .min(start.saturating_add(settings.p2p.max_blocks_per_message));
    // Preserve protocol-2 rolling compatibility: HF127 peers request the next
    // window only when this count reaches max_blocks_per_message. A shorter
    // byte-bounded response would deserialize on HF127 but silently stop its
    // sync. Framing changes therefore require an explicit negotiated
    // capability and are intentionally not introduced by HF128.
    let blocks = chain.blocks.get(start..end).unwrap_or(&[]);
    hf132_send_json(stream, &Hf132BorrowedChain {
        kind: "chain", start_height: from_height, blocks,
    })
}

fn send_wire(stream: &mut TcpStream, message: &WireMessage) -> Result<()> {
    hf132_send_json(stream, message)
}

struct Hf132WireWriter<'a> {
    stream: &'a mut TcpStream,
    deadline: Instant,
    wrote_any: bool,
}

impl Write for Hf132WireWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() { return Ok(0); }
        loop {
            let left = hf130_io_remaining(self.deadline)?;
            self.stream.set_write_timeout(Some(left.min(Duration::from_millis(200))))?;
            match self.stream.write(&bytes[..bytes.len().min(64 * 1024)]) {
                Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "peer write returned zero")),
                Ok(count) => { self.wrote_any = true; return Ok(count); }
                Err(err) if is_timeout(&err) => continue,
                Err(err) => return Err(err),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> { self.stream.flush() }
}

fn hf132_send_json<T: Serialize + ?Sized>(stream: &mut TcpStream, message: &T) -> Result<()> {
    let original_timeout = stream.write_timeout()?;
    let timeout = original_timeout.unwrap_or(Duration::from_secs(30));
    let deadline = hf130_effective_deadline(Instant::now() + timeout);
    let _scope = Hf130IoScope::deadline(deadline);
    // Serialize through a bounded 64 KiB buffer. Avoid a second, potentially
    // full-chain JSON allocation for every slow peer. into_parts prevents
    // BufWriter::drop from attempting to flush a failed partial frame again.
    let (result, sent) = {
        let writer = Hf132WireWriter { stream: &mut *stream, deadline, wrote_any: false };
        let mut buffered = io::BufWriter::with_capacity(64 * 1024, writer);
        let result = (|| -> Result<()> {
            hf130_io_remaining(deadline)?;
            serde_json::to_writer(&mut buffered, message)?;
            buffered.write_all(b"\n")?;
            buffered.flush()?;
            Ok(())
        })();
        let (writer, _) = buffered.into_parts();
        (result, writer.wrote_any)
    };
    let restored = stream.set_write_timeout(original_timeout);
    if result.is_err() && sent {
        // A partial JSON frame cannot safely be followed by another message.
        let _ = stream.shutdown(std::net::Shutdown::Both);
    }
    if result.is_ok() { restored?; }
    result
}

fn read_wire(reader: &mut BufReader<TcpStream>, max_bytes: usize) -> io::Result<WireMessage> {
    let original_timeout = reader.get_ref().read_timeout()?;
    let timeout = original_timeout.unwrap_or(Duration::from_secs(30));
    let deadline = hf130_effective_deadline(Instant::now() + timeout);
    let _scope = Hf130IoScope::deadline(deadline);
    let result = read_wire_until(reader, max_bytes, deadline);
    // Preserve the session timeout on success AND failure. Non-timeout errors
    // close the stream so callers cannot retry a discarded partial frame.
    let _ = reader.get_ref().set_read_timeout(original_timeout);
    if result.as_ref().err().map_or(false, |err| !is_timeout(err)) {
        let _ = reader.get_ref().shutdown(std::net::Shutdown::Both);
    }
    result
}

fn read_wire_until(
    reader: &mut BufReader<TcpStream>,
    max_bytes: usize,
    deadline: Instant,
) -> io::Result<WireMessage> {
    let mut raw = Vec::new();
    loop {
        let left = hf130_io_remaining(deadline).map_err(|err| {
            if raw.is_empty() || err.kind() == io::ErrorKind::Interrupted {
                err
            } else {
                // A partial frame cannot be discarded and then retried as if
                // the next bytes began a fresh message on the same connection.
                io::Error::new(io::ErrorKind::InvalidData, "incomplete frame at deadline")
            }
        })?;
        reader.get_ref().set_read_timeout(Some(left.min(Duration::from_millis(200))))?;
        let available = match reader.fill_buf() {
            Ok(bytes) => bytes,
            Err(err) if is_timeout(&err) => continue,
            Err(err) => return Err(err),
        };
        if available.is_empty() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed"));
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(available.len());
        if take > max_bytes.saturating_sub(raw.len()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }
        raw.extend_from_slice(&available[..take]);
        reader.consume(take);
        if raw.last() == Some(&b'\n') {
            break;
        }
    }
    hf130_io_remaining(deadline)?;
    serde_json::from_slice(&raw).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

fn is_timeout(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

fn validate_remote_network(
    settings: &Settings,
    network: &str,
    magic: &str,
    genesis_hash: &str,
) -> Result<()> {
    if network != settings.network.name {
        bail!("network mismatch");
    }
    if magic != settings.network.magic.as_str() {
        bail!("magic mismatch");
    }
    if genesis_hash != genesis_block(settings)?.block_hash().to_string() {
        bail!("genesis hash mismatch");
    }
    Ok(())
}

// The platform resolver cannot be cancelled safely. Keep at most four pending
// resolutions process-wide and share one pending request for the same name.
// Completed entries are short-lived and the cache is bounded independently.
struct Hf130DnsEntry {
    created: Instant,
    result: Mutex<Option<std::result::Result<Vec<SocketAddr>, String>>>,
}

fn hf130_dns_entries() -> &'static Mutex<HashMap<String, Arc<Hf130DnsEntry>>> {
    static DNS: OnceLock<Mutex<HashMap<String, Arc<Hf130DnsEntry>>>> = OnceLock::new();
    DNS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hf130_resolve_peer(addr: &str, deadline: Instant) -> Result<Vec<SocketAddr>> {
    hf130_io_remaining(deadline)?;
    let entry = {
        let mut entries = hf130_dns_entries().lock().map_err(|_| anyhow::anyhow!("DNS cache poisoned"))?;
        entries.retain(|_, entry| {
            entry.created.elapsed() < Duration::from_secs(60)
                || entry.result.lock().map(|r| r.is_none()).unwrap_or(false)
        });
        if let Some(entry) = entries.get(addr) {
            entry.clone()
        } else {
            let pending = entries.values().filter(|entry| {
                entry.result.lock().map(|r| r.is_none()).unwrap_or(false)
            }).count();
            if pending >= 4 {
                bail!("DNS resolver capacity busy; retry on the next bounded sync pass");
            }
            if entries.len() >= 64 {
                if let Some(oldest) = entries.iter()
                    .filter(|(_, entry)| entry.result.lock().map(|r| r.is_some()).unwrap_or(false))
                    .min_by_key(|(_, entry)| entry.created)
                    .map(|(name, _)| name.clone())
                {
                    entries.remove(&oldest);
                }
            }
            let entry = Arc::new(Hf130DnsEntry { created: Instant::now(), result: Mutex::new(None) });
            let worker_entry = entry.clone();
            let peer = addr.to_string();
            thread::Builder::new().name("qub-peer-dns".to_string()).spawn(move || {
                let resolved = peer.to_socket_addrs()
                    .map(|iter| iter.take(32).collect::<Vec<SocketAddr>>())
                    .map_err(|err| err.to_string());
                if let Ok(mut slot) = worker_entry.result.lock() {
                    *slot = Some(resolved);
                }
            }).with_context(|| format!("could not start resolver for {addr}"))?;
            entries.insert(addr.to_string(), entry.clone());
            entry
        }
    };
    loop {
        let left = hf130_io_remaining(deadline)?;
        if let Some(result) = entry.result.lock()
            .map_err(|_| anyhow::anyhow!("DNS result poisoned"))?.as_ref()
        {
            return result.clone().map_err(|err| anyhow::anyhow!("bad peer address {addr}: {err}"));
        }
        thread::sleep(left.min(Duration::from_millis(10)));
    }
}

fn hf130_connect_slice(remaining: Duration, cooperative: bool) -> Duration {
    if cooperative { remaining.min(Duration::from_secs(1)) } else { remaining }
}

fn connect_peer(addr: &str, timeout: Duration) -> Result<TcpStream> {
    let deadline = hf130_effective_deadline(Instant::now() + timeout);
    let _scope = Hf130IoScope::deadline(deadline);
    let sockets = if let Ok(socket) = addr.parse::<SocketAddr>() {
        vec![socket]
    } else {
        hf130_resolve_peer(addr, deadline)?
    };
    let mut last_err = None;
    let socket_count = sockets.len();
    for (index, socket) in sockets.into_iter().enumerate() {
        let address_budget = hf130_io_remaining(deadline)? / (socket_count - index) as u32;
        let address_deadline = Instant::now() + address_budget;
        loop {
            let left = match hf130_io_remaining(address_deadline) {
                Ok(left) => left,
                Err(err) if err.kind() == io::ErrorKind::TimedOut => {
                    last_err = Some(err);
                    break;
                }
                Err(err) => return Err(err.into()),
            };
            // connect_timeout creates a NEW connection on each attempt. Do
            // not use the 200ms IO polling interval here: that would repeatedly
            // discard valid handshakes on slower routes. Cancellation may wait
            // up to one second during connect; ordinary calls keep their full
            // allotted per-address timeout.
            let cooperative = hf130_io_context().cancel.is_some();
            let slice = hf130_connect_slice(left, cooperative);
            match TcpStream::connect_timeout(&socket, slice) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    return Ok(stream);
                }
                Err(err) if is_timeout(&err) && left > slice => continue,
                Err(err) => {
                    last_err = Some(err);
                    break;
                }
            }
        }
    }
    match last_err {
        Some(err) => Err(err).with_context(|| format!("could not connect to {addr}")),
        None => bail!("no socket addresses for {addr}"),
    }
}

fn should_skip_outbound(settings: &Settings, addr: &str) -> bool {
    is_self_or_empty_addr(settings, addr) || hf128_self_alias_is_quarantined(settings, addr)
}

fn hf128_self_alias_quarantine() -> &'static Mutex<HashSet<String>> {
    static QUARANTINE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    QUARANTINE.get_or_init(|| Mutex::new(HashSet::new()))
}

fn hf128_self_alias_key(settings: &Settings, addr: &str) -> String {
    format!(
        "{}\u{0}{}\u{0}{}",
        settings.network.name,
        settings.node.data_dir,
        normalize_peer_addr(addr)
    )
}

fn hf128_self_alias_is_quarantined(settings: &Settings, addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    if normalized.is_empty() {
        return false;
    }
    let key = hf128_self_alias_key(settings, &normalized);
    hf128_self_alias_quarantine()
        .lock()
        .map(|quarantine| quarantine.contains(&key))
        .unwrap_or(false)
}

fn hf128_quarantine_self_alias(settings: &Settings, addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    if normalized.is_empty() {
        return false;
    }
    let key = hf128_self_alias_key(settings, &normalized);
    let inserted = hf128_self_alias_quarantine()
        .lock()
        .map(|mut quarantine| quarantine.insert(key))
        .unwrap_or(false);
    if inserted {
        eprintln!(
            "HF128 learned self-alias quarantined for this process: {normalized}"
        );
    }
    inserted
}

fn is_self_or_empty_addr(settings: &Settings, addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    if normalized.is_empty() {
        return true;
    }
    let bind = normalize_peer_addr(&settings.p2p.bind);
    if normalized == bind {
        return true;
    }
    let advertised = effective_advertise_addr(settings);
    if !advertised.is_empty() && normalized == normalize_peer_addr(&advertised) {
        return true;
    }

    // Treat 0.0.0.0:port, 127.0.0.1:port and auto-detected LAN-IP:port as self.
    let Ok(candidate) = normalized.parse::<SocketAddr>() else {
        return false;
    };
    if let Ok(bind_addr) = bind.parse::<SocketAddr>() {
        if candidate.port() == bind_addr.port() {
            if candidate.ip().is_loopback() || candidate.ip().is_unspecified() {
                return true;
            }
            if let Some(local_ip) = local_lan_ip(settings) {
                if candidate.ip() == local_ip {
                    return true;
                }
            }
        }
    }
    false
}

fn normalize_peer_addr(addr: &str) -> String {
    addr.trim().trim_start_matches("tcp://").to_string()
}

fn known_peers(settings: &Settings) -> Result<Vec<String>> {
    let mut out = Vec::<String>::new();
    let mut push_unique = |addr: String| {
        let normalized = normalize_peer_addr(&addr);
        if !normalized.is_empty()
            && !should_skip_outbound(settings, &normalized)
            && !out.iter().any(|p| p == &normalized)
        {
            out.push(normalized);
        }
    };

    // 1) Always try release/config bootnodes first. These are the intended
    // bootstrap path and should not be starved by thousands of stale registry rows.
    for bootnode in release_bootnodes(settings) {
        push_unique(bootnode);
    }

    // 2) Then recently observed peers, newest first. Old registry data is still
    // shown in the GUI but should not dominate active sync/mining decisions.
    let now = unix_time_u32() as u64;
    let mut registry = load_peer_registry(settings).unwrap_or_default().peers;
    registry.sort_by(|a, b| b.last_seen_unix.cmp(&a.last_seen_unix));
    for peer in registry
        .iter()
        .filter(|p| now.saturating_sub(p.last_seen_unix) <= 60 * 60)
    {
        // HF51: only use actually dialable-looking public addresses for active
        // sync/mining decisions. Unreachable registry-only high-tip reports stay
        // visible in the GUI but must not dominate outbound connection choices.
        if is_public_socket_addr_text(&peer.listen_addr) {
            push_unique(peer.listen_addr.clone());
        }
        // observed_addr remains telemetry. For inbound sessions it is usually
        // an ephemeral source port, so dialing it creates avoidable 900 ms
        // failures and delays real bootnode/peer propagation.
    }

    // 3) Finally, persistent peerbook fallback. This gives the network a path
    // forward if all seeds are down, while keeping stale/unknown rows behind fresh peers.
    let mut peerbook = load_peer_set(settings)?.into_iter().collect::<Vec<_>>();
    peerbook.sort();
    for addr in peerbook {
        push_unique(addr);
    }

    Ok(out)
}

fn add_peer(settings: &Settings, peers: &Arc<Mutex<HashSet<String>>>, addr: &str) -> Result<()> {
    let normalized = normalize_peer_addr(addr);
    if is_self_or_empty_addr(settings, &normalized) {
        return Ok(());
    }
    // Never hold the active peer-set mutex during filesystem I/O. A slow disk
    // or antivirus scan must not stall connection handling or tip diffusion.
    let snapshot = {
        let mut p = peers.lock().expect("peer mutex poisoned");
        if p.contains(&normalized) {
            None
        } else if p.len() >= HF127_PEERBOOK_MAX {
            // Runtime peer discovery is bounded too; otherwise a remote peer
            // can advertise endless unique endpoints and force repeated clones
            // and disk writes even though the persisted peerbook is capped.
            None
        } else {
            p.insert(normalized);
            Some(p.clone())
        }
    };
    if let Some(snapshot) = snapshot {
        save_peer_set(settings, &snapshot)?;
    }
    Ok(())
}

fn load_peer_set(settings: &Settings) -> Result<HashSet<String>> {
    let path = peerbook_path(settings);
    if !path.exists() {
        return Ok(HashSet::new());
    }
    let raw = std::fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(HashSet::new());
    }
    let book: PeerBook = serde_json::from_str(&raw).unwrap_or_default();

    // HF127/v1.8.3: enforce the peerbook bound at read time too. A historical
    // oversized/corruptly duplicated peer file must not allocate an unbounded
    // runtime set or delay startup before the next successful save can compact
    // it. Official/configured bootstrap peers are retained first.
    let mut ordered = Vec::<String>::with_capacity(HF127_PEERBOOK_MAX);
    let append_unique = |ordered: &mut Vec<String>, addr: String| {
        let normalized = normalize_peer_addr(&addr);
        if !normalized.is_empty()
            && !is_self_or_empty_addr(settings, &normalized)
            && !ordered.iter().any(|peer| peer == &normalized)
            && ordered.len() < HF127_PEERBOOK_MAX
        {
            ordered.push(normalized);
        }
    };

    for peer in official_snapshot_peers(settings)
        .into_iter()
        .chain(release_bootnodes(settings))
    {
        append_unique(&mut ordered, peer);
        if ordered.len() >= HF127_PEERBOOK_MAX {
            break;
        }
    }

    for peer in book.peers {
        append_unique(&mut ordered, peer);
        if ordered.len() >= HF127_PEERBOOK_MAX {
            break;
        }
    }

    Ok(ordered.into_iter().collect())
}

fn hf127_peerbook_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn save_peer_set(settings: &Settings, peers: &HashSet<String>) -> Result<()> {
    let _guard = hf127_peerbook_write_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("peerbook write mutex poisoned"))?;
    let path = peerbook_path(settings);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // HF127/v1.8.3: the peerbook is a fallback, not an unbounded historical
    // archive. Keep official bootstrap peers first and cap the remaining rows so
    // stale addresses cannot dominate startup/dial scheduling forever.
    // Multiple inbound/outbound sessions can learn peers concurrently. Merge
    // the latest on-disk set with the caller snapshot while holding the writer
    // lock so an older snapshot can never overwrite a peer learned moments
    // earlier by another session.
    let mut combined = peers.clone();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(existing) = serde_json::from_str::<PeerBook>(&raw) {
            combined.extend(existing.peers.into_iter().map(|peer| normalize_peer_addr(&peer)));
        }
    }

    let mut list = Vec::<String>::new();
    let append_unique = |list: &mut Vec<String>, addr: String| {
        let normalized = normalize_peer_addr(&addr);
        if !normalized.is_empty()
            && !is_self_or_empty_addr(settings, &normalized)
            && !list.iter().any(|peer| peer == &normalized)
        {
            list.push(normalized);
        }
    };

    for peer in official_snapshot_peers(settings)
        .into_iter()
        .chain(release_bootnodes(settings))
    {
        append_unique(&mut list, peer);
        if list.len() >= HF127_PEERBOOK_MAX {
            break;
        }
    }

    let mut remainder = combined
        .iter()
        .map(|peer| normalize_peer_addr(peer))
        .filter(|peer| !is_self_or_empty_addr(settings, peer))
        .collect::<Vec<_>>();
    remainder.sort();
    remainder.dedup();
    for peer in remainder {
        if list.len() >= HF127_PEERBOOK_MAX {
            break;
        }
        append_unique(&mut list, peer);
    }

    crate::write_text_replace(
        &path,
        &serde_json::to_string_pretty(&PeerBook { peers: list })?,
    )
}

fn peerbook_path(settings: &Settings) -> PathBuf {
    let paths = NodePaths::from_settings(settings);
    let name = if settings.p2p.peer_file.trim().is_empty() {
        "peers.json"
    } else {
        settings.p2p.peer_file.trim()
    };
    paths.data_dir.join(name)
}

fn peer_registry_path(settings: &Settings) -> PathBuf {
    NodePaths::from_settings(settings)
        .data_dir
        .join("peer-registry.json")
}

fn runtime_identity_path(settings: &Settings) -> PathBuf {
    NodePaths::from_settings(settings)
        .data_dir
        .join("node-identity.json")
}

pub fn set_runtime_miner_address(settings: &Settings, miner_address: &str) -> Result<()> {
    let path = runtime_identity_path(settings);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let id = RuntimeIdentity {
        miner_address: miner_address.trim().to_string(),
    };
    std::fs::write(path, serde_json::to_string_pretty(&id)?)?;
    Ok(())
}

fn runtime_miner_address(settings: &Settings) -> Option<String> {
    if !settings.mining.miner_address.trim().is_empty() {
        return Some(settings.mining.miner_address.trim().to_string());
    }
    let path = runtime_identity_path(settings);
    let raw = std::fs::read_to_string(path).ok()?;
    let id: RuntimeIdentity = serde_json::from_str(&raw).ok()?;
    let value = id.miner_address.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn node_id_path(settings: &Settings) -> PathBuf {
    NodePaths::from_settings(settings).data_dir.join("node_id")
}

fn node_id_creation_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn node_id(settings: &Settings) -> Result<String> {
    let path = node_id_path(settings);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let value = raw.trim();
        if value.len() >= 16 && value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(value.to_string());
        }
    }
    // Multiple startup/probe threads can ask for identity concurrently. Lock
    // creation and re-check after acquiring it so one process never advertises
    // two transient IDs or writes a truncated race winner.
    let _guard = node_id_creation_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("node identity creation lock poisoned"))?;
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let value = raw.trim();
        if value.len() >= 16 && value.chars().all(|c| c.is_ascii_hexdigit()) {
            return Ok(value.to_string());
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    let id = hex::encode(bytes);
    std::fs::write(path, &id)?;
    Ok(id)
}

fn load_peer_registry(settings: &Settings) -> Result<PeerRegistry> {
    let path = peer_registry_path(settings);
    if !path.exists() {
        return Ok(PeerRegistry::default());
    }
    let raw = std::fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(PeerRegistry::default());
    }
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

fn save_peer_registry(settings: &Settings, registry: &PeerRegistry) -> Result<()> {
    let path = peer_registry_path(settings);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut registry = registry.clone();
    registry.peers.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    registry.peers.dedup_by(|a, b| a.node_id == b.node_id);
    crate::write_text_replace(&path, &serde_json::to_string_pretty(&registry)?)
}

fn hf127_peer_registry_write_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn merge_peer_registry(settings: &Settings, incoming: Vec<P2PObservedPeer>) -> Result<()> {
    let _guard = hf127_peer_registry_write_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("peer registry write mutex poisoned"))?;
    let mut registry = load_peer_registry(settings)?;
    for peer in incoming.into_iter().take(256) {
        if peer.node_id.trim().is_empty() {
            continue;
        }
        upsert_observed_peer(&mut registry, peer);
    }
    prune_peer_registry(&mut registry);
    save_peer_registry(settings, &registry)
}

fn record_peer_observation(
    settings: &Settings,
    observed_addr: &str,
    remote_node_id: &str,
    listen_addr: &str,
    height: u32,
    tip_hash: &str,
    user_agent: &str,
    role: &str,
    miner_address: &str,
) -> Result<()> {
    if remote_node_id.trim().is_empty() {
        return Ok(());
    }
    let self_id = node_id(settings).unwrap_or_default();
    if remote_node_id == self_id {
        return Ok(());
    }
    let _guard = hf127_peer_registry_write_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("peer registry write mutex poisoned"))?;
    let mut registry = load_peer_registry(settings)?;
    let now = unix_time_u32() as u64;
    let observed_addr = normalize_peer_addr(observed_addr);
    let listen_addr = normalize_peer_addr(listen_addr);
    if let Some(existing) = registry
        .peers
        .iter()
        .find(|peer| peer.node_id == remote_node_id)
    {
        let materially_changed = existing.observed_addr != observed_addr
            || existing.listen_addr != listen_addr
            || existing.height != height
            || existing.tip_hash != tip_hash
            || existing.user_agent != user_agent
            || existing.role != role
            || existing.miner_address != miner_address;
        if !materially_changed
            && now.saturating_sub(existing.last_seen_unix) < HF127_PEER_OBSERVATION_WRITE_SECS
        {
            return Ok(());
        }
    }
    upsert_observed_peer(
        &mut registry,
        P2PObservedPeer {
            node_id: remote_node_id.to_string(),
            observed_addr,
            listen_addr,
            height,
            tip_hash: tip_hash.to_string(),
            user_agent: user_agent.to_string(),
            role: role.to_string(),
            miner_address: miner_address.to_string(),
            last_seen_unix: now,
        },
    );
    prune_peer_registry(&mut registry);
    save_peer_registry(settings, &registry)
}

fn upsert_observed_peer(registry: &mut PeerRegistry, peer: P2PObservedPeer) {
    if let Some(existing) = registry
        .peers
        .iter_mut()
        .find(|p| p.node_id == peer.node_id)
    {
        if peer.last_seen_unix >= existing.last_seen_unix {
            *existing = peer;
        }
    } else {
        registry.peers.push(peer);
    }
}

fn prune_peer_registry(registry: &mut PeerRegistry) {
    let now = unix_time_u32() as u64;
    // Keep discovered peer telemetry for a week. A 24h cache made private/mainnet
    // bootstrap too seed-dependent when the seed was temporarily offline.
    let cutoff = now.saturating_sub(7 * 24 * 60 * 60);
    registry.peers.retain(|p| p.last_seen_unix >= cutoff);
    if registry.peers.len() > 256 {
        registry
            .peers
            .sort_by(|a, b| b.last_seen_unix.cmp(&a.last_seen_unix));
        registry.peers.truncate(256);
    }
}

fn lan_discovery_enabled(settings: &Settings) -> bool {
    matches!(settings.network.name.as_str(), "regtest-lan")
}

fn lan_discovery_port(settings: &Settings) -> u16 {
    settings.network.default_port.saturating_add(1)
}

fn start_lan_discovery(
    settings: Settings,
    peers: Arc<Mutex<HashSet<String>>>,
    chain: Arc<Mutex<ChainState>>,
) {
    hf127_spawn_detached("qub-lan-discovery", move || {
        if let Err(err) = lan_discovery_loop(settings, peers, chain) {
            eprintln!("lan discovery stopped: {err:#}");
        }
    });
}

fn lan_discovery_loop(
    settings: Settings,
    peers: Arc<Mutex<HashSet<String>>>,
    chain: Arc<Mutex<ChainState>>,
) -> Result<()> {
    let port = lan_discovery_port(&settings);
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port))
        .with_context(|| format!("failed to bind LAN discovery UDP port {port}"))?;
    socket.set_broadcast(true)?;
    socket.set_read_timeout(Some(Duration::from_millis(650)))?;
    let target = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), port);
    let local_id = node_id(&settings).unwrap_or_default();
    let genesis_hash = genesis_block(&settings)?.block_hash().to_string();
    let mut next_broadcast = Instant::now();
    let mut buf = [0u8; 8192];

    loop {
        if Instant::now() >= next_broadcast {
            if let Ok(local) = chain.lock() {
                let listen_addr = effective_advertise_addr(&settings);
                if !listen_addr.trim().is_empty() {
                    let beacon = LanDiscoveryBeacon {
                        marker: LAN_DISCOVERY_MAGIC.to_string(),
                        protocol: PROTOCOL_VERSION,
                        network: settings.network.name.clone(),
                        magic: settings.network.magic.clone(),
                        genesis_hash: genesis_hash.clone(),
                        listen_addr,
                        node_id: local_id.clone(),
                        miner_address: runtime_miner_address(&settings).unwrap_or_default(),
                        height: local.height(),
                        tip_hash: local.tip_hash().to_string(),
                        user_agent: USER_AGENT.to_string(),
                    };
                    if let Ok(raw) = serde_json::to_vec(&beacon) {
                        let _ = socket.send_to(&raw, target);
                    }
                }
            }
            next_broadcast = Instant::now() + Duration::from_secs(2);
        }

        match socket.recv_from(&mut buf) {
            Ok((len, from)) => {
                let Ok(beacon) = serde_json::from_slice::<LanDiscoveryBeacon>(&buf[..len]) else {
                    continue;
                };
                if beacon.marker != LAN_DISCOVERY_MAGIC || beacon.protocol != PROTOCOL_VERSION {
                    continue;
                }
                if beacon.network != settings.network.name
                    || beacon.magic != settings.network.magic
                    || beacon.genesis_hash != genesis_hash
                {
                    continue;
                }
                if beacon.node_id.trim().is_empty() || beacon.node_id == local_id {
                    continue;
                }
                let listen = normalize_peer_addr(&beacon.listen_addr);
                if listen.is_empty() || is_self_or_empty_addr(&settings, &listen) {
                    continue;
                }
                let _ = add_peer(&settings, &peers, &listen);
                let _ = record_peer_observation(
                    &settings,
                    &from.to_string(),
                    &beacon.node_id,
                    &listen,
                    beacon.height,
                    &beacon.tip_hash,
                    &beacon.user_agent,
                    "auto-lan",
                    &beacon.miner_address,
                );
            }
            Err(err)
                if matches!(
                    err.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(err) => return Err(err).context("LAN discovery receive failed"),
        }
    }
}

fn effective_advertise_addr(settings: &Settings) -> String {
    let configured = normalize_peer_addr(&settings.p2p.advertise_addr);
    if !configured.is_empty() {
        return configured;
    }
    let Ok(addr) = settings.p2p.bind.parse::<SocketAddr>() else {
        return String::new();
    };
    if addr.ip().is_loopback() {
        return String::new();
    }
    if addr.ip().is_unspecified() {
        if let Some(ip) = local_lan_ip(settings) {
            // Mainnet/testnet must not blindly advertise RFC1918/LAN/CGNAT
            // addresses. Those nodes are still useful outbound peers, but they
            // are not public relays. Regtest-LAN keeps LAN advertise behavior.
            if is_public_network(settings) && !is_public_ip(ip) {
                return String::new();
            }
            return format!("{}:{}", ip, addr.port());
        }
        return String::new();
    }
    if is_public_network(settings) && !is_public_ip(addr.ip()) {
        return String::new();
    }
    settings.p2p.bind.clone()
}

fn is_public_network(settings: &Settings) -> bool {
    matches!(settings.network.name.as_str(), "mainnet" | "testnet")
}

fn is_public_socket_addr_text(addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    if normalized.trim().is_empty() {
        return false;
    }
    let Ok(addrs) = normalized.to_socket_addrs() else {
        return false;
    };
    for socket in addrs {
        if is_public_ip(socket.ip()) {
            return true;
        }
    }
    false
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.octets()[0] == 0
                || v4.octets()[0] >= 224
                || (v4.octets()[0] == 100 && (64..=127).contains(&v4.octets()[1])))
        }
        IpAddr::V6(v6) => {
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_unique_local()
                || v6.segments()[0] & 0xffc0 == 0xfe80)
        }
    }
}

pub fn local_relay_status(settings: &Settings) -> (bool, bool, String) {
    let advertised = effective_advertise_addr(settings);
    if advertised.trim().is_empty() {
        return (
            false,
            true,
            "NAT/private: no public relay address advertised".to_string(),
        );
    }
    if !is_public_socket_addr_text(&advertised) {
        return (
            false,
            true,
            format!("NAT/private: advertised address {advertised} is not public"),
        );
    }
    (
        true,
        false,
        format!("Relay capable: advertising {advertised}"),
    )
}

fn local_lan_ip(settings: &Settings) -> Option<IpAddr> {
    for bootnode in &settings.p2p.bootnodes {
        if let Ok(addrs) = normalize_peer_addr(bootnode).to_socket_addrs() {
            for socket_addr in addrs {
                if let Some(ip) = local_ip_to(socket_addr) {
                    return Some(ip);
                }
            }
        }
    }
    local_ip_to("8.8.8.8:80".to_socket_addrs().ok()?.next()?)
}

fn local_ip_to(remote: SocketAddr) -> Option<IpAddr> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    let _ = socket.connect(remote);
    let ip = socket.local_addr().ok()?.ip();
    if ip.is_loopback() || ip.is_unspecified() {
        None
    } else {
        Some(ip)
    }
}

fn prefer_peer_tip_on_equal_work(settings: &Settings, addr: &str) -> bool {
    settings.network.name.contains("regtest") && is_bootnode(settings, addr)
}

fn is_bootnode(settings: &Settings, addr: &str) -> bool {
    let normalized = normalize_peer_addr(addr);
    settings
        .p2p
        .bootnodes
        .iter()
        .any(|b| normalize_peer_addr(b) == normalized)
}

fn is_benign_io(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::UnexpectedEof
            | io::ErrorKind::ConnectionReset
            | io::ErrorKind::ConnectionAborted
    ) || err
        .to_string()
        .to_ascii_lowercase()
        .contains("expected value at line 1 column 1")
}

fn is_benign_disconnect(err: &anyhow::Error) -> bool {
    let text = err.to_string().to_ascii_lowercase();
    text.contains("peer closed")
        || text.contains("connection reset")
        || text.contains("connection was aborted")
        || text.contains("os error 10053")
        || text.contains("unexpected eof")
        || text.contains("expected value at line 1 column 1")
}

fn refresh_registry_from_seed_domains(settings: &Settings) -> Result<()> {
    if !settings.p2p.enabled {
        return Ok(());
    }
    for seed in release_bootnodes(settings).into_iter().take(4) {
        let seed = normalize_peer_addr(&seed);
        if seed.is_empty() || should_skip_outbound(settings, &seed) {
            continue;
        }
        if let Ok(remote_peers) = fetch_peer_list(
            settings,
            &seed,
            Duration::from_millis(HF80_PEER_STATUS_FETCH_MS),
        ) {
            let _ = merge_peer_registry(settings, remote_peers);
        }
    }
    Ok(())
}

fn recent_chain_miner_observations(settings: &Settings) -> Vec<P2PObservedPeer> {
    let Ok(chain) = load_chain_for_hf90_catchup(settings) else {
        return Vec::new();
    };
    let local_miner = runtime_miner_address(settings).unwrap_or_default();
    let now = unix_time_u32() as u64;
    let mut out: Vec<P2PObservedPeer> = Vec::new();
    for (height, block) in chain.blocks.iter().enumerate().skip(1).rev().take(48) {
        let Some(coinbase) = block.transactions.first() else {
            continue;
        };
        let Some(out0) = coinbase.outputs.first() else {
            continue;
        };
        let Some(address) =
            address_from_script_pubkey(&settings.network.address_prefix, &out0.script_pubkey)
                .map(|a| a.to_string())
        else {
            continue;
        };
        if address.trim().is_empty() || address == local_miner {
            continue;
        }
        let age = now.saturating_sub(block.header.time as u64);
        // Treat recent block producers as globally-live miners even when they are behind NAT
        // and cannot be dialed directly. This is privacy-preserving and much more accurate
        // for the public GUI than direct reachability alone.
        if age > GLOBAL_PEER_LIVE_SECS {
            continue;
        }
        if let Some(existing) = out.iter_mut().find(|p| p.miner_address == address) {
            if (block.header.time as u64) >= existing.last_seen_unix {
                existing.height = height as u32;
                existing.tip_hash = block.block_hash().to_string();
                existing.last_seen_unix = block.header.time as u64;
            }
            continue;
        }
        out.push(P2PObservedPeer {
            node_id: format!("miner:{}", address),
            observed_addr: String::new(),
            listen_addr: String::new(),
            height: height as u32,
            tip_hash: block.block_hash().to_string(),
            user_agent: "observed-from-active-chain".to_string(),
            role: "miner · recent block".to_string(),
            miner_address: address,
            last_seen_unix: block.header.time as u64,
        });
    }
    out
}

fn upsert_status_by_identity(
    statuses: &mut Vec<P2PPeerStatus>,
    observed: &P2PObservedPeer,
) -> bool {
    let miner = observed.miner_address.trim();
    if let Some(existing) = statuses.iter_mut().find(|p| {
        p.node_id.as_deref() == Some(observed.node_id.as_str())
            || (!miner.is_empty() && p.miner_address.as_deref().unwrap_or("").trim() == miner)
    }) {
        apply_observed_identity(existing, observed);
        return true;
    }
    false
}
pub fn peer_status_cached(settings: &Settings) -> Result<P2PNetworkSnapshot> {
    if !settings.p2p.enabled {
        return Ok(P2PNetworkSnapshot {
            enabled: false,
            ..Default::default()
        });
    }
    let mut snapshot = P2PNetworkSnapshot {
        enabled: true,
        known_peers: known_peers(settings).map(|p| p.len()).unwrap_or(0),
        reachable_peers: 0,
        direct_reachable_peers: 0,
        globally_live_peers: 0,
        relay_capable: false,
        nat_private: false,
        stale_warning: String::new(),
        peers: Vec::new(),
    };
    let (relay_capable, nat_private, relay_message) = local_relay_status(settings);
    snapshot.relay_capable = relay_capable;
    snapshot.nat_private = nat_private;
    snapshot.stale_warning = relay_message;

    // HF88/v1.6.2: this is a zero/low-network UI snapshot. It never probes every
    // peer and never blocks wallet/block rendering. It shows cached registry rows
    // and official seed tip rows when the tiny HTTP tip is available. It avoids
    // rescanning chain.json. The strict mining guard still performs consensus validation before
    // hashing/submitting.
    let official_tip = official_http_tip(settings, 700).ok().flatten();
    if let Some((height, tip_hash)) = official_tip.clone() {
        // One HTTP publication is one observation, even when several DNS
        // aliases can download it. HF127 duplicated this row under two seed
        // names, which made the GUI appear to prove independent seed agreement.
        snapshot.peers.push(P2PPeerStatus {
            addr: "download.qubit-coin.io/mainnet/snapshots/tip.json".to_string(),
            reachable: false,
            global_live: true,
            height: Some(height),
            tip_hash: Some(tip_hash),
            user_agent: Some("official-http-publication".to_string()),
            error: None,
            node_id: None,
            observed_addr: None,
            listen_addr: None,
            role: Some("single HTTP publication (not an independent seed)".to_string()),
            miner_address: None,
            last_seen_unix: Some(unix_time_u32() as u64),
            seen_age_secs: Some(0),
        });
    }
    // HF88: HTTP snapshots can lag while seed nodes are live. Add a tiny direct
    // official-seed probe to the non-blocking peer view so known-tip/mining UI
    // does not freeze at a stale HTTP height.
    for (addr, height, tip_hash, _, node_id) in official_snapshot_peer_candidates(settings, 350)
        .into_iter()
        .take(3)
    {
        if height == 0 {
            continue;
        }
        snapshot.peers.push(P2PPeerStatus {
            addr,
            reachable: true,
            global_live: true,
            height: Some(height),
            tip_hash: Some(tip_hash),
            user_agent: Some("official-direct-tip".to_string()),
            error: None,
            node_id: if node_id.trim().is_empty() { None } else { Some(node_id) },
            observed_addr: None,
            listen_addr: None,
            role: Some("direct seed".to_string()),
            miner_address: None,
            last_seen_unix: Some(unix_time_u32() as u64),
            seen_age_secs: Some(0),
        });
    }

    let self_id = node_id(settings).unwrap_or_default();
    let registry = load_peer_registry(settings).unwrap_or_default();
    snapshot.known_peers = snapshot.known_peers.max(registry.peers.len());
    for observed in registry.peers {
        if observed.node_id == self_id {
            continue;
        }
        let display_addr = if !observed.listen_addr.trim().is_empty() {
            observed.listen_addr.clone()
        } else {
            observed.observed_addr.clone()
        };
        if upsert_status_by_identity(&mut snapshot.peers, &observed)
            || snapshot.peers.iter().any(|p| p.addr == display_addr)
        {
            continue;
        }
        snapshot.peers.push(P2PPeerStatus {
            addr: display_addr,
            reachable: false,
            global_live: observed_peer_globally_live(&observed),
            height: Some(observed.height),
            tip_hash: Some(observed.tip_hash.clone()),
            user_agent: Some(observed.user_agent.clone()),
            error: None,
            node_id: Some(observed.node_id.clone()),
            observed_addr: Some(observed.observed_addr.clone()),
            listen_addr: Some(observed.listen_addr.clone()),
            role: Some(observed.role.clone()),
            miner_address: Some(observed.miner_address.clone()),
            last_seen_unix: Some(observed.last_seen_unix),
            seen_age_secs: Some(observed_peer_seen_age(&observed)),
        });
    }

    // HF88: even if HTTP/direct probes are unavailable, show configured bootstrap
    // seeds as pending rows so the peer panel is transparent instead of looking
    // dead. These rows do not claim reachability or consensus; they only show
    // which entrypoints QUB Core is trying in bounded background pulses.
    if snapshot.peers.is_empty() {
        for seed in release_bootnodes(settings).into_iter().take(3) {
            let seed = normalize_peer_addr(&seed);
            if seed.is_empty() {
                continue;
            }
            snapshot.peers.push(P2PPeerStatus {
                addr: seed,
                reachable: false,
                global_live: false,
                height: None,
                tip_hash: None,
                user_agent: Some("configured-bootstrap-seed".to_string()),
                error: Some("waiting for bounded official/direct probe".to_string()),
                node_id: None,
                observed_addr: None,
                listen_addr: None,
                role: Some("bootstrap seed".to_string()),
                miner_address: None,
                last_seen_unix: None,
                seen_age_secs: None,
            });
        }
    }

    snapshot.peers.sort_by(|a, b| {
        let ar = (
            a.reachable as u8,
            a.global_live as u8,
            a.last_seen_unix.unwrap_or(0),
        );
        let br = (
            b.reachable as u8,
            b.global_live as u8,
            b.last_seen_unix.unwrap_or(0),
        );
        br.cmp(&ar)
    });
    let mut unique = Vec::<P2PPeerStatus>::new();
    for peer in snapshot.peers.into_iter() {
        let key = if !peer
            .miner_address
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            format!("miner:{}", peer.miner_address.as_deref().unwrap_or(""))
        } else if !peer.node_id.as_deref().unwrap_or("").trim().is_empty() {
            format!("node:{}", peer.node_id.as_deref().unwrap_or(""))
        } else {
            format!("addr:{}", peer.addr)
        };
        if unique.iter().any(|p| {
            let existing_key = if !p.miner_address.as_deref().unwrap_or("").trim().is_empty() {
                format!("miner:{}", p.miner_address.as_deref().unwrap_or(""))
            } else if !p.node_id.as_deref().unwrap_or("").trim().is_empty() {
                format!("node:{}", p.node_id.as_deref().unwrap_or(""))
            } else {
                format!("addr:{}", p.addr)
            };
            existing_key == key
        }) {
            continue;
        }
        unique.push(peer);
    }
    snapshot.peers = unique;
    for peer in &mut snapshot.peers {
        if !peer.reachable && peer.seen_age_secs.unwrap_or(u64::MAX) > GLOBAL_PEER_LIVE_SECS {
            peer.global_live = false;
        }
    }
    snapshot.known_peers = snapshot.peers.len().max(snapshot.known_peers);
    snapshot.reachable_peers = 0;
    snapshot.direct_reachable_peers = 0;
    snapshot.globally_live_peers = snapshot
        .peers
        .iter()
        .filter(|p| p.reachable || p.global_live)
        .count();

    // HF88: stale-warning vs local height is computed by the GUI snapshot layer
    // that already owns the current ChainState. Avoid another storage read here.
    Ok(snapshot)
}

pub fn peer_status(settings: &Settings) -> Result<P2PNetworkSnapshot> {
    if !settings.p2p.enabled {
        return Ok(P2PNetworkSnapshot {
            enabled: false,
            ..Default::default()
        });
    }
    let peers = known_peers(settings)?;
    let mut snapshot = P2PNetworkSnapshot {
        enabled: true,
        known_peers: peers.len(),
        reachable_peers: 0,
        direct_reachable_peers: 0,
        globally_live_peers: 0,
        relay_capable: false,
        nat_private: false,
        stale_warning: String::new(),
        peers: Vec::new(),
    };
    let _ = refresh_registry_from_seed_domains(settings);
    let mut registry = load_peer_registry(settings).unwrap_or_default();
    let (relay_capable, nat_private, relay_message) = local_relay_status(settings);
    snapshot.relay_capable = relay_capable;
    snapshot.nat_private = nat_private;
    snapshot.stale_warning = relay_message;

    // Probe a bounded number of directly-known peers. This keeps the GUI fast while
    // global miner status comes from seed/registry peer telemetry.
    let direct_probe_limit = settings.p2p.max_outbound_peers.max(4).min(8);
    for addr in peers.iter().take(direct_probe_limit) {
        if should_skip_outbound(settings, addr) {
            continue;
        }
        let mut status = P2PPeerStatus {
            addr: addr.clone(),
            ..Default::default()
        };
        if let Some(observed) = registry_match(&registry, addr) {
            apply_observed_identity(&mut status, observed);
        }
        match probe_peer(
            settings,
            addr,
            Duration::from_millis(HF80_PEER_STATUS_PROBE_MS),
        ) {
            Ok(info) => {
                apply_probe_identity(&mut status, &info);
                snapshot.direct_reachable_peers += 1;
                let _ = record_peer_observation(
                    settings,
                    addr,
                    &info.node_id,
                    &info.listen_addr,
                    info.height,
                    &info.tip_hash,
                    &info.user_agent,
                    &info.role,
                    &info.miner_address,
                );
                if let Ok(remote_peers) = fetch_peer_list(
                    settings,
                    addr,
                    Duration::from_millis(HF80_PEER_STATUS_FETCH_MS),
                ) {
                    let _ = merge_peer_registry(settings, remote_peers);
                    registry = load_peer_registry(settings).unwrap_or_default();
                    if status
                        .miner_address
                        .as_deref()
                        .unwrap_or("")
                        .trim()
                        .is_empty()
                    {
                        if let Some(observed) = registry_match(&registry, addr).or_else(|| {
                            status
                                .node_id
                                .as_deref()
                                .and_then(|id| registry_match_node(&registry, id))
                        }) {
                            apply_observed_identity(&mut status, observed);
                        }
                    }
                }
            }
            Err(err) => {
                status.error = Some(err.to_string());
            }
        }
        snapshot.peers.push(status);
    }

    registry = load_peer_registry(settings).unwrap_or(registry);
    let self_id = node_id(settings).unwrap_or_default();
    for observed in registry.peers {
        if observed.node_id == self_id {
            continue;
        }
        let display_addr = if !observed.listen_addr.trim().is_empty() {
            observed.listen_addr.clone()
        } else {
            observed.observed_addr.clone()
        };
        if upsert_status_by_identity(&mut snapshot.peers, &observed)
            || snapshot.peers.iter().any(|p| p.addr == display_addr)
        {
            continue;
        }
        let status = P2PPeerStatus {
            addr: display_addr.clone(),
            reachable: false,
            global_live: observed_peer_globally_live(&observed),
            height: Some(observed.height),
            tip_hash: Some(observed.tip_hash.clone()),
            user_agent: Some(observed.user_agent.clone()),
            error: None,
            node_id: Some(observed.node_id.clone()),
            observed_addr: Some(observed.observed_addr.clone()),
            listen_addr: Some(observed.listen_addr.clone()),
            role: Some(observed.role.clone()),
            miner_address: Some(observed.miner_address.clone()),
            last_seen_unix: Some(observed.last_seen_unix),
            seen_age_secs: Some(observed_peer_seen_age(&observed)),
        };
        // Do not direct-probe every registry row here. On a public network many miners
        // are behind NAT/firewall and hundreds of small probe timeouts can stall the GUI
        // for 30-100s. Direct status is computed from the bounded known-peer probe above;
        // global live status comes from seed registry + recent active-chain block authors.
        snapshot.peers.push(status);
    }

    for observed in recent_chain_miner_observations(settings) {
        if upsert_status_by_identity(&mut snapshot.peers, &observed) {
            continue;
        }
        snapshot.peers.push(P2PPeerStatus {
            addr: format!("miner:{}", observed.miner_address),
            reachable: false,
            global_live: observed_peer_globally_live(&observed),
            height: Some(observed.height),
            tip_hash: Some(observed.tip_hash.clone()),
            user_agent: Some(observed.user_agent.clone()),
            error: None,
            node_id: Some(observed.node_id.clone()),
            observed_addr: None,
            listen_addr: None,
            role: Some(observed.role.clone()),
            miner_address: Some(observed.miner_address.clone()),
            last_seen_unix: Some(observed.last_seen_unix),
            seen_age_secs: Some(observed_peer_seen_age(&observed)),
        });
    }

    // Dedupe display rows by miner address first, node_id second. Prefer rows with direct
    // reachability, then global-live, then freshest last_seen. This removes old/offline
    // duplicates for the same public payout address.
    snapshot.peers.sort_by(|a, b| {
        let ar = (
            a.reachable as u8,
            a.global_live as u8,
            a.last_seen_unix.unwrap_or(0),
        );
        let br = (
            b.reachable as u8,
            b.global_live as u8,
            b.last_seen_unix.unwrap_or(0),
        );
        br.cmp(&ar)
    });
    let mut unique = Vec::<P2PPeerStatus>::new();
    for peer in snapshot.peers.into_iter() {
        let key = if !peer
            .miner_address
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        {
            format!("miner:{}", peer.miner_address.as_deref().unwrap_or(""))
        } else if !peer.node_id.as_deref().unwrap_or("").trim().is_empty() {
            format!("node:{}", peer.node_id.as_deref().unwrap_or(""))
        } else {
            format!("addr:{}", peer.addr)
        };
        if unique.iter().any(|p| {
            let existing_key = if !p.miner_address.as_deref().unwrap_or("").trim().is_empty() {
                format!("miner:{}", p.miner_address.as_deref().unwrap_or(""))
            } else if !p.node_id.as_deref().unwrap_or("").trim().is_empty() {
                format!("node:{}", p.node_id.as_deref().unwrap_or(""))
            } else {
                format!("addr:{}", p.addr)
            };
            existing_key == key
        }) {
            continue;
        }
        unique.push(peer);
    }
    snapshot.peers = unique;

    // HF71/v1.5.8: stale registry rows must not render as "online" for days.
    // Direct TCP rows remain direct; registry-only rows older than the global-live
    // window are shown as offline/last seen and ignored by useful-peer counts.
    for peer in &mut snapshot.peers {
        if !peer.reachable {
            if peer.seen_age_secs.unwrap_or(u64::MAX) > GLOBAL_PEER_LIVE_SECS {
                peer.global_live = false;
            }
        }
    }

    snapshot.known_peers = snapshot.peers.len().max(snapshot.known_peers);
    snapshot.reachable_peers = snapshot.direct_reachable_peers;
    snapshot.globally_live_peers = snapshot
        .peers
        .iter()
        .filter(|p| p.reachable || p.global_live)
        .count();

    if let Ok(local) = load_chain_for_hf90_catchup(settings) {
        let local_height = local.height();
        let local_tip = local.tip_hash().to_string();
        let mut direct_conflicts = 0usize;
        let mut registry_high_only = 0usize;
        for p in &snapshot.peers {
            if p.reachable
                && p.height == Some(local_height)
                && p.tip_hash.as_deref() != Some(local_tip.as_str())
            {
                direct_conflicts += 1;
            }
            if !p.reachable
                && p.global_live
                && p.height.unwrap_or(0) > local_height.saturating_add(1)
            {
                registry_high_only += 1;
            }
        }
        if direct_conflicts > 0 {
            snapshot.stale_warning = format!("Stale/fork warning: {direct_conflicts} direct peer(s) disagree at local height #{local_height}");
        } else if registry_high_only > 0 {
            snapshot.stale_warning = format!("Telemetry note: {registry_high_only} unreachable high-tip report(s) ignored until a direct peer can validate them");
        }
    }
    Ok(snapshot)
}

fn nonempty_option(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn registry_match<'a>(registry: &'a PeerRegistry, addr: &str) -> Option<&'a P2PObservedPeer> {
    let normalized = normalize_peer_addr(addr);
    registry.peers.iter().find(|peer| {
        normalize_peer_addr(&peer.listen_addr) == normalized
            || normalize_peer_addr(&peer.observed_addr) == normalized
    })
}

fn registry_match_node<'a>(
    registry: &'a PeerRegistry,
    node_id: &str,
) -> Option<&'a P2PObservedPeer> {
    registry.peers.iter().find(|peer| peer.node_id == node_id)
}

fn apply_probe_identity(status: &mut P2PPeerStatus, info: &P2PProbeInfo) {
    status.reachable = true;
    status.global_live = true;
    status.seen_age_secs = Some(0);
    status.last_seen_unix = Some(unix_time_u32() as u64);
    status.height = Some(info.height);
    status.tip_hash = Some(info.tip_hash.clone());

    let current = status.user_agent.take();
    status.user_agent = nonempty_option(&info.user_agent).or(current);
    let current = status.node_id.take();
    status.node_id = nonempty_option(&info.node_id).or(current);
    let current = status.listen_addr.take();
    status.listen_addr = nonempty_option(&info.listen_addr).or(current);
    let current = status.role.take();
    status.role = nonempty_option(&info.role).or(current);
    let current = status.miner_address.take();
    status.miner_address = nonempty_option(&info.miner_address).or(current);
}

fn apply_observed_identity(status: &mut P2PPeerStatus, observed: &P2PObservedPeer) {
    status.global_live = status.global_live || observed_peer_globally_live(observed);
    status.seen_age_secs = Some(observed_peer_seen_age(observed));
    status.node_id = nonempty_option(&observed.node_id).or(status.node_id.take());
    status.observed_addr = nonempty_option(&observed.observed_addr).or(status.observed_addr.take());
    status.listen_addr = nonempty_option(&observed.listen_addr).or(status.listen_addr.take());
    status.role = nonempty_option(&observed.role).or(status.role.take());
    status.miner_address = nonempty_option(&observed.miner_address).or(status.miner_address.take());
    status.user_agent = nonempty_option(&observed.user_agent).or(status.user_agent.take());
    status.last_seen_unix = Some(observed.last_seen_unix).or(status.last_seen_unix);
    if status.height.is_none() {
        status.height = Some(observed.height);
    }
    if status.tip_hash.as_deref().unwrap_or("").trim().is_empty() {
        status.tip_hash = nonempty_option(&observed.tip_hash);
    }
}

fn observed_peer_seen_age(observed: &P2PObservedPeer) -> u64 {
    (unix_time_u32() as u64).saturating_sub(observed.last_seen_unix)
}

fn observed_peer_globally_live(observed: &P2PObservedPeer) -> bool {
    observed_peer_seen_age(observed) <= GLOBAL_PEER_LIVE_SECS
}

pub fn known_peer_addrs(settings: &Settings) -> Result<Vec<String>> {
    known_peers(settings)
}

fn hf131_probe_version(settings: &Settings) -> Result<WireMessage> {
    let chain = load_chain_for_hf90_catchup(settings)?;
    version_message_with_role(settings, &chain, if settings.mining.enabled { "miner" } else { "node" })
}

fn probe_peer(settings: &Settings, addr: &str, timeout: Duration) -> Result<P2PProbeInfo> {
    let deadline = hf130_effective_deadline(Instant::now() + timeout);
    let _scope = Hf130IoScope::deadline(deadline);
    let version = hf131_probe_version(settings)?;
    probe_peer_with_version(settings, addr, hf130_io_remaining(deadline)?, &version)
}

fn probe_peer_with_version(settings: &Settings, addr: &str, timeout: Duration, version: &WireMessage) -> Result<P2PProbeInfo> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + timeout);
    let deadline = Instant::now() + timeout;
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    let mut stream = connect_peer(addr, timeout)?;
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        bail!("peer probe deadline expired during connect");
    }
    stream.set_read_timeout(Some(left))?;
    stream.set_write_timeout(Some(left))?;
    // The batch captures this immutable Version once; a joined socket probe
    // must not reload or wait for Fast Chain Engine storage on every endpoint.
    send_wire(&mut stream, version)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes.min(64 * 1024)) {
            Ok(WireMessage::Version {
                protocol,
                network,
                magic,
                user_agent,
                height,
                tip_hash,
                genesis_hash,
                node_id,
                listen_addr,
                role,
                miner_address,
                ..
            }) => {
                if protocol != PROTOCOL_VERSION {
                    bail!("unsupported protocol {protocol}");
                }
                validate_remote_network(settings, &network, &magic, &genesis_hash)?;
                if remote_node_id_is_self(settings, &node_id) {
                    hf128_quarantine_self_alias(settings, addr);
                    bail!("peer address {addr} resolves back to this node_id");
                }
                return Ok(P2PProbeInfo {
                    height,
                    tip_hash,
                    user_agent,
                    node_id,
                    listen_addr,
                    role,
                    miner_address,
                });
            }
            Ok(_) => {}
            Err(err) if is_timeout(&err) => break,
            Err(err) => return Err(err.into()),
        }
    }
    bail!("no version response")
}

fn remote_node_id_is_self(settings: &Settings, remote_node_id: &str) -> bool {
    let remote = remote_node_id.trim();
    if remote.is_empty() {
        return false;
    }
    node_id(settings)
        .map(|local| !local.trim().is_empty() && local == remote)
        .unwrap_or(false)
}

fn fetch_peer_list(
    settings: &Settings,
    addr: &str,
    timeout: Duration,
) -> Result<Vec<P2PObservedPeer>> {
    let _hf130_entry_scope = Hf130IoScope::deadline(Instant::now() + timeout);
    let mut stream = connect_peer(addr, timeout)?;
    stream.set_read_timeout(Some(timeout.saturating_mul(2)))?;
    stream.set_write_timeout(Some(timeout))?;
    // HF88/v1.6.2: peer status/version probes only need local height/tip for
    // the Version message. Use the UI-fast loader so a bounded peer probe does
    // not replay-validate the entire chain and stall GUI snapshots.
    let chain =
        load_chain_for_hf90_catchup(settings)?;
    send_version(&mut stream, settings, &chain)?;
    send_wire(&mut stream, &WireMessage::GetPeerList)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let deadline = Instant::now() + timeout.saturating_mul(2);
    let deadline = hf130_effective_deadline(deadline);
    let _hf130_io_scope = Hf130IoScope::deadline(deadline);
    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes.min(64 * 1024)) {
            Ok(WireMessage::PeerList { peers }) => return Ok(peers),
            Ok(WireMessage::Version {
                protocol,
                network,
                magic,
                genesis_hash,
                ..
            }) => {
                if protocol != PROTOCOL_VERSION {
                    bail!("unsupported protocol {protocol}");
                }
                validate_remote_network(settings, &network, &magic, &genesis_hash)?;
            }
            Ok(_) => {}
            Err(err) if is_timeout(&err) => break,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(Vec::new())
}

fn finish_report(settings: &Settings, mut report: P2PSyncReport) -> Result<P2PSyncReport> {
    let chain = load_chain_for_hf90_catchup(settings)?;
    report.height = chain.height();
    report.tip_hash = chain.tip_hash().to_string();
    report.canonical_tip_proven = hf130_proof_matches_final(
        &report, report.height, &report.tip_hash, pending_block_relay_waiting(settings, &chain),
    );
    if report.writer_busy {
        report.progressed = false;
        report.fresh_at_tip = false;
        report.terminal = "busy".to_string();
    } else if report.writer_acquired {
        report.progressed = report.height > report.before_height
            || (!report.before_tip_hash.trim().is_empty()
                && report.tip_hash != report.before_tip_hash);
        let no_conflicting_target = report.best_peer_height < report.height
            || (report.best_peer_height == report.height
                && !report.best_peer_tip_hash.trim().is_empty()
                && report.best_peer_tip_hash == report.tip_hash);
        report.fresh_at_tip = !report.progressed
            && report.canonical_tip_proven
            && no_conflicting_target;
        // A report may have been finalized and merged before a later stage
        // actually adopted a chain. Re-derive the terminal state from the
        // final canonical identity so it cannot contradict `progressed`.
        report.terminal = if report.progressed {
            "progress"
        } else if report.fresh_at_tip {
            "fresh-at-tip"
        } else {
            "no-progress"
        }
        .to_string();
    } else if report.terminal.trim().is_empty() {
        report.terminal = "completed".to_string();
    }
    Ok(report)
}

#[cfg(test)]
mod hf131_peer_liveness_tests {
    use super::*;

    fn observation(addr: &str, node: &str, height: u32, tip: &str) -> (String, P2PProbeInfo) {
        (addr.to_string(), P2PProbeInfo {
            node_id: node.to_string(), height, tip_hash: tip.to_string(), ..P2PProbeInfo::default()
        })
    }

    #[test]
    fn hf131_silent_first_probe_does_not_hide_healthy_peers() {
        let peers = vec!["silent-seed".to_string(), "healthy-a".to_string(), "healthy-b".to_string()];
        let completed = AtomicUsize::new(0);
        let deadline = Instant::now() + Duration::from_secs(1);
        let results = hf131_probe_batch(&peers, deadline, |addr, _| {
            if addr == "silent-seed" {
                while let Ok(left) = hf130_io_remaining(deadline) {
                    thread::sleep(left.min(Duration::from_millis(5)));
                }
                completed.fetch_add(1, Ordering::SeqCst);
                None
            } else {
                completed.fetch_add(1, Ordering::SeqCst);
                Some(42)
            }
        });
        assert_eq!(completed.load(Ordering::SeqCst), 3, "all owned probes must be joined");
        assert_eq!(results.len(), 2, "a silent first source must not consume other sources' budgets");
    }

    #[test]
    fn hf131_probe_cancellation_is_inherited_and_joined() {
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let started = AtomicUsize::new(0);
        let completed = AtomicUsize::new(0);
        let cancellation = thread::spawn(move || {
            thread::sleep(Duration::from_millis(35));
            cancel.store(true, Ordering::SeqCst);
        });
        let start = Instant::now();
        let results = hf130_with_mining_cancellation(stop, || {
            hf131_probe_batch(&["a".into(), "b".into()], start + Duration::from_secs(2), |_, _| {
                started.fetch_add(1, Ordering::SeqCst);
                let deadline = start + Duration::from_secs(2);
                while hf130_io_remaining(deadline).is_ok() { thread::sleep(Duration::from_millis(5)); }
                completed.fetch_add(1, Ordering::SeqCst);
                None::<usize>
            })
        });
        cancellation.join().unwrap();
        assert!(results.is_empty());
        assert_eq!(completed.load(Ordering::SeqCst), started.load(Ordering::SeqCst), "every started worker must be joined before return");
        assert!(start.elapsed() < Duration::from_secs(1), "cancelled probes must not wait for the two-second deadline");
    }

    #[test]
    fn hf131_direct_quorum_counts_node_identity_once() {
        let view = hf131_parent_view(100, "tip-100", vec![
            observation("alias-a", "same-node", 100, "tip-100"),
            observation("alias-b", "same-node", 100, "tip-100"),
            observation("unknown", "", 100, "tip-100"),
        ]);
        assert_eq!(view.exact, 1);
        assert_eq!(hf130_quorum_verdict(view.exact, false, false, true), Hf130QuorumVerdict::Rejected);
        let independent = hf131_parent_view(100, "tip-100", vec![
            observation("peer-a", "node-a", 100, "tip-100"),
            observation("peer-b", "node-b", 100, "tip-100"),
        ]);
        assert_eq!(hf130_quorum_verdict(independent.exact, false, false, true), Hf130QuorumVerdict::Exact);
    }

    #[test]
    fn hf131_ahead_quorum_is_exact_pair_and_deduplicated() {
        let noisy = hf131_parent_view(100, "local", vec![
            observation("a", "node-a", 200, "future"),
            observation("alias-a", "node-a", 200, "future"),
            observation("b", "node-b", 200, "different"),
        ]);
        assert_eq!(noisy.ahead_height, 0);
        let quorum = hf131_parent_view(100, "local", vec![
            observation("a", "node-a", 101, "next"),
            observation("b", "node-b", 101, "next"),
            observation("local-peer", "node-c", 100, "local"),
        ]);
        assert_eq!(quorum.ahead_height, 101);
        assert!(hf131_parent_is_behind(100, 100, quorum.ahead_height));
        assert!(hf131_parent_is_behind(100, 101, 0), "an older exact HTTP tip must not hide an ahead official sample");
        assert!(!hf131_parent_is_behind(100, 99, 0));
    }

    #[test]
    fn hf131_equal_height_tie_never_creates_exact_proof() {
        let view = hf131_parent_view(100, "local", vec![
            observation("a", "node-a", 100, "sibling"),
            observation("b", "node-b", 100, "sibling"),
        ]);
        let verdict = hf130_quorum_verdict(view.exact, view.ahead_height > 100, !view.conflicts.is_empty(), true);
        let mut report = P2PSyncReport::default();
        assert!(hf130_apply_quorum_verdict(&mut report, verdict, 100, "local"));
        assert!(!report.canonical_tip_proven);
        assert!(report.canonical_proof.is_none());
        assert_eq!(hf130_quorum_verdict(view.exact, false, true, false), Hf130QuorumVerdict::Rejected);
    }

    #[test]
    fn hf131_repaired_block_acknowledgements_prove_direct_delivery() {
        for first in ["ack", "ack_after_suffix", "ack_after_overlap"] {
            for second in ["ack", "ack_after_suffix", "ack_after_overlap"] {
                let mut report = BlockRelayReport { official_attempts: 2, ..BlockRelayReport::default() };
                for (peer, transport) in [("198.51.100.1:17444", first), ("198.51.100.2:17444", second)] {
                    update_block_relay_report(&mut report, BlockRelayPeerResult {
                        peer: peer.into(), transport: transport.into(), status: "accepted".into(), ..BlockRelayPeerResult::default()
                    });
                }
                assert!(report.delivered(), "explicit repaired ACK pair {first}/{second} must count");
            }
        }
    }

    #[test]
    fn hf131_relay_duplicates_and_legacy_are_not_delivery() {
        for (peer, transport, status) in [
            ("198.51.100.1:17444", "ack_after_overlap", "accepted"),
            ("198.51.100.2:17444", "legacy", "legacy_sent"),
            ("198.51.100.2:17444", "ack_after_suffix", "rejected"),
        ] {
            let mut report = BlockRelayReport { official_attempts: 2, ..BlockRelayReport::default() };
            for result in [
                BlockRelayPeerResult { peer: "198.51.100.1:17444".into(), transport: "ack".into(), status: "accepted".into(), ..BlockRelayPeerResult::default() },
                BlockRelayPeerResult { peer: peer.into(), transport: transport.into(), status: status.into(), ..BlockRelayPeerResult::default() },
            ] { update_block_relay_report(&mut report, result); }
            assert!(!report.delivered());
        }
    }

    #[test]
    fn hf131_reachable_sync_peers_precede_silent_candidates() {
        let mut peers = vec!["silent-seed".to_string(), "direct-a".to_string(), "direct-b".to_string(), "healthy-seed".to_string()];
        let reachable = ["direct-a".to_string(), "direct-b".to_string(), "healthy-seed".to_string()].into_iter().collect::<HashSet<_>>();
        hf131_prefer_reachable(&mut peers, &reachable);
        assert_eq!(peers, vec!["direct-a", "direct-b", "healthy-seed", "silent-seed"]);
    }
}

#[cfg(test)]
mod hf129_coordinator_tests {
    use super::*;

    #[test]
    fn hf129_tip_observation_never_pairs_a_lower_hash_with_a_higher_height() {
        let mut report = P2PSyncReport::default();
        hf129_observe_tip(&mut report, 10, "hash-10");
        hf129_observe_tip(&mut report, 11, "");
        assert_eq!(report.best_peer_height, 11);
        assert!(report.best_peer_tip_hash.is_empty());
        hf129_observe_tip(&mut report, 11, "hash-11");
        assert_eq!(report.best_peer_tip_hash, "hash-11");
    }

    #[test]
    fn hf129_nested_writer_is_reentrant_and_background_coalesces() {
        let deadline = Instant::now() + Duration::from_millis(250);
        let deadline = hf130_effective_deadline(deadline);
        let _hf130_io_scope = Hf130IoScope::deadline(deadline);
        let outer = hf129_acquire_catchup_writer(true, deadline)
            .expect("outer acquire")
            .expect("outer writer");
        let nested = hf129_acquire_catchup_writer(false, deadline)
            .expect("nested acquire")
            .expect("same-thread nested writer");
        let other = thread::spawn(|| {
            hf129_acquire_catchup_writer(false, Instant::now() + Duration::from_millis(50))
                .expect("background acquire")
                .is_none()
        })
        .join()
        .expect("background thread");
        assert!(other);
        drop(nested);
        drop(outer);
        assert_eq!(
            HF129_CATCHUP_WRITER_DEPTH.with(|depth| depth.get()),
            0
        );
    }
}

#[cfg(test)]
mod hf125_tests {
    use super::*;

    #[test]
    fn hf129_official_tip_freshness_is_fail_closed() {
        let now = 2_000_000u64;
        assert!(hf129_official_tip_timestamp_is_fresh(Some(now), now));
        assert!(hf129_official_tip_timestamp_is_fresh(
            Some(now.saturating_sub(HF129_OFFICIAL_TIP_MAX_AGE_SECS)),
            now,
        ));
        assert!(!hf129_official_tip_timestamp_is_fresh(None, now));
        assert!(!hf129_official_tip_timestamp_is_fresh(
            Some(now.saturating_sub(HF129_OFFICIAL_TIP_MAX_AGE_SECS + 1)),
            now,
        ));
        assert!(!hf129_official_tip_timestamp_is_fresh(
            Some(now.saturating_add(HF129_OFFICIAL_TIP_FUTURE_SKEW_SECS + 1)),
            now,
        ));
    }

    fn regtest_settings() -> Settings {
        toml::from_str(include_str!("../config/regtest.toml"))
            .expect("regtest config parses")
    }

    fn dummy_tx(version: u32, marker: u8) -> Transaction {
        Transaction {
            version,
            inputs: vec![TxIn {
                previous_output: OutPoint::null(),
                signature_script: ScriptBuf(vec![marker]),
                sequence: u32::MAX,
            }],
            outputs: Vec::new(),
            locktime: marker as u32,
        }
    }

    #[test]
    fn hf128_learned_self_alias_quarantine_is_scoped_and_idempotent() {
        let mut settings = regtest_settings();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        settings.node.data_dir = format!("hf128-self-alias-{nonce}");
        let alias = format!("legacy-self-{nonce}.example:17444");

        assert!(!should_skip_outbound(&settings, &alias));
        assert!(hf128_quarantine_self_alias(&settings, &alias));
        assert!(should_skip_outbound(&settings, &alias));
        assert!(!hf128_quarantine_self_alias(&settings, &alias));

        let mut independent = settings.clone();
        independent.node.data_dir.push_str("-independent");
        assert!(!should_skip_outbound(&independent, &alias));
    }

    #[test]
    fn hf124_relay_batch_reserves_space_for_ordinary_transactions() {
        let settings = regtest_settings();
        let mut mempool = (0u8..20)
            .map(|marker| dummy_tx(TX_VERSION_POOL_SHARE, marker))
            .collect::<Vec<_>>();
        let ordinary = dummy_tx(1, 250);
        let ordinary_txid = ordinary.txid();
        mempool.push(ordinary);

        let batch = hf124_fair_mempool_relay_batch(&settings, &mempool, 8);
        assert_eq!(batch.len(), 8);
        assert!(batch.iter().any(|tx| tx.txid() == ordinary_txid));
        assert_eq!(
            batch
                .iter()
                .filter(|tx| is_pool_share_transaction(tx))
                .count(),
            7
        );
    }

    #[test]
    fn hf124_relay_batch_never_relays_more_than_one_block_of_shares() {
        let settings = regtest_settings();
        let mempool = (0u16..200)
            .map(|marker| {
                let mut tx = dummy_tx(TX_VERSION_POOL_SHARE, marker as u8);
                tx.locktime = marker as u32;
                tx
            })
            .collect::<Vec<_>>();

        let batch = hf124_fair_mempool_relay_batch(&settings, &mempool, 200);
        assert_eq!(batch.len(), settings.pools.max_share_txs_per_block);
        assert!(batch.iter().all(is_pool_share_transaction));
    }

    #[test]
    fn hf125_relay_report_requires_an_explicit_acknowledgement() {
        let mut report = BlockRelayReport::default();
        assert!(!report.delivered());

        update_block_relay_report(
            &mut report,
            BlockRelayPeerResult {
                peer: "127.0.0.1:1".to_string(),
                official: false,
                transport: "legacy".to_string(),
                status: "legacy_sent".to_string(),
                ..BlockRelayPeerResult::default()
            },
        );
        assert!(!report.delivered());

        update_block_relay_report(
            &mut report,
            BlockRelayPeerResult {
                peer: "127.0.0.1:2".to_string(),
                official: false,
                transport: "ack".to_string(),
                status: "accepted".to_string(),
                ..BlockRelayPeerResult::default()
            },
        );
        assert!(report.delivered());
        assert_eq!(report.acknowledged(), 1);

        // Once official delivery was attempted, one direct ACK alone is not
        // sufficient, but two distinct direct peers are a liveness quorum.
        report.official_attempts = 1;
        assert!(!report.delivered());
        update_block_relay_report(
            &mut report,
            BlockRelayPeerResult {
                peer: "127.0.0.1:4".to_string(),
                official: false,
                transport: "ack".to_string(),
                status: "already_known".to_string(),
                ..BlockRelayPeerResult::default()
            },
        );
        assert!(report.delivered());

        let mut official = BlockRelayReport {
            official_attempts: 1,
            ..BlockRelayReport::default()
        };
        update_block_relay_report(
            &mut official,
            BlockRelayPeerResult {
                peer: "127.0.0.1:3".to_string(),
                official: true,
                transport: "ack".to_string(),
                status: "already_known".to_string(),
                ..BlockRelayPeerResult::default()
            },
        );
        assert!(official.delivered());
        assert_eq!(official.acknowledged(), 1);
        assert_eq!(official.official_acknowledgements, 1);
    }

    #[test]
    fn hf125_submit_block_and_ack_wire_messages_roundtrip() {
        let settings = regtest_settings();
        let block = genesis_block(&settings).expect("regtest genesis");
        let request_id = 0xA5A5_1234_9876_4321;

        let submit = WireMessage::SubmitBlock {
            request_id,
            block: block.clone(),
        };
        let submit_raw = serde_json::to_vec(&submit).expect("serialize submit block");
        let submit_parsed: WireMessage =
            serde_json::from_slice(&submit_raw).expect("parse submit block");
        match submit_parsed {
            WireMessage::SubmitBlock {
                request_id: parsed_id,
                block: parsed_block,
            } => {
                assert_eq!(parsed_id, request_id);
                assert_eq!(parsed_block.block_hash(), block.block_hash());
            }
            other => panic!("unexpected submit message: {other:?}"),
        }

        let ack = WireMessage::BlockAck {
            request_id,
            block_hash: block.block_hash().to_string(),
            status: "accepted".to_string(),
            height: 0,
            tip_hash: block.block_hash().to_string(),
            reason: String::new(),
        };
        let ack_raw = serde_json::to_vec(&ack).expect("serialize block ack");
        let ack_parsed: WireMessage = serde_json::from_slice(&ack_raw).expect("parse block ack");
        match ack_parsed {
            WireMessage::BlockAck {
                request_id: parsed_id,
                block_hash,
                status,
                ..
            } => {
                assert_eq!(parsed_id, request_id);
                assert_eq!(block_hash, block.block_hash().to_string());
                assert_eq!(status, "accepted");
            }
            other => panic!("unexpected ack message: {other:?}"),
        }
    }


    #[test]
    fn hf126_same_height_competing_tip_classifier_is_exact() {
        assert!(hf126_is_same_height_competing_tip(25_807, "local", 25_807, "network"));
        assert!(!hf126_is_same_height_competing_tip(25_807, "local", 25_808, "network"));
        assert!(!hf126_is_same_height_competing_tip(25_807, "local", 25_807, "local"));
        assert!(!hf126_is_same_height_competing_tip(25_807, "local", 25_807, ""));
    }

    #[test]
    fn hf126_equal_height_reanchor_evidence_requires_independent_sources() {
        let mut evidence = Hf126EqualHeightTipEvidence {
            height: 25_807,
            tip_hash: "network".to_string(),
            http_matches: 1,
            official_tcp_matches: 0,
            direct_matches: 0,
        };
        assert!(!evidence.confirmed());

        evidence.official_tcp_matches = 1;
        assert!(!evidence.confirmed());

        evidence.http_matches = 0;
        evidence.official_tcp_matches = 2;
        assert!(evidence.confirmed());

        evidence.official_tcp_matches = 0;
        evidence.direct_matches = 2;
        assert!(!evidence.confirmed());

        evidence.http_matches = 1;
        assert!(evidence.confirmed());
    }

    #[test]
    fn hf127_submit_tx_and_ack_wire_messages_roundtrip() {
        let tx = dummy_tx(1, 77);
        let request_id = 0x1270_0000_0000_0001;
        let submit = WireMessage::SubmitTx {
            request_id,
            tx: tx.clone(),
        };
        let raw = serde_json::to_vec(&submit).expect("serialize SubmitTx");
        match serde_json::from_slice::<WireMessage>(&raw).expect("parse SubmitTx") {
            WireMessage::SubmitTx {
                request_id: parsed,
                tx: parsed_tx,
            } => {
                assert_eq!(parsed, request_id);
                assert_eq!(parsed_tx.txid(), tx.txid());
            }
            other => panic!("unexpected submit message: {other:?}"),
        }

        let ack = WireMessage::TxAck {
            request_id,
            txid: tx.txid().to_string(),
            status: "accepted".to_string(),
            height: 5,
            tip_hash: "tip".to_string(),
            reason: String::new(),
        };
        let raw = serde_json::to_vec(&ack).expect("serialize TxAck");
        match serde_json::from_slice::<WireMessage>(&raw).expect("parse TxAck") {
            WireMessage::TxAck {
                request_id: parsed,
                txid,
                status,
                ..
            } => {
                assert_eq!(parsed, request_id);
                assert_eq!(txid, tx.txid().to_string());
                assert_eq!(status, "accepted");
            }
            other => panic!("unexpected ack message: {other:?}"),
        }
    }

    #[test]
    fn hf127_transaction_delivery_prefers_official_or_two_peer_acknowledgements() {
        let mut report = TxRelayReport {
            official_attempts: 1,
            ..TxRelayReport::default()
        };
        update_tx_relay_report(
            &mut report,
            TxRelayPeerResult {
                peer: "peer-a".to_string(),
                transport: "ack".to_string(),
                status: "accepted".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        assert!(!report.delivered());

        // Repeating the same peer does not satisfy the independent-peer quorum.
        update_tx_relay_report(
            &mut report,
            TxRelayPeerResult {
                peer: "peer-a".to_string(),
                transport: "ack".to_string(),
                status: "already_known".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        assert!(!report.delivered());

        update_tx_relay_report(
            &mut report,
            TxRelayPeerResult {
                peer: "peer-b".to_string(),
                transport: "ack".to_string(),
                status: "already_known".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        assert!(report.delivered());

        let mut official = TxRelayReport {
            official_attempts: 1,
            ..TxRelayReport::default()
        };
        update_tx_relay_report(
            &mut official,
            TxRelayPeerResult {
                peer: "seed".to_string(),
                official: true,
                transport: "ack".to_string(),
                status: "accepted".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        assert!(official.delivered());

        let mut rejected = TxRelayReport {
            official_attempts: 1,
            ..TxRelayReport::default()
        };
        update_tx_relay_report(
            &mut rejected,
            TxRelayPeerResult {
                peer: "seed".to_string(),
                official: true,
                transport: "ack".to_string(),
                status: "rejected".to_string(),
                reason: "contextual rejection".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        for peer in ["peer-a", "peer-b"] {
            update_tx_relay_report(
                &mut rejected,
                TxRelayPeerResult {
                    peer: peer.to_string(),
                    transport: "ack".to_string(),
                    status: "accepted".to_string(),
                    ..TxRelayPeerResult::default()
                },
            );
        }
        assert!(rejected.delivered());
    }

    #[test]
    fn hf127_short_lived_submit_roles_are_not_peerbook_candidates() {
        assert!(hf127_is_submit_role("block-submit"));
        assert!(hf127_is_submit_role("tx-submit"));
        assert!(!hf127_is_submit_role("node"));
        assert!(!hf127_is_submit_role("miner"));
    }

    #[test]
    fn hf127_transaction_retry_backoff_is_bounded() {
        let report = TxRelayReport {
            attempts: 4,
            ..TxRelayReport::default()
        };
        assert_eq!(hf127_tx_retry_delay_secs(0, &report), 2);
        assert!(hf127_tx_retry_delay_secs(64, &report) <= HF127_TX_RETRY_IDLE_SECS);

        let mut delivered = TxRelayReport::default();
        update_tx_relay_report(
            &mut delivered,
            TxRelayPeerResult {
                peer: "peer-a".to_string(),
                transport: "ack".to_string(),
                status: "accepted".to_string(),
                ..TxRelayPeerResult::default()
            },
        );
        assert_eq!(
            hf127_tx_retry_delay_secs(0, &delivered),
            HF127_TX_RETRY_IDLE_SECS
        );
    }

    #[test]
    fn hf125_pending_block_relay_is_durable_and_network_scoped() {
        let mut settings = regtest_settings();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let data_dir = std::env::temp_dir().join(format!(
            "qub-hf125-pending-relay-{}-{nonce}",
            std::process::id()
        ));
        settings.node.data_dir = data_dir.to_string_lossy().to_string();
        let block = genesis_block(&settings).expect("regtest genesis");
        let local = ChainState::from_blocks(vec![block.clone()], &settings)
            .expect("genesis chain");

        schedule_pending_block_relay(&settings, &block, 0)
            .expect("write pending block relay");
        let pending = load_pending_block_relay(&settings)
            .expect("load pending relay")
            .expect("pending relay exists");
        assert_eq!(pending.network, settings.network.name);
        assert_eq!(pending.block_hash, block.block_hash().to_string());
        assert_eq!(pending.last_stale_parent_height, 0);
        assert!(pending.last_stale_parent_tip.is_empty());
        assert_eq!(pending.last_stale_parent_reports, 0);
        assert_eq!(pending.last_official_stale_parent_reports, 0);
        assert!(pending_block_relay_waiting(&settings, &local));

        clear_pending_block_relay(&settings);
        assert!(load_pending_block_relay(&settings)
            .expect("load cleared relay")
            .is_none());
        let _ = fs::remove_dir_all(data_dir);
    }

}

#[cfg(test)]
mod hf130_sync_tests {
    use super::*;

    fn socket_pair() -> (TcpStream, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind fixture listener");
        let client = TcpStream::connect(listener.local_addr().unwrap()).expect("connect fixture");
        let (server, _) = listener.accept().expect("accept fixture");
        (client, server)
    }

    #[test]
    fn hf130_drip_feed_frame_obeys_absolute_deadline() {
        let (client, mut server) = socket_pair();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let peer = thread::spawn(move || {
            for _ in 0..100 {
                if server.write_all(b" ").is_err() { break; }
                thread::sleep(Duration::from_millis(15));
            }
        });
        let start = Instant::now();
        let _scope = Hf130IoScope::deadline(start + Duration::from_millis(120));
        let mut reader = BufReader::new(client);
        let error = read_wire(&mut reader, 64 * 1024).unwrap_err();
        let elapsed = start.elapsed();
        drop(reader);
        peer.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(elapsed < Duration::from_millis(700), "drip feed escaped total deadline: {elapsed:?}");
    }

    #[test]
    fn hf130_blocked_write_obeys_absolute_deadline() {
        let (mut client, _server) = socket_pair();
        client.set_write_timeout(Some(Duration::from_secs(5))).unwrap();
        // Larger than a normal socket send buffer; the peer never consumes it.
        let message = WireMessage::Reject { reason: "x".repeat(16 * 1024 * 1024) };
        let start = Instant::now();
        let _scope = Hf130IoScope::deadline(start + Duration::from_millis(120));
        assert!(send_wire(&mut client, &message).is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn hf130_silent_peer_read_is_cancelled() {
        let (client, _server) = socket_pair();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let trigger = stop.clone();
        let cancel = thread::spawn(move || {
            thread::sleep(Duration::from_millis(40));
            trigger.store(true, Ordering::Relaxed);
        });
        let start = Instant::now();
        let error = hf130_with_mining_cancellation(stop, || {
            read_wire(&mut BufReader::new(client), 64 * 1024).unwrap_err()
        });
        cancel.join().unwrap();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(hf130_io_context().cancel.is_none(), "cancellation context leaked after closure");
    }

    #[test]
    fn hf130_nested_budget_cannot_extend_parent() {
        assert_eq!(hf130_connect_slice(Duration::from_secs(5), false), Duration::from_secs(5));
        assert_eq!(hf130_connect_slice(Duration::from_secs(5), true), Duration::from_secs(1));
        assert_eq!(hf130_connect_slice(Duration::from_millis(650), true), Duration::from_millis(650));
        let deadline = Instant::now() + Duration::from_millis(25);
        let _outer = Hf130IoScope::deadline(deadline);
        {
            let _inner = Hf130IoScope::deadline(Instant::now() + Duration::from_secs(10));
            assert_eq!(hf130_effective_deadline(Instant::now() + Duration::from_secs(20)), deadline);
            let inherited = hf130_io_context();
            thread::scope(|scope| {
                scope.spawn(|| {
                    let _child = Hf130IoScope::enter(inherited.clone());
                    assert_eq!(hf130_effective_deadline(Instant::now() + Duration::from_secs(30)), deadline);
                }).join().unwrap();
            });
        }
        thread::sleep(Duration::from_millis(30));
        assert_eq!(remaining_ms(Instant::now() + Duration::from_secs(20)), 0);
    }

    #[test]
    fn hf130_fresh_proof_rejects_ancestor_pending_and_changed_tip() {
        let mut report = P2PSyncReport::default();
        hf129_observe_tip(&mut report, 100, "official-100");
        hf129_mark_canonical_tip_proven(&mut report);
        assert!(!hf130_proof_matches_final(&report, 105, "local-105", true));
        assert!(!report.canonical_tip_proven, "ancestor permission created exact acknowledgement");
        report.canonical_proof = Some(Hf130CanonicalProof {
            height: 100, tip_hash: "official-100".to_string(), observed_at: Instant::now(),
            source: Hf130CanonicalEvidence::OfficialTcp,
        });
        assert!(hf130_proof_matches_final(&report, 100, "official-100", false));
        assert!(!hf130_proof_matches_final(&report, 100, "official-100", true));
        assert!(!hf130_proof_matches_final(&report, 101, "official-101", false));
        assert!(!hf130_proof_matches_final(&report, 100, "competing-100", false));
        let wire = serde_json::to_string(&report).unwrap();
        let replay: P2PSyncReport = serde_json::from_str(&wire).unwrap();
        assert!(!hf130_proof_matches_final(&replay, 100, "official-100", false));
        report.canonical_proof.as_mut().unwrap().observed_at = Instant::now() - Duration::from_secs(46);
        assert!(!hf130_proof_matches_final(&report, 100, "official-100", false));
        let exact = hf130_quorum_verdict(2, false, false, true);
        assert!(hf130_apply_quorum_verdict(&mut report, exact, 100, "official-100"));
        assert!(hf130_proof_matches_final(&report, 100, "official-100", false));
        let sibling = hf130_quorum_verdict(0, false, true, true);
        assert!(hf130_apply_quorum_verdict(&mut report, sibling, 100, "official-100"));
        assert!(!report.canonical_tip_proven, "mineable tie retained prior exact proof");
        assert!(!hf130_proof_matches_final(&report, 100, "official-100", false));
        let ahead = hf130_quorum_verdict(2, true, false, true);
        assert!(!hf130_apply_quorum_verdict(&mut report, ahead, 100, "official-100"));
        let pending_tie = hf130_quorum_verdict(0, false, true, false);
        assert!(!hf130_apply_quorum_verdict(&mut report, pending_tie, 100, "official-100"));
    }

    #[test]
    fn hf130_dns_cache_bounds_pending_work() {
        let names = (0..4).map(|n| format!("hf130-unresolved-fixture-{n}.invalid:1")).collect::<Vec<_>>();
        {
            let mut entries = hf130_dns_entries().lock().unwrap();
            for name in &names {
                entries.insert(name.clone(), Arc::new(Hf130DnsEntry {
                    created: Instant::now(), result: Mutex::new(None),
                }));
            }
        }
        // The actual resolution entry point must reject a fifth unresolved
        // name before attempting any platform DNS work.
        let result = hf130_resolve_peer("hf130-extra-fixture.invalid:1", Instant::now() + Duration::from_secs(1));
        let coalesced = hf130_resolve_peer(&names[0], Instant::now() + Duration::from_millis(20));
        {
            let mut entries = hf130_dns_entries().lock().unwrap();
            for name in &names { entries.remove(name); }
        }
        assert!(result.unwrap_err().to_string().contains("capacity busy"));
        assert!(coalesced.unwrap_err().to_string().contains("deadline expired"),
            "same unresolved name should share its pending slot, not request a fifth worker");
    }

    #[test]
    fn hf130_probe_unrelated_messages_share_one_deadline() {
        let (client, mut server) = socket_pair();
        client.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let peer = thread::spawn(move || {
            let mut raw = serde_json::to_vec(&WireMessage::Ping { nonce: 1 }).unwrap();
            raw.push(b'\n');
            for _ in 0..100 {
                if server.write_all(&raw).is_err() { break; }
                thread::sleep(Duration::from_millis(15));
            }
        });
        let start = Instant::now();
        let _scope = Hf130IoScope::deadline(start + Duration::from_millis(120));
        let mut reader = BufReader::new(client);
        let mut received = 0;
        while read_wire(&mut reader, 64 * 1024).is_ok() { received += 1; }
        let elapsed = start.elapsed();
        drop(reader);
        peer.join().unwrap();
        assert!(received > 1);
        assert!(elapsed < Duration::from_millis(700));
    }

    #[test]
    fn hf130_snapshot_generation_pairing_rejects_mismatch() {
        let tip = OfficialTipSnapshot {
            network: "mainnet".to_string(), height: 100, tip_hash: "hash-100".to_string(),
            chain_sha256: None, published_at_unix: Some(1),
        };
        let mut tail = OfficialTailSnapshot {
            network: "mainnet".to_string(), start_height: 100, tip_height: 101,
            tip_hash: "hash-101".to_string(), blocks: Vec::new(),
        };
        assert!(hf130_decode_tail_generation(&serde_json::to_string(&tail).unwrap(), &tip).is_err());
        tail.tip_height = 100;
        assert!(hf130_decode_tail_generation(&serde_json::to_string(&tail).unwrap(), &tip).is_err());
        tail.tip_hash = "hash-100".to_string();
        // Generation pairing accepts metadata here; consensus still rejects
        // empty/invalid block payloads in apply_official_tail_snapshot.
        assert!(hf130_decode_tail_generation(&serde_json::to_string(&tail).unwrap(), &tip).is_ok());
    }
}

#[cfg(test)]
mod hf132_sync_liveness_tests {
    use super::*;

    fn settings(name: &str) -> Settings {
        let mut settings: Settings = toml::from_str(include_str!("../config/regtest.toml")).unwrap();
        settings.node.data_dir = std::env::temp_dir().join(format!(
            "qub-hf132-p2p-{name}-{}-{}", std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos(),
        )).display().to_string();
        settings.p2p.enabled = false;
        NodePaths::from_settings(&settings).ensure_dirs().unwrap();
        settings
    }

    fn extend(settings: &Settings, chain: &mut ChainState, count: usize) {
        let mut wallet = WalletFile::new(&settings.network.name);
        let key = wallet.create_key(settings, "HF132 sync test", 0).unwrap();
        let address = Address::parse_with_prefix(&key.address, &settings.network.address_prefix).unwrap();
        for _ in 0..count {
            let block = mine_next_block(chain, settings, &address, MiningOptions {
                duty_cycle_percent: 100, max_hashes: Some(5_000_000),
            }).unwrap();
            chain.connect_block(block, settings).unwrap();
        }
    }

    fn sockets() -> (TcpStream, BufReader<TcpStream>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        server.set_read_timeout(Some(Duration::from_millis(100))).unwrap();
        (client, BufReader::new(server))
    }

    #[test]
    fn hf132_memory_idle_peer_releases_handshake_snapshot() {
        let settings = settings("memory-idle-peer");
        let local = load_or_init_chain(&settings).unwrap();
        let live = Arc::new(Mutex::new(local));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        let (server, addr) = listener.accept().unwrap();
        let worker_live = live.clone();
        let worker = thread::spawn(move || handle_peer(
            server, addr.to_string(), false, false, settings, worker_live,
            Arc::new(Mutex::new(HashSet::new())), Arc::new(Mutex::new(HashSet::new())),
            Arc::new(AtomicBool::new(false)),
        ));
        let mut reader = BufReader::new(client.try_clone().unwrap());
        send_wire(&mut client, &WireMessage::Ping { nonce: 132 }).unwrap();
        let mut pong = false;
        for _ in 0..12 {
            if matches!(read_wire(&mut reader, 1024 * 1024).unwrap(), WireMessage::Pong { nonce: 132 }) {
                pong = true; break;
            }
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        let released = loop {
            let counts = {
                let chain = live.lock().unwrap();
                (Arc::strong_count(&chain.blocks), Arc::strong_count(&chain.utxos),
                 Arc::strong_count(&chain.mempool))
            };
            if counts == (1, 1, 1) { break true; }
            if Instant::now() >= deadline { break false; }
            thread::sleep(Duration::from_millis(10));
        };
        let _ = client.shutdown(std::net::Shutdown::Both);
        worker.join().unwrap().unwrap();
        assert!(pong, "peer must have entered its persistent message loop");
        assert!(released, "an idle peer still pins the handshake's canonical generation");
    }

    #[test]
    fn hf132_memory_inbound_writers_drop_busy_candidates_then_retry() {
        let settings = settings("memory-writer");
        let local = load_or_init_chain(&settings).unwrap();
        let mut remote = local.clone();
        extend(&settings, &mut remote, 1);
        let live = Arc::new(Mutex::new(local));
        let guard = hf129_acquire_catchup_writer(true, Instant::now() + Duration::from_secs(10))
            .unwrap().expect("test writer");
        let (done_tx, done_rx) = mpsc::channel();
        let mut workers = Vec::new();
        for _ in 0..12 {
            let settings = settings.clone(); let live = live.clone();
            let block = remote.blocks[1].clone(); let done_tx = done_tx.clone();
            workers.push(thread::spawn(move || {
                let (mut stream, _reader) = sockets();
                let result = process_peer_message(&settings, "fixture-peer", WireMessage::Chain {
                    start_height: 1, blocks: vec![block],
                }, &mut stream, &live, &Arc::new(Mutex::new(HashSet::new())),
                    &Arc::new(AtomicBool::new(false)), &mut PeerSession::default(), false);
                done_tx.send(result.is_ok()).unwrap();
            }));
        }
        drop(done_tx);
        for _ in 0..12 {
            assert!(done_rx.recv_timeout(Duration::from_secs(3)).unwrap(), "busy frames must return promptly");
        }
        for worker in workers { worker.join().unwrap(); }
        assert_eq!(live.lock().unwrap().height(), 0);
        assert_eq!(load_committed_chain(&settings, true).unwrap().height(), 0);
        // Same-thread nesting is intentional: it exercises the coordinator's
        // ownership contract without a race against unrelated parallel tests.
        let (mut stream, _reader) = sockets();
        process_peer_message(&settings, "fixture-peer", WireMessage::Chain {
            start_height: 1, blocks: vec![remote.blocks[1].clone()],
        }, &mut stream, &live, &Arc::new(Mutex::new(HashSet::new())),
            &Arc::new(AtomicBool::new(false)), &mut PeerSession::default(), false).unwrap();
        assert_eq!(live.lock().unwrap().tip_hash(), remote.tip_hash());
        assert_eq!(load_committed_chain(&settings, true).unwrap().tip_hash(), remote.tip_hash());
        drop(guard);
    }

    #[test]
    fn hf132_memory_overlap_extension_matches_full_replay() {
        let settings = settings("memory-overlap");
        let mut local = load_or_init_chain(&settings).unwrap();
        extend(&settings, &mut local, 3);
        let mut remote = local.clone(); extend(&settings, &mut remote, 2);
        let reference = ChainState::from_blocks(remote.blocks.as_ref().clone(), &settings).unwrap();
        assert!(try_adopt_overlapping_blocks(&mut local, 2, remote.blocks[2..].to_vec(), &settings, false).unwrap());
        assert_eq!(local.blocks, reference.blocks);
        assert_eq!(local.utxos, reference.utxos);
        local.validate_all(&settings).unwrap();
    }

    #[test]
    fn hf132_memory_invalid_overlap_and_suffix_preserve_parent() {
        let settings = settings("memory-invalid");
        let mut local = load_or_init_chain(&settings).unwrap();
        extend(&settings, &mut local, 2);
        let mut remote = local.clone(); extend(&settings, &mut remote, 2);
        let before = local.clone();
        let mut invalid_overlap = remote.blocks[1..].to_vec();
        invalid_overlap[0].transactions.clear();
        assert!(try_adopt_overlapping_blocks(&mut local, 1, invalid_overlap, &settings, false).is_err());
        assert_eq!(local.blocks, before.blocks); assert_eq!(local.utxos, before.utxos);
        let mut invalid_suffix = remote.blocks[1..].to_vec();
        invalid_suffix.last_mut().unwrap().transactions.clear();
        assert!(try_adopt_overlapping_blocks(&mut local, 1, invalid_suffix, &settings, false).is_err());
        assert_eq!(local.blocks, before.blocks); assert_eq!(local.utxos, before.utxos);
    }

    #[test]
    fn hf132_memory_real_fork_keeps_full_validation_and_work_selection() {
        let settings = settings("memory-real-fork");
        let genesis = load_or_init_chain(&settings).unwrap();
        let mut local = genesis.clone(); extend(&settings, &mut local, 2);
        let mut remote = genesis; extend(&settings, &mut remote, 4);
        assert_ne!(local.blocks[1].block_hash(), remote.blocks[1].block_hash());
        let mut reference = local.clone();
        assert!(reference.try_adopt_peer_chain(remote.blocks.as_ref().clone(), &settings, false).unwrap());
        assert!(try_adopt_overlapping_blocks(&mut local, 1, remote.blocks[1..].to_vec(), &settings, false).unwrap());
        assert_eq!(local.blocks, reference.blocks); assert_eq!(local.utxos, reference.utxos);
    }

    #[test]
    fn hf132_memory_streamed_chain_is_byte_identical_to_legacy_frames() {
        let mut settings = settings("memory-wire");
        settings.p2p.max_blocks_per_message = 2;
        let mut local = load_or_init_chain(&settings).unwrap();
        extend(&settings, &mut local, 4);
        for start in [0u32, 1, 4, 99] {
            let (mut stream, mut reader) = sockets();
            send_chain(&mut stream, &settings, &local, start).unwrap();
            let mut actual = Vec::new(); reader.read_until(b'\n', &mut actual).unwrap();
            let end = local.blocks.len().min(start as usize + 2);
            let blocks = local.blocks.get(start as usize..end).unwrap_or(&[]).to_vec();
            let mut expected = serde_json::to_vec(&WireMessage::Chain { start_height: start, blocks }).unwrap();
            expected.push(b'\n');
            assert_eq!(actual, expected, "legacy protocol-2 bytes/count changed at {start}");
        }
    }

    #[test]
    fn hf132_memory_serialization_failure_closes_partial_frame_and_restores_timeout() {
        use serde::ser::SerializeSeq;
        use std::io::Read;
        struct FailsAfterWrite;
        impl Serialize for FailsAfterWrite {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                let mut seq = serializer.serialize_seq(None)?;
                let value = "x".repeat(4096);
                for _ in 0..40 { seq.serialize_element(&value)?; }
                Err(<S::Error as serde::ser::Error>::custom("forced serialization failure"))
            }
        }
        let (mut stream, mut reader) = sockets();
        reader.get_ref().set_read_timeout(Some(Duration::from_secs(3))).unwrap();
        stream.set_write_timeout(Some(Duration::from_secs(2))).unwrap();
        let receiver = thread::spawn(move || {
            let mut data = Vec::new(); reader.read_to_end(&mut data).unwrap(); data
        });
        let result = hf132_send_json(&mut stream, &FailsAfterWrite);
        assert!(result.is_err());
        assert_eq!(stream.write_timeout().unwrap(), Some(Duration::from_secs(2)));
        let bytes = receiver.join().unwrap();
        assert!(!bytes.is_empty()); assert_ne!(bytes.last(), Some(&b'\n'));
        assert!(send_wire(&mut stream, &WireMessage::Ping { nonce: 1 }).is_err());
    }

    #[test]
    fn hf132_completed_suffix_survives_expired_network_slice() {
        let settings = settings("expired-slice");
        let mut local = load_or_init_chain(&settings).unwrap();
        let mut remote = local.clone();
        extend(&settings, &mut remote, 2);
        {
            let _network_scope = Hf130IoScope::deadline(Instant::now());
            assert!(hf130_io_remaining(Instant::now()).is_err());
            assert!(try_adopt_overlapping_blocks(&mut local, 1, remote.blocks[1..].to_vec(), &settings, false).unwrap());
            save_chain(&settings, &local).unwrap();
        }
        let restarted = load_committed_chain(&settings, true).unwrap();
        assert_eq!(restarted.blocks, remote.blocks);
        assert_eq!(restarted.utxos, remote.utxos);
    }

    #[test]
    fn hf132_cancelled_suffix_keeps_committed_parent() {
        let settings = settings("cancelled-suffix");
        let mut local = load_or_init_chain(&settings).unwrap();
        let original_tip = local.tip_hash();
        let mut remote = local.clone();
        extend(&settings, &mut remote, 2);
        let result = hf130_with_mining_cancellation(Arc::new(AtomicBool::new(true)), || {
            try_adopt_overlapping_blocks(&mut local, 1, remote.blocks[1..].to_vec(), &settings, false)
        });
        assert!(result.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(local.tip_hash(), original_tip);
        assert_eq!(load_committed_chain(&settings, true).unwrap().tip_hash(), original_tip);
    }

    #[test]
    fn hf132_moving_tip_reuses_request_then_follows_committed_suffix() {
        let settings = settings("moving-tip");
        let local = load_or_init_chain(&settings).unwrap();
        let mut remote = local.clone();
        extend(&settings, &mut remote, 2);
        let (mut client, mut reader) = sockets();
        let mut session = PeerSession::default();
        assert!(request_peer_chain_if_needed(&mut client, &local, 1, &remote.blocks[1].block_hash().to_string(), &mut session).unwrap());
        assert!(!request_peer_chain_if_needed(&mut client, &local, 2, &remote.tip_hash().to_string(), &mut session).unwrap());
        assert!(matches!(read_wire(&mut reader, settings.p2p.max_message_bytes).unwrap(), WireMessage::GetChain { from_height: 1 }));
        session.height = 2;
        session.tip_hash = remote.tip_hash().to_string();
        let mut report = P2PSyncReport::default();
        process_client_message(&settings, "fixture-peer", WireMessage::Chain {
            start_height: 1, blocks: vec![remote.blocks[1].clone()],
        }, &mut client, &mut report, &mut session).unwrap();
        assert!(matches!(read_wire(&mut reader, settings.p2p.max_message_bytes).unwrap(), WireMessage::GetChain { from_height: 2 }));
        assert_eq!(report.blocks_connected, 1);
        assert_eq!(load_committed_chain(&settings, true).unwrap().height(), 1);
        process_client_message(&settings, "fixture-peer", WireMessage::Chain {
            start_height: 2, blocks: vec![remote.blocks[2].clone()],
        }, &mut client, &mut report, &mut session).unwrap();
        let complete = load_committed_chain(&settings, true).unwrap();
        assert_eq!(complete.blocks, remote.blocks);
        assert_eq!(complete.utxos, remote.utxos);
        assert_eq!(report.blocks_connected, 2);
    }

    #[test]
    fn hf132_duplicate_chunk_does_not_request_old_fork_window() {
        let settings = settings("duplicate-chunk");
        let mut local = load_or_init_chain(&settings).unwrap();
        extend(&settings, &mut local, 1);
        save_chain(&settings, &local).unwrap();
        let (mut client, mut reader) = sockets();
        let mut session = PeerSession::default();
        session.height = 2;
        session.tip_hash = "next-tip".into();
        request_peer_chain_if_needed(&mut client, &local, 2, "next-tip", &mut session).unwrap();
        assert!(matches!(read_wire(&mut reader, settings.p2p.max_message_bytes).unwrap(), WireMessage::GetChain { from_height: 2 }));
        let mut report = P2PSyncReport::default();
        process_client_message(&settings, "fixture-peer", WireMessage::Chain {
            start_height: 1, blocks: vec![local.blocks[1].clone()],
        }, &mut client, &mut report, &mut session).unwrap();
        assert_eq!(report.blocks_connected, 0);
        assert!(is_timeout(&read_wire(&mut reader, settings.p2p.max_message_bytes).unwrap_err()));
    }

    #[test]
    fn hf132_slow_peer_network_slice_leaves_healthy_peer_time() {
        let settings = settings("fair-peers");
        let local = load_or_init_chain(&settings).unwrap();
        let mut remote = local.clone();
        extend(&settings, &mut remote, 1);
        let mut version = version_message_with_role(&settings, &remote, "node").unwrap();
        if let WireMessage::Version { node_id, .. } = &mut version { *node_id = "independent-fixture-peer".into(); }
        let stalled = TcpListener::bind("127.0.0.1:0").unwrap();
        let healthy = TcpListener::bind("127.0.0.1:0").unwrap();
        let stalled_addr = stalled.local_addr().unwrap().to_string();
        let healthy_addr = healthy.local_addr().unwrap().to_string();
        let stalled_version = version.clone();
        let stalled_worker = thread::spawn(move || {
            let (mut stream, _) = stalled.accept().unwrap();
            stream.set_write_timeout(Some(Duration::from_millis(100))).unwrap();
            send_wire(&mut stream, &stalled_version).unwrap();
            // Advertising a valid tip and sending heartbeats is not block progress.
            let until = Instant::now() + Duration::from_secs(2);
            while Instant::now() < until {
                if send_wire(&mut stream, &WireMessage::Ping { nonce: 1 }).is_err() { break; }
                thread::sleep(Duration::from_millis(10));
            }
        });
        let served_blocks = remote.blocks[1..].to_vec();
        let healthy_worker = thread::spawn(move || {
            let (mut stream, _) = healthy.accept().unwrap();
            stream.set_write_timeout(Some(Duration::from_millis(500))).unwrap();
            send_wire(&mut stream, &version).unwrap();
            send_wire(&mut stream, &WireMessage::Chain { start_height: 1, blocks: served_blocks }).unwrap();
            let mut reader = BufReader::new(stream);
            reader.get_ref().set_read_timeout(Some(Duration::from_millis(100))).unwrap();
            while read_wire(&mut reader, 1_000_000).is_ok() {}
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut report = P2PSyncReport::default();
        let first_slice = hf132_peer_io_deadline(deadline, 2);
        let _ = sync_peer_adaptive_session(&settings, &stalled_addr, first_slice, &mut report);
        assert!(remaining_ms(deadline) >= 300, "the first peer exhausted the second peer's network opportunity");
        sync_peer_adaptive_session(&settings, &healthy_addr, hf132_peer_io_deadline(deadline, 1), &mut report).unwrap();
        stalled_worker.join().unwrap();
        healthy_worker.join().unwrap();
        assert_eq!(load_committed_chain(&settings, true).unwrap().tip_hash(), remote.tip_hash());
    }

    #[test]
    fn hf132_reconcile_rejects_stale_capture_and_keeps_current_equal_work_commit() {
        let settings = settings("capture");
        let genesis = load_or_init_chain(&settings).unwrap();
        let mut first = genesis.clone();
        extend(&settings, &mut first, 1);
        save_chain(&settings, &first).unwrap();
        let first_identity = fast_storage_identity(&settings).unwrap().unwrap();
        let mut second = genesis;
        extend(&settings, &mut second, 1);
        assert_ne!(first.tip_hash(), second.tip_hash());
        // Model the exact next CURRENT identity for a committed equal-work
        // reanchor; its storage authorization is tested in fast_storage tests.
        let mut second_identity = first_identity.clone();
        second_identity.generation += 1;
        second_identity.state_revision += 1;
        second_identity.tip_hash = second.tip_hash();
        assert!(!hf132_committed_capture_is_current(Some(&first_identity), Some(&second_identity)));
        assert!(hf132_committed_capture_is_current(Some(&second_identity), Some(&second_identity)));
        assert!(!hf132_committed_capture_is_current(None, None));
        let mut live = first;
        hf132_reconcile_committed_view(&mut live, second.clone(), &settings).unwrap();
        assert_eq!(live.tip_hash(), second.tip_hash());
        let mut changed_revision = second_identity.clone();
        changed_revision.state_revision += 1;
        assert!(!hf132_committed_capture_is_current(Some(&second_identity), Some(&changed_revision)));
    }

    #[test]
    fn hf132_stop_mining_waiter_does_not_cancel_active_sync_writer() {
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let auto = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(3);
            let guard = loop {
                if let Some(guard) = hf129_acquire_catchup_writer(false, deadline).unwrap() { break guard; }
                assert!(Instant::now() < deadline);
                thread::sleep(Duration::from_millis(5));
            };
            ready_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(3)).unwrap();
            assert_eq!(HF129_CATCHUP_WRITER_DEPTH.with(|depth| depth.get()), 1);
            drop(guard);
        });
        ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let waiter_stop = stop.clone();
        let mining = thread::spawn(move || hf130_with_mining_cancellation(waiter_stop, || {
            hf129_acquire_catchup_writer(true, Instant::now() + Duration::from_secs(3))
                .map(|guard| guard.is_some())
        }));
        let until = Instant::now() + Duration::from_secs(2);
        while HF129_PRIORITY_CATCHUP_WAITERS.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < until);
            thread::yield_now();
        }
        stop.store(true, Ordering::SeqCst);
        assert!(mining.join().unwrap().unwrap_err().to_string().contains("cancelled"));
        assert!(hf129_acquire_catchup_writer(false, Instant::now() + Duration::from_millis(50)).unwrap().is_none(),
            "Stop released the independent auto-sync writer");
        let mut report = P2PSyncReport::default();
        hf129_mark_writer_busy(&mut report, "coalesced-auto");
        assert!(report.writer_busy);
        assert!(!report.progressed && !report.fresh_at_tip && !report.canonical_tip_proven);
        release_tx.send(()).unwrap();
        auto.join().unwrap();
        let manual = hf129_acquire_catchup_writer(true, Instant::now() + Duration::from_secs(1)).unwrap();
        assert!(manual.is_some(), "writer did not become available after real completion");
    }

}
