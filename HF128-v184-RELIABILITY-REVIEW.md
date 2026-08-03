# HF128 / v1.8.4 Reliability Review

## Scope and invariants

HF128 changes mining scheduling, template policy, transport scheduling, cache
identity handling and update delivery. It does not change block serialization,
PoW, DAA, rewards, transaction validity, checkpoints, activation heights,
Protocol Epoch 2, pool consensus validation or Fast Chain Engine schema.

The release must preserve these invariants:

- every found GPU nonce is verified by the CPU consensus path;
- no old-parent candidate is submitted after the canonical guard fails;
- only one canonical writer owns a data directory;
- compatibility blocks must pass an unchanged HF127 validator;
- a hostname is not treated as an independent node identity;
- an installer is never executed before its manifest digest is verified.

## 1. Why hashrate fell

The HF127 automatic 100% CPU plan used one worker per logical CPU. HF126 used
two disjoint nonce lanes per logical CPU. The change was made while removing
caps and therefore produced the exact ironic result reported by operators: the
100% setting could show roughly half of the previous throughput.

HF128 restores the two-lane 100% plan. This is a scheduling choice, not a PoW
change. Lower percentages remain bounded by logical CPU capacity so 100% is a
clear, deliberate high-power mode.

The GPU slider previously changed the initial/tuning batch only. Several slider
positions could converge on the same continuously running OpenCL batch, and a
GUI edit did not reach already-running workers. HF128 makes the percentage an
effective duty ratio independent of batch tuning and restarts the mining plan
when controls change. GPU tuning measures three post-warmup samples per batch;
telemetry measures the effective rate including requested idle time.

Mining-plan replacement is acknowledgement-based. QUB Core signals Stop but
retains the old controller and drains only its final events. A new controller
cannot start until the old controller has joined all CPU/GPU workers and emits
`Stopped`; a slow driver therefore blocks replacement instead of overlapping
two OpenCL generations.

Large manual work-item values previously became one blocking kernel. Stop and
tip changes could not interrupt `clFinish`. HF128 bounds each dispatch to
2,097,152 work items while scanning all subsequent nonce chunks continuously.
The bound controls cancellation latency, not total search volume.

## 2. Exact pool + Melt failure equation

For a QUB Melt that burns `B` atoms and pays ordinary miner fee `F`:

```text
raw input-output delta                 = B + F
generic claimable fee                  = F
valid generic maximum coinbase         = subsidy + F
legacy POOLBLOCK1 expected coinbase     = subsidy + B + F
irreconcilable difference              = B
```

The default fee was not the cause, and increasing it cannot remove the `B`
difference. All pool miners selected the same high-priority Melt and repeatedly
constructed a locally invalid marked pool block. A solo miner omitted the pool
marker, mined `subsidy + F`, confirmed the Melt and removed the poisoned
transaction. This exactly explains the reported four-hour stop and immediate
recovery after switching to solo.

Changing the legacy pool validator would fork HF127 nodes and is forbidden in a
no-activation release. HF128 instead uses a coinbase script containing
`POOLCOMPAT1`, never `POOLBLOCK1`. Generic HF127 validation accepts the correct
claim, while HF128 template policy keeps deterministic pool outputs. JIN-fee
transactions wait for a marked pool block because legacy JIN accounting uses
the pool marker to select fee receivers.

## 3. Empty PPLNS window deadlock

Pool GUI mining submitted one share for the current parent and then required
that miner to have a confirmed score before hashing. When every active miner
was new, no miner could create the block that confirmed the first share.

HF128 derives a deterministic first payout from valid selected current-parent
shares in a markerless compatibility block. Those shares are validated normally
and become confirmed history. The next height uses ordinary marked PPLNS. If an
existing confirmed score belongs to another miner, a new miner hashes normally;
the current block pays the existing window and confirms the new share.

Selection reserves exactly one valid share for the target empty pool before the
global share cap is filled, then resumes the ordinary oldest/txid order. This
prevents a backlog from unrelated pools from recreating the bootstrap deadlock.

## 4. Explorer/Wallet lag and seed identity

