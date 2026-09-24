# HF131 r4 / QUB Core v1.8.5 test report

## Correction and scope

The operator's r3 Gate 1 reached the disposable mainnet preflight at height 0.
Only `mainnet_qub_jin_bootstrap_protocol_reserve` failed in the supplied output.
The helper runs the locked native suite, exact registry and release builds
before that command, so reaching it indicates those preceding commands returned
success. This inference is from the helper's fail-stop sequence; a completed
r3 native receipt was not produced and no r4 native PASS is inferred.

The reserve predicate was inherited from deployed HF128. It incorrectly
required the HF116 reserve even before native JIN existed. The actual ledger
correctly returns empty below mainnet JIN activation #5555. The same CLI
preflight is called later by distribution packaging, so fixing only the first
call site or accepting a failed command would leave the release broken.

r4 corrects the CLI check at its source:

| Chain height | Required evidence |
|---|---|
| Below #5555, including genesis | `ok: true, applicable: false`; explicitly not yet applicable because native JIN has not activated |
| #5555 through #16776 | Actual protocol ledger contains at least 42,000,000 JIN |
| #16777 onward | Actual infusion replay records at least the required bootstrap in lifetime infused JIN |

Applicable read/replay errors or insufficient balances still fail. Invalid
activation ordering or a zero required amount also fails. The existing chain
replay, economic/configuration, activation, reserve-lock and activation-not-missed
checks remain in place. The JSON check retains its name, ok and detail fields
and adds an explicit applicability field.

Only src/main.rs changes among Rust files: this CLI diagnostic, its helpers and
six internal tests. All other Rust, Cargo.toml, Cargo.lock, configuration files,
mining, ledger, bootstrap and consensus implementation are byte-identical to r3.
No native regression is skipped, ignored or removed. The r3 GUI fixture remains.

## Executed preparation checks

| Check | Result and scope |
|---|---|
| Preflight decision tests | 5 passed in a compiled Linux Rust harness using the exact shipped decision function and tests |
| Activation boundaries | #0, #5554, #5555, #16666, #16776, #16777 and a post-activation height covered; exact required amount and one unit below covered |
| State errors and invalid inputs | Applicable history errors, invalid activation ordering and zero requirement rejected |
| Negative decision mutation | Removing the pre-JIN branch makes the unchanged tests fail |
| Whole-source Rust parsing | All 11 Rust files parsed read-only, including the QUB-linked genesis test; source hashes unchanged |
| Source/privacy/runbook policy fixtures | 26 passed |
| Preparation/asset-copy recovery fixtures | 8 passed |
| Native receipt/cache/PE fixtures | 15 passed; simulated binaries, not QUB execution |
| Exact runner failure fixtures | 5 passed; simulated Cargo responses, including missing/ignored/zero tests and nonzero exit |
| Compatibility/source invariants | HF128 frozen configs, pool/mining slices and wire enum retained; 462 external locked dependencies unchanged |
| Scope comparison | Replacing the reviewed preflight branch/helpers/tests with r3 bytes reproduces the entire old src/main.rs exactly, including Explorer handlers |
| Python/Bash/runbook checks | Python AST, Bash and embedded Python/Bash syntax, ASCII and fail-stop structures passed |
| Registered native regressions | 58 exact HF130/HF131 names: 52 retained plus 6 daemon preflight tests; full execution pending in Gate 1 |
| Final source archive | Two deterministic builds, exact re-extraction, manifest and source-only export verified; external SHA256 sums bind the files |

The small compiled harness copies the decision function and five test bodies
from the actual source. It uses a standard boxed-error Result alias in place of
anyhow; it does not link QUB, its ledger, TOML or JSON serialization. A sixth
native test in src/main.rs constructs the real mainnet genesis, validates it,
confirms its JIN ledger stays empty, and calls the actual production wrapper.
That QUB-linked integration test remains mandatory in both all-target and seed
native suites; it has not been executed in this preparation environment.

Rust 1.90.0 compiler/parser/standard-library components used for these checks
were retrieved from the official distribution and SHA256-verified. Preparation
tools and harness binaries are not shipped in the source archive or release kit.
Read-only parsing does not type-check the full QUB project or test an OpenCL driver.

## Native evidence and remaining gates

Full r4 Windows Gate 1 remains mandatory: complete locked tests, the exact
registry, release executables and the corrected disposable mainnet preflight.
PowerShell parsing is included in Gate 0. No full Windows build, GUI mining,
live seed operation or deployment is claimed by this preparation report.

Extract the new r4 kit contents into the existing project root and run Gate 0,
then Gate 1. Cargo dependencies/cache remain in the existing target directory.
Old source, assets, wallet/mainnet data and failure logs are retained. New source
and evidence paths bind the r4 ZIP; a failed r3 attempt is not relabelled as r4.

After Gate 1: isolated GUI mainnet smoke, NYC3/AMS3 checks, dist packaging,
portable-copy smoke and publication in the existing order. A genesis preflight
PASS verifies the disposable configuration; it does not prove current mainnet
synchronization. No manual regtest/LAN lab or long fixed soak is added.

Deployed HF128/v1.8.4 and Explorer 0.7.3 remain the compatibility baseline.
Jinex Wallet is not a release dependency. No chain upgrade is introduced.
