# HF132 / QUB Core v1.8.5 test report

## Canary memory revision r1 — native verification pending

This candidate was prepared after the 2026-09-15 NYC3 pause report. The native
PASS results below belong to the original HF132 baseline, NOT to this revision.
The original source ZIP SHA256 is
9a6d5bfac8adc7ed13e5147dcdcd01eb57389d30a36f479fb80d21d967d77a62.

This revision releases persistent peer handshake snapshots, coordinates inbound
Chain adoption with the existing sync writer, avoids full replay for identical
overlap extensions, reuses the validated candidate in checkpoint repair, and
streams protocol-2 JSON through a bounded write buffer. Seven additional native
regressions are registered. No new consensus activation, block validity rule,
wire schema, peer block-window count, GPU kernel or Cargo.lock was changed.

Rust compilation and these new tests have NOT been executed in the preparation
environment: no Rust toolchain is available, and the standard download endpoints
were unreachable. The patch bundle is not a release-certified source archive.
Its runner invokes the existing Rust parser/package gate and then the complete
native Windows build/test workflow. A new receipt and a new live canary are
required. Existing HF132 receipts and the old Gate 4 installer are not evidence
or installation instructions for this candidate. Never deploy it on that basis.

## Original baseline report (historical evidence only)

This is source preparation evidence, not a Windows release receipt or a public
mainnet deployment certificate. The exact source ZIP is pinned by the external
PREPARE script and SHA256SUMS. No production seeds, wallets, Explorer, repository
or public release channel were changed during this preparation.

## Baselines

- HF131 r4 source ZIP: d26ad61009be5e51d99b606f3e761b400abcfe0243658baa316d5f6d1a9d379e
- Deployed HF128 / v1.8.4 source ZIP: 85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3
- Product version remains 1.8.5. No new activation or storage/wire schema.
- Cargo.lock is unchanged from HF131 r4. Official Rust/Cargo 1.98.1 was used
  for the Linux native preparation checks with the locked project dependencies.

## Executed native tests

| Command / scope | Result |
|---|---|
| cargo check --locked --all-targets | PASS |
| cargo test --locked --all-targets, default parallel test execution | PASS: 158 tests, zero failed or ignored |
| Library suite | 62 passed |
| GUI binary suite | 41 passed |
| RPC miner binary suite | 4 passed |
| Daemon/preflight suite | 6 passed |
| Integration suite | 45 passed |
| Exact shipped HF130/HF131/HF132 regression registry, scope all | PASS: all 78 required names executed |
| cargo build --locked --bin qubd | PASS: actual Linux daemon built |

The 78 registry tests are a subset of the complete suite, not 78 additional
unique cases. They include all 58 retained HF130/HF131 identities and 20 new
HF132 tests (7 P2P, 6 storage, 1 RPC, 6 GUI). Missing, ignored or zero executed
required names fail the registry runner. Full-suite source hashes were stable
while execution ran and are checked against the final packaged Rust files,
Cargo.toml, Cargo.lock and registry. Existing compiler warnings were not hidden;
no automatic cargo fix or whole-tree formatting was applied.

The new tests cover stalled-then-healthy peer recovery, moving tips and durable
successive suffixes, duplicate chunks, canceled versus socket-expired work,
stale reconciliation identity, independent mining Stop while a sync writer is
held, storage/read lock contention, validated baseline trust, genuine-fork local
transaction recovery and remote mempool exclusion, immutable RPC reads, shared
GUI reader admission, panic/error delivery, and real GUI Solo/Pool Sync/Stop
callbacks with both completion timers. They do not invoke real GPU hardware.

## Mainnet preflight boundary

The freshly built daemon was run with the real mainnet configuration in a new
temporary working directory. Genesis replay, economic/activation/checkpoint
checks and pre-JIN reserve applicability passed. The command failed only
`mainnet_seed_domains_resolve` in this execution environment. That is an
ENVIRONMENT-LIMITED result, not a complete preflight PASS and not evidence of a
global seed/DNS outage. No check was bypassed. Windows Gate 1 and dist packaging
still require the complete preflight to pass on the operator's network.

Genesis reserve applicability is exercised by the actual daemon's native tests:
before #5555 no JIN protocol reserve is required; from #5555 through #16776 the
actual reserve is required; from #16777 actual bootstrap history is required.
No historical ledger or activation was modified to satisfy this diagnostic.

## Release tooling and compatibility

| Scope | Result |
|---|---|
| Package policy fixtures | 26 passed |
| Source preparation/assets fixtures | 8 passed |
| Native build/evidence fixtures | 15 passed |
| Mainnet smoke helper fixtures | 23 passed |
| Public state verification fixtures | 22 passed |
| Deployment/rollback fixtures | 30 passed |
| Helper fixture total | 124 passed |
| Snapshot publisher isolated self-test | PASS |
| Python parser checks | 28 helper files; embedded Python also parsed |
| Bash parser checks | 14 shell files; 12 embedded Bash blocks |
| Runbook fail-stop policy | 24 PowerShell blocks, ASCII-safe |
| Actual PowerShell parser / Windows execution | NOT RUN here; PREPARE parses scripts and runbook on Windows |
| HF128 frozen pool/config/GPU-work/wire policy | PASS |
| Critical block/transaction/FCE/wire declarations | 13 unchanged against both baselines |
| Explorer API route/serialization functions | 14 unchanged against deployed HF128 |

Fixtures use isolated temporary directories and mocked service boundaries.
They are not evidence that a production service was started or upgraded.
The source package pipeline additionally requires read-only Rust parsing,
two identical deterministic ZIP builds, re-extraction/CRC/inventory checks,
exact PREPARE/sums/runbook naming, and real prepare/resume/assets/source-export
checks before releasing the kit. No runtime assets, chain state, wallet keys,
compiled binaries or local build caches are included in the source archive.

## Remaining operator gates

1. Windows/MSVC Gate 1: complete native tests, required regression registry,
   actual release executables and full disposable mainnet preflight.
2. Isolated GUI mainnet smoke: automatic/manual catch-up with Solo/Pool Start,
   pending Stop, actual moving-tip convergence by height AND full hash, CPU/GPU
   work and power controls. The closed HF131 smoke copy may supply history to a
   NEW checked HF132 private smoke copy. Unobserved cases remain NOT OBSERVED.
3. NYC3 canary then AMS3: host-native build/tests, seed/API checks, public RPC
   boundary, mixed-version tip/delivery checks and coherent snapshot publication.
   Explorer 0.7.3 static assets remain in place.
4. Dist packaging from verified native evidence, a copied portable mainnet smoke,
   then public publication, updater verification, repository/tag and announcement.

No manual regtest/LAN lab, fixed 150-block exercise or multi-hour soak is added.
No Windows/OpenCL driver result, present mainnet catch-up duration, reproduction
of the user's 1,921-second reader, or production deployment is claimed here.
No hard replay/catch-up duration or hashrate guarantee is made. Cold authoritative
validation and independent derived-history scans still scale with chain size.

The Telegram text is a draft. Publish it only after the runbook's release gates
establish that the linked HF132 binaries and release page are actually public.
