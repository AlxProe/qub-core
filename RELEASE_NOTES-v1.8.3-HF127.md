# QUB Core v1.8.3 HF127

HF127 is the mandatory reliable-transaction and low-latency mining release.

## Release identity

```text
QUB Core version: 1.8.3
Hotfix: HF127
Package generation: v183
Mandatory: true
New activation: none
chain_upgrade: false
Protocol Epoch 2: #24000 unchanged
Post-activation block version: 2 unchanged
P2P protocol: 2 unchanged
Fast Chain Engine: QUB-FCE-1 unchanged
```

## Reliable transaction delivery

- Adds `SubmitTx` and `TxAck` protocol messages.
- Distinguishes peer acceptance from a successful socket write and returns a positive ACK only after canonical confirmation or durable QUB-FCE-1 mempool commit.
- Accepts delivery proof from one official ACK, two distinct direct-peer ACKs, or one ACK in isolated deployments without official destinations.
- Adds a bounded `tx-submit` seed reserve lane with a short handshake deadline, strict message/submission budgets and no general peer privileges.
- Stores delivery attempts, ACK state, peer tip, retry timing and rejection information in the durable wallet outbox.
- Fails closed on corrupt/full outbox state, never evicts unconfirmed ordinary transactions, preserves ACK/backoff metadata on idempotent admission and limits manual retries to eight records.
- Runs retries independently from GUI polling and mining.
- Adds `tx-delivery-status` and `relay-pending-txs` operator commands.
- Removes network side effects from ordinary transaction-status polling.
- Requires validated canonical evidence before transaction signing.

## Low-latency propagation

- Enables `TCP_NODELAY` on peer sessions.
- Starts receiver-side block/transaction diffusion immediately after durable acceptance and before returning the submit ACK.
- Uses bounded detached outbound dials with exponential backoff.
- Limits active dial scheduling to four attempts per coordinator tick with a 900 ms connect timeout.
- Keeps bootnodes ahead of stale peerbook entries.
- Prevents ephemeral submission source ports from polluting the persistent peerbook.
- Caps the peerbook at 512 entries across load/runtime/save and preserves responsive mempool/block retry timers under unreachable-peer pressure.
- Uses fallible detached propagation/dial thread creation so resource pressure cannot crash the canonical node.
- Uses live canonical state for relay preparation when available.

## Full-power mining UI

- CPU range: 1–100%.
- GPU range: 0–100%.
- Exact numeric percentage inputs beside both sliders.
- 100% CPU means all logical processors and continuous duty.
- Advanced exact CPU worker and GPU work-item overrides remain available without a QUB-specific ceiling.
- 85%+ or any manual override requires a red, explicit, session-only hardware-risk confirmation.
- RPC miner no longer imposes the previous 128-worker QUB cap and uses disjoint nonce lanes.
- If any requested CPU worker cannot be created, GUI/CLI mining fails the round instead of silently using a partial nonce-lane plan; zero-worker rounds never report active mining.

## Compatibility

HF127 changes transport, delivery policy, peer coordination and GUI controls only. It does not change consensus serialization, block hashes, transaction hashes, DAA, rewards, checkpoints, QUB/JIN rules, pools or Fast Chain Engine schema.

Older protocol-v2 peers remain reachable through legacy `Tx` and `Block` fallback messages, but a legacy socket write is not treated as confirmed delivery.

## Required release gates

```powershell
cargo test --locked
cargo build --locked --release --bin qubd
cargo build --locked --release --bin qub-core
cargo build --locked --release --bin qub-rpc-miner

cargo test --locked hf124_ -- --nocapture
cargo test --locked hf125_ -- --nocapture
cargo test --locked hf126_ -- --nocapture
cargo test --locked hf127_ -- --nocapture

py .\scripts\test-hf123-fast-chain-regtest.py --qubd .\target\release\qubd.exe
py .\scripts\test-hf123-rpc-regtest.py --qubd .\target\release\qubd.exe --miner .\target\release\qub-rpc-miner.exe
py .\scripts\test-hf125-block-relay-regtest.py --qubd .\target\release\qubd.exe --timeout 240
py .\scripts\test-hf126-equal-height-fork-regtest.py --qubd .\target\release\qubd.exe --timeout 240
py .\scripts\test-hf127-tx-delivery-regtest.py --qubd .\target\release\qubd.exe --timeout 300
```

Mandatory final E2E line:

```text
HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS
```
