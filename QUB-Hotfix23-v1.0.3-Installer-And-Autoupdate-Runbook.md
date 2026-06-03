# QUB Hotfix23 / v1.0.3 — Installer + auto-update distribution

## What changed
- Windows public release is now a signed installer-first distribution.
- Default first-run wizard profile is Mainnet.
- Built-in Windows updater checks a single installer URL and can auto-download / auto-install.
- Updater stops the miner before installing a newer version.
- App and shortcuts use the embedded Qubit Coin logo icon.

## Public release flow
1. Build installer:
   `powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -Sign -CertSubject "Alexander Proestakis" -BuildInstaller`
2. Upload single file:
   `dist\updates\QUB-Core-Latest.exe`
3. Host it at:
   `https://download.qubit-coin.io/QUB-Core-Latest.exe`
4. Existing QUB Core users with auto-update enabled will download the new installer.

## Miner/update host on QUB-miner-cpu-ams3
- Create `/srv/qub-updates`
- Upload `QUB-Core-Latest.exe` there
- Add `deploy/digitalocean/Caddyfile.qub-updates` to Caddy and reload

## Silent in-place upgrade
The updater runs the installer with `/DIR=<current app dir>` so portable-style installs preserve:
- `data/mainnet/wallet.json`
- `data/testnet/wallet.json`
- `data/qub-core-gui-settings.json`
- generated `config/qub-core-*.toml`

## Safety
- Updater only accepts Authenticode `Valid` installers.
- Signer must contain `Alexander Proestakis`.
- Miner stops before install to avoid running stale consensus code.
