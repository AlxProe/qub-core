# HF124 / v181 Reliability Review

## Release identity

```text
QUB Core: v1.8.1
Hotfix: HF124
Package generation: v181
Storage engine: QUB-FCE-1
Consensus activation introduced by HF124: none
```

## Confirmed mainnet condition

At canonical height #25806, the observed mempool contained 196 pool-share transactions while the existing consensus limit remained 128 shares per block. Most shares referenced the current parent and were otherwise recent.

That observation confirmed one deterministic failure mode: an uncapped official template could contain a block body that `validate_pools_block()` must reject.

The earlier onset of intermittent stalls required a broader review. HF124 therefore addresses the full share-pressure lifecycle rather than only adding a numeric cap.

## Failure chain before HF124

### 1. Invalid local template construction

Pool shares received highest template priority, but candidate construction did not stop at `max_share_txs_per_block`. A miner could spend real hash work on a locally generated block that consensus would reject.

### 2. Mempool-triggered proof-of-work cancellation

The GUI miner compared the full mempool fingerprint every few seconds. Any new transaction stopped all CPU/GPU workers and rebuilt the template, even though the canonical parent, version and target were unchanged.

### 3. Share feedback loop

The GUI pool miner created a share at the start of each outer round. A mempool-triggered restart could therefore create another share, trigger more relays and cause another restart.

### 4. Historical reconstruction per share

Single-share admission reconstructed pool registry and active-window state. The active-window state scanned far more history than required, and each share also entered unrelated QNS/JIN/Library/governance validators.

### 5. Persistence under canonical lock

Incoming transaction handlers persisted Fast Chain Engine mempool state before releasing the canonical chain mutex. A share burst could delay block acceptance behind repeated state commits.

### 6. Relay starvation

Share-first ordering in bounded mempool batches could repeatedly relay the same high-priority share subset while delaying ordinary transactions.

## HF124 design

### Candidate safety

Candidate construction validates shares sequentially and selects at most the existing configured consensus cap. It then continues considering ordinary transactions. The resulting candidate is valid under the same block validator used for network blocks.

### Stable work lifecycle

Mining rounds now stop only for state that invalidates the current header work:

- canonical parent height/hash changes;
- expected block version changes;
- difficulty/target changes;
- explicit user stop or safety guard failure.

A mempool-only change does not invalidate the current header and therefore does not cancel hashing.

### Shared candidate construction

The contextually validated transaction set, fees, parent, target and version are assembled once per mining round in `CandidateBlockParts` and shared across CPU/GPU workers. Worker-local work changes only coinbase extra nonce and header nonce. This removes repeated template reconstruction from every worker loop.

Legacy GUI target-spacing jitter and last-winner cooldown sleeps are removed. Consensus/DAA and canonical parent/version/target checks remain the only automatic pacing and invalidation rules.

### Share emission bound

The GUI pool miner records the canonical parent for which it submitted its local share and will not submit another local share for that parent. Existing pending shares are detected after reload.

### Mempool bound

The local pool-share retention limit is:

```text
max_share_txs_per_block × share_stale_blocks
```

On mainnet this is:

```text
128 × 6 = 768 shares
```

This is local policy, not a consensus change. It bounds work and storage to the maximum horizon in which shares can still become confirmable.

### Validation context

- Process-wide bounded registry and share-window caches are keyed by network, complete pool settings, canonical height, canonical tip hash and spend height where required; the registry cache is reused by admission, templates, payouts and GUI share creation.
- Exact hits clone the confirmed registry/window without historical scanning.
- Direct canonical extensions replay only newly connected blocks.
- Reorganizations reuse only a cached entry whose hash is proven to exist on the candidate branch.
- Unknown branches fall back to the deterministic full registry reconstruction.
- Active/duplicate share state scans only `share_window_blocks` / `share_stale_blocks`.
- Rejected shares do not mutate the reusable context.

