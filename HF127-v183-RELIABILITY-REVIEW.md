# HF127 / v1.8.3 Reliability and Security Review

## Scope

HF127 reviews the complete transaction and mining communication path after HF126 made block production and equal-height fork recovery reliable. The review covers transaction construction, local admission, wallet outbox durability, peer delivery, seed saturation, peerbook behavior, outbound dialing, receiver diffusion, mining controls and independent-miner confirmation.

## Confirmed pre-HF127 failure model

The sender always admitted its own transaction locally before network delivery. Its local miner therefore saw the transaction immediately. Remote miners could not include the transaction until it reached their independent mempools.

The old transaction relay counted successful socket writes rather than peer acceptance. When normal inbound slots were full, reserve-lane sessions rejected ordinary node/miner roles, while the sender could already have counted its TCP write. Periodic relay could then be delayed by synchronous dials to stale peerbook entries. GUI status polling also caused repeated blind rebroadcasts.

The practical result was asymmetric:

```text
sender mining ON  -> transaction often confirms quickly
sender mining OFF -> transaction can remain local or arrive much later
Explorer mempool  -> can remain zero because it reports the AMS3 node mempool
```

There was no consensus requirement that the caller mine its own transaction.

## HF127 transaction protocol

### Explicit ACK

`SubmitTx` carries a request ID and full transaction. `TxAck` returns the request ID, txid, status, receiver height/tip and reason. A positive ACK is emitted only when the transaction is already canonically confirmed or its exact mempool state is durably committed to QUB-FCE-1.

Positive statuses:

```text
accepted
already_known
already_confirmed
```

Negative status:

```text
rejected
```

### Delivery quorum

Official seeds are preferred but are not a permanent liveness dependency.

Delivery is proven by:

```text
one positive official ACK
or two positive ACKs from distinct non-official peers
or one explicit positive ACK when no official destination was attempted
```

Duplicate ACKs from the same endpoint do not satisfy the direct-peer quorum. Legacy writes never count as proof.

### Durable outbox

`wallet-pending-txs.json` version 2 remains backward compatible through serde defaults. It records attempts, retry schedule, peer and official ACK totals, last ACK status/tip/height, rejection reason and delivered timestamp. Records remain until canonical confirmation depth is reached and are restored after restart or shallow reorg. Corrupt outbox data fails closed instead of being silently replaced. A full outbox never evicts an unconfirmed ordinary transaction, idempotent re-admission preserves ACK/backoff history, and manual retry is capped to a bounded batch of eight records.

### Reserve lane

A `tx-submit` reserve session accepts only bounded `Version`, `SubmitTx`, `Ping` and `Pong` messages. Legacy `Tx`, chain, mempool, peer-list and block-submission messages are rejected in the reserve lane. An unverified socket has a three-second handshake deadline; verified sessions retain bounded message, submission and lifetime budgets.

## Canonical construction gate

GUI and CLI signing paths use a hard current-parent guard. A transaction is not created when validated official/direct evidence indicates the local chain is behind or on an incompatible sibling. The gate runs before coin selection/signing and fails closed.

## Propagation review

### Receiver-side diffusion

A seed validates and durably persists a block or transaction first. It then starts outward diffusion before returning the producer/sender ACK. This reduces the period in which only the originator knows the new object.

### Socket latency

Accepted and outbound P2P streams use `TCP_NODELAY`. Persistent sessions announce changed canonical tips on the next one-second coordination tick, with a four-second fallback heartbeat.

### Outbound dialing

The central node loop no longer performs an unbounded sequence of blocking three-second dials. HF127 schedules at most four detached attempts per tick, uses a 900 ms connect timeout and applies per-peer exponential backoff. Mempool merge, persistence, tx retry, block retry and tip heartbeat continue independently.

### Peerbook hygiene

Short-lived `block-submit` and `tx-submit` connections use ephemeral source ports. HF127 does not persist those observed endpoints. The peerbook is capped at 512 entries on load, in memory and on save; bootnodes stay ahead of historical entries. Detached propagation/dial workers use fallible thread creation, so resource exhaustion is reported without crashing the canonical node.

## Mining-control review

The GUI exposes the full 1–100% CPU and 0–100% GPU ranges with exact numeric percentage inputs. High-power authorization is session-only and resets after setting changes or application restart.

At or above 85%, or with advanced manual worker/work-item overrides, a red warning describes heat, PSU, battery, cooling, driver and stability risks. Solo and pool start actions fail closed until the checkbox is accepted.

CPU worker creation uses fallible named thread creation and reports allocation failure rather than crashing. If any requested CPU worker cannot start, the mining round aborts instead of continuing with an incomplete nonce-lane plan; a round with no CPU/GPU worker cannot enter the active-mining state. GPU allocation remains subject to OpenCL device/driver limits. The RPC miner removes the prior QUB worker cap, separates worker nonce lanes and applies the same fail-closed worker-start rule.

## Consensus invariants

The following remain unchanged:

```text
Protocol Epoch 2 activation #24000
post-activation block version 2
P2P protocol number 2
QUB-FCE-1 storage schema
block and transaction serialization
DAA, rewards, supply and checkpoints
pool-share consensus cap and payout rules
QUB/JIN mechanics
```

## Required validation

The source package is not considered production-ready until it passes the dependency-backed Cargo builds and all retained/new E2Es in the deployment runbook. The critical HF127 E2E proves reserve-lane delivery, explicit ACK, sender mining disabled, seed-to-independent-miner propagation, confirmation by another wallet/miner, two-confirmation outbox cleanup and receiver persistence across restart.
