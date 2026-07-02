# QUB Core v1.7.7 HF120

HF120 is a mandatory forward-only QUB mainnet protocol upgrade.

Activation height: **#24000**.

This release introduces **Protocol Epoch 2** with a block-version gate. From block #24000 onward, official upgraded QUB mainnet nodes require block version 2. Old/custom miners that keep mining block version 1 after activation will be on a fork rejected by upgraded official nodes.

## What changed

- Adds Protocol Epoch 2 activation at mainnet block #24000.
- Requires block version 2 at and after #24000.
- Builds solo and pool block templates with the correct expected block version for the next height.
- Adds CLI `info` and `preflight` visibility for Protocol Epoch 2.
- Keeps HF117 mempool/reorg tx recovery and wallet-pending-txs.json.
- Keeps HF118 QUB Melt / JIN Infuse GUI enablement.
- Keeps HF119 non-blocking QUB/JIN preview/signing and public-GUI winner brake.

## What did not change

- No rollback.
- No address blacklist.
- No reward confiscation.
- No DAA change.
- No checkpoint change.
- No genesis/economics change.
- No QUB/JIN activation-height change.
- Verified Governance remains scheduled/active according to its existing #21000 rule.
- JIN Coin -> JIN Token conversion remains disabled until bridge release.

## Operator note

Seeds/direct nodes/explorer infrastructure should be upgraded before #24000 so the official network is already serving and enforcing Protocol Epoch 2 at activation.
