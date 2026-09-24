# HF130 / v1.8.5 preparation report - r7

Date: 2026-09-13. Source candidate; deployment readiness is not established.

## Operator evidence and diagnosis

The operator reports all of r6 Gate 1 passed on Windows, including native builds,
Rust regressions, real-process regressions and validation of an old mainnet copy
at height 44423. That result establishes historical data compatibility, not
current mainnet freshness. The r6 helper also reported local node/storage/RPC
checks passing after 300 blocks, export activity and follower catch-up.

The recovered GUI lab has A at height 806 and B at 656. The supplied status and
screenshot show that GUI Sync made no progress; its log has an empty terminal
and target 0. The r6 GUI explicitly returned P2PSyncReport::default() on regtest.
Its official catch-up entry points also exclude regtest. The screenshot also
shows an incorrect payout warning: the GUI validator hardcoded qub although
this isolated configuration uses rqub.

The prior miner timeout and subsequent 8-block gap recovery are distinct events.
The gap was successfully restored. LAG has no passing GUI evidence yet.
Recorded CPU/GPU/pool observations must retain their original verdicts and r6
binary identities; their presence in a helper report does not make them passes.

## r7 correction

- Explicit regtest Sync invokes the existing p2p::sync_quick with at most
  60 seconds of network budget and the canonical writer coordinator.
  Network/PoW/chain adoption checks remain in that shared implementation.
- Automatic regtest wallet refresh stays local, preserving manual lab control.
  Disabled P2P and local-only outcomes have explicit terminal values.
- Mainnet/testnet use the same official deep-repair and automatic ladders with
  the same budgets. No public seeds are added to regtest. A regtest report does
  not become an official retained target or official freshness proof.
- The payout warning uses the prefix loaded into the active settings snapshot.
  A missing snapshot does not cause a guessed mainnet warning; native mining
  still validates the actual configured address. Cross-network prefixes and
  invalid checksums remain rejected. Official-seed repair is unavailable in regtest.
- Three native GUI regressions cover public/regtest/disabled dispatch, address
  prefixes/checksums, and real loopback P2P adoption of a consensus-valid block
  through the same handler used by the GUI worker. The registry has 26 required
  tests: 14 library/seed and 12 GUI. The small native loopback fixture is retained
  in the system temporary directory because detached maintenance may still use it.

Only src/bin/qub_core.rs changes among Rust/Cargo/config/installer files.
All daemon, storage, P2P, RPC, pool, GPU, consensus, Cargo and configuration
bytes remain identical to r6. The deployed HF128 pool/GPU/config freeze and all
462 external locked dependency records remain unchanged.

## Preparation verification and limits

The delivery process checks source inventory/privacy, exact change scope,
the required native-test declaration registry, Python syntax, packaging-policy
fixtures, runbook block structure, embedded Bash/Python syntax, ZIP CRC,
re-extraction and deterministic rebuild. Accompanying preparation evidence
records actual commands/results. These are source/helper checks.

This environment has no Cargo/rustc, Windows PowerShell/GUI or GPU runtime.
The new Rust regressions and Windows continuation have NOT executed here.
No actual r7 GUI catch-up, offline recovery, pool resume or full 150-block
convergence is claimed. A one-block loopback regression, even after it passes,
does not establish responsiveness during the full manual GUI scenario.

The separate v4 helper retains the stopped r6 lab and creates a new local lab
copy for the r7 GUI investigation. It verifies source/build identity and exact
gap anchors, records provenance and checks copied bytes before launching any
process. It does not use funding/mining to rebuild the gap. Its Python fixture
tests are control-flow/filesystem tests, not native mining or GUI tests.

The deployment runbook retains native, mainnet, mixed-version, GPU,
NYC3/AMS3, publisher, Explorer/Jinex, soak, distribution and upgrade gates.
Historical r6 results remain attributed to r6. No deployment, repository/tag
publication, distribution upload or Telegram message was performed.
