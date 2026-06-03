# QUB Hotfix32E / v1.1.6 — OpenCL GPU Kernel Rewrite + UI Fixes

Testnet-only OpenCL GPU mining alpha. No chain upgrade, no activation height, no consensus changes.

Changes:
- Rewrites OpenCL scan kernel using CPU first-block midstate and 16-word rolling SHA256 schedule.
- Reduces OpenCL work-items to diagnostic micro-batches that should complete fast on AMD/Nvidia.
- GPU telemetry should only count completed OpenCL hash scans.
- GPU remains disabled on mainnet.
- Web map/List icons moved into tab buttons.
- Language dropdown uses flag PNGs: assets/flags/en-US.png and assets/flags/el-GR.png.
- Confirmed mined block card uses your-confirmed-mined-block icon with pending fallback.

Build testnet first.
