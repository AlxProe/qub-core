use crate::*;
use anyhow::{bail, Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs, UdpSocket};
#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

const PROTOCOL_VERSION: u32 = 2;
const USER_AGENT: &str = "/QUB Core:1.7.9/"; // HF122
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
}

fn load_chain_for_hf90_catchup(settings: &Settings) -> Result<ChainState> {
    // HF107/v1.6.9: catch-up workers can be invoked repeatedly while the GUI is
    // rendering. Replaying the whole chain at every tiny probe is slow enough to
    // let the public tip move away again. Use the persisted chain/UTXO snapshot
    // with checkpoint + recent-link checks for the read side, then consensus-
    // validate every newly received block through connect_block / from_blocks.
    load_or_init_chain_for_ui_fast(settings).or_else(|_| load_or_init_chain(settings))
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
    Tx {
        tx: Transaction,
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

pub fn run_node(settings: Settings) -> Result<()> {
    if !settings.p2p.enabled {
        bail!("p2p.enabled=false in config; set it true for node mode");
    }

    let chain = Arc::new(Mutex::new(load_or_init_chain(&settings)?));
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
        thread::spawn(move || loop {
            match listener.accept() {
                Ok((stream, addr)) => {
                    if accept_inbound.load(Ordering::Relaxed)
                        >= accept_settings.p2p.max_inbound_peers
                    {
                        continue;
                    }
                    accept_inbound.fetch_add(1, Ordering::Relaxed);
                    let s = accept_settings.clone();
                    let c = accept_chain.clone();
                    let p = accept_peers.clone();
                    let a = accept_active.clone();
                    let inbound = accept_inbound.clone();
                    thread::spawn(move || {
                        let peer = addr.to_string();
                        if let Err(err) = handle_peer(stream, peer.clone(), false, s, c, p, a) {
                            if !is_benign_disconnect(&err) {
                                eprintln!("p2p inbound {peer} ended: {err:#}");
                            }
                        }
                        inbound.fetch_sub(1, Ordering::Relaxed);
                    });
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(100))
                }
                Err(err) => {
                    eprintln!("p2p accept error: {err}");
                    thread::sleep(Duration::from_secs(1));
                }
            }
        });
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
    let mut last_light_catchup = Instant::now() - Duration::from_secs(60);

    loop {
        let known = {
            let p = peers.lock().expect("peer mutex poisoned");
            p.iter().cloned().collect::<Vec<_>>()
        };
        let mut spawned = 0usize;
        for addr in known {
            if spawned >= settings.p2p.max_outbound_peers {
                break;
            }
            if should_skip_outbound(&settings, &addr) {
                continue;
            }
            let already_active = {
                let a = active.lock().expect("active peer mutex poisoned");
                a.contains(&addr)
            };
            if already_active {
                continue;
            }
            match connect_peer(&addr, Duration::from_secs(3)) {
                Ok(stream) => {
                    spawned += 1;
                    let s = settings.clone();
                    let c = chain.clone();
                    let p = peers.clone();
                    let a = active.clone();
                    thread::spawn(move || {
                        if let Err(err) = handle_peer(stream, addr.clone(), true, s, c, p, a) {
                            if !is_benign_disconnect(&err) {
                                eprintln!("p2p outbound {addr} ended: {err:#}");
                            }
                        }
                    });
                }
                Err(_) => {}
            }
        }

        if let Ok(disk_chain) = load_or_init_chain(&settings) {
            let mut relay_after_merge = Vec::<Transaction>::new();
            let mut local = chain.lock().expect("chain mutex poisoned");

            // HF75/v1.5.8: mempool preservation has priority over disk/memory
            // chain-tip reconciliation. GUI/CLI actions update chain.json first;
            // the embedded node must merge those txs before it ever writes its
            // in-memory chain back to disk, otherwise pending JIN/Library/Blast/
            // MultiSend txs can appear for hours and then vanish during a later
            // sync/adoption cycle.
            relay_after_merge.extend(merge_mempool_from_chain(&mut local, &disk_chain, &settings));

            if disk_chain.height() > local.height() || disk_chain.tip_hash() != local.tip_hash() {
                // HF107/v1.6.9: if a detached catch-up/repair worker wrote a taller
                // validated chain to disk, the embedded P2P node must adopt it into
                // memory, not overwrite it with an older in-memory tip on the next
                // heartbeat. This was a likely cause of nodes visibly knowing
                // network #N but staying frozen at local #(N-k).
                if disk_chain.height() > local.height() {
                    let mut adopted = disk_chain.clone();
                    // HF117: if disk was advanced by a catch-up worker while the
                    // embedded node still held a stale in-memory suffix, resurrect
                    // txs from that replaced suffix before overwriting memory.
                    let keep_mempool = local.reorg_mempool_candidates_for(&adopted);
                    adopted.rebuild_mempool_from(keep_mempool, &settings);
                    relay_after_merge.extend(adopted.mempool.iter().take(64).cloned());
                    *local = adopted;
                    let _ = save_chain(&settings, &local);
                } else {
                    let disk_blocks = disk_chain.blocks.clone();
                    match local.try_adopt_peer_chain(disk_blocks, &settings, false) {
                        Ok(true) => {
                            relay_after_merge.extend(merge_mempool_from_chain(
                                &mut local,
                                &disk_chain,
                                &settings,
                            ));
                            let _ = save_chain(&settings, &local);
                        }
                        Ok(false) => {
                            if local.tip_hash() != disk_chain.tip_hash()
                                || !relay_after_merge.is_empty()
                            {
                                let _ = save_chain(&settings, &local);
                            }
                        }
                        Err(_) => {
                            let _ = save_chain(&settings, &local);
                        }
                    }
                }
            } else if disk_chain.tip_hash() == local.tip_hash()
                && disk_chain.mempool_txids() != local.mempool_txids()
            {
                // HF76/v1.5.8: persist either direction of mempool merge. HF75 saved only
                // when disk contributed new txs; if the embedded node had accepted peer txs
                // while disk had fewer, the next GUI/status read could see them as vanished.
                let _ = save_chain(&settings, &local);
            }
            let mempool_for_periodic = if last_mempool_relay.elapsed() >= Duration::from_secs(6) {
                last_mempool_relay = Instant::now();
                // HF117: recover exact wallet-created txs from the persistent
                // outbox before the periodic relay snapshot. This covers the case
                // where a tx left mempool in a stale block and the GUI is idle.
                if let Ok(report) = reconcile_pending_txs(&settings, &mut local) {
                    if report.reaccepted > 0 {
                        let _ = save_chain(&settings, &local);
                    }
                }
                let mut txs = local
                    .mempool
                    .iter()
                    .filter(|tx| hf106_jin_sale_standardness_policy(tx, &settings).is_ok())
                    .cloned()
                    .collect::<Vec<_>>();
                // HF107/v1.6.9: pool shares and zero-fee protocol markers need
                // fast relay or pool membership appears to vanish before a block
                // sees it. Do not rebroadcast non-standard huge JIN sale buys;
                // otherwise the hot-potato tx re-enters every miner's mempool.
                txs.sort_by_key(|tx| {
                    (
                        mempool_template_priority(&settings, tx),
                        tx.txid().to_string(),
                    )
                });
                txs.into_iter().take(96).collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            drop(local);
            for tx in relay_after_merge
                .into_iter()
                .chain(mempool_for_periodic.into_iter())
            {
                let _ = relay_tx_to_known_peers(&settings, &tx, None);
            }
        }
        // HF88/v1.6.2: outbound loop is the always-on lifeline. Keep a bounded
        // catch-up heartbeat running even if the GUI is idle or inbound bind is
        // unavailable. This is detached from UI snapshots.
        if last_light_catchup.elapsed() >= Duration::from_secs(18) {
            last_light_catchup = Instant::now();
            let _ = hf90_auto_catchup(&settings, 120_000);
        }

        let local = chain.lock().expect("chain mutex poisoned");
        println!(
            "p2p status height={} tip={} mempool={} peers={} active={}",
            local.height(),
            local.tip_hash(),
            local.mempool.len(),
            peers.lock().expect("peer mutex poisoned").len(),
            active.lock().expect("active peer mutex poisoned").len()
        );
        drop(local);
        let sleep_secs = settings.p2p.connect_interval_secs.max(1).min(3);
        thread::sleep(Duration::from_secs(sleep_secs));
    }
}

pub fn sync_chain_once(settings: &Settings) -> Result<P2PSyncReport> {
    // HF74/v1.5.8 fixed2: keep CLI callers on the tiered official sync path.
    // If the local chain is on a valid-but-noncanonical branch at the same/near
    // height, tail/suffix sync cannot append. In that case, allow canonical
    // snapshot repair instead of falling into long peer loops.
    let mut report = hf90_manual_catchup(settings, 120_000).unwrap_or_default();
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
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
        while Instant::now() < deadline {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(msg) => process_client_message(settings, &addr, msg, &mut stream, &mut report)?,
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(1_000));
    let peer_budget = max_peers.max(1).saturating_mul(2).min(32);
    let peers = prioritized_outbound_peers(settings, peer_budget)
        .unwrap_or_else(|_| known_peers(settings).unwrap_or_default());
    for addr in peers.into_iter().take(peer_budget) {
        if Instant::now() >= deadline {
            break;
        }
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        match sync_peer_adaptive_session(settings, &addr, deadline, &mut report) {
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

fn sync_peer_adaptive_session(
    settings: &Settings,
    addr: &str,
    deadline: Instant,
    report: &mut P2PSyncReport,
) -> Result<()> {
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
    let _ = send_wire(&mut stream, &WireMessage::GetChain { from_height });

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
                        report.best_peer_height = report.best_peer_height.max(*height);
                    }
                    _ => {}
                }
                process_client_message(settings, addr, msg, &mut stream, report)?;

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

fn official_snapshot_peer_candidates(
    settings: &Settings,
    timeout_ms: u64,
) -> Vec<(String, u32, String, u128)> {
    let mut out: Vec<(String, u32, String, u128)> = Vec::new();
    for addr in official_snapshot_peers(settings).into_iter().take(4) {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let started = Instant::now();
        match probe_peer(settings, &addr, Duration::from_millis(timeout_ms.max(150))) {
            Ok(info) => out.push((
                addr,
                info.height,
                info.tip_hash,
                started.elapsed().as_millis(),
            )),
            Err(_) => out.push((addr, 0, String::new(), started.elapsed().as_millis())),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.3.cmp(&b.3)).then(a.0.cmp(&b.0)));
    out
}

fn best_official_peer_tip(settings: &Settings, timeout_ms: u64) -> Result<(usize, u32, String)> {
    let candidates = official_snapshot_peer_candidates(settings, timeout_ms);
    let contacted = candidates
        .iter()
        .filter(|(_, height, _, _)| *height > 0)
        .count();
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    for (_, height, tip, _) in candidates {
        if height >= best_height {
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
    if settings.network.name != "mainnet" {
        return false;
    }
    let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(700).min(2_500))
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
    if settings.network.name != "mainnet" {
        return false;
    }
    let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(700).min(2_500))
    else {
        return false;
    };
    height == parent_height && !tip.trim().is_empty() && tip == parent_hash
}

fn hf115_http_exact_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    if !hf115_http_tip_acknowledges_local(settings, local, timeout_ms) {
        return false;
    }
    let local_tip = local.tip_hash().to_string();
    let (contacted, quorum_ahead_height, quorum_conflicts) = direct_parent_view(
        settings,
        local.height(),
        &local_tip,
        settings.p2p.max_outbound_peers.max(6).min(12),
        timeout_ms.max(220).min(700),
    )
    .unwrap_or((0, 0, Vec::new()));
    report.peers_contacted = report.peers_contacted.max(contacted);
    report.best_peer_height = report.best_peer_height.max(quorum_ahead_height);
    if quorum_ahead_height > local.height() || !quorum_conflicts.is_empty() {
        return false;
    }
    mark_fresh_tip_trusted(settings, local);
    true
}

fn official_tip_summary(
    settings: &Settings,
    report: &mut P2PSyncReport,
    timeout_ms: u64,
) -> (usize, u32, String) {
    let (contacted, height, tip) =
        best_official_peer_tip(settings, timeout_ms).unwrap_or((0, 0, String::new()));
    report.peers_contacted = report.peers_contacted.max(contacted);
    report.best_peer_height = report.best_peer_height.max(height);
    (contacted, height, tip)
}

fn official_tip_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
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
        return true;
    }
    false
}

fn official_http_tip_greenlight(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &ChainState,
    timeout_ms: u64,
) -> bool {
    if let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(700)) {
        report.peers_contacted = report.peers_contacted.max(1);
        report.best_peer_height = report.best_peer_height.max(height);
        let ok = if settings.network.name == "mainnet" {
            hf114_official_tip_acknowledges_local(settings, local, height, &tip)
        } else {
            official_tip_compatible_with_local(local, height, &tip)
        };
        if ok {
            mark_fresh_tip_trusted(settings, local);
            return true;
        }
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
    if settings.network.name != "mainnet" {
        return official_tip_greenlight(settings, report, local, timeout_ms)
            || official_http_tip_greenlight(settings, report, local, timeout_ms);
    }

    let mut direct_contacted = 0usize;
    let mut contacted_total = 0usize;
    let mut best_height = 0u32;
    let mut best_tip = String::new();

    if let Ok((contacted, height, tip)) = best_official_peer_tip(settings, timeout_ms.max(650)) {
        if contacted > 0 {
            direct_contacted = contacted;
            contacted_total = contacted_total.saturating_add(contacted);
            if height > best_height || (height == best_height && !tip.trim().is_empty()) {
                best_height = height;
                best_tip = tip;
            }
        }
    }

    if let Ok(Some((height, tip))) = official_http_tip(settings, timeout_ms.max(1_500)) {
        contacted_total = contacted_total.saturating_add(1);
        if height > best_height || (height == best_height && !tip.trim().is_empty()) {
            best_height = height;
            best_tip = tip;
        }
    }

    report.peers_contacted = report.peers_contacted.max(contacted_total);
    report.best_peer_height = report.best_peer_height.max(best_height);

    // HF107: for mainnet mining, HTTP-only is not enough to green-light. HTTP
    // snapshots are excellent for catch-up, but the actual mining OK must see at
    // least one official/direct TCP seed sample so a stale published tip cannot
    // green-light an old local parent while the live seed already moved ahead.
    if direct_contacted == 0 || best_height == 0 {
        // HF116: exact official HTTP/public-snapshot acknowledgement is enough to
        // keep mainnet alive when TCP seeds are unreachable. HF114 rejected this
        // and could freeze every honest miner at the public tip until a seed or
        // two exact peers came back. This remains exact-tip only and is vetoed by
        // a two-peer direct quorum above/a conflicting same-height quorum.
        if hf115_http_exact_greenlight(settings, report, local, timeout_ms.max(900)) {
            return true;
        }
        // HF113: if all official/direct seeds are temporarily unreachable, keep
        // the network alive only when a directly reachable peer quorum agrees
        // with our validated local tip. This avoids seed dependency without
        // trusting one random future/ghost peer.
        if hf113_peer_quorum_greenlight(settings, local, 12, timeout_ms.max(420)) {
            mark_fresh_tip_trusted(settings, local);
            return true;
        }
        return false;
    }
    // HF114: exact mainnet acknowledgement only. HF113 still allowed
    // "official tip is an ancestor of my longer local branch", which is the
    // all-self-mined stale mode users were seeing. If local is ahead of the
    // official/direct tip, do not hash on that suffix; the repair/re-anchor path
    // will roll back or replace it before mining resumes.
    if !hf114_official_tip_acknowledges_local(settings, local, best_height, &best_tip) {
        // HF116: if an official TCP seed is reachable but stale/lower, do not let
        // it freeze the network after the next public block. Exact HTTP/public
        // snapshot acknowledgement wins, and if the published snapshot also lags,
        // two direct peers acknowledging our exact local tip are enough to keep
        // mining alive while still rejecting two-peer ahead/conflict quorums.
        if hf115_http_exact_greenlight(settings, report, local, timeout_ms.max(900)) {
            return true;
        }
        if best_height > 0
            && best_height < local.height()
            && hf113_peer_quorum_greenlight(settings, local, 12, timeout_ms.max(420))
        {
            mark_fresh_tip_trusted(settings, local);
            return true;
        }
        return false;
    }

    mark_fresh_tip_trusted(settings, local);
    true
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
        return hf104_canonical_greenlight(settings, &mut report, local, timeout_ms.max(1_200));
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
    let mut contacted = 0usize;
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    let deadline = Instant::now()
        + Duration::from_millis(
            timeout_ms
                .max(150)
                .saturating_mul(max_peers.max(1) as u64)
                .min(4_000),
        );
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
            left.min(Duration::from_millis(timeout_ms.max(120))),
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
    local: &ChainState,
    max_peers: usize,
    timeout_ms: u64,
) -> bool {
    let Ok(peers) = prioritized_outbound_peers(settings, max_peers.max(3).min(16)) else {
        return false;
    };
    let mut contacted = 0usize;
    let mut compatible = 0usize;
    let mut same_tip = 0usize;
    let mut ahead_counts: HashMap<(u32, String), usize> = HashMap::new();
    let mut conflict_counts: HashMap<String, usize> = HashMap::new();
    let local_height = local.height();
    let local_tip = local.tip_hash().to_string();
    let per_peer = Duration::from_millis(timeout_ms.max(120).min(700));

    for addr in peers.into_iter() {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let Ok(info) = probe_peer(settings, &addr, per_peer) else {
            continue;
        };
        if info.height == 0 {
            continue;
        }
        contacted = contacted.saturating_add(1);
        if info.height > local_height {
            *ahead_counts
                .entry((info.height, info.tip_hash.clone()))
                .or_insert(0) += 1;
            continue;
        }
        if info.height == local_height
            && !info.tip_hash.trim().is_empty()
            && info.tip_hash != local_tip
        {
            *conflict_counts.entry(info.tip_hash.clone()).or_insert(0) += 1;
            continue;
        }
        if local_is_at_or_past_tip(local, info.height, &info.tip_hash) {
            compatible = compatible.saturating_add(1);
            if info.height == local_height
                && !info.tip_hash.trim().is_empty()
                && info.tip_hash == local_tip
            {
                same_tip = same_tip.saturating_add(1);
            }
        }
    }

    // Two directly reachable peers agreeing on a future tip means we should not
    // green-light the old parent. One peer alone is not enough; it could be a
    // stale/private/future telemetry row.
    if ahead_counts.values().any(|count| *count >= 2) {
        return false;
    }
    if conflict_counts.values().any(|count| *count >= 2) {
        return false;
    }

    // HF114: fallback green-light also needs two peers to acknowledge the exact
    // current tip. Compatible older ancestors are telemetry only; using them as OK
    // made local-ahead self-mined branches look canonical during seed outages.
    let _ = contacted;
    let _ = compatible;
    same_tip >= 2
}

fn hf115_direct_parent_ack_quorum(
    settings: &Settings,
    parent_height: u32,
    parent_hash: &str,
    max_peers: usize,
    timeout_ms: u64,
) -> bool {
    if settings.network.name != "mainnet" {
        return false;
    }
    let Ok(peers) = prioritized_outbound_peers(settings, max_peers.max(3).min(16)) else {
        return false;
    };
    let mut same_parent = 0usize;
    let mut ahead_counts: HashMap<(u32, String), usize> = HashMap::new();
    let mut conflict_counts: HashMap<String, usize> = HashMap::new();
    let per_peer = Duration::from_millis(timeout_ms.max(120).min(700));
    for addr in peers.into_iter() {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let Ok(info) = probe_peer(settings, &addr, per_peer) else {
            continue;
        };
        if info.height == 0 {
            continue;
        }
        if info.height > parent_height {
            *ahead_counts
                .entry((info.height, info.tip_hash.clone()))
                .or_insert(0) += 1;
        } else if info.height == parent_height && !info.tip_hash.trim().is_empty() {
            if info.tip_hash == parent_hash {
                same_parent = same_parent.saturating_add(1);
            } else {
                *conflict_counts.entry(info.tip_hash.clone()).or_insert(0) += 1;
            }
        }
    }
    if ahead_counts.values().any(|count| *count >= 2) {
        return false;
    }
    if conflict_counts.values().any(|count| *count >= 2) {
        return false;
    }
    same_parent >= 2
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
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
        return None;
    }
    let parent_hash_s = parent_hash.to_string();

    if let Ok((contacted, official_h, official_tip)) =
        best_official_peer_tip(settings, timeout_ms.max(180).min(900))
    {
        if contacted > 0 {
            if official_h > parent_height {
                return Some(format!(
                    "official seed moved to #{} while candidate parent is #{}",
                    official_h, parent_height
                ));
            }
            if settings.network.name == "mainnet" && official_h < parent_height {
                if hf115_http_tip_acknowledges_parent(
                    settings,
                    parent_height,
                    &parent_hash_s,
                    timeout_ms.max(900),
                ) || hf115_direct_parent_ack_quorum(
                    settings,
                    parent_height,
                    &parent_hash_s,
                    12,
                    timeout_ms.max(420),
                ) {
                    return None;
                }
                return Some(format!("candidate parent #{} is ahead of official seed tip #{}; waiting for HF116 canonical acknowledgement/re-anchor", parent_height, official_h));
            }
            if official_h == parent_height {
                if official_tip.trim().is_empty() && settings.network.name == "mainnet" {
                    if hf115_http_tip_acknowledges_parent(
                        settings,
                        parent_height,
                        &parent_hash_s,
                        timeout_ms.max(900),
                    ) {
                        return None;
                    }
                    return Some(format!(
                        "official seed did not acknowledge candidate parent hash at #{}",
                        parent_height
                    ));
                }
                if !official_tip.trim().is_empty() && official_tip != parent_hash_s {
                    if hf115_http_tip_acknowledges_parent(
                        settings,
                        parent_height,
                        &parent_hash_s,
                        timeout_ms.max(900),
                    ) {
                        return None;
                    }
                    return Some(format!(
                        "official seed reports a different hash at #{}",
                        parent_height
                    ));
                }
            }
            return None;
        }
    }

    // If no official seed could be sampled, use direct peer quorum only. A single
    // high/future peer is ignored so the old orange/future-tip problem cannot
    // stop otherwise healthy miners. HF114 additionally requires two peers to
    // acknowledge the exact candidate parent before mainnet miners keep hashing
    // during a seed outage; older compatible ancestors are not enough.
    let Ok(peers) = prioritized_outbound_peers(settings, 12) else {
        return None;
    };
    let mut contacted = 0usize;
    let mut same_parent = 0usize;
    let mut ahead_counts: HashMap<(u32, String), usize> = HashMap::new();
    let mut conflict_counts: HashMap<String, usize> = HashMap::new();
    let per_peer = Duration::from_millis(timeout_ms.max(120).min(550));
    for addr in peers.into_iter() {
        let Ok(info) = probe_peer(settings, &addr, per_peer) else {
            continue;
        };
        if info.height == 0 {
            continue;
        }
        contacted = contacted.saturating_add(1);
        if info.height > parent_height {
            *ahead_counts
                .entry((info.height, info.tip_hash.clone()))
                .or_insert(0) += 1;
        } else if info.height == parent_height && !info.tip_hash.trim().is_empty() {
            if info.tip_hash == parent_hash_s {
                same_parent = same_parent.saturating_add(1);
            } else {
                *conflict_counts.entry(info.tip_hash.clone()).or_insert(0) += 1;
            }
        }
    }
    if let Some(((height, _), count)) = ahead_counts.iter().find(|(_, count)| **count >= 2) {
        return Some(format!(
            "direct peer quorum ({count}) moved to #{} while candidate parent is #{}",
            height, parent_height
        ));
    }
    if let Some((_, count)) = conflict_counts.iter().find(|(_, count)| **count >= 2) {
        return Some(format!(
            "direct peer quorum ({count}) disagrees at candidate parent #{}",
            parent_height
        ));
    }
    if settings.network.name == "mainnet" && contacted >= 2 && same_parent < 2 {
        if hf115_http_tip_acknowledges_parent(
            settings,
            parent_height,
            &parent_hash_s,
            timeout_ms.max(900),
        ) {
            return None;
        }
        return Some(format!("candidate parent #{} is not acknowledged by two direct peers; preventing local-stale self-mining", parent_height));
    }
    None
}
fn best_known_live_tip(
    settings: &Settings,
    report: &mut P2PSyncReport,
    timeout_ms: u64,
) -> (usize, u32, String) {
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
    report.best_peer_height = report.best_peer_height.max(direct_height);
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
    let mut contacted = 0usize;
    let mut best_height = 0u32;
    let mut best_tip = String::new();
    if let Ok((c, h, tip)) = best_official_peer_tip(settings, timeout_ms.max(150)) {
        contacted = contacted.saturating_add(c);
        if h > best_height || (h == best_height && !tip.trim().is_empty()) {
            best_height = h;
            best_tip = tip;
        }
    }
    if let Ok(Some((h, tip))) = official_http_tip(settings, timeout_ms.max(700)) {
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
        if let Ok((c, h, tip)) = best_reachable_peer_tip(settings, 8, timeout_ms.max(150)) {
            contacted = contacted.saturating_add(c);
            if h > best_height || (h == best_height && !tip.trim().is_empty()) {
                best_height = h;
                best_tip = tip;
            }
        }
    }
    (contacted, best_height, best_tip)
}

