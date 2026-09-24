# HF129 / QUB Core v1.8.5 Full Deployment Runbook

Release-candidate date: 2026-08-25  
Artifact: `HF129-v185-source-no-assets-r6.zip`  
Archive root: `qubd-v1.8.5/`  
Runbook revision: r6 - platform-scoped Windows and Linux validation  
Baseline attachment SHA256: `85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3`

Withdrawn preparation artifacts (do not deploy any listed hash):

- r1 `d9782e26091bc7858eaa588b959de816947a685f31ec6630247095e4935449ea`
  contained invented Windows/repository/key paths and obsolete Explorer
  provenance;
- r2 `64f6d654e28044113d1e8fa9292601c242230e0c22d40b87d89b076e85ae5f16`
  corrected those paths, but its cache-busting filename did not match the
  filename expected by Gate 0 and individually pasted interactive commands
  could continue after a `throw`.
- r3 `9b1c82a98b8f7a2b129caa290d73b9cb61e9141bb73defd49f7d40df9d49ea35`
  fixed naming and fail-stop execution, but Gate 1 incorrectly applied the
  strict no-assets package contract after Gate 0 intentionally restored local
  GUI runtime assets;
- r4 `d29d5e47e90796e9dcba4902db61d3384f7eab2c8da98f5bf8976f8d80e4956b`
  separated the two contract modes, but its local mode used an incomplete
  extension allow-list and rejected an existing HF128 MP4 runtime animation;
- r5 `12672d057d1c47d05964f0bce6474d3abba46ce67cf36065ecbbe08c50768870`
  fixed local asset handling and strict extraction checks, but Gate 1 invoked
  the Linux Bash publisher test from Windows PowerShell. On a Windows host
  without a configured WSL distribution, the Bash relay failed before Cargo.

The HF129 Rust/source-code changes are unchanged by r6. Revision r6 runs the
strict no-assets contract against the untouched ZIP extraction before assets
are copied, and again at seed/repository gates. The Windows build gate permits
regular files only inside the top-level local `assets/` tree, independent of
passive media format, while still rejecting secret/key/runtime-state,
executable/script/archive, symlink, special-file and Explorer-asset material.
The publisher shell self-test runs on both Linux seed staging hosts in Gate 4;
Gate 1 is Windows-native and does not require WSL or Git Bash.

This is a non-consensus reliability release. Follow the order exactly: local
native gates, reproducible source build, NYC3 canary, AMS3 seed, publisher,
separate Explorer API, public/browser/wallet soak, Windows distribution, repo,
then Telegram. Never announce before the six-hour soak passes.

## Stop conditions and preserved state

Stop on any compile/test failure, unexpected consensus diff, missing asset,
same seed identity, non-v2 mainnet tip, seed divergence, Pool-to-Solo event,
publisher cycle over 115 seconds, public tip age approaching 180 seconds,
snapshot source other than `fce`, API/Explorer/Jinex freeze, hash mismatch,
signature failure or clean-install failure.

Never delete, overwrite or roll back:

- `wallet.json` or any private key/seed file;
- `chain-v2/`, `CURRENT.json`, journal/state generations or relay outboxes;
- `node_id`, `peers.json` or regional seed identity/config by accident;
- deployed QUB Explorer v0.7.3 web assets from this source archive. Its exact
  source is not included here.

Only one state-changing QUB process may use a data directory. This runbook uses
the established HF128 operator paths and host identities; do not substitute a
generic path or hostname unless the infrastructure has actually changed.

```text
Windows shell:       Windows PowerShell
Local release root:  C:\Users\proes\Desktop\qub-node
Extracted source:    C:\Users\proes\Desktop\qub-node\qubd-v1.8.5
Repository:          C:\Users\proes\Desktop\qub-node\qub-core-opensource
SSH private key:     %USERPROFILE%\.ssh\jinex_ed25519
NYC3:                deploy@167.99.57.45
AMS3:                deploy@159.223.222.103
Seed staging root:   /opt/jinex/staging/src/qubd
```

## Gate 0 - verify and extract the source-only artifact

Place these two files directly in `C:\Users\proes\Desktop\qub-node`:

- `HF129-v185-source-no-assets-r6.zip`
- `HF129-v185-r6-SHA256SUMS.txt`

Do not rename them. Copy and paste the **entire** block below, including the
opening `. {` and final `}`. The dot-sourced script block is deliberate: one
failure prevents every later statement and keeps the established variables for
the next gate. Never continue after a red error or manually print a PASS line.