### Batch and persistence behavior

- Inbound mempool batches share one validation context.
- Pool-share transactions take a dedicated fast path.
- At most once per five-second window, P2P takes a cheap copy-on-write snapshot while briefly holding the canonical mutex.
- The Fast Chain Engine commit runs after the mutex is released, so disk I/O cannot pause inbound block acceptance or mining.
- If the canonical tip or mempool identity changes while the snapshot is being written, the dirty flag is restored for a later convergence commit.
- A stale same-tip snapshot that is already a subset of the live owner is ignored without full mempool revalidation; genuinely divergent GUI/CLI state is merged and revalidated instead of replacing concurrent P2P submissions.
- Successful block/chain commits clear the dirty flag because they already include current mempool state.

### Relay fairness

A bounded relay batch reserves capacity for ordinary transactions whenever they exist, and share relays are capped to one block’s share capacity. Locally created shares use a bounded official-first fanout; inbound one-by-one shares are deferred to the six-second fair heartbeat. Periodic heartbeats and accepted inbound batches propagate one bounded `Mempool` message per peer rather than opening one fresh connection for every transaction. Each fanout captures one immutable Fast Chain Engine snapshot before opening destination sockets, so canonical storage is not reloaded once per peer.

### Ephemeral share durability

Pool shares are parent-bound work markers rather than wallet payments. HF124 does not store them in the durable wallet pending outbox, and reconciliation drops share records created by older releases. This prevents stale share resurrection and repeated long-lived rebroadcast.

## Consensus boundary

HF124 does not relax any block rule. A network block with more than 128 pool-share transactions continues to fail consensus validation. The early count check only moves that rejection before expensive pool-state reconstruction.

## Tests added

```text
candidate parts reused across multiple coinbase extra nonces
→ identical validated non-coinbase transaction tail
→ distinct coinbase and merkle root per worker job

400 valid shares (beyond the bounded ordinary scan budget) + ordinary QUB transaction
→ candidate contains exactly 128 shares
→ ordinary transaction remains included
→ mined block connects successfully

policy_limit + 1 shares
→ exactly policy_limit shares retained

129-share manually constructed block
→ proof of work recomputed
→ consensus rejects with too many pool share txs

200 old-parent shares, then 100 new-parent shares
→ next candidate selects all 72 old survivors before 56 new shares
→ selected parent heights are monotonic

same-tip concurrent persistence branches
→ mempool union is merged and revalidated
→ both transactions survive a fresh Fast Chain Engine reload

full stale legacy share prefix followed by valid newer shares
→ stale entries are rejected once
→ rebuild continues scanning
→ valid newer shares fill the bounded queue

one assembled CandidateBlockParts shared across workers
→ non-coinbase transaction selection is identical
→ only coinbase extra nonce / merkle root / header nonce vary

share-dominated relay batch + ordinary transaction
→ ordinary transaction receives reserved relay capacity
→ share count never exceeds one block's share capacity

legacy GUI pool share in wallet pending outbox
→ new shares are never persisted there
→ old share records are dropped instead of resurrected
```

Existing full regression suites remain mandatory.

## Operational expectations

After enough miners update:

- a canonical block can be mined even while the share mempool exceeds 128;
- the first block confirms at most 128 shares;
- the remaining share backlog drains over subsequent blocks;
- new mempool traffic does not repeatedly reset active GPU/CPU work;
- ordinary QUB/JIN/Library transactions continue to propagate;
- seed block acceptance is not blocked by per-share persistence.

## Remaining boundaries

- Consensus continues rejecting any block body above the existing 128-share limit; HF124 official builders enforce that limit before hashing.
- The pool-share target and the 128-share consensus cap are unchanged.
- Extremely large non-share protocol bursts can still require bounded protocol-state work.
- Full dependency-backed Rust compilation and real mainnet smoke testing must be performed on the release workstation and seed build hosts.
