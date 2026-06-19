# QUB Core v1.7.6 HF118 source pack

HF118 / v1.7.6 is a non-consensus GUI hotfix on top of HF117. It does not change DAA, block validity, activation heights, genesis, checkpoints, seeds, or protocol economics.

## HF118 / v1.7.6 mainnet GUI enablement

HF118 officially enables the already-active HF116 QUB/JIN mechanics in the public QUB Core GUI after mainnet activation #16777:

- QUB balance card: **Melt** is enabled for local wallets once QUB/JIN infusion is active.
- JIN Coin balance card: **Infuse** is enabled for local wallets once QUB/JIN infusion is active.
- New QUB/JIN action window with Infuse/Melt tabs.
- Live preview/safety checks for active JIN inside QUB, true max QUB supply, per-QUB backing, exact infusion step, melt payout, and minimum JIN guard.
- GUI-created QUB/JIN transactions use the same HF117 wallet-pending-txs.json outbox, exact bounded rebroadcast, status tracking, and stale/reorg recovery path as normal QUB/JIN sends.
- Explorer buttons are polished to use compact square icons with small corner rounding, low padding, and centered images.

## Unchanged

- No chain upgrade.
- No seed update.
- No DAA change.
- No activation-height change.
- QUB/JIN infusion activation remains #16777.
- Verified Governance v1 remains scheduled for #21000.
- JIN Coin -> JIN Token conversion remains disabled until the bridge is live.
- HF117 mempool/reorg recovery remains included.
