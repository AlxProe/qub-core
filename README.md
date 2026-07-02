# QUB Core v1.7.7 HF120 source pack

HF120 / v1.7.7 is a forward-only mainnet protocol upgrade package on top of HF119. It introduces Protocol Epoch 2 at mainnet block #24000 by requiring block version 2 from that height onward.

## HF120 / Protocol Epoch 2

HF120 does **not** roll back history, blacklist addresses, confiscate rewards, change DAA, change economics, change genesis, or change existing checkpoint history.

It does add a real consensus activation gate:

- Blocks before #24000 keep the existing block version.
- Blocks at #24000 and later must use block version 2.
- Old/custom miners that continue producing block version 1 after #24000 will be mining a fork that upgraded official QUB mainnet nodes reject.

## Included previous fixes

- HF117 stale/reorg transaction recovery and wallet-pending-txs.json raw transaction outbox.
- HF118 official GUI enablement for QUB Melt and JIN Infuse.
- HF119 non-blocking QUB/JIN preview/signing and public-GUI winner brake.

## Activation

- Mainnet Protocol Epoch 2 activation: #24000.
- Current target release height discussed during packaging: ~#21450.
- This gives roughly 2,550 blocks of public upgrade notice.

## Assets

This is a no-assets source package. Runtime UI assets must be copied from a previous local installation/package before building a public Windows bundle.