```powershell
. {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    $Zip = Join-Path $ProjectRoot 'HF129-v185-source-no-assets-r6.zip'
    $Sums = Join-Path $ProjectRoot 'HF129-v185-r6-SHA256SUMS.txt'
    $Extract = Join-Path $ProjectRoot '_hf129_r6_extract'
    $Work = Join-Path $ProjectRoot 'qubd-v1.8.5'
    $Repo = Join-Path $ProjectRoot 'qub-core-opensource'
    $Key = Join-Path $env:USERPROFILE '.ssh\jinex_ed25519'
    $NYC3 = 'deploy@167.99.57.45'
    $AMS3 = 'deploy@159.223.222.103'

    if (-not (Test-Path $ProjectRoot -PathType Container)) { throw "Local release root missing: $ProjectRoot" }
    if (-not (Test-Path $Zip -PathType Leaf)) { throw "HF129 r6 source ZIP missing: $Zip" }
    if (-not (Test-Path $Sums -PathType Leaf)) { throw "HF129 r6 sums missing: $Sums" }

    $sumLine = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF129-v185-source-no-assets-r6\.zip$' } |
        Select-Object -First 1
    if (-not $sumLine) { throw 'HF129 r6 source ZIP entry missing from r6 SHA256SUMS' }
    $expected = ($sumLine -split '\s+')[0].ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "Invalid expected ZIP SHA256: $expected" }
    $actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "HF129 r6 ZIP SHA256 mismatch: expected $expected, got $actual" }
    if ($actual -in @(
        'd9782e26091bc7858eaa588b959de816947a685f31ec6630247095e4935449ea',
        '64f6d654e28044113d1e8fa9292601c242230e0c22d40b87d89b076e85ae5f16',
        '9b1c82a98b8f7a2b129caa290d73b9cb61e9141bb73defd49f7d40df9d49ea35',
        'd29d5e47e90796e9dcba4902db61d3384f7eab2c8da98f5bf8976f8d80e4956b',
        '12672d057d1c47d05964f0bce6474d3abba46ce67cf36065ecbbe08c50768870'
    )) { throw "Withdrawn HF129 artifact selected: $actual" }

    Remove-Item -LiteralPath $Extract -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract
    $roots = @(Get-ChildItem $Extract -Directory)
    if ($roots.Count -ne 1 -or $roots[0].Name -ne 'qubd-v1.8.5') { throw 'Archive root contract failed' }
    $Candidate = $roots[0].FullName
    if (-not (Test-Path (Join-Path $Candidate 'Cargo.toml') -PathType Leaf)) { throw 'Cargo.toml missing' }

    $RunbookText = Get-Content (Join-Path $Candidate 'HF129-v185-DEPLOY-RUNBOOK.md') -Raw -Encoding UTF8
    foreach ($Marker in @(
        'Runbook revision: r6 - platform-scoped Windows and Linux validation',
        'C:\Users\proes\Desktop\qub-node',
        'HF129-v185-source-no-assets-r6.zip',
        '/opt/jinex/staging/src/qubd'
    )) {
        if (-not $RunbookText.Contains($Marker)) { throw "HF129 r6 runbook marker missing: $Marker" }
    }

    # Run the complete strict source contract against the untouched extraction.
    # This must happen before any private/local runtime asset enters Candidate.
    python (Join-Path $Candidate 'scripts\test-hf129-source-contracts.py')
    if ($LASTEXITCODE) { throw 'HF129 strict extracted-source contract failed' }

    # Privacy/no-assets gate runs against the untouched extraction before any
    # private runtime asset is copied into the local build candidate.
    $allowedAssets = @(
        (Join-Path $Candidate 'assets\README.md').ToLowerInvariant(),
        (Join-Path $Candidate 'explorer\public\assets\README.md').ToLowerInvariant()
    )
    $assetFiles = @(Get-ChildItem $Candidate -Recurse -Force -File | Where-Object {
        $_.FullName.Substring($Candidate.Length).Replace('\','/') -match '(?i)(^|/)assets/'
    })
    foreach ($f in $assetFiles) {
        if ($allowedAssets -notcontains $f.FullName.ToLowerInvariant()) { throw "Private asset in source ZIP: $($f.FullName)" }
    }

    $allowedSecurityUtility = 'deploy/digitalocean/generate-rpc-token.sh'
    $forbiddenExact = @('wallet.json','node_id','peers.json','chain.json','current.json','previous.json','legacy-export-status.json','.env')
    $forbiddenLeaf = '(?i)^(blocks-.*\.jsonl|state-.*\.json|pending-.*|.*\.(pem|key|pfx|p12|keystore|exe|dll|so|dylib|msi|pyc)|.*(secret|token|credential).*)$'
    $forbiddenDir = '(?i)(^|[\\/])(\.git|target|dist|data|chain-v2|__pycache__)([\\/]|$)'
    $bad = @(Get-ChildItem $Candidate -Recurse -Force -File | Where-Object {
        $rel = $_.FullName.Substring($Candidate.Length).TrimStart('\','/').Replace('\','/')
        ($rel -ne $allowedSecurityUtility) -and
        (($forbiddenExact -contains $_.Name.ToLowerInvariant()) -or ($_.Name -match $forbiddenLeaf) -or ($rel -match $forbiddenDir))
    })
    if ($bad.Count) { $bad.FullName; throw 'Runtime/private/build material found in source ZIP' }
    if (Test-Path (Join-Path $Candidate '.git')) { throw '.git must not be packaged' }

    # Resolve and copy known-good private assets into the candidate before the
    # established working directory is changed.
    $AssetSources = @(
        (Join-Path $Work 'assets'),
        (Join-Path $ProjectRoot 'qubd-v1.8.4\assets'),
        (Join-Path $env:LOCALAPPDATA 'Programs\Qubit Coin Core\assets')
    )
    $AssetSource = $AssetSources | Where-Object {
        (Test-Path $_ -PathType Container) -and
        (@(Get-ChildItem $_ -Recurse -File | Where-Object { $_.Name -ne 'README.md' }).Count -gt 0)
    } | Select-Object -First 1
    if (-not $AssetSource) { throw 'No populated known-good HF128/current/installed runtime asset source found' }

    Remove-Item -LiteralPath (Join-Path $Candidate 'assets') -Recurse -Force
    New-Item -ItemType Directory -Force (Join-Path $Candidate 'assets') | Out-Null
    Copy-Item (Join-Path $AssetSource '*') (Join-Path $Candidate 'assets') -Recurse -Force

    # Publish only the fully validated/prepared candidate. Preserve any current
    # v1.8.5 tree under a unique name and restore it if the final move fails.
    $PreviousWork = $null
    try {
        if (Test-Path $Work) {
            $PreviousWork = Join-Path $ProjectRoot ("qubd-v1.8.5-before-r6-" + (Get-Date -Format 'yyyyMMdd-HHmmss'))
            Move-Item -LiteralPath $Work -Destination $PreviousWork
        }
        Move-Item -LiteralPath $Candidate -Destination $Work
    } catch {
        if ((-not (Test-Path $Work)) -and $PreviousWork -and (Test-Path $PreviousWork)) {
            Move-Item -LiteralPath $PreviousWork -Destination $Work
        }
        throw
    } finally {
        Remove-Item -LiteralPath $Extract -Recurse -Force -ErrorAction SilentlyContinue
    }

    $Source = $Work
    Set-Location $Source
    "HF129 r6 ZIP SHA256: $actual"
    "HF129 source: $Source"
    "Assets copied from: $AssetSource"
    "Previous local v1.8.5 backup: $PreviousWork"
    "Repository: $Repo"
    "SSH key: $Key"
    Write-Host 'HF129 r6 PATH / ZIP / PRIVACY / EXTRACTION GATE: PASS'
}
```

## Gate 1 - locked native build and source contracts

The preparation environment did not have Rust/Cargo. This Windows-native gate
is mandatory and must be recorded on the established stable-Rust Windows host
before any server upload. It deliberately does not invoke Bash, WSL or Git Bash;
the publisher shell self-test runs against the exact uploaded source on both
Linux seed staging hosts in Gate 4.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $env:QUB_BUILD_CONFIG = 'mainnet'
    python .\scripts\test-hf129-source-contracts.py --allow-runtime-assets
    if ($LASTEXITCODE) { throw 'HF129 source contracts failed' }

    cargo fmt --all -- --check
    if ($LASTEXITCODE) { throw 'cargo fmt failed' }
    cargo check --locked --all-targets
    if ($LASTEXITCODE) { throw 'cargo check failed' }
    cargo test --locked --all-targets
    if ($LASTEXITCODE) { throw 'cargo test failed' }
    cargo build --locked --release --bin qubd
    if ($LASTEXITCODE) { throw 'qubd release build failed' }
    cargo build --locked --release --bin qub-core --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'qub-core release build failed' }
    cargo build --locked --release --bin qub-rpc-miner --target x86_64-pc-windows-msvc
    if ($LASTEXITCODE) { throw 'qub-rpc-miner release build failed' }
    Write-Host 'HF129 NATIVE BUILD GATE: PASS'
}
```

Run the focused tests by name as an explicit proof they were not silently
filtered out:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    foreach ($TestName in @(
        'hf129_official_tip_freshness_is_fail_closed',
        'hf129_tip_observation_never_pairs_a_lower_hash_with_a_higher_height',
        'hf129_nested_writer_is_reentrant_and_background_coalesces',
        'hf129_pool_restart_is_generation_bound_and_never_solo',
        'hf129_snapshot_capture_order_is_latest_wins',
        'hf129_full_newer_head_older_preserves_visible_tip'
    )) {
        cargo test --locked $TestName
        if ($LASTEXITCODE) { throw "Focused HF129 test failed: $TestName" }
    }
    Write-Host 'HF129 FOCUSED TEST GATE: PASS'
}
```

