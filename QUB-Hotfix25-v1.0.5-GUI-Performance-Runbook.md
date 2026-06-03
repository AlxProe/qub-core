# QUB Core Hotfix25 / v1.0.5 — GUI Performance + Clean UI

This release is not a consensus upgrade and does not change activation heights.

## What changed

- Removes Unicode text glyph icons from the GUI to prevent mojibake / weird symbol rendering.
- Moves periodic chain snapshot refresh off the UI thread.
- Lowers idle repaint pressure while keeping active mining/update screens responsive.
- Caches Windows MCI MP3 aliases so sounds are not reopened from disk on every tick.
- Hides Windows child process consoles for PowerShell/cmd/reg invocations used by updater/autostart.
- Keeps the testnet unsigned-updater bypass gated to testnet only.

## Testnet canary build

```powershell
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -BuildInstaller -SkipTests
```

Upload:

```powershell
scp -i $env:USERPROFILE\.ssh\jinex_ed25519 .\dist\updates\QUB-Core-Latest.exe deploy@159.223.222.103:/tmp/QUB-Core-Latest-testnet.exe
```

On the backend:

```bash
sudo mv /tmp/QUB-Core-Latest-testnet.exe /srv/qub-updates/testnet/QUB-Core-Latest.exe
sudo chmod 644 /srv/qub-updates/testnet/QUB-Core-Latest.exe
curl -I https://download.qubit-coin.io/testnet/QUB-Core-Latest.exe
```

## Mainnet build

Only after testnet canary passes:

```powershell
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -BuildInstaller -SkipTests
```

For public mainnet auto-install, use signed builds.
