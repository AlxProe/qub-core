# QUB Core v1.0.0 — Mainnet release readiness checklist

This checklist is the gate between LAN/testnet rehearsal and a public mainnet release.

## Must be frozen before mainnet

- `config/mainnet.toml` genesis time, bits, nonce, magic, default port, address prefix.
- At least 2–3 independent bootnodes in different networks/locations.
- Public SHA256 checksums for the source archive, release bundle, installer, and executables.
- Authenticode-signed Windows executables and installer.
- Written release notes with exact version, config hash, genesis hash, and bootnode list.

## Must pass before mainnet announcement

- 24h+ LAN rehearsal with 6+ machines and no persistent divergence.
- 24h+ public testnet rehearsal with at least 2 seed nodes and external miners.
- All nodes converge after seed restart, miner restart, and temporary network partition.
- `qubd validate` passes on every node after mining, sync, and restart.
- Wallet deletion warning tested.
- Address-only mining tested.
- Defender/SmartScreen outcome documented; any true Defender detections submitted as false positives before public distribution.
- Release bundle rebuilt from a clean checkout and hash-verified.

## What hotfix8 makes production-grade

- Static CRT release build to avoid missing VCRUNTIME runtime on clean Windows installs.
- Normal user entrypoint: `QUB-Core.exe`.
- First-run setup wizard for regtest-LAN, testnet, and mainnet profiles.
- P2P embedded inside QUB Core for normal miners.
- Public-address-only peer display by default.
- Advanced raw peer diagnostics separated behind `peers-raw`.
- Optional Inno Setup installer script.
- Optional Authenticode signing in the release build script.

## What still depends on real infrastructure

- Real signing certificate and timestamping service availability.
- Final public DNS names/IPs for bootnodes.
- Final mainnet genesis launch timestamp.
- External security review and reproducible build environment.

Do not reuse regtest-LAN data for mainnet. Do not launch mainnet with only one bootnode.

## Hotfix9 gate

Hotfix9 adds `qubd preflight`. Public testnet/mainnet bundles should be built without `-SkipPreflight`; a failed preflight means the bundle is not launchable.