Then run every existing real-process regression script that is applicable on
the host, especially HF123 storage/RPC, HF125 block relay, HF126 equal-height
recovery and HF127 transaction delivery:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    foreach ($Regression in @(
        '.\scripts\test-hf123-fast-chain-regtest.py',
        '.\scripts\test-hf123-rpc-regtest.py',
        '.\scripts\test-hf125-block-relay-regtest.py',
        '.\scripts\test-hf126-equal-height-fork-regtest.py',
        '.\scripts\test-hf127-tx-delivery-regtest.py'
    )) {
        python $Regression
        if ($LASTEXITCODE) { throw "Real-process regression failed: $Regression" }
    }
    Write-Host 'HF123-HF127 REAL-PROCESS REGRESSION GATE: PASS'
}
```

### Required post-commit fault injection

On a disposable regtest data directory, inject one failure and one 30-second
delay after the atomic `CURRENT.json` rename for each of state cleanup, journal
cleanup, status refresh and legacy export. During each injection:

1. connect at least one valid new block;
2. require the caller returns success once `CURRENT` is committed;
3. sample `status-fast`, the in-process API and `CURRENT.json`;
4. require equal height/hash and API response within two seconds;
5. require no wallet or chain-v2 deletion;
6. remove the injection and rerun the full native suite.

The fault hook may exist only on the test branch/build. It must not be committed
or shipped.

## Gate 2 - mining, Sync and GUI timing matrix

Use isolated regtest/LAN nodes and both a CPU-only system and the intended
OpenCL GPU family. Mine at least 300 accelerated blocks, covering a legacy
export trigger. Record block interval, commit latency and storage stats.

Required state transitions:

| Scenario | Required result |
|---|---|
| Pool A running -> manual Sync/Repair -> writer completes | exactly one Pool A controller resumes; no Solo controller/event/UI label |
| Pool A running -> auto-heal pause -> Stopped acknowledgement | pending mode remains Pool A and Start Solo is disabled |
| Pool A pending resume -> manual Stop | resume is cancelled and stays stopped |
| Solo running -> repair | Solo resumes once, same generation successor semantics |
| PlanRejected | stopped, no automatic fallback |
| Pool A reconfigure CPU/GPU | same Pool A resumes after old generation stops |
| old generation sends a late Stopped | ignored for the newer generation |
| automatic writer owns gate, then manual Sync | manual request survives and reaches the real writer |
| older full snapshot finishes after newer capture | older result is rejected; height/hash/Recent Blocks do not roll back |
| newer full payload carries an older final head | newer balances/activity apply while the already-newer visible head remains unchanged |
| catch-up runs while blocks advance | Recent Blocks advances within one 20-second mining refresh interval |

Use process/controller logs to prove there was never simultaneous old/new GPU
dispatch. Manual Sync is not complete merely because a local wallet snapshot
returned; require the typed network outcome `progress` or `fresh-at-tip`.

## Gate 3 - consensus-diff freeze

Compare HF128 r3 and HF129. Product-version labels may change; operational
storage/P2P/UI/API/publisher files may change. Any change below is a blocker
unless independently reviewed as consensus work:

- genesis/checkpoints and all activation heights;
- block/transaction/hash serialization, PoW, Merkle, DAA and fork choice;
- reward/maturity/monetary constants;
- QNS/JIN/infusion/melt/Library/governance/pool validation;
- pool markers, splits and PPLNS rules;
- `QUB-FCE-1` schema/magic.

Explicitly require this compatibility byte string to remain:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if (-not (Select-String .\src\pools.rs -SimpleMatch 'sig.extend_from_slice(b"/QUB Core v1.8.4/");' -Quiet)) {
        throw 'HF128 pool template signature changed'
    }
    Write-Host 'HF129 POOL TEMPLATE COMPATIBILITY GATE: PASS'
}
```

Confirm mainnet #24000/version 2 and the historic QNS/JIN/pool/checkpoint/DAA/
Blast activation constants in the source-contract output. Do not update a
constant simply to match v1.8.5 branding.

## Gate 4 - build and stage both seeds

Create one clean source tar from a separate staging copy. Do not tar the
Windows working tree directly after private GUI assets or `target/` exist.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $Deploy = 'C:\temp\qubd_hf129_v185_seed_deploy'
    $Tar = 'C:\temp\qubd-hf129-v185-source.tar.gz'

    if (-not (Test-Path $Key -PathType Leaf)) { throw "SSH key missing: $Key" }
    Remove-Item -LiteralPath $Deploy -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Tar -Force -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $Deploy | Out-Null

    robocopy.exe $Source $Deploy /E /R:1 /W:1 `
        /XD .git target dist data chain-v2 .idea node_modules __pycache__ assets `
        /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log *.key *.pem *.pfx *.p12 `
            wallet.json node_id peers.json chain.json CURRENT.json PREVIOUS.json `
            legacy-export-status.json .env
    if ($LASTEXITCODE -gt 7) { throw "Seed staging robocopy failed: $LASTEXITCODE" }

    foreach ($AssetReadme in @('assets\README.md','explorer\public\assets\README.md')) {
        $Destination = Join-Path $Deploy $AssetReadme
        New-Item -ItemType Directory -Force (Split-Path $Destination) | Out-Null
        Copy-Item (Join-Path $Source $AssetReadme) $Destination -Force
    }

    python (Join-Path $Deploy 'scripts\test-hf129-source-contracts.py')
    if ($LASTEXITCODE) { throw 'Clean seed staging source contract failed' }
    tar.exe -czf $Tar -C $Deploy .
    if ($LASTEXITCODE) { throw 'Seed source tar creation failed' }
    $TarHash = (Get-FileHash $Tar -Algorithm SHA256).Hash.ToLowerInvariant()
    $TarHash
    scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf129-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'NYC3 source upload failed' }
    scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf129-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'AMS3 source upload failed' }
    Write-Host 'HF129 CLEAN SEED SOURCE UPLOAD GATE: PASS'
}
```

Build and test the exact uploaded tar on both established staging trees:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf129-v185-source.tar.gz
EXPECTED_HASH=__EXPECTED_TAR_HASH__
CFG=/opt/qub/mainnet/mainnet-seed.toml

test "$(sha256sum "$ARCHIVE" | awk '{print $1}')" = "$EXPECTED_HASH"
sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"
chmod 0755 deploy/digitalocean/*.sh

python3 scripts/test-hf129-source-contracts.py
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh
cargo fmt --all -- --check
cargo check --locked --lib --tests --bin qubd
cargo test --locked --lib --tests --bin qubd
cargo build --locked --release --bin qubd
LC_ALL=C grep -aFq '/QUB Core:1.8.5/' target/release/qubd
sha256sum target/release/qubd

timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > /tmp/hf129-staged-status.json
python3 - <<'PY'
import json
s=json.load(open('/tmp/hf129-staged-status.json'))
assert s['ok'] is True and s['network']=='mainnet'
assert s['version']=='1.8.5' and int(s['height'])>=24000
assert int(s['tip_block_version'])==2 and int(s['next_block_expected_version'])==2
for key in ('hf129_post_commit_live_first','hf129_priority_sync_writer','hf129_monotonic_gui_snapshots','hf129_typed_mining_resume'):
    assert s[key] is True, key
print(s['height'],s['tip_hash'])
PY
'@
    $SeedBuild = $SeedBuild.Replace('__EXPECTED_TAR_HASH__', $TarHash)

    $SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF129 build gate failed' }
    $SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF129 build gate failed' }

    $NycBuildHash = (ssh.exe -i $Key $NYC3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'NYC3 staged binary hash read failed' }
    $AmsBuildHash = (ssh.exe -i $Key $AMS3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'AMS3 staged binary hash read failed' }
    if ($NycBuildHash -notmatch '^[0-9a-f]{64}$' -or $AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Invalid remote build hash' }
    if ($NycBuildHash -ne $AmsBuildHash) { throw "Seed builds differ: NYC3=$NycBuildHash AMS3=$AmsBuildHash" }
    "Reproducible Linux qubd SHA256: $NycBuildHash"
    Write-Host 'HF129 REPRODUCIBLE SEED BUILD GATE: PASS'
}
```

