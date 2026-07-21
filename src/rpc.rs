use crate::*;
use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RPC_VERSION: &str = "1.8.0";
const RPC_MIN_TOKEN_BYTES: usize = 24;
const RPC_MAX_TOKEN_FILE_BYTES: u64 = 4096;
const RPC_MAX_MINING_STATS_WINDOW: usize = 4096;
const RPC_MAX_LONG_POLL_MS: u64 = 30_000;

#[derive(Clone)]
enum RpcBackend {
    Embedded(Arc<Mutex<ChainState>>),
    StandaloneReadOnly,
}

impl RpcBackend {
    fn mode_name(&self) -> &'static str {
        match self {
            Self::Embedded(_) => "embedded-node",
            Self::StandaloneReadOnly => "standalone-read-only",
        }
    }

    fn state_changes_enabled(&self) -> bool {
        matches!(self, Self::Embedded(_))
    }

    fn with_chain_read<T>(
        &self,
        settings: &Settings,
        f: impl FnOnce(&ChainState) -> Result<T>,
    ) -> Result<T> {
        match self {
            Self::Embedded(chain) => {
                let guard = chain
                    .lock()
                    .map_err(|_| anyhow::anyhow!("chain mutex poisoned"))?;
                f(&guard)
            }
            Self::StandaloneReadOnly => {
                let chain = load_or_init_chain_for_ui_fast(settings)?;
                f(&chain)
            }
        }
    }

    fn with_chain_write<T>(
        &self,
        _settings: &Settings,
        f: impl FnOnce(&mut ChainState) -> Result<T>,
    ) -> Result<T> {
        match self {
            Self::Embedded(chain) => {
                let mut guard = chain
                    .lock()
                    .map_err(|_| anyhow::anyhow!("chain mutex poisoned"))?;
                f(&mut guard)
            }
            Self::StandaloneReadOnly => {
                bail!("state-changing RPC requires embedded node mode: run `qubd node` with rpc.enabled=true")
            }
        }
    }
}

#[derive(Debug, Clone)]
struct MiningJob {
    id: String,
    created_at_unix: u64,
    expires_at_unix: u64,
    height: u32,
    parent_hash: Hash256,
    header: BlockHeader,
    coinbase: Transaction,
    non_coinbase_transactions: Arc<Vec<Transaction>>,
    mode: String,
    payout_label: String,
    extra_nonce: u64,
}

impl MiningJob {
    fn block_with_nonce(&self, nonce: u32) -> Block {
        let mut transactions = Vec::with_capacity(self.non_coinbase_transactions.len() + 1);
        transactions.push(self.coinbase.clone());
        transactions.extend(self.non_coinbase_transactions.iter().cloned());
        let mut header = self.header.clone();
        header.nonce = nonce;
        Block {
            header,
            transactions,
        }
    }
}

#[derive(Default)]
struct JobCache {
    jobs: HashMap<String, MiningJob>,
    order: VecDeque<String>,
}

impl JobCache {
    fn prune(&mut self, now: u64, max_jobs: usize) {
        self.jobs.retain(|_, job| job.expires_at_unix >= now);
        self.order.retain(|id| self.jobs.contains_key(id));
        while self.jobs.len() >= max_jobs.max(1) {
            let Some(id) = self.order.pop_front() else {
                break;
            };
            self.jobs.remove(&id);
        }
    }

    fn insert(&mut self, job: MiningJob, max_jobs: usize) {
        self.prune(unix_time_secs(), max_jobs);
        self.order.push_back(job.id.clone());
        self.jobs.insert(job.id.clone(), job);
    }

    fn get(&mut self, id: &str, max_jobs: usize) -> Option<MiningJob> {
        self.prune(unix_time_secs(), max_jobs);
        self.jobs.get(id).cloned()
    }

    fn invalidate_parent(&mut self, parent_hash: Hash256) {
        self.jobs.retain(|_, job| job.parent_hash != parent_hash);
        self.order.retain(|id| self.jobs.contains_key(id));
    }
}

#[derive(Debug)]
struct RateWindow {
    start: Instant,
    count: u32,
}

#[derive(Debug, Clone)]
struct IpNetwork {
    address: IpAddr,
    prefix: u8,
}

impl IpNetwork {
    fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("empty CIDR entry");
        }
        let (address_text, prefix_text) = match trimmed.split_once('/') {
            Some(parts) => parts,
            None => {
                let address = IpAddr::from_str(trimmed)
                    .with_context(|| format!("invalid allowed IP/CIDR {trimmed}"))?;
                let prefix = if address.is_ipv4() { 32 } else { 128 };
                return Ok(Self { address, prefix });
            }
        };
        let address = IpAddr::from_str(address_text.trim())
            .with_context(|| format!("invalid CIDR address {address_text}"))?;
        let prefix = prefix_text
            .trim()
            .parse::<u8>()
            .with_context(|| format!("invalid CIDR prefix {prefix_text}"))?;
        let max = if address.is_ipv4() { 32 } else { 128 };
        if prefix > max {
            bail!("CIDR prefix {prefix} exceeds {max}");
        }
        Ok(Self { address, prefix })
    }

    fn contains(&self, candidate: IpAddr) -> bool {
        match (self.address, candidate) {
            (IpAddr::V4(network), IpAddr::V4(candidate)) => {
                ipv4_prefix_match(network, candidate, self.prefix)
            }
            (IpAddr::V6(network), IpAddr::V6(candidate)) => {
                ipv6_prefix_match(network, candidate, self.prefix)
            }
            _ => false,
        }
    }
}

fn ipv4_prefix_match(network: Ipv4Addr, candidate: Ipv4Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let mask = u32::MAX.checked_shl((32 - prefix) as u32).unwrap_or(0);
    (u32::from(network) & mask) == (u32::from(candidate) & mask)
}

