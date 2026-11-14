QUB Core 1.8.7 HF134 - Linux x86_64

qubd-linux-x86_64 is the exact verified Linux seed binary. Tested on glibc 2.35; maximum referenced glibc symbol 2.34.
Verify SHA256SUMS.txt before use. Set executable permissions (chmod 755 qubd-linux-x86_64).
Use your existing mainnet configuration and wallet/chain data. Stop the previous node before starting its replacement; do not run two writers on the same data directory.
Normal startup performs consensus validation and can take over an hour on current mainnet history. Do not delete data to skip it.
No new consensus activation is introduced by HF134. Epoch 3 / ASERT-MTP from block 100000 remains in effect.
See release notes and exact platform source provenance.