The release binary hashes built on the two equivalent Linux hosts should match.
If they do not, stop and resolve toolchain/source reproducibility before deploy.

Verify seed identities are non-empty and different:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsId = (ssh.exe -i $Key $AMS3 'sudo cat /opt/qub/mainnet/data/node_id').Trim()
    if ($LASTEXITCODE) { throw 'AMS3 node_id read failed' }
    $NycId = (ssh.exe -i $Key $NYC3 'sudo cat /opt/qub/mainnet/data/node_id').Trim()
    if ($LASTEXITCODE) { throw 'NYC3 node_id read failed' }
    if ($AmsId -notmatch '^[0-9a-fA-F]{16,}$' -or $NycId -notmatch '^[0-9a-fA-F]{16,}$') { throw 'Invalid seed identity' }
    if ($AmsId -eq $NycId) { throw 'STOP: duplicated AMS3/NYC3 node_id' }
    Write-Host 'HF129 DISTINCT SEED IDENTITY GATE: PASS'
}
```

Do not regenerate identity as part of an ordinary release. If duplicated, use
the explicit HF128 identity-recovery procedure with backups and only NYC3
offline, then repeat every gate.

## Gate 5 - NYC3 canary first

Keep AMS3 serving. On NYC3, save exact timestamped paths for the binary and
regional config, stop mainnet/testnet with bounds, install only the staged
binary plus `mainnet-seed-nyc3.toml`, start mainnet, and recover automatically on
any failure. Run the entire block from the same Windows PowerShell session. It
sends one non-interactive fail-stop Bash script to NYC3; do not paste its Bash
body line-by-line into an interactive SSH shell.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $NycDeploy = @'
set -euo pipefail
SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
REGIONAL="$SRC/deploy/digitalocean/mainnet-seed-nyc3.toml"
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
STAMP=$(date +%Y%m%d-%H%M%S)
BACKUP=/opt/qub/bin/backups/qubd.before-hf129-$STAMP
CFG_BACKUP=$CFG.before-hf129-$STAMP

test -x "$STAGED"; test -f "$REGIONAL"
grep -Fq 'advertise_addr = "seed-nyc3.qubit-coin.io:17444"' "$REGIONAL"
grep -Fq 'bootnodes = ["seed-ams3.qubit-coin.io:17444"]' "$REGIONAL"
HASH=$(sha256sum "$STAGED" | awk '{print $1}')
sudo install -d /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"; sudo cp "$CFG" "$CFG_BACKUP"
recover() { rc=$?; trap - EXIT; if [ "$rc" -ne 0 ]; then sudo systemctl stop "$MAIN" "$TEST" 2>/dev/null || true; sudo install -m0755 "$BACKUP" "$BIN"; sudo install -m0644 "$CFG_BACKUP" "$CFG"; sudo systemctl start "$MAIN"; sudo systemctl start "$TEST" 2>/dev/null || true; fi; exit "$rc"; }
trap recover EXIT
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true
sudo install -m0755 -o root -g root "$STAGED" "$BIN"
sudo install -m0644 -o root -g root "$REGIONAL" "$CFG"
test "$(sha256sum "$BIN" | awk '{print $1}')" = "$HASH"
sudo systemctl start "$MAIN"
for i in $(seq 1 120); do sleep 5; systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444' && break; test "$i" != 120; done
PID=$(systemctl show -p MainPID --value "$MAIN")
test "$(sudo sha256sum /proc/$PID/exe | awk '{print $1}')" = "$HASH"
timeout 300s "$BIN" --config "$CFG" status-fast >/tmp/hf129-nyc3-status.json
python3 - <<'PY'
import json
s=json.load(open('/tmp/hf129-nyc3-status.json'))
assert s['ok'] is True and s['network']=='mainnet'
assert s['version']=='1.8.5' and s['height']>=24000
assert s['tip_block_version']==2 and s['next_block_expected_version']==2
for k in ('hf129_post_commit_live_first','hf129_priority_sync_writer','hf129_monotonic_gui_snapshots','hf129_typed_mining_resume'):
    assert s[k] is True, k
print(s['height'],s['tip_hash'])
PY
! sudo ss -ltnp | grep -q ':17445'
sudo systemctl start "$TEST" 2>/dev/null || true
trap - EXIT
echo "BACKUP=$BACKUP CFG_BACKUP=$CFG_BACKUP"
'@
    $NycDeploy | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF129 canary install failed or rolled back' }
    Write-Host 'HF129 NYC3 CANARY INSTALL GATE: PASS'
}
```

Keep NYC3 canary online for at least 60 minutes and five new blocks. Sample
`status-fast` every five seconds. Height may stay flat when no block exists but
must never decrease; hash may change at equal height only through verified
re-anchor. Compare AMS3/NYC3 cumulative-work-selected height/hash before AMS3.

## Gate 6 - AMS3 seed, publisher and separate Explorer API

### 6.1 Install AMS3 seed and publisher units

Use the same recoverable seed procedure with
`mainnet-seed-ams3.toml`/NYC3 bootnode. Before installing, stop the snapshot
timer/service. Back up all of these to exact timestamped names:

- `/opt/qub/bin/qubd`;
- `/opt/qub/bin/publish-mainnet-snapshot.sh`;
- `/opt/qub/mainnet/mainnet-seed.toml`;
- `/etc/systemd/system/qub-mainnet-snapshot-publish.service`;
- `/etc/systemd/system/qub-mainnet-snapshot-publish.timer`.

Caddy and the public Windows manifest are not modified in this sub-step; they
are backed up immediately before their own later gates.