fn ipv6_prefix_match(network: Ipv6Addr, candidate: Ipv6Addr, prefix: u8) -> bool {
    if prefix == 0 {
        return true;
    }
    let network = u128::from_be_bytes(network.octets());
    let candidate = u128::from_be_bytes(candidate.octets());
    let mask = u128::MAX.checked_shl((128 - prefix) as u32).unwrap_or(0);
    (network & mask) == (candidate & mask)
}

struct RpcServerState {
    settings: Settings,
    backend: RpcBackend,
    token: String,
    allowed_networks: Vec<IpNetwork>,
    jobs: Mutex<JobCache>,
    extra_nonce: AtomicU64,
    active_connections: AtomicUsize,
    rate_windows: Mutex<HashMap<IpAddr, RateWindow>>,
    mining_stats_cache: Mutex<HashMap<usize, (String, Value)>>,
}

impl RpcServerState {
    fn peer_allowed(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return true;
        }
        self.settings.rpc.allow_remote
            && self
                .allowed_networks
                .iter()
                .any(|network| network.contains(ip))
    }

    fn rate_allowed(&self, ip: IpAddr) -> bool {
        let Ok(mut windows) = self.rate_windows.lock() else {
            return false;
        };
        if windows.len() > 4096 {
            windows.retain(|_, entry| entry.start.elapsed() < Duration::from_secs(120));
        }
        let entry = windows.entry(ip).or_insert_with(|| RateWindow {
            start: Instant::now(),
            count: 0,
        });
        if entry.start.elapsed() >= Duration::from_secs(60) {
            entry.start = Instant::now();
            entry.count = 0;
        }
        if entry.count >= self.settings.rpc.max_requests_per_minute {
            return false;
        }
        entry.count = entry.count.saturating_add(1);
        true
    }
}

struct ActiveConnectionGuard {
    state: Arc<RpcServerState>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.state.active_connections.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

pub fn start_embedded(settings: Settings, chain: Arc<Mutex<ChainState>>) -> Result<()> {
    let bind = settings.rpc.bind.clone();
    let listener = prepare_listener(&settings, &bind)?;
    let state = Arc::new(build_server_state(settings, RpcBackend::Embedded(chain))?);
    println!("QUB HF123 embedded RPC listening on http://{bind}");
    println!(
        "RPC mode={} remote={} state_changes=true mining_templates=true",
        state.backend.mode_name(),
        state.settings.rpc.allow_remote
    );
    thread::Builder::new()
        .name("qub-rpc-accept".to_string())
        .spawn(move || serve(listener, state))
        .context("failed to spawn embedded RPC accept loop")?;
    Ok(())
}

pub fn run_standalone(settings: Settings, bind_override: Option<&str>) -> Result<()> {
    let bind = bind_override
        .unwrap_or(settings.rpc.bind.as_str())
        .to_string();
    let listener = prepare_listener(&settings, &bind)?;
    let state = Arc::new(build_server_state(
        settings,
        RpcBackend::StandaloneReadOnly,
    )?);
    println!("QUB HF123 standalone read-only RPC listening on http://{bind}");
    println!("State-changing endpoints require embedded `qubd node` mode.");
    serve(listener, state);
    Ok(())
}

fn prepare_listener(settings: &Settings, bind: &str) -> Result<TcpListener> {
    if !settings.rpc.enabled {
        bail!("rpc.enabled=false; explicitly enable RPC in the selected config")
    }
    let addresses = bind
        .to_socket_addrs()
        .with_context(|| format!("invalid RPC bind {bind}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        bail!("RPC bind {bind} resolves to no addresses");
    }
    let non_loopback = addresses.iter().any(|address| !address.ip().is_loopback());
    if non_loopback && !settings.rpc.allow_remote {
        bail!("non-loopback RPC bind requires rpc.allow_remote=true");
    }
    if non_loopback && settings.rpc.allowed_cidrs.is_empty() {
        bail!("remote RPC requires a non-empty rpc.allowed_cidrs allowlist");
    }
    if non_loopback {
        eprintln!(
            "WARNING: QUB RPC has no built-in TLS. Bind only behind a private network, firewall, WireGuard/Tailscale, or a TLS reverse proxy."
        );
    }
    TcpListener::bind(bind).with_context(|| format!("failed to bind QUB RPC API on {bind}"))
}

fn build_server_state(settings: Settings, backend: RpcBackend) -> Result<RpcServerState> {
    let token = resolve_auth_token(&settings)?;
    let allowed_networks = settings
        .rpc
        .allowed_cidrs
        .iter()
        .map(|entry| IpNetwork::parse(entry))
        .collect::<Result<Vec<_>>>()?;
    Ok(RpcServerState {
        settings,
        backend,
        token,
        allowed_networks,
        jobs: Mutex::new(JobCache::default()),
        extra_nonce: AtomicU64::new(unix_time_secs().rotate_left(17)),
        active_connections: AtomicUsize::new(0),
        rate_windows: Mutex::new(HashMap::new()),
        mining_stats_cache: Mutex::new(HashMap::new()),
    })
}

pub fn resolve_auth_token(settings: &Settings) -> Result<String> {
    let inline = settings.rpc.auth_token.trim();
    let token_file = settings.rpc.auth_token_file.trim();
    let inline_real = !inline.is_empty() && !looks_placeholder_token(inline);
    if inline_real && !token_file.is_empty() {
        bail!("configure either rpc.auth_token or rpc.auth_token_file, not both");
    }
    let token = if !token_file.is_empty() {
        let path = Path::new(token_file);
        let metadata = fs::metadata(path)
            .with_context(|| format!("failed to stat RPC token file {}", path.display()))?;
        if !metadata.is_file() {
            bail!("RPC token path is not a regular file: {}", path.display());
        }
        if metadata.len() == 0 || metadata.len() > RPC_MAX_TOKEN_FILE_BYTES {
            bail!("RPC token file must be 1..{RPC_MAX_TOKEN_FILE_BYTES} bytes");
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = metadata.permissions().mode();
            if mode & 0o077 != 0 {
                bail!(
                    "RPC token file {} must be owner-only (chmod 600)",
                    path.display()
                );
            }
        }
        fs::read_to_string(path)
            .with_context(|| format!("failed to read RPC token file {}", path.display()))?
            .trim()
            .to_string()
    } else {
        inline.to_string()
    };
    if token.len() < RPC_MIN_TOKEN_BYTES || looks_placeholder_token(&token) {
        bail!(
            "RPC authentication token must be at least {RPC_MIN_TOKEN_BYTES} bytes and not a placeholder"
        );
    }
    if token.len() > RPC_MAX_TOKEN_FILE_BYTES as usize {
        bail!("RPC authentication token is too large");
    }
    Ok(token)
}

fn looks_placeholder_token(value: &str) -> bool {
    let upper = value.trim().to_ascii_uppercase();
    upper.is_empty()
        || upper.contains("CHANGE_THIS")
        || upper.contains("YOUR_")
        || upper.contains("PASTE_")
        || upper.contains("EXAMPLE")
        || upper.contains("PLACEHOLDER")
}

fn serve(listener: TcpListener, state: Arc<RpcServerState>) {
    for incoming in listener.incoming() {
        match incoming {
            Ok(mut stream) => {
                let peer = match stream.peer_addr() {
                    Ok(peer) => peer,
                    Err(err) => {
                        eprintln!("RPC peer address error: {err}");
                        continue;
                    }
                };
                if !state.peer_allowed(peer.ip()) {
                    let _ = write_json(&mut stream, 403, json!({"error":"forbidden_peer"}));
                    continue;
                }
                if !state.rate_allowed(peer.ip()) {
                    let _ = write_json(&mut stream, 429, json!({"error":"rate_limited"}));
                    continue;
                }
                let previous = state.active_connections.fetch_add(1, Ordering::AcqRel);
                if previous >= state.settings.rpc.max_connections {
                    state.active_connections.fetch_sub(1, Ordering::AcqRel);
                    let _ = write_json(&mut stream, 503, json!({"error":"server_busy"}));
                    continue;
                }
                let state_clone = Arc::clone(&state);
                if let Err(err) = thread::Builder::new()
                    .name("qub-rpc-request".to_string())
                    .spawn(move || {
                        let _guard = ActiveConnectionGuard {
                            state: Arc::clone(&state_clone),
                        };
                        if let Err(err) = handle_connection(&mut stream, peer, &state_clone) {
                            eprintln!("RPC request from {peer} failed: {err:#}");
                        }
                    })
                {
                    state.active_connections.fetch_sub(1, Ordering::AcqRel);
                    eprintln!("failed to spawn RPC request thread: {err}");
                }
            }
            Err(err) => eprintln!("RPC accept error: {err}"),
        }
    }
}

fn handle_connection(
    stream: &mut TcpStream,
    peer: SocketAddr,
    state: &Arc<RpcServerState>,
) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(
        state.settings.rpc.read_timeout_secs,
    )))?;
    stream.set_write_timeout(Some(Duration::from_secs(
        state.settings.rpc.write_timeout_secs,
    )))?;

    let request = match read_http_request(
        stream,
        state.settings.rpc.max_header_bytes,
        state.settings.rpc.max_body_bytes,
    ) {
        Ok(request) => request,
        Err(err) => {
            let detail = err.to_string();
            let status = if detail.contains("too large") || detail.contains("exceeds") {
                413
            } else {
                400
            };
            return write_json(
                stream,
                status,
                json!({"error":"bad_request","detail":detail}),
            );
        }
    };

    let supplied = match supplied_token(&request) {
        Ok(token) => token,
        Err(err) => {
            return write_json(
                stream,
                400,
                json!({"error":"bad_auth_headers","detail":err.to_string()}),
            );
        }
    };
    if !constant_time_token_eq(&state.token, supplied) {
        return write_json(stream, 401, json!({"error":"unauthorized"}));
    }

    let response = route_request(state, peer, &request);
    match response {
        Ok((status, value)) => write_json(stream, status, value),
        Err(err) => {
            let detail = err.to_string();
            if let Some(expected) = detail.strip_prefix("method_not_allowed:") {
                write_json(
                    stream,
                    405,
                    json!({"error":"method_not_allowed","expected":expected}),
                )
            } else {
                write_json(
                    stream,
                    500,
                    json!({"error":"internal_error","detail":detail}),
                )
            }
        }
    }
}

