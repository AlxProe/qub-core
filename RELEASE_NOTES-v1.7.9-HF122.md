# QUB Core v1.7.9 HF122

HF122 is the first full production release of QUB-native headless node, authenticated RPC, mining-worker and mining-observability infrastructure.

## Consensus status

HF122 introduces **no new consensus activation**.

- Protocol Epoch 2 activated at mainnet block #24000 and remains unchanged.
- Blocks below #24000 use block version 1.
- Blocks from #24000 onward require block version 2.
- No rollback.
- No address blacklist or payout-order rule.
- No DAA change.
- No checkpoint, genesis or economics change.
- No QUB/JIN rule change.

## Headless node and RPC

HF122 activates the authenticated RPC groundwork introduced in HF121:

- chain status and canonical tip;
- block lookup by height or hash;
- transaction lookup;
- bounded mempool view;
- mining status and analytics;
- tracked solo mining templates;
- tracked existing on-chain pool templates;
- compact template batches for parallel workers;
- tracked block submission;
- validated raw transaction submission;
- long-poll canonical-tip events.

Embedded RPC runs inside `qubd node` and shares the node's canonical in-memory `ChainState`. Standalone `rpc-api` remains read-only; template creation, block submission and transaction submission require embedded node mode.

## Reference miner

The release adds `qub-rpc-miner`, a QUB-native reference CPU worker supporting:

- solo payout-address mode;
- existing on-chain pool mode;
- independent parallel template jobs;
- automatic refresh and retry;
- tracked submit responses;
- token, token-file or environment authentication.

This worker defines the native HF122 protocol. It is not a Bitcoin Stratum implementation and does not make stock AxeOS/Bitaxe hardware directly compatible. The dedicated QUB miner adapter remains a separate hardware-tested milestone.

## Mining observability

HF122 adds objective recent-block analytics:

- payout-address and on-chain pool-label distribution;
- top-label share;
- Herfindahl-Hirschman Index (HHI);
- effective label count;
- longest and current same-label streak;
- longest and current exact two-label alternation;
- coinbase-only percentage;
- average, median and p90 block interval;
- block-version distribution.

A payout or pool label is an observable on-chain label, not proof of a unique human, machine, cluster or operator.

## Security posture

- RPC remains disabled in standard mainnet/testnet configs.
- The supplied headless config is loopback-only.
- All requests require token authentication with constant-time comparison.
- Unix token files require owner-only permissions.
- Remote binding requires explicit enablement and CIDR allowlisting.
- No built-in TLS; direct public exposure is unsupported.
- Header/body sizes, connections, request rate, timeouts, job lifetime, cache and batch size are bounded.
- Duplicate sensitive headers, folded headers, transfer-encoding and chunked request bodies are rejected.
- Mining submit accepts only tracked, unexpired jobs tied to the current canonical parent and expected block version.
- Untracked arbitrary block submit is unavailable.
- State-changing standalone RPC is unavailable.

## Explorer v0.7

The companion Explorer v0.7 adds a Mining page with 64/256/1024/4096-block windows, distribution, HHI, effective labels, alternation/streak metrics, interval statistics, coinbase-only percentage and block-version reporting.

## Included previous work

HF117 transaction recovery, HF118 QUB/JIN Melt/Infuse, HF119 non-blocking QUB/JIN flow, HF120 Protocol Epoch 2 and HF121 status/snapshot hardening remain included.