Run the entire block from the same Windows PowerShell session. It sends one
non-interactive fail-stop Bash script to AMS3.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsDeploy = @'
set -euo pipefail
SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
SNAP_BIN=/opt/qub/bin/publish-mainnet-snapshot.sh
CFG=/opt/qub/mainnet/mainnet-seed.toml
REGIONAL="$SRC/deploy/digitalocean/mainnet-seed-ams3.toml"
SERVICE_SRC="$SRC/deploy/digitalocean/qub-mainnet-snapshot-publish.service"
TIMER_SRC="$SRC/deploy/digitalocean/qub-mainnet-snapshot-publish.timer"
SERVICE_DST=/etc/systemd/system/qub-mainnet-snapshot-publish.service
TIMER_DST=/etc/systemd/system/qub-mainnet-snapshot-publish.timer
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
SNAP_SERVICE=qub-mainnet-snapshot-publish.service
SNAP_TIMER=qub-mainnet-snapshot-publish.timer
STAMP=$(date +%Y%m%d-%H%M%S)
BACKUP_DIR=/opt/qub/bin/backups/hf129-$STAMP
TIMER_WAS_ACTIVE=0
SNAP_BIN_EXISTED=0
SERVICE_EXISTED=0
TIMER_EXISTED=0

systemctl is-active --quiet "$SNAP_TIMER" && TIMER_WAS_ACTIVE=1 || true
test -x "$STAGED"
test -f "$REGIONAL" -a -f "$SERVICE_SRC" -a -f "$TIMER_SRC"
LC_ALL=C grep -aFq '/QUB Core:1.8.5/' "$STAGED"
grep -Fq 'advertise_addr = "seed-ams3.qubit-coin.io:17444"' "$REGIONAL"
grep -Fq 'bootnodes = ["seed-nyc3.qubit-coin.io:17444"]' "$REGIONAL"
bash -n "$SRC/deploy/digitalocean/publish-mainnet-snapshot.sh"
sudo install -d -m0755 -o root -g root "$BACKUP_DIR"
sudo cp "$BIN" "$BACKUP_DIR/qubd"
sudo cp "$CFG" "$BACKUP_DIR/mainnet-seed.toml"
if [ -f "$SNAP_BIN" ]; then SNAP_BIN_EXISTED=1; sudo cp "$SNAP_BIN" "$BACKUP_DIR/publish-mainnet-snapshot.sh"; fi
if [ -f "$SERVICE_DST" ]; then SERVICE_EXISTED=1; sudo cp "$SERVICE_DST" "$BACKUP_DIR/qub-mainnet-snapshot-publish.service"; fi
if [ -f "$TIMER_DST" ]; then TIMER_EXISTED=1; sudo cp "$TIMER_DST" "$BACKUP_DIR/qub-mainnet-snapshot-publish.timer"; fi

recover() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN" "$TEST" 2>/dev/null || true
    sudo install -m0755 -o root -g root "$BACKUP_DIR/qubd" "$BIN"
    sudo install -m0644 -o root -g root "$BACKUP_DIR/mainnet-seed.toml" "$CFG"
    if [ "$SNAP_BIN_EXISTED" -eq 1 ]; then sudo install -m0755 -o root -g root "$BACKUP_DIR/publish-mainnet-snapshot.sh" "$SNAP_BIN"; else sudo rm -f "$SNAP_BIN"; fi
    if [ "$SERVICE_EXISTED" -eq 1 ]; then sudo install -m0644 -o root -g root "$BACKUP_DIR/qub-mainnet-snapshot-publish.service" "$SERVICE_DST"; else sudo rm -f "$SERVICE_DST"; fi
    if [ "$TIMER_EXISTED" -eq 1 ]; then sudo install -m0644 -o root -g root "$BACKUP_DIR/qub-mainnet-snapshot-publish.timer" "$TIMER_DST"; else sudo rm -f "$TIMER_DST"; fi
    sudo systemctl daemon-reload || true
    sudo systemctl start "$MAIN" || true
    sudo systemctl start "$TEST" 2>/dev/null || true
    if [ "$TIMER_WAS_ACTIVE" -eq 1 ]; then sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true; fi
  fi
  exit "$rc"
}
trap recover EXIT

sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true
sudo install -m0755 -o root -g root "$STAGED" "$BIN"
sudo install -m0755 -o root -g root "$SRC/deploy/digitalocean/publish-mainnet-snapshot.sh" "$SNAP_BIN"
sudo install -m0644 -o root -g root "$REGIONAL" "$CFG"
sudo install -m0644 -o root -g root "$SERVICE_SRC" "$SERVICE_DST"
sudo install -m0644 -o root -g root "$TIMER_SRC" "$TIMER_DST"
sudo install -d -o deploy -g deploy -m0755 /srv/qub-updates/mainnet/snapshots
sudo -u deploy test -r /opt/qub/mainnet/data/chain-v2/CURRENT.json
sudo -u deploy test -w /srv/qub-updates
sudo systemctl daemon-reload
sudo systemctl start "$MAIN"
for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then sudo systemctl status "$MAIN" --no-pager -l || true; exit 1; fi
done
HASH=$(sha256sum "$STAGED" | awk '{print $1}')
PID=$(systemctl show -p MainPID --value "$MAIN")
test "$(sudo sha256sum /proc/$PID/exe | awk '{print $1}')" = "$HASH"
timeout --signal=TERM --kill-after=20s 300s "$BIN" --config "$CFG" status-fast > /tmp/hf129-ams3-status.json
python3 - <<'PY'
import json
s=json.load(open('/tmp/hf129-ams3-status.json'))
assert s['ok'] is True and s['network']=='mainnet'
assert s['version']=='1.8.5' and int(s['height'])>=24000
assert int(s['tip_block_version'])==2 and int(s['next_block_expected_version'])==2
print(s['height'],s['tip_hash'])
PY
! sudo ss -ltnp | grep -q ':17445'
sudo systemctl start "$TEST" 2>/dev/null || true
trap - EXIT
echo "HF129 AMS3 seed/unit install PASS; rollback backup: $BACKUP_DIR"
'@
    $AmsDeploy | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF129 seed/publisher-unit install failed or rolled back' }
    Write-Host 'HF129 AMS3 SEED / PUBLISHER UNIT INSTALL GATE: PASS'
}
```

Repeat the NYC3 status/version/hash/listener checks. Require AMS3 and NYC3 have
different `node_id`, independent regional configs and exactly the same selected
height/hash before enabling publication.

Verify the installed unit definitions are exactly the reviewed files:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsUnitVerify = @'
set -euo pipefail
systemctl cat qub-mainnet-snapshot-publish.service
systemctl cat qub-mainnet-snapshot-publish.timer
sha256sum /etc/systemd/system/qub-mainnet-snapshot-publish.{service,timer}
systemd-analyze verify /etc/systemd/system/qub-mainnet-snapshot-publish.service /etc/systemd/system/qub-mainnet-snapshot-publish.timer
'@
    $AmsUnitVerify | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 publisher unit verification failed' }
    Write-Host 'HF129 AMS3 PUBLISHER UNIT VERIFICATION: PASS'
}
```

