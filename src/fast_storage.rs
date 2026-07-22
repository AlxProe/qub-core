use crate::*;
use anyhow::{bail, Context, Result};
use num_bigint::BigUint;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, Weak};
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

static FAST_LOADS: AtomicU64 = AtomicU64::new(0);
static FAST_COMMITS: AtomicU64 = AtomicU64::new(0);
static FAST_BYTES_READ: AtomicU64 = AtomicU64::new(0);
static FAST_BYTES_WRITTEN: AtomicU64 = AtomicU64::new(0);
static FAST_LEGACY_EXPORTS: AtomicU64 = AtomicU64::new(0);
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
    pub last_load_millis: u64,
    pub max_load_millis: u64,
    pub last_commit_millis: u64,
    pub max_commit_millis: u64,
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

fn acquire_lease(paths: &NodePaths) -> Result<FastStorageLease> {
    fs::create_dir_all(&paths.fast_storage_dir)?;
    for _ in 0..2 {
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&paths.fast_storage_lock_file)
        {
            Ok(mut file) => {
                writeln!(
                    file,
                    "pid={} created_at_unix={}",
                    std::process::id(),
                    now_secs()
                )?;
                file.flush()?;
                return Ok(FastStorageLease {
                    path: paths.fast_storage_lock_file.clone(),
                });
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
        let pointer = read_pointer_path(paths, settings, &paths.fast_pointer_file)?;
        let (state, bytes) = read_state(paths, settings, &pointer)?;
        Ok((pointer, state, bytes))
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
    paths: &NodePaths,
    settings: &Settings,
    pointer: &FastStoragePointer,
    state: &FastStorageState,
) -> Result<Vec<Block>> {
    let path = safe_child(paths, &pointer.blocks_file)?;
    let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
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
    let (state, _) = read_state(paths, settings, &pointer)?;
    let blocks = read_blocks(paths, settings, &pointer, &state)?;
    let chain = chain_from_state(blocks, state, settings, full_validate)?;
    Ok((chain, pointer.identity()))
}

fn load_fast_chain_once(
    settings: &Settings,
    paths: &NodePaths,
    full_validate: bool,
) -> Result<(ChainState, FastStorageIdentity, bool)> {
    match load_fast_chain_from_pointer_path(
        settings,
        paths,
        &paths.fast_pointer_file,
        full_validate,
    ) {
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
    cleanup_old_state_files(paths, &pointer)?;
    cleanup_old_journal_files(paths, &pointer)?;
    Ok(pointer)
}

fn cleanup_old_state_files(paths: &NodePaths, current: &FastStoragePointer) -> Result<()> {
    let mut keep = HashSet::<String>::new();
    keep.insert(current.state_file.clone());
    if paths.fast_previous_pointer_file.exists() {
        if let Ok(previous) = serde_json::from_reader::<_, FastStoragePointer>(BufReader::new(
            File::open(&paths.fast_previous_pointer_file)?,
        )) {
            keep.insert(previous.state_file);
        }
    }
    let mut states = fs::read_dir(&paths.fast_storage_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("state-") && name.ends_with(".json") {
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
    states.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, name, _) in states.iter().take(4) {
        keep.insert(name.clone());
    }
    for (_, name, path) in states {
        if !keep.contains(&name) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
}

fn cleanup_old_journal_files(paths: &NodePaths, current: &FastStoragePointer) -> Result<()> {
    let mut keep = HashSet::<String>::new();
    keep.insert(current.blocks_file.clone());
    if paths.fast_previous_pointer_file.exists() {
        if let Ok(previous) = serde_json::from_reader::<_, FastStoragePointer>(BufReader::new(
            File::open(&paths.fast_previous_pointer_file)?,
        )) {
            keep.insert(previous.blocks_file);
        }
    }

    let mut journals = fs::read_dir(&paths.fast_storage_dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("blocks-") && name.ends_with(".jsonl") {
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
    journals.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, name, _) in journals.iter().take(3) {
        keep.insert(name.clone());
    }
    for (_, name, path) in journals {
        if !keep.contains(&name) {
            let _ = fs::remove_file(path);
        }
    }
    Ok(())
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
    let pointer = write_state_and_pointer(paths, None, blocks_file, state)?;
    FAST_COMMITS.fetch_add(1, Ordering::Relaxed);
    write_status_from_pointer(paths, &pointer)?;
    if legacy_is_current {
        let legacy = LegacyExportStatus {
            schema_version: 1,
            network: settings.network.name.clone(),
            height: chain.height(),
            tip_hash: chain.tip_hash(),
            exported_at_unix: now_secs(),
        };
        let _ = atomic_replace_json(&paths.fast_legacy_export_status_file, &legacy, true);
    } else {
        export_chain_json_to(settings, paths, &paths.chain_file)?;
    }
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
    chain: &ChainState,
    current: &FastStoragePointer,
) -> Result<()> {
    let candidate_work = total_work_value(chain)?;
    let committed_work = parse_work(&current.total_work_hex)?;
    if candidate_work < committed_work {
        bail!(
            "stale Fast Chain Engine persistence rejected: candidate #{} {} has less work than committed #{} {}",
            chain.height(),
            chain.tip_hash(),
            current.committed_height,
            current.tip_hash
        );
    }
    Ok(())
}

pub(crate) fn commit_chain(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
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
        ensure_candidate_not_behind_committed(chain, &pointer)?;
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
    write_status_from_pointer(paths, &pointer)?;
    maybe_refresh_legacy_export(settings, paths, chain, &pointer)?;
    let millis = elapsed_millis(started);
    FAST_LAST_COMMIT_MILLIS.store(millis, Ordering::Relaxed);
    record_max(&FAST_MAX_COMMIT_MILLIS, millis);
    Ok(pointer.identity())
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
    let _ = atomic_replace_json(&paths.chain_status_file, &status, true);
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


pub fn load_committed_chain(settings: &Settings, full_validate: bool) -> Result<ChainState> {
    let paths = NodePaths::from_settings(settings);
    paths.ensure_dirs()?;
    if paths.fast_storage_exists() {
        return load_fast_chain(settings, &paths, full_validate).map(|(chain, _)| chain);
    }
    if !paths.chain_file.exists() {
        return ChainState::new_with_genesis(settings);
    }
    let raw = fs::read_to_string(&paths.chain_file)
        .with_context(|| format!("read {}", paths.chain_file.display()))?;
    let persisted: PersistedChainState = serde_json::from_str(&raw)?;
    if full_validate {
        ChainState::from_persisted(persisted, settings)
    } else {
        ChainState::from_persisted_unchecked_for_ui(persisted, settings)
    }
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

fn maybe_refresh_legacy_export(
    settings: &Settings,
    paths: &NodePaths,
    chain: &ChainState,
    pointer: &FastStoragePointer,
) -> Result<()> {
    if should_refresh_legacy_export(settings, paths, pointer) {
        export_chain_json_to(settings, paths, &paths.chain_file)?;
        let status = LegacyExportStatus {
            schema_version: 1,
            network: settings.network.name.clone(),
            height: chain.height(),
            tip_hash: chain.tip_hash(),
            exported_at_unix: now_secs(),
        };
        atomic_replace_json(&paths.fast_legacy_export_status_file, &status, true)?;
        FAST_LEGACY_EXPORTS.fetch_add(1, Ordering::Relaxed);
    }
    Ok(())
}

pub fn export_chain_json(settings: &Settings, output_path: &Path) -> Result<(u32, Hash256, u64)> {
    let paths = NodePaths::from_settings(settings);
    if paths.fast_storage_exists() {
        export_chain_json_to(settings, &paths, output_path)
    } else {
        let raw = fs::read(&paths.chain_file)
            .with_context(|| format!("read {}", paths.chain_file.display()))?;
        atomic_replace_bytes(output_path, &raw, true)?;
        let chain = super::load_or_init_chain_for_ui_fast(settings)?;
        Ok((chain.height(), chain.tip_hash(), raw.len() as u64))
    }
}

fn export_chain_json_to(
    settings: &Settings,
    paths: &NodePaths,
    output_path: &Path,
) -> Result<(u32, Hash256, u64)> {
    let (pointer, state, _, _) =
        read_pointer_state_with_recovery(paths, settings)?;
    let blocks_path = safe_child(paths, &pointer.blocks_file)?;
    let file = File::open(&blocks_path)?;
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
    Ok((pointer.committed_height, pointer.tip_hash, bytes))
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

pub(crate) fn publish_live_chain(settings: &Settings, candidate: &ChainState) {
    let Some(live) = live_chain_arc(settings) else {
        return;
    };
    let Ok(mut current) = live.try_lock() else {
        // Caller may already hold the canonical mutex; in that case the state is
        // already current and no secondary publication is needed.
        return;
    };
    if current.tip_hash() == candidate.tip_hash() {
        // HF124/v1.8.1: same-tip saves are commonly produced by GUI/CLI
        // transaction submission while the embedded node is concurrently
        // receiving other mempool entries. Replacing the live owner with one
        // snapshot could silently drop the other side of that race.
        let current_txids = current.mempool_txids();
        let candidate_txids = candidate.mempool_txids();

        // The normal coalesced P2P save publishes an older copy-on-write
        // snapshot after the live owner has already accepted the same or newer
        // transactions. If every candidate entry is already in the owner,
        // preserve the owner without revalidating the whole mempool under its
        // mutex; the dirty post-save identity check will persist any newer
        // entries in the next bounded commit.
        if candidate_txids.is_subset(&current_txids) {
            return;
        }

        // A GUI/CLI writer can instead carry additional same-tip transactions.
        // Merge and revalidate the deterministic union only for that real
        // divergence; invalid/conflicting entries are filtered by normal policy.
        let candidates = current.reorg_mempool_candidates_for(candidate);
        let mut merged = current.clone();
        merged.rebuild_mempool_from(candidates, settings);
        *current = merged;
        return;
    }

    let candidate_work = total_work_value(candidate).ok();
    let current_work = total_work_value(&current).ok();
    let should_publish = match (candidate_work, current_work) {
        (Some(candidate_work), Some(current_work)) => candidate_work > current_work,
        _ => candidate.height() > current.height(),
    };
    if should_publish {
        *current = candidate.clone();
    }
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
