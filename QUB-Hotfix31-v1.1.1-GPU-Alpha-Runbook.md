# QUB Hotfix31 / v1.1.1 — Testnet GPU Mining Alpha Guard + Network Dropdown

Scope:
- No chain upgrade
- No activation height
- No consensus/PoW/QNS rule change
- Testnet-first only

Changes:
- Version bump to v1.1.1.
- Settings network selector changed from four buttons to a dropdown list to reduce accidental profile switching.
- Testnet-only GPU mining alpha controls added.
- Mainnet GPU mining remains disabled.
- The GPU power slider is available only in the testnet build.
- CPU mining remains the safe baseline path.

Important:
This package does not change consensus and does not require seed activation. The OpenCL GPU miner must pass testnet validation before mainnet enablement. Do not treat this as a public GPU mining release.

Build testnet:
```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -BuildInstaller -SkipTests
```

Deploy testnet via manifest channel as v1.1.0 did:
```powershell
scp -i $env:USERPROFILE\.ssh\jinex_ed25519 -r .\dist\updates\testnet\windows-x64\* deploy@159.223.222.103:/tmp/qub-testnet-windows-x64/
```

Backend:
```bash
sudo rsync -av --delete /tmp/qub-testnet-windows-x64/ /srv/qub-updates/testnet/windows-x64/
sudo chmod -R 755 /srv/qub-updates/testnet
sudo find /srv/qub-updates/testnet -type f -exec chmod 644 {} \;
curl -I https://download.qubit-coin.io/testnet/windows-x64/manifest.json
```

Testing checklist:
- Testnet app remains separate from mainnet app.
- Network profile uses dropdown.
- GPU slider is enabled only in testnet build.
- Mainnet build keeps GPU disabled.
- CPU mining still works normally.
- No chain fork / no activation needed.
