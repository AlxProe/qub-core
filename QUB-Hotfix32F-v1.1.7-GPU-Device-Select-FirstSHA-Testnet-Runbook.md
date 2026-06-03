# QUB Hotfix32F / v1.1.7 — OpenCL GPU Device Selection + First-SHA Assist (Testnet)

Scope:
- No chain upgrade.
- No activation height.
- No consensus / PoW rule change.
- Mainnet GPU mining remains disabled.
- Testnet-only OpenCL GPU mining alpha.

Changes:
- Auto-selects the strongest OpenCL GPU instead of the first GPU returned by the driver stack.
- Strongly prefers Nvidia discrete GPUs when present, then high-memory/high-CU GPUs.
- GPU kernel now performs only the first SHA256 pass over candidate headers.
- CPU completes the final SHA256, target comparison, and submit verification.
- GPU telemetry should update after completed first-SHA batches.
- Build script now copies confirmed mined block icons and language flag PNGs into release bundles.

Flag assets:
- assets/flags/en-US.png
- assets/flags/el-GR.png

Confirmed block assets:
- assets/your-confirmed-mined-block.png
- assets/your-confirmed-mined-block-white.png

Testing:
- Start QUB Core Testnet.
- Use CPU 1%, GPU 25%.
- Wait 30-60 seconds.
- Expected: GPU device should prefer Nvidia RTX if present, GPU hashrate > 0, GPU total hashes increasing.

If GPU still stays at 0:
- Capture GPU device line, workers, status line, OpenCL scan status, GPU driver version.
- Do not deploy GPU to mainnet.
