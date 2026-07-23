# QUB Core v1.8.2 HF126

HF126 is the mandatory equal-height fork-recovery and mining-liveness release.

## Consensus status

HF126 introduces no new chain activation.

- Protocol Epoch 2 remains active at mainnet block #24000.
- Blocks from #24000 onward continue to require block version 2.
- DAA, block rewards, checkpoints, genesis, economics and QUB/JIN rules are unchanged.
- The 128 pool-share-transactions-per-block consensus limit is unchanged.
- The Fast Chain Engine schema and P2P protocol number remain unchanged.
- The USDJ bridge is not part of this release.

## Incident addressed

A locally mined and durably committed block could remain unacknowledged while public/direct sources reported another valid block at the same height. The previous green-light logic classified this equal-height proof-of-work tie as a permanent mining stop. The UI could therefore show a running miner with zero hashrate while repeatedly probing tips and never allowing either branch to gain additional cumulative work.

## Core fixes

- Keeps ordinary fully validated equal-height proof-of-work branches mineable.
- Stops mining only for a genuinely ahead/higher-work view, invalid state, parent change or other consensus-safety condition.
- Adds a strict verified equal-work re-anchor path for a locally mined unacknowledged tip.
- Requires durable pending-relay identity plus matching multi-source evidence for the exact competing hash.
- Fully replays consensus/checkpoints before an equal-work replacement.
- Preserves normal Fast Chain Engine monotonicity; ordinary writes still reject arbitrary equal-work siblings.
- Extends acknowledged block delivery with bounded overlap repair when a receiver is on the equal-height sibling branch.
- Retries the exact found block after the receiver adopts the higher-work suffix.
- Records competing stale-parent reports in the durable pending relay status.
- Increases bounded acknowledgement/relay windows for branch repair.

## GUI fixes

- Reports `MINING` only while measured hashrate is non-zero.
- Reports `WAITING` during sync, acknowledgement or re-anchor work and `PREPARING` during template startup.
- Shows local and observed network tips separately at the same height.
- Renames the recent block panel to `Recent chain blocks`.
- Displays separate local and network candidate rows instead of presenting one as global truth before convergence.
- Preserves the same-height network hash in GUI snapshots.
- Coalesces repetitive catch-up/mining-wait status lines.
- Clarifies stale local candidate wording.

## Retained fixes

HF123 Fast Chain Engine, HF124 share-pressure mining liveness, HF125 acknowledged block delivery, authenticated RPC, `qub-rpc-miner`, snapshot publication and the optimized Explorer API remain included.

## Required validation

```text
cargo test --locked
three release builds
HF124 focused regressions
HF125 focused regressions
HF126 focused regressions
HF123 Fast Chain Engine E2E
HF123 RPC E2E
HF125 reliable block-delivery E2E
HF126 equal-height overlap-delivery E2E
real-mainnet status-fast / storage-stats / validate / preflight
GUI equal-height local/network smoke
NYC3 canary
AMS3 deployment
fresh snapshot and public Windows verification
```
