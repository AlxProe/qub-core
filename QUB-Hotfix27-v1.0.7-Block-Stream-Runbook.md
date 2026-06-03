# QUB Hotfix27 / v1.0.7 — Block stream consistency + GUI cleanup

Scope:
- No chain upgrade.
- No activation-height change.
- No consensus/economics/QNS rule change.
- Removes public GUI target-spacing wait.
- Cleans QNS wording.
- Improves recent block age display for future timestamps.
- Keeps recent blocks based on local canonical chain state.
- Keeps private unsigned updater bridge enabled for the closed QUB Miners group.

Important:
- `pace_to_target_spacing` remains in the settings struct only for backward-compatible reading of old JSON settings, but the GUI no longer shows it and the miner ignores it.
- If a miner has wrong system time, the GUI will display `clock +Xs` instead of misleading `0s`.
- MP4 mining visual assets are packaged/staged by name, but this native egui build avoids decoding media on the UI thread. It uses cached static fallback until a proper embedded video decoder is added.

Build:
```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core

$ISCC = @(
  "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
  "C:\Program Files\Inno Setup 6\ISCC.exe",
  "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -BuildInstaller -SkipTests
```

Testnet first:
- Upload `dist\updates\QUB-Core-Latest.exe` to `/srv/qub-updates/testnet/QUB-Core-Latest.exe`.
- Confirm `curl -I https://download.qubit-coin.io/testnet/QUB-Core-Latest.exe` returns `200`.
- Update a testnet v1.0.6 client to v1.0.7.
- Verify:
  - no weird Unicode labels;
  - no target-spacing checkbox;
  - QNS displays `N names` only;
  - block ages do not stick at `0s` for future timestamps;
  - recent global blocks and pending mined card refresh faster.

Mainnet rollout:
- After testnet passes, build `-Config mainnet`.
- Upload versioned installer and latest installer to `/srv/qub-updates/mainnet/`.
- Users on broken v1.0.5 must manual install v1.0.6 or later once; v1.0.7 can then be delivered through private unsigned updater.
