# QUB Core v1.7.8 HF121 — reviewed source revision r3

HF121 / v1.7.8 is a mandatory operational reliability package on top of HF120. It **does not add a new consensus activation**. HF120 Protocol Epoch 2 activated at mainnet block **#24000** and remains fixed there.

This `r3` label identifies the reviewed source archive only. The public application identity remains **QUB Core v1.7.8 / HF121 / v178**.

## HF120 Protocol Epoch 2 remains unchanged

- Protocol Epoch 2 activation height: **#24000**
- Blocks below #24000: **version 1**
- Blocks at and above #24000: **version 2**
- No rollback
- No address blacklist
- No reward confiscation
- No DAA change
- No checkpoint, genesis, or economics change

## HF121 operational work

- Adds `chain-status.json`, a tiny operational metadata cache refreshed after successful chain persistence.
- Adds `status-fast`, which reads the metadata cache in normal operation and uses a bounded-memory streaming scan only as a recovery fallback. It does not replay consensus or load wallet state.
- Adds a bounded-memory, exact-schema snapshot publisher that verifies the full block hash-link chain and the HF120 #24000 block-version boundary before publishing.
- Builds every snapshot generation in staging and publishes `tip.json` last as the commit marker.
- Adds `/api/v1/status-fast` to the explorer API without exposing local filesystem paths.
- Adds local-only, token-authenticated `rpc-api` groundwork for future headless node, pool, and miner infrastructure.
- Keeps remote mining templates and block submission disabled in HF121.
- Hardens state-file replacement so Linux uses atomic rename-over-target and Windows preserves/restores the previous file if replacement fails.
- Keeps HF117 stale/reorg transaction recovery, HF118 QUB/JIN Melt/Infuse GUI support, HF119 non-blocking QUB/JIN flow, and the complete HF120 epoch gate.

## Security posture of the HF121 RPC groundwork

- `rpc.enabled=true` is required.
- A non-placeholder `rpc.auth_token` is required.
- HF121 permits loopback binds only (`127.0.0.1`, `localhost`, or `::1`).
- Every request requires `X-QUB-RPC-Token`.
- Browser CORS is not enabled for RPC.
- Request headers and socket wait times are bounded.
- Mining template and submit-block endpoints return HTTP 501 and cannot alter chain state.

## Source/package notes

This is a no-assets source package. Runtime UI/media assets are intentionally not included. Copy the known-good `assets` directory from the currently deployed QUB Core source tree before building the Windows release.

Run the local and seed build/test gates before public deployment. The snapshot publisher also includes a standalone self-test:

```bash
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh
```
