# QUB Explorer Static v0.6

Explorer-only static update.

Live-root note:
- The production root discovered during v0.3/v0.4 deployments is `/srv/qub-updates/explorer`.
- Do not deploy this package into `/srv/qub-explorer` unless Caddy has been changed to serve that root.

Deploy rule:
- Copy `index.html` to the live root.
- Copy only the included canonical icon files into the existing live `assets/` folder:
  - `enj.png`
  - `enj-white.png`
  - `jin-token.png`
  - `jin-token-white.png`
- Do not `rsync --delete` the full `assets/` folder from this package.

v0.6 changes:
- Explorer label/cache namespace remains `Explorer v0.6` / `qub-explorer-cache-v06`.
- Asset URLs use `v06-qubjin` cache busting.
- Includes canonical ENJ and JIN Token PNG/white PNG icons.
- Restores Enjin Matrixchain JIN Token backing fetch.
- Reads same-origin published Enjin metrics first, then falls back to browser JSON-RPC.
- Parses QUBJIN1 marker data for:
  - JIN Coin infuse into QUB.
  - QUB melt for native JIN Coin.
- Calculates QUB/JIN infusion state from the indexed chain:
  - sale-reserve lock height.
  - infusion activation height.
  - 42,000,000 JIN bootstrap.
  - active JIN inside QUB.
  - cumulative JIN infused into QUB.
  - standard JIN per QUB.
  - melted QUB supply.
  - true max QUB supply.
  - indexed JIN infuse and QUB melt events.
- Keeps JIN Coin melt disabled in the UI.
- Keeps Stablecoins disabled/preview in Assets.

This package does not change QUB Core, seeds, snapshots, or consensus.


FINAL2 notes:
- Restores the full Enjin Matrixchain JIN Token storage key used by the working v0.5 twins build.
- Local file:// opens skip same-origin metrics probes and go directly to Matrixchain RPC.
- Live deployments should still publish same-origin Enjin metrics JSON for maximum reliability.
