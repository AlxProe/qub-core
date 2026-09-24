# QUB Core 1.8.6 / HF133 — mandatory mainnet upgrade

**Install before block #100000.** Official downloads:
https://download.qubit-coin.io/

## Windows upgrade

If your older auto-updater closes the app but leaves the old version installed,
stop mining, close QUB Core and run the official 1.8.6 installer over the
existing installation. Reopen the app and confirm version 1.8.6. Keep the
existing wallet and chain-data folders. The manual installer path was verified;
the older embedded updater's permanent fix remains open.

Installer SHA256:
`f1dc68a7bfa04ea534b20194e88f107eaa338454f9c6d89912aa6dadcf3fce7e`

## Release assets

The attached Windows installer, portable ZIP, Linux archive, canonical source
ZIP, metadata, provenance and checksums are the same verified bytes served by
the official download origin. No binary has been rebuilt for this GitHub release.
For source checksum verification use `QUB-Core-v1.8.6-HF133-source.zip`, not
GitHub's automatically generated source archives.

The repository preserves the canonical source contents and adds publication
documentation while retaining the existing repository governance files.

## Published release notes

# QUB Core 1.8.6 / HF133

**Mandatory mainnet chain upgrade. Activation: block #100000.**

Install QUB Core 1.8.6 before mainnet reaches block 100000. This requirement
applies to solo miners, pool miners, wallets and node operators. Use the
official release and the published SHA256 checksums.

HF133 changes the mainnet difficulty adjustment to ASERT-MTP from block
100000, with a 60-second target, a one-hour half-life and an 11-block
median-time-past window. The activation uses protocol epoch 3 / block version
3. Earlier blocks continue to use their historical consensus rules.

The change is intended to reduce the repeated difficulty swings and long
pauses observed with variable mining power. It cannot guarantee a block every
60 seconds or remove pauses when mining power disappears.

This release also includes the synchronization and memory corrections from
HF129-HF132 and memory-r1: bounded incoming chain synchronization, release of
old connection snapshots, avoidance of redundant full-history replay for an
identical overlap, and buffered chain-message serialization. GPU kernels,
reward amounts, pool rules, QNS/JIN rules and the chain storage format retain
their existing behavior.

Public 1.8.4 and the private 1.8.5/HF132 canary do not implement epoch 3. After
activation, they cannot follow the upgraded chain. Version-2 blocks mined
after activation are rejected by upgraded nodes. Do not downgrade to an
epoch-2-only binary after activation.

The release follows the normal public download and updater route; there is no
private miner activation list. Activation is defined by height, not a fixed
calendar time.

Windows packages use the established official-origin unsigned policy. Verify
the published SHA256 and obtain the installer from
https://download.qubit-coin.io. An unsigned installer is not an Authenticode
signature. The updater checks the official HTTPS origin, installer version,
size and SHA256 under this policy.

The Windows and Linux builds passed the fourteen HF133 native regressions.
The Linux validator replayed captured mainnet through block 75162; the GUI
copy validated through 75250 and passed manual Solo/Pool checks. The NYC3
canary completed over 30 minutes of observation, ending at block 75343 with
matching seed/API ancestry and no OOM or seed restart during that observation.
These checks cover the captured data and pre-activation operation. Offline
mainnet-parameter tests cover the new activation boundary; live mainnet has
not yet crossed that boundary in this evidence.
