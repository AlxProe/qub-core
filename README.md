# QUB Core v1.8.2 HF126 — Equal-Height Fork Recovery

HF126 / v1.8.2 is a mandatory, non-consensus liveness and observability release for equal-height competing tips. It includes the HF123 Fast Chain Engine, the HF124 mining-liveness engine and the HF125 acknowledged block-delivery protocol.

Protocol Epoch 2 remains unchanged:

- blocks below mainnet height **#24000** use block version **1**;
- blocks at and above **#24000** require block version **2**;
- DAA, block rewards, checkpoints, genesis, economics and QUB/JIN consensus rules are unchanged;
- HF126 introduces no new activation height or block format.

## Incident addressed

A locally mined block could be fully valid, durably committed and displayed as local `pending #N`, while independent network sources reported a different valid block at the same height. The previous mining green-light logic treated this normal proof-of-work tie as a permanent stop condition:

```text
local tip #N = A
network tip #N = B
same cumulative work
→ mining paused indefinitely
```

That behavior prevented either branch from gaining the next unit of cumulative work. It also made the GUI show `MINING` while the real hashrate was zero, mixed local and network candidates into one “global” list and repeatedly replaced the bottom status history with alternating catch-up messages.

HF126 restores normal cumulative-work liveness while retaining strict safeguards for a locally mined block that has never been acknowledged publicly.

## HF126 fork-recovery model

### Ordinary equal-height ties remain mineable

A fully validated local branch and a different same-height network branch are an unresolved proof-of-work tie. HF126 does not treat the sibling hash alone as proof that the local parent is invalid.

Mining may continue on the active validated local tip. The first branch to receive a valid next block becomes higher-work and wins through the existing chain-selection rules.

Mining still stops immediately for:

```text
higher validated network height/work
invalid local state
wrong required block version
changed canonical parent during a mining round
explicit stop
```

### Verified re-anchor for an unacknowledged local tip

A local tip with an active `pending-block-relay.json` record receives stricter handling. QUB Core may replace an equal-work sibling only when all of the following hold:

```text
- mainnet;
- local pending relay exactly matches the local tip;
- independent official/direct observations agree on one exact competing hash;
- the downloaded branch fully replays consensus and checkpoints;
- height and cumulative work are equal;
- the replacement uses the explicit verified Fast Chain Engine commit policy.
```

Ordinary Fast Chain Engine writes continue to reject equal-work, equal-height, different-tip overwrites.

### Winning-branch overlap delivery

If a found block extends one side of an equal-height tie, a receiving seed may still be on the sibling tip. HF126 block delivery now handles that response on the same connection:

```text
SubmitBlock
→ stale_parent at the sibling height
→ send bounded overlapping chain window
→ receiver validates the common ancestor and higher-work suffix
→ resubmit the exact found block
→ accepted/already_known BlockAck
```

This extends the HF125 ancestor-suffix repair to sibling-fork delivery.

## GUI and operator improvements

HF126 makes the local and observed network state explicit:

- the top mining indicator reports `MINING` only when measured hashrate is non-zero;
- paused workers display `WAITING`, and startup/template work displays `PREPARING`;
- Live Chain shows separate local and network heights/tips;
- same-height local and network candidates appear as separate rows;
- `Recent global blocks` is renamed to `Recent chain blocks`;
- a local pending candidate is not presented as a confirmed global block;
- repetitive background-sync and mining-wait messages are coalesced in the status history;
- local stale candidates are described as replaced by the active higher-work chain.

## Retained reliability layers

HF126 keeps all previous fixes:

- HF123 QUB-FCE-1 Fast Chain Engine;
- HF124 valid share-capped templates, stable mining rounds and bounded share processing;
- HF125 `SubmitBlock` / `BlockAck`, durable pending delivery, seed reserve lane, atomic persistence and automatic retry;
- authenticated RPC and `qub-rpc-miner`;
- optimized read-only Explorer API and snapshot publisher.

## Mandatory build and validation gates

```bash
cargo test --locked
cargo build --locked --release --bin qubd
cargo build --locked --release --bin qub-core
cargo build --locked --release --bin qub-rpc-miner
cargo test --locked hf124_ -- --nocapture
cargo test --locked hf125_ -- --nocapture
cargo test --locked hf126_ -- --nocapture
```

End-to-end gates:

```bash
python3 scripts/test-hf123-fast-chain-regtest.py --qubd target/release/qubd
python3 scripts/test-hf123-rpc-regtest.py --qubd target/release/qubd --miner target/release/qub-rpc-miner
python3 scripts/test-hf125-block-relay-regtest.py --qubd target/release/qubd
python3 scripts/test-hf126-equal-height-fork-regtest.py --qubd target/release/qubd
```

Windows PowerShell equivalents are included in `HF126-v182-DEPLOY-RUNBOOK.md`.

## Documentation

- `README-FAST-CHAIN-ENGINE.md` — Fast Chain Engine storage and recovery model.
- `README-RPC-MINER.md` — headless RPC and reference miner.
- `HF126-v182-RELIABILITY-REVIEW.md` — fork-choice, persistence and delivery review.
- `HF126-v182-DEPLOY-RUNBOOK.md` — complete local, seed, Windows, repository and public rollout procedure.
- `RELEASE_NOTES-v1.8.2-HF126.md` — release notes.

## No-assets package

Runtime images, audio and fonts are intentionally excluded. Copy the known-good `assets` directory from the deployed QUB Core tree before building the Windows distribution.
