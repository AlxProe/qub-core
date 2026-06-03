# QUB Hotfix28 / v1.0.8 — Realtime Miner + GUI Consistency

Scope:
- No chain upgrade.
- No activation height change.
- No consensus, QNS, economics, block reward, or address-format change.

Main fixes:
- Restores the v1.0.6 mining asset system: cached GIF animations + cached `mining-on.mp3` loop.
- Removes the v1.0.7 MP4 fallback behavior from the native GUI path.
- Makes the miner status line update while hashing so it does not remain stuck on an old `Block relayed...` message.
- Keeps `Respect 60s target spacing` removed/disabled.
- Tightens miner sync checks so v1.0.8 does not create a performance disadvantage versus older builds.
- Shows `v1.0.8 (Testnet)` in the header when using testnet.
- Smooths profile switching between testnet/mainnet by resetting payout address if the old address belongs to another network.
- Swaps direct/global status colors: direct is blue, global/online is green.
- Shows optional `online.png` / `offline.png` icons in peer list and local miner row when present.
- Keeps QNS wording simplified: `N names`, no activation wording in user-facing places.

Required optional assets for full visual experience:
- assets/mining-off.png
- assets/mining-off-white.png
- assets/mining-prep.gif
- assets/mining-prep-white.gif
- assets/mining-on.gif
- assets/mining-on-white.gif
- assets/mining-on.mp3
- assets/online.png
- assets/offline.png

Testnet deployment first:
1. Build with `cargo test`.
2. Build testnet installer.
3. Upload only `/testnet/QUB-Core-Latest.exe`.
4. Confirm v1.0.7 -> v1.0.8 updates through the private updater.
5. Verify the mining icon moves while mining and the mining loop sound plays if enabled.
6. Verify the status line changes from relayed/accepted messages back to current mining status.
7. Verify switching testnet/mainnet replaces incompatible payout addresses.

Do not deploy mainnet until testnet confirms no performance regression.