Start manual publication only through systemd so it uses the same user,
environment and `/run/qub-snapshot-publisher/publish.lock` as the timer. Never
direct-run the script with its different default `/tmp` lock while the timer may
run:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsPublisherStart = @'
set -euo pipefail
sudo systemctl start qub-mainnet-snapshot-publish.service
systemctl show qub-mainnet-snapshot-publish.service -p Result -p ExecMainStatus -p ActiveEnterTimestamp -p InactiveEnterTimestamp
journalctl -u qub-mainnet-snapshot-publish.service -n 200 --no-pager
sudo systemctl enable --now qub-mainnet-snapshot-publish.timer
systemctl list-timers qub-mainnet-snapshot-publish.timer --all
'@
    $AmsPublisherStart | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 first publication/timer start failed' }
    Write-Host 'HF129 AMS3 FIRST PUBLICATION / TIMER START: PASS'
}
```

For at least ten consecutive cycles require:

- `Result=success`, `ExecMainStatus=0`, no lock-contention/skip line;
- each complete cycle is below approximately 115 seconds;
- `tip.json.source == "fce"`, `recovered_from_previous == false`;
- `tip.json` height/hash exactly equals a pre-captured FCE/`status-fast`
  identity for that generation;
- public `published_at_unix` age always stays below 180 seconds;
- snapshot height/hash converges to AMS3 FCE within two configured cycles;
- service last/next trigger changes and timer remains active.

The 20-minute service timeout is only a kill bound. A repeated cycle over 115
seconds is NO-GO because Core stops treating the HTTP tip as fresh before the
next run.

### 6.2 Caddy cache policy

Adapt and validate the reviewed `Caddyfile.qub-updates`, then reload only after
`caddy validate` succeeds. Test origin and public responses:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsCaddyHeaders = @'
set -euo pipefail
curl -fskI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tip.json
curl -fskI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
'@
    $AmsCaddyHeaders | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Caddy snapshot header request failed' }
}
```

`tip.json` must be `no-store`; tails must be max-age 30/revalidated and must not
also receive bulk max-age 60. Repeat publicly and record `Cache-Control`, `Age`,
`CF-Cache-Status` and response height/hash with a nonce query.

### 6.3 Separate versioned Explorer API binary

Do **not** run `deploy/digitalocean/install-explorer-api-service.sh`: it builds
without the reviewed staged artifact and overwrites shared `/opt/qub/bin/qubd`.

Install the already-tested binary as
`/opt/qub/bin/qubd-explorer-api-hf129`, and override only the Explorer API unit:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsApiInstall = @'
set -euo pipefail
SEED_UNIT=qub-seed-mainnet.service
API_UNIT=qub-explorer-api-mainnet.service
API_BIN=/opt/qub/bin/qubd-explorer-api-hf129
CFG=/opt/qub/mainnet/mainnet-seed.toml
SRC=/opt/jinex/staging/src/qubd
SEED_PID_BEFORE=$(systemctl show -p MainPID --value "$SEED_UNIT")
SEED_EXE_HASH_BEFORE=$(sudo sha256sum /proc/$SEED_PID_BEFORE/exe | awk '{print $1}')
sudo install -m0755 -o root -g root "$SRC/target/release/qubd" "$API_BIN"
sudo install -d /etc/systemd/system/$API_UNIT.d
sudo tee /etc/systemd/system/$API_UNIT.d/hf129.conf >/dev/null <<EOF
[Service]
ExecStart=
ExecStart=$API_BIN --config $CFG explorer-api 127.0.0.1:18765
EOF
sudo systemctl daemon-reload
sudo systemctl restart "$API_UNIT"
for i in $(seq 1 60); do sleep 2; systemctl is-active --quiet "$API_UNIT" && ss -ltn | grep -q ':18765' && break; test "$i" != 60; done
SEED_PID_AFTER=$(systemctl show -p MainPID --value "$SEED_UNIT")
test "$SEED_PID_AFTER" = "$SEED_PID_BEFORE"
test "$(sudo sha256sum /proc/$SEED_PID_AFTER/exe | awk '{print $1}')" = "$SEED_EXE_HASH_BEFORE"
'@
    $AmsApiInstall | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 versioned Explorer API install failed' }
    Write-Host 'HF129 AMS3 VERSIONED EXPLORER API INSTALL: PASS'
}
```

Gate the root and status separately-the old runbook incorrectly required
`version` from an older status schema:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsApiGate = @'
set -euo pipefail
curl -fsS http://127.0.0.1:18765/api/v1/ >/tmp/hf129-api-root.json
curl -fsS http://127.0.0.1:18765/api/v1/status-fast >/tmp/hf129-api-status.json
curl -fsS -w '%{time_total}\n' -o /tmp/hf129-api-summary.json http://127.0.0.1:18765/api/v1/summary
python3 - <<'PY'
import json
r=json.load(open('/tmp/hf129-api-root.json')); s=json.load(open('/tmp/hf129-api-status.json')); q=json.load(open('/tmp/hf129-api-summary.json'))
assert r['version']=='1.8.5'
assert s['version']=='1.8.5' and s['ok'] is True
assert s['hf129_post_commit_live_first'] and s['hf129_priority_sync_writer']
assert q['height']==s['height']
PY
'@
    $AmsApiGate | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Explorer API root/status/summary gate failed' }
    Write-Host 'HF129 AMS3 EXPLORER API GATE: PASS'
}
```

Immediately after several new blocks, require `/api/v1/summary` returns the new
height within two seconds. `/health` alone is insufficient because it does not
prove chain-route freshness. Verify the public API has `Cache-Control: no-store`
and is not served stale by a proxy/CDN.

## Gate 7 - preserve Explorer v0.7.3 and test Jinex compatibility

The bundled `explorer/public/index.html` is legacy v0.7 source. It has HF129
request deadlines/latest-wins hardening, but still rebuilds the complete index
after a changed tail and is not the deployed v0.7.3 artifact. Do not copy or
rsync this tree onto the production webroot.

The published v0.7.3 **r4** no-assets artifact has SHA256
`8eb8ac79c503b564c1a2d49152840ac03c85b4fff5e7a1cfa18565d694dfe749`.
r3 is not an acceptable target: r4 explicitly corrected its browser CORS
preflight and concurrent-initial-loader failures. Prove the seven live runtime
files match r4; do not assume deployment provenance from the `v0.7.3` label
alone:

| Runtime file | Expected SHA256 |
|---|---|
| `index.html` | `71031b2a5e316c5540aaa105d5038d8a989cad997d20ad19c3eec2e466240fcc` |
| `config.js` | `d5a11e160aa600d01289af32b52892776f7455d9f808ad27a6988cbde5c325e1` |
| `v073.js` | `527329ec01df764165ef018dbf75b6c3fff0a040fa67a7f498ec084734181ad9` |
| `v073-live-sync.js` | `6eff01fd75f7b1b6f4dfe24d1b460cb9cfa0ff6d640686bdb08569d6882ad4e8` |
| `v073-qr.js` | `c954dc807dacab8c755b22ba67fc568cad7aebb4367717ccb1d86730cccbc6ab` |
| `v073-jin-accounting.worker.js` | `eb858da0121dea01fab28afce11a365ed6e9902ea3b861eebc6df82eb40bb095` |
| `qrcode-generator.js` | `79ec86f82856005b1c887905cfccfcfbec3821ca61c7fd5a952faa5f778f791c` |

