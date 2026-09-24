# HF132 / v1.8.5 reliability review

HF132 starts from the exact HF131 r4 source and targets compatibility with the
currently deployed HF128 / v1.8.4. It changes synchronization scheduling,
validated-candidate reuse, read concurrency and GUI status. It introduces no
chain activation, new consensus rule, schema or dependency update.

## What the reported logs establish

The user's copy reached #71487 after a 25,000+ block catch-up, reported an
observed #71515 while a wallet snapshot remained unfinished for 1,921 seconds,
and later reported local #71515. This establishes eventual progress and a long
period without a completed visible snapshot. It does not prove a deadlock,
identify the busy thread, or prove that Start Solo canceled another sync.
No trace or performance recording from that machine was available.

Source inspection found multiple ways to amplify mature-chain work or delay
writers. The changes below address those concrete paths. Their individual
contribution to the user's elapsed time has not been measured.

## Findings, fixes and retained safety boundaries

| Finding | HF132 change | Boundary retained |
|---|---|---|
| Existing FCE reads held the storage writer mutex through disk reading and sometimes full replay | Read immutable captured FCE generations without that writer mutex | Pointer/state identity, committed journal prefix, checksums and authoritative replay remain |
| Busy UI capture could fall back into a blocking full load | Return a retryable busy result; keep one GUI reader | No promotion of structural UI state to a validated mining baseline |
| Manual/automatic wallet refresh bypassed reader admission and could abandon a slow receiver | One attached reader, panic-safe admission and actual outcome delivery | Manual network repair still queues independently; no false completion |
| Long readers could immediately trigger the next periodic read | Restart refresh intervals on successful completion | Block progress and peer freshness remain independently tracked |
| Validated HTTP/session candidates were replayed again during adoption | Reuse the exact validated candidate | Same work/fork-choice rules, local disconnected transaction restoration, remote mempool excluded |
| Duplicate chunks and moving advertisements triggered obsolete recovery work | Recognize known chunks; follow new suffixes from the committed parent | Every new block is validated; genuine fork recovery remains |
| A stalled peer consumed the network opportunity for healthier peers | Bound each peer's socket slice | Existing authentication, exact-tip evidence and independent peer rules |
| Per-chunk chain diffusion ran under catch-up writer admission | Coalesce it and release the writer before detached bounded diffusion | Block ACK thresholds and durable retry/outbox semantics are not relaxed |
| Mining Stop could wait through private replay or writer admission | Cooperative cancellation between blocks and while queued | In-progress storage mutation completes CURRENT/live publication |
| Captured disk state could be stale when daemon live state became available | Recheck exact CURRENT identity and cumulative work before replacement | Explicit committed equal-work recovery remains possible |
| Slow embedded RPC read callback held canonical mutex | Immutable snapshot under short lock, then compute response | Existing authorization, write/submit locking and parent guards |
| RPC mining-statistics cache key and value could come from separate captures | Cache the computed value under its own captured tip | Existing response format and tip-based cache behavior |
| Elapsed time looked like synchronization progress | Show observed height progress and expire stale rates/ETA | Same height alone never proves same hash or network freshness |

## Start / Stop while syncing

Solo and Pool remain typed mining intents, with pinned payout and generation.
Starting while behind does not clear or cancel the independent catch-up. The
mining controller waits for the existing canonical-parent/network safety guard.
Manual Sync and automatic refresh cannot silently switch Pool to Solo, change
its payout, or resurrect a stopped generation. Stop clears mining intent and
cancels its own worker/waiter while the independent sync and snapshot reader
continue. Final parent checks still apply when work is found or submitted.

Tests exercise both the actual GUI busy/error/completion callbacks and a
separately held catch-up writer with a canceled mining waiter. These are
internal fixtures, not evidence that a real OpenCL driver or current mainnet
has passed smoke on the user's Windows machine.

## Compatibility and surrounding functionality

The review compared critical transaction/block/FCE/wire schemas against both
HF131 r4 and HF128. The source policy pins deployed pool logic, all seven config
TOMLs, GPU hashing/scanning/work policy and the P2P wire enum including serde
attributes. Epoch 2 remains at #24000 and block version 2. No new activation,
reward, DAA, genesis, checkpoint, pool-share or QNS/JIN rule is introduced.

HF131 GPU resource ownership, worker shutdown, found-candidate retention,
payout resolution and RPC retry fixes remain. Full src/gpu_miner.rs is unchanged
from HF131 r4; its hashing/work slices remain those of HF128. No new hashrate
or device utilization claim is made.

The Explorer tree, fourteen Explorer API route/serialization functions and
snapshot publisher retain compatibility. Explorer 0.7.3 static deployment is
preserved; the seed/API bridge is checked in the existing two-host runbook.
Jinex Wallet is not deployed or required. Seed downtime cannot change consensus;
healthy direct peers receive time, while an isolated node still cannot prove
that its private chain is the public network tip.

The inherited mainnet preflight fix treats the protocol reserve check as not
applicable before JIN activation #5555. At applicable heights it still checks
the actual reserve/bootstrap state. Genesis preflight is configuration evidence,
not proof of present-day synchronization.

## Practical limits and release evidence

Cold authoritative validation, a true divergent fork, legacy migration and
GUI-derived QNS/JIN/governance history scans can still be expensive. They are
not replaced by trusting an unvalidated cache. Cancellation is cooperative
between blocks; a single validator or filesystem operation is not forcibly
interrupted. Short network deadlines do not discard completed valid CPU work.
There is no guaranteed maximum total replay or catch-up time.

See `HF132-v185-TEST-REPORT.md` for exact executed tests and remaining gates.
The release route remains native tests -> private mainnet GUI smoke -> NYC3
and AMS3 -> dist -> copied portable smoke -> public publication -> GitHub and
Telegram. No manual regtest/LAN lab or fixed long soak was added. Actual Windows
CPU/GPU behavior, moving mainnet tip convergence and mixed-version live operation
remain observed release gates; absent observations are not recorded as PASS.
