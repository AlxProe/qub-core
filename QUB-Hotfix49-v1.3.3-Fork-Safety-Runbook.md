# QUB Hotfix49 / v1.4.7 — Fork-safety, mining guard, DAA v2

HF49 is a mandatory QUB Core hotfix for the post-pooled-mining mainnet fork incident.

## Consensus / protocol

- Mainnet fork-safety checkpoint:
  - height: `#10367`
  - hash: `21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000`
- Mainnet QNS/JIN/Pools activation heights remain unchanged:
  - QNS: `#1000`
  - JIN: `#5555`
  - QNS miner split: `#8305`
  - JIN conversion: `#8305`
  - Pools: `#9999`
- DAA v2 activates on mainnet at `#10500`.
- DAA v2 activates on testnet at `#3330`.

## What this fixes

- Rejects branches that diverged before the canonical post-pools checkpoint.
- Stops same-height arbitrary hash flipping on public networks.
- Adds mining guard before solo/pool mining.
- Mining is paused if the node is below the mainnet checkpoint, has no direct TCP peers, remains behind direct peers after sync, or sees a direct same-height fork.
- Default bootnodes now include multiple global seed hostnames.
- Active sync prioritizes bootnodes and fresh observed peers over stale registry rows.
- DAA v2 retargets every block using a rolling 20-block window after activation.

## Deployment

1. Build and smoke-test locally.
2. Build mainnet installer.
3. Publish as mandatory update.
4. Keep official seeds on canonical branch.
5. Ask miners on wrong hashes to update and repair chain if needed.

## Important

HF49 does not make seeds consensus authorities. The checkpoint only rejects the known stale branch before `#10367`. After that point, normal proof-of-work validation continues, with stronger mining safeguards and faster difficulty recovery after `#10500`.
