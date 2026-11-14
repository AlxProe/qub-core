# QUB Core 1.8.7 / HF134

QUB Core is the Qubit Coin node, wallet and CPU/GPU miner, written in Rust.

Version 1.8.7 updates Windows installation, synchronization and storage on the
existing mainnet. HF133 ASERT-MTP / protocol epoch 3 has applied since block
100000. **HF134 introduces no new consensus activation.**

## Download and upgrade

- [Official downloads](https://download.qubit-coin.io/)
- [Windows 1.8.7 installer](https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.7-Windows-x64-mainnet-Setup.exe)
- [GitHub release: portable, Linux and exact platform source archives](https://github.com/AlxProe/qub-core/releases/tag/v1.8.7-hf134)
- [Release notes](../release/HF134/RELEASE_NOTES-v1.8.7-HF134.md)
- [SHA256 checksums](../release/HF134/SHA256SUMS.txt)
- [Explorer](https://explorer.qubit-coin.io/)

Keep a secure wallet backup. Stop mining, close Core and install over the existing
mainnet installation. Keep wallet and chain data. Reopen, confirm version 1.8.7
and let validation/sync finish. If an older embedded updater does not complete,
run the official installer manually once. The new updater takes effect after upgrading.

Installer SHA256:

```text
a73eaf8f91b8a026e270a5f7afdc04ae757e1cf57bafcbfa4112f47dc8713746
```

The Windows installer is unsigned under the existing official-origin policy.
Only use official HTTPS downloads and the published checksum.

## What changes

- Windows update preparation leaves the GUI thread. A separate native helper
  verifies the staged installer, waits for the exact GUI process, installs,
  verifies the installed version, and relaunches. Failed automatic attempts are recorded.
- Known older/identical full snapshots avoid redundant replay. Prefix reuse
  requires locally validated history; replacement blocks are checked normally.
- Atomic disk/live publication and cumulative-work selection are preserved.
  Writer diagnostics and coinbase UTXO optimizations are included.
- The Linux seed build includes the subsequently verified UTXO sorting change.

## Exact source provenance

The repository root contains the **441 exact files used for the Linux build**.
The Windows installer was built from a variant differing in only `Cargo.lock`
and `src/fast_storage.rs`. Those exact Windows files are retained under
`release/HF134/windows-source-overrides/`. Use the named platform source ZIPs
for the original tested inventories; GitHub's automatically generated ZIP is a
repository snapshot with additional publication documentation.

- [Platform file hashes](../release/HF134/SOURCE-MANIFESTS.json)
- [Build and verification provenance](../release/HF134/RELEASE-PROVENANCE.json)
- [Linux binary usage](../release/HF134/README-LINUX.txt)

Historical root README/runbooks and older release directories remain historical
records. Their candidate notices and old build/release manifests do not describe
this final publication. Existing license, security and contribution files are preserved.
No GitHub Actions workflow is introduced or automatically rebuilding these binaries.
Bit-for-bit reproducibility across arbitrary compiler environments is not claimed.

## Next patch

The three deferred items are recorded in [NEXT-PATCH.json](../release/HF134/NEXT-PATCH.json):
long-running memory behavior, removal of seed/HTTP acknowledgments from mining
readiness, and cold-start/recovery duration. **1.8.7 does not implement
seed-independent mining or establish a long-term RAM plateau.**
