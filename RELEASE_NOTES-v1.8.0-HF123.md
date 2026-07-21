# QUB Core v1.8.0 HF123

HF123 is the Fast Chain Engine and low-latency full-node release.

## Consensus status

HF123 introduces no new consensus activation.

- Protocol Epoch 2 remains active at mainnet block #24000.
- Blocks below #24000 use block version 1.
- Blocks from #24000 onward require block version 2.
- DAA, checkpoints, genesis, economics and QUB/JIN rules are unchanged.
- The USDJ bridge is not part of this release.

## Fast Chain Engine

Normal persistence now uses `QUB-FCE-1` instead of repeatedly rewriting the complete monolithic chain file:

- append-only block journal;
- immutable state snapshots;
- atomic `CURRENT.json` and `PREVIOUS.json` pointers;
- state SHA-256 verification;
- journal suffix recovery after interrupted writes;
- complete previous-commit recovery when the current pointer, state or journal is unusable;
- fail-closed startup if both committed generations are unusable, without silently reimporting a potentially stale compatibility export;
- one-time validated legacy migration;
- periodic compatibility `chain.json` export;
- operator storage metrics and explicit export commands.

## Canonical in-memory node

P2P, embedded RPC and GUI snapshots now share one canonical in-memory state owner. Incoming P2P messages no longer reload the complete chain from disk. ChainState snapshots share immutable block, UTXO and mempool data through copy-on-write Arcs.

## Catch-up and GUI

- suffix blocks are validated in memory and persisted in a controlled batch;
- catch-up and snapshot workers are true single-flight;
- GUI derived views are cached per canonical tip;
- lightweight Fast Chain Engine metadata replaces full-chain height probes;
- the status area keeps a timestamped history instead of rapidly replacing one line;
- Explorer API reads an immutable committed cache instead of reparsing the chain for every request.

## Snapshot publisher

The production publisher exports one committed Fast Chain Engine generation and retains exact public schemas. It validates mainnet identity, complete block hash-link continuity and the Protocol Epoch 2 version boundary before publication. Artifacts are staged and `tip.json` is published last.

## Headless infrastructure

HF122 authenticated RPC and `qub-rpc-miner` remain included. RPC is disabled by default in standard public configs and has no built-in TLS. Mining statistics expose neutral aggregate measurements only.

## Mandatory validation

Before release:

```text
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
HF123 Fast Chain Engine regtest E2E
HF123 RPC regtest E2E
real-mainnet status-fast / storage-stats / validate / preflight
clean installation smoke
```
