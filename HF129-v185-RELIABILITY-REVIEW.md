# HF129 / QUB Core v1.8.5 Reliability Review

Review date: 2026-08-13  
Baseline: HF128 v1.8.4 source-no-assets r3  
Scope: storage publication, P2P catch-up, GUI mining/snapshot state, Explorer
API, bundled Explorer fetcher, snapshot publisher and release operations

## Executive decision

HF129 is suitable for native build and staged canary validation. It is not
authorized for production until every Gate 0–8 item in the deployment runbook
passes. The code intentionally avoids consensus, PoW, DAA, fork-choice, payout,
pool-template and chain-format changes.

The most credible periodic liveness cause was a full legacy-chain export on the
canonical storage commit path. The highest-confidence GUI causes were false
completion on P2P writer contention, unconditional catch-up epoch resets,
snapshot suppression for the entire detached repair lifetime, out-of-order
snapshot application and untyped mining restart state.

## Root-cause evidence and treatment

| Observation | HF128 failure mode | HF129 treatment |
|---|---|---|
| Tip/API/UI can pause every few hours | Every 256 blocks or six hours, a full `chain.json` replay/serialize/fsync ran synchronously immediately after `CURRENT` | Full compatibility export is detached, coalesced and starts only after live canonical publication |
| Durable disk can be ahead of live owner | Cleanup, status or export could fail after the atomic `CURRENT` rename; the caller received an error and skipped live publication | All post-commit maintenance is non-fatal telemetry; `save_chain` publishes the committed candidate immediately |
| Manual Sync often works instantly after auto appears stuck | Lower P2P writer used `try_lock`; a loser returned a normal local report and the GUI discarded the real outcome | Manual/mining/deep requests wait at the real writer; reports are typed and propagated |
| Auto can repeat the same blocks for an hour | Every generic worker epoch reset the GUI stall timer and lost the observed higher target | Only progress/fresh-at-tip resets; higher target is retained across no-progress/busy/failure |
| Sync duration exceeds its advertised bound | Sequential URLs/addresses and rescue stages could each receive fresh or raised timeouts | One absolute deadline is carried through stages and remaining budgets never increase |
| Recent Blocks can freeze while mining at tip | GUI suppressed all snapshot refreshes for the 240/420-second catch-up lifetime | Quick live-owner snapshots continue during detached repair |
| Recent Blocks/height can roll backward | Background and wallet workers applied results unconditionally; an older expensive result could finish last | Full capture and final cheap head capture have separate monotonic lanes; every arrival ordering preserves the newest visible head |
| Pool mining resumes as Solo | Empty string doubled as Solo mode; generic timer had no restart cause or controller generation | Typed `Solo`/`Pool(pool_id)` intent, exact mode on handle, cause and retiring generation on restart plan |
| Explorer auto-refresh freezes | One fetch had no abort timeout; global `LOADING` blocked the 30-second timer forever | Abort deadlines and latest-request generation guard |
| Explorer API overload amplifies lag | Same-tip mempool revisions invalidated full-chain cache; 32 occupied workers also rejected status | Small mempool overlay refresh and eight reserved status/health slots |
| Explorer and Wallet share a stale snapshot | Publisher silently fell back to infrequently refreshed legacy `chain.json`; timer definition was not versioned | FCE-only default, exact export identity, visible lock failure, versioned unit/timer and source metadata |
| Frozen HTTP tip can look trusted | `published_at_unix` was parsed but discarded as liveness evidence | Missing/stale/future timestamp is rejected for target/green-light use |

## Safety invariants

### Consensus boundary

The review verified that HF129 does not intentionally modify:

- mainnet activation heights or block-version rules;
- genesis, checkpoints, hash serialization, Merkle construction or PoW;
- DAA, cumulative-work comparison or fork choice;
- coinbase reward, maturity or money-supply limits;
- QNS, JIN, infusion/melt, Library or governance consensus;
- pool markers, share validation, PPLNS split or HF128 compatibility modes;
- Fast Chain Engine schema/magic.

The `/QUB Core v1.8.4/` pool coinbase byte string is deliberately retained. A
blind product-version replacement there would alter block-template bytes.

### Storage commit invariant

The atomic `CURRENT.json` rename remains the durable commit point. Before it,
all failures remain fatal. After it:

1. cleanup/status/export failures are warnings and counters;
2. the exact committed candidate is published to the in-process live owner;
3. compatibility export cannot take the canonical storage mutex or writer lease;
4. at most one exporter per data directory/process runs, and cross-process work
   is mutually excluded/best-effort through `LEGACY-EXPORT.lock`;
5. Explorer/API readers use the live owner or an immutable FCE identity.

Fault injection for each post-commit side effect is a mandatory native Gate 1
test. It must show disk identity equals live identity equals API identity while
the injected side effect fails or sleeps.

### Sync invariant

Only one state-changing catch-up writer runs in a process, including nested
public workflows and their low-level mutators. Same-thread nesting is safely
reentrant; background work never queues behind itself. Priority requests cannot
be acknowledged until they have either acquired the real writer or reached
their absolute deadline. A report is not fresh merely because it ended or an
arbitrary peer is lower: it requires explicit canonical official/quorum proof.
Every observed target keeps height and hash paired.

