# QUB Core v1.0.0 - automatic discovery runbook

This build removes normal-user bootstrap/IP setup.

## User experience

Normal users run only:

```text
QUB-Core.exe
```

They choose a profile in the first-run wizard and finish setup. They do not type IP addresses, bootnodes, seed mode, or advertise addresses.

## LAN rehearsal

`regtest-lan` uses automatic UDP peer discovery on:

```text
TCP 19444  P2P blocks/tx/peer exchange
UDP 19445  LAN peer discovery beacon
```

For the 8-PC LAN rehearsal:

1. Install/copy the same release bundle to every PC.
2. Delete old rehearsal data on every PC before a clean test:

```powershell
Remove-Item -Recurse -Force .\data\regtest-lan -ErrorAction SilentlyContinue
Remove-Item -Force .\data\qub-core-gui-settings.json -ErrorAction SilentlyContinue
Remove-Item -Force .\config\qub-core-regtest-lan.toml -ErrorAction SilentlyContinue
```

3. Open `QUB-Core.exe` on every PC.
4. Pick `Regtest-LAN rehearsal`.
5. Finish setup.
6. Create/paste payout address.
7. Start mining after peers appear.

No PC is manually designated as seed/bootstrap in the GUI. Once two or more QUB Core instances are open on the same LAN and firewall allows TCP/UDP, they discover each other automatically.

## Public testnet/mainnet

Public testnet/mainnet do not rely on users entering IPs. They use official DNS seed domains shipped in the config and reinforced in the binary:

```text
mainnet: seed.qubit-coin.io:17444
testnet: seed.qubit-coin.io:18444
```

Before public release, point those DNS records to independent bootnode servers. Do not publish your home IP in the app. If a server IP changes, update DNS; users keep running the same app.

## Bootnode operations

A bootnode is just a normal always-on QUB full node with stable uptime and an official DNS name. It is not special in consensus and does not need to mine.

Run several bootnodes in different places before mainnet:

```text
seed.qubit-coin.io -> your DO droplet IP


```

## Preflight

The build script runs `preflight` by default. For public testnet/mainnet it rejects raw-IP bootnodes; seed entries must be DNS names and must resolve before a public release build passes.

```powershell
.\tools\qubd.exe --config .\config\mainnet.toml preflight
.\tools\qubd.exe --config .\config\testnet.toml preflight
.\tools\qubd.exe --config .\config\regtest-lan.toml preflight
```

## Release gate

Do not announce mainnet until these are true:

```text
cargo test passes
8-PC regtest-lan converges after restarts
public testnet runs 24h+ with seed.qubit-coin.io reachable
mainnet DNS seed record resolves to the production seed node
Windows build is signed
SHA256SUMS.txt is published from the signed final bundle
```
