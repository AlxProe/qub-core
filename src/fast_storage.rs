use crate::*;
use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
#[cfg(windows)]
use std::os::windows::process::CommandExt;
#[cfg(windows)]
use std::process::Command;

pub const HF123_FAST_STORAGE_SCHEMA_VERSION: u32 = 1;
pub const HF123_FAST_STORAGE_MAGIC: &str = "QUB-FCE-1";
pub const HF123_FAST_STATUS_RECENT_BLOCKS: usize = 64;
pub const HF123_LEGACY_EXPORT_INTERVAL_SECS: u64 = 6 * 60 * 60;
pub const HF123_LEGACY_EXPORT_BLOCK_INTERVAL: u32 = 256;
pub const HF123_STORAGE_LOCK_STALE_SECS: u64 = 15 * 60;

/// HF126/v1.8.2 keeps normal Fast Chain Engine commits strictly
/// cumulative-work monotonic, while permitting one explicitly verified
/// equal-work/same-height branch replacement during stale-local re-anchoring.
/// The verified policy is never used by ordinary block, mempool or RPC writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FastCommitPolicy {
    Normal,
    VerifiedEqualWorkReanchor,
}

static FAST_LOADS: AtomicU64 = AtomicU64::new(0);
static FAST_COMMITS: AtomicU64 = AtomicU64::new(0);
static FAST_BYTES_READ: AtomicU64 = AtomicU64::new(0);
static FAST_BYTES_WRITTEN: AtomicU64 = AtomicU64::new(0);
static FAST_LEGACY_EXPORTS: AtomicU64 = AtomicU64::new(0);
static FAST_LEGACY_EXPORT_FAILURES: AtomicU64 = AtomicU64::new(0);
static FAST_LAST_LOAD_MILLIS: AtomicU64 = AtomicU64::new(0);
static FAST_MAX_LOAD_MILLIS: AtomicU64 = AtomicU64::new(0);
static FAST_LAST_COMMIT_MILLIS: AtomicU64 = AtomicU64::new(0);
static FAST_MAX_COMMIT_MILLIS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastRecentBlock {
    pub height: u32,
    pub hash: Hash256,
    pub version: u32,
    pub time: u32,
    pub tx_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastStorageIdentity {
    pub schema_version: u32,
    pub magic: String,
    pub network: String,
    pub generation: u64,
    pub state_revision: u64,
    pub committed_height: u32,
    pub tip_hash: Hash256,
    pub tip_block_version: u32,
    pub mempool_digest: Hash256,
    pub total_work_hex: String,
    pub committed_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FastStoragePointer {
    schema_version: u32,
    magic: String,
    network: String,
    generation: u64,
    state_revision: u64,
    blocks_file: String,
    state_file: String,
    state_sha256: String,
    journal_bytes: u64,
    committed_height: u32,
    tip_hash: Hash256,
    tip_header: BlockHeader,
    tip_tx_count: usize,
    mempool_digest: Hash256,
    total_work_hex: String,
    committed_at_unix: u64,
}

impl FastStoragePointer {
    fn identity(&self) -> FastStorageIdentity {
        FastStorageIdentity {
            schema_version: self.schema_version,
            magic: self.magic.clone(),
            network: self.network.clone(),
            generation: self.generation,
            state_revision: self.state_revision,
            committed_height: self.committed_height,
            tip_hash: self.tip_hash,
            tip_block_version: self.tip_header.version,
            mempool_digest: self.mempool_digest,
            total_work_hex: self.total_work_hex.clone(),
            committed_at_unix: self.committed_at_unix,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FastStorageState {
    schema_version: u32,
    magic: String,
    network: String,
    generation: u64,
    state_revision: u64,
    committed_height: u32,
    tip_hash: Hash256,
    tip_header: BlockHeader,
    tip_tx_count: usize,
    journal_bytes: u64,
    mempool_digest: Hash256,
    total_work_hex: String,
    utxos: Vec<PersistedUtxo>,
    mempool: Vec<Transaction>,
    recent_blocks: Vec<FastRecentBlock>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct LegacyExportStatus {
    schema_version: u32,
    network: String,
    height: u32,
    tip_hash: Hash256,
    exported_at_unix: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FastStorageStats {
    pub ok: bool,
    pub storage_engine: String,
    pub network: String,
    pub data_dir: String,
    pub fast_storage_dir: String,
    pub generation: u64,
    pub state_revision: u64,
    pub height: u32,
    pub tip_hash: String,
    pub tip_block_version: u32,
    pub journal_bytes: u64,
    pub state_bytes: u64,
    pub legacy_chain_bytes: u64,
    pub mempool_tx_count: usize,
    pub mempool_digest: String,
    pub process_loads: u64,
    pub process_commits: u64,
    pub process_bytes_read: u64,
    pub process_bytes_written: u64,
    pub process_legacy_exports: u64,
    pub process_legacy_export_failures: u64,
    pub legacy_export_running: bool,
    pub last_load_millis: u64,
    pub max_load_millis: u64,
    pub last_commit_millis: u64,
    pub max_commit_millis: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainJsonExportReport {
    pub height: u32,
    pub tip_hash: Hash256,
    pub bytes: u64,
    pub source: String,
    pub storage_engine: String,
    pub generation: u64,
    pub state_revision: u64,
    pub committed_at_unix: u64,
    pub recovered_from_previous: bool,
}

struct FastStorageLease {
    path: PathBuf,
}

impl Drop for FastStorageLease {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn now_nanos_u64() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

fn elapsed_millis(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u128::from(u64::MAX)) as u64
}

fn record_max(target: &AtomicU64, value: u64) {
    let mut current = target.load(Ordering::Relaxed);
    while value > current {
        match target.compare_exchange_weak(
            current,
            value,
            Ordering::Relaxed,
            Ordering::Relaxed,
        ) {
            Ok(_) => break,
            Err(observed) => current = observed,
        }
    }
}

fn sha256_hex(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

fn hash_file_prefix(path: &Path, bytes: u64) -> Result<String> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let mut reader = BufReader::new(file.take(bytes));
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 1024];
    let mut total = 0u64;
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
    if total != bytes {
        bail!("short Fast Chain Engine journal: expected {bytes} byte(s), read {total}");
    }
    FAST_BYTES_READ.fetch_add(total, Ordering::Relaxed);
    Ok(hex::encode(hasher.finalize()))
}

fn mempool_digest(txs: &[Transaction]) -> Hash256 {
    let mut ids = txs.iter().map(Transaction::txid).collect::<Vec<_>>();
    ids.sort();
    let mut raw = Vec::with_capacity(ids.len().saturating_mul(32));
    for txid in ids {
        raw.extend_from_slice(txid.as_bytes());
    }
    Hash256::double_sha256(&raw)
}

fn sorted_utxos(chain: &ChainState) -> Vec<PersistedUtxo> {
    let mut out = chain
        .utxos
        .iter()
        .map(|(outpoint, coin)| PersistedUtxo {
            outpoint: outpoint.clone(),
            coin: coin.clone(),
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.outpoint.key().cmp(&b.outpoint.key()));
    out
}

fn recent_blocks(chain: &ChainState) -> Vec<FastRecentBlock> {
    let start = chain
        .blocks
        .len()
        .saturating_sub(HF123_FAST_STATUS_RECENT_BLOCKS);
    chain.blocks[start..]
        .iter()
        .enumerate()
        .map(|(offset, block)| FastRecentBlock {
            height: start.saturating_add(offset) as u32,
            hash: block.block_hash(),
            version: block.header.version,
            time: block.header.time,
            tx_count: block.transactions.len(),
        })
        .collect()
}

fn total_work_value(chain: &ChainState) -> Result<BigUint> {
    BigUint::parse_bytes(chain.total_work_hex()?.as_bytes(), 16)
        .context("parse candidate total work")
}

fn parse_work(value: &str) -> Result<BigUint> {
    BigUint::parse_bytes(value.as_bytes(), 16).context("parse committed total work")
}

fn state_from_chain(
    chain: &ChainState,
    generation: u64,
    state_revision: u64,
    journal_bytes: u64,
) -> Result<FastStorageState> {
    let tip = chain.blocks.last().context("chain has no blocks")?;
    Ok(FastStorageState {
        schema_version: HF123_FAST_STORAGE_SCHEMA_VERSION,
        magic: HF123_FAST_STORAGE_MAGIC.to_string(),
        network: chain.network.clone(),
        generation,
        state_revision,
        committed_height: chain.height(),
        tip_hash: tip.block_hash(),
        tip_header: tip.header.clone(),
        tip_tx_count: tip.transactions.len(),
        journal_bytes,
        mempool_digest: mempool_digest(&chain.mempool),
        total_work_hex: chain.total_work_hex()?,
        utxos: sorted_utxos(chain),
        mempool: chain.mempool.as_ref().clone(),
        recent_blocks: recent_blocks(chain),
    })
}

fn pointer_from_state(
    blocks_file: String,
    state_file: String,
    state_sha256: String,
    state: &FastStorageState,
) -> FastStoragePointer {
    FastStoragePointer {
        schema_version: state.schema_version,
        magic: state.magic.clone(),
        network: state.network.clone(),
        generation: state.generation,
        state_revision: state.state_revision,
        blocks_file,
        state_file,
        state_sha256,
        journal_bytes: state.journal_bytes,
        committed_height: state.committed_height,
        tip_hash: state.tip_hash,
        tip_header: state.tip_header.clone(),
        tip_tx_count: state.tip_tx_count,
        mempool_digest: state.mempool_digest,
        total_work_hex: state.total_work_hex.clone(),
        committed_at_unix: now_secs(),
    }
}

fn atomic_replace_bytes(path: &Path, body: &[u8], sync_data: bool) -> Result<u64> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("qub-fce");
    let nonce = now_nanos_u64();
    let tmp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&tmp)
            .with_context(|| format!("create {}", tmp.display()))?;
        file.write_all(body)?;
        file.flush()?;
        if sync_data {
            file.sync_data()?;
        }
        drop(file);
        replace_file_atomic(&tmp, path, nonce)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result?;
    FAST_BYTES_WRITTEN.fetch_add(body.len() as u64, Ordering::Relaxed);
    Ok(body.len() as u64)
}

fn replace_file_atomic(tmp: &Path, path: &Path, nonce: u64) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("qub-fce");
    match fs::rename(tmp, path) {
        Ok(()) => {}
        Err(first_err) => {
            if !path.exists() {
                return Err(anyhow::Error::new(first_err))
                    .with_context(|| format!("replace {}", path.display()));
            }
            let backup = path.with_file_name(format!(
                ".{name}.{}.{}.bak",
                std::process::id(),
                nonce
            ));
            fs::rename(path, &backup).with_context(|| {
                format!(
                    "move existing {} aside after replacement failure: {first_err}",
                    path.display()
                )
            })?;
            if let Err(second_err) = fs::rename(tmp, path) {
                let _ = fs::rename(&backup, path);
                return Err(anyhow::Error::new(second_err))
                    .with_context(|| format!("install replacement {}", path.display()));
            }
            let _ = fs::remove_file(&backup);
        }
    }
    if let Some(parent) = path.parent() {
        if let Ok(dir) = File::open(parent) {
            let _ = dir.sync_all();
        }
    }
    Ok(())
}

fn atomic_replace_json<T: Serialize>(path: &Path, value: &T, sync_data: bool) -> Result<u64> {
    atomic_replace_bytes(path, &serde_json::to_vec(value)?, sync_data)
}

fn safe_child(paths: &NodePaths, name: &str) -> Result<PathBuf> {
    let candidate = Path::new(name);
    if candidate.components().count() != 1
        || candidate.file_name().and_then(|v| v.to_str()) != Some(name)
    {
        bail!("invalid Fast Chain Engine file name");
    }
    Ok(paths.fast_storage_dir.join(candidate))
}

fn lock_owner_alive(path: &Path) -> bool {
    let Ok(raw) = fs::read_to_string(path) else {
        return false;
    };
    let pid = raw
        .split_whitespace()
        .find_map(|part| part.strip_prefix("pid="))
        .and_then(|value| value.parse::<u32>().ok());
    let Some(pid) = pid else {
        return false;
    };
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        Path::new("/proc").join(pid.to_string()).exists()
    }
    #[cfg(windows)]
    {
        // HF123: Windows does not expose /proc. Query the process table only
        // after the lock has exceeded the conservative stale age. Failure to
        // query is treated as "alive" so an uncertain result never permits a
        // second writer. CREATE_NO_WINDOW prevents a console flash in the GUI.
        let filter = format!("PID eq {pid}");
        let output = Command::new("tasklist.exe")
            .args(["/FI", filter.as_str(), "/NH"])
            .creation_flags(0x0800_0000)
            .output();
        let Ok(output) = output else {
            return true;
        };
        String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            fields.get(1).and_then(|value| value.parse::<u32>().ok()) == Some(pid)
        })
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        // Unknown platforms fail closed: never remove a possibly live lock.
        true
    }
}

fn initialize_created_lease<W: Write>(path: PathBuf, mut file: W) -> Result<FastStorageLease> {
    // Ownership starts at create_new, before any fallible initialization. A
    // disk-full/write/flush failure must not strand WRITE.lock in a live node.
    let lease = FastStorageLease { path };
    let initialized = (|| -> std::io::Result<()> {
        writeln!(
            file,
            "pid={} created_at_unix={}",
            std::process::id(),
            now_secs()
        )?;
        file.flush()
    })();
    // Close the handle before removing a failed lease, including on Windows.
    drop(file);
    initialized?;
    Ok(lease)
}

fn acquire_lease(paths: &NodePaths) -> Result<FastStorageLease> {
    fs::create_dir_all(&paths.fast_storage_dir)?;
    for _ in 0..2 {
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&paths.fast_storage_lock_file)
        {
            Ok(file) => {
                return initialize_created_lease(paths.fast_storage_lock_file.clone(), file);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = fs::metadata(&paths.fast_storage_lock_file)
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                    .map(|age| age.as_secs() >= HF123_STORAGE_LOCK_STALE_SECS)
                    .unwrap_or(false);
                if stale && !lock_owner_alive(&paths.fast_storage_lock_file) {
                    let _ = fs::remove_file(&paths.fast_storage_lock_file);
                    continue;
                }
                bail!(
                    "Fast Chain Engine writer lock is active at {}. Run only one state-changing QUB process per data directory; remove the lock only after confirming its owner is gone",
                    paths.fast_storage_lock_file.display()
                );
            }
            Err(err) => return Err(err.into()),
        }
    }
    bail!("could not acquire Fast Chain Engine writer lock")
}

fn validate_pointer(settings: &Settings, pointer: &FastStoragePointer) -> Result<()> {
    if pointer.schema_version != HF123_FAST_STORAGE_SCHEMA_VERSION
        || pointer.magic != HF123_FAST_STORAGE_MAGIC
        || pointer.network != settings.network.name
    {
        bail!("Fast Chain Engine pointer schema/network mismatch");
    }
    if pointer.committed_height < MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT
        && settings.network.name == "mainnet"
        && pointer.committed_height > 0
    {
        // Pre-checkpoint chains are valid during initial sync; no rejection.
    }
    let _ = safe_child(&NodePaths::from_settings(settings), &pointer.blocks_file)?;
    let _ = safe_child(&NodePaths::from_settings(settings), &pointer.state_file)?;
    Ok(())
}

fn read_pointer_path(
    paths: &NodePaths,
    settings: &Settings,
    path: &Path,
) -> Result<FastStoragePointer> {
    let file = File::open(path).with_context(|| format!("open {}", path.display()))?;
    let pointer: FastStoragePointer = serde_json::from_reader(BufReader::new(file))?;
    validate_pointer(settings, &pointer)?;
    // Ensure paths resolve below the configured storage root.
    let _ = safe_child(paths, &pointer.blocks_file)?;
    let _ = safe_child(paths, &pointer.state_file)?;
    Ok(pointer)
}

fn read_pointer_with_recovery(
    paths: &NodePaths,
    settings: &Settings,
) -> Result<(FastStoragePointer, bool)> {
    let mut last_error = None;
    for attempt in 0..4u64 {
        match read_pointer_path(paths, settings, &paths.fast_pointer_file) {
            Ok(pointer) => return Ok((pointer, false)),
            Err(err) => {
                last_error = Some(err);
                if attempt < 3 {
                    std::thread::sleep(Duration::from_millis(10 + attempt * 20));
                }
            }
        }
    }
    if paths.fast_previous_pointer_file.exists() {
        let pointer = read_pointer_path(paths, settings, &paths.fast_previous_pointer_file)
            .context("CURRENT.json failed and PREVIOUS.json is not usable")?;
        return Ok((pointer, true));
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Fast Chain Engine pointer unavailable")))
}

fn read_state(
    paths: &NodePaths,
    settings: &Settings,
    pointer: &FastStoragePointer,
) -> Result<(FastStorageState, u64)> {
    let path = safe_child(paths, &pointer.state_file)?;
    let body = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    FAST_BYTES_READ.fetch_add(body.len() as u64, Ordering::Relaxed);
    let actual = sha256_hex(&body);
    if actual != pointer.state_sha256 {
        bail!(
            "Fast Chain Engine state checksum mismatch: expected {}, got {}",
            pointer.state_sha256,
            actual
        );
    }
    let state: FastStorageState = serde_json::from_slice(&body)?;
    if state.schema_version != HF123_FAST_STORAGE_SCHEMA_VERSION
        || state.magic != HF123_FAST_STORAGE_MAGIC
        || state.network != settings.network.name
        || state.generation != pointer.generation
        || state.state_revision != pointer.state_revision
        || state.committed_height != pointer.committed_height
        || state.tip_hash != pointer.tip_hash
        || state.tip_header != pointer.tip_header
        || state.journal_bytes != pointer.journal_bytes
        || state.mempool_digest != pointer.mempool_digest
        || state.total_work_hex != pointer.total_work_hex
    {
        bail!("Fast Chain Engine state/pointer mismatch");
    }
    if mempool_digest(&state.mempool) != state.mempool_digest {
        bail!("Fast Chain Engine mempool digest mismatch");
    }
    Ok((state, body.len() as u64))
}

fn read_pointer_state_with_recovery(
    paths: &NodePaths,
    settings: &Settings,
) -> Result<(FastStoragePointer, FastStorageState, u64, bool)> {
    let current_result = (|| -> Result<(FastStoragePointer, FastStorageState, u64)> {
        let mut last_error = None;
        for _ in 0..4 {
            let pointer = read_pointer_path(paths, settings, &paths.fast_pointer_file)?;
            match read_state(paths, settings, &pointer) {
                Ok((state, bytes)) => return Ok((pointer, state, bytes)),
                Err(err) => last_error = Some(err),
            }
            // A concurrent commit+cleanup may have retired the selected state
            // before its file was opened. Re-capture CURRENT before falling
            // back to PREVIOUS; never return a fabricated/stale identity.
        }
        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("committed state unavailable")))
    })();

    match current_result {
        Ok((pointer, state, bytes)) => Ok((pointer, state, bytes, false)),
        Err(current_error) if paths.fast_previous_pointer_file.exists() => {
            let pointer = read_pointer_path(
                paths,
                settings,
                &paths.fast_previous_pointer_file,
            )
            .with_context(|| {
                format!(
                    "CURRENT Fast Chain Engine state is unusable ({current_error:#}); PREVIOUS pointer is also unusable"
                )
            })?;
            let (state, bytes) = read_state(paths, settings, &pointer).with_context(|| {
                format!(
                    "CURRENT Fast Chain Engine state is unusable ({current_error:#}); PREVIOUS state is also unusable"
                )
            })?;
            Ok((pointer, state, bytes, true))
        }
        Err(current_error) => Err(current_error)
            .context("CURRENT Fast Chain Engine state is unusable and PREVIOUS.json is unavailable"),
    }
}

