# QUB Core v1.7.6 HF118

HF118 is a mandatory GUI enablement and polish hotfix for QUB Core.

This is not a chain upgrade.

No activation heights changed. No DAA change. No seed update. No checkpoint change. No genesis/economics change.

## Highlights

- Officially enables QUB Melt for JIN in the QUB balance card after HF116 activation.
- Officially enables Infuse JIN into QUB in the native JIN Coin balance card after HF116 activation.
- Adds a dedicated QUB/JIN action window with Infuse and Melt modes.
- Shows live preview/safety checks for active JIN inside QUB, true max QUB supply, current per-QUB backing, minimum exact infusion step, expected melt payout, and minimum JIN guard.
- GUI QUB/JIN transactions use the HF117 wallet-pending-txs.json outbox, exact bounded rebroadcast, status tracking, and stale/reorg recovery path.
- Polishes QUB Explorer buttons: compact square icon, light corner rounding, much lower padding, centered image.

## Unchanged

- QUB/JIN infusion activation remains #16777.
- Verified Governance v1 remains scheduled for #21000.
- JIN Coin -> JIN Token conversion remains disabled until the bridge is live.
- DAA v2 remains unchanged.
- Mainnet checkpoint remains unchanged.
- Seeds are not updated by this release.
- HF117 mempool/reorg recovery remains included.

## Security

Never share:
- `wallet.json`
- `ethereum-wallets.json`
- private keys
- seed phrases
- SSH keys
- local chain data
