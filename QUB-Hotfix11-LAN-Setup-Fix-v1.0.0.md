# QUB Core v1.0.0 hotfix11 LAN setup fix

This hotfix prevents the most common LAN rehearsal failure: every GUI node accidentally being configured as an isolated bootstrap node.

## Correct topology

Use exactly one first seed/bootstrap node at the start of a clean LAN rehearsal.

Seed PC:

```text
Setup profile: Regtest-LAN rehearsal
This PC is the first seed/bootstrap node: ON
Bootnodes: empty
Accept peer connections: ON
Advertise address: SEED_LAN_IP:19444, for example 192.168.1.20:19444
Mining: OFF until at least one peer is reachable
```

Miner PCs:

```text
Setup profile: Regtest-LAN rehearsal
This PC is the first seed/bootstrap node: OFF
Bootnodes: SEED_LAN_IP:19444, for example 192.168.1.20:19444
Accept peer connections: ON
Advertise address: blank for auto-detect, or THIS_PC_LAN_IP:19444
Mining: ON only after the GUI shows at least one reachable peer
```

Only the first seed should ever have `bootnodes=0`. If every PC logs `bootnodes=0`, every PC is isolated and they will mine separate forks.

## Clean restart for a rehearsal

On every PC, close QUB Core, then delete only the LAN rehearsal state:

```powershell
Remove-Item -Recurse -Force .\data\regtest-lan -ErrorAction SilentlyContinue
Remove-Item -Force .\data\qub-core-gui-settings.json -ErrorAction SilentlyContinue
Remove-Item -Force .\config\qub-core-regtest-lan.toml -ErrorAction SilentlyContinue
```

Then open only:

```text
QUB-Core.exe
```

Do not launch seed/debug `.cmd` files for normal miners.

## Verification

On each PC:

```powershell
.\tools\qubd-cli.cmd peers
.\tools\qubd-cli.cmd sync
.\tools\qubd-cli.cmd validate
.\tools\qubd-cli.cmd info
```

The important values are:

```text
height
bestblockhash
reachable peers
```

Short forks can happen while blocks are being found. Persistent same-height/different-hash after several sync cycles is not acceptable.
