# QUB Hotfix32b / v1.1.3 — OpenCL GPU Mining Alpha, Testnet Only

## Scope

This release is a testnet-only GPU mining alpha.

No chain upgrade, no activation height, no PoW rule change, no QNS rule change.

The GPU miner searches the existing QUB block header nonce space with OpenCL. Every GPU-found nonce is verified again on CPU before a block is connected or broadcast.

## GPU safety gates

- GPU mining is enabled only for the testnet build/channel.
- Mainnet GPU mining remains disabled.
- If OpenCL is unavailable, CPU mining continues.
- If the OpenCL kernel fails to build, CPU mining continues.
- If a GPU candidate fails CPU PoW verification, it is ignored.

## Expected telemetry

The GUI should show separate values for:

- CPU hashrate
- GPU hashrate
- CPU total hashes
- GPU total hashes
- CPU workers
- GPU workers
- GPU device
- target block

## Test plan

1. Install/update the testnet build only.
2. Set CPU to a low value, e.g. 1% or 5%.
3. Set GPU to 25%, 50%, then 75%.
4. Verify GPU device appears.
5. Verify GPU hashrate moves above zero.
6. Verify CPU hashrate remains separate.
7. Mine several testnet blocks.
8. Confirm every accepted block validates with `qubd validate`.
9. Confirm mainnet build keeps GPU disabled.

## Rollout

Deploy testnet first. Do not deploy GPU mining to mainnet until this alpha passes on AMD and Nvidia machines.