fn read_http_request(
    stream: &mut TcpStream,
    max_header: usize,
    max_body: usize,
) -> Result<HttpRequest> {
    let mut data = Vec::<u8>::with_capacity(2048);
    let mut chunk = [0u8; 2048];
    let header_end = loop {
        if data.len() > max_header {
            bail!("request headers too large");
        }
        if let Some(position) = find_header_end(&data) {
            break position;
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            bail!("connection closed before request headers");
        }
        data.extend_from_slice(&chunk[..n]);
    };
    if header_end > max_header {
        bail!("request headers exceed configured limit");
    }
    let header_bytes = &data[..header_end];
    let header_text = std::str::from_utf8(header_bytes).context("request headers are not UTF-8")?;
    let mut lines = header_text
        .split(|c| c == '\r' || c == '\n')
        .filter(|line| !line.is_empty());
    let request_line = lines.next().context("missing request line")?;
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap_or("").to_ascii_uppercase();
    let target = request_parts.next().unwrap_or("").to_string();
    let version = request_parts.next().unwrap_or("");
    if request_parts.next().is_some() || !version.starts_with("HTTP/1.") {
        bail!("invalid HTTP request line");
    }
    if target.is_empty() || !target.starts_with('/') {
        bail!("invalid HTTP target");
    }
    let mut headers = HashMap::<String, String>::new();
    let mut sensitive_counts = HashMap::<String, usize>::new();
    for line in lines {
        if line.starts_with(' ') || line.starts_with('\t') {
            bail!("folded HTTP headers are not supported");
        }
        let (name, value) = line.split_once(':').context("malformed HTTP header")?;
        let name = name.trim().to_ascii_lowercase();
        if name.is_empty() {
            bail!("empty HTTP header name");
        }
        let value = value.trim().to_string();
        if matches!(
            name.as_str(),
            "content-length" | "transfer-encoding" | "authorization" | "x-qub-rpc-token"
        ) {
            let count = sensitive_counts.entry(name.clone()).or_insert(0);
            *count += 1;
            if *count > 1 {
                bail!("duplicate sensitive HTTP header: {name}");
            }
        }
        headers.insert(name, value);
    }
    if headers.contains_key("transfer-encoding") {
        bail!("Transfer-Encoding is not supported");
    }
    let content_length = headers
        .get("content-length")
        .map(|value| value.parse::<usize>().context("invalid Content-Length"))
        .transpose()?
        .unwrap_or(0);
    if content_length > max_body {
        bail!("request body too large");
    }
    let body_start = header_end
        + if data.get(header_end..header_end + 4) == Some(b"\r\n\r\n") {
            4
        } else {
            2
        };
    let mut body = if body_start <= data.len() {
        data[body_start..].to_vec()
    } else {
        Vec::new()
    };
    if body.len() > content_length {
        bail!("request contains bytes beyond Content-Length");
    }
    while body.len() < content_length {
        let remaining = content_length - body.len();
        let take = chunk.len().min(remaining);
        let n = stream.read(&mut chunk[..take])?;
        if n == 0 {
            bail!("connection closed before request body completed");
        }
        body.extend_from_slice(&chunk[..n]);
    }
    Ok(HttpRequest {
        method,
        target,
        headers,
        body,
    })
}

