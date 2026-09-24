# HF129 / QUB Core v1.8.5 Preparation Test Report

Report date: 2026-08-25  
Input: `HF128-v184-source-no-assets-r3(1).zip`  
Input SHA256: `85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3`

Report revision: r6 platform-gate correction. All earlier preparation ZIPs are
withdrawn and must not be deployed:

- r1 `d9782e26091bc7858eaa588b959de816947a685f31ec6630247095e4935449ea`
  contained invented local/repository/key paths and obsolete Explorer
  provenance;
- r2 `64f6d654e28044113d1e8fa9292601c242230e0c22d40b87d89b076e85ae5f16`
  corrected those paths, but its cache-busting filename and checksum filename
  did not match Gate 0. Individually pasted interactive commands could also
  continue after a `throw` and print a false PASS;
- r3 `9b1c82a98b8f7a2b129caa290d73b9cb61e9141bb73defd49f7d40df9d49ea35`
  corrected artifact naming and fail-stop execution, but Gate 1 incorrectly
  applied the strict no-assets package contract after Gate 0 had restored the
  approved private GUI runtime assets into the local Windows build tree;
- r4 `d29d5e47e90796e9dcba4902db61d3384f7eab2c8da98f5bf8976f8d80e4956b`
  separated strict and local contract modes, but the local mode used an
  incomplete extension allow-list and rejected an existing HF128 MP4 runtime
  animation;
- r5 `12672d057d1c47d05964f0bce6474d3abba46ce67cf36065ecbbe08c50768870`
  fixed local asset handling and strict extraction checks, but its Windows
  native-build gate invoked the Linux Bash publisher test. A Windows WSL relay
  without an installed distribution therefore stopped before Cargo.

The HF129 Rust/source-code changes are unchanged by r6. Gate 0 executes the
strict no-assets contract against the untouched extraction before copying any
private asset. Server staging and the public repository also remain strict.
The Windows build mode accepts regular files only inside its top-level local
`assets/` tree without guessing passive media extensions, while continuing to
reject runtime state, secrets, keys, executable/script/archive material,
symlinks, special files and public Explorer assets. The publisher shell test is
mandatory on both exact Linux seed staging trees in Gate 4 and is not a Windows
or WSL prerequisite.

## Decision

The HF129 source tree passed every validation executable in the preparation
environment. It is ready for the mandatory native build and staged release
gates in `HF129-v185-DEPLOY-RUNBOOK.md`; it is **not production-authorized** by
this report alone.

The preparation environment did not provide Cargo, rustc, rustfmt, PowerShell,
the Windows signing toolchain, production data, seed access, deployed QUB
Explorer v0.7.3 source, or Jinex Wallet gateway/client source. Consequently,
native compile/tests, Windows artifacts, server canaries, external-client tests
and the six-hour production soak remain explicit NO-GO gates.

## Completed preparation checks

