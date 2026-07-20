# HF122 / v1.7.9 Security Review

## Review scope

HF122 adds operational and mining infrastructure only. It does not alter the HF120 Protocol Epoch 2 consensus boundary or any economic rule.

Reviewed surfaces:

- embedded and standalone RPC server;
- authentication and remote-bind policy;
- HTTP request parsing;
- mining template construction and job tracking;
- block and transaction submission;
- mining observability;
- reference CPU miner;
- headless deployment templates;
- Explorer v0.7 analytics;
- preservation of HF120/HF121 behavior.

## Consensus invariants

The following remain unchanged:

```text
MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT = 24000
PROTOCOL_EPOCH_1_BLOCK_VERSION = 1
PROTOCOL_EPOCH_2_BLOCK_VERSION = 2
```

HF122 does not add:

- rollback logic;
- payout-address restrictions;
- miner alternation/fairness rules;
- DAA changes;
- checkpoint changes;
- reward/economics changes;
- QUB/JIN changes;
- bridge assets.

Templates use `expected_block_version()` and submissions pass the normal `connect_block()` consensus path.

## Authentication

RPC startup requires a real token from exactly one source:

- `rpc.auth_token`, or
- `rpc.auth_token_file`.

Controls:

- minimum token length: 24 bytes;
- placeholder strings rejected;
- token file limited to 4 KiB;
- Unix token file must have no group/other permission bits;
- simultaneous real inline token and token file rejected;
- constant-time token comparison;
- exactly one of custom token header or Bearer header accepted;
- duplicate sensitive authentication headers rejected.

Recommended production token: 32 random bytes encoded as 64 hex characters.

## Network exposure

Default production posture:

```text
RPC disabled in standard mainnet/testnet configs.
Headless RPC bound to 127.0.0.1:17445.
No built-in TLS.
```

A non-loopback bind requires:

```text
rpc.allow_remote = true
rpc.allowed_cidrs = [explicit allowlist]
```

This is defense in depth, not a recommendation to expose RPC. Raw remote RPC should remain behind SSH port forwarding, WireGuard/Tailscale, or a separately reviewed TLS gateway. Caddy/Cloudflare should not be placed in front of state-changing RPC without an additional authentication and network-design review.

## HTTP parser hardening

HF122 rejects:

- requests over configured header/body limits;
- malformed request lines;
- non-UTF-8 headers;
- folded headers;
- duplicate `Content-Length`;
- duplicate `Transfer-Encoding`;
- duplicate `Authorization`;
- duplicate `X-QUB-RPC-Token`;
- any transfer encoding/chunked request body;
- bytes beyond the declared content length.

Sockets have configured read/write timeouts. Connections and per-IP requests per minute are bounded. Active connection count uses a drop guard so thread exit decrements the counter.

## RPC state model

### Embedded mode

`qubd node` starts RPC against the same `Arc<Mutex<ChainState>>` used by P2P. This prevents a second state-changing process from independently loading and writing the same chain file.

State-changing endpoints available:

- tracked block submission;
- validated transaction submission.

### Standalone mode

`qubd rpc-api` is read-only. Template, block-submit and transaction-submit requests return an embedded-node-required response.

## Mining template security

Template creation:

- validates payout address or existing on-chain pool ID;
- uses current canonical height, parent, required difficulty and expected block version;
- creates a fresh coinbase/extra nonce;
- tracks each job in a bounded cache;
- limits template batch size;
- shares immutable non-coinbase transaction tails across batch jobs;
- expires jobs after a configured TTL.

Submission:

- accepts only a tracked job ID and nonce;
- rejects unknown/expired jobs;
- recalculates the block hash and verifies proof of work;
- runs a bounded non-repairing live-tip guard;
- requires current canonical height/parent to match the job;
- requires the currently expected block version;
- connects through normal consensus validation;
- persists chain state before relay;
- invalidates sibling jobs for the accepted parent.

Untracked arbitrary full-block submission is intentionally unsupported.

## Transaction submission

`POST /rpc/v1/tx/submit`:

- deserializes a structured QUB transaction;
- verifies the transaction ID;
- submits through normal mempool validation;
- persists only after acceptance;
- broadcasts only accepted transactions;
- returns structured rejection without bypassing validation.

## Denial-of-service boundaries

Configured bounds include:

- maximum connections;
- per-IP requests per minute;
- header/body bytes;
- read/write timeouts;
- job TTL;
- cached jobs;
- template batch;
- mining statistics window;
- long-poll duration;
- mempool response limit.

Mining statistics copy only a bounded recent chain window and cache results per window/tip. Batch jobs share transaction tails to reduce memory amplification.

Remaining operational risk: every accepted connection still uses one bounded thread. This is acceptable for authenticated local/private infrastructure, not for anonymous public exposure.

## Mining-observability interpretation

Distribution is based on observable coinbase payout addresses and on-chain pool labels. One operator can use multiple labels and multiple operators can coordinate one label. Therefore:

```text
label count != human count
alternation != proof of independence
address share != exact hashrate share
```

The API and Explorer display this limitation explicitly.

## Reference miner boundary

`qub-rpc-miner` is a protocol reference and test worker. It:

- accepts one token source;
- supports HTTP loopback/private endpoints;
- validates template fields and lengths;
- performs QUB double-SHA256 target comparison;
- submits only tracked job IDs/nonces;
- stops and refreshes on expiry.

It does not provide TLS, Bitcoin Stratum, AxeOS compatibility, fleet management, thermal management, or payout accounting.

## Headless deployment controls

The supplied systemd service:

- runs as non-root `deploy`;
- uses a separate data directory;
- keeps RPC loopback-only;
- applies restrictive umask;
- sets file-descriptor limits;
- enables systemd filesystem/kernel hardening;
- permits writes only below `/opt/qub/headless`.

Token files and optional miner environment files must be mode `0600`.

## Testing performed in the review environment

Completed:

- Rust structural/offline headless compilation;
- RPC unit tests;
- reference-miner unit tests;
- core/consensus regression tests including #23999/#24000/#24001;
- regtest embedded-RPC end-to-end;
- unauthorized request rejection;
- duplicate sensitive-header rejection;
- unique tracked template batch;
- reference miner block discovery and accepted submit;
- canonical height increment;
- mining-observability attribution;
- Explorer JavaScript syntax validation;
- shell/Python syntax checks.

The review environment could not fetch the project’s real external Cargo registry dependencies or compile the complete eframe/wgpu GUI stack. A local real-dependency `cargo test` plus release builds for all three binaries is a mandatory release gate.

## Required release gates

Do not publish unless all pass on the actual source tree:

```text
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
python/py scripts/test-hf122-rpc-regtest.py
real-mainnet validate/preflight/status-fast
NYC3 canary
AMS3 seed and snapshot health
headless RPC loopback smoke
Windows installer/hash verification
Explorer v0.7 smoke
secret/runtime-file audit
```

## Security conclusion

HF122 is suitable for authenticated local/private headless and mining infrastructure after the required real-dependency gates pass. It is not a public anonymous mining API and it does not yet provide the hardware adapter required for stock Bitaxe/AxeOS devices.
