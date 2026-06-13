# QUB Core

QUB Core is the reference implementation of the QUB network.

It is a from-scratch Rust implementation of a non-EVM, UTXO-based, CPU/GPU Proof-of-Work blockchain with wallet, node, miner, pool, sync/repair, mempool, and multi-asset functionality.

Current open-source baseline:

QUB Core v1.7.4 HF116

## What is included

- Rust QUB node
- QUB Core GUI
- CPU/GPU mining support
- Solo mining
- Pool mining logic
- Wallet and address tools
- Sync / repair / auto-heal logic
- Mempool and block validation logic
- JIN support
- Stablecoins UI / Ethereum contract support scaffolding
- Verified Governance v1 scheduled activation logic

## Current mainnet status

Verified Governance v1 activation remains scheduled for block #21000.

Consensus before #21000 is unchanged.

## What this is not

This repository is not an investment promise, not a memecoin launch, not an airdrop campaign, and not a profit guarantee.

Mining rewards depend on network hashrate, difficulty, uptime, stale/orphan behavior, pool/solo mode, and other network conditions.

## Build

Install Rust stable first.

Then:

cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core

## Run

Example:

cargo run --release --bin qub-core -- --config .\config\mainnet.toml

## Security

Never share:

- wallet.json
- ethereum-wallets.json
- private keys
- seed phrases
- SSH keys
- .env files

Do not open untrusted wallets or chain data.

## Assets

Runtime UI assets are not included in this source repository yet. Some icons may fall back to text placeholders when building directly from source.

## License

Apache License 2.0.