| Check | Result | Evidence summary |
|---|---:|---|
| Attached HF128 archive identity/integrity | PASS | SHA256 matched the value above; ZIP extraction completed with one expected source root |
| HF129 source contracts | PASS | version, commit/live-publication order, sync coordinator/proof, paired tip identity, mining mode/generation, two-lane GUI ordering, publisher units, strict no-assets and bounded local-runtime-assets contracts |
| Local GUI asset compatibility | PASS | local mode accepted `.gitkeep`, PNG icons including `jin-token.png`, MP3 sounds, MP4 animations including `mining-on-muted-white.mp4`, nested flag PNGs, the optional TTF font and a passive future asset extension |
| Contract-mode negative tests | PASS | strict mode rejected local runtime assets; local mode still rejected public Explorer assets, runtime-state names, sensitive text, private-key/executable/script/archive extensions and symlinks |
| Snapshot publisher self-test | PASS | legacy refusal, non-zero lock contention, valid FCE generation, exact export identity, PREVIOUS refusal and epoch-version rejection |
| Bash syntax | PASS | every shipped `deploy/` and `scripts/` shell script parsed with `bash -n` |
| Python syntax | PASS | every shipped Python script parsed with Python 3.12 AST, without creating bytecode |
| Bundled Explorer JavaScript syntax | PASS | both inline worker and main scripts parsed by Node.js 24 |
| TOML/JSON syntax | PASS | all shipped TOML and JSON files parsed with the Python standard-library parsers |
| Rust structural parse comparison | PASS | HF129 produced no new parser errors or missing nodes versus the supplied HF128 baseline |
| Protected mining/config bytes | PASS | GPU miner, pools, core regression fixture/test, mainnet configs and both regional seed configs are byte-identical to HF128 |
| Source hygiene | PASS | no merge markers or trailing whitespace in source-code file types |
| Operator path contract | PASS | restored `C:\Users\proes\Desktop\qub-node`, `qub-core-opensource`, `%USERPROFILE%\.ssh\jinex_ed25519` and `/opt/jinex/staging/src/qubd`; forbidden invented paths are source-contract failures |
| Operator execution contract | PASS | every PowerShell fence is one fail-stop script block; NYC3/AMS3 Bash runs non-interactively; focused/native regressions are checked individually |
| Platform-scoped validation contract | PASS | Windows Gate 1 contains no Bash/WSL command; the exact staged source executes the publisher self-test under Bash on both Linux seed hosts in Gate 4 |
| Windows console encoding contract | PASS | deployment runbook is ASCII-only, avoiding Windows PowerShell 5.1 UTF-8/no-BOM mojibake |
| PowerShell structural parse comparison | PASS | r6 has the same 15 parser `ERROR` / 0 missing nodes as r5; all are the existing grammar limitation around native `--` arguments, with no new structural errors |
| Explorer provenance | PASS | production gate targets v0.7.3 r4 SHA256 `8eb8ac79...fe749`; obsolete r3 is explicitly rejected |
| Source-only/privacy/archive contract | PASS | one `qubd-v1.8.5/` root, no symlinks/special files/runtime secrets/caches/binaries, and only the two approved asset README placeholders |

Structural Rust parser counts (`ERROR` / missing nodes):

| File | HF128 | HF129 |
|---|---:|---:|
| `src/p2p.rs` | 16 / 0 | 16 / 0 |
| `src/fast_storage.rs` | 3 / 0 | 3 / 0 |
| `src/lib.rs` | 8 / 0 | 8 / 0 |
| `src/main.rs` | 0 / 0 | 0 / 0 |
| `src/bin/qub_core.rs` | 10 / 0 | 10 / 0 |
| `src/bin/qub_rpc_miner.rs` | 0 / 0 | 0 / 0 |
| `src/rpc.rs` | 0 / 0 | 0 / 0 |

The non-zero counts are pre-existing grammar/parser limitations in the HF128
source and are identical in HF129. Structural parsing is only a regression aid;
it does not substitute for Rust name resolution, type checking, borrow checking,
linking or tests.

## Mandatory unexecuted release evidence

Before deployment, record all of the following exactly as required by the
runbook:

- `cargo fmt --all -- --check`;
- `cargo check --locked --all-targets` and `cargo test --locked --all-targets`;
- every focused HF129 test and HF123-HF127 real-process regression;
- post-`CURRENT` failure/latency injection;
- CPU-only and CPU+GPU Pool/Sync/Stop controller matrix;
- reproducible Linux seed builds and signed Windows clean-install/update tests;
- NYC3 canary before AMS3, distinct seed identities and exact canonical tip
  convergence;
- repeated FCE publisher cycles below approximately 115 seconds;
- deployed Explorer v0.7.3 and Jinex provenance/compatibility/fault tests;
- at least six hours of monotonic seed/API/Explorer/Jinex/Core observation.

Any failed or missing item keeps the release at NO-GO. Final artifact hashes are
recorded separately in `HF129-v185-r6-SHA256SUMS.txt` so this report can remain
inside the hashed source archive without a circular checksum dependency.