fn find_header_end(data: &[u8]) -> Option<usize> {
    data.windows(4)
        .position(|window| window == b"\r\n\r\n")
        .or_else(|| data.windows(2).position(|window| window == b"\n\n"))
}

fn supplied_token(request: &HttpRequest) -> Result<&str> {
    let custom = request.headers.get("x-qub-rpc-token").map(String::as_str);
    let bearer = request.headers.get("authorization").and_then(|value| {
        value
            .strip_prefix("Bearer ")
            .or_else(|| value.strip_prefix("bearer "))
    });
    match (custom, bearer) {
        (Some(_), Some(_)) => {
            bail!("supply either X-QUB-RPC-Token or Authorization Bearer, not both")
        }
        (Some(token), None) | (None, Some(token)) => Ok(token),
        (None, None) => Ok(""),
    }
}

pub fn constant_time_token_eq(expected: &str, supplied: &str) -> bool {
    let a = expected.as_bytes();
    let b = supplied.as_bytes();
    let max_len = a.len().max(b.len());
    let mut diff = (a.len() ^ b.len()) as u64;
    for index in 0..max_len {
        let left = a.get(index).copied().unwrap_or(0);
        let right = b.get(index).copied().unwrap_or(0);
        diff |= u64::from(left ^ right);
    }
    diff == 0
}

fn route_request(
    state: &Arc<RpcServerState>,
    _peer: SocketAddr,
    request: &HttpRequest,
) -> Result<(u16, Value)> {
    let (path, query) = split_path_query(&request.target);
    let path = path.trim_end_matches('/');
    let method = request.method.as_str();

    if path.is_empty() || path == "/rpc" || path == "/rpc/v1" {
        require_method(method, "GET")?;
        return Ok((
            200,
            json!({
                "service":"QUB RPC API",
                "version":RPC_VERSION,
                "network":state.settings.network.name,
                "hf122_headless_infrastructure":true,
                "backend_mode":state.backend.mode_name(),
                "state_changes_enabled":state.backend.state_changes_enabled(),
                "token_authenticated":true,
                "remote_allowed":state.settings.rpc.allow_remote,
                "tls_built_in":false,
                "endpoints":[
                    "/rpc/v1/status",
                    "/rpc/v1/chain/tip",
                    "/rpc/v1/chain/block/<height-or-hash>",
                    "/rpc/v1/chain/tx/<txid>",
                    "/rpc/v1/mempool",
                    "/rpc/v1/mining/status",
                    "/rpc/v1/mining/stats?window=256",
                    "/rpc/v1/mining/template?address=<qub-address>",
                    "/rpc/v1/mining/template-batch?address=<qub-address>&count=4",
                    "/rpc/v1/mining/submit-block",
                    "/rpc/v1/tx/submit",
                    "/rpc/v1/events/tip?after=<hash>&wait_ms=30000"
                ],
                "note":"Use embedded qubd node mode for template, submit-block and tx submission. A payout label is not proof of a unique operator."
            }),
        ));
    }

    if matches!(path, "/health" | "/rpc/v1/health") {
        require_method(method, "GET")?;
        return Ok((
            200,
            json!({"ok":true,"service":"qub-rpc","version":RPC_VERSION}),
        ));
    }

    if matches!(path, "/rpc/v1/status" | "/rpc/v1/chain/tip") {
        require_method(method, "GET")?;
        return Ok((200, chain_status_value(state)?));
    }

    if let Some(identifier) = path.strip_prefix("/rpc/v1/chain/block/") {
        require_method(method, "GET")?;
        return match chain_block_value(state, identifier) {
            Ok(value) => Ok((200, value)),
            Err(err) => Ok((
                404,
                json!({"error":"block_not_found","detail":err.to_string()}),
            )),
        };
    }

    if let Some(txid) = path.strip_prefix("/rpc/v1/chain/tx/") {
        require_method(method, "GET")?;
        return match chain_tx_value(state, txid) {
            Ok(value) => Ok((200, value)),
            Err(err) => Ok((
                400,
                json!({"error":"invalid_txid","detail":err.to_string()}),
            )),
        };
    }

    if path == "/rpc/v1/mempool" {
        require_method(method, "GET")?;
        let limit = query_usize(&query, "limit", 250, 1000);
        return Ok((200, mempool_value(state, limit)?));
    }

    if path == "/rpc/v1/mining/status" {
        require_method(method, "GET")?;
        return Ok((200, mining_status_value(state)?));
    }

    if path == "/rpc/v1/mining/stats" {
        require_method(method, "GET")?;
        let window = query_usize(&query, "window", 256, RPC_MAX_MINING_STATS_WINDOW);
        return Ok((200, mining_stats_value(state, window)?));
    }

    if path == "/rpc/v1/mining/template" || path == "/rpc/v1/mining/template-batch" {
        require_method(method, "GET")?;
        if !state.backend.state_changes_enabled() {
            return Ok((409, json!({"error":"embedded_node_required"})));
        }
        let count = if path.ends_with("template-batch") {
            query_usize(
                &query,
                "count",
                state.settings.rpc.max_template_batch.min(4),
                state.settings.rpc.max_template_batch,
            )
        } else {
            1
        };
        let full = query
            .get("full")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let templates = match create_templates(state, &query, count, full) {
            Ok(templates) => templates,
            Err(err) => {
                return Ok((
                    400,
                    json!({"error":"template_request_rejected","detail":err.to_string()}),
                ));
            }
        };
        if count == 1 {
            return Ok((
                200,
                templates
                    .into_iter()
                    .next()
                    .unwrap_or_else(|| json!({"error":"no_template"})),
            ));
        }
        return Ok((
            200,
            json!({"ok":true,"count":templates.len(),"templates":templates}),
        ));
    }

    if path == "/rpc/v1/mining/submit-block" {
        require_method(method, "POST")?;
        if !state.backend.state_changes_enabled() {
            return Ok((409, json!({"error":"embedded_node_required"})));
        }
        return submit_block(state, &request.body);
    }

    if path == "/rpc/v1/tx/submit" {
        require_method(method, "POST")?;
        if !state.backend.state_changes_enabled() {
            return Ok((409, json!({"error":"embedded_node_required"})));
        }
        return submit_transaction(state, &request.body);
    }

    if path == "/rpc/v1/events/tip" {
        require_method(method, "GET")?;
        let after = query.get("after").cloned().unwrap_or_default();
        let wait_ms = query_u64(&query, "wait_ms", 0, RPC_MAX_LONG_POLL_MS);
        return Ok((200, tip_event_value(state, &after, wait_ms)?));
    }

    Ok((404, json!({"error":"not_found"})))
}