fn read_blocks(
    settings: &Settings,
    pointer: &FastStoragePointer,
    state: &FastStorageState,
    file: File,
) -> Result<Vec<Block>> {
    let metadata = file.metadata()?;
    if metadata.len() < pointer.journal_bytes {
        bail!(
            "Fast Chain Engine journal is shorter than committed prefix: {} < {}",
            metadata.len(),
            pointer.journal_bytes
        );
    }
    let expected = pointer.committed_height as usize + 1;
    let mut blocks = Vec::with_capacity(expected);
    let mut reader = BufReader::new(file.take(pointer.journal_bytes));
    let mut line = Vec::new();
    let mut previous = Hash256::zero();
    let genesis = genesis_block(settings)?;
    loop {
        line.clear();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        FAST_BYTES_READ.fetch_add(read as u64, Ordering::Relaxed);
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if line.is_empty() {
            bail!("empty Fast Chain Engine block journal record");
        }
        let block: Block = serde_json::from_slice(&line)
            .with_context(|| format!("parse block journal record #{}", blocks.len()))?;
        let height = blocks.len() as u32;
        let expected_version = expected_block_version(settings, height);
        if block.header.version != expected_version {
            bail!(
                "Fast Chain Engine block version mismatch at #{height}: expected {expected_version}, got {}",
                block.header.version
            );
        }
        if height == 0 {
            if block != genesis {
                bail!("Fast Chain Engine genesis mismatch");
            }
        } else if block.header.prev_block_hash != previous {
            bail!("Fast Chain Engine hash-link mismatch at #{height}");
        }
        previous = block.block_hash();
        blocks.push(block);
    }
    if blocks.len() != expected {
        bail!(
            "Fast Chain Engine block count mismatch: expected {expected}, got {}",
            blocks.len()
        );
    }
    if previous != pointer.tip_hash || previous != state.tip_hash {
        bail!("Fast Chain Engine tip hash mismatch");
    }
    if blocks
        .last()
        .map(|block| block.header.clone())
        .as_ref()
        != Some(&pointer.tip_header)
    {
        bail!("Fast Chain Engine tip header mismatch");
    }
    if let Some(expected_hash) =
        consensus_checkpoint_hash(settings, MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT)
    {
        if let Some(block) = blocks.get(MAINNET_FORK_SAFETY_CHECKPOINT_HEIGHT as usize) {
            if block.block_hash().to_string() != expected_hash {
                bail!("Fast Chain Engine fork-safety checkpoint mismatch");
            }
        }
    }
    Ok(blocks)
}

fn chain_from_state(
    blocks: Vec<Block>,
    state: FastStorageState,
    settings: &Settings,
    full_validate: bool,
) -> Result<ChainState> {
    let mut utxos = HashMap::with_capacity(state.utxos.len());
    for record in state.utxos {
        if utxos.insert(record.outpoint, record.coin).is_some() {
            bail!("duplicate Fast Chain Engine persisted UTXO");
        }
    }
    let chain = ChainState {
        network: state.network,
        blocks: Arc::new(blocks),
        utxos: Arc::new(utxos),
        mempool: Arc::new(state.mempool),
    };
    if full_validate {
        chain.validate_all(settings)?;
    }
    Ok(chain)
}

