# QUB Core v1.8.5 - HF132

HF132 improves synchronization and Solo/Pool mining reliability for users
upgrading from deployed v1.8.4. No chain upgrade or new activation is required.
It includes the previously unreleased HF129-HF131 work.

- Auto/manual refresh use one snapshot reader, avoiding overlapping mature-chain
  reads. Busy snapshots retry without blocking on a full history replay.
- Storage reads and embedded RPC responses release canonical writer locks
  before expensive work. Validated sync candidates avoid a second full replay.
- Incremental sync follows a moving tip, recognizes duplicate chunks and gives
  healthy peers time after a stalled peer. Catch-up does not synchronously wait
  for per-chunk chain diffusion while holding its writer admission.
- Solo/Pool Start preserves mining intent during catch-up. Stop cancels the
  pending mining request while independent synchronization continues. Mining
  still requires a valid parent and the existing network evidence.
- Sync status distinguishes the last completed wallet read, observed block
  progress and network freshness. Stale speed/ETA expires.
- Pool-share preparation retains bounded batches, cancellation and parent checks.
  Restart preserves the selected pool and owned payout. QNS payouts are pinned
  for the mining run.
- CPU/GPU found-candidate handling, worker retirement and OpenCL resource cleanup
  fixes from HF131 remain. Hash kernels, nonce work and power rules are unchanged.
- RPC persistence/relay failures remain visible and retryable. An immutable
  RPC read cannot hold the live chain mutex throughout a slow callback.
- Preparation uses separate source directories and reusable verified build
  evidence; the deployment route uses mainnet GUI smoke without a manual LAN lab.

Epoch 2 at #24000, block version 2, genesis, difficulty, rewards, pool/share rules,
transaction formats and QUB-FCE-1 remain unchanged. Explorer 0.7.3 remains in place.
No wallet reset, seed authority change or Jinex Wallet deployment is required.

Candidate evidence and limitations are in `HF132-v185-TEST-REPORT.md`.
Actual Windows/GPU and mainnet behavior must pass the deployment runbook.
No fixed synchronization time or hashrate increase is promised.

After publication:

- Download: https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
- Source/release: https://github.com/AlxProe/qub-core/releases/tag/v1.8.5-hf132

These are the planned release locations; this source package does not establish
that the public release has already been published.
