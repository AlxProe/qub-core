# QUB Core v1.0.0 Seed Node + Mini-PC Runbook

Use this for the controlled LAN test before a public mainnet announcement.

## Network shape

Recommended first connected test:

```text
1 seed/full node
5 GUI solo miners
same LAN
same network config: regtest for rehearsal, then testnet/mainnet only after rehearsal passes
```

The seed node keeps `qubd.exe node` running. Miner PCs run `qub-core.exe` and use the seed as a bootnode. In this hotfix, GUI miners do outbound sync/relay; they are not inbound P2P listeners unless you separately run `qubd.exe node`.

Do not run `Start-QUB-Seed-Node.cmd` and `Start-Qubit-Coin-Core.cmd` against the same `data_dir` at the same time. Use a seed-only machine first.

## Seed machine

1. Pick one stable mini-PC or laptop.
2. Give it a stable LAN IP, for example `192.168.1.20`.
3. Open Windows Defender Firewall inbound TCP for the selected P2P port:
   - regtest: `19444`
   - testnet: `18444`
   - mainnet: `17444`
4. Edit config:

```toml
[p2p]
enabled = true
bind = "0.0.0.0:19444"
advertise_addr = "192.168.1.20:19444"
bootnodes = []
```

5. Start the seed node:

```powershell
.\Start-QUB-Seed-Node.cmd
```

Keep that window open.

## GUI miner machines

On each GUI miner PC, edit the same network config:

```toml
[p2p]
enabled = true
bind = "127.0.0.1:19444"
advertise_addr = ""
bootnodes = ["192.168.1.20:19444"]
```

Then run:

```powershell
.\Start-Qubit-Coin-Core.cmd
```

Inside the GUI:

1. Create a local mining address or paste a valid address.
2. Leave `Respect 60s target spacing in GUI miner` enabled.
3. Use CPU power 30-60% for the first hour.
4. Start solo mining.

## Optional extra relay nodes

Only on machines that are not mining with the GUI, you can run additional relay/full nodes:

```toml
[p2p]
enabled = true
bind = "0.0.0.0:19444"
advertise_addr = "RELAY_LAN_IP:19444"
bootnodes = ["192.168.1.20:19444"]
```

Then run:

```powershell
.\Start-QUB-Seed-Node.cmd
```

## Health checks

On each machine:

```powershell
.\Sync-QUB-Once.cmd
.\qubd-cli.cmd validate
.\qubd-cli.cmd info
.\qubd-cli.cmd balance
```

All machines should converge to the same `bestblockhash` after sync.

## Expected behavior

- New blocks mined by any GUI miner are broadcast to bootnodes.
- GUI miners sync before candidate creation and re-check the network tip during mining; if the tip changes, the round is stopped and rebuilt.
- If two miners find competing blocks, best-work chain adoption resolves it once one branch pulls ahead.
- The GUI peer web/list panel shows local mining and recent blocks. P2P telemetry is still intentionally minimal in this v1.0.0 build.

## Stop criteria

Stop the test and collect logs if any machine shows:

```text
validate failure
same height but different bestblockhash for more than 2 target intervals
repeated p2p invalid-message score disconnects
wallet/network mismatch
mempool double-spend warnings from your own local test commands
```

## Data reset rule

Before official launch, delete rehearsal data only:

```powershell
Remove-Item -Recurse -Force .\data\regtest -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force .\data\testnet -ErrorAction SilentlyContinue
```

Never delete mainnet data after public mining starts.
