# HF133 candidate: QUB ASERT-MTP-1h

Status: native verification candidate, not a published mainnet upgrade. This
source builds QUB Core 1.8.6 and contains the exact HF132 memory-r1 corrections.
The candidate activation height is 100,000. It is not a public commitment.

## Consensus specification

Only `settings.network.name == "mainnet"` activates the new rules. Heights are
zero-based with genesis at zero. Let H = 100,000. Before H, all existing target,
version, transaction, reward, pool, QNS, JIN and timestamp rules remain in force.

For next block height n >= H, the supplied branch prefix must contain exactly n
blocks. Its anchor is block H-1. Compute:

- T0 = decoded target from the anchor's compact bits.
- M0 = median timestamp of the eleven blocks ending with the anchor.
- Mp = median timestamp of the eleven blocks ending with the parent of n.
- error = Mp - M0 - 60 * (n-H), using signed wide arithmetic.
- target = integer_ASERT(T0, error, half_life=3600 seconds).

The first upgraded block therefore carries the anchor's target. No additional
v2 adjustment is applied at the boundary. All later targets use this same
branch's anchor, so compact rounding does not compound once per block.
Reorganizations crossing H select their own H-1 anchor; no anchor is cached.
The proof-of-work limit remains compact `0x1d07ffff` and the minimum target is 1.

The integer ASERT polynomial, signed division toward zero and arithmetic shifts
follow the BCHN ASERT specification:
https://upgradespecs.bitcoincashnode.org/2020-11-15-asert/
Its exponent uses 16 fractional bits. The production implementation uses i128,
u128 and bounded BigUint operations, never floating point. Extreme exponents
saturate before a large allocation. Targets are encoded by the existing QUB
compact-target encoder. The block hash byte order and proof-of-work comparison
are unchanged.

This is a QUB adaptation, **not standard BCH ASERT parameters**. BCH uses a
600-second schedule, a two-day half-life and a different anchor/time adapter.
QUB uses a 60-second schedule, one-hour half-life and two median-of-11 clocks.
The median adapter addresses the absence of a historical QUB lower timestamp
bound, without retrospectively invalidating old headers. A single backdated
pre-upgrade anchor timestamp cannot by itself impose its full clock offset.

At n >= H:

1. Block version must be 3 (Protocol Epoch 3).
2. Header time must be strictly greater than the median of the preceding eleven
   blocks. The existing future-time rule (7200 seconds in mainnet config) also
   remains applicable.
3. Required bits must equal the calculated compact ASERT target exactly.
4. Every existing PoW, link, transaction, reward and contextual check still runs.

Templates use max(current clock, preceding MTP+1), fail on overflow, and reject
an impossible local clock window. Solo, pool and RPC templates use the common
candidate builder. Both full replay and incremental block connection enforce
MTP. The Fast Chain Engine's authoritative load already performs full replay;
its structural UI-only load also checks expected block versions.

## Evidence and limits

The included fixture contains 128 real mainnet headers #74170..#74297 from the
2026-09-22 peer diagnostic. All 107 available historical v2 retargets must still
match. Header hashes and predecessor links are verified independently in Python
and the Rust test verifies QUB's header hash implementation. This is **not** a
full historical transaction replay.

There are 4,408 generated cross-language arithmetic vectors. They are generated
from our Python reference, **not upstream BCHN vectors**. An independent
80-digit Decimal exponential oracle checks all 65,536 fractional exponents and
signed boundaries. Native execution of the Rust vectors remains required.

The model covers 135 scenarios, 540,000 rounds: steady hashrate, a 10x increase
followed by a 100x decrease and recovery, and alternating hashrate. Each uses
an expected-work run and eight seeded stochastic runs. Assumptions include
12 seconds of template/validation/relay overhead and a timestamp at the start
of mining. This model is a comparative stress test, not a forecast of mainnet.
For steady hashrate, the one-hour candidate's late mean is 59.82 seconds across
the eight stochastic runs, versus 288.72 for v2. The largest sampled candidate
interval is 499 seconds versus 62,608 seconds for v2. Other traffic/CPU/relay
failures can still cause delays.

The 30-minute candidate responds faster but is more sensitive to changes in
hashrate; 2-hour and 4.8-hour candidates recover more slowly in the step model.
One hour is the candidate compromise, not a claim of a proven universal optimum.
A sudden loss of hashrate can still delay the next block for hours: parent-based
ASERT cannot change its target before another block is found. Zero miners means
zero blocks. MTP reduces a single time outlier; it does not prevent majority
hashrate timestamp manipulation. Mainnet consensus code needs native arithmetic
checks, Linux compilation and real-mainnet replay before release approval.

No testnet or regtest daemon or mining process is used by the HF133 workflow.
Historical test files remain in the source archive as history and are not run
by this candidate's explicit test filter.