fn hf82_catchup_gate() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn remaining_ms(deadline: Instant) -> u64 {
    deadline
        .saturating_duration_since(Instant::now())
        .as_millis()
        .min(u64::MAX as u128) as u64
}

fn hf82_catchup_impl(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_full_snapshot: bool,
) -> Result<P2PSyncReport> {
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    // Single-flight guard: do not let startup refresh, wallet sync, miner green-light,
    // pool worker, tx builder and embedded node all run chain repair at once. If a
    // catch-up is already active, return the current local view immediately; the
    // active worker will update chain.json and the next UI tick will observe it.
    let _guard = match hf82_catchup_gate().try_lock() {
        Ok(guard) => guard,
        Err(_) => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.clamp(1_500, 90_000));
    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    report.height = local.height();
    report.tip_hash = local.tip_hash().to_string();

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
            report.best_peer_height = report.best_peer_height.max(http_h);
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
    hf82_catchup_impl(
        settings,
        total_timeout_ms.min(HF82_AUTO_CATCHUP_MS).max(1_500),
        false,
    )
}

pub fn hf82_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf82_catchup_impl(
        settings,
        total_timeout_ms.min(HF82_MINING_CATCHUP_MS).max(2_500),
        false,
    )
}

pub fn hf82_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf82_catchup_impl(settings, total_timeout_ms.max(8_000), true)
}

