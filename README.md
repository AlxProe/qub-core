# HF133 / v1.8.6 native candidate

This is not a public release. See HF133-CONSENSUS.md and HF133-PUBLIC-RELEASE.md.
Run the supplied HF133 native bundle; historical HF132 build instructions below
do not apply to this candidate. No testnet/regtest run is required or authorized.

# QUB Core v1.8.5 / HF132

HF132 is a synchronization and mining reliability release candidate for the
deployed HF128 / v1.8.4 network. It incorporates the unreleased HF129-HF131
fixes and adds the mature-chain synchronization review described below.
There is no new chain activation, block format or economic rule.

Use `HF132-v185-DEPLOY-RUNBOOK.md` for the current deployment procedure.
Older release documents retained in this tree are historical. Preparation
evidence and remaining platform/live checks are in `HF132-v185-TEST-REPORT.md`.
The source archive alone does not establish production availability.

## Synchronization and mining

- Auto/manual wallet refresh share one attached snapshot reader. A slow read
  cannot be abandoned by a timeout and replaced by overlapping readers.
- Existing FCE readers no longer hold the storage writer mutex during full
  disk reads/replay. Busy UI captures defer without entering a full replay.
- Already validated synchronization candidates retain their validation work.
  Cold disk baselines and genuinely divergent candidates still undergo full
  consensus validation; structural UI snapshots cannot authorize adoption.
- Committed suffix progress survives a moving target. Duplicate peer chunks
  avoid needless fork backtracking. Healthy peers get network time after a
  stalled peer; chain diffusion runs after the catch-up writer is released.
- Start keeps the selected Solo/Pool intent while the mining safety guard
  establishes an eligible parent. Stop cancels mining without canceling the
  independent chain catch-up. Parent checks and delivery proofs remain.
- Displayed progress derives from observed blocks, with expiring rate/ETA.
  Wallet completion, last observed height and verified network tip are distinct.
- Embedded RPC read callbacks work on an immutable captured chain, releasing
  the canonical mutex before expensive response calculation.

HF131 resource cleanup, found-candidate handling, pinned QNS payouts and RPC
retryability fixes are retained. Pool-share cancellation and bounded work
batches from HF130 remain. Actual CPU/GPU behavior requires native Windows
mainnet smoke; this source does not promise a hashrate or catch-up duration.

## Compatibility

Mainnet epoch 2 remains at #24000, block version 2, P2P protocol 2 and storage
format QUB-FCE-1. Genesis, checkpoints, difficulty, rewards, pool/share rules,
transaction serialization and QNS/JIN activations remain unchanged.
The deployed HF128 pool source, all seven configuration TOMLs, GPU kernel,
nonce scanner/work-item policy and P2P wire enum are pinned by source policy.
HF131 GPU resource lifetime changes are retained around those unchanged work
routines. The pool coinbase string `/QUB Core v1.8.4/` is deliberately retained
as existing template data. Cargo.lock is unchanged from HF131 r4.

Explorer 0.7.3 remains deployed; its static assets are not republished by this
release. The existing seed/API compatibility bridge and snapshot publisher
checks remain in the runbook. Jinex Wallet is not a deployment dependency.

## Release workflow

Internal/native tests -> isolated GUI mainnet smoke -> NYC3 and AMS3 seeds/API
-> dist packaging -> copied portable smoke -> public dist -> GitHub/Telegram.
No operator regtest/LAN lab or fixed multi-hour soak is required. Native tests
still use isolated fixture networks internally. Unobserved hardware/live
behavior must be recorded honestly rather than marked as a passed test.

The release kit extracts directly into the existing `qub-node` directory.
PREPARE pins the source ZIP hash and creates a separate hash-named source and
evidence directory. Existing assets and Cargo cache are reused without moving
the old working source. A closed, checked previous mainnet smoke copy may be
the history source for a fresh HF132 smoke directory.

Runtime graphics/audio/fonts/video, wallets, chain data, private keys and build
outputs are excluded from this source archive. Gate 0 restores checked local
passive assets after verifying untouched source. Do not regenerate the source
manifest during deployment: that would approve different, unreviewed bytes.

See `RELEASE_NOTES-v1.8.5-HF132.md`, `HF132-v185-RELIABILITY-REVIEW.md` and
`HF132-v185-TEST-REPORT.md`. `HF132-v185-TELEGRAM-POST.txt` is an unpublished
announcement draft to use only after the publication gates pass.
