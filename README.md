# QUB Core v1.8.4 HF128 — Mining and Network Liveness

HF128 is a mandatory, non-consensus reliability release built on the HF123
Fast Chain Engine, HF124 mining liveness, HF125 acknowledged block delivery,
HF126 equal-height recovery and HF127 reliable transaction delivery.

## Network identity

- No chain upgrade and no new activation height.
- Protocol Epoch 2 remains at mainnet block **#24000**.
- Post-activation block version remains **2**.
- P2P protocol remains **2**.
- Fast Chain Engine remains **QUB-FCE-1**.
- Block/transaction hashes, PoW, DAA, rewards, checkpoints, pools, QUB and JIN
  validation rules are unchanged.

## HF128 highlights

- Restores the pre-HF127 automatic 100% CPU saturation plan: two disjoint nonce
  lanes per logical CPU, with no fixed worker cap.
- Makes the GPU percentage a real measured active/idle duty independent of
  stable per-device batch tuning; 100% dispatches continuously.
- Applies changed mining controls to active workers through an acknowledged
  stop/restart generation and reports applied rather than desired power.
- Splits oversized OpenCL work into continuous interruptible dispatches and
  corrects CPU/GPU hashrate and cumulative telemetry across round changes.
- Removes the pool-wide QUB Melt deadlock through a backward-compatible,
  markerless `POOLCOMPAT1` template policy accepted by unchanged HF127 nodes.
- Bootstraps an empty PPLNS window from valid pending shares, so a solo miner is
  no longer required to start or restart pool mining.
- Replaces handshake-from-genesis floods with one staged suffix/overlap request,
  preserves HF127 protocol-2 count-window framing, rejects self-alias connections
  and deduplicates seed evidence by persisted node ID.
- Queues manual Sync/Repair behind an active auto-catchup instead of dropping it.
- Loads Explorer API chain data and its exact FCE commit identity atomically.
- Repairs Windows update staging, timeout, SHA-256, error and installer-result
  handling.

## Pool compatibility policy

`POOLCOMPAT1` is a diagnostic/template tag only. It deliberately does not match
the consensus `POOLBLOCK1` parser. It is used only when a legacy marked pool
block would be impossible (QUB Melt) or when valid pending shares must bootstrap
an empty confirmed window. QUB payouts remain deterministic. Native-JIN-fee
transactions wait for the next ordinary marked pool block.

## Operator commands retained

```bash
qubd --config <config> status-fast
qubd --config <config> storage-stats
qubd --config <config> validate
qubd --config <config> sync
qubd --config <config> peers
qubd --config <config> tx-delivery-status [txid]
qubd --config <config> relay-pending-txs
```

## Release documentation

- `RELEASE_NOTES-v1.8.4-HF128.md`
- `HF128-v184-RELIABILITY-REVIEW.md`
- `HF128-v184-DEPLOY-RUNBOOK.md`
- `README-FAST-CHAIN-ENGINE.md`
- `README-RPC-MINER.md`

Runtime image/audio/font assets are intentionally omitted from source-only
packages. Copy the known-good deployed assets into `assets/` before building the
Windows GUI distribution.
