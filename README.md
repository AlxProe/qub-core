# QUB Core v1.7.9 HF122 — Headless Node, Authenticated RPC and Mining Infrastructure

HF122 / v1.7.9 completes the first production release of QUB-native headless node and mining infrastructure started in HF121. It adds **no new consensus activation**.

Protocol Epoch 2 remains active exactly as deployed:

- blocks below mainnet height **#24000** use block version **1**;
- blocks at and above **#24000** require block version **2**;
- no rollback, address-specific rule, DAA change, checkpoint change, genesis change, economics change, or QUB/JIN rule change is introduced by HF122.

## HF122 infrastructure

- Embedded authenticated RPC in `qubd node`, sharing the same canonical in-memory chain state as P2P.
- Read-only standalone RPC mode for diagnostics.
- Canonical chain, block, transaction and mempool read endpoints.
- Tracked solo and existing on-chain pool mining templates.
- Compact independent template batches for parallel workers.
- Canonical-parent, expiry, version and proof-of-work guards on block submission.
- Authenticated raw transaction submission through the normal mempool validation and relay path.
- Long-poll tip events.
- `qub-rpc-miner`, a QUB-native reference CPU worker.
- Mining-distribution observability: payout/pool labels, HHI, effective label count, same-label streaks, exact two-label alternation, coinbase-only rate, timing percentiles and block-version distribution.
- QUB Explorer v0.7 mining analytics support.
- A separate headless-mainnet configuration and hardened systemd service examples.

## Security defaults

- RPC is disabled by default in normal mainnet and testnet configs.
- The supplied headless config binds RPC to `127.0.0.1:17445`.
- Every RPC request requires a non-placeholder token.
- Token files must be owner-only on Unix (`chmod 600`).
- Remote binding requires both `allow_remote=true` and an explicit CIDR allowlist.
- RPC has no built-in TLS and must not be exposed directly to the public Internet.
- Request headers, bodies, connections, timeouts, request rates, cached jobs and batch sizes are bounded.
- Duplicate sensitive headers, folded headers and chunked request bodies are rejected.
- Mining jobs are tracked, expire, and are invalidated after a parent-tip change.
- Arbitrary untracked block submission is not supported.
- State-changing RPC requires embedded `qubd node` mode.

## Important hardware note

`qub-rpc-miner` is the QUB-native reference CPU worker and protocol reference. Stock Bitcoin Stratum/AxeOS devices, including Bitaxe Gamma, are **not directly compatible** with HF122 RPC. A reviewed QUB Stratum/worker adapter will be built and hardware-tested separately.

## Build

```bash
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

## Regtest RPC end-to-end test

```bash
python3 scripts/test-hf122-rpc-regtest.py \
  --qubd target/release/qubd \
  --miner target/release/qub-rpc-miner
```

On Windows PowerShell:

```powershell
py .\scripts\test-hf122-rpc-regtest.py `
  --qubd .\target\release\qubd.exe `
  --miner .\target\release\qub-rpc-miner.exe
```

Expected ending:

```text
HF122 RPC REGTEST E2E: PASS
```

## Documentation

- `README-RPC-MINER.md` — RPC and reference-miner usage.
- `HF122-v179-SECURITY-REVIEW.md` — security model, findings and remaining boundaries.
- `HF122-v179-DEPLOY-RUNBOOK.md` — end-to-end local, seed, headless-node, Explorer, distribution, GitHub and announcement workflow.
- `RELEASE_NOTES-v1.7.9-HF122.md` — release notes.

## No-assets package

Runtime image, audio and font assets are intentionally excluded. Copy the known-good `assets` directory from the currently deployed QUB Core source tree before producing the Windows distribution.