Consensus/checkpoint validation remains below the orchestrator and is required
before every adopted tail, snapshot, suffix or direct chain is saved.

### Mining invariant

Restart authorization is the conjunction of:

- user intent is still `Run(mode)`;
- no manual Stop or pending update is authoritative;
- retiring generation matches the plan;
- retiring handle mode matches the plan mode;
- a rejected generation is never restarted;
- Pool mode contains the same non-empty Pool ID;
- canonical catch-up is no longer known to be ahead.

There is no conversion from invalid/empty Pool mode to Solo.

### GUI snapshot invariant

Request-start order and completion order are not trusted. The full anti-rollback
key is assigned while the immutable canonical chain clone is serialized by the
short GUI capture gate. Wallet/activity derivation happens after that gate is
released, then a cheap head/Recent-Blocks view is recaptured under the same gate
with its own sequence. A newer full result carrying an older head preserves the
newer visible head; a newer head carrying older derived data updates only that
head lane. Legitimate verified re-anchors remain representable because ordering
is not a height-only comparison.

## Operational hardening

- Snapshot publishing rejects legacy source by default.
- PREVIOUS recovery is rejected unless explicitly authorized for a controlled
  recovery.
- Published bytes must equal the exporter-reported height/hash.
- `tip.json` records `source`, generation, state revision, commit time and
  recovery state, and is atomically installed last.
- Publisher lock contention exits 75 so monitoring cannot record a green skip.
- The service is a low-priority, hardened oneshot with a 20-minute bound; the
  timer schedules from unit inactivity so runs do not overlap.
- Core accepts public tip metadata as liveness evidence for 180 seconds. With a
  60-second inactive interval and five-second timer jitter, the repeated-cycle
  publisher duration must stay below approximately 115 seconds; slower cycles
  are a deployment blocker even though the service timeout is longer.
- `tip.json` is no-store; tails are short-lived/revalidated; bulk files retain a
  small public cache.
- Explorer API root and `status-fast` are separate version/health gates. The
  previous runbook incorrectly expected `version` from an older status schema.

## Known scope limitations

The archive contains a bundled snapshot-driven Explorer page, but not the
deployed QUB Explorer v0.7.3 source/release artifact. It contains no Jinex Wallet
gateway/client source. Therefore:

- HF129 cannot make a source-level claim about those separately deployed builds;
- their hashes, source/release provenance and configuration are deployment
  inputs;
- the additive HF129 keys in `tip.json` must be parsed by the deployed Jinex
  gateway/client before production; strict exact-key parsing is unreviewed;
- timeout, latest-wins, monotonic tip and six-hour soak checks are release gates;
- a joint Explorer/Jinex freeze with moving seeds must be investigated first in
  the shared AMS3 publisher/API/gateway path.

## Validation completed in the preparation environment

- attached HF128 ZIP integrity and clean extraction;
- HF129 source-contract test;
- snapshot publisher fail-closed self-test, including lock contention, exact
  FCE identity, PREVIOUS refusal, legacy refusal and epoch-version rejection;
- Bash and Python syntax checks;
- bundled Explorer worker/main JavaScript syntax checks;
- structural Rust parsing against the HF128 baseline, with no new parse errors;
- source-only/no-assets and private/runtime path scan.

Exact preparation commands and limitations are recorded in
`HF129-v185-TEST-REPORT.md`.

Native Rust compilation, unit/integration tests, PowerShell release build,
Authenticode signing and real network soak were not possible in the preparation
environment. Their absence is explicit and blocks deployment until the runbook
records passing evidence.

## Residual risks and gates

| Risk | Control / release gate |
|---|---|
| Rust type/borrow issue not visible to structural parsing | `cargo check --locked --all-targets` and `cargo test --locked --all-targets` on two approved hosts |
| Mining controller timing differs on real OpenCL driver | CPU-only and CPU+GPU Pool pause/resume/Stop integration matrix |
| In-process detached compatibility export still consumes seed I/O | six-hour soak across a 256-block/6h trigger, seed commit latency/storage counters and no block-gap regression; publisher service scheduling does not lower this thread's priority |
| Full public publisher cycle exceeds HTTP-tip freshness window | repeated-cycle duration below approximately 115s and public tip age always below 180s |
| Public CDN overrides origin cache policy | origin and public header checks including `Age`/`CF-Cache-Status`, repeated nonce queries |
| External Explorer/Jinex has its own latch | deployed-hash provenance and black-box frozen-response/late-response tests |
| Two seeds diverge or share identity | distinct `node_id`, independent regional config, exact cumulative-work tip comparison before AMS3 and release |

## Go / no-go rule

GO only when every mandatory gate is recorded with command output, timestamp,
host and artifact SHA256. Any compile/test failure, unexpected consensus diff,
Pool-to-Solo event, stale public FCE source, repeated cross-client freeze,
same seed identity, chain divergence, unsigned/mismatched installer or rollback
failure is an immediate NO-GO.