fn require_method(actual: &str, expected: &str) -> Result<()> {
    if actual != expected {
        bail!("method_not_allowed:{expected}");
    }
    Ok(())
}

fn chain_status_value(state: &Arc<RpcServerState>) -> Result<Value> {
    state.backend.with_chain_read(&state.settings, |chain| {
        let tip = chain.blocks.last().context("chain has no blocks")?;
        Ok(json!({
            "ok":true,
            "service":"qub-rpc",
            "version":RPC_VERSION,
            "network":state.settings.network.name,
            "backend_mode":state.backend.mode_name(),
            "height":chain.height(),
            "tip_hash":chain.tip_hash().to_string(),
            "tip_block_version":tip.header.version,
            "tip_block_time":tip.header.time,
            "next_height":chain.height().saturating_add(1),
            "next_block_expected_version":expected_block_version(&state.settings, chain.height().saturating_add(1)),
            "protocol_epoch_2_activation_height":protocol_epoch_2_activation_height(&state.settings),
            "mempool_transactions":chain.mempool.len(),
            "state_changes_enabled":state.backend.state_changes_enabled(),
        }))
    })
}

fn chain_block_value(state: &Arc<RpcServerState>, identifier: &str) -> Result<Value> {
    state.backend.with_chain_read(&state.settings, |chain| {
        let (height, block) = if let Ok(height) = identifier.parse::<usize>() {
            let block = chain.blocks.get(height).context("block height not found")?;
            (height, block)
        } else {
            let hash = Hash256::from_hex(identifier)?;
            chain
                .blocks
                .iter()
                .enumerate()
                .find(|(_, block)| block.block_hash() == hash)
                .context("block hash not found")?
        };
        Ok(json!({
            "ok":true,
            "height":height,
            "hash":block.block_hash().to_string(),
            "confirmations":chain.blocks.len().saturating_sub(height),
            "block":block,
        }))
    })
}

fn chain_tx_value(state: &Arc<RpcServerState>, txid_text: &str) -> Result<Value> {
    let txid = Hash256::from_hex(txid_text)?;
    state.backend.with_chain_read(&state.settings, |chain| {
        for (height, block) in chain.blocks.iter().enumerate() {
            if let Some((index, transaction)) = block
                .transactions
                .iter()
                .enumerate()
                .find(|(_, transaction)| transaction.txid() == txid)
            {
                return Ok(json!({
                    "ok":true,
                    "status":"confirmed",
                    "height":height,
                    "block_hash":block.block_hash().to_string(),
                    "confirmations":chain.blocks.len().saturating_sub(height),
                    "index":index,
                    "transaction":transaction,
                    "raw_hex":hex::encode(transaction.serialize_base()),
                }));
            }
        }
        if let Some(transaction) = chain
            .mempool
            .iter()
            .find(|transaction| transaction.txid() == txid)
        {
            return Ok(json!({
                "ok":true,
                "status":"mempool",
                "transaction":transaction,
                "raw_hex":hex::encode(transaction.serialize_base()),
            }));
        }
        Ok(json!({"ok":false,"status":"not_found","txid":txid.to_string()}))
    })
}

fn mempool_value(state: &Arc<RpcServerState>, limit: usize) -> Result<Value> {
    state.backend.with_chain_read(&state.settings, |chain| {
        let transactions = chain
            .mempool
            .iter()
            .take(limit)
            .map(|transaction| {
                json!({
                    "txid":transaction.txid().to_string(),
                    "version":transaction.version,
                    "inputs":transaction.inputs.len(),
                    "outputs":transaction.outputs.len(),
                    "raw_bytes":transaction.serialize_base().len(),
                })
            })
            .collect::<Vec<_>>();
        Ok(json!({
            "ok":true,
            "count":chain.mempool.len(),
            "returned":transactions.len(),
            "transactions":transactions,
        }))
    })
}

