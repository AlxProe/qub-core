# QUB Core v1.8.5 - HF131

HF131 addresses mining and synchronization reliability while remaining compatible
with deployed v1.8.4. There is no chain upgrade or new activation height.

- CPU and GPU mining retain a valid found block when worker completion races a controller network check. Stop and final parent checks remain enforced.
- GUI mining decisions no longer depend on stale or untrusted display-only peer heights. Healthy direct peers receive probe time when an official seed is unavailable.
- Pool-share preparation retains HF130 cancellation, parent checks and bounded batches. Mining restart keeps the selected solo/pool intent and payout.
- QNS mining payouts resolve against the local validated chain and stay fixed for that run. Pool payouts still require the corresponding local wallet key.
- OpenCL initialization and repeated GPU sessions release owned resources and DLL references, including partial failures. Hash kernels, nonce scanning and power/dispatch rules are unchanged.
- Failed RPC transaction persistence leaves the live mempool retryable. A block accepted locally with a relay failure now reports that failure and the need to retry.
- Source setup uses a separate directory; verified native build evidence can be reused for distribution packaging. Existing source, assets and Cargo caches are retained.

Protocol epoch 2 at #24000, block version 2, genesis, difficulty, rewards, pool
rules and QUB-FCE-1 remain unchanged. Explorer 0.7.3 remains in place.

Download: https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
Source/release: https://github.com/AlxProe/qub-core/releases/tag/v1.8.5-hf131

Preparation review and fixture results are documented separately. Production
availability is established by the deployment runbook, not by this source ZIP.
