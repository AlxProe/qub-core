# HF128 / QUB Core v1.8.4 source revision r3

## Scope

Revision r3 supersedes r2 for source publication and Windows distribution
builds. It was found at the stage-I release boundary after the r2 seed,
Explorer API, snapshot and public seed gates had already passed.

The correction is deliberately narrow:

- `build.rs` now embeds `ProductVersion 1.8.4` and `FileVersion 1.8.4.0`
  instead of the stale v1.8.1 values.
- `scripts/build-windows-release.ps1` checks every native Cargo exit code.
- The builder removes only the three expected Windows release executables
  before compiling and refuses to package if any fresh output is missing.
- Stage I verifies ProductVersion on the GUI, both tools and installer, then
  proves every updater copy is byte-identical to the installer.

## Invariants

r3 changes no Rust runtime, Linux build behavior, consensus rule, serialized
data, P2P protocol, chain state, activation height, reward/DAA/pool rule,
economic rule, seed configuration, Explorer API behavior or updater trust
policy. The already-deployed r2 Linux seed and Explorer API binaries remain the
approved HF128 production binaries.

## Required gate

Build the public Windows distribution only from r3. Publication remains blocked
unless the stage-I output ends with:

```text
HF128 r3 WINDOWS DISTRIBUTION GATE: PASS
```