Before release, record:

- deployed Explorer v0.7.3 directory/file SHA256 and its source/release commit;
- deployed Jinex Wallet gateway/client version, configuration and source/release
  commit;
- the exact shared public `tip`/tail/API endpoints each consumes.

Compatibility test the additive HF129 `tip.json` fields against the deployed
Jinex parser. If it uses strict exact-key decoding, move the new diagnostics to
a separate metadata file or release a tested Jinex parser update before Core.

Black-box fault matrix for both Explorer and Jinex:

1. endpoint hangs longer than its timeout;
2. old response finishes after a newer response;
3. tail changes while full source is slow;
4. HTTP tip is frozen for more than 180 seconds;
5. API summary is slow while `status-fast` advances;
6. publisher returns a valid same-height refresh;
7. seeds advance five blocks while one source fails.

Neither app may freeze indefinitely, apply an older tip over a newer tip or
claim synced from stale HTTP liveness. If external code cannot pass, keep the
Core release at NO-GO; do not claim HF129 fixed that component.

## Gate 8 - six-hour cross-client production soak

Leave these open and sample every 5-10 seconds for at least six hours, covering
at least one 256-block or six-hour in-process legacy export trigger:

- AMS3 and NYC3 `status-fast` plus storage stats;
- public `tip.json` and tail-64;
- local and public API root/status/summary latency;
- QUB Explorer v0.7.3 displayed/API tip;
- Jinex Wallet gateway/client tip;
- one Pool miner and one Solo/non-mining Core GUI Recent Blocks view.

Every sample needs timestamp, height, hash, latency and source. Allow one
configured publication interval, never an unbounded freeze. Require:

- canonical commit/API/Recent Blocks continue during detached legacy export;
- seed commit latency and block gaps do not spike for 1-2 hours;
- publisher cycles remain below ~115 seconds and public tip age below 180;
- AMS3/NYC3 converge to the same cumulative-work-selected identity;
- no Pool resume becomes Solo and manual Stop remains authoritative;
- Explorer/Jinex never remain frozen while both seeds advance;
- API summary advances after each sampled new block.

Any failure ends the release. Capture logs before rollback.

## Gate 9 - Windows distribution, signing and public manifest

From the tested source with known-good private assets:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    if (-not $env:QUB_SIGN_CERT_THUMBPRINT) { throw 'QUB_SIGN_CERT_THUMBPRINT is not set in this established release workstation session' }
    .\scripts\build-windows-release.ps1 `
        -Config mainnet `
        -Sign `
        -CertThumbprint $env:QUB_SIGN_CERT_THUMBPRINT `
        -BuildInstaller
    if ($LASTEXITCODE) { throw 'Windows release build failed' }
    Write-Host 'HF129 SIGNED WINDOWS RELEASE BUILD: PASS'
}
```

The script must build fresh executables, include all four required HF129 documents,
Authenticode-sign all required EXEs/installer, generate SHA256 sidecars and emit
the mainnet `manifest.json`. Require `mandatory=true` for mainnet and preserve
the complete unchanged HF128 consensus-family/checkpoint/protocol metadata in
the public manifest. Verify ProductVersion 1.8.5, signature chain, artifact size
and hashes independently; stop if any required manifest field is absent.

Upload versioned installer, latest copy, sidecars and `SHA256SUMS.txt` first.
Install them under a staging name and verify hashes on AMS3. Atomically rename
`manifest.json` last; it is the release pointer. Never publish a manifest before
its exact artifact exists.

Perform both:

- clean install on a fresh Windows profile/device;
- update an actual HF128 v1.8.4 installation through the public manifest.

Require bounded download, size/SHA256/ProductVersion/signature verification,
successful relaunch, preserved wallet/settings/data, correct assets, Sync,
Solo, Pool, Stop and Recent Blocks behavior. Corrupt SHA/size, HTTP/cross-origin
and modified staged EXE tests must fail visibly and keep the old Core runnable.

## Gate 10 - repository update, tag and release

Only after Gate 0-9 and the six-hour soak pass. Use the established
`qub-core-opensource` repository and mirror the exact verified no-assets ZIP,
not the Windows working tree containing private runtime assets.

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'

    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    $Repo = Join-Path $ProjectRoot 'qub-core-opensource'
    $Zip = Join-Path $ProjectRoot 'HF129-v185-source-no-assets-r6.zip'
    $Sums = Join-Path $ProjectRoot 'HF129-v185-r6-SHA256SUMS.txt'
    $Extract = Join-Path $ProjectRoot '_hf129_repo_extract'
    $Sync = Join-Path $ProjectRoot '_repo_sync_hf129'

    if (-not (Test-Path (Join-Path $Repo '.git') -PathType Container)) { throw "Repo missing: $Repo" }
    if (-not (Test-Path $Zip -PathType Leaf)) { throw "HF129 source ZIP missing: $Zip" }
    if (-not (Test-Path $Sums -PathType Leaf)) { throw "HF129 source sums missing: $Sums" }

    $sumLine = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF129-v185-source-no-assets-r6\.zip$' } |
        Select-Object -First 1
    if (-not $sumLine) { throw 'HF129 source ZIP entry missing from SHA256SUMS' }
    $Expected = ($sumLine -split '\s+')[0].ToLowerInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw 'HF129 source ZIP hash mismatch before repo update' }

    Set-Location $Repo
    if (@(git status --porcelain).Count -gt 0) { throw 'Repository has uncommitted changes' }
    git fetch origin --tags --prune
    if ($LASTEXITCODE) { throw 'git fetch failed' }
    git checkout main
    if ($LASTEXITCODE) { throw 'git checkout main failed' }
    git pull --ff-only origin main
    if ($LASTEXITCODE) { throw 'git pull --ff-only failed' }

    Remove-Item -LiteralPath $Extract -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $Sync -Recurse -Force -ErrorAction SilentlyContinue
    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract
    $SourceRoot = Join-Path $Extract 'qubd-v1.8.5'
    if (-not (Test-Path (Join-Path $SourceRoot 'Cargo.toml') -PathType Leaf)) { throw 'HF129 ZIP root missing' }

    python (Join-Path $SourceRoot 'scripts\test-hf129-source-contracts.py')
    if ($LASTEXITCODE) { throw 'HF129 extracted source contract failed before repo mirror' }

    New-Item -ItemType Directory -Force $Sync | Out-Null
    robocopy.exe $SourceRoot $Sync /MIR /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Source mirror failed: $LASTEXITCODE" }

    foreach ($Name in @('LICENSE','NOTICE','SECURITY.md','CONTRIBUTING.md','CODE_OF_CONDUCT.md','.gitignore')) {
        $Existing = Join-Path $Repo $Name
        if (Test-Path $Existing) { Copy-Item $Existing (Join-Path $Sync $Name) -Force }
    }
    if (Test-Path (Join-Path $Repo '.github')) {
        Copy-Item (Join-Path $Repo '.github') (Join-Path $Sync '.github') -Recurse -Force
    }

    robocopy.exe $Sync $Repo /MIR /XD (Join-Path $Repo '.git') /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Repository mirror failed: $LASTEXITCODE" }

    Set-Location $Repo
    python .\scripts\test-hf129-source-contracts.py
    if ($LASTEXITCODE) { throw 'Repository source contract failed' }
    cargo fmt --all -- --check
    if ($LASTEXITCODE) { throw 'Repository cargo fmt failed' }
    cargo check --locked --all-targets
    if ($LASTEXITCODE) { throw 'Repository cargo check failed' }
    cargo test --locked --all-targets
    if ($LASTEXITCODE) { throw 'Repository cargo test failed' }
    git diff --check
    if ($LASTEXITCODE) { throw 'git diff --check failed' }

    $Changes = @(git status --porcelain)
    $Changes
    if ($Changes.Count -eq 0) { throw 'Repository has no HF129 changes; do not create a tag' }
    git diff --stat
    git diff -- src\pools.rs src\lib.rs src\p2p.rs

    git add -A
    git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
    git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh
    git diff --cached --check
    if ($LASTEXITCODE) { throw 'Staged diff check failed' }
    $Staged = @(git diff --cached --name-status)
    $Staged
    foreach ($Required in @(
        'README.md',
        'RELEASE_NOTES-v1.8.5-HF129.md',
        'HF129-v185-RELIABILITY-REVIEW.md',
        'HF129-v185-DEPLOY-RUNBOOK.md',
        'HF129-v185-TEST-REPORT.md',
        'deploy/digitalocean/mainnet-seed-ams3.toml',
        'deploy/digitalocean/mainnet-seed-nyc3.toml',
        'deploy/digitalocean/qub-mainnet-snapshot-publish.service',
        'deploy/digitalocean/qub-mainnet-snapshot-publish.timer'
    )) {
        if (-not ($Staged -match [regex]::Escape($Required))) { throw "Required HF129 path is not staged: $Required" }
    }

    git commit -m 'Update QUB Core to v1.8.5 HF129'
    if ($LASTEXITCODE) { throw 'HF129 commit failed' }
    $Commit = (git rev-parse HEAD).Trim()
    git push origin main
    if ($LASTEXITCODE) { throw 'HF129 main push failed' }
    git fetch origin main
    if ((git rev-parse origin/main).Trim() -ne $Commit) { throw 'Remote main does not match the HF129 commit' }

    if (git tag -l 'v1.8.5-hf129') { throw 'Local tag v1.8.5-hf129 already exists; never move a release tag' }
    $RemoteTag = git ls-remote --tags origin 'refs/tags/v1.8.5-hf129'
    if ($RemoteTag) { throw 'Remote tag v1.8.5-hf129 already exists; never move a release tag' }
    git tag -a v1.8.5-hf129 -m 'QUB Core v1.8.5 HF129' $Commit
    if ($LASTEXITCODE) { throw 'HF129 annotated tag creation failed' }
    git push origin v1.8.5-hf129
    if ($LASTEXITCODE) { throw 'HF129 tag push failed' }

    Write-Host "HF129 REPO/TAG GATE: PASS - $Commit"
}
```

