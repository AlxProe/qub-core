# HF133 official mainnet release route

This candidate implements the requested public upgrade path. No private miner
coordination, testnet or regtest is part of the procedure. No server operation
or public publication is performed by the native verification helper.

Completed preparation:
- Isolated source based on verified HF132 plus the exact memory-r1 correction.
- Mainnet-only height activation, historical rule preservation and epoch 3.
- Offline mainnet header/arithmetic/model checks and fourteen native Rust tests.
- Source inventory, external bundle checksum, native test-count verification.
- Public release notes and chain-upgrade metadata draft.

Remaining gates, in order:
1. Run the provided Windows native helper and review its report. The helper
   discovers all fourteen named tests, requires all fourteen to pass, builds
   three release executables and runs a disposable mainnet-genesis preflight.
   It never uses the old all-tests gate or starts networking/mining.
2. Compile the same source on Linux. Review the full current mainnet using the
   existing saved chain and the new validator. Header fixtures alone cannot
   establish full transaction replay compatibility.
3. Exercise the Windows GUI and use a sequential seed canary with the existing
   memory limits and rollback protections. AMS3 hosts production Jinex: do not
   overwrite shared binaries or touch Explorer/testnet/Jinex services. Observe
   both height and full hash; check ancestors when short-lived tips differ.
4. Refresh canonical mainnet height/hash from both seeds and the public API.
   Check ancestors on differing tips; do not equate a stale tip with a fork.
   Decide a FINAL activation height with sufficient public notice. Candidate
   H=100000 must not be used automatically if testing consumed its notice window.
   Minimum remaining headroom at publication: 4320 blocks, AND a conservative
   wall-clock notice estimate using recent fastest sustained block cadence.
   4320 blocks is not automatically three days. Target at least seven days of
   public notice; with variable hashrate, use extra height headroom. Rebuild and
   rerun all gates if changing H before publication. Freeze H after publication.
5. Prepare the official Windows installer, Linux binary, source archive and
   checksums from the verified source. Review actual installer metadata and
   executable hashes. Existing historical HF132 publishers are disabled for
   this candidate: they encode non-upgrade metadata and cannot approve HF133.
6. Publish through the normal official release, download and updater channels.
   Advertise mandatory=true, chain_upgrade=true, version=1.8.6, hotfix=HF133,
   activation_feature=asert_mtp, FINAL activation_height, block version=3.
   Publish the same height and checksums in the public upgrade notice. Allow
   anyone, including unannounced new miners, to obtain the same public release.
7. Install the verified release sequentially on the seeds before activation;
   observe memory, P2P heartbeat and canonical ancestry. A pre-activation canary
   validates historical compatibility and resource behavior; the offline tests
   cover the new boundary. Once mainnet crosses H, never roll back to a binary
   that only understands epoch 2. Any post-activation fix must retain epoch 3.

The current native bundle is a verification input, not public-release approval.
No fabricated pass receipts, signatures, published checksums or live-height
claims are included. The 128-header arithmetic fixture ends at 74297. Separately,
the 2026-09-23 03:43 UTC preflight matched both seeds and the local API at 74786;
the public snapshot at 74785 matched their exact ancestor. These are captured
observations, not a live freshness check at publication time.

r4 changes the unpublished candidate height from 80000 to 100000. Prior Windows,
Linux replay and GUI passes used H=80000 and are historical evidence, not binary
approval for r4. Rerun the gates above against r4's source and executable hashes.
The new height is preparation for release review, not a public commitment.

At the captured height, H=100000 leaves 25214 blocks: 11.91 days if the fastest
observed 32-block MTP cadence were sustained, or 22.198 days at the fastest
120-block cadence. These are extrapolations, not calendar guarantees. Sustained
15/20-second blocks instead give 4.377/5.837 days, below the seven-day target.
Review those faster-hashrate scenarios again before publication. Do not declare
the notice gate passed merely because this candidate has compiled. If the
conservative publication-time estimate falls below the target, move H farther
out while still unpublished, then rebuild and repeat the required gates.
