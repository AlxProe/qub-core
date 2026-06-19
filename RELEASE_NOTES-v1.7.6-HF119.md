# QUB Core v1.7.6 HF119

HF119 supersedes the local HF118 candidate before public deployment.

This is not a chain upgrade.

No activation heights changed. No DAA change. No seed update. No checkpoint change. No genesis/economics change.

## Highlights

- Fixes the QUB/JIN Infuse/Melt GUI freeze risk by moving QUB/JIN preview calculation off the UI thread.
- Opening Infuse/Melt windows is now immediate; exact preview/state calculation runs in a background worker.
- Typing amounts/fees in the Infuse/Melt windows no longer performs synchronous chain scans on every frame.
- QUB/JIN action signing remains on a worker thread and now uses a shorter bounded pre-action catch-up path.
- QUB/JIN Infuse/Melt transactions continue to use the HF117 `wallet-pending-txs.json` recovery path and exact bounded rebroadcast.
- Adds HF119 public-GUI winner brake for solo mining: if the same local payout address already mined the latest solo block(s), the official GUI waits a one-tip wall-clock cooldown before hashing the next height.
- The winner brake is local mining policy only; it does not change block validity, DAA, activation heights, seeds, or consensus.
- Keeps HF118 Explorer button polish: compact square-rounded icon buttons with centered images.

## Important mining note

HF119 can only throttle updated public QUB Core GUI miners. It cannot invalidate blocks from older/custom miners without a chain upgrade. The goal is to remove the remaining official-GUI path that allowed last-winner head-start after a long hash round.

## Unchanged

- QUB/JIN activation remains #16777.
- Verified Governance v1 remains scheduled for block #21000.
- JIN Public Sale rules remain unchanged.
- JIN Coin -> JIN Token conversion remains disabled until bridge release.
- DAA v2 remains unchanged.
- Mainnet checkpoint remains unchanged.
- Seeds are not updated by this release.

## Security

Never share:

- `wallet.json`
- `ethereum-wallets.json`
- `wallet-pending-txs.json`
- private keys
- seed phrases
- SSH keys
- local chain data

This repository is source-only and does not include runtime wallets, private keys, installers, or user data.
