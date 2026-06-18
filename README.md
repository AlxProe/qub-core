# QUB Core v1.7.5 HF117 source pack

Full replacement crate for `qub-node/qubd`.

HF117 / v1.7.5 is a non-consensus hotfix. It does not change DAA, block validity, activation heights, genesis, checkpoints, seeds, or protocol economics. It focuses on mempool/reorg recovery, exact pending-transaction rebroadcast, and GUI miner pacing/rebuild reliability.

Included:

- UTXO active-chain state and deterministic replay validation.
- Double-SHA256 block headers, merkle root, compact PoW target, local CPU mining.
- BTC-like QUB economics at 10x block speed: 60-second target, 5 QUB initial subsidy, 2,100,000-block halvings, 21M max supply cap.
- Early-launch difficulty adjustment: 60-block mainnet window, bounded 4x per retarget.
- Fees, coinbase maturity, mempool conflict checks.
- secp256k1 ECDSA P2PKH-style transfers with HASH160 pubkey hashes.
- P2P bootnodes, peer discovery, block/chain sync, tx relay, and best-work chain adoption.
- Local wallet create/import-safe mining flows for QUB Core.
- Native JIN support and deterministic non-custodial pooled mining.


## HF117 / v1.7.5 non-consensus reliability hotfix

HF117 targets the production symptoms where normal QUB transactions could disappear after a stale local suffix/reorg and where a fast GUI miner could keep taking sequential coinbase-only blocks because pacing was disabled.

Changes:

```text
- No chain upgrade.
- No seed update.
- No activation-height change.
- DAA and PoW validity are unchanged.
- Reorg adoption resurrects non-coinbase txs from disconnected local blocks back into mempool when still valid.
- Persistent raw pending-tx outbox: wallet-pending-txs.json.
- GUI/CLI-created transactions are remembered until confirmed and rebroadcast by exact txid/raw tx.
- Transaction status checks can recover a NotFound wallet tx from the outbox after stale-chain recovery.
- GUI solo/pool mining target-spacing pacing is enabled by default.
- Pacing uses lightweight live-tip probes instead of heavy sync loops while waiting.
- Mempool double-spend checks run before heavy contextual validators.
- Explorer static bundle updated to v0.6 shell; runtime assets remain external in no-assets builds.
```

Quick regtest:

```powershell
cargo test
Remove-Item -Recurse -Force .\data\regtest -ErrorAction SilentlyContinue
cargo run -- --config config/regtest.toml init
cargo run -- --config config/regtest.toml wallet-new
cargo run -- --config config/regtest.toml mine 3
cargo run -- --config config/regtest.toml balance
cargo run -- --config config/regtest.toml validate
```

Create and mine a transfer in PowerShell:

```powershell
$walletNew = cargo run --quiet -- --config config/regtest.toml wallet-new 2>&1
$RECIPIENT = (($walletNew | Select-String -Pattern '^address:\s+(\S+)' | Select-Object -Last 1).Matches.Groups[1].Value)

cargo run -- --config config/regtest.toml send $RECIPIENT 1.25 0.00001
cargo run -- --config config/regtest.toml mine 1
cargo run -- --config config/regtest.toml validate
```

Run P2P seed/full node:

```powershell
cargo run -- --config config/regtest.toml node
```

Sync once from configured bootnodes:

```powershell
cargo run -- --config config/regtest.toml sync
```

Security note: v1 local wallet stores plaintext `secret_key_hex` when enabled. Mainnet config keeps it disabled by default. Set `QUB_ALLOW_PLAINTEXT_WALLET=1` only when you explicitly accept that risk.

## Windows GUI miner

This source pack includes a native Windows-friendly GUI miner binary:

```powershell
cargo build --release --bin qub-core
.\target\release\qub-core.exe --config config/regtest.toml
```

For release packaging and mini-PC instructions see:

```text
README-MINER-WINDOWS.md
scripts/build-windows-release.ps1
assets/README.md
```

Place your UI assets at:

```text
assets/qubit-coin-logo.png
assets/jin-coin-logo.png
```

## Hotfix44 / v1.3.0 pooled mining

v1.3.0 adds deterministic non-custodial pooled mining.

Mainnet pool activation is fixed at block `#9999`. Testnet activation is `#3`, and regtest is active from `#1`.

Pool rewards are paid directly from the block coinbase using confirmed PPLNS share proofs. The manager never custodies miner rewards. Pool creation and top-up payments split 50/50 between the `pools.qub` protocol address and the miner who mines the protocol transaction, matching the QNS payment split model.

CLI:

```text
pool-list
pool-info <pool-id>
pool-create <name> [commission_bps] [capacity_slots] [manager-address] [fee]
pool-top-up <pool-id> <extra_capacity_slots> [fee]
pool-set-commission <pool-id> <new_commission_bps> [fee]
pool-join <pool-id> [miner-address]
pool-mine <pool-id> [blocks] [miner-address]
```

See `QUB-Hotfix44-v1.3.0-Pooled-Mining-Runbook.md` for the rollout and smoke-test sequence.

## v1.3.2 Hotfix46 — Full Pools GUI

This release keeps the v1.3.0 pooled-mining activation schedule and adds the GUI flows needed by normal users and pool managers.

Mainnet consensus remains:

```text
QNS active:              #1000
JIN active:              #5555
QNS miner split:         #8305
JIN conversion:          #8305
Pools activation:        #9999
```

Testnet pooled mining was packaged for activation at block `#3285` in this hotfix branch.

GUI additions:

```text
Pools window
Browse / Join pools
Create pool
Manage my pool
Rename pool
Decrease commission
Pay extra for capacity increase
Start pool mining
```

Pool names are still not unique. The pool ID is the identity. Emoji are allowed, while control, zero-width, and bidi override characters are rejected.

`POOLRENAME1` is added as a manager-only pool action. Because this is a post-v1.3.0 pool-management consensus marker, v1.3.2 should be treated as mandatory before mainnet reaches block `#9999`.

## v1.3.2 Hotfix47 — Pool-first GUI and sync UX

This release keeps the v1.3.1 pooled-mining consensus rules and mainnet pool activation height. It improves the post-activation GUI:

- Pool mining is presented as the recommended/default mainnet mining mode.
- QUB Core never auto-selects the first pool. Start pool mining stays disabled until a user selects/mines/joins a pool.
- The last joined/mined pool is remembered and highlighted.
- Pools browse window is simplified to a direct list with row-level Mine/Stop and More -> Join/Manage actions.
- Create pool and Manage pool open as separate attached-data windows.
- Recent global blocks show the pool name for pool-mined blocks.
- Pool-mined blocks do not show a misleading solo “Your mined block” visual card.
- Left-side controls are collapsible for smaller displays.
- Address balances and Address Activity can collapse into compact right-side views.
- GUI snapshot refresh and miner-side P2P convergence run a little more aggressively to reduce stale-looking local views.

## v1.4.7 Hotfix49 — Fork-safety, mining guard, DAA v2

This release adds a mandatory mainnet fork-safety checkpoint at block `#10367` and hash `21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000`, preventing nodes from accepting branches that diverged before the post-pooled-mining recovery point. It also adds mining guards, stronger bootnode defaults, fresh-peer prioritization, and DAA v2 activation for faster return toward 60-second block spacing after sudden hashrate swings.

Mainnet DAA v2 activates at `#10500`; testnet DAA v2 activates at `#3330`.