fn mining_status_value(state: &Arc<RpcServerState>) -> Result<Value> {
    let status = chain_status_value(state)?;
    let cached_jobs = state.jobs.lock().map(|cache| cache.jobs.len()).unwrap_or(0);
    Ok(json!({
        "ok":true,
        "version":RPC_VERSION,
        "backend_mode":state.backend.mode_name(),
        "template_available":state.backend.state_changes_enabled(),
        "submit_available":state.backend.state_changes_enabled(),
        "tx_submit_available":state.backend.state_changes_enabled(),
        "job_ttl_secs":state.settings.rpc.job_ttl_secs,
        "max_cached_jobs":state.settings.rpc.max_cached_jobs,
        "max_template_batch":state.settings.rpc.max_template_batch,
        "cached_jobs":cached_jobs,
        "tracked_jobs_required":true,
        "status":status,
        "note":"HF123 jobs are exact canonical-parent jobs. Stock Bitcoin Stratum/AxeOS compatibility requires a separate QUB adapter."
    }))
}

fn mining_stats_value(state: &Arc<RpcServerState>, window: usize) -> Result<Value> {
    let tip_hash = state
        .backend
        .with_chain_read(&state.settings, |chain| Ok(chain.tip_hash().to_string()))?;

    if let Ok(cache) = state.mining_stats_cache.lock() {
        if let Some((cached_tip, value)) = cache.get(&window) {
            if cached_tip == &tip_hash {
                return Ok(value.clone());
            }
        }
    }

    let value = state.backend.with_chain_read(&state.settings, |chain| {
        Ok(mining_stats_json(&state.settings, &chain.blocks, window))
    })?;

    if let Ok(mut cache) = state.mining_stats_cache.lock() {
        if cache.len() >= 16 && !cache.contains_key(&window) {
            cache.clear();
        }
        cache.insert(window, (tip_hash, value.clone()));
    }

    Ok(value)
}

fn create_templates(
    state: &Arc<RpcServerState>,
    query: &HashMap<String, String>,
    count: usize,
    include_full: bool,
) -> Result<Vec<Value>> {
    let address_text = query
        .get("address")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    let pool_text = query
        .get("pool_id")
        .map(String::as_str)
        .filter(|value| !value.trim().is_empty());
    if address_text.is_some() == pool_text.is_some() {
        bail!("provide exactly one of address or pool_id");
    }
    if count == 0 || count > state.settings.rpc.max_template_batch {
        bail!("template batch count exceeds configured limit");
    }

    state.backend.with_chain_read(&state.settings, |chain| {
        let (mode, payout_label, parts, pool_outputs) = if let Some(address_text) = address_text {
            let address =
                Address::parse_with_prefix(address_text, &state.settings.network.address_prefix)?;
            let label = address.to_string();
            let parts = build_candidate_block_parts(chain, &state.settings, Some(&label))?;
            ("solo".to_string(), label, parts, None)
        } else {
            let pool_id = Hash256::from_hex(pool_text.unwrap_or_default())?;
            if !pools_active(&state.settings, chain.height().saturating_add(1)) {
                bail!("pooled mining is not active");
            }
            let registry = pools_registry_from_blocks(&state.settings, &chain.blocks)?;
            if !registry.contains_key(&pool_id) {
                bail!("unknown pool_id");
            }
            let parts = build_candidate_block_parts(chain, &state.settings, None)?;
            let outputs = expected_pool_coinbase_outputs(
                &state.settings,
                &chain.blocks,
                pool_id,
                parts.reward_atoms as u128,
            )?;
            (
                "pool".to_string(),
                format!("pool:{pool_id}"),
                parts,
                Some((pool_id, outputs)),
            )
        };

        let tail = Arc::new(parts.non_coinbase_transactions.clone());
        let created = unix_time_secs();
        let expires = created.saturating_add(state.settings.rpc.job_ttl_secs);
        let mut templates = Vec::<Value>::with_capacity(count);
        for _ in 0..count {
            let extra_nonce = state.extra_nonce.fetch_add(1, Ordering::AcqRel);
            let coinbase = if mode == "solo" {
                let address = Address::parse_with_prefix(
                    &payout_label,
                    &state.settings.network.address_prefix,
                )?;
                create_coinbase(parts.height, parts.reward_atoms, &address, extra_nonce)?
            } else {
                let (pool_id, outputs) = pool_outputs.as_ref().context("missing pool outputs")?;
                Transaction {
                    version: 1,
                    inputs: vec![TxIn {
                        previous_output: OutPoint::null(),
                        signature_script: pool_block_marker_script(
                            parts.height,
                            extra_nonce,
                            *pool_id,
                        ),
                        sequence: u32::MAX,
                    }],
                    outputs: outputs.clone(),
                    locktime: 0,
                }
            };
            let mut txids = Vec::with_capacity(tail.len() + 1);
            txids.push(coinbase.txid());
            txids.extend(tail.iter().map(Transaction::txid));
            let merkle_root = merkle_root(&txids);
            let header = BlockHeader {
                version: parts.version,
                prev_block_hash: parts.prev_block_hash,
                merkle_root,
                time: parts.time,
                bits: parts.bits,
                nonce: 0,
            };
            let counter = state.extra_nonce.load(Ordering::Acquire);
            let id_material = format!(
                "QUB-HF123-JOB|{}|{}|{}|{}|{}|{}",
                state.settings.network.name,
                parts.height,
                parts.prev_block_hash,
                payout_label,
                extra_nonce,
                counter
            );
            let job_id = Hash256::double_sha256(id_material.as_bytes()).to_string();
            let job = MiningJob {
                id: job_id.clone(),
                created_at_unix: created,
                expires_at_unix: expires,
                height: parts.height,
                parent_hash: parts.prev_block_hash,
                header: header.clone(),
                coinbase: coinbase.clone(),
                non_coinbase_transactions: Arc::clone(&tail),
                mode: mode.clone(),
                payout_label: payout_label.clone(),
                extra_nonce,
            };
            state
                .jobs
                .lock()
                .map_err(|_| anyhow::anyhow!("job cache mutex poisoned"))?
                .insert(job.clone(), state.settings.rpc.max_cached_jobs);
            templates.push(template_value(&state.settings, &job, include_full));
        }
        Ok(templates)
    })
}

