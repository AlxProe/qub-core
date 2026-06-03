# QUB Hotfix30 / v1.1.0 — Manifest updater + mainnet/testnet channel split

This release is infrastructure hardening. It does **not** change consensus, QNS rules, activation heights, PoW, rewards, or the chain format.

## Goals

- Separate Windows install identities for mainnet and testnet.
- Prevent testnet canary updates from replacing the mainnet installation.
- Replace `QUB-Core-Latest.exe` guessing with a manifest-based updater.
- Keep private unsigned updates working for the closed miner group until Authenticode signing is ready.
- Make each build know its install/update channel at compile time.

## Channels

Mainnet:

```text
Qubit Coin Core
%LOCALAPPDATA%\Programs\Qubit Coin Core
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
```

Testnet:

```text
Qubit Coin Core Testnet
%LOCALAPPDATA%\Programs\Qubit Coin Core Testnet
https://download.qubit-coin.io/testnet/windows-x64/manifest.json
```

## Build commands

Testnet first:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -BuildInstaller -SkipTests
```

Mainnet:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -BuildInstaller -SkipTests
```

The build script sets `QUB_BUILD_CONFIG` before compiling, so the binary knows whether it is the testnet or mainnet channel.

## Upload layout

After a testnet build, upload all files from:

```text
dist\updates\testnet\windows-x64\
```

to:

```text
/srv/qub-updates/testnet/windows-x64/
```

After a mainnet build, upload all files from:

```text
dist\updates\mainnet\windows-x64\
```

to:

```text
/srv/qub-updates/mainnet/windows-x64/
```

Keep the old root `QUB-Core-Latest.exe` only as a legacy convenience file. The updater source of truth is now `manifest.json`.

## Manifest fields

```json
{
  "channel": "mainnet-windows-x64",
  "network": "mainnet",
  "version": "1.1.0",
  "platform": "windows-x64",
  "kind": "installer",
  "mandatory": false,
  "consensus_family": "qns-1000",
  "published_at": "...Z",
  "url": "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.1.0-Windows-x64-mainnet-Setup.exe",
  "latest_url": "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe",
  "sha256": "..."
}
```

The updater checks channel, network, version, and SHA256 before installing.

## Security warning

This build still allows unsigned private updates while the network is in closed alpha. Do not expose this update flow as a public trust model. Before public release, require:

- Authenticode signature verification.
- Signed manifests or manifest signature verification.
- A real code-signing certificate.

## Deployment rule

Do not run `docker compose up -d caddy` on the Jinex backend without the correct env file. For update uploads, only move files under `/srv/qub-updates`; no Caddy restart is required.