fn load_fast_chain_from_pointer_path(
    settings: &Settings,
    paths: &NodePaths,
    pointer_path: &Path,
    full_validate: bool,
) -> Result<(ChainState, FastStorageIdentity)> {
    let pointer = read_pointer_path(paths, settings, pointer_path)?;
    // Pin the journal before potentially lengthy state deserialization. Once
    // open it survives Unix unlink; Windows denies deletion of an open file.
    // If either open loses a cleanup race, load_fast_chain retries CURRENT.
    let journal = File::open(safe_child(paths, &pointer.blocks_file)?)?;
    let (state, _) = read_state(paths, settings, &pointer)?;
    let blocks = read_blocks(settings, &pointer, &state, journal)?;
    let chain = chain_from_state(blocks, state, settings, full_validate)?;
    Ok((chain, pointer.identity()))
}

fn load_fast_chain_once(
    settings: &Settings,
    paths: &NodePaths,
    full_validate: bool,
) -> Result<(ChainState, FastStorageIdentity, bool)> {
    let mut current_result = load_fast_chain_from_pointer_path(
        settings, paths, &paths.fast_pointer_file, full_validate,
    );
    for _ in 0..3 {
        super::p2p::hf132_check_sync_budget()?;
        let missing_file = current_result.as_ref().err().map(|err| {
            err.chain().any(|cause| cause.downcast_ref::<std::io::Error>()
                .map(|io| io.kind() == std::io::ErrorKind::NotFound).unwrap_or(false))
        }).unwrap_or(false);
        if !missing_file {
            break;
        }
        // A retired file is a capture race, not evidence CURRENT is corrupt.
        current_result = load_fast_chain_from_pointer_path(
            settings, paths, &paths.fast_pointer_file, full_validate,
        );
    }
    // Cooperative cancellation is not evidence that CURRENT is corrupt.
    // Propagate it before recovery can load/replay PREVIOUS instead.
    super::p2p::hf132_check_sync_budget()?;
    match current_result {
        Ok((chain, identity)) => {
            FAST_LOADS.fetch_add(1, Ordering::Relaxed);
            Ok((chain, identity, false))
        }
        Err(current_error) if paths.fast_previous_pointer_file.exists() => {
            let (chain, identity) = load_fast_chain_from_pointer_path(
                settings,
                paths,
                &paths.fast_previous_pointer_file,
                full_validate,
            )
            .with_context(|| {
                format!(
                    "CURRENT Fast Chain Engine commit is unusable ({current_error:#}); PREVIOUS commit is also unusable"
                )
            })?;
            FAST_LOADS.fetch_add(1, Ordering::Relaxed);
            Ok((chain, identity, true))
        }
        Err(current_error) => Err(current_error)
            .context("CURRENT Fast Chain Engine commit is unusable and PREVIOUS.json is unavailable"),
    }
}

pub(crate) fn load_fast_chain(
    settings: &Settings,
    paths: &NodePaths,
    full_validate: bool,
) -> Result<(ChainState, FastStorageIdentity)> {
    let started = Instant::now();
    let mut last_error = None;
    for attempt in 0..4u64 {
        super::p2p::hf132_check_sync_budget()?;
        match load_fast_chain_once(settings, paths, full_validate) {
            Ok((chain, identity, used_previous)) => {
                if used_previous {
                    eprintln!(
                        "warning: Fast Chain Engine CURRENT.json was unusable; recovered from PREVIOUS.json at #{} ({})",
                        identity.committed_height,
                        identity.tip_hash
                    );
                }
                let millis = elapsed_millis(started);
                FAST_LAST_LOAD_MILLIS.store(millis, Ordering::Relaxed);
                record_max(&FAST_MAX_LOAD_MILLIS, millis);
                return Ok((chain, identity));
            }
            Err(err) => {
                super::p2p::hf132_check_sync_budget()?;
                last_error = Some(err);
                if attempt < 3 {
                    std::thread::sleep(Duration::from_millis(15 + attempt * 25));
                }
            }
        }
    }
    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Fast Chain Engine load failed")))
}

