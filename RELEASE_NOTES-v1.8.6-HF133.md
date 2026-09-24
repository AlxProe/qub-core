# QUB Core 1.8.6 / HF133 — public release draft

NOT PUBLISHED. Activation height 100,000 is a candidate until release review.

HF133 introduces a mainnet consensus upgrade intended to reduce difficulty
oscillation. From the final announced activation height, QUB uses ASERT-MTP
with a 60-second target, one-hour half-life, median-time-past validation and
block version 3. Earlier blocks retain their original consensus rules.

The release also includes the unreleased HF129–HF132 sync, publication and
memory corrections, including memory-r1. GPU kernels, reward amounts, pool
rules, QNS/JIN rules, wire format and chain storage format remain unchanged.

All mainnet users must install the official release before activation. Public
1.8.4 and the HF132 1.8.5 canary do not implement the new rules. After activation
an old node cannot validate the upgraded chain, and its mined version-2 blocks
will not be accepted by upgraded nodes. There is no private miner activation
list or requirement to arrange a coordinated private test with a miner.

The official release must include final source/binary checksums, the installer,
upgrade notice and mandatory chain-upgrade metadata. Do not publish this draft
or use candidate binaries on a live node before the release gates pass.
