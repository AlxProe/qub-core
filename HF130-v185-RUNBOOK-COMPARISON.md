# HF130 r6 deployment review against the working HF128 procedure

Review date: 2026-09-12. The HF128 workflow comparison was completed in r2 on 2026-09-09. The initial HF130 runbook needed corrections before use.
This revision restores the established operator workflow while retaining the
HF130 changes needed for source integrity, snapshot publication and recovery.
It is a reviewed execution plan, not evidence of a completed live deployment.

## Exact inputs

| Input | SHA256 |
|---|---|
| Operator attachment `HF128-v184-DEPLOY-RUNBOOK(2).md` | `86164e475417e4d3eacd2abe8cd776ddc68796860c9bbdafa6245e429ca50465` |
| Deployed HF128 source ZIP r3 | `85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3` |
| Initial HF130 source ZIP, superseded for deployment preparation | `a93044ac6a6159795d54d1af0122f0084dbb132fb2b0d7a45f5552109d71bd74` |

The attachment is byte-identical to the HF128 runbook inside the authoritative
baseline. The review used the actual HF130 Windows build/verifier, seed/API
installers, publisher, dist publisher, source exporter and packaging policy.

## Workflow corrections retained from r2

| Area | Initial HF130 problem or gap | r2 behavior |
|---|---|---|
| Windows signing | A certificate and signed release were newly mandatory. HF128 explicitly supported release without a certificate. | The established environment variable selects signing when present. Otherwise the runbook explicitly selects `OfficialOriginUnsigned`. The build and verifier require `NotSigned` in that mode; invalid signatures never qualify. Both modes retain exact official HTTPS URLs, hashes, sizes and ProductVersion checks. |
| Inno Setup | The script required `iscc.exe` in PATH; HF128 searched the machine and per-user installation locations. | The build helper resolves PATH, both Program Files locations and LocalAppData before starting Cargo. |
| Existing mainnet state | HF128 A8 had a real-mainnet validation block. The initial HF130 flow supplied regtest tests and later a disposable-genesis preflight, with no equivalent explicit local gate. | Before upload, copy only the closed application's existing `chain-v2` into a unique local temporary directory. Check unchanged CURRENT during the copy, status/version, full validation and preflight. Wallet/settings files are not copied. The original data path is retained. |
| Linux build hashes | Cross-host binary equality was mandatory without pinning compiler/linker environments. | Both seeds build/test the exact source inventory; each staged hash must match its own installed and running binary. Different host toolchains are not presented as a reproducible-build test. |
| Snapshot directory access | The service runs as `deploy`, but the installer only made `snapshots` writable. Canonical aliases are replaced in its parent. | Record both directories' original metadata, grant the required parent and child directory ownership/access, check it before stopping the seed, and restore that metadata if installation fails. No recursive ownership change. |
| First publication evidence | Output displayed systemd results but did not explicitly require a changed generation before enabling the timer. | Assert successful Result, zero exit status, a new `.current` target and active timer. The longer cycle/freshness gates remain required. |
| Origin TLS diagnostic | HF128 used a loopback origin check that allowed the origin certificate. The new command required the local trust store to trust that certificate. | Preserve that established diagnostic only on explicitly pinned loopback, with proxies and redirects excluded. Public requests always verify TLS normally. |
| Cloudflare/public dist | The detailed purge list was replaced with prose; nonce-only requests could pass while ordinary client URLs remained stale. | Restore exact purge URLs, including snapshot sidecars. Download the ordinary seven channel URLs and both root Latest files; compare their bytes/hashes under the same selected Windows trust policy. No nonce is used to satisfy the public gate. |
| Preparation identity | Reusing the first ZIP/runbook name risks selecting an old cached download. | Distinct r2 ZIP, sums and external runbook names. Gate 0 rejects the initial HF130 ZIP hash, validates exact source, and uses the workstation's real PowerShell parser before moving the local source tree. |

The actual owner/access change needs validation with the production `deploy`
account and systemd sandbox. An attempted local unprivileged fixture was
unavailable: this runtime only maps UID/GID 0 and disallows dropping the needed
capability. That is recorded as unavailable, never as a passed permission test.

