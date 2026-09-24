# QUB Core 1.8.6 / HF133

QUB Core is the Qubit Coin node, wallet and CPU/GPU miner, written in Rust.

**Mandatory mainnet upgrade: install 1.8.6 before block #100000.**
This applies to wallets, solo miners, pool miners and node operators.
Activation is determined by block height, not a calendar date.

## Download and update

- [Official downloads](https://download.qubit-coin.io/)
- [Windows 1.8.6 installer](https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.6-Windows-x64-mainnet-Setup.exe)
- [Linux x64 node](https://download.qubit-coin.io/releases/v1.8.6-HF133/QUB-Core-v1.8.6-linux-x64-mainnet.tar.gz)
- [Windows portable package](https://download.qubit-coin.io/releases/v1.8.6-HF133/QUB-Core-v1.8.6-windows-x64-mainnet.zip)
- [Canonical source archive](https://download.qubit-coin.io/releases/v1.8.6-HF133/QUB-Core-v1.8.6-HF133-source.zip)
- [Release checksums](https://download.qubit-coin.io/releases/v1.8.6-HF133/SHA256SUMS.txt)
- [Explorer](https://explorer.qubit-coin.io/)

If an older Windows updater closes QUB Core but leaves it on the old version,
stop mining, close QUB Core, download the official installer and run it over
the existing installation. Reopen QUB Core and check that it displays 1.8.6.
Keep the existing wallet and chain-data folders. This manual upgrade path
was verified; a permanent fix for the older embedded updater remains open.

Windows installer SHA256:

```text
f1dc68a7bfa04ea534b20194e88f107eaa338454f9c6d89912aa6dadcf3fce7e
```

## What changes

At block #100000, HF133 activates ASERT-MTP difficulty adjustment with a
60-second target, one-hour half-life and an 11-block median-time-past window.
It uses protocol epoch 3 and block version 3. Earlier blocks retain their
historical rules. Older epoch-2-only releases, including 1.8.4, cannot follow
the upgraded chain after activation.

The change addresses repeated difficulty swings under variable mining power.
It does not promise a block every 60 seconds or eliminate pauses when mining
power disappears. Sync and memory corrections also ship in this release.
Reward amounts, pool rules, GPU kernels, QNS/JIN rules and the chain storage
format retain their existing behavior.

## Source and verification

The repository includes all 432 files from the published source archive,
byte-for-byte. These include the 226 original tested source files and the
201 external Windows build assets used in native verification. Existing
LICENSE, NOTICE, CONTRIBUTING.md and SECURITY.md are retained from the
previous repository commit. This GitHub README and the public release/notice
documents are additional publication documentation.

The root README and earlier runbooks are historical preparation records.
For the final HF133 release decision, use:

- [Source/build guide](../README-RELEASE-HF133.md)
- [Final metadata](../release/HF133/RELEASE-METADATA.json)
- [Release notes](../release/HF133/RELEASE_NOTES-v1.8.6-HF133.md)
- [Build provenance](../release/HF133/BUILD-PROVENANCE.json)

The original manifests and SHA256SUMS.txt still describe their original
source inventories; they have not been regenerated to approve changed code.
The official source ZIP SHA256 is:

```text
4c32f1c3c0d2457068b4283b391be820ce85ca5ee6422b57dc2ce9e917b895b4
```

GitHub's automatically generated source ZIP has different archive bytes.
Use the named canonical source asset when checking the checksum above.

Windows/Linux native regressions, captured-mainnet replay, GUI Solo/Pool
smoke and pre-activation seed/API checks passed. Offline mainnet-parameter
tests cover the activation boundary. These records do not establish live
post-activation behavior or reproducible builds across arbitrary toolchains.
See the source/build guide for compile-only commands.
