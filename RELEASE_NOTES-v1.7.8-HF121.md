# QUB Core v1.7.8 HF121

HF121 is a mandatory operational reliability update for QUB Core. This archive is reviewed source revision `r2`; the public release identity remains **v1.7.8 / HF121**.

HF121 does **not** introduce another chain activation and does **not** move or alter HF120 Protocol Epoch 2.

## Consensus status

- HF120 Protocol Epoch 2 remains scheduled at mainnet block #24000.
- Blocks below #24000 require block version 1.
- Blocks from #24000 onward require block version 2.
- No rollback.
- No address blacklist.
- No reward confiscation.
- No DAA change.
- No checkpoint, genesis, or economics change.

## Operational reliability

- Added `chain-status.json`, written after successful chain persistence.
- Added a genuinely lightweight `status-fast` path: metadata in normal operation, bounded-memory stream scan as recovery fallback.
- Added a bounded-memory snapshot publisher that does not run full validation in the timer path.
- Snapshot publication verifies mainnet identity, every block-to-block hash link, and the HF120 version-1/version-2 boundary at #24000.
- Snapshot files preserve the exact schemas already consumed by QUB Core and Explorer.
- Snapshot generations are staged and `tip.json` is published last.
- Added explorer `/api/v1/status-fast` without leaking local server paths.
- Added local-only, token-authenticated RPC groundwork for future official daemon, pool, and miner infrastructure.
- Mining work-template and submit-block RPC methods remain intentionally disabled and return HTTP 501.
- Hardened persistent state replacement to avoid remove-before-rename and partial direct-write fallback behavior.

## Security review corrections included in r2

The first HF121 candidate was not released. The reviewed r2 source corrects the following before deployment:

- The original `status-fast` candidate parsed the whole chain into a generic JSON tree; r2 no longer does that.
- The original RPC candidate checked whether a token looked configured but did not authenticate requests; r2 performs token authentication and remains loopback-only.
- The original RPC candidate used unbounded connection threads and broad CORS; r2 uses bounded synchronous handling with timeouts and no RPC CORS.
- The original status API exposed local filesystem paths; r2 redacts them from explorer/RPC responses.
- The original publisher held the whole chain in Python memory and had a weaker multi-file commit sequence; r2 is bounded-memory and publishes the generation marker last.

## Still included

- HF117 stale/reorg transaction recovery.
- `wallet-pending-txs.json` raw transaction outbox.
- HF118 QUB Melt / JIN Infuse GUI enablement.
- HF119 non-blocking QUB/JIN preview/signing.
- HF119 public-GUI mining brake.
- HF120 Protocol Epoch 2 at #24000.

## Operator note

Use:

```bash
qubd --config <mainnet-seed.toml> status-fast
```

for hot-path health checks. Do not put full `validate`, heavy `info`, or wallet-loading commands inside seed deployment hooks or the snapshot timer.
