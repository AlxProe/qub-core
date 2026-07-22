# QUB Core v1.8.1 HF124 — Mining Liveness

HF124 / v1.8.1 is a mandatory, non-consensus mining-liveness and mempool-pressure release. It preserves the HF123 Fast Chain Engine and the active Protocol Epoch 2 rules.

Protocol Epoch 2 remains unchanged:

- blocks below mainnet height **#24000** use block version **1**;
- blocks at and above **#24000** require block version **2**;
- DAA, checkpoints, genesis, economics and QUB/JIN consensus rules are unchanged.

## Why HF124 exists

A sustained pool-share burst exposed several interacting local-node liveness problems:

- the candidate builder could select more pool-share transactions than the existing consensus limit of 128 per block;
- GUI mining rounds restarted whenever the mempool changed, even though a mempool-only change does not invalidate the current proof-of-work parent;
- a pool miner could submit another local share after each round restart;
- each incoming share could rebuild historical pool context and trigger unrelated protocol-state validation;
- P2P persisted mempool-only changes while holding the canonical state mutex;
- share-first relay ordering could crowd ordinary QUB/JIN/Library traffic out of bounded relay batches.

Together these paths could create continuous share growth while miners repeatedly abandoned work or hashed templates that could never be accepted.

## HF124 liveness engine

HF124 adds the following non-consensus controls:

- **exact candidate cap:** at most `max_share_txs_per_block` pool shares are selected; extra shares remain pending for later blocks, with the oldest still-confirmable shares drained first;
- **ordinary-transaction preservation:** additional shares never prevent eligible non-share transactions from being considered;
- **stable mining rounds:** mempool-only changes no longer cancel CPU/GPU mining; canonical parent/version/target changes still stop the round immediately;
- **shared candidate assembly:** the validated transaction set is built once per round and reused by all CPU/GPU workers; each worker changes only coinbase extra nonce and header nonce;
- **consensus/DAA-only pacing:** legacy GUI target-spacing jitter and last-winner cooldown sleeps are removed; canonical parent/version/target guards remain;
- **one local share per parent:** the GUI pool miner cannot create a self-amplifying share/rebuild loop;
- **bounded share mempool policy:** pool-share retention is limited to the maximum confirmable stale horizon;
- **cached pool registry:** pool context is reconstructed once and incrementally advanced by canonical tip, then reused by mempool admission, block templates, pool payouts and GUI share creation rather than scanning genesis for every share;
- **bounded share window:** share membership and duplicate checks scan only their consensus-relevant windows;
- **batch admission:** inbound mempool batches reuse one validation context and one bounded confirmed-share window;
- **share fast path:** pool-share admission skips unrelated QNS, JIN, Library and governance state reconstruction;
- **coalesced non-blocking persistence:** mempool-only Fast Chain Engine state is snapshotted under the canonical mutex at most once per five-second window, then committed after releasing the mutex; stale same-tip snapshots that are already subsets of the live owner are ignored without revalidation;
- **fair relay batches:** ordinary traffic receives reserved relay capacity during share bursts;
- **bounded relay transport:** locally created shares use a small official-first fanout; inbound one-by-one shares are deferred to the fair heartbeat; periodic and inbound batch propagation sends one bounded `Mempool` message per peer instead of opening one connection per transaction;
- **single-snapshot fanout:** each relay fanout captures one immutable Fast Chain Engine snapshot instead of reloading canonical storage for every destination peer;
- **ephemeral share handling:** pool shares are excluded from the durable wallet pending outbox, and legacy persisted share records are dropped during reconciliation;
- **early consensus rejection:** a block declaring more than 128 pool shares is rejected before expensive pool-state reconstruction.

The existing consensus limit remains 128 pool-share transactions per block. HF124 does not change that limit; it makes official template construction obey it deterministically.

## Fast Chain Engine and infrastructure

HF123 remains fully included:

- `QUB-FCE-1` append-only block journal and immutable state snapshots;
- canonical in-memory P2P owner;
- copy-on-write chain snapshots;
- batched catch-up persistence;
- `status-fast`, `storage-stats` and `export-chain-json`;
- authenticated headless RPC and `qub-rpc-miner`;
- optimized read-only Explorer mempool API.

Standard public seed RPC remains disabled. The Explorer API remains a separate read-only service.

## Mandatory build and validation gates

```bash
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

HF124-specific regression gate:

```bash
cargo test hf124_ -- --nocapture
```

HF123 storage and RPC regressions remain mandatory:

```bash
python3 scripts/test-hf123-fast-chain-regtest.py \
  --qubd target/release/qubd

python3 scripts/test-hf123-rpc-regtest.py \
  --qubd target/release/qubd \
  --miner target/release/qub-rpc-miner
```

Windows PowerShell:

```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
cargo test hf124_ -- --nocapture

py .\scripts\test-hf123-fast-chain-regtest.py `
  --qubd .\target\release\qubd.exe

py .\scripts\test-hf123-rpc-regtest.py `
  --qubd .\target\release\qubd.exe `
  --miner .\target\release\qub-rpc-miner.exe
```

## Documentation

- `README-FAST-CHAIN-ENGINE.md` — Fast Chain Engine storage and recovery model.
- `README-RPC-MINER.md` — headless RPC and reference miner.
- `HF124-v181-RELIABILITY-REVIEW.md` — liveness, safety and boundary review.
- `HF124-v181-DEPLOY-RUNBOOK.md` — complete mainnet deployment procedure.
- `RELEASE_NOTES-v1.8.1-HF124.md` — release notes.

## No-assets package

Runtime images, audio and fonts are intentionally excluded. Copy the known-good `assets` directory from the deployed QUB Core tree before building the Windows distribution.
