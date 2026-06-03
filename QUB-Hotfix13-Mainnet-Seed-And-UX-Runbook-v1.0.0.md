# QUB Core v1.0.0 hotfix13 - mainnet seed + UX runbook

## What changed

- Public DNS seed collapsed to one hostname: `seed.qubit-coin.io`.
- Mainnet seed endpoint: `seed.qubit-coin.io:17444`.
- Testnet seed endpoint: `seed.qubit-coin.io:18444`.
- Normal GUI users still never type IPs or bootnodes.
- Regtest-LAN remains automatic UDP discovery.
- Bottom-left mining status image/animation is asset-driven.
- All sound toggles default to ON for fresh GUI settings.

## Required GUI assets

Place these in `assets/` before building the release bundle:

```text
qubit-coin-logo.png
jin-coin-logo.png
mined.mp3
network-mined.mp3
mining-on.mp3
mining-off.png
mining-off-white.png
mining-prep.gif
mining-prep-white.gif
mining-on.gif
mining-on-white.gif
```

Optional font:

```text
assets/fonts/Ubuntu-BoldItalic.ttf
```

## Clean GUI prefs to see new sound defaults

Existing PCs may already have old settings saved. To force hotfix13 defaults:

```powershell
Remove-Item -Force .\data\qub-core-gui-settings.json -ErrorAction SilentlyContinue
```

## Build regtest-lan bundle

```powershell
cargo clean
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan
```

## Build testnet bundle

Only after `seed.qubit-coin.io` resolves and the testnet seed service is running:

```powershell
cargo clean
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -Sign -CertSubject "Alexander Proestakis" -BuildInstaller
```

## Build mainnet bundle

Only after `seed.qubit-coin.io` resolves and the mainnet seed service is running:

```powershell
cargo clean
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -Sign -CertSubject "Alexander Proestakis" -BuildInstaller
```

## DigitalOcean seed services

Use `deploy/digitalocean/README-seed-droplet.md`.

The same droplet can run both services because the networks use different ports:

```text
mainnet: 17444/tcp
testnet: 18444/tcp
```

Cloudflare DNS should be:

```text
A seed -> 159.223.222.103, DNS only / gray cloud
```

## Mainnet-as-testnet caution

You can rehearse privately on mainnet before announcement if nobody else has the build or seed domain. Still, if the chain is reset, every old `data/mainnet` folder must be deleted on every test machine before the real launch. Never distribute a reset mainnet privately and then treat it as final unless the genesis/config is frozen.
