# HF131 / v1.8.5 reliability review

Scope: current source, inherited HF129/HF130 changes and new HF131 fixes against
verified deployed HF128/v1.8.4. The user cancelled manual regtest/LAN deployment
labs; internal tests and real GUI mainnet/portable checks remain mandatory.
No live server changes or public publication occurred in preparation. Full
QUB native execution remains an operator gate; see the current test report.

## r4 preflight correction

The operator's r3 native helper reached its disposable mainnet preflight and
failed the inherited HF128 reserve check at height zero. That check required
JIN before native JIN activation. r4 labels the reserve check not yet applicable
below #5555, retains the actual reserve threshold through #16776, and retains
replayed lifetime bootstrap enforcement from #16777. Read/replay errors fail.
The same corrected CLI handles native-build and shipping-dist preflight.
Only src/main.rs changes among Rust files, for this diagnostic and six tests.
The existing JIN ledger, bootstrap implementation, constants, validation,
mining, wire protocol, Explorer API and storage code are unchanged from r3.
No earlier test is skipped and no manual regtest/LAN exercise is added.

## Baseline and compatibility

HF128-v184-source-no-assets-r3.zip SHA256:
85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3

HF130 r7 parent source ZIP SHA256:
aa809f650e2325e3aafa1dfd2d2f2104ff95723910bba69ecd3e6d99c625e74c

HF130 r6 passed the user's Windows native/preflight gates. HF130 r7 did not
complete their source-directory swap, so no r7 native pass is claimed. HF131
requires its own native gate; an earlier version's PASS cannot certify this code.

The policy pins the HF128 pool implementation, seven config files, GPU kernel,
nonce scanner/tuning helpers and exact P2P WireMessage definition including
serde attributes. Activation values, protocol 2 and QUB-FCE-1 stay fixed. The
GPU file now differs only for ownership/cleanup and its tests; its old whole-file
freeze was replaced with precise unchanged mining-code invariants. Dependency
lock changes are checked against HF128, excluding the local package version.

Explorer 0.7.3 r4 was inspected from its exact source ZIP (SHA256
8eb8ac79c503b564c1a2d49152840ac03c85b4fff5e7a1cfa18565d694dfe749).
Fourteen chain/API serializers are byte-identical to HF128. The deployed static
Explorer is retained; the older explorer/public tree in Core is not published.
Jinex Wallet is not deployed and is not a release dependency.

## Findings and changes

| Area | Failure path | HF131 change |
|---|---|---|
| CPU/GPU solo and pool | Worker sets round_stop before channel send; controller can exit without receiving the valid block | Join all workers, recover any completed candidate, honor manual Stop, retain final parent checks |
| GUI sync/mining | Stale or self-labelled official peer telemetry feeds a mining veto | Display observations require freshness/reachability; controller's actual P2P evidence governs mining |
| Seed availability | Serial seed/HTTP probes exhaust deadline before direct peers are sampled | Bounded joined observation lanes, independent node IDs, responsive-peer ordering and early ordinary sync |
| Block delivery | Explicit ACK after suffix/overlap repair not counted as delivery | Count repaired explicit acknowledgements, reject legacy/error/duplicate evidence |
| Mining payout | Advertised QNS input never resolves in controller | Resolve locally, pin across retries/restarts, reject invalid or unowned pool payouts permanently |
| OpenCL | Partial constructor errors and repeated DLL loads leak resources | Reverse-order ownership guards and shared library lifetime; no kernel/dispatch change |
| Storage lease | Initial write/flush failure strands canonical WRITE.lock | Own lease before fallible initialization and close handle before cleanup |
| RPC transactions | Failed save leaves live mempool modified and retry skips persistence | Clone, validate, persist and publish only on success |
| RPC blocks | Relay/outbox error becomes an empty apparently non-pending report | Keep durable acceptance, report relay error and retry persistence as unknown |
| Source preparation | Renaming an in-use old source directory fails | Separate immutable-version directory; cache remains in its existing location |
| Build/dist | Repeat full tests/builds; delete old outputs/dist | Hash-bound native receipt, fresh dist directory and verification before public upload |
| Installer | Existing customized configs overwritten on upgrade | Preserve existing config files; mainnet recipe default and current release notes |
| Seed/public release | Partial copies and listener ambiguity weaken recovery | Atomic local replacements, PID-owned listener checks, explicit backups and manifest-last publication/recovery |

Independent cross-review covered GUI candidate handoff/restart intent, GPU ABI
and ownership, P2P scoped probes/deadlines, and preflight data isolation. It caught
and corrected per-peer chain reloads in the initial parallel-probe draft and a
stale local capture after an early successful sync. No new external dependency
was added.

Retained HF130 fixes include bounded/cancellable pool-share search, absolute
P2P framing deadlines, DNS work limits, coherent GUI snapshot ordering, queued
catch-up coalescing, durable canonical publication ahead of background cleanup,
and snapshot generation pairing. These remain covered by the exact registry.

## Limits and operational interpretation

Seed downtime must not consume all time before healthy peers can answer. The
existing independent evidence threshold remains: one official acknowledgement
or two distinct direct peers as applicable. One isolated miner, one untrusted
peer or no reachable peer is not proof of a network tip. This release does not
promise safe continued mining through complete network isolation.

Pool share preparation still has the existing mainnet share target and serial
share hashing before block work. Cancellation is bounded, but this does not
promise uninterrupted GPU hashrate or a share/block within a fixed duration.

The new OpenCL lifetime paths need real Windows driver Start/Stop observations.
A small preflight decision harness is compiled during r4 preparation; the full
QUB binaries are not compiled here. Static review, fixture PASS and API compatibility checks do not replace the user's mandatory native
build and GUI mainnet smoke. There is no claim that every latent bug is removed.
The shorter release flow trades the old long manual lab/soak for focused internal
regressions, observed mainnet/portable checks and continued post-release monitoring.

The source archive has no license document; Cargo declares MIT. Existing
repository license/governance files are preserved by the repository procedure.
No copyright holder or replacement license was invented during this review.
