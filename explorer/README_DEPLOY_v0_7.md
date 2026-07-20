# QUB Explorer v0.7 deployment

QUB Explorer v0.7 remains a static, read-only application. It adds a Mining page with objective on-chain observability for payout labels, pools, concentration, streaks, exact two-label alternation, block intervals, coinbase-only rates, and block-version distribution.

## Deployment

Upload the contents of this directory to the existing Explorer static root or Cloudflare Pages project. Preserve the same-origin `/mainnet/snapshots/*` routes already used by v0.6.

Runtime image assets are intentionally excluded from the no-assets package. Copy the existing Explorer assets into `assets/` before deployment.

## Safety

- The Explorer never treats a payout address as proof of a unique operator.
- Mining statistics are descriptive, not a consensus rule or accusation.
- Protocol Epoch 2 remains fixed at block #24000; v0.7 only displays block versions.