fn template_value(settings: &Settings, job: &MiningJob, include_full: bool) -> Value {
    let block = job.block_with_nonce(0);
    let header_bytes = job.header.serialize();
    let target = target_from_compact(job.header.bits)
        .map(hex::encode)
        .unwrap_or_default();
    let mut value = json!({
        "ok":true,
        "job_id":job.id,
        "network":settings.network.name,
        "mode":job.mode,
        "payout_label":job.payout_label,
        "height":job.height,
        "parent_hash":job.parent_hash.to_string(),
        "version":job.header.version,
        "time":job.header.time,
        "bits":job.header.bits,
        "bits_hex":format!("0x{:08x}", job.header.bits),
        "target_hex":target,
        "nonce":0,
        "nonce_offset":76,
        "header_bytes":80,
        "header_hex":hex::encode(&header_bytes),
        "header_prefix_hex":hex::encode(&header_bytes[..76]),
        "merkle_root":job.header.merkle_root.to_string(),
        "extra_nonce":job.extra_nonce,
        "created_at_unix":job.created_at_unix,
        "expires_at_unix":job.expires_at_unix,
        "transaction_count":job.non_coinbase_transactions.len() + 1,
        "coinbase_txid":job.coinbase.txid().to_string(),
        "coinbase_hex":hex::encode(job.coinbase.serialize_base()),
        "pow":"double-sha256",
        "target_comparison":"reverse the 32-byte internal double-SHA256 digest before comparing it as big-endian bytes to target_hex",
        "stock_bitcoin_stratum_compatible":false,
    });
    if include_full {
        value["block"] = serde_json::to_value(block).unwrap_or(Value::Null);
    }
    value
}

#[derive(Deserialize)]
struct SubmitBlockRequest {
    job_id: String,
    nonce: u32,
}

fn submit_block(state: &Arc<RpcServerState>, body: &[u8]) -> Result<(u16, Value)> {
    let request: SubmitBlockRequest =
        serde_json::from_slice(body).context("invalid submit-block JSON")?;
    let job = state
        .jobs
        .lock()
        .map_err(|_| anyhow::anyhow!("job cache mutex poisoned"))?
        .get(&request.job_id, state.settings.rpc.max_cached_jobs);
    let Some(job) = job else {
        return Ok((404, json!({"error":"unknown_or_expired_job"})));
    };
    if job.expires_at_unix < unix_time_secs() {
        return Ok((409, json!({"error":"expired_job"})));
    }
    let block = job.block_with_nonce(request.nonce);
    let block_hash = block.block_hash();
    if !verify_header_pow(&block.header)? {
        return Ok((
            422,
            json!({"error":"insufficient_pow","hash":block_hash.to_string()}),
        ));
    }
    if let Some(reason) = crate::p2p::hf113_live_tip_pause_reason(
        &state.settings,
        job.height.saturating_sub(1),
        job.parent_hash,
        1200,
    ) {
        return Ok((409, json!({"error":"canonical_tip_guard","detail":reason})));
    }

    let accepted = state.backend.with_chain_write(&state.settings, |chain| {
        if chain.tip_hash() == block_hash {
            return Ok(false);
        }
        if chain.height().saturating_add(1) != job.height || chain.tip_hash() != job.parent_hash {
            bail!(
                "stale_job: local tip moved to #{} {}",
                chain.height(),
                chain.tip_hash()
            );
        }
        if block.header.version != expected_block_version(&state.settings, job.height) {
            bail!("job block version is no longer valid");
        }
        chain.connect_block(block.clone(), &state.settings)?;
        save_chain(&state.settings, chain)?;
        Ok(true)
    });

    let accepted = match accepted {
        Ok(value) => value,
        Err(err) if err.to_string().contains("stale_job") => {
            return Ok((409, json!({"error":"stale_job","detail":err.to_string()})));
        }
        Err(err) => return Err(err),
    };
    let relayed = if accepted {
        crate::p2p::broadcast_block(&state.settings, &block).unwrap_or(0)
    } else {
        0
    };
    if let Ok(mut jobs) = state.jobs.lock() {
        jobs.invalidate_parent(job.parent_hash);
    }
    Ok((
        200,
        json!({
            "ok":true,
            "accepted":accepted,
            "already_accepted":!accepted,
            "height":job.height,
            "hash":block_hash.to_string(),
            "mode":job.mode,
            "payout_label":job.payout_label,
            "relayed_to_peers":relayed,
        }),
    ))
}

fn submit_transaction(state: &Arc<RpcServerState>, body: &[u8]) -> Result<(u16, Value)> {
    let value: Value = serde_json::from_slice(body).context("invalid tx-submit JSON")?;
    let tx_value = value.get("transaction").cloned().unwrap_or(value);
    let transaction: Transaction =
        serde_json::from_value(tx_value).context("invalid transaction object")?;
    let txid = transaction.txid();
    let accepted = state.backend.with_chain_write(&state.settings, |chain| {
        let accepted_txid =
            chain.accept_transaction_to_mempool(transaction.clone(), &state.settings)?;
        if accepted_txid != txid {
            bail!("transaction ID mismatch");
        }
        save_chain(&state.settings, chain)?;
        Ok(())
    });
    if let Err(err) = accepted {
        return Ok((
            422,
            json!({"error":"transaction_rejected","detail":err.to_string(),"txid":txid.to_string()}),
        ));
    }
    let relayed = crate::p2p::broadcast_tx(&state.settings, &transaction).unwrap_or(0);
    Ok((
        200,
        json!({"ok":true,"txid":txid.to_string(),"relayed_to_peers":relayed}),
    ))
}

