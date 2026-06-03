# QUB Core v1.0.0 — LAN rehearsal runbook

Use `regtest-lan` for connected multi-PC testing. Do not use old `regtest` for network convergence testing.

## Clean before hotfix8

Run on every PC:

```powershell
Remove-Item -Recurse -Force .\data\regtest-lan -ErrorAction SilentlyContinue
Remove-Item -Force .\data\qub-core-gui-settings.json -ErrorAction SilentlyContinue
```

## Build and distribute

```powershell
cargo clean
powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 -Config regtest-lan
```

Distribute:

```text
dist\QUB-Core-v1.0.0-windows-x64-regtest-lan\
```

Normal users open:

```text
QUB-Core.exe
```

## Seed machine

Choose a stable LAN IP, for example:

```text
192.168.1.20
```

Open inbound TCP `19444` in Windows Firewall.

In the QUB Core setup wizard:

```text
Profile: Regtest-LAN rehearsal
Bootnodes: empty
Accept peer connections: ON
Advertise address: 192.168.1.20:19444
```

Leave it running. Seed nodes are bootstrap/relay/discovery nodes; they are not consensus authorities.

## Miner machines

Open inbound TCP `19444` if possible.

In the QUB Core setup wizard:

```text
Profile: Regtest-LAN rehearsal
Bootnodes: 192.168.1.20:19444
Accept peer connections: ON
Advertise address: blank or THIS_PC_LAN_IP:19444
```

Then create/paste payout address and start solo mining.

## Checks

```powershell
.\tools\qubd-cli.cmd sync
.\tools\qubd-cli.cmd validate
.\tools\qubd-cli.cmd info
.\tools\qubd-cli.cmd peers
```

All nodes should converge on the same `height` and `bestblockhash`. Short forks can happen; persistent divergence for more than 3 target intervals is a stop condition.
