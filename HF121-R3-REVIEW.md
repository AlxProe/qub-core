# HF121 reviewed source revision r3

## Review result

The original, unreleased HF121 source candidate required revision before production use. Revision r3 additionally isolates snapshot self-tests from the live root-owned publication lock. The consensus delta inherited from HF120 was correct and remains untouched, but the new operational surfaces needed memory, publication, and RPC hardening.

## Load/memory review

- `status-fast` now reads `chain-status.json` in normal operation.
- A missing/stale cache triggers a serde streaming visitor that keeps only one block at a time and ignores persisted UTXO/mempool arrays without allocating them.
- The snapshot publisher retains only the latest 4096 blocks while scanning.
- The full canonical chain file is copied byte-for-byte instead of parsed and reserialized in memory.

## Snapshot lock and self-test isolation

- The live publisher keeps its default lock at `/tmp/qub-mainnet-snapshot-publish.lock`.
- `LOCK_FILE` can be overridden for isolated tooling.
- The self-test uses private lock files and logs inside its own temporary directory.
- The self-test invokes the publisher through `bash`, so Windows archive extraction cannot break the test by dropping executable metadata.

## Snapshot integrity review

Before publication, the publisher checks:

1. Top-level network is `mainnet`.
2. The blocks array exists and is complete.
3. Every block references the exact previous block hash.
4. Every block below #24000 is version 1.
5. Every block at/above #24000 is version 2.
6. Tail schemas and tip values are internally consistent.
7. SHA-256 sidecars match the staged files.

`tip.json` is replaced last and serves as the generation commit marker.

## RPC review

HF121 RPC is intentionally groundwork-only:

- loopback only;
- explicit `rpc.enabled=true`;
- real auth token required;
- token checked for every request;
- no browser CORS;
- bounded request headers and socket timeouts;
- no enabled state-changing route;
- template/submit routes return 501.

## Consensus review

No HF121-r2 code changes the HF120 boundary:

```text
height 23999 -> version 1
height 24000 -> version 2
height 24001 -> version 2
```

The source adds regression tests for this boundary and for fast-status metadata/stream fallback behavior.

## Mandatory remaining gate

The archive has source-level parsing checks and the snapshot publisher self-test. A full project compile cannot be substituted by review: run `cargo test`, then release builds for both `qubd` and `qub-core` on the normal Windows build machine and on both seed droplets before public deployment.

## Verification performed during the review

Completed in the review environment:

- Rust syntax parsing with `rustfmt` across all `src/` and `tests/` files.
- Shell syntax parsing for deployment scripts.
- Snapshot publisher valid-generation self-test.
- Snapshot publisher rejection test for an invalid post-activation block version.
- Isolated Rust compilation/execution harness for the streaming status visitor and hardened state-file replacement.
- Isolated Rust compilation/execution harness for loopback detection, RPC token comparison, and HTTP header parsing.
- Mainnet/testnet/regtest config byte-for-byte comparison against HF120.
- Consensus-anchor comparison confirming the #24000 version boundary is unchanged.
- No-assets/runtime/secret file audit.

A complete QUB project build was not run in this review environment because the full pinned GUI/WGPU dependency registry is unavailable here. The normal `cargo test` and release build gates on the Windows build machine and both seed droplets remain mandatory.
