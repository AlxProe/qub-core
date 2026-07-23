# HF125 / v182 Reliability Review

## Release identity

```text
QUB Core version: 1.8.2
Hotfix: HF125
Package generation: v182
Consensus activation: none
Fast Chain Engine schema: QUB-FCE-1 unchanged
P2P protocol number: 2 unchanged
```

## Confirmed failure model

The affected miner could report a locally found and durably saved block while the public canonical height remained unchanged for hours. This established that template validity alone was not the remaining problem. The delivery lifecycle had no end-to-end success criterion.

Before HF125:

```text
local connect/save succeeds
→ one-shot socket writes are attempted
→ GUI reports relay-like success
→ no peer acceptance is proven
→ seed may be saturated, behind or unaware
→ local tip can remain private indefinitely
```

## Reliability design

### Explicit acceptance protocol

`SubmitBlock` carries a random request ID and the full block. `BlockAck` returns the same request ID, block hash, receiver height/tip, status and rejection reason. The sender verifies both request ID and block hash before counting the result.

Accepted delivery statuses are:

```text
accepted
already_known
```

`stale_parent` and `rejected` do not count as delivery. A legacy write never counts as acknowledgement.

### Public-network delivery criterion

The relay report records total and official attempts separately. When official seeds are attempted, at least one official `accepted`/`already_known` acknowledgement is required to clear the pending record. This avoids treating acceptance by an arbitrary isolated peer as proof that the public seed layer received the block. Isolated regtest deployments with no official peers accept any explicit acknowledgement.

### Durable retry state

The pending file is network scoped and contains:

```text
schema version
network
block hash and height
full block
creation/last-attempt timestamps
attempt count
last acknowledgement count
last error summary
```

Malformed files are quarantined. A pending record is removed when:

```text
an eligible peer acknowledges delivery;
the local canonical chain has advanced beyond that block;
or the local block at that height no longer matches the pending hash.
```

### Reserve inbound lane

The accept loop allows a small reserve above `max_inbound_peers`. Connections in that reserve must identify their first Version role as `block-submit`; otherwise they are rejected and closed. The reserve connection has a bounded lifetime.

This preserves the normal peer cap while ensuring saturated seeds can still receive a newly found block.

### Behind-peer repair

A receiver that reports `stale_parent` also returns its current height and tip hash. The sender only performs repair if that exact receiver tip is present as an ancestor in the sender's validated chain. It sends bounded headers/chain suffixes and retries the same block once. No published snapshot is trusted for this decision.

### Atomic storage and live publication

`connect_block_persist_atomic` uses a copy-on-write `ChainState` clone, validates/connects the block, commits QUB-FCE-1, and only then replaces caller memory.

`publish_live_chain` no longer discards a successful save when the embedded P2P mutex is busy. A per-data-directory deferred candidate and one worker publish the strongest candidate after the mutex becomes available. Same-tip mempool state is merged rather than blindly replaced.

### Local multi-process protection

On mainnet, Fast Chain Engine rejects a candidate persistence request when committed work is greater, or when work/height are equal but the tip differs. This prevents two local writers from racing equal-work sibling blocks into the same storage directory.

### Snapshot and fork-choice boundary

HTTP snapshots and seed status remain transport/liveness aids. Adoption is decided by validated cumulative work. A lower published ancestor cannot force the node to discard a valid higher-work suffix. Equal-height competing tips wait for normal cumulative-work resolution.

### Coordination liveness

The coordinator no longer performs long repair work synchronously. Pending relay and automatic catch-up are each guarded by one atomic single-flight worker. Persistent sessions send Version/Inv based on elapsed wall time, so a continuous Mempool stream cannot suppress tip advertisement.

## Compatibility

- Wire protocol number remains `2`.
- New peers use `SubmitBlock`/`BlockAck`.
- Older peers that close/reject the new message receive the complete legacy relay sequence.
- Old peers cannot provide explicit acknowledgement; delivery remains pending until an updated eligible peer confirms the block.
- Consensus serialization, block hashes, transaction rules and Fast Chain Engine schema are unchanged.

## Tests added

### Rust tests

- explicit acknowledgement required for delivered relay report;
- official-attempt policy requires official acknowledgement;
- `SubmitBlock` / `BlockAck` serialization roundtrip;
- durable network-scoped pending relay file;
- atomic block connect persists before caller publication;
- mainnet equal-work/same-height sibling persistence rejection.

### Real-process regtest E2E

`scripts/test-hf125-block-relay-regtest.py` proves:

1. a receiver configured with `max_inbound_peers = 0` accepts block submission through the reserve lane;
2. height-1 delivery receives explicit acknowledgement and clears pending state;
3. a durable height-2 pending block repairs a genesis receiver on the same connection;
4. the exact tip persists across receiver restart.

Expected ending:

```text
HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS
```

## Operational invariants

After deployment:

```text
public seed RPC remains disabled;
seed listener 17444 remains active;
block version remains 2 after #24000;
pending local blocks expose explicit relay status;
no next mining round starts while the current local tip awaits delivery;
Explorer API remains a separate read-only service;
Windows manifest declares mandatory=true and chain_upgrade=false.
```

## Validation boundary

The source/package audit can verify syntax structure, release markers, configuration identity, patch reproducibility and artifact hygiene. The authoritative executable gate remains the actual Rust compile/test and regtest E2E run on the release machine and both seed hosts.