The Peers UI duplicated one HTTP tip publication under two seed hostnames. That
was presentation, not proof that two TCP nodes agreed. Separately, the supplied
seed template advertised the generic alias and included itself as a bootnode,
allowing DNS hairpins. If two droplets were cloned with the same `node_id`, the
old quorum code also counted aliases as independent sources.

HF128 provides regional AMS3/NYC3 configs, rejects its own remote node ID and
deduplicates official observations by node ID. Deployment must still verify
that the two persisted node IDs differ. The generic DNS name may point at AMS3
for discovery, but it is never evidence of a third node.

The first AMS3 canary proved that rejection alone was insufficient: the generic
alias remained in release bootnodes and was retried by heartbeat, discovery and
catch-up paths, producing a local DNS-hairpin connection storm. The r2 canary
correction learns that alias only after an actual remote `node_id` equality,
quarantines it for the lifetime of the process and removes it from the live peer
set. The quarantine is network/data-directory scoped and is not persisted, so
a restart revalidates DNS identity once rather than pinning an obsolete DNS
relationship forever.

The Core Explorer API also had a cache race: it read identity N, then loaded the
chain after commit N+1, and cached that newer chain under N. HF128 returns the
chain and exact identity from one immutable FCE load.

The supplied Explorer v0.7.2 can still leave its refresh latch set after a hung
fetch, and the supplied private Wallet gateway can still replay/cache chain data
inefficiently. Those packages were not changed in this Core release. Their
timeouts, generation guards and gateway cache will be handled in Explorer
v0.7.3 / the Wallet release.

## 5. Transport amplification

HF127 requested Headers and Chain from height zero immediately after each
outbound handshake, then could issue ten overlapping request pairs for one
view. On seeds this multiplies serialization, disk reads and network frames,
which can starve the HTTP API and mobile gateway even while the canonical node
continues mining.

HF128 waits for Version, requests one suffix, escalates overlap by four only on
no progress and suppresses duplicate view requests for 30 seconds. It deliberately
retains HF127's protocol-2 count-window Chain framing: a shorter byte-bounded
response would parse on HF127 but HF127 would not request its continuation.
Byte-cap framing is deferred until an explicit capability can be negotiated.

## 6. Updater failure model

The old updater used unbounded PowerShell web requests, overwrote the final
installer path directly, treated SHA-256 as optional, discarded GUI install
errors and wrote a success/restart marker before knowing the installer result.
Any hung request left `update_check_in_flight=true`, making Check now appear
dead.

HF128 stages through HTTPS to operation-unique `.part` paths, bounds
retries/time/streamed size, requires a manifest digest, atomically renames the
verified artifact and exposes failures. It preserves explicit Auto update
opt-outs. Unsigned private builds are accepted only from the exact official
same-origin path; custom/cross-origin artifacts require the pinned Authenticode
signer. Size, SHA-256, ProductVersion and signature are checked again immediately
before launch. The launcher captures the installer exit code. Only exit code
zero creates the post-update marker; failure writes a result file and relaunches
the old version.

## Residual risks and release gates

The r3 Windows release correction aligns executable `ProductVersion` and
`FileVersion` with v1.8.4. The release builder now removes only the three
expected target executables before compiling and checks every Cargo native exit
code, so a failed build cannot package a stale binary. This is Windows
resource/build-pipeline hardening only; the deployed r2 Linux seed/API binaries
and all runtime/consensus behavior are unchanged.

- Actual GPU throughput remains hardware, driver, clock, thermal and OpenCL
  implementation dependent. HF128 makes power application measurable; it
  cannot promise a specific multiplier on every GPU.
- Authenticode should be required for public mainnet installers. Private builds
  may use the existing explicit unsigned-build policy, but manifest SHA-256 is
  still mandatory.
- `POOLCOMPAT1` payouts are policy-deterministic but generic consensus does not
  enforce pool semantics. That is the mechanism that preserves backward
  compatibility; it must remain rare and visible.
- Explorer and Wallet client-side latches/caches require their own release.
- A seed restart can produce one self-alias rejection before the learned
  quarantine is established. Repeated rejections for the same alias are a
  release blocker.

Production publication is blocked until locked Cargo tests/builds, retained
real-process E2Es, regional seed identity checks, real-mainnet validation, pool
bootstrap/Melt smokes and Windows updater failure/success smokes all pass.
