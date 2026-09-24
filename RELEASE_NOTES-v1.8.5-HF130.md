# QUB Core v1.8.5 / HF130 release notes

Candidate date: 2026-09-09. Deployed baseline: HF128 / v1.8.4.
HF129 was not deployed. HF130 is a reliability release with no new consensus activation.

## Pool mining and synchronization

Pool share preparation previously searched synchronously until a valid nonce
was found, without responding to Stop or a changed chain parent. HF130 uses
bounded batches, checks cancellation during each batch, verifies the parent,
and reuses progress only for that parent. Final admission uses a fresh chain
view. CPU/GPU block hashing plans and pool share validation are retained.

Sync deadlines now reach socket IO and HTTP child processes. A peer sending
partial data cannot renew the whole operation forever. DNS work is coalesced
and capped instead of creating unbounded abandoned resolvers. Scoped workers
inherit cancellation/deadlines and retire before ownership is released. This
bounds cooperative waits; it is not a hard real-time guarantee against an
unresponsive kernel, filesystem or GPU driver.

Fresh-at-tip evidence is tied to the observed height, hash and time. An older
official ancestor, pending local block relay, or a hash belonging to a different
height cannot become an exact-current acknowledgement. Consensus validation
and the existing equal-work branch policy remain in force.

Automatic deep catch-up no longer continually queues manual repair passes.
Completed passes publish their own outcomes. Manual repair retains its priority
and Stop cancels a mining resume. Resume state pins Pool ID and payout address;
the GUI waits for controller and child-worker completion before starting
another generation. Explicit exit reasons distinguish requested cancellation,
rejected plans and unexpected exits.

## Chain, GUI and publication

A failed final GUI head recapture keeps its earlier sequence, so late results
cannot overtake a newer reader using an old chain. Head identity and QUB balances
are applied together, including valid lower-height reorganizations.

Fast Chain Engine CURRENT commit returns before cleanup/status maintenance.
The live chain is published first; maintenance runs after that publication,
coalesces, rechecks live pointers and yields to active writers. Cleanup/status
errors do not turn a durable commit into a reported failed block installation.
The legacy full-chain export stays detached from the commit path.

Finite daemon CLI commands complete their metadata/cleanup pass after live
publication before returning, with maintenance errors remaining non-fatal.
They use explicit full-chain exports when required. Desktop Core and persistent
node/API services keep background maintenance and compatibility exports.

The FCE-only snapshot publisher verifies exported height/hash and commits a
complete immutable generation with one atomic pointer change. Existing public
URLs and JSON schemas remain available. Initial migration preserves old public
bytes, including on injected alias replacement failures. HTTP requests across a
successful switch can observe different generations; consumers must check
identity/checksums and retry within their budget. A symlink switch does not make
several separate HTTP requests transactional.

The HF129 bundled Explorer/API freshness work is retained. The separately
deployed Explorer v0.7.3 r4 and Jinex Wallet sources were not included; their
live compatibility/freshness checks remain explicit deployment requirements.

## Packaging and deployment

HF130 adds an exact SHA256 source inventory and separate archive/local-build/
checkout modes. Local builds accept passive existing runtime assets, including
legacy MP4 and `.gitkeep`; source exports contain only manifest-owned files.
Target/dist/Git metadata are excluded only in workspace modes and never enter
a source archive. Modified/unexpected source files fail the gate. This scanner
is a packaging control, not a claim to detect every conceivable secret hidden
in arbitrary media.

The Windows build gate uses native Windows tools. Publisher tests run on Linux.
Deployment blocks stop on native failures. A focused test runner checks its
expected test registry and explicit passing results, rejecting zero-test or
ignored-test successes. The runbook covers NYC3 before AMS3, distinct node
identities, retained rollback binaries, publisher migration, verified Windows dist
with the update manifest published last, repository publication and soak gates.

## Compatibility and validation limits

Mainnet Epoch 2 remains **24000**, post-activation block version **2**, P2P
protocol **2**, and storage format **QUB-FCE-1**. HF128 pool/GPU source and all
seven config TOMLs are unchanged. External locked dependencies are unchanged.
Keep `/QUB Core v1.8.4/` inside the pool template; it is not an app version label.

This preparation environment has no Rust compiler, rustfmt, PowerShell,
Windows signing toolchain, GPU or live seed access. Native builds, Rust tests,
real-process regressions, mixed HF128/HF130 mining, signing and the six-hour
production soak are not claimed as completed. See the test report and runbook.
Community reports provide motivation and hypotheses; without affected-node
telemetry, this candidate does not prove all reported stalls share one cause.

Rollback must preserve wallet, chain, regional identity and relay state. Keep
the verified HF130 snapshot-export binary and generation publisher when rolling
only the daemon back to HF128: HF128's exporter report lacks required provenance
fields. Follow the runbook instead of restoring flat snapshot files over a
managed generation layout.

Existing repository license/notice/community files and `.github/` workflows
are preserved as checkout governance. Unlisted governance does not enter the
source export; manifest-owned files retain their exact byte checks.