Create the GitHub release only after the pushed commit/tag and public installer
hashes are rechecked:

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    $Source = Join-Path $ProjectRoot 'qubd-v1.8.5'
    $Installer = Join-Path $Source 'dist\installer\QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
    $InstallerSidecar = Join-Path $Source 'dist\updates\mainnet\windows-x64\QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe.sha256'

    foreach ($Required in @(
        (Join-Path $ProjectRoot 'HF129-v185-source-no-assets-r6.zip'),
        (Join-Path $ProjectRoot 'HF129-v185-r6-SHA256SUMS.txt'),
        $Installer,
        $InstallerSidecar,
        (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF129.md')
    )) {
        if (-not (Test-Path $Required -PathType Leaf)) { throw "Release attachment missing: $Required" }
    }

    gh release create v1.8.5-hf129 `
        (Join-Path $ProjectRoot 'HF129-v185-source-no-assets-r6.zip') `
        (Join-Path $ProjectRoot 'HF129-v185-r6-SHA256SUMS.txt') `
        $Installer `
        $InstallerSidecar `
        --repo AlxProe/qub-core `
        --title 'QUB Core v1.8.5 HF129 - Canonical Freshness, Sync & Mining Resume Reliability' `
        --notes-file (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF129.md')
    if ($LASTEXITCODE) { throw 'GitHub release creation failed' }
    Write-Host 'HF129 GITHUB RELEASE: PASS'
}
```

## Gate 11 - Telegram post

Post `HF129-v185-TELEGRAM-POST.txt` only after the merged/tagged release and all
public download checks succeed. Replace its checksum placeholders with the
actual published SHA256 values. Verify every link in an incognito browser and
from a second network before posting.

## Post-release monitoring

For the first 24 hours, keep the Gate 8 sampler running and alert on:

- no new block for two target intervals when independent miners are visible;
- AMS3/NYC3 height/hash divergence;
- public tip age >150 seconds or source not FCE;
- publisher non-success/lock contention/cycle >115 seconds;
- `/api/v1/summary` latency >2 seconds or lag beyond two publication intervals;
- Explorer/Jinex/Core GUI monotonicity violation;
- legacy exporter failure counter/commit latency spike;
- any Pool-to-Solo or duplicate mining-controller report.

Keep the exact deployment backups until at least 72 healthy hours and one full
Windows update cycle.

## Rollback

HF129 has no consensus activation. Rollback binaries and operational config,
not chain/wallet identity. Order:

1. atomically restore the previous public Windows manifest so no new clients
   receive HF129;
2. stop the HF129 snapshot timer/service and restore its exact previous script,
   units and Caddy config;
3. restore the versioned Explorer API binary/drop-in if that layer failed;
4. roll back NYC3 to its exact timestamped binary/config and prove healthy;
5. roll back AMS3 only after NYC3 is healthy;
6. require different seed IDs and the same cumulative-work-selected tip;
7. publish one snapshot through the restored systemd service and verify public
   Explorer/Jinex/Core advance;
8. leave `chain-v2`, wallets, node IDs, peers and durable outboxes untouched.

Rollback is complete only when the old public manifest is live, both P2P 17444
listeners are healthy, 17445 is not public, API is read-only and responsive,
publisher/timer is healthy, and all cross-client tips advance again.

## Evidence record

For every gate archive: UTC start/end, operator, host, source/installer SHA256,
Rust/toolchain version, exact commands/output, seed IDs, height/hash samples,
publisher cycle durations, public headers, Explorer/Jinex provenance, Windows
signature evidence, backups and rollback rehearsal result. A checklist without
command output is not release evidence.
