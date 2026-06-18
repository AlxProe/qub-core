# QUB Core v1.7.5 HF117

HF117 is a mandatory reliability hotfix for QUB Core.

This is not a chain upgrade.

No activation heights changed. No DAA change. No seed update. No checkpoint change. No genesis/economics change.

## Highlights

- Fixes the stale/reorg QUB transaction recovery gap.
- Reorg adoption now resurrects valid non-coinbase transactions from disconnected local blocks back into mempool.
- Adds persistent raw transaction outbox: `wallet-pending-txs.json`.
- Wallet-created transactions are remembered until confirmations and can be reaccepted/rebroadcast if they temporarily become NotFound after stale-chain recovery.
- Normal QUB sends now get stronger exact bounded rebroadcast behavior.
- Mempool input-conflict checks run earlier before heavier contextual validators.
- GUI mining target-spacing pacing is enabled by default.
- Mining pacing uses lightweight live-tip checks and cancels/rebuilds if the network tip moves.
- Designed to reduce sequential winner head-start and coinbase-only streak behavior from public GUI miners.
- Explorer source bundle is updated, but runtime assets remain external in no-assets builds.

## Unchanged

- Verified Governance v1 remains scheduled for block #21000.
- QUB/JIN infusion remains at #16777.
- JIN Public Sale rules remain unchanged.
- JIN Coin -> JIN Token conversion remains disabled until bridge release.
- DAA v2 remains unchanged.
- Mainnet checkpoint remains unchanged.
- Seeds are not updated by this release.

## Security

Never share:
- `wallet.json`
- `ethereum-wallets.json`
- private keys
- seed phrases
- SSH keys
- local chain data

This repository is source-only and does not include runtime wallets, private keys, installers, or user data.
