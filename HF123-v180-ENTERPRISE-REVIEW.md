# HF123 / v180 Enterprise Review

## Release identity

```text
QUB Core: v1.8.0
Hotfix: HF123
Package generation: v180
Storage engine: QUB-FCE-1
```

HF123 is a non-consensus architecture release. Protocol Epoch 2 remains active at mainnet height #24000 and block version 2 remains required thereafter.

## Primary risk addressed

Before HF123, a growing `chain.json` file served simultaneously as persistence, synchronization handoff and GUI read model. Repeated full-file reads, JSON parsing, state reconstruction and full-file writes produced multi-minute or multi-hour behavior on ordinary hardware.

HF123 separates:

- canonical in-memory state;
- committed block history;
- mutable UTXO/mempool state;
- operational status metadata;
- compatibility exports.

## Fast Chain Engine safety model

### Committed state

A committed pointer references:

- one append-only block-journal generation;
- one immutable state snapshot;
- the committed journal byte prefix;
- height, tip hash/header, total work and mempool digest;
- state SHA-256.

The current pointer is atomically replaced only after journal and state persistence complete.

### Previous commit

`PREVIOUS.json` preserves the immediately preceding pointer. Readers attempt it if the complete `CURRENT.json` commit is missing, malformed or inconsistent, including referenced state or journal validation failures. If both commits are unusable, startup fails closed. QUB Core does not silently recreate Fast Chain Engine state from a potentially stale compatibility `chain.json` export.

### Interrupted writes

Bytes after the committed journal prefix are not part of canonical state. The next writer truncates that suffix before appending.

### Branch replacement

A branch change creates a new journal generation. The committed old generation remains available through the previous pointer during the transition.

### Validation boundaries

Fast structural loading verifies:

- storage schema and network;
- pointer/state consistency;
- state checksum;
- mempool digest;
- genesis;
- height-specific block version;
- block-to-block hash links;
- committed block count and tip;
- fork-safety checkpoint when present.

`qubd validate` remains the full consensus replay gate and is mandatory before public deployment.

## Concurrency model

- One state-changing process per data directory.
- Process-local persistence mutex.
- On-disk writer lease with PID and conservative stale age.
- Windows stale-owner checks use the local process table; uncertain results fail closed.
- P2P owns the canonical `Arc<Mutex<ChainState>>`.
- GUI, embedded RPC and Explorer receive immutable copy-on-write snapshots.
- A separate headless service uses a separate data directory and P2P port.

## Performance model

Normal block append no longer serializes the complete historical block list. It appends new block records and commits the current UTXO/mempool state once.

Catch-up connects a suffix in memory and persists once per successful repair stage. The GUI no longer reads the complete compatibility export every refresh, and P2P handlers no longer load chain state per message.

Operational counters expose:

```text
process loads / commits
bytes read / written
last and maximum load duration
last and maximum commit duration
legacy export count
journal/state/legacy sizes
```

## Compatibility export

`chain.json` remains available for external compatibility but is not the hot persistence path. It is refreshed periodically or explicitly through:

```bash
qubd --config <config> export-chain-json <output>
```

The public snapshot publisher calls this committed exporter before generating public artifacts.

## RPC posture

HF122 RPC security remains:

- disabled by default in normal mainnet/testnet configs;
- loopback-only headless template;
- token or owner-only token-file authentication;
- constant-time comparison;
- remote CIDR allowlist requirements;
- request, connection, rate, job and timeout bounds;
- rejection of duplicate sensitive headers and transfer encoding;
- tracked, expiring mining jobs;
- state-changing endpoints only in embedded node mode;
- no built-in TLS and no supported direct public exposure.

## Observability posture

Mining statistics are aggregate-only. They report payout/pool-label distribution, HHI, effective label count, coinbase-only rate, intervals and block versions. They do not infer human identity, operator ownership or coordination from address ordering.

## Migration controls

Production migration requires:

1. Complete data-directory backup.
2. Existing chain validation with the release binary.
3. First startup under bounded operational monitoring.
4. `status-fast`, `storage-stats`, `validate` and `preflight` checks after migration.
5. Canonical tip comparison with the public snapshot.
6. Retention of the legacy chain export and backup until the release is proven stable.

## Remaining boundaries

- Startup still parses committed block records once to build the in-memory chain.
- The current state snapshot contains the complete UTXO set and mempool; future storage versions may segment or database-index this state.
- The first legacy migration requires a full consensus validation and can take significant time.
- Bitaxe/AxeOS requires a separately reviewed adapter and physical hardware testing.
- Full Rust dependency compilation must be performed on the release workstation and seed build hosts; static artifact audits are not a substitute.

## Mandatory release gates

```text
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
HF123 Fast Chain Engine regtest E2E
HF123 RPC regtest E2E
snapshot publisher self-test
real-mainnet full validate
real-mainnet preflight
NYC3 canary migration
AMS3 migration and snapshot publication
public seed reachability
Windows installer/header/hash verification
public manifest and snapshot verification
clean install smoke
```

A release must stop on any failed gate, unexpected pre-#24000 state, non-v2 mainnet tip, storage-pointer inconsistency, seed listener failure, snapshot generation mismatch or public artifact hash mismatch.