fn hf85_catchup_pulse(
    settings: &Settings,
    total_timeout_ms: u64,
    allow_full_snapshot: bool,
) -> Result<P2PSyncReport> {
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.clamp(1_500, 60_000));

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
    // HF88: background auto pulses are intentionally append/tail only. Full
    // snapshot repair is reserved for manual Sync/Repair or mining workers so
    // the GUI cannot starve its first local wallet/block snapshot.
    hf85_catchup_pulse(settings, total_timeout_ms.min(10_000).max(1_500), false)
}

pub fn hf85_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    // Worker-thread mining green-light can also use the canonical HTTP snapshot
    // fallback to avoid infinite one/few-block-behind loops.
    hf85_catchup_pulse(settings, total_timeout_ms.min(16_000).max(2_500), true)
}

pub fn hf85_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf85_catchup_pulse(settings, total_timeout_ms.max(8_000), true)
}

/// HF88/v1.6.2: detached GUI catch-up is still bounded, but it may use the
/// official HTTP snapshot fallback when suffix/tail sync does not move a stale
/// local chain. This runs off the UI thread and is single-purpose: make nodes
/// that were left open overnight catch up again without manual babysitting.
pub fn hf86_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    let mut report = hf85_catchup_pulse(settings, total_timeout_ms.min(12_000).max(2_000), false)?;
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
        report.best_peer_height = report.best_peer_height.max(official_h);
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
    let mut report = hf85_manual_catchup(settings, total_timeout_ms.max(12_000))?;
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
        report.best_peer_height = report.best_peer_height.max(official_h);
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
    let mut report = hf85_mining_catchup(settings, total_timeout_ms.max(8_000))?;
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
        report.best_peer_height = report.best_peer_height.max(official_h);
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
    let mut best_h = 0u32;
    let mut best_tip = String::new();
    if let Ok(Some((h, tip))) = official_http_tip(settings, timeout_ms.min(2_000).max(700)) {
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        report.best_peer_height = report.best_peer_height.max(h);
        best_h = h;
        best_tip = tip;
    }
    if let Ok((contacted, h, tip)) = best_official_peer_tip(settings, timeout_ms.min(900).max(250))
    {
        report.peers_contacted = report.peers_contacted.saturating_add(contacted);
        report.best_peer_height = report.best_peer_height.max(h);
        if h > best_h || (h == best_h && !tip.trim().is_empty()) {
            best_h = h;
            best_tip = tip;
        }
    }
    // HF107: for canonical catch-up/mining, do not let a random reachable peer
    // outrank official seed/HTTP tips. On mainnet, if official sources are absent,
    // return no canonical target and let mining wait; peer tips remain telemetry.
    if best_h == 0 && settings.network.name != "mainnet" {
        if let Ok((contacted, h, tip)) =
            best_reachable_peer_tip(settings, 6, timeout_ms.min(700).max(220))
        {
            report.peers_contacted = report.peers_contacted.saturating_add(contacted);
            report.best_peer_height = report.best_peer_height.max(h);
            if h > best_h || (h == best_h && !tip.trim().is_empty()) {
                best_h = h;
                best_tip = tip;
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    // HF107/v1.6.9: one catch-up writer at a time across GUI, embedded P2P,
    // mining, transaction workers and manual Sync. The UI remains live; extra
    // callers get the current local view while the active writer advances disk.
    let _guard = match hf82_catchup_gate().try_lock() {
        Ok(guard) => guard,
        Err(_) => return finish_report(settings, report),
    };

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

    let before = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &before.blocks)?;
    let mut local_h = before.height();
    let mut local_tip = before.tip_hash().to_string();
    report.height = local_h;
    report.tip_hash = local_tip.clone();

    let (mut best_h, mut best_tip) = hf88_best_tip(settings, &mut report, 2_500);
    let same_height_wrong_tip =
        best_h == local_h && !best_tip.trim().is_empty() && best_tip != local_tip;
    let should_catch = best_h == 0
        || best_h > local_h
        || same_height_wrong_tip
        || !fresh_tip_still_matches_light_network(settings, &before, 1_200);
    if !should_catch {
        mark_fresh_tip_trusted(settings, &before);
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
            if snapshot_step && left < 6_000 {
                continue;
            }
            if full_http_step && left < 12_000 {
                continue;
            }

            let step_budget = left.min(*max_ms).max(if full_http_step {
                16_000
            } else if snapshot_step {
                12_000
            } else {
                3_000
            });
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

            let (new_best_h, new_best_tip) = hf88_best_tip(settings, &mut report, 1_100);
            if new_best_h > best_h || (new_best_h == best_h && !new_best_tip.trim().is_empty()) {
                best_h = new_best_h;
                best_tip = new_best_tip;
            }
            if best_h > 0 && local_is_at_or_past_tip(&after, best_h, &best_tip) {
                mark_fresh_tip_trusted(settings, &after);
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
                if let Ok(full) = sync_official_http_snapshot(
                    settings,
                    if allow_snapshot { 120_000 } else { 90_000 },
                ) {
                    merge_sync_reports(&mut report, full);
                }
            }
        }
    }

    if let Ok(final_chain) = load_chain_for_hf90_catchup(settings) {
        report.height = final_chain.height();
        report.tip_hash = final_chain.tip_hash().to_string();
        if hf104_canonical_greenlight(settings, &mut report, &final_chain, 1_200)
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
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(150_000), false)
}

pub fn hf88_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(180_000), true)
}

pub fn hf88_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf88_catchup_ladder(settings, total_timeout_ms.max(90_000).min(180_000), true)
}

pub fn hf90_auto_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf90_catchup_ladder(
        settings,
        total_timeout_ms.clamp(60_000, 600_000),
        true,
        "auto",
    )
}

