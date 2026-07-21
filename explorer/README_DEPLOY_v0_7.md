# QUB Explorer v0.7 deployment

QUB Explorer v0.7 remains a static, read-only application. It adds a Mining page with objective on-chain observability for payout labels, on-chain pools, payout-label concentration, block intervals, coinbase-only rates and block-version distribution. It reports aggregate on-chain observations only and does not infer operator identity or coordination.

## Package contents

```text
index.html
_headers
assets/README.md
```

Runtime image assets are intentionally excluded. Copy the existing v0.6 `assets/` files before deployment.

## AMS3 static-root deployment

Back up the existing index and replace only `index.html`; this preserves the live assets directory:

```bash
STAMP="$(date +%Y%m%d-%H%M%S)"
sudo cp /srv/qub-explorer/index.html \
  "/srv/qub-explorer/index.html.backup-before-v0.7-$STAMP"
sudo install -m 0644 index.html /srv/qub-explorer/index.html
```

No seed restart is required for the static page.

## Cloudflare Pages alternative

Upload the extracted directory after restoring the existing assets. `_headers` supplies conservative static response headers.

## Safety and interpretation

- A payout address or pool label is not proof of a unique human, machine, cluster or operator.
- Protocol Epoch 2 remains fixed at block #24000; v0.7 only displays and analyzes block versions.
