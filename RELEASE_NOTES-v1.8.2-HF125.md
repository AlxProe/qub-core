# QUB Core v1.8.2 HF125

HF125 is the mandatory reliable-block-delivery and canonical-liveness release.

## Consensus status

HF125 introduces no new chain activation.

- Protocol Epoch 2 remains active at mainnet block #24000.
- Blocks from #24000 onward continue to require block version 2.
- DAA, block rewards, checkpoints, genesis, economics and QUB/JIN rules are unchanged.
- The 128 pool-share-transactions-per-block consensus limit is unchanged.
- The USDJ bridge is not part of this release.

## Incident addressed

After HF124 made block templates valid under pool-share pressure, miners could still find and persist a valid local block without the block becoming publicly canonical. The prior relay path sent blocks without requiring an acceptance response. Saturated seeds could decline new sockets, continuous mempool traffic could suppress tip heartbeats, and a successful Fast Chain Engine commit could briefly fail to update the embedded P2P live owner. Published snapshot state could also lag behind a valid local suffix.

## Fixes

- Adds explicit `SubmitBlock` / `BlockAck` delivery with request IDs and acceptance status.
- Requires an official seed acknowledgement on public networks before clearing the durable delivery record.
- Stores unacknowledged local blocks in `pending-block-relay.json` and retries automatically across process restarts.
- Adds `block-relay-status` and `relay-pending-block` operator commands.
- Reserves short-lived inbound capacity for role-declared block-submit connections when the normal inbound peer cap is full.
- Repairs a receiver that is behind on a validated ancestor by serving the missing suffix on the same connection and retrying the exact block.
- Preserves protocol-v2 compatibility and falls back to legacy `Block + Inv + Headers + Chain` relay for older peers.
- Makes local block connect/persistence transactional: durable commit succeeds before caller memory advances.
- Queues deferred publication to the embedded canonical owner when its mutex is temporarily busy.
- Prevents a lower published snapshot from replacing a validated higher-work local suffix.
- Rejects equal-work, equal-height, different-tip mainnet storage overwrites from competing local processes.
- Moves heavy automatic catch-up and pending-block retries into bounded single-flight workers so relay coordination remains responsive.
- Sends Version/Inv heartbeats on wall-clock cadence even during continuous mempool traffic.
- Updates GUI, CLI and RPC mining output to distinguish durable local acceptance, explicit peer acknowledgement and pending automatic retry.

## Retained fixes

HF124 mining-liveness controls, HF123 Fast Chain Engine, authenticated RPC, `qub-rpc-miner`, snapshot publication and the optimized Explorer API remain included.

## Required validation

```text
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
cargo test hf125_ -- --nocapture
HF123 Fast Chain Engine E2E
HF123 RPC E2E
HF125 reliable block delivery E2E
real-mainnet status-fast / storage-stats / validate / preflight
NYC3 canary
AMS3 deployment
separate Explorer API binary update
Windows installer and public hash verification
clean-install block-delivery smoke
```
