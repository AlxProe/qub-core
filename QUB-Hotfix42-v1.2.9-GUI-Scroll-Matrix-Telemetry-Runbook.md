# QUB Hotfix42 v1.2.9 — GUI Scroll + Matrixchain Telemetry

Scope: GUI-only hotfix.

No chain upgrade. No activation height. No consensus changes.

Changes:
- Adds horizontal scrolling to the central dashboard panel for small screens.
- Removes the duplicate Sync button from Address Activity.
- Adds Mined QUB supply to Live chain data.
- Adds best-effort direct Enjin Matrixchain RPC telemetry for JIN Token supply/infusion values.
- Keeps mainnet consensus overrides from v1.2.6/v1.2.8.

Mainnet consensus values remain:
- QNS activation: #1000
- JIN activation: #5555
- QNS miner split activation: #8305
- JIN conversion activation: #8305

Matrixchain telemetry notes:
- GUI fetches directly from Enjin Matrixchain JSON-RPC in the Windows GUI build.
- These values are telemetry only and do not affect QUB consensus.
- If RPC fetch/decode fails, safe placeholder values remain visible.
