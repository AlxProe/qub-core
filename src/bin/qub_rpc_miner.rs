use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const APP_VERSION: &str = "v1.7.9";
const MAX_WORKERS: usize = 128;
const DEFAULT_REFRESH_SECS: u64 = 20;

#[derive(Debug, Clone)]
struct Options {
    rpc_url: String,
    token: String,
    address: Option<String>,
    pool_id: Option<String>,
    workers: usize,
    batch: usize,
    refresh_secs: u64,
    once: bool,
    max_rounds: Option<u64>,
}

#[derive(Debug, Clone)]
struct HttpEndpoint {
    host: String,
    port: u16,
    base_path: String,
}

#[derive(Debug, Clone)]
struct MiningTemplate {
    job_id: String,
    height: u32,
    header_prefix: [u8; 76],
    target: [u8; 32],
    expires_at_unix: u64,
    mode: String,
    payout_label: String,
}

#[derive(Debug)]
struct FoundNonce {
    job_id: String,
    height: u32,
    nonce: u32,
    hash_hex: String,
    hashes: u64,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let options = parse_options()?;
    let endpoint = parse_http_url(&options.rpc_url)?;

    println!("QUB RPC Miner {APP_VERSION}");
    println!(
        "RPC: {}:{}{}",
        endpoint.host, endpoint.port, endpoint.base_path
    );
    println!(
        "Mode: {} | workers={} batch={} refresh={}s",
        if options.address.is_some() {
            "solo"
        } else {
            "pool"
        },
        options.workers,
        options.batch,
        options.refresh_secs
    );
    println!(
        "Reference CPU worker. Stock Bitcoin Stratum/AxeOS devices require a separate QUB adapter."
    );

    let status = rpc_json(
        &endpoint,
        &options.token,
        "GET",
        "/rpc/v1/mining/status",
        None,
    )?;
    if status.get("ok").and_then(Value::as_bool) != Some(true) {
        bail!("RPC mining status is not healthy: {status}");
    }
    if status.get("template_available").and_then(Value::as_bool) != Some(true) {
        bail!("RPC template endpoint is not available; run embedded `qubd node` mode");
    }