fn tip_event_value(state: &Arc<RpcServerState>, after: &str, wait_ms: u64) -> Result<Value> {
    let deadline = Instant::now() + Duration::from_millis(wait_ms.min(RPC_MAX_LONG_POLL_MS));
    loop {
        let status = chain_status_value(state)?;
        let tip = status.get("tip_hash").and_then(Value::as_str).unwrap_or("");
        if after.is_empty() || tip != after || Instant::now() >= deadline {
            return Ok(json!({
                "ok":true,
                "changed":after.is_empty() || tip != after,
                "status":status,
            }));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

fn query_usize(query: &HashMap<String, String>, key: &str, default: usize, max: usize) -> usize {
    query
        .get(key)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
        .max(1)
        .min(max.max(1))
}

fn query_u64(query: &HashMap<String, String>, key: &str, default: u64, max: u64) -> u64 {
    query
        .get(key)
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default)
        .min(max)
}

fn split_path_query(target: &str) -> (String, HashMap<String, String>) {
    let mut parts = target.splitn(2, '?');
    let path = parts.next().unwrap_or("/").to_string();
    let mut query = HashMap::new();
    if let Some(raw_query) = parts.next() {
        for pair in raw_query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut key_value = pair.splitn(2, '=');
            let key = url_decode(key_value.next().unwrap_or(""));
            let value = url_decode(key_value.next().unwrap_or(""));
            query.insert(key, value);
        }
    }
    (path, query)
}

fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                if let (Some(high), Some(low)) =
                    (hex_value(bytes[index + 1]), hex_value(bytes[index + 2]))
                {
                    output.push((high << 4) | low);
                    index += 3;
                } else {
                    output.push(bytes[index]);
                    index += 1;
                }
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&output).to_string()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(10 + byte - b'a'),
        b'A'..=b'F' => Some(10 + byte - b'A'),
        _ => None,
    }
}

fn unix_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn write_json(stream: &mut TcpStream, status: u16, value: Value) -> Result<()> {
    let body = serde_json::to_vec_pretty(&value)?;
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "OK",
    };
    let auth_header = if status == 401 {
        "WWW-Authenticate: Bearer realm=\"QUB RPC\"\r\n"
    } else {
        ""
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nX-Frame-Options: DENY\r\nReferrer-Policy: no-referrer\r\n{auth_header}Connection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes())?;
    stream.write_all(&body)?;
    stream.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_token_comparison_is_strict() {
        assert!(constant_time_token_eq(
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyz"
        ));
        assert!(!constant_time_token_eq(
            "abcdefghijklmnopqrstuvwxyz",
            "abcdefghijklmnopqrstuvwxyZ"
        ));
        assert!(!constant_time_token_eq("short", "shorter"));
    }

    #[test]
    fn cidr_matching_supports_ipv4_and_ipv6() {
        let v4 = IpNetwork::parse("10.20.0.0/16").unwrap();
        assert!(v4.contains("10.20.5.4".parse().unwrap()));
        assert!(!v4.contains("10.21.5.4".parse().unwrap()));
        let v6 = IpNetwork::parse("2001:db8::/32").unwrap();
        assert!(v6.contains("2001:db8:1::1".parse().unwrap()));
        assert!(!v6.contains("2001:db9::1".parse().unwrap()));
    }

    #[test]
    fn header_end_detection_works() {
        assert_eq!(find_header_end(b"GET / HTTP/1.1\r\n\r\n"), Some(14));
        assert_eq!(find_header_end(b"GET / HTTP/1.1\n\n"), Some(14));
    }

    fn test_token_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "qub-hf122-rpc-{name}-{}-{}.token",
            std::process::id(),
            unix_time_secs()
        ))
    }

    #[test]
    fn token_file_is_loaded_and_inline_ambiguity_is_rejected() {
        let path = test_token_path("valid");
        std::fs::write(
            &path,
            b"0123456789abcdef0123456789abcdef
",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }

        let mut settings = Settings::load_from_path("config/regtest.toml").unwrap();
        settings.rpc.auth_token.clear();
        settings.rpc.auth_token_file = path.to_string_lossy().to_string();
        assert_eq!(
            resolve_auth_token(&settings).unwrap(),
            "0123456789abcdef0123456789abcdef"
        );

        settings.rpc.auth_token = "abcdef0123456789abcdef0123456789".to_string();
        assert!(resolve_auth_token(&settings)
            .unwrap_err()
            .to_string()
            .contains("either rpc.auth_token or rpc.auth_token_file"));

        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[test]
    fn token_file_rejects_group_or_other_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = test_token_path("insecure");
        std::fs::write(
            &path,
            b"0123456789abcdef0123456789abcdef
",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();

        let mut settings = Settings::load_from_path("config/regtest.toml").unwrap();
        settings.rpc.auth_token.clear();
        settings.rpc.auth_token_file = path.to_string_lossy().to_string();
        assert!(resolve_auth_token(&settings)
            .unwrap_err()
            .to_string()
            .contains("chmod 600"));

        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn job_cache_is_bounded() {
        let mut cache = JobCache::default();
        for index in 0..8u64 {
            let job = MiningJob {
                id: format!("job-{index}"),
                created_at_unix: unix_time_secs(),
                expires_at_unix: unix_time_secs() + 60,
                height: 1,
                parent_hash: Hash256::zero(),
                header: BlockHeader {
                    version: 1,
                    prev_block_hash: Hash256::zero(),
                    merkle_root: Hash256::zero(),
                    time: 1,
                    bits: 0x207fffff,
                    nonce: 0,
                },
                coinbase: Transaction {
                    version: 1,
                    inputs: vec![],
                    outputs: vec![],
                    locktime: 0,
                },
                non_coinbase_transactions: Arc::new(Vec::new()),
                mode: "solo".to_string(),
                payout_label: "test".to_string(),
                extra_nonce: index,
            };
            cache.insert(job, 4);
        }
        assert!(cache.jobs.len() <= 4);
    }
}
