# QUB Hotfix32D / v1.1.5 — GPU midstate telemetry + confirmed card icons + language selector

Scope:
- Testnet OpenCL GPU mining alpha hardening.
- No chain upgrade.
- No activation height.
- Mainnet GPU remains disabled.

Key changes:
- GPU kernel now receives CPU-precomputed SHA256 midstate and only scans nonce-dependent work.
- OpenCL batches are intentionally tiny for alpha telemetry stability.
- GPU hashrate/total-hashes should update after completed micro-batches.
- Confirmed mined block card uses your-confirmed-mined-block icon assets.
- Theme and peer tab UI icon hooks improved.
- Adds English / Greek language setting. Fresh setup can default Greek if system env language starts with el; otherwise English.

Flag asset names:
- assets/flags/en-US.png
- assets/flags/el-GR.png

Testnet only:
- Build with `-Config testnet`.
- Deploy only testnet manifest channel.
- Start with CPU 1%, GPU 25%, then 50%, then 75%.
- Expected GPU total hashes should increase if OpenCL kernel completes.
