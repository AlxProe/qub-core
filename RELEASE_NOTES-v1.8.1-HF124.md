# QUB Core v1.8.1 HF124

HF124 is the mandatory mining-liveness and pool-share pressure release.

## Consensus status

HF124 introduces no new chain activation.

- Protocol Epoch 2 remains active at mainnet block #24000.
- Blocks from #24000 onward continue to require block version 2.
- The 128 pool-share-transactions-per-block limit is unchanged.
- DAA, checkpoints, genesis, economics and QUB/JIN rules are unchanged.
- The USDJ bridge is not part of this release.

## Incident addressed

During a sustained share burst, the mainnet mempool exceeded the existing per-block pool-share limit. The previous official candidate builder could include every selected share even though consensus accepts at most 128 pool-share transactions in one block.

The stall could begin before the threshold because several additional hot paths amplified each new share:

- GUI CPU/GPU mining restarted on every mempool fingerprint change;
- GUI pool mining could submit another share after a round restart;
- single-share admission reconstructed historical pool state;
- pool shares passed through unrelated protocol validators;
- P2P committed mempool-only state while holding the canonical state mutex;
- share-first relay batches could delay ordinary transactions.

## Fixes

- Candidate templates include no more than 128 pool-share transactions and drain the oldest still-confirmable shares first.
- Excess shares stay in the mempool and do not prevent ordinary transactions from being selected.
- Mempool-only changes no longer cancel active CPU/GPU proof-of-work rounds; the validated non-coinbase transaction set is assembled once and reused by all workers for that parent.
- The validated candidate transaction set is assembled once per round and shared by CPU/GPU workers.
- Legacy GUI target-spacing jitter and last-winner cooldown sleeps are removed; consensus/DAA and canonical-parent guards remain authoritative.
- GUI pool mining emits at most one local share per canonical parent.
- Pool-share mempool retention is bounded to the confirmable stale horizon; stale legacy prefixes cannot crowd valid newer shares out during startup/reorg rebuild.
- Confirmed pool registry state is cached by exact canonical tip, advanced incrementally and reused by admission, templates, payouts and GUI share creation.
- Share-window scans are bounded to the relevant consensus windows.
- Inbound mempool batches reuse one pool-validation context.
- Pool shares use a dedicated validation fast path.
- Mempool-only persistence snapshots copy-on-write state under the canonical mutex at most once per five-second window, commits it after releasing the mutex, and re-dirties the state if the live owner moved during the write; stale same-tip subset snapshots are ignored without revalidation, while genuinely divergent same-tip mempool unions are merged and revalidated so concurrent transactions are not lost.
- Relay batches reserve space for ordinary QUB/JIN/Library traffic.
- Locally created shares use a bounded official-first fanout; inbound one-by-one shares are deferred to the fair heartbeat; periodic/inbound batch propagation sends one bounded `Mempool` message per peer instead of one connection per transaction.
- Every relay fanout captures one immutable Fast Chain Engine snapshot instead of reloading canonical storage once per destination peer.
- Pool shares are excluded from the durable wallet pending outbox; legacy share records are removed during reconciliation.
- Blocks declaring more than the allowed share count are rejected before expensive pool-state work.

## Retained infrastructure

HF123 Fast Chain Engine, authenticated RPC, `qub-rpc-miner`, snapshot publication and the optimized Explorer API remain included.

## Required validation

```text
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
cargo test hf124_ -- --nocapture
HF123 Fast Chain Engine E2E
HF123 RPC E2E
real-mainnet status-fast / storage-stats / validate / preflight
NYC3 canary
AMS3 deployment
Windows installer and public hash verification
clean-install mining and sync smoke
```
