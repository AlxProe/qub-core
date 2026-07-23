# HF126 / QUB Core v1.8.2 Reliability Review

## Scope

HF126 is a non-consensus liveness, fork-recovery, block-delivery and GUI-observability update. It does not change block validity, proof-of-work calculation, DAA, rewards, checkpoints, genesis, transaction serialization, QUB/JIN semantics, pool-share limits, Fast Chain Engine schema or the P2P protocol number.

## Video-derived failure state

The supplied GUI recording showed all of the following simultaneously:

```text
- a locally validated pending block at height N;
- a different network-reported block at the same height N;
- mining controls in the running state;
- zero measured CPU/GPU hashrate;
- repeated equal-height green-light waits and catch-up pulses;
- local and network candidates presented in one recent/global view.
```

This is a liveness bug: two same-work branches cannot resolve if every updated miner refuses to extend either branch.

## Fork-choice invariants

HF126 preserves cumulative-work fork choice:

1. A higher-work candidate may replace a lower-work chain through the existing path.
2. A lower-work candidate cannot replace a stronger chain.
3. An ordinary equal-work, same-height, different-tip write remains rejected by QUB-FCE-1.
4. An ordinary equal-height tie remains mineable; the next valid block creates the higher-work branch.
5. An equal-work replacement is available only through `save_chain_verified_equal_work_reanchor()`.

## Verified re-anchor boundary

The special re-anchor path is mainnet-only and requires:

- an active pending relay record whose block hash equals the local tip;
- a different exact hash at the same height;
- one of these evidence combinations:
  - official HTTP plus official TCP;
  - two official TCP observations;
  - official HTTP plus two direct-peer observations;
- a downloaded tail/full snapshot whose metadata equals the confirmed competing height/hash;
- a common ancestor;
- complete consensus/checkpoint replay;
- equal or greater cumulative work;
- atomic Fast Chain Engine persistence;
- synchronous publication to the registered live canonical owner.

The storage mutex is released before waiting on the live-chain mutex, avoiding storage/live lock inversion.

If the live owner advanced while the re-anchor waited, the publication path never replaces a stronger or higher live chain.

## Pending relay evidence

HF125 `BlockAck(stale_parent)` responses now persist the most frequently reported competing height/hash and the number of total/official reports. These fields are serde-defaulted, so old pending relay files remain readable.

The pending relay is cleared only after successful adoption/acknowledgement or verified re-anchor.

## Equal-height mining safety

HF126 changes only the interpretation of a different hash at the same height:

```text
same height + different valid hash = unresolved PoW tie
higher height/work                 = stop and catch up
changed local parent               = stop/rebuild
invalid state/version/target       = reject
```

A lack of two exact acknowledgements at the same height is no longer enough to halt all mainnet miners.

A locally mined unacknowledged tip first attempts strict verified re-anchor. If evidence is temporarily unavailable, continuing to mine that fully validated tip is normal Nakamoto-style tie resolution; successful delivery must then carry the winning branch to peers.

## Overlap delivery

HF125 already repaired a receiver whose tip was a direct ancestor. HF126 adds sibling repair:

- receiver answers `stale_parent` at a height below the submitted block;
- sender checks whether the reported hash is its ancestor;
- if yes, sender transmits the missing suffix;
- if not, sender transmits a bounded 32-block overlap window;
- receiver identifies the common ancestor and validates the higher-work chain;
- sender resubmits the exact block;
- delivery succeeds only after `accepted` or `already_known` acknowledgement.

The repair is attempted once per peer connection and uses bounded acknowledgement/overall deadlines.

## Fast Chain Engine policy

`FastCommitPolicy::Normal` is unchanged in effect for all ordinary saves.

`FastCommitPolicy::VerifiedEqualWorkReanchor` rejects every candidate except a mainnet, same-height, same-work, different-tip sibling. It cannot be used to persist:

```text
lower work
lower height
same tip
higher work
non-mainnet state
```

Higher-work adoption continues through the normal commit path.

## GUI correctness

The GUI no longer equates “miner object exists” with active hashing. The header derives state from measured hashrate and current coordination text.

Network and local hashes remain separate. A same-height observed network block is not inserted into local recent history before validation. Repetitive background statuses are coalesced by category rather than appended every pulse.

## Test coverage

HF126 adds:

- equal-height classifier tests;
- independent-evidence threshold tests;
- durable pending-relay backward-compatibility checks;
- verified equal-work Fast Chain Engine persistence/live-owner test;
- a multi-process regtest E2E that creates two height-2 siblings, mines height 3 on one branch, delivers a bounded overlap to the sibling receiver, requires explicit acknowledgement and verifies persistence across receiver restart.

## Residual operational boundaries

- Network liveness still requires actual proof-of-work; software cannot guarantee a block interval.
- HTTP snapshots are transport/evidence inputs, not independent consensus authority.
- Equal-height display is provisional until local replay or greater cumulative work converges.
- Old peers may use legacy relay and cannot provide explicit acknowledgement; durable retry remains active until an updated peer acknowledges.