pub fn hf90_manual_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
    hf90_catchup_ladder(
        settings,
        total_timeout_ms.clamp(90_000, 720_000),
        true,
        "manual",
    )
}

pub fn hf90_mining_catchup(settings: &Settings, total_timeout_ms: u64) -> Result<P2PSyncReport> {
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    // HF111/v1.7.1: this is the heavy but safe repair used by manual Repair and
    // GUI auto-heal when a miner has been behind for too long. It is official-
    // source only: first the fresh HTTP tail/snapshot, then official direct full
    // chain, then suffix/overlap repair. All adopted blocks are consensus-
    // validated by the lower-level repair functions before chain.json is saved.
    let _guard = match hf82_catchup_gate().try_lock() {
        Ok(guard) => guard,
        Err(_) => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.clamp(180_000, 900_000));

    for round in 0..3usize {
        if remaining_ms(deadline) < 5_000 {
            break;
        }
        let local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();

        if hf104_canonical_greenlight(settings, &mut report, &local, 1_500) {
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
            let budget = left.min(*max_ms).max(8_000);
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
            if hf104_canonical_greenlight(settings, &mut report, &after, 1_500) {
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
    _reason: &str,
) -> Result<P2PSyncReport> {
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    // HF107/v1.6.9: one strong catch-up writer at a time inside the process.
    // GUI, embedded P2P, mining and manual Sync may all ask for repair; only
    // one should write chain.json while the others keep showing local state.
    let _guard = match hf82_catchup_gate().try_lock() {
        Ok(guard) => guard,
        Err(_) => return finish_report(settings, report),
    };

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(30_000));
    let mut no_progress_rounds = 0usize;

    for round in 0..4usize {
        if remaining_ms(deadline) < 3_000 {
            break;
        }
        let current = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &current.blocks)?;
        report.height = current.height();
        report.tip_hash = current.tip_hash().to_string();

        let (mut best_h, mut best_tip) = hf88_best_tip(settings, &mut report, 2_800);
        if let Ok((contacted, h, tip)) = best_official_peer_tip(settings, 2_800) {
            report.peers_contacted = report.peers_contacted.saturating_add(contacted);
            report.best_peer_height = report.best_peer_height.max(h);
            if h > best_h || (h == best_h && !tip.trim().is_empty()) {
                best_h = h;
                best_tip = tip;
            }
        }

        let wrong_tip = best_h == current.height()
            && !best_tip.trim().is_empty()
            && best_tip != current.tip_hash().to_string();
        if best_h > 0 && local_is_at_or_past_tip(&current, best_h, &best_tip) && !wrong_tip {
            mark_fresh_tip_trusted(settings, &current);
            return finish_report(settings, report);
        }
        if best_h == 0 && fresh_tip_still_matches_light_network(settings, &current, 900) {
            mark_fresh_tip_trusted(settings, &current);
            return finish_report(settings, report);
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
            let full_http_step = *step == "official-http-full-snapshot";
            let direct_full_step = *step == "official-direct-full-chain";
            let step_budget = left.min(*max_ms).max(if direct_full_step {
                18_000
            } else if full_http_step {
                16_000
            } else {
                2_500
            });
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
                let (fresh_h, fresh_tip) = hf88_best_tip(settings, &mut report, 1_800);
                let target_h = fresh_h.max(best_h);
                let target_tip = if fresh_h >= best_h && !fresh_tip.trim().is_empty() {
                    fresh_tip
                } else {
                    best_tip.clone()
                };
                if target_h == 0 || local_is_at_or_past_tip(&after_step, target_h, &target_tip) {
                    mark_fresh_tip_trusted(settings, &after_step);
                    return finish_report(settings, report);
                }
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

    let (_seen, visible_h, visible_tip) = hf97_visible_tip(settings, 1_200);
    if visible_h > 0 {
        if let Ok(cur) = load_chain_for_hf90_catchup(settings) {
            if !local_is_at_or_past_tip(&cur, visible_h, &visible_tip) {
                if let Ok(full) = sync_official_http_snapshot(settings, 150_000) {
                    merge_sync_reports(&mut report, full);
                }
                if let Ok(cur_after_http) = load_chain_for_hf90_catchup(settings) {
                    if !local_is_at_or_past_tip(&cur_after_http, visible_h, &visible_tip) {
                        if let Ok(full_direct) = sync_official_direct_full_chain(settings, 180_000)
                        {
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
    report.best_peer_height = report.best_peer_height.max(visible_h);
    if visible_h > final_chain.height()
        && !local_is_at_or_past_tip(&final_chain, visible_h, &visible_tip)
    {
        mark_hf97_uncatchable_tip(settings, &final_chain, visible_h);
    }
    if fresh_tip_still_matches_light_network(settings, &final_chain, 1_200) {
        mark_fresh_tip_trusted(settings, &final_chain);
    }
    finish_report(settings, report)
}

fn hf80_fast_official_catchup(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &mut ChainState,
    total_timeout_ms: u64,
) -> Result<()> {
    if !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(1_500));
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
        let _ = stream.set_read_timeout(Some(Duration::from_millis(3_200)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(3_500)));
        let mut reader = BufReader::new(stream.try_clone()?);

        let local_before = load_chain_for_hf90_catchup(settings)?;
        let before_height = local_before.height();
        let before_tip = local_before.tip_hash().to_string();
        let from_height = before_height.saturating_add(1);

        let _ = send_version(&mut stream, settings, &local_before);
        let _ = send_wire(&mut stream, &WireMessage::GetHeaders { from_height });
        let _ = send_wire(&mut stream, &WireMessage::GetChain { from_height });

        let peer_deadline = Instant::now() + left.min(Duration::from_millis(16_000));
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
                    report.best_peer_height = report.best_peer_height.max(height);
                    if height >= report.best_peer_height {
                        peer_tip = tip_hash;
                    }
                }
                Ok(WireMessage::Headers { headers }) => {
                    if let Some(last) = headers.last() {
                        report.best_peer_height = report.best_peer_height.max(last.height);
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
                            true,
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
    let timeout_secs = ((timeout_ms.max(1_000) + 999) / 1000).max(2).to_string();
    let mut command = Command::new(curl_binary());
    hide_snapshot_command_window(&mut command);
    let status = command
        .arg("-fsSL")
        .arg("--connect-timeout")
        .arg("5")
        .arg("--max-time")
        .arg(timeout_secs)
        .arg("--output")
        .arg(path)
        .arg(url)
        .status()
        .with_context(|| format!("failed to start curl for {url}"))?;
    if !status.success() {
        bail!("snapshot download failed from {url} with status {status}");
    }
    Ok(())
}

fn download_http_text(url: &str, timeout_ms: u64, label: &str) -> Result<String> {
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

pub fn official_http_tip(settings: &Settings, timeout_ms: u64) -> Result<Option<(u32, String)>> {
    for url in official_http_tip_urls(settings) {
        let raw = match download_http_text(&url, timeout_ms.min(6_000).max(1_500), "official-tip") {
            Ok(raw) => raw,
            Err(_) => continue,
        };
        let tip: OfficialTipSnapshot = serde_json::from_str(&raw)
            .with_context(|| format!("invalid official tip json from {url}"))?;
        if tip.network != settings.network.name {
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
        repaired.mempool.clear();
        let mut connected = 0usize;
        for block in persisted.blocks.into_iter().skip(local_height as usize + 1) {
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
        repaired.mempool.clear();
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

    let (_, mut official_height, mut official_tip) =
        official_tip_summary(settings, &mut report, 420);
    if let Ok(Some((http_height, http_tip))) = official_http_tip(settings, 900) {
        report.peers_contacted = report.peers_contacted.max(1);
        report.best_peer_height = report.best_peer_height.max(http_height);
        if http_height >= official_height || official_tip.trim().is_empty() {
            official_height = http_height;
            official_tip = http_tip;
        }
    }

    let same_height_conflict = official_height == local_height_before
        && !official_tip.trim().is_empty()
        && official_tip != local_tip_before;
    let local_ahead = hf114_official_tip_is_local_ancestor(
        settings,
        &local_before,
        official_height,
        &official_tip,
    );
    if !same_height_conflict && !local_ahead {
        report.height = local_height_before;
        report.tip_hash = local_tip_before;
        return finish_report(settings, report);
    }

    urls.sort_by_key(|url| official_tail_window_from_url(url));
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(2_000));
    for url in urls {
        if Instant::now() >= deadline {
            break;
        }
        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let per_url_timeout = left_ms.min(8_000).max(1_500);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let raw = match download_http_text(&url, per_url_timeout, "official-tail-hf114-reanchor") {
            Ok(raw) => raw,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        let tail: OfficialTailSnapshot = match serde_json::from_str(&raw) {
            Ok(tail) => tail,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        report.best_peer_height = report.best_peer_height.max(tail.tip_height);
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
        if candidate.height() < local_height_before
            || (candidate.height() == local_height_before
                && candidate.tip_hash().to_string() != local_tip_before)
        {
            save_chain(settings, &candidate)?;
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
        if let Err(err) = download_http_snapshot_to(&tmp, &url, total_timeout_ms.max(8_000)) {
            report.peer_errors = report.peer_errors.saturating_add(1);
            let _ = fs::remove_file(&tmp);
            if !err.to_string().contains("404") { /* try next official URL */ }
            continue;
        }

        let raw = fs::read_to_string(&tmp)
            .with_context(|| format!("failed reading HF114 re-anchor snapshot from {url}"))?;
        let persisted: PersistedChainState = serde_json::from_str(&raw)
            .with_context(|| format!("invalid HF114 re-anchor snapshot json from {url}"))?;
        let (mut candidate, connected_hint) =
            apply_official_snapshot_fast_path(settings, &local_before, persisted).with_context(
                || format!("HF114 re-anchor snapshot failed consensus replay from {url}"),
            )?;

        validate_chain_consensus_checkpoints(settings, &candidate.blocks)?;
        report.best_peer_height = report.best_peer_height.max(candidate.height());
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
            save_chain(settings, &candidate)?;
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
    let mut report = P2PSyncReport::default();
    let mut urls = official_http_tail_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }

    let local_before = load_chain_for_hf90_catchup(settings)?;
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

    for url in urls {
        if Instant::now() >= deadline {
            break;
        }
        let left_ms = deadline
            .saturating_duration_since(Instant::now())
            .as_millis()
            .min(u64::MAX as u128) as u64;
        let per_url_timeout = left_ms.min(8_000).max(1_500);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        let raw = match download_http_text(&url, per_url_timeout, "official-tail") {
            Ok(raw) => raw,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        let tail: OfficialTailSnapshot = match serde_json::from_str(&raw) {
            Ok(tail) => tail,
            Err(_) => {
                report.peer_errors = report.peer_errors.saturating_add(1);
                continue;
            }
        };
        report.best_peer_height = report.best_peer_height.max(tail.tip_height);
        let (mut candidate, connected_hint) =
            match apply_official_tail_snapshot(settings, &local_before, tail) {
                Ok(v) => v,
                Err(_) => {
                    report.peer_errors = report.peer_errors.saturating_add(1);
                    continue;
                }
            };
        if candidate.height() > local_height_before
            || (candidate.height() == local_height_before
                && candidate.tip_hash().to_string() != local_tip_before)
        {
            let keep_mempool = local_before.reorg_mempool_candidates_for(&candidate);
            candidate.rebuild_mempool_from(keep_mempool, settings);
            save_chain(settings, &candidate)?;
            mark_fresh_tip_trusted(settings, &candidate);
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report.blocks_connected.saturating_add(
                connected_hint.max(candidate.height().saturating_sub(local_height_before) as usize),
            );
            report.height = candidate.height();
            report.tip_hash = candidate.tip_hash().to_string();
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
    let mut report = P2PSyncReport::default();
    let urls = official_http_snapshot_urls(settings);
    if urls.is_empty() {
        return finish_report(settings, report);
    }

    let local_before = load_chain_for_hf90_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    let mut tmp = std::env::temp_dir();
    tmp.push(format!(
        "qub-{}-official-snapshot-{}.json",
        settings.network.name,
        std::process::id()
    ));

    for url in urls {
        let _ = fs::remove_file(&tmp);
        report.peers_contacted = report.peers_contacted.saturating_add(1);
        if let Err(err) = download_http_snapshot_to(&tmp, &url, total_timeout_ms) {
            report.peer_errors = report.peer_errors.saturating_add(1);
            let _ = fs::remove_file(&tmp);
            if !err.to_string().contains("404") { /* try next official URL */ }
            continue;
        }

        let raw = fs::read_to_string(&tmp)
            .with_context(|| format!("failed reading downloaded snapshot from {url}"))?;
        let persisted: PersistedChainState = serde_json::from_str(&raw)
            .with_context(|| format!("invalid snapshot json from {url}"))?;
        let (mut candidate, connected_hint) =
            apply_official_snapshot_fast_path(settings, &local_before, persisted)
                .with_context(|| format!("snapshot failed consensus repair from {url}"))?;

        report.best_peer_height = report.best_peer_height.max(candidate.height());
        if candidate.height() > local_height_before
            || (candidate.height() == local_height_before
                && candidate.tip_hash().to_string() != local_tip_before)
        {
            let keep_mempool = local_before.reorg_mempool_candidates_for(&candidate);
            candidate.rebuild_mempool_from(keep_mempool, settings);
            save_chain(settings, &candidate)?;
            mark_fresh_tip_trusted(settings, &candidate);
            report.chains_adopted = report.chains_adopted.saturating_add(1);
            report.blocks_connected = report.blocks_connected.saturating_add(
                connected_hint.max(candidate.height().saturating_sub(local_height_before) as usize),
            );
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

/// HF107/v1.6.9: emergency canonical full-chain pull from official direct seeds.
fn sync_official_direct_full_chain(
    settings: &Settings,
    total_timeout_ms: u64,
) -> Result<P2PSyncReport> {
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || !matches!(settings.network.name.as_str(), "mainnet" | "testnet") {
        return finish_report(settings, report);
    }

    let local_before = load_chain_for_hf90_catchup(settings)?;
    let local_height_before = local_before.height();
    let local_tip_before = local_before.tip_hash().to_string();
    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(20_000));

    let mut candidates = official_snapshot_peer_candidates(settings, 1_500);
    if candidates.is_empty() {
        candidates = official_snapshot_peers(settings)
            .into_iter()
            .map(|addr| (addr, 0, String::new(), 0))
            .collect();
    }

    for (addr, candidate_height, candidate_tip, _) in candidates.into_iter().take(4) {
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
        report.best_peer_height = report.best_peer_height.max(candidate_height);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(18_000)));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(5_000)));
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
                    report.best_peer_height = report.best_peer_height.max(height);
                    last_rx = Instant::now();
                }
                Ok(WireMessage::Headers { headers }) => {
                    if let Some(last) = headers.last() {
                        if last.height >= peer_height {
                            peer_height = last.height;
                            peer_tip = last.hash.clone();
                        }
                        report.best_peer_height = report.best_peer_height.max(last.height);
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
                        report.best_peer_height = report
                            .best_peer_height
                            .max(candidate_height_now)
                            .max(peer_height);
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(4_000));
    for (addr, candidate_height, _candidate_tip, _latency_ms) in
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
        report.best_peer_height = report.best_peer_height.max(candidate_height);
        let _ = stream.set_read_timeout(Some(Duration::from_millis(
            OFFICIAL_SNAPSHOT_READ_TIMEOUT_MS,
        )));
        let _ = stream.set_write_timeout(Some(Duration::from_millis(2_000)));
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
                            report.best_peer_height = report.best_peer_height.max(height);
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
                                report.best_peer_height = report.best_peer_height.max(last.height);
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
                            let more_expected = blocks.len() == settings.p2p.max_blocks_per_message;
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
    let connect_left = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(3_500));
    if connect_left.is_zero() {
        return Ok(());
    }

    let mut stream = connect_peer(addr, connect_left)?;
    report.peers_contacted = report.peers_contacted.saturating_add(1);
    stream.set_read_timeout(Some(Duration::from_millis(
        FORCE_ANCHOR_SYNC_READ_TIMEOUT_MS,
    )))?;
    stream.set_write_timeout(Some(Duration::from_millis(5_000)))?;
    let mut reader = BufReader::new(stream.try_clone()?);

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
                        report.best_peer_height = report.best_peer_height.max(*height);
                    }
                    _ => {}
                }
                process_client_message(settings, addr, msg, &mut stream, report)?;

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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled {
        return finish_report(settings, report);
    }

    let deadline = Instant::now() + Duration::from_millis(total_timeout_ms.max(3_000));
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
            report.best_peer_height = report.best_peer_height.max(info.height);
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
        merged.best_peer_height = merged.best_peer_height.max(report.best_peer_height);
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
    into.best_peer_height = into.best_peer_height.max(other.best_peer_height);
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
    for from_height in adaptive_from_heights(local.height(), peer_height, peer_tip_differs)
        .into_iter()
        .take(10)
    {
        send_wire(stream, &WireMessage::GetHeaders { from_height })?;
        send_wire(stream, &WireMessage::GetChain { from_height })?;
    }
    Ok(())
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

    // HF67/v1.5.2: direct append fast-path for the common official-seed +1/+N
    // catch-up case. Some users were stuck at e.g. local #10533 while official
    // seeds were #10534: the snapshot path requested the right suffix but the
    // chain-adoption path could still fail to advance and the miner looped on
    // "official seed height is ahead". If the peer suffix starts exactly at our
    // next height, connect the blocks one-by-one against the current local UTXO
    // set. This is the same validation as mining/normal block connection, keeps
    // mempool cleanup correct, and avoids replacing any earlier local history.
    if start == local.blocks.len() {
        let mut candidate = local.clone();
        for block in blocks.clone() {
            candidate.connect_block(block, settings)?;
        }
        if candidate.height() > local.height() {
            *local = candidate;
            return Ok(true);
        }
        return Ok(false);
    }

    let mut candidate_blocks = local.blocks[..start].to_vec();
    candidate_blocks.extend(blocks);

    let local_height_before = local.height();
    let normal = local.try_adopt_peer_chain(
        candidate_blocks.clone(),
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
        let candidate = ChainState::from_blocks(candidate_blocks, settings)?;
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
    let mut contacted = 0usize;
    let mut raw_best_direct_height = 0u32;
    let mut ahead_counts: HashMap<(u32, String), usize> = HashMap::new();
    let mut conflict_counts: HashMap<String, usize> = HashMap::new();
    let mut first_conflict_addr: HashMap<String, String> = HashMap::new();

    for addr in known_peers(settings)?.into_iter().take(max_peers.max(1)) {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let Ok(info) = probe_peer(settings, &addr, Duration::from_millis(timeout_ms.max(100)))
        else {
            continue;
        };
        if info.height == 0 {
            continue;
        }
        contacted = contacted.saturating_add(1);
        raw_best_direct_height = raw_best_direct_height.max(info.height);
        if info.height > parent_height {
            *ahead_counts
                .entry((info.height, info.tip_hash.clone()))
                .or_insert(0) += 1;
        } else if info.height == parent_height
            && !info.tip_hash.trim().is_empty()
            && info.tip_hash != expected_parent_hash
        {
            *conflict_counts.entry(info.tip_hash.clone()).or_insert(0) += 1;
            first_conflict_addr
                .entry(info.tip_hash.clone())
                .or_insert(addr);
        }
    }

    // HF116: on mainnet, one noisy/future/private peer must not pause every
    // miner or make a found block miss the submit window. Only a two-peer quorum
    // is returned as an actionable ahead/conflict signal. Non-mainnet keeps the
    // old more aggressive direct-peer behavior for tests/dev networks.
    if settings.network.name == "mainnet" {
        let quorum_ahead = ahead_counts
            .iter()
            .filter(|(_, count)| **count >= 2)
            .map(|((height, _), _)| *height)
            .max()
            .unwrap_or(0);
        let mut conflicts = Vec::new();
        for (hash, count) in conflict_counts
            .iter()
            .filter(|(_, count)| **count >= 2)
            .take(4)
        {
            let addr = first_conflict_addr
                .get(hash)
                .cloned()
                .unwrap_or_else(|| "peer-quorum".to_string());
            conflicts.push(format!("{}@{} ({} peers)", addr, hash, count));
        }
        return Ok((contacted, quorum_ahead, conflicts));
    }

    let mut conflicts = Vec::new();
    for (hash, _) in conflict_counts.iter().take(4) {
        let addr = first_conflict_addr
            .get(hash)
            .cloned()
            .unwrap_or_else(|| "peer".to_string());
        conflicts.push(format!("{}@{}", addr, hash));
    }
    Ok((contacted, raw_best_direct_height, conflicts))
}

fn force_official_tip_if_ahead(
    settings: &Settings,
    report: &mut P2PSyncReport,
    local: &mut ChainState,
    timeout_ms: u64,
) -> Result<()> {
    let (_, mut official_height, mut official_tip) = official_tip_summary(settings, report, 420);
    if let Ok(Some((http_height, http_tip))) = official_http_tip(settings, 900) {
        report.peers_contacted = report.peers_contacted.max(1);
        report.best_peer_height = report.best_peer_height.max(http_height);
        if http_height >= official_height || official_tip.trim().is_empty() {
            official_height = http_height;
            official_tip = http_tip;
        }
    }

    if hf114_official_tip_acknowledges_local(settings, local, official_height, &official_tip) {
        mark_fresh_tip_trusted(settings, local);
        return Ok(());
    }

    // HF114: local-ahead is not "safe but ahead"; it is an unacknowledged
    // private suffix. Re-anchor it immediately to the official HTTP tail if the
    // official tip is an ancestor, instead of waiting for canonical height to pass
    // the local height while the user manually stops mining.
    let initial_same_height_conflict = official_height == local.height()
        && !official_tip.trim().is_empty()
        && official_tip != local.tip_hash().to_string();
    let initial_local_ahead =
        hf114_official_tip_is_local_ancestor(settings, local, official_height, &official_tip);
    if settings.network.name == "mainnet" && (initial_local_ahead || initial_same_height_conflict) {
        let reanchor =
            sync_official_http_tail_reanchor_hf114(settings, timeout_ms.min(18_000).max(6_000))?;
        merge_sync_reports(report, reanchor);
        *local = load_chain_for_hf90_catchup(settings)?;
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        if hf104_canonical_greenlight(settings, report, local, HF82_LIGHT_TIP_PROBE_MS.max(700)) {
            return Ok(());
        }
        let (_, check_height, check_tip) = official_tip_summary(settings, report, 420);
        if hf114_official_tip_acknowledges_local(settings, local, check_height, &check_tip) {
            mark_fresh_tip_trusted(settings, local);
            return Ok(());
        }
        bail!(
            "mining green-light wait: local tip #{} is not acknowledged by official/direct canonical tip #{} after HF114 re-anchor attempt. Mining is paused before extending a self-mined stale branch.",
            local.height(),
            check_height
        );
    }

    if official_height > local.height()
        && hf97_uncatchable_tip_quarantined(settings, local, official_height)
    {
        report.best_peer_height = local.height();
        report.height = local.height();
        report.tip_hash = local.tip_hash().to_string();
        mark_fresh_tip_trusted(settings, local);
        return Ok(());
    }

    let official_tip_differs = official_height == local.height()
        && !official_tip.trim().is_empty()
        && official_tip != local.tip_hash().to_string();
    if official_height > local.height() || official_tip_differs {
        let heal = hf90_mining_catchup(settings, timeout_ms.max(HF82_PARENT_CATCHUP_MS))?;
        merge_sync_reports(report, heal);
        *local = load_chain_for_hf90_catchup(settings)?;
        if settings.network.name == "mainnet" {
            let reanchor = sync_official_http_tail_reanchor_hf114(
                settings,
                timeout_ms.min(18_000).max(6_000),
            )?;
            merge_sync_reports(report, reanchor);
            *local = load_chain_for_hf90_catchup(settings)?;
        }
        validate_chain_consensus_checkpoints(settings, &local.blocks)?;
        if hf104_canonical_greenlight(settings, report, local, HF82_LIGHT_TIP_PROBE_MS) {
            return Ok(());
        }

        let (_, mut latest_official_height, mut latest_official_tip) =
            official_tip_summary(settings, report, 350);
        if let Ok(Some((http_height, http_tip))) = official_http_tip(settings, 900) {
            report.peers_contacted = report.peers_contacted.max(1);
            report.best_peer_height = report.best_peer_height.max(http_height);
            if http_height >= latest_official_height || latest_official_tip.trim().is_empty() {
                latest_official_height = http_height;
                latest_official_tip = http_tip;
            }
        }
        if hf114_official_tip_acknowledges_local(
            settings,
            local,
            latest_official_height,
            &latest_official_tip,
        ) {
            mark_fresh_tip_trusted(settings, local);
            return Ok(());
        }

        let latest_same_height_conflict = latest_official_height == local.height()
            && !latest_official_tip.trim().is_empty()
            && latest_official_tip != local.tip_hash().to_string();
        let latest_local_ahead = hf114_official_tip_is_local_ancestor(
            settings,
            local,
            latest_official_height,
            &latest_official_tip,
        );
        if settings.network.name == "mainnet" && (latest_local_ahead || latest_same_height_conflict)
        {
            let reanchor = sync_official_http_tail_reanchor_hf114(
                settings,
                timeout_ms.min(18_000).max(6_000),
            )?;
            merge_sync_reports(report, reanchor);
            *local = load_chain_for_hf90_catchup(settings)?;
            validate_chain_consensus_checkpoints(settings, &local.blocks)?;
            if hf104_canonical_greenlight(settings, report, local, 700) {
                return Ok(());
            }
        }

        if latest_official_height > local.height() {
            let gap = latest_official_height.saturating_sub(local.height());
            if gap > 4096 {
                let http =
                    sync_official_http_snapshot(settings, timeout_ms.min(20_000).max(10_000))?;
                merge_sync_reports(report, http);
                *local = load_chain_for_hf90_catchup(settings)?;
                if hf104_canonical_greenlight(settings, report, local, 700) {
                    return Ok(());
                }
            }
        }

        if latest_official_height > local.height() {
            if hf97_greenlight_local_tip_after_uncatchable_height(
                settings,
                report,
                local,
                latest_official_height,
                "mining-greenlight-official-ahead",
            )? {
                return Ok(());
            }
            bail!(
                "mining green-light wait: official seed height {} is {} block(s) ahead of local height {}. QUB Core is auto-catching up with HF98 detached catch-up; manual Sync is optional only if progress stops.",
                latest_official_height,
                latest_official_height.saturating_sub(local.height()),
                local.height()
            );
        }
        if latest_official_height == local.height()
            && !latest_official_tip.trim().is_empty()
            && latest_official_tip != local.tip_hash().to_string()
        {
            bail!(
                "mining green-light wait: official seed tip at #{} differs from local tip. HF114 bounded repair/re-anchor is running before hashing.",
                latest_official_height
            );
        }
        if settings.network.name == "mainnet"
            && latest_official_height > 0
            && latest_official_height < local.height()
            && !hf114_official_tip_acknowledges_local(
                settings,
                local,
                latest_official_height,
                &latest_official_tip,
            )
        {
            bail!(
                "mining green-light wait: local height #{} is ahead of official/direct height #{} without acknowledgement. HF116 prevents extending this stale suffix.",
                local.height(),
                latest_official_height
            );
        }
    }
    Ok(())
}
/// HF54/v1.5.2 hard template gate. This runs immediately before building a
/// mining candidate and again immediately before submitting a found block.
/// It prevents GUI/CLI miners from extending a local branch when direct peers
/// are already ahead or disagree at the candidate parent height. Registry-only
/// high-tip telemetry is deliberately ignored here.
pub fn mining_parent_guard(
    settings: &Settings,
    parent_height: u32,
    expected_parent_hash: Hash256,
) -> Result<P2PSyncReport> {
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
        return finish_report(settings, P2PSyncReport::default());
    }

    let mut report = hf90_mining_catchup(settings, 90_000).unwrap_or_default();
    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if (hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS))
        && local.height() == parent_height
        && local.tip_hash() == expected_parent_hash
    {
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
    force_official_tip_if_ahead(
        settings,
        &mut report,
        &mut local,
        HF80_MINING_PARENT_GATE_MS,
    )?;
    if settings.network.name == "mainnet"
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
    report.best_peer_height = report.best_peer_height.max(best_direct_height);

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
        report.best_peer_height = report.best_peer_height.max(retry_best_height);
        if retry_best_height > parent_height {
            if hf97_uncatchable_tip_quarantined(settings, &local, retry_best_height) {
                report.best_peer_height = parent_height;
            } else {
                bail!(
                    "mining paused: direct peer height {} is ahead of candidate parent #{}. Adaptive sync is still catching up; candidate will be rebuilt automatically.",
                    retry_best_height,
                    parent_height
                );
            }
        }
        if !retry_conflicts.is_empty() {
            bail!(
                "mining paused: direct peer(s) disagree at candidate parent #{}: {}. Candidate discarded; sync/repair first.",
                parent_height,
                retry_conflicts.join(", ")
            );
        }
    }

    if !conflicts.is_empty() {
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
    let mut report = P2PSyncReport::default();
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
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
    report.best_peer_height = report.best_peer_height.max(best_direct_height);
    if !conflicts.is_empty() {
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
    finish_report(settings, report)
}

/// HF49 fork-safety guard used by CLI/GUI mining entrypoints. It deliberately
/// trusts only locally validated chain data and direct TCP peers. Unreachable
/// registry-only high-tip reports are useful telemetry, but they are not enough
/// to make a node mine on a different branch.
pub fn mining_safety_check(settings: &Settings) -> Result<P2PSyncReport> {
    if !settings.p2p.enabled || matches!(settings.network.name.as_str(), "regtest" | "regtest-lan")
    {
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
            return finish_report(settings, report);
        }
    }

    let mut report = hf90_mining_catchup(settings, 120_000).unwrap_or_default();
    let mut local = load_chain_for_hf90_catchup(settings)?;
    validate_chain_consensus_checkpoints(settings, &local.blocks)?;
    if hf104_canonical_greenlight(settings, &mut report, &local, HF82_LIGHT_TIP_PROBE_MS) {
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
        report.best_peer_height = report.best_peer_height.max(catchup.best_peer_height);
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
    force_official_tip_if_ahead(settings, &mut report, &mut local, HF80_MINING_FAST_GATE_MS)?;
    if settings.network.name == "mainnet"
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
        report.best_peer_height = report.best_peer_height.max(retry.best_peer_height);
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
    report.best_peer_height = report.best_peer_height.max(best_direct_height);
    if !conflicting_direct.is_empty() {
        bail!(
            "mining paused: direct peer(s) report a different hash at local height #{}: {}. Do not mine until branches converge.",
            local_height,
            conflicting_direct.join(", ")
        );
    }
    if best_direct_height > local_height {
        if hf97_uncatchable_tip_quarantined(settings, &local, best_direct_height) {
            report.best_peer_height = report.best_peer_height.min(local_height);
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

fn hf115_mempool_relay_batch(settings: &Settings, mempool: &[Transaction]) -> Vec<Transaction> {
    let mut txs = mempool
        .iter()
        .filter(|tx| hf106_jin_sale_standardness_policy(tx, settings).is_ok())
        .collect::<Vec<_>>();
    txs.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });
    txs.into_iter()
        .take(hf115_mempool_inbound_limit(settings).min(HF116_MEMPOOL_RELAY_BATCH_TXS))
        .cloned()
        .collect::<Vec<_>>()
}

fn merge_mempool_from_chain(
    local: &mut ChainState,
    source: &ChainState,
    settings: &Settings,
) -> Vec<Transaction> {
    // HF76/v1.5.8: merge by txid and let full mempool admission resolve feature-state
    // conflicts. This is used before any p2p loop save so GUI/CLI-created txs cannot
    // be overwritten by an older in-memory node view.
    let mut accepted = Vec::<Transaction>::new();
    let mut known = local.mempool_txids();
    for tx in source
        .mempool
        .iter()
        .cloned()
        .take(effective_mempool_max_transactions(settings))
    {
        let txid = tx.txid();
        if known.contains(&txid) {
            continue;
        }
        if local
            .accept_transaction_to_mempool(tx.clone(), settings)
            .is_ok()
        {
            known.insert(txid);
            accepted.push(tx);
        }
    }
    accepted
}

pub fn rebroadcast_local_mempool(settings: &Settings, max_txs: usize) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let mut txs = chain.mempool.iter().collect::<Vec<_>>();
    // HF107/v1.6.9: prioritize pool shares and high-impact JIN protocol txs
    // deterministically so large JIN sale purchases do not bounce around the
    // network while ordinary traffic is relayed first.
    txs.sort_by_cached_key(|tx| {
        (
            mempool_template_priority(settings, *tx),
            tx.txid().to_string(),
        )
    });
    let mut sent = 0usize;
    for tx in txs
        .into_iter()
        .filter(|tx| hf106_jin_sale_standardness_policy(tx, settings).is_ok())
        .take(max_txs.max(1).min(HF116_MEMPOOL_RELAY_BATCH_TXS))
    {
        sent = sent.saturating_add(relay_tx_to_known_peers(settings, tx, None).unwrap_or(0));
    }
    Ok(sent)
}

pub fn broadcast_block(settings: &Settings, block: &Block) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let mut sent = 0usize;
    // HF116: relay the found block plus a small recent suffix, not the entire
    // chain to every peer. HF114 could spend the submit window serializing/sending
    // the whole chain repeatedly, which amplified JIN/Library/mempool load and
    // made block propagation look stalled on weak links. Peers that are far behind
    // can still request older windows through GetChain/GetHeaders.
    let tail_from = chain.height().saturating_sub(128);
    for addr in
        prioritized_outbound_peers(settings, settings.p2p.max_outbound_peers.max(16).min(48))?
    {
        let Ok(mut stream) = connect_peer(&addr, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let _ = send_version(&mut stream, settings, &chain);
        let mut ok = send_wire(
            &mut stream,
            &WireMessage::Block {
                block: block.clone(),
            },
        )
        .is_ok();
        if ok {
            ok = send_inv(&mut stream, &chain).is_ok();
        }
        if ok {
            ok = send_headers(&mut stream, settings, &chain, tail_from).is_ok();
        }
        if ok {
            ok = send_chain(&mut stream, settings, &chain, tail_from).is_ok();
        }
        if ok {
            sent += 1;
        }
    }
    Ok(sent)
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

pub fn broadcast_tx(settings: &Settings, tx: &Transaction) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    broadcast(settings, WireMessage::Tx { tx: tx.clone() })
}

/// HF114/v1.7.2: exact, bounded transaction relay for GUI-created high-value
/// actions. HF113 repeatedly rebroadcasted the whole mempool after JIN buys; on
/// weak links that could tie up relay sockets and make mining look like block
/// time slowed after a buy attempt. This path relays only the requested tx to a
/// small official-first peer set with short per-peer timeouts.
pub fn broadcast_tx_limited(
    settings: &Settings,
    tx: &Transaction,
    max_peers: usize,
    timeout_ms: u64,
) -> Result<usize> {
    if !settings.p2p.enabled {
        return Ok(0);
    }
    if hf106_jin_sale_standardness_policy(tx, settings).is_err() {
        return Ok(0);
    }
    let chain = load_chain_for_hf90_catchup(settings)?;
    let per_peer = Duration::from_millis(timeout_ms.max(150).min(2_000));
    let mut sent = 0usize;
    for addr in prioritized_outbound_peers(settings, max_peers.max(1).min(32))? {
        if should_skip_outbound(settings, &addr) {
            continue;
        }
        let Ok(mut stream) = connect_peer(&addr, per_peer) else {
            continue;
        };
        let _ = stream.set_read_timeout(Some(per_peer));
        let _ = stream.set_write_timeout(Some(per_peer));
        let _ = send_version(&mut stream, settings, &chain);
        if send_wire(&mut stream, &WireMessage::Tx { tx: tx.clone() }).is_ok() {
            sent = sent.saturating_add(1);
        }
    }
    Ok(sent)
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
    let source = source_peer.map(normalize_peer_addr).unwrap_or_default();
    let mut sent = 0usize;
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
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let chain = load_chain_for_hf90_catchup(settings)?;
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
    let mut sent = 0usize;
    for addr in
        prioritized_outbound_peers(settings, settings.p2p.max_outbound_peers.max(16).min(48))?
    {
        let Ok(mut stream) = connect_peer(&addr, Duration::from_secs(2)) else {
            continue;
        };
        stream.set_read_timeout(Some(Duration::from_secs(2)))?;
        stream.set_write_timeout(Some(Duration::from_secs(2)))?;
        let chain = load_chain_for_hf90_catchup(settings)?;
        let _ = send_version(&mut stream, settings, &chain);
        if send_wire(&mut stream, &message).is_ok() {
            sent += 1;
        }
    }
    Ok(sent)
}

fn handle_peer(
    mut stream: TcpStream,
    peer_addr: String,
    outbound: bool,
    settings: Settings,
    chain: Arc<Mutex<ChainState>>,
    peers: Arc<Mutex<HashSet<String>>>,
    active: Arc<Mutex<HashSet<String>>>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
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
        send_wire(&mut stream, &WireMessage::GetAddr)?;
        send_wire(&mut stream, &WireMessage::GetPeerList)?;
        send_wire(&mut stream, &WireMessage::GetMempool)?;
        if outbound {
            send_wire(&mut stream, &WireMessage::GetHeaders { from_height: 0 })?;
            send_wire(&mut stream, &WireMessage::GetChain { from_height: 0 })?;
        }
        let mut reader = BufReader::new(stream.try_clone()?);
        let mut peer_errors = 0u32;
        let mut last_inv = Instant::now();
        let mut session = PeerSession::default();

        loop {
            match read_wire(&mut reader, settings.p2p.max_message_bytes) {
                Ok(message) => {
                    if let Err(err) = process_peer_message(
                        &settings,
                        &peer_key,
                        message,
                        &mut stream,
                        &chain,
                        &peers,
                        &mut session,
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
                }
                Err(err) if is_timeout(&err) => {
                    if last_inv.elapsed() >= Duration::from_secs(4) {
                        let local = chain.lock().expect("chain mutex poisoned").clone();
                        // Heartbeat identity/status too, not just inv. This keeps the seed
                        // registry fresh for NAT/firewalled miners that keep one long-lived
                        // outbound connection and therefore do not re-send Version often.
                        send_version(&mut stream, &settings, &local)?;
                        send_inv(&mut stream, &local)?;
                        send_wire(&mut stream, &WireMessage::GetAddr)?;
                        send_wire(&mut stream, &WireMessage::GetPeerList)?;
                        send_wire(&mut stream, &WireMessage::GetMempool)?;
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

fn process_peer_message(
    settings: &Settings,
    peer_addr: &str,
    message: WireMessage,
    stream: &mut TcpStream,
    chain: &Arc<Mutex<ChainState>>,
    peers: &Arc<Mutex<HashSet<String>>>,
    session: &mut PeerSession,
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
            node_id,
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
            session.node_id = node_id.clone();
            session.listen_addr = listen_addr.clone();
            session.role = role.clone();
            session.miner_address = miner_address.clone();
            record_peer_observation(
                settings,
                peer_addr,
                &node_id,
                &listen_addr,
                height,
                &tip_hash,
                &user_agent,
                &role,
                &miner_address,
            )?;
            // Persist both the advertised listen address and the actually observed TCP
            // address. If the DNS seed is down, nodes can still redial recently seen
            // peers from the local peerbook instead of depending on the seed forever.
            add_peer(settings, peers, peer_addr)?;
            if !listen_addr.trim().is_empty() {
                add_peer(settings, peers, &listen_addr)?;
            }
            let local = chain.lock().expect("chain mutex poisoned");
            let local_height = local.height();
            let local_tip = local.tip_hash().to_string();
            drop(local);
            if height > local_height || (height == local_height && tip_hash != local_tip) {
                let local_snapshot = chain.lock().expect("chain mutex poisoned").clone();
                send_adaptive_chain_requests(stream, &local_snapshot, height, &tip_hash)?;
            }
            send_inv(stream, &chain.lock().expect("chain mutex poisoned"))?;
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
                send_adaptive_chain_requests(stream, &local_snapshot, height, &tip_hash)?;
            }
        }
        WireMessage::GetHeaders { from_height } => {
            let local = chain.lock().expect("chain mutex poisoned").clone();
            send_headers(stream, settings, &local, from_height)?;
        }
        WireMessage::Headers { headers } => {
            let local_height = chain.lock().expect("chain mutex poisoned").height();
            if headers.last().map(|h| h.height).unwrap_or(0) > local_height {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: local_height.saturating_add(1),
                    },
                )?;
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
            if blocks.is_empty() {
                return Ok(());
            }
            let more_expected = blocks.len() == settings.p2p.max_blocks_per_message;
            let next_from = start_height.saturating_add(blocks.len() as u32);
            let mut local = chain.lock().expect("chain mutex poisoned");
            let mut changed = false;
            if start_height <= local.height().saturating_add(1) {
                let local_height_before = local.height();
                match try_adopt_overlapping_blocks(
                    &mut local,
                    start_height,
                    blocks,
                    settings,
                    prefer_peer_tip_on_equal_work(settings, peer_addr),
                ) {
                    Ok(true) => {
                        save_chain(settings, &local)?;
                        changed = true;
                    }
                    Ok(false) => {
                        if start_height == local_height_before.saturating_add(1) {
                            // Missing suffix did not connect; request an overlapping fork window instead of full genesis history.
                            request_earlier_fork_window(stream, local_height_before, start_height)?;
                        }
                    }
                    Err(_) => {
                        request_earlier_fork_window(stream, local_height_before, start_height)?;
                    }
                }
            } else {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: local.height().saturating_add(1),
                    },
                )?;
            }
            drop(local);
            if changed {
                let _ = relay_chain_to_known_peers(settings, Some(peer_addr));
            }
            if more_expected {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: next_from,
                    },
                )?;
            }
        }
        WireMessage::Block { block } => {
            let mut local = chain.lock().expect("chain mutex poisoned");
            let mut changed = false;
            if block.header.prev_block_hash == local.tip_hash() {
                local.connect_block(block, settings)?;
                save_chain(settings, &local)?;
                changed = true;
            } else {
                let h = local.height();
                request_earlier_fork_window(stream, h, h.saturating_add(1))?;
            }
            drop(local);
            if changed {
                let _ = relay_chain_to_known_peers(settings, Some(peer_addr));
            }
        }
        WireMessage::Tx { tx } => {
            let accepted = {
                let mut local = chain.lock().expect("chain mutex poisoned");
                if local
                    .accept_transaction_to_mempool(tx.clone(), settings)
                    .is_ok()
                {
                    save_chain(settings, &local)?;
                    true
                } else {
                    false
                }
            };
            if accepted {
                let _ = relay_tx_to_known_peers(settings, &tx, Some(peer_addr));
            }
        }
        WireMessage::GetMempool => {
            let txs = {
                let local = chain.lock().expect("chain mutex poisoned");
                hf115_mempool_relay_batch(settings, &local.mempool)
            };
            send_wire(stream, &WireMessage::Mempool { txs })?;
        }
        WireMessage::Mempool { txs } => {
            let mut accepted_txs = Vec::new();
            {
                let mut local = chain.lock().expect("chain mutex poisoned");
                for tx in txs.into_iter().take(hf115_mempool_inbound_limit(settings)) {
                    if local
                        .accept_transaction_to_mempool(tx.clone(), settings)
                        .is_ok()
                    {
                        accepted_txs.push(tx);
                    }
                }
                if !accepted_txs.is_empty() {
                    save_chain(settings, &local)?;
                }
            }
            for tx in accepted_txs {
                let _ = relay_tx_to_known_peers(settings, &tx, Some(peer_addr));
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
    if session.node_id.trim().is_empty() {
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
            node_id,
            role,
            miner_address,
            ..
        } => {
            if protocol != PROTOCOL_VERSION {
                bail!("unsupported protocol {protocol}");
            }
            validate_remote_network(settings, &network, &magic, &genesis_hash)?;
            record_peer_observation(
                settings,
                addr,
                &node_id,
                &listen_addr,
                height,
                &tip_hash,
                &user_agent,
                &role,
                &miner_address,
            )?;
            report.best_peer_height = report.best_peer_height.max(height);
            let local = load_chain_for_hf90_catchup(settings)?;
            if height > local.height()
                || (height == local.height() && tip_hash != local.tip_hash().to_string())
            {
                send_adaptive_chain_requests(stream, &local, height, &tip_hash)?;
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
            if start_height <= local.height().saturating_add(1) {
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
            } else {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: local.height().saturating_add(1),
                    },
                )?;
            }
            if changed {
                let _ = relay_chain_to_known_peers(settings, Some(addr));
            }
            if more_expected {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: next_from,
                    },
                )?;
            }
        }
        WireMessage::Block { block } => {
            let mut local = load_chain_for_hf90_catchup(settings)?;
            let mut changed = false;
            if block.header.prev_block_hash == local.tip_hash() {
                local.connect_block(block, settings)?;
                save_chain(settings, &local)?;
                report.blocks_connected += 1;
                changed = true;
            } else {
                let h = local.height();
                request_earlier_fork_window(stream, h, h.saturating_add(1))?;
            }
            if changed {
                let _ = relay_chain_to_known_peers(settings, Some(addr));
            }
        }
        WireMessage::Tx { tx } => {
            let mut local = load_chain_for_hf90_catchup(settings)?;
            if local
                .accept_transaction_to_mempool(tx.clone(), settings)
                .is_ok()
            {
                save_chain(settings, &local)?;
                report.txs_accepted += 1;
                let _ = relay_tx_to_known_peers(settings, &tx, Some(addr));
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
            if headers.last().map(|h| h.height).unwrap_or(0) > local.height() {
                send_wire(
                    stream,
                    &WireMessage::GetChain {
                        from_height: local.height().saturating_add(1),
                    },
                )?;
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
            let mut accepted_txs = Vec::<Transaction>::new();
            for tx in txs.into_iter().take(hf115_mempool_inbound_limit(settings)) {
                if local
                    .accept_transaction_to_mempool(tx.clone(), settings)
                    .is_ok()
                {
                    accepted_txs.push(tx);
                }
            }
            if !accepted_txs.is_empty() {
                save_chain(settings, &local)?;
                report.txs_accepted = report.txs_accepted.saturating_add(accepted_txs.len());
            }
            for tx in accepted_txs {
                let _ = relay_tx_to_known_peers(settings, &tx, Some(addr));
            }
        }
        WireMessage::Ping { nonce } => send_wire(stream, &WireMessage::Pong { nonce })?,
        WireMessage::Inv {
            height, tip_hash, ..
        } => {
            report.best_peer_height = report.best_peer_height.max(height);
            let local = load_chain_for_hf90_catchup(settings)?;
            if height > local.height()
                || (height == local.height() && tip_hash != local.tip_hash().to_string())
            {
                send_adaptive_chain_requests(stream, &local, height, &tip_hash)?;
            }
        }
        WireMessage::GetAddr
        | WireMessage::GetPeerList
        | WireMessage::Pong { .. }
        | WireMessage::Reject { .. } => {}
    }
    Ok(())
}

fn send_version(stream: &mut TcpStream, settings: &Settings, chain: &ChainState) -> Result<()> {
    send_wire(
        stream,
        &WireMessage::Version {
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
            role: if settings.mining.enabled {
                "miner".to_string()
            } else {
                "node".to_string()
            },
            miner_address: runtime_miner_address(settings)
                .unwrap_or_else(|| settings.mining.miner_address.clone()),
        },
    )
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
    let blocks = if start < chain.blocks.len() {
        chain.blocks[start..end].to_vec()
    } else {
        Vec::new()
    };
    send_wire(
        stream,
        &WireMessage::Chain {
            start_height: from_height,
            blocks,
        },
    )
}

fn send_wire(stream: &mut TcpStream, message: &WireMessage) -> Result<()> {
    let mut raw = serde_json::to_vec(message)?;
    raw.push(b'\n');
    stream.write_all(&raw)?;
    stream.flush()?;
    Ok(())
}

fn read_wire(reader: &mut BufReader<TcpStream>, max_bytes: usize) -> io::Result<WireMessage> {
    let mut raw = Vec::new();
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "peer closed"));
        }
        let take = available
            .iter()
            .position(|b| *b == b'\n')
            .map(|pos| pos + 1)
            .unwrap_or(available.len());
        raw.extend_from_slice(&available[..take]);
        reader.consume(take);
        if raw.len() > max_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }
        if raw.last() == Some(&b'\n') {
            break;
        }
    }
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

fn connect_peer(addr: &str, timeout: Duration) -> Result<TcpStream> {
    let mut last_err = None;
    for socket in addr
        .to_socket_addrs()
        .with_context(|| format!("bad peer address {addr}"))?
    {
        match TcpStream::connect_timeout(&socket, timeout) {
            Ok(stream) => return Ok(stream),
            Err(err) => last_err = Some(err),
        }
    }
    match last_err {
        Some(err) => Err(err).with_context(|| format!("could not connect to {addr}")),
        None => bail!("no socket addresses for {addr}"),
    }
}

fn should_skip_outbound(settings: &Settings, addr: &str) -> bool {
    is_self_or_empty_addr(settings, addr)
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
            && !is_self_or_empty_addr(settings, &normalized)
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
        if is_public_socket_addr_text(&peer.observed_addr) {
            push_unique(peer.observed_addr.clone());
        }
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
    {
        let mut p = peers.lock().expect("peer mutex poisoned");
        p.insert(normalized);
        save_peer_set(settings, &p)?;
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
    Ok(book
        .peers
        .into_iter()
        .map(|p| normalize_peer_addr(&p))
        .filter(|p| !is_self_or_empty_addr(settings, p))
        .collect())
}

fn save_peer_set(settings: &Settings, peers: &HashSet<String>) -> Result<()> {
    let path = peerbook_path(settings);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut list = peers
        .iter()
        .map(|p| normalize_peer_addr(p))
        .filter(|p| !is_self_or_empty_addr(settings, p))
        .collect::<Vec<_>>();
    list.sort();
    list.dedup();
    std::fs::write(
        path,
        serde_json::to_string_pretty(&PeerBook { peers: list })?,
    )?;
    Ok(())
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

fn node_id(settings: &Settings) -> Result<String> {
    let path = node_id_path(settings);
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
    std::fs::write(path, serde_json::to_string_pretty(&registry)?)?;
    Ok(())
}

fn merge_peer_registry(settings: &Settings, incoming: Vec<P2PObservedPeer>) -> Result<()> {
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
    let mut registry = load_peer_registry(settings)?;
    upsert_observed_peer(
        &mut registry,
        P2PObservedPeer {
            node_id: remote_node_id.to_string(),
            observed_addr: normalize_peer_addr(observed_addr),
            listen_addr: normalize_peer_addr(listen_addr),
            height,
            tip_hash: tip_hash.to_string(),
            user_agent: user_agent.to_string(),
            role: role.to_string(),
            miner_address: miner_address.to_string(),
            last_seen_unix: unix_time_u32() as u64,
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
    thread::spawn(move || {
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
        for seed in release_bootnodes(settings).into_iter().take(2) {
            let seed = normalize_peer_addr(&seed);
            if seed.is_empty() || should_skip_outbound(settings, &seed) {
                continue;
            }
            snapshot.peers.push(P2PPeerStatus {
                addr: seed,
                reachable: false,
                global_live: true,
                height: Some(height),
                tip_hash: Some(tip_hash.clone()),
                user_agent: Some("official-http-tip".to_string()),
                error: None,
                node_id: None,
                observed_addr: None,
                listen_addr: None,
                role: Some("seed tip".to_string()),
                miner_address: None,
                last_seen_unix: Some(unix_time_u32() as u64),
                seen_age_secs: Some(0),
            });
        }
    }
    // HF88: HTTP snapshots can lag while seed nodes are live. Add a tiny direct
    // official-seed probe to the non-blocking peer view so known-tip/mining UI
    // does not freeze at a stale HTTP height.
    for (addr, height, tip_hash, _) in official_snapshot_peer_candidates(settings, 350)
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
            node_id: None,
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

fn probe_peer(settings: &Settings, addr: &str, timeout: Duration) -> Result<P2PProbeInfo> {
    let mut stream = connect_peer(addr, timeout)?;
    stream.set_read_timeout(Some(timeout))?;
    stream.set_write_timeout(Some(timeout))?;
    // HF88/v1.6.2: peer status/version probes only need local height/tip for
    // the Version message. Use the UI-fast loader so a bounded peer probe does
    // not replay-validate the entire chain and stall GUI snapshots.
    let chain =
        load_or_init_chain_for_ui_fast(settings).or_else(|_| load_or_init_chain(settings))?;
    send_version(&mut stream, settings, &chain)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let deadline = Instant::now() + timeout.saturating_mul(2);
    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes) {
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

fn fetch_peer_list(
    settings: &Settings,
    addr: &str,
    timeout: Duration,
) -> Result<Vec<P2PObservedPeer>> {
    let mut stream = connect_peer(addr, timeout)?;
    stream.set_read_timeout(Some(timeout.saturating_mul(2)))?;
    stream.set_write_timeout(Some(timeout))?;
    // HF88/v1.6.2: peer status/version probes only need local height/tip for
    // the Version message. Use the UI-fast loader so a bounded peer probe does
    // not replay-validate the entire chain and stall GUI snapshots.
    let chain =
        load_or_init_chain_for_ui_fast(settings).or_else(|_| load_or_init_chain(settings))?;
    send_version(&mut stream, settings, &chain)?;
    send_wire(&mut stream, &WireMessage::GetPeerList)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let deadline = Instant::now() + timeout.saturating_mul(2);
    while Instant::now() < deadline {
        match read_wire(&mut reader, settings.p2p.max_message_bytes) {
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
    Ok(report)
}