fn write_full_journal(path: &Path, blocks: &[Block]) -> Result<u64> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("blocks.jsonl");
    let nonce = now_nanos_u64();
    let tmp = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = (|| -> Result<u64> {
        let file = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        let mut writer = BufWriter::new(file);
        for block in blocks {
            serde_json::to_writer(&mut writer, block)?;
            writer.write_all(b"\n")?;
        }
        writer.flush()?;
        let file = writer.into_inner().map_err(|err| err.into_error())?;
        let bytes = file.metadata()?.len();
        file.sync_data()?;
        drop(file);
        replace_file_atomic(&tmp, path, nonce)?;
        FAST_BYTES_WRITTEN.fetch_add(bytes, Ordering::Relaxed);
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn append_journal(
    path: &Path,
    committed_bytes: u64,
    blocks: &[Block],
) -> Result<u64> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .with_context(|| format!("open {}", path.display()))?;
    let actual = file.metadata()?.len();
    if actual < committed_bytes {
        bail!(
            "Fast Chain Engine journal shortened below committed prefix: {actual} < {committed_bytes}"
        );
    }
    if actual > committed_bytes {
        file.set_len(committed_bytes)?;
    }
    file.seek(SeekFrom::Start(committed_bytes))?;
    let mut writer = BufWriter::new(file);
    for block in blocks {
        serde_json::to_writer(&mut writer, block)?;
        writer.write_all(b"\n")?;
    }
    writer.flush()?;
    let file = writer.into_inner().map_err(|err| err.into_error())?;
    let bytes = file.metadata()?.len();
    file.sync_data()?;
    FAST_BYTES_WRITTEN.fetch_add(bytes.saturating_sub(committed_bytes), Ordering::Relaxed);
    Ok(bytes)
}

fn write_state_and_pointer(
    paths: &NodePaths,
    previous: Option<&FastStoragePointer>,
    blocks_file: String,
    state: FastStorageState,
) -> Result<FastStoragePointer> {
    let state_file = format!(
        "state-{:020}-{:020}.json",
        state.generation, state.state_revision
    );
    let state_path = safe_child(paths, &state_file)?;
    let state_body = serde_json::to_vec(&state)?;
    let state_sha = sha256_hex(&state_body);
    atomic_replace_bytes(&state_path, &state_body, true)?;
    let pointer = pointer_from_state(blocks_file, state_file, state_sha, &state);
    if let Some(previous) = previous {
        atomic_replace_json(&paths.fast_previous_pointer_file, previous, true)?;
    }
    atomic_replace_json(&paths.fast_pointer_file, &pointer, true)?;
    // HF130: CURRENT is the durable commit point. There must be no cleanup,
    // status serialization, or export I/O between this rename and live publish.
    // The caller enqueues maintenance only after publishing the committed chain.
    Ok(pointer)
}

fn old_file_candidates(paths: &NodePaths, prefix: &str, suffix: &str, retain: usize) -> Result<Vec<PathBuf>> {
    // Directory walking, metadata I/O and sorting never hold the canonical
    // storage mutex. Eligibility is checked again under the writer lease.
    let mut files = fs::read_dir(&paths.fast_storage_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(prefix) && name.ends_with(suffix) {
                let modified = entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .unwrap_or(UNIX_EPOCH);
                Some((modified, name, entry.path()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(files.into_iter().skip(retain).map(|(_, _, path)| path).collect())
}

fn cleanup_candidates_if_idle(settings: &Settings, paths: &NodePaths, candidates: &[PathBuf]) -> Result<bool> {
    // Normal saves already hold this mutex before taking the on-disk lease.
    // Try-lock makes maintenance yield to a queued/active canonical writer.
    let Ok(_storage) = super::storage_lock().try_lock() else {
        return Ok(false);
    };
    let _lease = acquire_lease(paths)?;
    let current = read_pointer_path(paths, settings, &paths.fast_pointer_file)?;
    let mut keep = HashSet::<String>::new();
    keep.insert(current.state_file.clone());
    keep.insert(current.blocks_file.clone());
    if paths.fast_previous_pointer_file.exists() {
        // A corrupt/unreadable PREVIOUS makes cleanup fail closed. Never guess
        // which immutable files a recovery reader may still need.
        let previous = read_pointer_path(paths, settings, &paths.fast_previous_pointer_file)?;
        keep.insert(previous.state_file);
        keep.insert(previous.blocks_file);
    }
    // Bound the number of filesystem mutations per lease. Open reader handles
    // remain valid on Unix; Windows may refuse deletion, which is harmless.
    for path in candidates.iter().take(8) {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !keep.contains(name) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(true)
}

#[derive(Default)]
struct PostCommitWorkers {
    running: HashSet<PathBuf>,
    pending: HashSet<PathBuf>,
}

static CLI_INLINE_STORAGE_MAINTENANCE: AtomicBool = AtomicBool::new(false);

/// Select once at finite CLI startup, before any chain access or worker starts.
/// Such commands finish metadata/cleanup after live publication in their own
/// thread. Full compatibility exports remain explicit. GUI and long-running
/// services keep the default detached maintenance/export policy.
pub fn use_inline_storage_maintenance_for_cli() {
    CLI_INLINE_STORAGE_MAINTENANCE.store(true, Ordering::SeqCst);
}

fn post_commit_workers() -> &'static StdMutex<PostCommitWorkers> {
    static WORKERS: OnceLock<StdMutex<PostCommitWorkers>> = OnceLock::new();
    WORKERS.get_or_init(|| StdMutex::new(PostCommitWorkers::default()))
}

fn run_post_commit_maintenance(settings: &Settings) -> Result<()> {
    let paths = NodePaths::from_settings(settings);
    #[cfg(test)]
    hf130_maintenance_tests::before_maintenance(&paths)?;

    // Capture/serialize status without the storage mutex. status-fast reads
    // CURRENT/state itself, so this compatibility cache is never authoritative.
    let (pointer, _) = read_pointer_with_recovery(&paths, settings)?;
    if let Err(err) = write_status_from_pointer(&paths, &pointer) {
        eprintln!("warning: HF130 detached status refresh failed: {err:#}");
    }
    let mut candidates = old_file_candidates(&paths, "state-", ".json", 4)?;
    candidates.extend(old_file_candidates(&paths, "blocks-", ".jsonl", 3)?);
    if !candidates.is_empty() {
        cleanup_candidates_if_idle(settings, &paths, &candidates)?;
    }
    Ok(())
}

/// Call only after the committed chain has been published to the live owner
/// (or queued there). The default detached scheduler performs no filesystem
/// I/O. Finite CLI commands instead finish one maintenance pass before exit;
/// errors remain non-fatal because CURRENT has already committed.
pub(crate) fn schedule_post_commit_maintenance(settings: &Settings) {
    schedule_post_commit_maintenance_in_mode(
        settings,
        CLI_INLINE_STORAGE_MAINTENANCE.load(Ordering::SeqCst),
    );
}

fn schedule_post_commit_maintenance_in_mode(settings: &Settings, inline: bool) {
    if inline {
        if let Err(err) = run_post_commit_maintenance(settings) {
            eprintln!("warning: HF130 CLI storage maintenance failed; CURRENT remains committed: {err:#}");
        }
        return;
    }
    let key = NodePaths::from_settings(settings).fast_storage_dir;
    {
        let Ok(mut workers) = post_commit_workers().lock() else {
            return;
        };
        if !workers.running.insert(key.clone()) {
            workers.pending.insert(key);
            return;
        }
    }
    let worker_key = key.clone();
    let settings = settings.clone();
    let spawn = thread::Builder::new()
        .name("qub-post-commit".to_string())
        .spawn(move || loop {
            if let Err(err) = run_post_commit_maintenance(&settings) {
                eprintln!("warning: HF130 detached storage maintenance failed; CURRENT remains committed: {err:#}");
            }
            // Even the legacy scheduler reads pointer/status files, so it also
            // belongs in this detached worker, never the caller's save path.
            schedule_legacy_export(&settings);
            let Ok(mut workers) = post_commit_workers().lock() else {
                break;
            };
            if workers.pending.remove(&worker_key) {
                continue;
            }
            workers.running.remove(&worker_key);
            break;
        });
    if let Err(err) = spawn {
        if let Ok(mut workers) = post_commit_workers().lock() {
            workers.running.remove(&key);
            workers.pending.remove(&key);
        }
        eprintln!("warning: HF130 could not start detached storage maintenance: {err}");
    }
}

fn initialize_fast_storage(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
    legacy_is_current: bool,
    validate_before_commit: bool,
) -> Result<FastStorageIdentity> {
    if validate_before_commit {
        chain.validate_all(settings)?;
    }
    let _lease = acquire_lease(paths)?;
    let generation = now_nanos_u64().max(1);
    let blocks_file = format!("blocks-{generation:020}.jsonl");
    let blocks_path = safe_child(paths, &blocks_file)?;
    let journal_bytes = write_full_journal(&blocks_path, &chain.blocks)?;
    let state = state_from_chain(chain, generation, 1, journal_bytes)?;
    if legacy_is_current {
        let legacy = LegacyExportStatus {
            schema_version: 1,
            network: settings.network.name.clone(),
            height: chain.height(),
            tip_hash: chain.tip_hash(),
            exported_at_unix: now_secs(),
        };
        let _ = atomic_replace_json(&paths.fast_legacy_export_status_file, &legacy, true);
    }
    let pointer = write_state_and_pointer(paths, None, blocks_file, state)?;
    FAST_COMMITS.fetch_add(1, Ordering::Relaxed);
    Ok(pointer.identity())
}

pub(crate) fn migrate_legacy_chain(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
) -> Result<FastStorageIdentity> {
    if paths.fast_storage_exists() {
        // Fail closed once QUB-FCE-1 exists. CURRENT/PREVIOUS recovery is
        // handled by load_fast_chain(); silently replacing an existing engine
        // from an infrequently refreshed compatibility export could regress the
        // local canonical tip. Explicit operator recovery must be used instead.
        return load_fast_chain(settings, paths, true)
            .map(|(_, identity)| identity)
            .context("existing Fast Chain Engine is unusable; automatic legacy re-migration is disabled");
    }
    initialize_fast_storage(settings, paths, chain, true, false)
}

fn ensure_candidate_not_behind_committed(
    settings: &Settings,
    chain: &ChainState,
    current: &FastStoragePointer,
    policy: FastCommitPolicy,
) -> Result<()> {
    let candidate_work = total_work_value(chain)?;
    let committed_work = parse_work(&current.total_work_hex)?;
    let candidate_height = chain.height();
    let candidate_tip = chain.tip_hash();
    let equal_work_sibling = settings.network.name == "mainnet"
        && candidate_work == committed_work
        && candidate_height == current.committed_height
        && candidate_tip != current.tip_hash;
    let verified_equal_work_reanchor =
        policy == FastCommitPolicy::VerifiedEqualWorkReanchor && equal_work_sibling;
    let stale = candidate_work < committed_work
        || (candidate_work == committed_work && candidate_height < current.committed_height)
        || (equal_work_sibling && !verified_equal_work_reanchor);
    if stale {
        bail!(
            "stale Fast Chain Engine persistence rejected: candidate #{} {} work={} cannot replace committed #{} {} work={}",
            candidate_height,
            candidate_tip,
            candidate_work.to_str_radix(16),
            current.committed_height,
            current.tip_hash,
            committed_work.to_str_radix(16),
        );
    }
    if policy == FastCommitPolicy::VerifiedEqualWorkReanchor
        && !verified_equal_work_reanchor
    {
        bail!(
            "verified equal-work re-anchor rejected: candidate #{} {} must be a different same-height/same-work mainnet tip from committed #{} {}",
            candidate_height,
            candidate_tip,
            current.committed_height,
            current.tip_hash,
        );
    }
    Ok(())
}

fn commit_chain_with_policy(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
    policy: FastCommitPolicy,
) -> Result<FastStorageIdentity> {
    let started = Instant::now();
    let _lease = acquire_lease(paths)?;
    let current = if paths.fast_storage_exists() {
        let (pointer, _, _, used_previous) =
            read_pointer_state_with_recovery(paths, settings)?;
        if used_previous {
            eprintln!(
                "warning: Fast Chain Engine writer recovered the last valid PREVIOUS commit at #{} ({})",
                pointer.committed_height,
                pointer.tip_hash
            );
        }
        ensure_candidate_not_behind_committed(settings, chain, &pointer, policy)?;
        Some(pointer)
    } else {
        None
    };

    let (generation, state_revision, blocks_file, journal_bytes) = match current.as_ref() {
        Some(pointer)
            if chain.height() >= pointer.committed_height
                && chain
                    .blocks
                    .get(pointer.committed_height as usize)
                    .map(Block::block_hash)
                    == Some(pointer.tip_hash) =>
        {
            let path = safe_child(paths, &pointer.blocks_file)?;
            let first_new = pointer.committed_height as usize + 1;
            let bytes = append_journal(
                &path,
                pointer.journal_bytes,
                chain.blocks.get(first_new..).unwrap_or(&[]),
            )?;
            (
                pointer.generation,
                pointer.state_revision.saturating_add(1),
                pointer.blocks_file.clone(),
                bytes,
            )
        }
        _ => {
            let generation = current
                .as_ref()
                .map(|pointer| pointer.generation.saturating_add(1))
                .unwrap_or_else(|| now_nanos_u64().max(1));
            let blocks_file = format!("blocks-{generation:020}.jsonl");
            let path = safe_child(paths, &blocks_file)?;
            let bytes = write_full_journal(&path, &chain.blocks)?;
            (generation, 1, blocks_file, bytes)
        }
    };

    let state = state_from_chain(chain, generation, state_revision, journal_bytes)?;
    let pointer = write_state_and_pointer(paths, current.as_ref(), blocks_file, state)?;
    FAST_COMMITS.fetch_add(1, Ordering::Relaxed);
    let millis = elapsed_millis(started);
    FAST_LAST_COMMIT_MILLIS.store(millis, Ordering::Relaxed);
    record_max(&FAST_MAX_COMMIT_MILLIS, millis);
    Ok(pointer.identity())
}

pub(crate) fn commit_chain(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
) -> Result<FastStorageIdentity> {
    commit_chain_with_policy(settings, paths, chain, FastCommitPolicy::Normal)
}

pub(crate) fn commit_chain_verified_equal_work_reanchor(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
) -> Result<FastStorageIdentity> {
    commit_chain_with_policy(
        settings,
        paths,
        chain,
        FastCommitPolicy::VerifiedEqualWorkReanchor,
    )
}

fn fast_status_from_pointer(
    paths: &NodePaths,
    pointer: &FastStoragePointer,
    state: &FastStorageState,
    state_bytes: u64,
) -> FastChainStatus {
    let (legacy_bytes, legacy_secs, legacy_nanos) = fs::metadata(&paths.chain_file)
        .map(|metadata| {
            let (secs, nanos) = super::chain_file_stamp(&metadata);
            (metadata.len(), secs, nanos)
        })
        .unwrap_or((0, 0, 0));
    FastChainStatus {
        schema_version: FAST_CHAIN_STATUS_SCHEMA_VERSION,
        network: state.network.clone(),
        height: state.committed_height,
        tip_hash: state.tip_hash,
        tip_block_version: state.tip_header.version,
        tip_block_time: state.tip_header.time,
        tip_tx_count: state.tip_tx_count,
        chain_file_bytes: legacy_bytes,
        chain_file_modified_unix_secs: legacy_secs,
        chain_file_modified_subsec_nanos: legacy_nanos,
        generated_at_unix: now_secs(),
        storage_engine: HF123_FAST_STORAGE_MAGIC.to_string(),
        storage_generation: state.generation,
        state_revision: state.state_revision,
        primary_state_bytes: state_bytes,
        journal_bytes: pointer.journal_bytes,
        mempool_tx_count: state.mempool.len(),
        mempool_digest: state.mempool_digest,
        recent_blocks: state.recent_blocks.clone(),
    }
}

fn write_status_from_pointer(paths: &NodePaths, pointer: &FastStoragePointer) -> Result<()> {
    // Status metadata is derived from the immutable committed state. Failure is
    // non-fatal because status-fast can reconstruct it from pointer/state.
    let settings_network = pointer.network.clone();
    let state_path = safe_child(paths, &pointer.state_file)?;
    let body = fs::read(&state_path)?;
    let state: FastStorageState = serde_json::from_slice(&body)?;
    if state.network != settings_network {
        bail!("status state network mismatch");
    }
    let status = fast_status_from_pointer(paths, pointer, &state, body.len() as u64);
    atomic_replace_json(&paths.chain_status_file, &status, true)?;
    Ok(())
}

pub(crate) fn load_fast_status(
    settings: &Settings,
    paths: &NodePaths,
) -> Result<(FastChainStatus, FastChainStatusSource)> {
    let (pointer, state, state_bytes, used_previous) =
        read_pointer_state_with_recovery(paths, settings)?;
    if used_previous {
        eprintln!(
            "warning: status-fast recovered the last valid PREVIOUS Fast Chain Engine commit at #{} ({})",
            pointer.committed_height,
            pointer.tip_hash
        );
    }
    let status = fast_status_from_pointer(paths, &pointer, &state, state_bytes);
    let _ = atomic_replace_json(&paths.chain_status_file, &status, true);
    Ok((status, FastChainStatusSource::FastStorageMetadata))
}


/// Loads one committed chain view and returns the identity of the exact Fast
/// Chain Engine commit that produced it. Keeping both values in the same read
/// closes the identity-before-load race for cache users such as Explorer API.
pub fn load_committed_chain_with_identity(
    settings: &Settings,
    full_validate: bool,
) -> Result<(ChainState, Option<FastStorageIdentity>)> {
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.fast_storage_exists() {
        return load_fast_chain(settings, &paths, full_validate)
            .map(|(chain, identity)| (chain, Some(identity)));
    }
    if !paths.chain_file.exists() {
        return ChainState::new_with_genesis(settings).map(|chain| (chain, None));
    }
    let raw = fs::read_to_string(&paths.chain_file)
        .with_context(|| format!("read {}", paths.chain_file.display()))?;
    let persisted: PersistedChainState = serde_json::from_str(&raw)?;
    let chain = if full_validate {
        ChainState::from_persisted(persisted, settings)
    } else {
        ChainState::from_persisted_unchecked_for_ui(persisted, settings)
    }?;
    Ok((chain, None))
}

pub fn load_committed_chain(settings: &Settings, full_validate: bool) -> Result<ChainState> {
    load_committed_chain_with_identity(settings, full_validate).map(|(chain, _)| chain)
}

/// HF129 lightweight Explorer overlay for a mempool-only FCE revision. It reads
/// the exact immutable state file but never replays/deserializes the block
/// journal, preventing ordinary mempool churn from stalling every chain route.
pub fn load_committed_mempool_with_identity(
    settings: &Settings,
) -> Result<(Vec<Transaction>, Option<FastStorageIdentity>)> {
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.fast_storage_exists() {
        let (pointer, state, _, _) = read_pointer_state_with_recovery(&paths, settings)?;
        return Ok((state.mempool, Some(pointer.identity())));
    }
    let chain = load_committed_chain(settings, false)?;
    Ok((chain.mempool.as_ref().clone(), None))
}

pub fn fast_storage_identity(settings: &Settings) -> Result<Option<FastStorageIdentity>> {
    let paths = NodePaths::from_settings(settings);
    if !paths.fast_storage_exists() {
        return Ok(None);
    }
    let (pointer, _) = read_pointer_with_recovery(&paths, settings)?;
    Ok(Some(pointer.identity()))
}

pub fn fast_storage_stats(settings: &Settings) -> Result<FastStorageStats> {
    let paths = NodePaths::from_settings(settings);
    let (pointer, state, state_bytes, _) =
        read_pointer_state_with_recovery(&paths, settings)?;
    let journal_path = safe_child(&paths, &pointer.blocks_file)?;
    let journal_bytes = fs::metadata(&journal_path)?.len();
    let legacy_chain_bytes = fs::metadata(&paths.chain_file).map(|m| m.len()).unwrap_or(0);
    Ok(FastStorageStats {
        ok: true,
        storage_engine: HF123_FAST_STORAGE_MAGIC.to_string(),
        network: settings.network.name.clone(),
        data_dir: paths.data_dir.display().to_string(),
        fast_storage_dir: paths.fast_storage_dir.display().to_string(),
        generation: pointer.generation,
        state_revision: pointer.state_revision,
        height: pointer.committed_height,
        tip_hash: pointer.tip_hash.to_string(),
        tip_block_version: pointer.tip_header.version,
        journal_bytes,
        state_bytes,
        legacy_chain_bytes,
        mempool_tx_count: state.mempool.len(),
        mempool_digest: state.mempool_digest.to_string(),
        process_loads: FAST_LOADS.load(Ordering::Relaxed),
        process_commits: FAST_COMMITS.load(Ordering::Relaxed),
        process_bytes_read: FAST_BYTES_READ.load(Ordering::Relaxed),
        process_bytes_written: FAST_BYTES_WRITTEN.load(Ordering::Relaxed),
        process_legacy_exports: FAST_LEGACY_EXPORTS.load(Ordering::Relaxed),
        process_legacy_export_failures: FAST_LEGACY_EXPORT_FAILURES.load(Ordering::Relaxed),
        legacy_export_running: legacy_export_running_for(&paths),
        last_load_millis: FAST_LAST_LOAD_MILLIS.load(Ordering::Relaxed),
        max_load_millis: FAST_MAX_LOAD_MILLIS.load(Ordering::Relaxed),
        last_commit_millis: FAST_LAST_COMMIT_MILLIS.load(Ordering::Relaxed),
        max_commit_millis: FAST_MAX_COMMIT_MILLIS.load(Ordering::Relaxed),
    })
}

fn should_refresh_legacy_export(
    settings: &Settings,
    paths: &NodePaths,
    pointer: &FastStoragePointer,
) -> bool {
    if !paths.chain_file.exists() || !paths.fast_legacy_export_status_file.exists() {
        return true;
    }
    let Ok(file) = File::open(&paths.fast_legacy_export_status_file) else {
        return true;
    };
    let Ok(status) = serde_json::from_reader::<_, LegacyExportStatus>(BufReader::new(file)) else {
        return true;
    };
    if status.network != settings.network.name {
        return true;
    }
    pointer.committed_height.saturating_sub(status.height)
        >= HF123_LEGACY_EXPORT_BLOCK_INTERVAL
        || now_secs().saturating_sub(status.exported_at_unix)
            >= HF123_LEGACY_EXPORT_INTERVAL_SECS
}

#[derive(Default)]
struct LegacyExportWorkers {
    running: HashSet<PathBuf>,
    pending: HashSet<PathBuf>,
}

fn legacy_export_workers() -> &'static StdMutex<LegacyExportWorkers> {
    static WORKERS: OnceLock<StdMutex<LegacyExportWorkers>> = OnceLock::new();
    WORKERS.get_or_init(|| StdMutex::new(LegacyExportWorkers::default()))
}

fn legacy_export_running_for(paths: &NodePaths) -> bool {
    legacy_export_workers()
        .lock()
        .map(|workers| workers.running.contains(&paths.fast_storage_dir))
        .unwrap_or(false)
}

fn try_acquire_legacy_export_lease(paths: &NodePaths) -> Result<Option<FastStorageLease>> {
    fs::create_dir_all(&paths.fast_storage_dir)?;
    let path = paths.fast_storage_dir.join("LEGACY-EXPORT.lock");
    for attempt in 0..2 {
        match OpenOptions::new().create_new(true).write(true).open(&path) {
        Ok(mut file) => {
            // Own the lease immediately. If initializing or flushing the lock
            // file fails, Drop removes it instead of leaving a false permanent
            // exporter owner behind.
            let lease = FastStorageLease { path: path.clone() };
            writeln!(
                file,
                "pid={} created_at_unix={}",
                std::process::id(),
                now_secs()
            )?;
            file.flush()?;
            return Ok(Some(lease));
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            let stale = fs::metadata(&path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .map(|age| age.as_secs() >= HF123_STORAGE_LOCK_STALE_SECS)
                .unwrap_or(false);
            if stale && !lock_owner_alive(&path) {
                if attempt == 0 {
                    fs::remove_file(&path).with_context(|| {
                        format!("remove stale legacy export lease {}", path.display())
                    })?;
                    continue;
                }
            }
            // A different QUB process using this data directory is already
            // exporting. Do not wait and, critically, do not take WRITE.lock.
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
        }
    }
    Ok(None)
}

fn run_legacy_export_once(
    settings: &Settings,
    paths: &NodePaths,
) -> (Result<ChainJsonExportReport>, bool) {
    match try_acquire_legacy_export_lease(paths) {
        Ok(Some(_export_lease)) => (
            export_chain_json_to(settings, paths, &paths.chain_file).and_then(|report| {
                let status = LegacyExportStatus {
                    schema_version: 1,
                    network: settings.network.name.clone(),
                    height: report.height,
                    tip_hash: report.tip_hash,
                    exported_at_unix: now_secs(),
                };
                atomic_replace_json(&paths.fast_legacy_export_status_file, &status, true)?;
                Ok(report)
            }),
            false,
        ),
        Ok(None) => (
            Err(anyhow::anyhow!(
                "legacy export is already active in another process"
            )),
            true,
        ),
        Err(err) => (Err(err), false),
    }
}

/// HF129/v1.8.5 best-effort compatibility export scheduler.
///
/// Canonical durability is CURRENT + its immutable journal/state. `chain.json`
/// exists only for older tools, so its full-chain serialization must never sit
/// on the canonical storage mutex or delay publication of the live owner.
pub(crate) fn schedule_legacy_export(settings: &Settings) {
    if CLI_INLINE_STORAGE_MAINTENANCE.load(Ordering::SeqCst) {
        return;
    }
    let paths = NodePaths::from_settings(settings);
    let pointer = match read_pointer_with_recovery(&paths, settings) {
        Ok((pointer, _)) => pointer,
        Err(_) => return,
    };
    if !should_refresh_legacy_export(settings, &paths, &pointer) {
        return;
    }

    let key = paths.fast_storage_dir.clone();
    {
        let Ok(mut workers) = legacy_export_workers().lock() else {
            return;
        };
        if workers.running.contains(&key) {
            workers.pending.insert(key);
            return;
        }
        workers.running.insert(key.clone());
    }

    let settings = settings.clone();
    let spawn = thread::Builder::new()
        .name("qub-legacy-export".to_string())
        .spawn(move || {
            let paths = NodePaths::from_settings(&settings);
            // A detached compatibility export is best effort, but one transient
            // I/O failure or another process's lease must not drop the work
            // forever when there is no subsequent block save to retrigger it.
            // Retry in this detached thread with a bounded backoff; canonical
            // CURRENT/live publication is completely independent of this loop.
            // The final delay crosses the 15-minute stale-lease threshold, so a
            // crashed external exporter is recovered even if no later block save
            // occurs to schedule another worker.
            let retry_delays = [1u64, 3, 10, 30, HF123_STORAGE_LOCK_STALE_SECS + 5];
            let (mut result, mut coalesced_external) =
                run_legacy_export_once(&settings, &paths);
            for delay_secs in retry_delays {
                if result.is_ok() {
                    break;
                }
                thread::sleep(Duration::from_secs(delay_secs));
                (result, coalesced_external) = run_legacy_export_once(&settings, &paths);
            }

            match result {
                Ok(_) => {
                    FAST_LEGACY_EXPORTS.fetch_add(1, Ordering::Relaxed);
                }
                Err(err) => {
                    if !coalesced_external {
                        FAST_LEGACY_EXPORT_FAILURES.fetch_add(1, Ordering::Relaxed);
                        eprintln!(
                            "warning: HF129 detached legacy chain.json export failed; canonical CURRENT remains live: {err:#}"
                        );
                    }
                }
            }

            let pending = legacy_export_workers()
                .lock()
                .map(|mut workers| {
                    workers.running.remove(&key);
                    workers.pending.remove(&key)
                })
                .unwrap_or(false);
            if pending {
                schedule_legacy_export(&settings);
            }
        });
    if let Err(err) = spawn {
        if let Ok(mut workers) = legacy_export_workers().lock() {
            workers.running.remove(&paths.fast_storage_dir);
            workers.pending.remove(&paths.fast_storage_dir);
        }
        FAST_LEGACY_EXPORT_FAILURES.fetch_add(1, Ordering::Relaxed);
        eprintln!(
            "warning: HF129 could not start detached legacy export worker; canonical CURRENT remains live: {err}"
        );
    }
}

pub fn export_chain_json(settings: &Settings, output_path: &Path) -> Result<(u32, Hash256, u64)> {
    export_chain_json_report(settings, output_path)
        .map(|report| (report.height, report.tip_hash, report.bytes))
}

pub fn export_chain_json_report(
    settings: &Settings,
    output_path: &Path,
) -> Result<ChainJsonExportReport> {
    let paths = NodePaths::from_settings(settings);
    if paths.fast_storage_exists() {
        export_chain_json_to(settings, &paths, output_path)
    } else {
        let raw = fs::read(&paths.chain_file)
            .with_context(|| format!("read {}", paths.chain_file.display()))?;
        atomic_replace_bytes(output_path, &raw, true)?;
        let chain = super::load_or_init_chain_for_ui_fast(settings)?;
        Ok(ChainJsonExportReport {
            height: chain.height(),
            tip_hash: chain.tip_hash(),
            bytes: raw.len() as u64,
            source: "legacy".to_string(),
            storage_engine: "legacy-chain-json".to_string(),
            generation: 0,
            state_revision: 0,
            committed_at_unix: 0,
            recovered_from_previous: false,
        })
    }
}

fn export_chain_json_to(
    settings: &Settings,
    paths: &NodePaths,
    output_path: &Path,
) -> Result<ChainJsonExportReport> {
    let (pointer, state, recovered_from_previous, file) =
        open_export_commit(settings, paths)?;
    if file.metadata()?.len() < pointer.journal_bytes {
        bail!("Fast Chain Engine journal shorter than committed prefix");
    }
    if let Some(parent) = output_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let name = output_path
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("chain.json");
    let nonce = now_nanos_u64();
    let tmp = output_path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        nonce
    ));
    let result = (|| -> Result<u64> {
        let target = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        let mut writer = BufWriter::new(target);
        writer.write_all(b"{\"network\":")?;
        serde_json::to_writer(&mut writer, &state.network)?;
        writer.write_all(b",\"blocks\":[")?;
        let mut reader = BufReader::new(file.take(pointer.journal_bytes));
        let mut line = Vec::new();
        let expected = pointer.committed_height as usize + 1;
        for index in 0..expected {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 {
                bail!("Fast Chain Engine journal ended during export at block #{index}");
            }
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            if index > 0 {
                writer.write_all(b",")?;
            }
            writer.write_all(&line)?;
        }
        writer.write_all(b"],\"utxos\":")?;
        serde_json::to_writer(&mut writer, &state.utxos)?;
        writer.write_all(b",\"mempool\":")?;
        serde_json::to_writer(&mut writer, &state.mempool)?;
        writer.write_all(b"}\n")?;
        writer.flush()?;
        let file = writer.into_inner().map_err(|err| err.into_error())?;
        let bytes = file.metadata()?.len();
        file.sync_data()?;
        drop(file);
        replace_file_atomic(&tmp, output_path, nonce)?;
        FAST_BYTES_WRITTEN.fetch_add(bytes, Ordering::Relaxed);
        Ok(bytes)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    let bytes = result?;
    Ok(ChainJsonExportReport {
        height: pointer.committed_height,
        tip_hash: pointer.tip_hash,
        bytes,
        source: "fce".to_string(),
        storage_engine: HF123_FAST_STORAGE_MAGIC.to_string(),
        generation: pointer.generation,
        state_revision: pointer.state_revision,
        committed_at_unix: pointer.committed_at_unix,
        recovered_from_previous,
    })
}

fn open_export_commit(
    settings: &Settings,
    paths: &NodePaths,
) -> Result<(FastStoragePointer, FastStorageState, bool, File)> {
    let mut last_error = None;
    for _ in 0..4 {
        let (pointer, state, _, recovered) = read_pointer_state_with_recovery(paths, settings)?;
        match File::open(safe_child(paths, &pointer.blocks_file)?) {
            Ok(journal) => return Ok((pointer, state, recovered, journal)),
            Err(err) => last_error = Some(err),
        }
        // State is already owned in memory. If its journal was retired before
        // open, retry a whole captured commit; never mix a newer file with it.
    }
    Err(last_error.map(anyhow::Error::from)
        .unwrap_or_else(|| anyhow::anyhow!("committed export journal unavailable")))
}

fn live_registry() -> &'static StdMutex<HashMap<PathBuf, Weak<StdMutex<ChainState>>>> {
    static REGISTRY: OnceLock<StdMutex<HashMap<PathBuf, Weak<StdMutex<ChainState>>>>> =
        OnceLock::new();
    REGISTRY.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn registry_key(settings: &Settings) -> PathBuf {
    let path = PathBuf::from(&settings.node.data_dir);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    fs::canonicalize(&absolute).unwrap_or(absolute)
}

pub fn register_live_chain(settings: &Settings, chain: &Arc<StdMutex<ChainState>>) {
    let mut registry = live_registry().lock().expect("live chain registry poisoned");
    registry.insert(registry_key(settings), Arc::downgrade(chain));
}

pub fn unregister_live_chain(settings: &Settings) {
    let mut registry = live_registry().lock().expect("live chain registry poisoned");
    registry.remove(&registry_key(settings));
}

pub fn live_chain_arc(settings: &Settings) -> Option<Arc<StdMutex<ChainState>>> {
    let key = registry_key(settings);
    let mut registry = live_registry().lock().ok()?;
    let arc = registry.get(&key).and_then(Weak::upgrade);
    if arc.is_none() {
        registry.remove(&key);
    }
    arc
}

pub fn live_chain_snapshot(settings: &Settings) -> Option<ChainState> {
    let chain = live_chain_arc(settings)?;
    let snapshot = chain.lock().ok()?.clone();
    Some(snapshot)
}

/// Capture the copy-on-write live view without waiting for its writer. A busy
/// owner is distinct from an absent owner: UI callers must retry instead of
/// falling through to an expensive disk load or consensus replay.
pub fn try_live_chain_snapshot(settings: &Settings) -> Result<Option<ChainState>> {
    let Some(chain) = live_chain_arc(settings) else {
        return Ok(None);
    };
    let result = match chain.try_lock() {
        Ok(snapshot) => Ok(Some(snapshot.clone())),
        Err(std::sync::TryLockError::WouldBlock) => {
            bail!("live chain snapshot is busy; retry after the active writer")
        }
        Err(std::sync::TryLockError::Poisoned(_)) => bail!("live chain mutex poisoned"),
    };
    result
}

fn apply_live_candidate_locked(
    settings: &Settings,
    current: &mut ChainState,
    candidate: &ChainState,
) {
    if current.tip_hash() == candidate.tip_hash() {
        // Same-tip saves are commonly produced by GUI/CLI transaction
        // submission while the embedded node concurrently receives other
        // mempool entries. Never replace one side of that race blindly.
        let current_txids = current.mempool_txids();
        let candidate_txids = candidate.mempool_txids();
        if candidate_txids.is_subset(&current_txids) {
            return;
        }
        let candidates = current.reorg_mempool_candidates_for(candidate);
        let mut merged = current.clone();
        merged.rebuild_mempool_from(candidates, settings);
        *current = merged;
        return;
    }

    let candidate_work = total_work_value(candidate).ok();
    let current_work = total_work_value(current).ok();
    let should_publish = match (candidate_work, current_work) {
        (Some(candidate_work), Some(current_work)) => candidate_work > current_work,
        _ => candidate.height() > current.height(),
    };
    if should_publish {
        *current = candidate.clone();
    }
}

fn deferred_live_publications() -> &'static StdMutex<HashMap<PathBuf, ChainState>> {
    static PENDING: OnceLock<StdMutex<HashMap<PathBuf, ChainState>>> = OnceLock::new();
    PENDING.get_or_init(|| StdMutex::new(HashMap::new()))
}

fn deferred_live_workers() -> &'static StdMutex<HashSet<PathBuf>> {
    static WORKERS: OnceLock<StdMutex<HashSet<PathBuf>>> = OnceLock::new();
    WORKERS.get_or_init(|| StdMutex::new(HashSet::new()))
}

fn merge_deferred_candidate(settings: &Settings, existing: &mut ChainState, candidate: &ChainState) {
    if existing.tip_hash() == candidate.tip_hash() {
        apply_live_candidate_locked(settings, existing, candidate);
        return;
    }
    let candidate_work = total_work_value(candidate).ok();
    let existing_work = total_work_value(existing).ok();
    let replace = match (candidate_work, existing_work) {
        (Some(candidate_work), Some(existing_work)) => candidate_work > existing_work,
        _ => candidate.height() > existing.height(),
    };
    if replace {
        *existing = candidate.clone();
    }
}

fn spawn_deferred_live_worker(settings: Settings, key: PathBuf) {
    let should_spawn = {
        let Ok(mut workers) = deferred_live_workers().lock() else {
            return;
        };
        workers.insert(key.clone())
    };
    if !should_spawn {
        return;
    }

    thread::spawn(move || {
        loop {
            let candidate = deferred_live_publications()
                .lock()
                .ok()
                .and_then(|mut pending| pending.remove(&key));
            let Some(candidate) = candidate else {
                break;
            };
            let Some(live) = live_chain_arc(&settings) else {
                continue;
            };
            if let Ok(mut current) = live.lock() {
                apply_live_candidate_locked(&settings, &mut current, &candidate);
            };
        }

        if let Ok(mut workers) = deferred_live_workers().lock() {
            workers.remove(&key);
        }

        // Close the small insertion/removal race: a publisher may have queued a
        // candidate while this worker was finishing but still registered.
        let restart = deferred_live_publications()
            .lock()
            .map(|pending| pending.contains_key(&key))
            .unwrap_or(false);
        if restart {
            spawn_deferred_live_worker(settings, key);
        }
    });
}

pub(crate) fn publish_live_chain(settings: &Settings, candidate: &ChainState) {
    let Some(live) = live_chain_arc(settings) else {
        return;
    };
    if let Ok(mut current) = live.try_lock() {
        apply_live_candidate_locked(settings, &mut current, candidate);
        return;
    }

    // HF125/v1.8.2: a GUI/CLI block can be durably committed while the embedded
    // P2P owner briefly holds its mutex. HF123/HF124 silently dropped this live
    // publication, leaving persistent peer sessions advertising the old tip.
    // Queue one best candidate per data directory and publish it from a detached
    // single-flight worker as soon as the canonical mutex becomes available.
    let key = registry_key(settings);
    if let Ok(mut pending) = deferred_live_publications().lock() {
        match pending.get_mut(&key) {
            Some(existing) => merge_deferred_candidate(settings, existing, candidate),
            None => {
                pending.insert(key.clone(), candidate.clone());
            }
        }
    } else {
        return;
    }
    spawn_deferred_live_worker(settings.clone(), key);
}

/// HF126/v1.8.2 publishes a fully validated equal-work re-anchor synchronously
/// after its durable commit. The storage mutex is released by the caller before
/// this blocking live-owner update, avoiding the storage/live lock inversion.
pub(crate) fn publish_live_chain_verified_equal_work_reanchor(
    settings: &Settings,
    candidate: &ChainState,
) -> Result<()> {
    let Some(live) = live_chain_arc(settings) else {
        return Ok(());
    };
    let mut current = live
        .lock()
        .map_err(|_| anyhow::anyhow!("live chain mutex poisoned during verified re-anchor"))?;
    if current.tip_hash() == candidate.tip_hash() {
        apply_live_candidate_locked(settings, &mut current, candidate);
        return Ok(());
    }
    let candidate_work = total_work_value(candidate)?;
    let current_work = total_work_value(&current)?;

    // The durable commit happened before this live-owner publication. Another
    // thread may have advanced the live owner while we were waiting for its
    // mutex. Never replace a stronger live chain with the verified sibling.
    if current_work > candidate_work
        || (current_work == candidate_work && current.height() > candidate.height())
    {
        return Ok(());
    }

    // A delayed live owner may still be behind the already committed candidate.
    if candidate_work > current_work
        || (candidate_work == current_work && candidate.height() > current.height())
    {
        *current = candidate.clone();
        return Ok(());
    }

    let allowed_equal_height_sibling = settings.network.name == "mainnet"
        && candidate_work == current_work
        && candidate.height() == current.height()
        && candidate.tip_hash() != current.tip_hash();
    if !allowed_equal_height_sibling {
        bail!(
            "verified live re-anchor rejected: candidate #{} {} work={} does not safely replace current #{} {} work={}",
            candidate.height(),
            candidate.tip_hash(),
            candidate_work.to_str_radix(16),
            current.height(),
            current.tip_hash(),
            current_work.to_str_radix(16),
        );
    }
    *current = candidate.clone();
    Ok(())
}

pub fn live_chain_identity(settings: &Settings) -> Option<FastStorageIdentity> {
    let chain = live_chain_arc(settings)?;
    let chain = chain.lock().ok()?;
    let tip = chain.blocks.last()?;
    Some(FastStorageIdentity {
        schema_version: HF123_FAST_STORAGE_SCHEMA_VERSION,
        magic: HF123_FAST_STORAGE_MAGIC.to_string(),
        network: chain.network.clone(),
        generation: 0,
        state_revision: 0,
        committed_height: chain.height(),
        tip_hash: tip.block_hash(),
        tip_block_version: tip.header.version,
        mempool_digest: mempool_digest(&chain.mempool),
        total_work_hex: chain.total_work_hex().ok()?,
        committed_at_unix: now_secs(),
    })
}

pub(crate) fn initialize_new_chain(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
) -> Result<FastStorageIdentity> {
    initialize_fast_storage(settings, paths, chain, false, false)
}

pub(crate) fn fast_storage_file_prefix_hash(
    settings: &Settings,
) -> Result<Option<String>> {
    let paths = NodePaths::from_settings(settings);
    if !paths.fast_storage_exists() {
        return Ok(None);
    }
    let (pointer, _) = read_pointer_with_recovery(&paths, settings)?;
    let path = safe_child(&paths, &pointer.blocks_file)?;
    Ok(Some(hash_file_prefix(&path, pointer.journal_bytes)?))
}

#[cfg(test)]
mod hf131_lease_tests {
    use super::*;

    struct FaultyLeaseFile {
        file: File,
        fail_write: bool,
    }

    impl Write for FaultyLeaseFile {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if self.fail_write {
                return Err(std::io::Error::other("HF131 injected lease write failure"));
            }
            self.file.write(bytes)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("HF131 injected lease flush failure"))
        }
    }

    fn failed_initialization_releases_owned_lease(fail_write: bool) {
        let path = std::env::temp_dir().join(format!(
            "qub-hf131-lease-{}-{}.lock",
            std::process::id(),
            now_nanos_u64()
        ));
        let file = OpenOptions::new().create_new(true).write(true).open(&path).unwrap();
        let result = initialize_created_lease(path.clone(), FaultyLeaseFile { file, fail_write });
        let error = match result {
            Err(error) => error.to_string(),
            Ok(_) => panic!("injected initialization error was ignored"),
        };
        assert!(error.contains(if fail_write { "write failure" } else { "flush failure" }));
        assert!(!path.exists(), "failed initializer stranded its writer lease");

        // The next writer can acquire this exact path immediately; it does not
        // need a fifteen-minute timeout or manual deletion of an owned lock.
        let file = OpenOptions::new().create_new(true).write(true).open(&path).unwrap();
        let lease = initialize_created_lease(path.clone(), file).unwrap();
        assert!(path.exists());
        assert!(lock_owner_alive(&path));
        drop(lease);
        assert!(!path.exists());
    }

    #[test]
    fn hf131_writer_lease_write_failure_releases_path() {
        failed_initialization_releases_owned_lease(true);
    }

    #[test]
    fn hf131_writer_lease_flush_failure_releases_path() {
        failed_initialization_releases_owned_lease(false);
    }
}

#[cfg(test)]
mod hf130_maintenance_tests {
    use super::*;
    use std::sync::mpsc::{self, Receiver, Sender};

    struct TestHook {
        entered: Sender<()>,
        resume: StdMutex<Option<Receiver<()>>>,
        fail: bool,
    }

    fn hooks() -> &'static StdMutex<HashMap<PathBuf, Arc<TestHook>>> {
        static HOOKS: OnceLock<StdMutex<HashMap<PathBuf, Arc<TestHook>>>> = OnceLock::new();
        HOOKS.get_or_init(|| StdMutex::new(HashMap::new()))
    }

    pub(super) fn before_maintenance(paths: &NodePaths) -> Result<()> {
        let hook = hooks().lock().unwrap().get(&paths.fast_storage_dir).cloned();
        if let Some(hook) = hook {
            let resume = hook.resume.lock().unwrap().take();
            if let Some(resume) = resume {
                let _ = hook.entered.send(());
                let _ = resume.recv();
            }
            if hook.fail {
                bail!("HF130 injected post-CURRENT maintenance failure");
            }
        }
        Ok(())
    }

    struct BlockMaintenance {
        key: PathBuf,
        entered: Receiver<()>,
        resume: Option<Sender<()>>,
    }

    impl BlockMaintenance {
        fn new(settings: &Settings, fail: bool) -> Self {
            let key = NodePaths::from_settings(settings).fast_storage_dir;
            let (entered_tx, entered) = mpsc::channel();
            let (resume, resume_rx) = mpsc::channel();
            hooks().lock().unwrap().insert(key.clone(), Arc::new(TestHook {
                entered: entered_tx,
                resume: StdMutex::new(Some(resume_rx)),
                fail,
            }));
            Self { key, entered, resume: Some(resume) }
        }

        fn wait_until_blocked(&self) {
            self.entered.recv_timeout(Duration::from_secs(5))
                .expect("detached maintenance did not reach the injected delay");
        }
    }

    impl Drop for BlockMaintenance {
        fn drop(&mut self) {
            // Disconnect releases a blocked worker even when an assertion panics.
            self.resume.take();
            hooks().lock().unwrap().remove(&self.key);
        }
    }

    struct Fixture(Settings);

    impl Fixture {
        fn new(name: &str) -> Self {
            let mut settings: Settings = toml::from_str(include_str!("../config/regtest.toml")).unwrap();
            settings.node.data_dir = std::env::temp_dir().join(format!(
                "qub-hf130-{name}-{}-{}", std::process::id(), now_nanos_u64(),
            )).display().to_string();
            // Match node startup: the data directory exists before the live
            // owner is registered. On Windows canonicalize adds the verbatim
            // path prefix only once the directory exists; registering earlier
            // would store a different registry key from the first save lookup.
            NodePaths::from_settings(&settings).ensure_dirs().unwrap();
            Self(settings)
        }

        fn assert_live_owner(&self, expected: &Arc<StdMutex<ChainState>>) {
            let registered = live_chain_arc(&self.0)
                .expect("test fixture lost its registered live owner");
            assert!(Arc::ptr_eq(&registered, expected), "test fixture selected another live owner");
        }

        fn wait_for_workers(&self) {
            let key = NodePaths::from_settings(&self.0).fast_storage_dir;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let maintenance = post_commit_workers().lock().unwrap().running.contains(&key);
                let export = legacy_export_running_for(&NodePaths::from_settings(&self.0));
                if !maintenance && !export {
                    return;
                }
                assert!(Instant::now() < deadline, "detached storage workers did not finish");
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            unregister_live_chain(&self.0);
            // Avoid deleting a directory out from under a failed/slow worker.
            let key = NodePaths::from_settings(&self.0).fast_storage_dir;
            let running = post_commit_workers().lock().unwrap().running.contains(&key)
                || legacy_export_running_for(&NodePaths::from_settings(&self.0));
            if !running {
                let _ = fs::remove_dir_all(&self.0.node.data_dir);
            }
        }
    }

    fn extend(settings: &Settings, chain: &mut ChainState, count: usize) {
        let mut wallet = WalletFile::new(&settings.network.name);
        let key = wallet.create_key(settings, "HF130 test", 0).unwrap();
        let miner = Address::parse_with_prefix(&key.address, &settings.network.address_prefix).unwrap();
        for _ in 0..count {
            let block = mine_next_block(chain, settings, &miner, MiningOptions {
                duty_cycle_percent: 100,
                max_hashes: Some(5_000_000),
            }).unwrap();
            chain.connect_block(block, settings).unwrap();
        }
    }

    fn cleanup_for_test(settings: &Settings, paths: &NodePaths, candidates: &[PathBuf]) -> Result<()> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !cleanup_candidates_if_idle(settings, paths, candidates)? {
            assert!(Instant::now() < deadline, "test could not obtain idle storage mutex");
            thread::yield_now();
        }
        Ok(())
    }

    fn save_with_deadline(settings: &Settings, chain: &ChainState) {
        let settings = settings.clone();
        let chain = chain.clone();
        let (completed, result) = mpsc::channel();
        let worker = thread::spawn(move || {
            let _ = completed.send(save_chain(&settings, &chain));
        });
        result.recv_timeout(Duration::from_secs(5))
            .expect("canonical save waited for blocked post-commit maintenance")
            .unwrap();
        worker.join().unwrap();
    }

    #[test]
    fn hf130_live_publish_precedes_delayed_maintenance() {
        let fixture = Fixture::new("live-before-maintenance");
        let settings = &fixture.0;
        let genesis = ChainState::new_with_genesis(settings).unwrap();
        let live = Arc::new(StdMutex::new(genesis.clone()));
        register_live_chain(settings, &live);
        fixture.assert_live_owner(&live);
        let mut candidate = genesis;
        extend(settings, &mut candidate, 1);
        let blocked = BlockMaintenance::new(settings, false);
        save_with_deadline(settings, &candidate);
        blocked.wait_until_blocked();
        fixture.assert_live_owner(&live);
        assert_eq!(live.lock().unwrap().tip_hash(), candidate.tip_hash());
        assert_eq!(fast_storage_identity(settings).unwrap().unwrap().tip_hash, candidate.tip_hash());
        let paths = NodePaths::from_settings(settings);
        assert!(!paths.chain_status_file.exists(), "status should still be blocked");
        // A new durable commit also proceeds while the old maintenance pass is
        // delayed; the old pass must eventually capture the newer CURRENT.
        extend(settings, &mut candidate, 1);
        save_with_deadline(settings, &candidate);
        fixture.assert_live_owner(&live);
        assert_eq!(live.lock().unwrap().tip_hash(), candidate.tip_hash());
        drop(blocked);
        fixture.wait_for_workers();
        let (status, _) = load_fast_status(settings, &paths).unwrap();
        assert_eq!(status.tip_hash, candidate.tip_hash());
    }

    #[test]
    fn hf130_post_current_maintenance_error_does_not_fail_save() {
        let fixture = Fixture::new("maintenance-error");
        let settings = &fixture.0;
        let genesis = ChainState::new_with_genesis(settings).unwrap();
        let live = Arc::new(StdMutex::new(genesis.clone()));
        register_live_chain(settings, &live);
        fixture.assert_live_owner(&live);
        let mut candidate = genesis;
        extend(settings, &mut candidate, 1);
        let blocked = BlockMaintenance::new(settings, true);
        save_with_deadline(settings, &candidate);
        blocked.wait_until_blocked();
        fixture.assert_live_owner(&live);
        assert_eq!(live.lock().unwrap().tip_hash(), candidate.tip_hash());
        assert_eq!(load_committed_chain(settings, true).unwrap().tip_hash(), candidate.tip_hash());
        drop(blocked);
        fixture.wait_for_workers();
        assert_eq!(fast_storage_identity(settings).unwrap().unwrap().tip_hash, candidate.tip_hash());
    }

    #[test]
    fn hf130_inline_cli_maintenance_failure_retains_committed_tip() {
        let fixture = Fixture::new("inline-cli-maintenance-error");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let mut chain = ChainState::new_with_genesis(settings).unwrap();
        extend(settings, &mut chain, 1);
        initialize_new_chain(settings, &paths, &chain).unwrap();
        let mut failure = BlockMaintenance::new(settings, true);
        // Select the real inline branch without changing process-global mode
        // while the other library tests execute concurrently.
        drop(failure.resume.take());
        schedule_post_commit_maintenance_in_mode(settings, true);
        failure.wait_until_blocked();
        assert_eq!(load_committed_chain(settings, true).unwrap().tip_hash(), chain.tip_hash());
        assert!(!post_commit_workers().lock().unwrap().running.contains(&paths.fast_storage_dir));
        assert!(!legacy_export_running_for(&paths));
        assert!(!paths.fast_storage_lock_file.exists());
        assert!(!paths.fast_storage_dir.join("LEGACY-EXPORT.lock").exists());
    }

    #[test]
    fn hf130_cleanup_rechecks_current_previous() {
        let fixture = Fixture::new("cleanup-pointers");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let chain = ChainState::new_with_genesis(settings).unwrap();
        initialize_new_chain(settings, &paths, &chain).unwrap();
        let old = read_pointer_path(&paths, settings, &paths.fast_pointer_file).unwrap();
        for _ in 0..6 {
            commit_chain(settings, &paths, &chain).unwrap();
        }
        let current = read_pointer_path(&paths, settings, &paths.fast_pointer_file).unwrap();
        let previous = read_pointer_path(&paths, settings, &paths.fast_previous_pointer_file).unwrap();
        let candidates = vec![
            safe_child(&paths, &old.state_file).unwrap(),
            safe_child(&paths, &current.state_file).unwrap(),
            safe_child(&paths, &previous.state_file).unwrap(),
            safe_child(&paths, &current.blocks_file).unwrap(),
        ];
        cleanup_for_test(settings, &paths, &candidates).unwrap();
        assert!(!candidates[0].exists());
        assert!(candidates[1..].iter().all(|path| path.exists()));
        fs::write(&paths.fast_previous_pointer_file, b"invalid pointer").unwrap();
        let leftover = paths.fast_storage_dir.join("state-obsolete.json");
        fs::write(&leftover, b"keep on uncertain recovery identity").unwrap();
        assert!(cleanup_for_test(settings, &paths, &[leftover.clone()]).is_err());
        assert!(leftover.exists());
    }

    #[test]
    fn hf130_open_journal_survives_cleanup() {
        let fixture = Fixture::new("reader-lifetime");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let genesis = ChainState::new_with_genesis(settings).unwrap();
        let mut first = genesis.clone();
        extend(settings, &mut first, 1);
        initialize_new_chain(settings, &paths, &first).unwrap();
        let (pointer, state, _, journal) = open_export_commit(settings, &paths).unwrap();
        let mut second = genesis.clone();
        extend(settings, &mut second, 2);
        commit_chain(settings, &paths, &second).unwrap();
        let mut third = genesis;
        extend(settings, &mut third, 3);
        commit_chain(settings, &paths, &third).unwrap();
        let retired = safe_child(&paths, &pointer.blocks_file).unwrap();
        cleanup_for_test(settings, &paths, &[retired]).unwrap();
        let blocks = read_blocks(settings, &pointer, &state, journal).unwrap();
        assert_eq!(blocks.last().unwrap().block_hash(), first.tip_hash());
        assert_eq!(load_fast_chain(settings, &paths, true).unwrap().0.tip_hash(), third.tip_hash());
    }

    #[test]
    fn hf130_cleanup_yields_to_canonical_writer() {
        let fixture = Fixture::new("cleanup-yield");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let chain = ChainState::new_with_genesis(settings).unwrap();
        initialize_new_chain(settings, &paths, &chain).unwrap();
        let obsolete = paths.fast_storage_dir.join("state-obsolete.json");
        fs::write(&obsolete, b"pending cleanup").unwrap();
        let writer = super::super::storage_lock().lock().unwrap();
        assert!(!cleanup_candidates_if_idle(settings, &paths, &[obsolete.clone()]).unwrap());
        assert!(obsolete.exists());
        drop(writer);
        cleanup_for_test(settings, &paths, &[obsolete.clone()]).unwrap();
        assert!(!obsolete.exists());
    }

    #[test]
    fn hf132_ui_snapshot_defers_while_live_owner_is_busy() {
        let fixture = Fixture::new("hf132-ui-busy");
        let settings = &fixture.0;
        let chain = ChainState::new_with_genesis(settings).unwrap();
        let live = Arc::new(StdMutex::new(chain.clone()));
        register_live_chain(settings, &live);
        let held = live.lock().unwrap();
        let snapshot = try_live_chain_snapshot(settings);
        assert!(snapshot.unwrap_err().to_string().contains("snapshot is busy"));
        let loaded = super::super::load_or_init_chain_for_ui_fast(settings);
        assert!(loaded.unwrap_err().to_string().contains("snapshot is busy"));
        assert!(!NodePaths::from_settings(settings).fast_storage_exists(),
            "busy live owner must not fall through to disk initialization");
        drop(held);
        let loaded = super::super::load_or_init_chain_for_ui_fast(settings).unwrap();
        assert!(Arc::ptr_eq(&loaded.blocks, &chain.blocks));
        assert!(Arc::ptr_eq(&loaded.utxos, &chain.utxos));
    }

    #[test]
    fn hf132_committed_readers_do_not_wait_for_storage_writer() {
        let fixture = Fixture::new("hf132-reader-lock");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let mut chain = ChainState::new_with_genesis(settings).unwrap();
        extend(settings, &mut chain, 2);
        initialize_new_chain(settings, &paths, &chain).unwrap();
        let expected = chain.tip_hash();
        let writer = super::super::storage_lock().lock().unwrap();
        let copied_settings = settings.clone();
        let (sender, receiver) = mpsc::channel();
        let reader = thread::spawn(move || {
            let result = (|| -> Result<(Hash256, Hash256)> {
                let ui = super::super::load_or_init_chain_for_ui_fast(&copied_settings)?;
                let verified = super::super::load_or_init_chain(&copied_settings)?;
                Ok((ui.tip_hash(), verified.tip_hash()))
            })();
            let _ = sender.send(result);
        });
        let result = receiver.recv_timeout(Duration::from_secs(2));
        // Release before asserting, so even a regression can finish its worker.
        drop(writer);
        reader.join().unwrap();
        assert_eq!(result.expect("committed read queued behind storage writer").unwrap(),
            (expected, expected));
    }

    #[test]
    fn hf132_ui_genesis_initialization_defers_to_storage_writer() {
        let fixture = Fixture::new("hf132-initialize-busy");
        let settings = &fixture.0;
        let writer = super::super::storage_lock().lock().unwrap();
        let result = super::super::load_or_init_chain_for_ui_fast(settings);
        drop(writer);
        assert!(result.unwrap_err().to_string().contains("initialization is busy"));
        assert!(!NodePaths::from_settings(settings).fast_storage_exists());
    }

    #[test]
    fn hf132_validated_candidate_adoption_matches_public_replay() {
        let fixture = Fixture::new("hf132-validated-adoption");
        let settings = &fixture.0;
        let genesis = ChainState::new_with_genesis(settings).unwrap();
        let mut local = genesis.clone();
        extend(settings, &mut local, 1);
        let mut sibling = genesis.clone();
        extend(settings, &mut sibling, 1);
        assert_ne!(local.tip_hash(), sibling.tip_hash());
        let mut stronger = sibling.clone();
        extend(settings, &mut stronger, 1);
        for candidate in [genesis, local.clone(), sibling, stronger] {
            for prefer_equal in [false, true] {
                let mut replayed = local.clone();
                let mut retained = local.clone();
                let replay_result = replayed.try_adopt_peer_chain(
                    candidate.blocks.as_ref().clone(), settings, prefer_equal).unwrap();
                let retained_result = retained.try_adopt_validated_peer_chain(
                    candidate.clone(), settings, prefer_equal).unwrap();
                assert_eq!(retained_result, replay_result);
                assert_eq!(retained.blocks, replayed.blocks);
                assert_eq!(retained.utxos, replayed.utxos);
                assert_eq!(retained.mempool, replayed.mempool);
            }
        }
    }

    #[test]
    fn hf132_verified_disk_baseline_rejects_ui_only_utxo_state() {
        let fixture = Fixture::new("hf132-validated-baseline");
        let settings = &fixture.0;
        let paths = NodePaths::from_settings(settings);
        let mut chain = ChainState::new_with_genesis(settings).unwrap();
        extend(settings, &mut chain, 1);
        assert!(!chain.utxos.is_empty());
        // A structurally coherent, checksummed persisted state is still not a
        // consensus-validated baseline. Reproduce a wrong UTXO snapshot without
        // corrupting the pointer checksum or block history.
        chain.utxos = Arc::new(HashMap::new());
        initialize_new_chain(settings, &paths, &chain).unwrap();
        let original_pointer = fs::read(&paths.fast_pointer_file).unwrap();
        let ui = super::super::load_or_init_chain_for_ui_fast(settings).unwrap();
        assert_eq!(ui.height(), 1);
        let verified = super::super::load_or_init_chain(settings);
        assert!(format!("{:#}", verified.unwrap_err()).contains("UTXO mismatch after replay"));
        assert_eq!(fs::read(&paths.fast_pointer_file).unwrap(), original_pointer);
        assert!(!paths.fast_previous_pointer_file.exists());
        assert!(!paths.fast_storage_lock_file.exists());
    }

    #[test]
    fn hf132_validated_adoption_restores_local_tx_and_discards_peer_mempool() {
        let fixture = Fixture::new("hf132-peer-mempool");
        let settings = &fixture.0;
        let mut wallet = WalletFile::new(&settings.network.name);
        let address = Address::parse_with_prefix(
            &wallet.create_key(settings, "funder", 0).unwrap().address,
            &settings.network.address_prefix).unwrap();
        let mut recipient_wallet = WalletFile::new(&settings.network.name);
        let recipient = Address::parse_with_prefix(
            &recipient_wallet.create_key(settings, "recipient", 0).unwrap().address,
            &settings.network.address_prefix).unwrap();
        let options = MiningOptions { duty_cycle_percent: 100, max_hashes: Some(5_000_000) };
        let mut base = ChainState::new_with_genesis(settings).unwrap();
        for _ in 0..3 {
            let block = mine_next_block(&base, settings, &address, options).unwrap();
            base.connect_block(block, settings).unwrap();
        }
        let local_tx = wallet.create_signed_transaction(&base, settings, &recipient,
            Amount::from_str("1.00").unwrap(), Amount::from_str("0.00001").unwrap()).unwrap();
        let peer_tx = wallet.create_signed_transaction(&base, settings, &recipient,
            Amount::from_str("2.00").unwrap(), Amount::from_str("0.00001").unwrap()).unwrap();
        let mut stale = base.clone();
        stale.accept_transaction_to_mempool(local_tx.clone(), settings).unwrap();
        let block = mine_next_block(&stale, settings, &address, options).unwrap();
        stale.connect_block(block, settings).unwrap();
        assert!(!stale.tx_in_mempool(local_tx.txid()));
        let mut stronger = base;
        extend(settings, &mut stronger, 2);
        stronger.accept_transaction_to_mempool(peer_tx.clone(), settings).unwrap();
        let mut public = stale.clone();
        assert!(public.try_adopt_peer_chain(stronger.blocks.as_ref().clone(), settings, false).unwrap());
        assert!(stale.try_adopt_validated_peer_chain(stronger, settings, false).unwrap());
        assert_eq!(stale.mempool, public.mempool);
        assert!(stale.tx_in_mempool(local_tx.txid()));
        assert!(!stale.tx_in_mempool(peer_tx.txid()));
    }
}