## Working infrastructure retained

| HF128 workflow | HF130 r2 mapping |
|---|---|
| Local source and assets, A1-A4 | Gate 0; `C:\Users\proes\Desktop\qub-node`, source `qubd-v1.8.5`, existing private assets. Additional known desktop-install asset path included. |
| Native/real-process/mainnet/GUI checks, A5-A9 | Gates 1-2; native Windows commands, no WSL dependency. Existing data: `C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet`. |
| Source staging/builds and identities, B-C2 | Gates 3-4; `/opt/jinex/staging/src/qubd`; different preserved seed node IDs. |
| NYC3 canary then AMS3, D-E | Gates 5-6; `deploy@167.99.57.45` then `deploy@159.223.222.103`; `/opt/qub/bin/qubd`, `/opt/qub/mainnet/mainnet-seed.toml`, existing data and service names. |
| Explorer API and snapshots, F-H | Gate 6; separate versioned API/exporter binaries, existing `qub-explorer-api-caddy` socket bridge, same public API/snapshot URLs. |
| Windows build/manifest/dist/CDN/smoke, I-M | Gate 9; same official origin and `mainnet/windows-x64`, with seven uploaded channel files and the two root aliases produced server-side. |
| Repository/tag/release, N | Gate 10; `qub-core-opensource`, existing `main` workflow, tag `v1.8.5-hf130`, GitHub `AlxProe/qub-core`. |
| Announcement, monitoring and rollback, O-Q | Gate 11 and rollback sections; Telegram remains a draft until native/live/public checks pass. |

The original full Caddy configuration and deployed Explorer v0.7.3 web files
are preserved. The bundled Explorer is not used to overwrite that deployment.
HF130's managed snapshot generations and separately retained exporter are
intentional changes: after a seed-only rollback, HF128's older exporter cannot
satisfy the HF130 publisher's required export-report fields. Reverting only
the seed binary while retaining the compatible publisher/exporter is documented.

## Verification and limits

The workflow-only r2 kept all Rust/Cargo/configuration bytes identical to the
initial HF130. r3 corrected library compile errors, r4 completed GUI test imports,
and r5 corrected Windows storage-fixture initialization. The operator reports
r5 Gate 1 blocks 1/2 completed, with all 22 focused regressions passing. Block 3
then exposed the old immediate-derived-file assumption in the HF123 FCE script.
r6 validates committed FCE artifacts, adds CLI lease-release checks, and gives
finite CLI commands inline post-live maintenance. Persistent services and GUI
retain detached work. Rust changes in main.rs/fast_storage.rs require a new
build and complete Gate 1; the focused registry is now 23 tests. Other Rust,
Cargo/configuration and installer recipes are unchanged from r5. Gate 0 preserves
the Cargo cache and refreshes all corrected module timestamps.

Available preparation checks passed: 13 source-policy tests, 12 publisher
scenarios, 5 simulated-Cargo runner tests, and 9 executions of the actual Linux
dist metadata validator against valid or deliberately invalid fixture files.
Python/Bash syntax, ASCII/fail-stop runbook structure and deterministic source
archive re-extraction are also checked. The test report and raw preparation
evidence state the exact scope.

No Rust compiler or Windows PowerShell runtime was available here. No corrected r6 Windows build,
Authenticode execution, Inno Setup execution, real mainnet-copy validation,
GPU test, live seed deployment, Caddy reload, CDN purge, source publication or
Telegram send was performed. The corresponding gates must still run on the
operator's workstation and seeds. The review therefore establishes corrected
workflow compatibility, not a guarantee that native or production gates pass.

For the reported r5 Gate 1 block 3 failure, restart with Gate 0 using the three new r6 files, then run Gate 1 from its first block.
Do not patch or mix the previous source tree manually: its exact source manifest
would no longer match. Gate 0 preserves the previous local tree and restores
its passive assets into the inventory-verified r6 candidate before the directory swap.
