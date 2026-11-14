# QUB Core 1.8.7 / HF134 — native verification candidate

This candidate is based on published HF133 commit
`eff6cf8031332fb66210416ace73521cf906bf29`. It is not a published release.

## Windows updater

- Hash files through a 64 KiB heap buffer. The previous 1 MiB stack buffer was
  also used on the Windows GUI thread before spawning the installer launcher.
- Prepare the update off the UI thread. A copied native helper verifies the
  staged bytes, version and existing signature policy and holds the installer
  against changes. The GUI acknowledges readiness, closes through its event
  loop, and the helper waits for its exact process handle to signal exit.
- Launch Setup and the installed GUI directly with separate arguments. Keep a
  setup log, phase log and failure receipt. Verify the installed GUI version
  before relaunch. Do not force-close unrelated processes or claim rollback on
  an unsuccessful/partially completed installation.
- A failed update suppresses automatic retries until a successful explicit
  retry clears the receipt. Existing 1.8.4/1.8.6 clients still contain their old
  updater; users affected there may need the official installer once to upgrade.

## Stale mined tips and synchronization

- A known older/identical full snapshot no longer triggers full-chain replay.
- For a competing branch, reuse only the exact block bodies in the common
  prefix of the consensus-validated live chain. Reconstruct that prefix's UTXO
  effects with the existing UTXO code, then run normal `connect_block` checks
  on every replacement block. Without a validated live baseline, perform full
  replay. Remote UTXO and mempool snapshots are not trusted.
- Preserve cumulative-work selection, equal-work reanchor evidence, checkpoint
  rules and atomic disk/live publication. The internal validated reanchor path
  avoids a second replay of the candidate. The public API still validates input.
- Report the actual writer phase and last checked height. The GUI records
  bounded diagnostic history in `data/hf134-sync.jsonl` (one rotated copy).

HF133 ASERT-MTP, activation **100000**, epoch/block version and historical
consensus rules remain unchanged. No miner coordination, testnet or regtest
execution is part of this verification workflow.

## Evidence and remaining gates

The live writer stall is reported twice, both following the user's own stale
mined tip. This agrees with the expensive replay path found in the code, but
the precise running owner was not captured in those incidents. The historical
Windows report showed a verified installer and GUI crash before launcher
creation; the stack buffer is a concrete defect, not a proven crash dump trace.

The native package runs targeted offline MAINNET fixture tests, unchanged HF133
DAA tests, Windows hashing/handle/installer-verification tests and builds the
three binaries. It does not install them or touch live data/services. Successful
compilation is followed by real-mainnet export replay, GUI Solo/Pool/stale-tip
smoke and updater end-to-end installation/relaunch verification before release.
Linux/server canaries and publication remain separate gates.
