# QUB Core v1.8.0 HF123 — Fast Chain Engine

HF123 / v1.8.0 is the QUB Core storage, synchronization and GUI-performance release. It introduces **no new consensus activation**.

Protocol Epoch 2 remains active exactly as deployed:

- blocks below mainnet height **#24000** use block version **1**;
- blocks at and above **#24000** require block version **2**;
- DAA, checkpoints, genesis, economics and QUB/JIN consensus rules are unchanged.

## Fast Chain Engine

HF123 replaces the monolithic `chain.json` hot path with `QUB-FCE-1`:

```text
chain-v2/
├── CURRENT.json
├── PREVIOUS.json
├── WRITE.lock
├── blocks-<generation>.jsonl
├── state-<generation>-<revision>.json
└── legacy-export-status.json
```

Key properties:

- append-only committed block journal;
- immutable state snapshots and atomic current/previous pointers;
- one-time validated migration from the existing `chain.json`;
- recovery from the previous fully validated committed generation;
- fail-closed behavior if both committed generations are unusable;
- truncation of uncommitted journal suffixes after interrupted writes;
- one canonical in-memory `ChainState` shared by P2P, embedded RPC and GUI snapshots;
- copy-on-write block, UTXO and mempool snapshots;
- batched catch-up persistence instead of repeated full-chain rewrites;
- infrequent compatibility `chain.json` export for external tools;
- `status-fast`, `storage-stats` and `export-chain-json` operator commands.

## GUI and synchronization

HF123 removes repeated full-chain JSON loading from hot P2P and GUI paths. It adds:

- true single-flight chain catch-up and snapshot workers;
- cached derived QNS, pool, JIN, governance and QUB/JIN views per canonical tip;
- stable timestamped status history;
- lightweight local height detection through Fast Chain Engine metadata;
- immutable Explorer API cache keyed by the committed storage identity.

## Headless RPC and reference miner

HF122 authenticated infrastructure remains included:

- embedded, token-authenticated RPC in `qubd node`;
- canonical chain, block, transaction and mempool endpoints;
- tracked solo and existing on-chain pool templates;
- compact parallel template batches;
- guarded tracked block submission;
- `qub-rpc-miner` reference CPU worker.

Standard mainnet/testnet RPC remains disabled by default. The supplied headless configuration is loopback-only. Raw RPC has no built-in TLS and must not be exposed directly to the public Internet.

Mining observability is intentionally neutral and aggregate-only: payout/pool label distribution, top-label share, HHI, effective label count, coinbase-only rate, block intervals and block-version distribution. A payout label is not proof of a unique operator or exact hashrate share.

## Build and mandatory release gates

```bash
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

Fast Chain Engine E2E:

```bash
python3 scripts/test-hf123-fast-chain-regtest.py \
  --qubd target/release/qubd
```

RPC/miner E2E:

```bash
python3 scripts/test-hf123-rpc-regtest.py \
  --qubd target/release/qubd \
  --miner target/release/qub-rpc-miner
```

Windows PowerShell:

```powershell
py .\scripts\test-hf123-fast-chain-regtest.py `
  --qubd .\target\release\qubd.exe

py .\scripts\test-hf123-rpc-regtest.py `
  --qubd .\target\release\qubd.exe `
  --miner .\target\release\qub-rpc-miner.exe
```

Expected endings:

```text
HF123 FAST CHAIN ENGINE REGTEST E2E: PASS
HF123 RPC REGTEST E2E: PASS
```

## Documentation

- `README-FAST-CHAIN-ENGINE.md` — storage format, migration and recovery model.
- `README-RPC-MINER.md` — headless RPC and reference-miner usage.
- `HF123-v180-ENTERPRISE-REVIEW.md` — safety and architecture review.
- `HF123-v180-DEPLOY-RUNBOOK.md` — end-to-end deployment procedure.
- `RELEASE_NOTES-v1.8.0-HF123.md` — release notes.

## No-assets package

Runtime image, audio and font assets are intentionally excluded. Copy the known-good `assets` directory from the currently deployed QUB Core source tree before producing the Windows distribution.
