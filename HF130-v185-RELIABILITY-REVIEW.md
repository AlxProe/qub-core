# HF130 / v1.8.5 implementation review

Date: 2026-09-09. This is a source review and preparation record, not deployment approval.
The preceding review was `HF130-v185-REVIEW-20260909.md`. HF128 / v1.8.4 is
still the deployed baseline; HF129-r6 was an undeployed input to this candidate.

## Authoritative inputs

| Artifact | Verified SHA256 |
|---|---|
| HF128-v184-source-no-assets-r3.zip | `85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3` |
| HF129-v185-source-no-assets-r6.zip | `85160e0f72ed7308ab05793b4981863d8fcfc6b7ebff61f29cbdd1c68afad33c` |

The new tree was extracted from the exact r6 input, then checked against the
exact deployed HF128 archive. No previous HF129 r1-r5 working tree was reused.

## Findings and implementation

| Finding | HF130 implementation | Evidence still required |
|---|---|---|
| R1: unbounded synchronous pool-share nonce search | One-second batches, cancellation checkpoints every at most 256 hashes, nonce cursor tied to parent, parent validation before accepting a found nonce and fresh chain reload before admission. Standalone Join has a 30-second search budget. | Native batch tests and CPU/GPU mixed-version mining; time spent in storage/driver calls is not hard real-time bounded. |
| R2: socket timeouts renewed by partial reads; abandoned DNS/HTTP work | Inherited absolute deadline and Stop context through real socket chunks, HTTP children and sync stages; DNS pending work coalesced/capped; controller-owned relay children retire before completion. | Loopback drip-feed/silent-peer and nested-budget tests, slow/unavailable peer field tests. |
| R3: failed head recapture given a fresh sequence; mixed head/balance lanes | A failed read retains its original sequence. QUB balances and canonical head are transferred together; capture order permits a valid lower-height reorg. | Native out-of-order snapshot tests and live GUI observation. |
| R4: cleanup/status IO between CURRENT commit and live publication | CURRENT commit returns before derived maintenance; live publication is called first. A coalesced detached worker performs status/scanning outside the canonical mutex, with bounded deletion batches and fresh CURRENT/PREVIOUS protection. Readers pin journal handles and retry capture races. | Native delayed/failing maintenance, reader-lifetime and busy-writer tests; real process/API latency under load. |
| R5: archive checks reject legitimate build/checkout trees; zero-test PASS | Exact source manifest; explicit archive/local-build/checkout modes; manifest-only exporter; local passive assets never exported. Native runner requires its expected registry and explicit test successes. Windows line endings preserved by `.gitattributes`. | Native Windows helper execution, the selected signed/official-origin-unsigned installer policy and Linux staging gates. |
| R6: automatic forced ticks perpetually queue more deep passes | Automatic request reason is separate from manual priority; auto ticks coalesce during a pass and outcomes are delivered for each completed pass. | Native request-queue test and long-running automatic sync observation. |
| R7: ancestor/tie evidence can become false fresh-at-tip proof | Exact height/hash/time/source proof, not just a boolean; pending relay and conflicting observations prevent exact freshness. HF126 tie mineability is distinct from exact quorum agreement. HTTP payloads are paired to the advertised generation. | Native proof/paired-generation tests and mixed HF128/HF130 fork/relay scenarios. |
| R8: failed multi-file publication leaves mixed public generation | Immutable generations selected through one atomic `.current`; migration preserves old bytes; failed alias/pointer replacement retains the previous generation. URLs and JSON schema are preserved. | Full-chain publication timing/disk-capacity, Caddy and actual external-client checks. |
| R9: shared Stop flag makes unexpected exit undetectable | Explicit exit reasons; join actual controller, drain terminal events, then consider same-intent restart. Pool ID and miner address are pinned, and update/Stop state is checked at restart. Child workers are joined during normal retirement/unwind. | Native exit/intent/unwind tests; real OpenCL controller handover and manual Stop smoke. |

## Independent review corrections during implementation

Cross-review found and corrected a GUI declaration/address shadowing problem,
a terminal-event drain race, missing inherited cancellation in relay children,
and the distinction between exact direct-quorum proof and mineable sibling
branches. Deployment review also found source-tree contamination by preflight,
Windows line-ending conversion, incomplete recovery after stopping publisher
units, and rollback requirements for mutable Windows download aliases.
These reviews improve the candidate; they do not replace compilation.

## r6 finite CLI lifetime correction (2026-09-12)

The operator's r5 real-process test exposed an outdated immediate sidecar-file
assertion after init. Source review also found that detached maintenance could
outlive a finite CLI command's main thread. r6 selects inline metadata/cleanup
for finite qubd commands after live publication. Non-fatal maintenance errors
preserve commit success; no detached compatibility export is launched there.
GUI and long-running node/API services retain their background policy. This
retains cleanup during multi-block CLI mining without relying on work after exit.
The expanded real-process regression checks the committed FCE identity and lease
release across repeated writes. A native inline-failure regression was added;
r6 native compilation/execution and end-to-end validation remain pending.

## Compatibility freeze

The deployed `src/pools.rs`, `src/gpu_miner.rs` and all seven `config/*.toml`
files remain byte-identical. The 462 external locked dependency records are
unchanged. `tests/v1_core.rs` has one intentional test adaptation to wait for
actual asynchronous export/status output instead of assuming immediate files.

The pool template compatibility bytes `/QUB Core v1.8.4/`, activation heights,
Epoch 2 at 24000, block version 2 and `QUB-FCE-1` remain unchanged. Public wire
messages and persistent storage structures are preserved. Changes to sync
coordination/proof are operational policy; their mixed-version behavior still
requires the runbook's native and live gates.

## Remaining practical limits

- Pool block workers still wait for a valid local share under the existing
  participation policy. Batching makes this wait cancellable and parent-aware;
  it does not reduce the mathematical share difficulty. Very slow CPUs may
  still prepare shares for a long time. No claim of faster hashes is made.
- A cancelled operation still waits for safe retirement. Kernel/filesystem/GPU
  calls can take longer than cooperative checkpoints. `panic=abort` terminates
  a release process; it does not run Rust unwinding destructors or auto-restart.
- Bounded cleanup can still delay a later writer during a slow filesystem delete;
  that cleanup is no longer on the save's post-CURRENT live-publication path.
- Multiple HTTP requests can straddle an atomic generation switch. Identity/
  checksum checks and retries are necessary; the publisher's atomic pointer is
  not a multi-request transaction for old external clients.
- Full-chain export/scan time and available disk space are not benchmarked here.
  Existing 180-second public-tip freshness requires publication to fit within
  the timer budget; the runbook stops rollout if a cycle exceeds 115 seconds.
- Explorer v0.7.3 r4 and Jinex client/gateway sources are outside this archive.
  Their provenance and actual compatibility are required before rollout.
- Reports from 6-7 September describe symptoms, without node logs/CPU timing or
  peer traces. They support the investigation but do not establish which path
  caused each user's 100+ block stall.

The correct release state is: implemented source candidate; preparation tests
reported separately; native and production readiness not yet established.
