# QUB Core 1.8.7 / HF134

This is an update for the existing QUB mainnet. HF133 ASERT-MTP rules,
activation height 100000, the protocol epoch and historical consensus rules
remain unchanged. There is no new activation height in this release.

Windows updater
- Moves update preparation off the GUI thread and reduces stack use during hashing.
- Uses a separate native helper to verify the staged installer, wait for the
  exact GUI process to exit, install, verify the installed version and relaunch.
- Records failures and suppresses repeated automatic attempts until an explicit retry.
- Older clients retain their old updater until upgraded. If updating does not
  complete, stop mining, close Core and install the official 1.8.7 installer once.

Synchronization and storage
- Avoids replaying an already known older or identical full snapshot.
- Reuses the exact common prefix only from a locally validated chain and checks
  replacement blocks normally. Remote UTXO snapshots are not trusted.
- Preserves cumulative-work selection and atomic disk/live publication.
- Adds writer phase/progress diagnostics and optimizes coinbase UTXO updates.
- The Linux seed build includes the subsequently verified UTXO sorting change.
  Platform source archives and provenance identify the exact sources of each binary.

Upgrade
Keep a secure wallet backup. Stop mining and close QUB Core. Run the official
installer over the existing mainnet installation; do not uninstall or delete
wallet/chain data. Reopen Core, confirm 1.8.7 and allow validation/sync to finish.
Do not downgrade to a pre-HF133/epoch-2-only release after activation.

Known follow-up work
Long-running memory behavior, removing seed/HTTP acknowledgments from mining
readiness, and startup/recovery duration remain work for the next patch.
Version 1.8.7 does not claim seed-independent mining or a proven long-term RAM plateau.

Official downloads: https://download.qubit-coin.io/
GitHub release: https://github.com/AlxProe/qub-core/releases/tag/v1.8.7-hf134

The Windows installer is unsigned under the existing official-origin trust
policy. Use the official HTTPS download and published checksum.
