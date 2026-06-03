# QUB Core v1.0.0 Hotfix8 — Windows install + LAN rehearsal runbook

This runbook is for controlled regtest-LAN testing before public testnet or mainnet.

## What changed in hotfix8

- QUB-Core.exe is the normal user entry point. Normal users do not need `.cmd` files.
- Release builds target `x86_64-pc-windows-msvc` with static CRT to avoid `VCRUNTIME140.dll was not found` failures on clean Windows PCs.
- First-run setup wizard writes a generated config under `config/qub-core-*.toml`.
- Peer views hide IP addresses by default. Peers show public payout address, or `Guest` when no public payout address is known.
- Mined-block sound plays `assets/mined.mp3` on Windows, with a Windows beep only as fallback.
- Advanced tools are in `tools\` only.
- Regtest-LAN PoW was tuned for connected multi-PC rehearsal. Wipe old `data/regtest-lan` on every PC before testing this version.

## Required assets

Put these files in the source tree before building:

```text
assets/qubit-coin-logo.png
assets/jin-coin-logo.png
assets/mined.mp3
assets/fonts/Ubuntu-BoldItalic.ttf   optional
```

The logo is loaded at runtime. The font is optional. Do not distribute private keys.

## Build release bundle

From the source folder:

```powershell
cargo clean
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan
```

The output folder is:

```text
dist\QUB-Core-v1.0.0-windows-x64-regtest-lan\
```

Normal users open:

```text
QUB-Core.exe
```

Advanced operators can use:

```text
tools\qubd-cli.cmd
tools\Start-QUB-Seed-Node.cmd
tools\Sync-QUB-Once.cmd
```

## Optional installer build

Install Inno Setup, then:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan -BuildInstaller
```

The installer is written under:

```text
dist\installer\
```

## Optional Authenticode signing

Use a real code-signing certificate. Examples:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
  -Config regtest-lan `
  -Sign `
  -CertSubject "Alexander Proestakis"
```

or:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
  -Config regtest-lan `
  -Sign `
  -CertThumbprint "PASTE_CERT_THUMBPRINT_HERE"
```

## Clean old rehearsal data

Run on every PC before hotfix8 LAN testing because regtest-LAN genesis/PoW changed:

```powershell
Remove-Item -Recurse -Force .\data\regtest-lan -ErrorAction SilentlyContinue
Remove-Item -Force .\data\qub-core-gui-settings.json -ErrorAction SilentlyContinue
Remove-Item -Force .\data\regtest-lan\node-identity.json -ErrorAction SilentlyContinue
```

## First seed PC

Pick one LAN machine as bootstrap node, for example:

```text
192.168.1.20
```

Open Windows Firewall inbound TCP:

```text
19444
```

Open `QUB-Core.exe`, choose:

```text
Profile: Regtest-LAN rehearsal
Bootnodes: empty
Let this PC accept peer connections: ON
Advertise address: 192.168.1.20:19444
```

You may leave this PC non-mining, or mine with low CPU after the other machines are connected.

## Miner PCs

Open Windows Firewall inbound TCP `19444` if you want direct peer-to-peer connectivity.

Open `QUB-Core.exe`, choose:

```text
Profile: Regtest-LAN rehearsal
Bootnodes: 192.168.1.20:19444
Let this PC accept peer connections: ON
Advertise address: blank, or LAN_IP_OF_THIS_PC:19444
```

Then:

```text
Create local mining address, or paste a payout address.
Start solo mining.
```

## What you should see

- The map/list should show public payout addresses or `Guest`, not IP addresses.
- The block history should show global active-chain blocks.
- The local miner green dot appears when the local public address mines a recent block.
- All nodes should converge to the same `height` and `bestblockhash` after sync windows.

## Health checks

From the bundle root:

```powershell
.\tools\qubd-cli.cmd sync
.\tools\qubd-cli.cmd validate
.\tools\qubd-cli.cmd info
.\tools\qubd-cli.cmd peers
```

The default `peers` command hides IP addresses. For operator-only diagnostics:

```powershell
.\tools\qubd-cli.cmd peers-raw
```

## Stop criteria

Stop and collect logs if any of these persist for more than 3 target intervals:

- same height but different bestblockhash across several nodes
- repeated validation failure
- repeated stuck sync
- peer list never sees other nodes after firewall checks
- QUB-Core.exe crash or black window on startup

## Notes on block time

A 60-second target is probabilistic. Blocks are not supposed to arrive exactly every 60 seconds. Regtest-LAN uses GUI pacing plus easier PoW so connected mini-PC tests are practical, while still exercising sync/reorg behavior.