    let mut round = 0u64;
    loop {
        round = round.saturating_add(1);
        if let Some(max_rounds) = options.max_rounds {
            if round > max_rounds {
                println!("Maximum rounds reached.");
                return Ok(());
            }
        }

        let count = options.workers.min(options.batch).max(1);
        let target_query = if let Some(address) = &options.address {
            format!("address={}", url_encode(address))
        } else {
            format!(
                "pool_id={}",
                url_encode(options.pool_id.as_deref().unwrap_or_default())
            )
        };
        let path = format!("/rpc/v1/mining/template-batch?{target_query}&count={count}");
        let response = rpc_json(&endpoint, &options.token, "GET", &path, None)?;
        let templates = parse_template_response(&response)?;
        if templates.is_empty() {
            bail!("RPC returned no mining templates");
        }

        let height = templates[0].height;
        let mode = templates[0].mode.clone();
        let payout_label = templates[0].payout_label.clone();
        let expiry = templates
            .iter()
            .map(|template| template.expires_at_unix)
            .min()
            .unwrap_or_else(unix_time_secs);
        println!(
            "Round #{round}: height #{height}, mode={mode}, payout={payout_label}, jobs={}, expires_in={}s",
            templates.len(),
            expiry.saturating_sub(unix_time_secs())
        );

        let stop = Arc::new(AtomicBool::new(false));
        let hashes = Arc::new(AtomicU64::new(0));
        let (sender, receiver) = mpsc::channel::<FoundNonce>();
        let round_started = Instant::now();
        let mut handles = Vec::with_capacity(templates.len());

        for template in templates {
            let worker_stop = Arc::clone(&stop);
            let worker_hashes = Arc::clone(&hashes);
            let worker_sender = sender.clone();
            handles.push(thread::spawn(move || {
                mine_template(template, worker_stop, worker_hashes, worker_sender)
            }));
        }
        drop(sender);

        let refresh_deadline = Instant::now() + Duration::from_secs(options.refresh_secs);
        let mut found = None;
        while Instant::now() < refresh_deadline && unix_time_secs().saturating_add(2) < expiry {
            match receiver.recv_timeout(Duration::from_millis(250)) {
                Ok(result) => {
                    found = Some(result);
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        stop.store(true, Ordering::Release);
        for handle in handles {
            let _ = handle.join();
        }

        let elapsed = round_started.elapsed().as_secs_f64().max(0.001);
        let total_hashes = hashes.load(Ordering::Acquire);
        println!(
            "Round #{round}: {} hashes in {:.2}s ({:.3} MH/s)",
            total_hashes,
            elapsed,
            total_hashes as f64 / elapsed / 1_000_000.0
        );

        if let Some(found) = found {
            println!(
                "Found candidate: height #{} nonce={} hash={} worker_hashes={}",
                found.height, found.nonce, found.hash_hex, found.hashes
            );
            let submit = rpc_json(
                &endpoint,
                &options.token,
                "POST",
                "/rpc/v1/mining/submit-block",
                Some(json!({"job_id":found.job_id,"nonce":found.nonce})),
            )?;
            println!(
                "Submit response: {}",
                serde_json::to_string_pretty(&submit)?
            );
            if submit.get("accepted").and_then(Value::as_bool) == Some(true) {
                println!("Block accepted.");
                if options.once {
                    return Ok(());
                }
            } else if options.once {
                return Ok(());
            }
        } else if options.once {
            println!("No valid nonce found in this round.");
            return Ok(());
        }
    }
}

fn parse_options() -> Result<Options> {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }

    let rpc_url = take_value(&mut args, "--rpc")
        .or_else(|| env::var("QUB_RPC_URL").ok())
        .unwrap_or_else(|| "http://127.0.0.1:17445".to_string());
    let inline_token = take_value(&mut args, "--token");
    let token_file = take_value(&mut args, "--token-file");
    let env_token = env::var("QUB_RPC_TOKEN").ok();
    let configured_sources = usize::from(inline_token.is_some())
        + usize::from(token_file.is_some())
        + usize::from(env_token.is_some());
    if configured_sources != 1 {
        bail!("provide exactly one RPC token source: --token, --token-file, or QUB_RPC_TOKEN");
    }
    let token = if let Some(token) = inline_token {
        token
    } else if let Some(path) = token_file {
        read_token_file(&path)?
    } else {
        env_token.unwrap_or_default()
    };
    if token.trim().len() < 24 {
        bail!("RPC token must contain at least 24 bytes");
    }

    let address = take_value(&mut args, "--address");
    let pool_id = take_value(&mut args, "--pool-id");
    if address.is_some() == pool_id.is_some() {
        bail!("provide exactly one of --address or --pool-id");
    }

    let workers = parse_usize(
        take_value(&mut args, "--workers").as_deref().unwrap_or("1"),
        "workers",
        1,
        MAX_WORKERS,
    )?;
    let batch = parse_usize(
        take_value(&mut args, "--batch")
            .as_deref()
            .unwrap_or(&workers.to_string()),
        "batch",
        1,
        MAX_WORKERS,
    )?;
    let refresh_secs = parse_u64(
        take_value(&mut args, "--refresh-secs")
            .as_deref()
            .unwrap_or(&DEFAULT_REFRESH_SECS.to_string()),
        "refresh-secs",
        2,
        300,
    )?;
    let once = take_switch(&mut args, "--once");
    let max_rounds = take_value(&mut args, "--max-rounds")
        .map(|value| parse_u64(&value, "max-rounds", 1, u64::MAX))
        .transpose()?;

    if !args.is_empty() {
        bail!("unknown arguments: {}", args.join(" "));
    }

    Ok(Options {
        rpc_url,
        token: token.trim().to_string(),
        address,
        pool_id,
        workers,
        batch,
        refresh_secs,
        once,
        max_rounds,
    })
}

fn print_help() {
    println!(
        "QUB RPC Miner {APP_VERSION}\n\n\
Usage:\n  qub-rpc-miner --rpc http://127.0.0.1:17445 --token-file <path> --address <qub-address> [options]\n  qub-rpc-miner --rpc http://127.0.0.1:17445 --token-file <path> --pool-id <pool-id> [options]\n\n\
Options:\n  --rpc <url>             HTTP RPC endpoint (default http://127.0.0.1:17445)\n  --token <token>         RPC token (visible in process list; token-file is preferred)\n  --token-file <path>     Read RPC token from a local file\n  --address <address>     Solo payout address\n  --pool-id <hex>         Existing on-chain QUB pool ID\n  --workers <1..128>      Number of independent CPU work jobs\n  --batch <1..128>        Maximum templates requested per round\n  --refresh-secs <2..300> Refresh work after this interval (default 20)\n  --once                  Run a single round / submit at most one candidate\n  --max-rounds <n>        Stop after n rounds\n\n\
Environment:\n  QUB_RPC_URL\n  QUB_RPC_TOKEN\n"
    );
}

fn take_value(args: &mut Vec<String>, name: &str) -> Option<String> {
    let index = args.iter().position(|arg| arg == name)?;
    if index + 1 >= args.len() {
        return None;
    }
    args.remove(index);
    Some(args.remove(index))
}

fn take_switch(args: &mut Vec<String>, name: &str) -> bool {
    if let Some(index) = args.iter().position(|arg| arg == name) {
        args.remove(index);
        true
    } else {
        false
    }
}

fn parse_usize(input: &str, name: &str, min: usize, max: usize) -> Result<usize> {
    let value = input
        .parse::<usize>()
        .with_context(|| format!("invalid --{name}"))?;
    if value < min || value > max {
        bail!("--{name} must be {min}..{max}");
    }
    Ok(value)
}

fn parse_u64(input: &str, name: &str, min: u64, max: u64) -> Result<u64> {
    let value = input
        .parse::<u64>()
        .with_context(|| format!("invalid --{name}"))?;
    if value < min || value > max {
        bail!("--{name} must be {min}..{max}");
    }
    Ok(value)
}

fn read_token_file(path: &str) -> Result<String> {
    let metadata =
        fs::metadata(path).with_context(|| format!("failed to stat token file {path}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 4096 {
        bail!("token file must be a regular file containing 1..4096 bytes");
    }
    Ok(fs::read_to_string(path)
        .with_context(|| format!("failed to read token file {path}"))?
        .trim()
        .to_string())
}

fn parse_http_url(input: &str) -> Result<HttpEndpoint> {
    let value = input.trim();
    let rest = value
        .strip_prefix("http://")
        .context("only plain http:// RPC URLs are supported by the reference miner")?;
    let (authority, base_path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{}", path.trim_matches('/'))),
        None => (rest, String::new()),
    };
    if authority.is_empty() || authority.contains('@') {
        bail!("invalid RPC URL authority");
    }
    let (host, port) = if authority.starts_with('[') {
        let close = authority.find(']').context("invalid IPv6 RPC URL")?;
        let host = authority[1..close].to_string();
        let suffix = &authority[close + 1..];
        let port = suffix
            .strip_prefix(':')
            .unwrap_or("17445")
            .parse::<u16>()
            .context("invalid RPC port")?;
        (host, port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if port.chars().all(|c| c.is_ascii_digit()) {
            (
                host.to_string(),
                port.parse::<u16>().context("invalid RPC port")?,
            )
        } else {
            (authority.to_string(), 17445)
        }
    } else {
        (authority.to_string(), 17445)
    };
    if host.trim().is_empty() {
        bail!("RPC host is empty");
    }
    Ok(HttpEndpoint {
        host,
        port,
        base_path: if base_path == "/" {
            String::new()
        } else {
            base_path
        },
    })
}

fn rpc_json(
    endpoint: &HttpEndpoint,
    token: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let address = if endpoint.host.contains(':') {
        format!("[{}]:{}", endpoint.host, endpoint.port)
    } else {
        format!("{}:{}", endpoint.host, endpoint.port)
    };
    let socket = address
        .to_socket_addrs()
        .with_context(|| format!("failed to resolve {address}"))?
        .next()
        .context("RPC address resolved to no sockets")?;
    let mut stream = TcpStream::connect_timeout(&socket, Duration::from_secs(5))
        .with_context(|| format!("failed to connect to RPC {socket}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(10)))?;

    let body_bytes = body
        .map(|value| serde_json::to_vec(&value))
        .transpose()?
        .unwrap_or_default();
    let request_path = format!("{}{}", endpoint.base_path, path);
    let host_header = if endpoint.host.contains(':') {
        format!("[{}]:{}", endpoint.host, endpoint.port)
    } else {
        format!("{}:{}", endpoint.host, endpoint.port)
    };
    let request = format!(
        "{method} {request_path} HTTP/1.1\r\nHost: {host_header}\r\nX-QUB-RPC-Token: {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body_bytes.len()
    );
    stream.write_all(request.as_bytes())?;
    if !body_bytes.is_empty() {
        stream.write_all(&body_bytes)?;
    }
    stream.flush()?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response)?;
    let header_end = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .context("RPC response is missing HTTP header terminator")?;
    let headers = std::str::from_utf8(&response[..header_end])?;
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|value| value.parse::<u16>().ok())
        .context("invalid RPC HTTP status")?;
    let value: Value = serde_json::from_slice(&response[header_end + 4..])
        .context("RPC response body is not JSON")?;
    if !(200..300).contains(&status) {
        bail!("RPC HTTP {status}: {}", serde_json::to_string(&value)?);
    }
    Ok(value)
}

fn parse_template_response(value: &Value) -> Result<Vec<MiningTemplate>> {
    let rows = value
        .get("templates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    rows.iter().map(parse_template).collect()
}

fn parse_template(value: &Value) -> Result<MiningTemplate> {
    let prefix = decode_fixed::<76>(
        value
            .get("header_prefix_hex")
            .and_then(Value::as_str)
            .context("template missing header_prefix_hex")?,
        "header prefix",
    )?;
    let target = decode_fixed::<32>(
        value
            .get("target_hex")
            .and_then(Value::as_str)
            .context("template missing target_hex")?,
        "target",
    )?;
    Ok(MiningTemplate {
        job_id: value
            .get("job_id")
            .and_then(Value::as_str)
            .context("template missing job_id")?
            .to_string(),
        height: value
            .get("height")
            .and_then(Value::as_u64)
            .context("template missing height")? as u32,
        header_prefix: prefix,
        target,
        expires_at_unix: value
            .get("expires_at_unix")
            .and_then(Value::as_u64)
            .context("template missing expires_at_unix")?,
        mode: value
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string(),
        payout_label: value
            .get("payout_label")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

fn mine_template(
    template: MiningTemplate,
    stop: Arc<AtomicBool>,
    total_hashes: Arc<AtomicU64>,
    sender: mpsc::Sender<FoundNonce>,
) {
    let mut header = [0u8; 80];
    header[..76].copy_from_slice(&template.header_prefix);
    let mut pending_hashes = 0u64;
    let mut worker_total = 0u64;
    let mut nonce = 0u32;

    loop {
        if stop.load(Ordering::Acquire)
            || unix_time_secs().saturating_add(1) >= template.expires_at_unix
        {
            break;
        }
        header[76..80].copy_from_slice(&nonce.to_le_bytes());
        let digest = double_sha256(&header);
        pending_hashes = pending_hashes.saturating_add(1);
        worker_total = worker_total.saturating_add(1);
        if hash_meets_target(digest, template.target) {
            total_hashes.fetch_add(pending_hashes, Ordering::AcqRel);
            let _ = sender.send(FoundNonce {
                job_id: template.job_id,
                height: template.height,
                nonce,
                hash_hex: hex::encode(digest),
                hashes: worker_total,
            });
            stop.store(true, Ordering::Release);
            return;
        }
        if pending_hashes >= 1_000_000 {
            total_hashes.fetch_add(pending_hashes, Ordering::AcqRel);
            pending_hashes = 0;
        }
        if nonce == u32::MAX {
            break;
        }
        nonce = nonce.wrapping_add(1);
    }
    total_hashes.fetch_add(pending_hashes, Ordering::AcqRel);
}

fn double_sha256(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    let second = Sha256::digest(first);
    let mut out = [0u8; 32];
    out.copy_from_slice(&second);
    out
}

fn hash_meets_target(digest_internal: [u8; 32], target_be: [u8; 32]) -> bool {
    let mut hash_be = digest_internal;
    hash_be.reverse();
    hash_be <= target_be
}

fn decode_fixed<const N: usize>(text: &str, name: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(text).with_context(|| format!("invalid {name} hex"))?;
    if bytes.len() != N {
        bail!("{name} must contain exactly {N} bytes");
    }
    let mut out = [0u8; N];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn url_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn unix_time_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_urls() {
        let endpoint = parse_http_url("http://127.0.0.1:19445/base").unwrap();
        assert_eq!(endpoint.host, "127.0.0.1");
        assert_eq!(endpoint.port, 19445);
        assert_eq!(endpoint.base_path, "/base");
    }

    #[test]
    fn pow_comparison_uses_reversed_internal_digest() {
        let mut digest = [0u8; 32];
        digest[0] = 1;
        let mut target = [0xffu8; 32];
        target[31] = 1;
        assert!(hash_meets_target(digest, target));

        let mut harder = [0u8; 32];
        harder[31] = 0;
        assert!(!hash_meets_target(digest, harder));
    }

    #[test]
    fn url_encoding_is_stable() {
        assert_eq!(url_encode("qub1abc"), "qub1abc");
        assert_eq!(url_encode("a b"), "a%20b");
    }
}
