# Qubit Coin Core v1.0.0 — Testnet/Mainnet Go-Live Runbook

This runbook is intentionally strict. Do not launch public mainnet until every gate below is green.

## 1. Build from a clean tree

```powershell
cargo clean
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config testnet -Sign -CertSubject "Alexander Proestakis" -BuildInstaller
```

For mainnet, first edit `config\mainnet.toml` and set at least two independent bootnodes:

```toml
[p2p]
bootnodes = [
  "SEED_1_PUBLIC_IP_OR_DNS:17444",
  "SEED_2_PUBLIC_IP_OR_DNS:17444"
]
```

Then build:

```powershell
cargo clean
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config mainnet -Sign -CertSubject "Alexander Proestakis" -BuildInstaller
```

The build script runs `qubd preflight` by default. If preflight fails, do not distribute that bundle.

## 2. Seed nodes

Run at least two seed nodes before releasing the public client. Prefer separate machines, networks, and providers.

Seed node config:

```toml
[p2p]
enabled = true
bind = "0.0.0.0:17444"
advertise_addr = "THIS_SEED_PUBLIC_IP_OR_DNS:17444"
bootnodes = ["OTHER_SEED_PUBLIC_IP_OR_DNS:17444"]
```

Open inbound TCP `17444` on each seed.

Start seed:

```powershell
.\tools\qubd.exe --config .\config\mainnet.toml preflight
.\tools\qubd.exe --config .\config\mainnet.toml node
```

## 3. Miner/user client

Normal users run only:

```text
QUB-Core.exe
```

In the setup wizard, mainnet users should use the shipped mainnet config. No `.cmd` file is needed for normal users.

## 4. Go/no-go checks

Before public mainnet announcement, verify:

```powershell
.\tools\qubd.exe --config .\config\mainnet.toml preflight
.\tools\qubd.exe --config .\config\mainnet.toml validate
.\tools\qubd.exe --config .\config\mainnet.toml peers
.\tools\qubd.exe --config .\config\mainnet.toml sync
```

Required operational gates:

- Signed `QUB-Core.exe`, `tools\qubd.exe`, and installer.
- Published `SHA256SUMS.txt` from the final signed bundle.
- 2+ independent bootnodes online.
- 24h+ public testnet with no persistent tip divergence.
- Restart/reconnect test passes: nodes converge to the same `height` and `bestblockhash` after seed restarts.
- Defender/SmartScreen behavior documented; false positives submitted if needed.
- No screenshots/logs exposing raw peer IPs except operator-only `peers-raw`.

## 5. Mainnet launch sequence

1. Freeze `config\mainnet.toml`.
2. Build signed mainnet bundle.
3. Save and publish SHA256 checksums.
4. Start seed nodes.
5. Run `preflight` on seed nodes and a clean client.
6. Start mining from multiple independent QUB Core clients.
7. Confirm all nodes converge to the same chain tip.
8. Only then announce the public download.
