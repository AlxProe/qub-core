# Qubit Coin Core v1.0.3 — Windows miner guide

QUB Core is the normal Windows app for miners and users.

## Normal user entry point

Open:

```text
QUB-Core.exe
```

Do not start from command files unless you are operating a seed node or debugging.

## First run

The setup wizard asks only for the network profile: Regtest-LAN rehearsal, Testnet, or Mainnet. Peer discovery is automatic. Normal users do not enter bootnodes, seed IPs, or advertise addresses.

After setup, create a local mining address or paste an external payout address. If you paste only an address, QUB Core can mine without storing a private key.

## Assets

Place these beside the app under `assets\`:

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
fonts\Ubuntu-BoldItalic.ttf   optional
```

`mined.mp3` plays when this machine mines a block. `network-mined.mp3` plays when another miner finds a block. `mining-on.mp3` plays once every 4 seconds while hashing so it can sync with the 4-second `mining-on.gif` loop. All sound toggles default to ON for fresh settings.

## Privacy in the GUI

The GUI does not show peer IP addresses by default. It shows the public payout address announced by a peer. If a peer has not announced a payout address, it shows `Guest`.

Advanced operators can inspect raw peer data through:

```powershell
.\tools\qubd-cli.cmd peers-raw
```

## Health checks

From the installed/bundle folder:

```powershell
.\tools\qubd-cli.cmd sync
.\tools\qubd-cli.cmd validate
.\tools\qubd-cli.cmd info
.\tools\qubd-cli.cmd peers
```

The default `peers` command redacts IP addresses.

## VCRUNTIME140.dll

Official hotfix8 release builds use static CRT linking. Build with:

```powershell
cargo clean
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan
```

Copy the resulting `dist\QUB-Core-v1.0.3-windows-x64-regtest-lan\` folder to test machines. Do not copy old `target\release` binaries.

## Advanced tools

Advanced tools live in:

```text
tools\
```

Use them for seed nodes, sync checks, validation, and raw diagnostics. Normal miners should open only `QUB-Core.exe`.

## Automatic discovery update

Normal users should not edit bootnodes or IP addresses. Open `QUB-Core.exe`, choose a profile in the first-run wizard, create/paste a payout address, and start mining.

For `regtest-lan`, QUB Core uses automatic LAN discovery. Allow TCP `19444` and UDP `19445` through Windows Firewall.

For public testnet/mainnet, QUB Core uses `seed.qubit-coin.io` DNS seed shipped in the release config. Users do not enter the developer's IP address.
