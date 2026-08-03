# QUB Core v1.8.4 / HF128 — Mining and Network Liveness

HF128 is a mandatory operational-reliability release. It introduces no chain
upgrade, no activation height and no consensus rule change.

## Release identity

```text
QUB Core: 1.8.4
Hotfix: HF128
Package generation: v184
Canary correction: r2
Windows release correction: r3
New activation height: none
chain_upgrade: false
P2P protocol: 2 unchanged
Fast Chain Engine: QUB-FCE-1 unchanged
Protocol Epoch 2: #24000 unchanged
Post-Epoch-2 block version: 2 unchanged
```

The r3 source supersedes r2 for public Windows builds. It corrects the embedded
Windows `ProductVersion`/`FileVersion` to 1.8.4 and makes every native Cargo
build fail closed before packaging, preventing stale executables from surviving
a failed build. It changes no Rust runtime, Linux binary behavior, consensus,
P2P, chain state, seed configuration or activation rule; the deployed r2 seed
rollout remains valid.

## Mining power and telemetry

- Restores the proven full-power automatic CPU plan at 100%: two independent
  nonce lanes per logical processor, with no fixed software worker cap.
- Keeps lower CPU percentages on the ordinary logical-processor capacity plan
  and applies the requested active/idle ratio without a 250 ms sleep ceiling.
- Separates GPU batch tuning from GPU power. The GPU percentage now controls a
  measured, interruptible active/idle duty ratio; 100% dispatches continuously.
- Tunes each GPU from several measured samples per candidate batch and reuses
  the selected batch for the mining session.
- Bounds a single OpenCL dispatch to a watchdog-safe nonce chunk. This is not a
  total-work or hashrate cap: the next consecutive chunk starts immediately at
  100% power.
- Applies CPU/GPU percentage, override and device changes to active mining by
  retaining the old controller until its explicit `Stopped` acknowledgement;
  no replacement OpenCL generation starts from a wall-clock guess.
- Reports applied CPU/GPU duty rather than echoing an unapplied GUI preference.
- Corrects round-final CPU rate timing and per-device GPU cumulative counters.
- Prevents impossible manual CPU plans from entering an endless retry storm.

High-power authorization remains mandatory at 85% or above and for manual
overrides. Increasing into that range stops the old plan and requires an
explicit new confirmation.

## Pool liveness and QUB Melt compatibility

HF127 had two independent pool-wide stop conditions:

1. A `POOLBLOCK1` containing QUB Melt could never satisfy both the generic
   claimable-fee equation and the legacy pool raw-fee payout equation.
2. A pool with no confirmed PPLNS score waited for a solo-mined block to confirm
   its first pending share.

HF128 fixes both without changing validation:

- Ordinary blocks continue to use `POOLBLOCK1` and unchanged consensus PPLNS.
- A selected QUB Melt uses a markerless, policy-tagged compatibility coinbase.
  Its QUB outputs use the same deterministic pool split, but old HF127 nodes see
  an ordinary generic block and accept the already-correct claimable reward.
- Native-JIN-fee transactions are deferred from a markerless compatibility
  block so legacy JIN fee accounting cannot assign them with solo semantics.
- An empty confirmed share window can be bootstrapped from valid current-parent
  shares selected into the same markerless compatibility block. The following
  parent returns automatically to ordinary `POOLBLOCK1` PPLNS.
- Reserves exactly one valid target-pool share before the global per-block share
  cap, so an older backlog from other pools cannot starve a new pool.
- GUI, CLI pool mining and RPC templates use the same centralized planner.
- RPC v1 keeps `mode: "pool"`; additive `pool_candidate_mode` and
  `compatibility_reason` fields expose HF128 policy without breaking miners.

`POOLCOMPAT1` is diagnostic/template policy only. Consensus deliberately does
not parse it. Existing protocol-v2 HF127 nodes accept these blocks unchanged.

## Faster and safer synchronization

- Removes the unconditional full-chain-from-genesis request on every outbound
  handshake.
- Requests one missing suffix, then expands the overlap only when the first
  response cannot connect.
- Suppresses duplicate requests for the same local/remote view for 30 seconds.
- Retains HF127 protocol-2 Chain count windows for rolling continuation
  compatibility. Byte-cap framing is deferred until peers can negotiate it.
- Queues manual Sync/Repair behind an active automatic catch-up instead of
  silently discarding the manual request.
- Rejects connections that report the local node ID through another DNS alias.
- Learns and process-quarantines an outbound DNS alias after that alias reports
  the local node ID. This stops a legacy generic seed name that resolves back
  to AMS3 from re-entering every heartbeat/sync/relay retry loop.
- The quarantine is scoped to network + data directory and is deliberately
  memory-only. A restart performs one fresh identity check, so a legitimate
  future DNS move is not permanently suppressed.
- Deduplicates official seed evidence by node ID, not hostname.
- Ignores empty-ID/empty-tip responses when selecting an official best tip and
  serializes first-run node-ID creation inside a process.
- Shows one HTTP publication as one observation in Peers instead of duplicating
  it under two seed labels.

## Explorer-facing Core API consistency

The embedded/standalone Explorer API now loads an immutable Fast Chain Engine
snapshot and the identity of that exact commit in one operation. A concurrent
block commit can no longer cache generation N+1 under generation N and leave
API readers temporarily stuck on the wrong cache identity.

Explorer v0.7.2 and Jinex Wallet were audited read-only. Their remaining client
and gateway timeout/cache changes are intentionally deferred to their own
releases; HF128 does not modify either private/public application package.

## Updater recovery

- Bounded HTTPS downloads with three attempts and a GUI watchdog.
- Mandatory 64-hex SHA-256 for manifest-based updates.
- Size limits, `.part` staging, version/hash-specific filenames and atomic
  promotion only after verification.
- Reuses an already verified staged installer instead of downloading it again.
- Keeps Ready/Installing state out of the periodic recheck loop.
- Surfaces Check-now worker, staging and installer-launch failures in the GUI.
- Preserves user Auto check/download/install opt-outs.
- Accepts an unsigned private installer only when manifest, redirects and
  artifact stay on the exact official HTTPS origin; custom origins require the
  pinned valid Authenticode signer.
- Re-verifies size, digest, ProductVersion and signature immediately before
  installer execution.
- Checks the installer exit code, relaunches the previous version on failure and
  writes the post-update restart marker only after a successful install.

Publish the installer first and `manifest.json` last. Never publish an empty or
placeholder SHA-256.

## Required release gates

```powershell
cargo test --locked
cargo build --locked --release --bin qubd
cargo build --locked --release --bin qub-core
cargo build --locked --release --bin qub-rpc-miner
cargo test --locked hf128_ -- --nocapture
```

Run all retained HF123/HF125/HF126/HF127 real-process E2Es, the two-seed
identity/tip checks, a pool bootstrap block, a pool+Melt compatibility block,
and a clean Windows updater install before publication.
