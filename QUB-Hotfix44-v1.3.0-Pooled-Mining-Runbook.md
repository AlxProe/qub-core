# QUB Hotfix44 / v1.3.0 — Deterministic On-Chain Pooled Mining

This release turns pooled mining into a protocol feature rather than a custodial pool server.

## Activation

- Regtest / regtest LAN: active from block `#1`.
- Testnet: active from block `#3`.
- Mainnet: active from block `#9999` by hardcoded mainnet consensus override.

Mainnet legacy activation constants remain hardcoded:

- QNS: `#1000`
- JIN: `#5555`
- QNS miner split: `#8305`
- JIN conversion: `#8305`
- Pools: `#9999`

## Protocol name/address

The reserved QNS-style protocol name is:

```text
pools.qub
```

Protocol payment address per network is configured in `[pools].protocol_address`.

## Pool mechanics

A pool is created by a normal QUB transaction containing a `POOL_CREATE` marker output. The pool ID is the create transaction ID.

Pool creation payment is non-refundable and split like QNS registration payments:

```text
50% -> pools.qub protocol address
50% -> miner fee in the block containing the transaction
```

Pool fields:

```text
pool_id
name
manager_address
commission_bps
capacity_slots
created_height
```

Names are UTF-8 and emoji-safe, but reject control characters, newlines, tabs, zero-width characters and bidi override characters. Names are not unique; `pool_id` is the unique identity.

## Capacity

Capacity means active miner slots in the rolling share window, not self-reported hashrate.

Default values:

```toml
base_capacity_slots = 8
capacity_step_slots = 8
max_capacity_slots = 128
max_active_pools = 1024
```

Managers can only increase capacity via `POOL_TOPUP`. Capacity cannot decrease.

## Commission

Commission is stored in basis points:

```text
0 bps = 0%
2000 bps = 20% max
```

Managers can only lower commission. Commission increases are invalid.

## Join / shares

Joining does not require QUB balance. A miner joins by submitting a zero-fee PoW-gated `POOL_SHARE` transaction.

The first confirmed valid share makes the miner active in that pool's rolling window.

Consensus checks:

```text
pool exists
pool active
share target met
recent parent block
signature matches miner address
not duplicate
capacity available or miner already active
miner address not active in another pool window
```

## Rewards

Pool blocks use deterministic direct coinbase outputs. No pool manager custody and no internal claim ledger.

Payout input is the pool's confirmed shares in the previous rolling PPLNS window:

```toml
share_window_blocks = 360
```

The current block's share transactions count only for future pool blocks.

Reward split per asset:

```text
manager_commission = floor(total_reward * commission_bps / 10000)
remaining reward -> miners by confirmed share count
rounding -> largest remainder, tie-break by lexicographic address
```

JIN fee rewards from JIN fee-paying transactions in a pool block are distributed by the same pool payout plan. JIN itself is not mined.

## CLI quickstart: regtest

Create a wallet and mine enough spendable balance for pool creation:

```powershell
cargo run -- --config config/regtest.toml wallet-new
cargo run -- --config config/regtest.toml mine 8
```

Create a pool:

```powershell
cargo run -- --config config/regtest.toml pool-create "🔥Fair Pool🔥" 500 8
```

Confirm the pool creation transaction:

```powershell
cargo run -- --config config/regtest.toml mine 1
```

List pools:

```powershell
cargo run -- --config config/regtest.toml pool-list
```

Join by submitting a share:

```powershell
cargo run -- --config config/regtest.toml pool-join <pool_id>
```

Confirm at least one share transaction. This can be a normal mined block; shares in the current block are intentionally not used to pay the current block:

```powershell
cargo run -- --config config/regtest.toml mine 1
```

Mine a pool block:

```powershell
cargo run -- --config config/regtest.toml pool-mine <pool_id> 1
```

## CLI commands

```text
pool-list
pool-info <pool-id>
pool-create <name> [commission_bps] [capacity_slots] [manager-address] [fee]
pool-top-up <pool-id> <extra_capacity_slots> [fee]
pool-set-commission <pool-id> <new_commission_bps> [fee]
pool-join <pool-id> [miner-address]
pool-mine <pool-id> [blocks] [miner-address]
```

## Explorer API

```text
GET /api/v1/pools?limit=25&offset=0
GET /api/v1/pool/<pool-id>
```

Transaction JSON includes parsed pool markers and pool-share details.

## Mainnet rollout

Do local smoke test, then testnet. Mainnet pool protocol activates at block `#9999`.

Old clients will reject post-activation pool data, so all active miners should upgrade before `#9999`.
