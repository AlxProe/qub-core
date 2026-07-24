# QUB Core v1.8.3 HF127 — Reliable Transactions & Low-Latency Mining

HF127 / v1.8.3 is a mandatory, non-consensus reliability release built on the HF123 Fast Chain Engine, HF124 mining-liveness engine, HF125 acknowledged block delivery, and HF126 equal-height fork recovery.

## Network identity

- No new activation height.
- Protocol Epoch 2 remains active from mainnet block **#24000**.
- Post-activation block version remains **2**.
- P2P protocol number remains **2**.
- Fast Chain Engine remains **QUB-FCE-1**.
- Block, transaction, address, reward, DAA, checkpoint, pool, QUB and JIN consensus rules are unchanged.

## Reliable transaction delivery

Ordinary wallet and protocol transactions now use explicit application-level delivery messages:

```text
SubmitTx
TxAck
```

A transaction is considered delivered when QUB Core receives:

```text
one positive official-seed ACK
or two positive ACKs from distinct direct peers
or one explicit positive ACK in an isolated deployment with no official destination
```

Successful socket writes are no longer presented as proof of peer acceptance. A positive ACK is returned only after the receiver has confirmed the transaction in canonical state or durably committed it to QUB-FCE-1. Raw transactions and delivery metadata remain durable in `wallet-pending-txs.json` until canonical confirmation depth is reached.

Seeds expose a bounded `tx-submit` reserve lane so transactions can still be accepted when normal inbound peer slots are full. Delivery retries run in an independent single-flight worker and do not depend on sender mining, GUI polling, Explorer availability, or manual relay.

## Transaction construction safety

Before signing, GUI and CLI transaction flows require validated current-chain evidence. Coin selection and signing do not proceed from a stale, incompatible or unacknowledged local branch.

Operator commands:

```bash
qubd --config <config> tx-delivery-status [txid]
qubd --config <config> relay-pending-txs
```

## Low-latency block and transaction propagation

HF127 reduces the time advantage of a miner that has just found a block:

- latency-sensitive P2P sockets use `TCP_NODELAY`;
- official/configured bootnodes are attempted before historical peerbook entries;
- outbound dialing is detached, bounded and protected by per-peer exponential backoff;
- ephemeral `block-submit` and `tx-submit` source ports are not persisted into the peerbook;
- seeds begin outward diffusion after durable acceptance and before returning the producer ACK;
- live block/transaction relay reuses the registered in-memory canonical state when available;
- block/transaction delivery can complete without making official seeds a permanent liveness dependency.

## Full-power mining controls

QUB Core exposes the complete **1–100% CPU** and **0–100% GPU** ranges. CPU and GPU percentages are available through both sliders and exact numeric inputs.

At **85% or above**, or when a manual worker/work-item override is used, QUB Core displays a red hardware-risk warning and requires a session-only mandatory confirmation before solo or pool mining can start.

At 100% CPU power, QUB Core uses all logical processors with continuous duty. Explicit CPU worker and GPU work-item overrides remain available for advanced operators and are not constrained by a QUB-specific software ceiling; operating-system and device-driver limits still apply. If any requested CPU worker cannot be created, the round fails closed instead of silently running a partial nonce-lane plan. If no CPU/GPU worker starts, the GUI does not claim that mining is active.

The reference RPC miner no longer imposes the previous 128-worker QUB limit and assigns disjoint nonce lanes to workers.

## Retained reliability layers

HF127 retains:

- HF123 append-only Fast Chain Engine and single canonical in-memory owner;
- HF124 consensus-valid pool-share templates, stable mining rounds, batched persistence and fair relay;
- HF125 explicit `SubmitBlock` / `BlockAck`, durable pending block retry and reserve submission lane;
- HF126 mineable equal-height PoW ties, verified re-anchor path and sibling-branch overlap delivery;
- Explorer API mempool optimization and the v0.7.1 same-origin Explorer stack.

## Release documentation

- `RELEASE_NOTES-v1.8.3-HF127.md`
- `HF127-v183-RELIABILITY-REVIEW.md`
- `HF127-v183-DEPLOY-RUNBOOK.md`
- `README-FAST-CHAIN-ENGINE.md`
- `README-RPC-MINER.md`

Runtime image/audio/font assets are intentionally omitted from source-only packages. Copy known-good assets into `assets/` before building the Windows GUI distribution.
