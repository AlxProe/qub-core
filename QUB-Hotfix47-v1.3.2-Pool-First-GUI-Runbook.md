# QUB Hotfix47 / v1.3.2 — Pool-first GUI, compact panels, sync polish

## Scope

GUI/P2P-UX hotfix on top of v1.3.1.

Consensus rules remain the same as v1.3.1:

- QNS mainnet activation: #1000
- JIN mainnet activation: #5555
- QNS miner split: #8305
- JIN conversion: #8305
- Pools mainnet activation: #9999
- `POOLRENAME1` remains supported

## What changed

- Pool mining is now presented as the recommended/default mining path.
- No automatic first-pool/Genesis-pool preselection.
- Last joined/mined pool is persisted in GUI prefs and used as the default selected pool.
- Start pool mining is disabled until a real pool is selected.
- Pools window is now a direct browse/join list, not collapsible sections.
- Row actions are simplified: Mine/Stop + More -> Join/Manage.
- Create pool and Manage pool open in separate attached windows with safety text.
- Pool-mined recent blocks display the pool name instead of the manager address/“you”.
- Pool-mined blocks do not show the solo mined-block visual card.
- Left panel categories are collapsible.
- Right balances/activity panels can collapse to compact icon/value views.
- Background snapshot refresh now does a short P2P convergence pass before reading the local chain.
- Mining loops run slightly stronger P2P convergence to reduce stale-looking views.

## Local build

```powershell
cd C:\Users\proes\Desktop\qub-node\qubd-v1.3.2
New-Item -ItemType Directory -Force .\assets | Out-Null
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
```

## Mainnet local smoke

```powershell
cargo run --release --bin qub-core -- --config config/mainnet.toml
```

Check:

1. Title shows v1.3.2.
2. Left categories collapse/expand.
3. Pools sub-category is directly under Mining controls.
4. Start pool mining is disabled until a pool is selected.
5. Pools window shows only the pool list and row actions.
6. No Select buttons exist in pool rows.
7. Last joined/mined pool is highlighted.
8. Mine/Stop buttons use mining icons.
9. Create pool opens a separate Create pool window.
10. More -> Manage opens a separate Manage pool window attached to that pool.
11. Recent global blocks show pool names for pool-mined blocks.
12. Pool blocks do not show the solo mined-block card.
13. Address balances/activity compact panels can collapse/expand.
14. Miner remains connected and follows current mainnet height.

## Distribution manifest

Treat as mandatory GUI/protocol-compatibility replacement for v1.3.1:

```text
consensus_family = qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename
activation_height = 9999
mandatory = true
chain_upgrade = false
```
