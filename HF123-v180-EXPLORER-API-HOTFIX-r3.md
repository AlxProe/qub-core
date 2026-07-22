# HF123 v180 Explorer API mempool hotfix — source revision r3

This non-consensus source revision fixes the HF123 standalone Explorer API mempool path.

## Root causes

1. `explorer_mempool()` formatted every pending transaction through `tx_json()`.
2. `tx_json()` rebuilt the complete confirmed output/spend index for every transaction.
3. With N mempool transactions, the endpoint performed N complete chain-index scans.
4. Concurrent public requests could also refill the complete Explorer chain cache in parallel after a Fast Chain Engine identity change.

## Fixes

- Build the confirmed output/spend index once per mempool response.
- Reuse it for every pending transaction.
- Add a process-wide single-flight guard around Explorer chain-cache refills.
- Recheck the Fast Chain Engine identity after waiting for the refill guard.

## Scope

No change to consensus, block serialization, Protocol Epoch 2, DAA, checkpoints, Fast Chain Engine format, P2P, mining templates, wallet rules, QUB/JIN rules, or snapshot schemas.

The public version remains QUB Core v1.8.0 / HF123 / v180.
