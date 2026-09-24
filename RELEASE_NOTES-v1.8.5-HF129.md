# QUB Core v1.8.5 / HF129 Release Notes

Release-candidate date: 2026-08-25  
Release class: mandatory mainnet reliability update; no consensus activation

HF129 addresses the observed case where block production, QUB Explorer and
Jinex Wallet could stop advancing for long periods even though one or both
seeds were still moving. It also closes the Pool-to-Solo restart path triggered
when a synchronization pause overlapped a retiring mining controller.

## What changed

### Canonical commit freshness

- The live canonical owner is published immediately after the durable Fast
  Chain Engine `CURRENT.json` commit.
- The monolithic compatibility `chain.json` export no longer runs synchronously
  on the canonical commit path every 256 blocks or six hours. It now runs in one
  detached, coalesced, best-effort worker after live publication.
- Cleanup, status-file and compatibility-export errors that occur after the
  `CURRENT.json` rename can no longer make a successfully installed commit look
  failed to the caller or prevent live-tip publication.
- The compatibility exporter uses its own cross-process lease and never takes
  the canonical writer lease. Storage telemetry exposes exporter activity and
  failure counters.

### Sync/Repair orchestration

- Automatic, manual and mining catch-up now meet at the real process-wide P2P
  writer coordinator.
- Background pulses coalesce immediately. Manual, mining-safety and deep repair
  requests wait within their single absolute deadline instead of being consumed
  as false successful no-ops when the writer is busy.
- Sync reports now distinguish writer-busy, progress, fresh-at-tip,
  no-progress and failure. Fresh-at-tip requires explicit canonical official or
  quorum proof, and observed target height/hash always remain a paired identity.
- Only real canonical progress or a freshly verified at-tip result resets the
  GUI stall/escalation clock.
- HTTP, DNS, peer probes and rescue stages share bounded remaining-time budgets;
  no long rescue stage is started after the total deadline has expired.
- A public `tip.json` timestamp older than three minutes, missing, or too far in
  the future is not accepted as current liveness/mining evidence. The snapshot
  remains usable only as consensus-validated bootstrap transport.

### Mining resume safety

- Mining resume state is an explicit `Solo` or `Pool(pool_id)` intent.
- Every controller has a generation and exact mode. A restart plan is valid only
  for the retiring generation and the same acknowledged mode.
- Manual Stop, rejected plans and invalid/empty Pool IDs fail closed.
- A Pool miner paused for canonical catch-up or reconfiguration can resume only
  the same Pool. It never falls through to Solo.
- HF128 CPU/GPU worker plans, OpenCL duty behavior, pool templates, payout
  rules, PoW and candidate validation are unchanged.

### GUI, Explorer API and snapshot publication

- GUI snapshots receive a capture sequence while the immutable canonical chain
  view is actually cloned, then recapture a cheap canonical head immediately
  before delivery. Separate full/head lanes prevent either completion order
  from rolling back height, hash, balances or Recent Blocks.
- Quick Recent Blocks snapshots remain available while detached catch-up is
  running.
- The bundled Explorer fetch worker has bounded requests and latest-request-wins
  application, so one hung/late load cannot leave the auto-refresh latch stuck.
- Explorer API avoids replaying the full block journal for a same-tip
  mempool-only state revision and reserves a small status/health connection lane.
- `status-fast` includes additive v1.8.5/HF129 operational fields.
- Production snapshot publication is FCE-only and verifies that exported bytes
  exactly match the reported height/hash identity. Legacy and PREVIOUS recovery
  require explicit operator flags. Lock contention is a visible non-zero result.
- Versioned publisher service/timer units and cache policy are included.

## Consensus and compatibility

HF129 changes no consensus rule or activation height:

- mainnet Epoch 2 remains block **#24000**;
- post-activation block version remains **2**;
- P2P protocol remains **2**;
- Fast Chain Engine format remains **QUB-FCE-1**;
- PoW, DAA/fork choice, rewards, maturity, monetary limits, QNS, JIN, pools,
  checkpoints and transaction/block serialization remain unchanged;
- the HF128 pool coinbase signature bytes remain deliberately
  `/QUB Core v1.8.4/` because those bytes are template compatibility data, not a
  product-version label.

HF129 can be rolled back to the last known-good HF128 binary without chain or
wallet migration. Never roll back or delete `chain-v2`, `wallet.json`,
`node_id`, peer state or durable relay outboxes.

## Required release gates

Do not deploy directly from this source-only archive. The release operator must
first copy the known-good private runtime assets, then pass the complete
`HF129-v185-DEPLOY-RUNBOOK.md`, including:

- locked native Cargo build, formatting and all tests on an approved Rust host;
- focused HF129 unit tests and existing HF123-HF128 regressions;
- real-process regtest, CPU/GPU Pool-pause-resume and manual-Stop tests;
- NYC3 canary before AMS3;
- independent seed identity and exact height/hash convergence;
- fail-closed FCE snapshot publication and public cache-header checks;
- deployed QUB Explorer v0.7.3 and Jinex Wallet gateway hash/provenance plus
  timeout/latest-wins/monotonic soak evidence;
- clean Windows install and HF128-to-HF129 updater smoke;
- at least six hours of post-deploy monotonic cross-client observation before
  the repository release and Telegram announcement.

## Source-only validation performed for this archive

The preparation environment passed the HF129 source contracts, snapshot
publisher self-test, Bash/Python/JavaScript syntax checks, archive/privacy scan
and Rust structural parsing. It did not contain `cargo`, `rustc`, `rustfmt`,
PowerShell or a Windows signing toolchain. Native compile/test and signed Windows
release evidence are therefore mandatory deployment gates, not claimed results.

## Scope boundary

The supplied HF128 archive did not contain the deployed QUB Explorer v0.7.3
source artifact or the Jinex Wallet gateway/client source. HF129 fixes the QUB
Core, bundled legacy Explorer page, Explorer API and shared snapshot publisher
paths present in this archive. The runbook deliberately requires provenance and
black-box freshness tests for the separately deployed Explorer and Jinex
components; it does not claim unreviewed external code was changed.
