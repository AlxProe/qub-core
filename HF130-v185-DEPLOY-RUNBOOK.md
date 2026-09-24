# HF130 / QUB Core v1.8.5 Full Deployment Runbook

Release-candidate date: 2026-09-13  
Artifact: `HF130-v185-source-no-assets-r7.zip`  
Archive root: `qubd-v1.8.5/`  
Runbook revision: HF130 r7 - regtest GUI Sync and active-network payout validation  
Deployed baseline: HF128 / v1.8.4, ZIP SHA256
`85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3`.

HF129 was never deployed. This HF130 release candidate supersedes every HF129
preparation archive. The source ZIP contains no runtime graphics, media, keys,
wallets, chain state or compiled binaries. Restore local passive assets from
the established installation only after verifying the untouched extraction.

This document is an execution plan, not evidence that a gate has passed.
`HF130-v185-TEST-REPORT.md` distinguishes preparation checks from unexecuted
Windows, Rust, GPU, mixed-client and production checks. A source-contract PASS
is not a successful build or proof of mining recovery. All required native and
operational gates below must pass before public distribution or announcement.

HF130 changes reliability behavior, not consensus activation. Order: local
native and mining gates; immutable source staging; Linux builds; NYC3 canary;
AMS3 seed/publisher/API; external-client checks; six-hour soak; verified Windows
distribution; repository/tag/release; Telegram. Keep HF128 serving until the
preceding gate has actual evidence. Do not deploy any HF129 artifact.

This r7 supersedes the r6 preparation ZIP (SHA256
`f573f8e87ccf2f2b982ca05b40d6df9156791fe1b848c57a110b0752faa781fd`).
The operator reports all of r6 Gate 1 passed, plus the separate r6 local
node/storage/RPC lab. The subsequent GUI LAG case failed: A was at #806 and
B at #656, but GUI Sync returned an empty report without contacting a peer.

r7 fixes the GUI's regtest dispatch: an explicit Sync uses the existing bounded
P2P sync path. Automatic regtest refresh stays local, so opening the GUI does
not consume a controlled gap before the operator clicks Sync. Mainnet/testnet
retain the existing official repair and automatic ladders. Regtest peer results
are not treated as official canonical proofs or retained mainnet targets.
The payout warning reads the address prefix from the active settings snapshot;
Repair from official seed is unavailable outside mainnet/testnet.

Only `src/bin/qub_core.rs` changes among Rust/Cargo/config/installer inputs.
The daemon, shared P2P/storage/consensus code, RPC miner, pool/GPU code, all
configuration TOMLs and locked dependencies are byte-identical to r6.
Three new native GUI regressions bring the required registry to 26 tests
(14 library/seed and 12 GUI), including real loopback P2P block adoption through
the same handler used by the GUI worker. They have NOT executed here.

For the existing A #806 / B #656 lab, use the separate
`HF130-r7-GATE2-CONTINUE.md` and `HF130-r7-GATE2-LAB-v4.zip`.
Run this Gate 0 once, then the continuation's native build block (equivalent to
Gate 1 blocks 1 and 2). The helper creates a new local copy of the stopped lab
with the same 150-block gap and r7 binaries. It preserves the original r6 lab
and reports, and does not mine, bootstrap funding or import a public snapshot.
Old observations remain r6 evidence; the new GUI report starts empty.
This continuation is a regression investigation, not an automatic Gate 2 PASS.
Complete the remaining applicable release gates before deployment.

For a fresh release validation, execute Gate 1 below in full. Existing r6
real-process and old-mainnet-copy results remain recorded as r6 evidence;
no changed executable is called tested merely because its predecessor passed.

The HF128 workflow review and r2 corrections remain in force; see
`HF130-v185-RUNBOOK-COMPARISON.md`. Use the external
`HF130-v185-DEPLOY-RUNBOOK-r7.md` with the r7 ZIP/sums. Inside source, the canonical
runbook name remains `HF130-v185-DEPLOY-RUNBOOK.md`.

Gate 0 backs up the previous source and assets and transfers its regular local
`target` cache after the verified source swap. The source backup no longer
contains that cache after a successful move. Cargo rechecks fingerprints;
do not run `cargo clean`. An optional cache-transfer failure is reported and
does not certify a build. This preparation environment has no Rust compiler
or Windows runtime. Native and GUI results remain required on the operator host.

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

- `HF130-v185-source-no-assets-r7.zip`
- `HF130-v185-r7-SHA256SUMS.txt`

Do not rename them. Copy and paste the **entire** block below, including the
opening `. {` and final `}`. The dot-sourced script block is deliberate: one
failure prevents every later statement and keeps the established variables for
the next gate. Never continue after a red error or manually print a PASS line.

```powershell
. {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    $Zip = Join-Path $ProjectRoot 'HF130-v185-source-no-assets-r7.zip'
    $Sums = Join-Path $ProjectRoot 'HF130-v185-r7-SHA256SUMS.txt'
    $Extract = Join-Path $ProjectRoot ('_hf130_extract_' + [guid]::NewGuid().ToString('N'))
    $Work = Join-Path $ProjectRoot 'qubd-v1.8.5'
    $Repo = Join-Path $ProjectRoot 'qub-core-opensource'
    $Key = Join-Path $env:USERPROFILE '.ssh\jinex_ed25519'
    $NYC3 = 'deploy@167.99.57.45'
    $AMS3 = 'deploy@159.223.222.103'

    if (-not (Test-Path $ProjectRoot -PathType Container)) { throw "Local release root missing: $ProjectRoot" }
    if (-not (Test-Path $Zip -PathType Leaf)) { throw "HF130 source ZIP missing: $Zip" }
    if (-not (Test-Path $Sums -PathType Leaf)) { throw "HF130 sums missing: $Sums" }
    Set-Location -LiteralPath $ProjectRoot
    python -I -c "import sys; print(sys.version); sys.exit(0 if sys.version_info >= (3, 10) else 1)"
    if ($LASTEXITCODE) { throw 'Python 3.10 or newer is required for the supplied regression tools' }

    $SumLines = @(Get-Content $Sums -Encoding ASCII | Where-Object { $_ -match '^[0-9a-fA-F]{64}  HF130-v185-source-no-assets-r7\.zip$' })
    if ($SumLines.Count -ne 1) { throw 'Need exactly one exact HF130 ZIP checksum entry' }
    $expected = ($SumLines[0] -split '  ')[0].ToLowerInvariant()
    if ($expected -notmatch '^[0-9a-f]{64}$') { throw "Invalid expected ZIP SHA256: $expected" }
    $actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $expected) { throw "HF130 ZIP SHA256 mismatch: expected $expected, got $actual" }
    if ($actual -in @(
        'f573f8e87ccf2f2b982ca05b40d6df9156791fe1b848c57a110b0752faa781fd',
        '5e33a0bac5a10e056333ee22626561fc7a9e28fe39dbaf921b798feb6fa579c8',
        '66eae99675fd748019ebe49da21bee5949b90188a8e8ef4bad7b1671c0753305',
        'ecc2a538a07f58de931a374190243fe7a01286a9f7a3940231c2441640ed21c3',
        'd04d59398c03c507018b80cb2aa8c2fd5b97f44cb04c63da39b4880fa18e2a0d',
        'a93044ac6a6159795d54d1af0122f0084dbb132fb2b0d7a45f5552109d71bd74',
        'd9782e26091bc7858eaa588b959de816947a685f31ec6630247095e4935449ea',
        '64f6d654e28044113d1e8fa9292601c242230e0c22d40b87d89b076e85ae5f16',
        '9b1c82a98b8f7a2b129caa290d73b9cb61e9141bb73defd49f7d40df9d49ea35',
        'd29d5e47e90796e9dcba4902db61d3384f7eab2c8da98f5bf8976f8d80e4956b',
        '12672d057d1c47d05964f0bce6474d3abba46ce67cf36065ecbbe08c50768870',
        '85160e0f72ed7308ab05793b4981863d8fcfc6b7ebff61f29cbdd1c68afad33c'
    )) { throw "Superseded preparation artifact selected: $actual" }

    if (Test-Path $Extract) { throw "Extraction directory already exists; inspect it and use a new empty directory: $Extract" }
    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract
    $roots = @(Get-ChildItem $Extract -Directory)
    if ($roots.Count -ne 1 -or $roots[0].Name -ne 'qubd-v1.8.5') { throw 'Archive root contract failed' }
    $Candidate = $roots[0].FullName
    if (-not (Test-Path (Join-Path $Candidate 'Cargo.toml') -PathType Leaf)) { throw 'Cargo.toml missing' }

    $RunbookText = Get-Content (Join-Path $Candidate 'HF130-v185-DEPLOY-RUNBOOK.md') -Raw -Encoding UTF8
    foreach ($Marker in @(
        'Runbook revision: HF130 r7 - regtest GUI Sync and active-network payout validation',
        'C:\Users\proes\Desktop\qub-node',
        'HF130-v185-source-no-assets-r7.zip',
        '/opt/jinex/staging/src/qubd'
    )) {
        if (-not $RunbookText.Contains($Marker)) { throw "HF130 runbook marker missing: $Marker" }
    }

    # Run the complete strict source contract against the untouched extraction.
    # This must happen before any private/local runtime asset enters Candidate.
    python (Join-Path $Candidate 'scripts\test-hf130-source-contracts.py') --mode archive --root $Candidate
    if ($LASTEXITCODE) { throw 'HF130 strict extracted-source contract failed' }

    # Use this Windows host's real parser before moving the working tree.
    foreach ($ScriptName in @('build-windows-release.ps1', 'verify-hf130-windows-release.ps1')) {
        $ScriptFile = Get-Item -LiteralPath (Join-Path $Candidate ('scripts\' + $ScriptName))
        $ParseTokens = $null
        $ParseErrors = $null
        $null = [System.Management.Automation.Language.Parser]::ParseFile($ScriptFile.FullName, [ref]$ParseTokens, [ref]$ParseErrors)
        if ($ParseErrors.Count) { $ParseErrors | Format-List; throw "PowerShell syntax failed: $($ScriptFile.FullName)" }
    }
    $PowerShellBlocks = [regex]::Matches($RunbookText, '(?ms)^```powershell\r?\n(.*?)^```\s*$')
    if ($PowerShellBlocks.Count -eq 0) { throw 'No PowerShell runbook blocks found.' }
    foreach ($Block in $PowerShellBlocks) {
        $ParseTokens = $null
        $ParseErrors = $null
        $null = [System.Management.Automation.Language.Parser]::ParseInput($Block.Groups[1].Value, [ref]$ParseTokens, [ref]$ParseErrors)
        if ($ParseErrors.Count) { $ParseErrors | Format-List; throw 'PowerShell runbook syntax failed.' }
    }

    # Resolve and copy known-good private assets into the candidate before the
    # established working directory is changed.
    $AssetSources = @(
        (Join-Path $Work 'assets'),
        (Join-Path $ProjectRoot 'qubd-v1.8.4\assets'),
        'C:\Users\proes\Desktop\Qubit Coin Core\assets',
        (Join-Path $env:LOCALAPPDATA 'Programs\Qubit Coin Core\assets')
    )
    $AssetSource = $AssetSources | Where-Object {
        (Test-Path $_ -PathType Container) -and
        (Test-Path (Join-Path $_ 'qubit-coin-logo.png') -PathType Leaf)
    } | Select-Object -First 1
    if (-not $AssetSource) { throw 'No known-good asset source with qubit-coin-logo.png found; Windows resource metadata requires this asset' }

    New-Item -ItemType Directory -Force (Join-Path $Candidate 'assets') | Out-Null
    Get-ChildItem -LiteralPath $AssetSource -Force | Where-Object { $_.Name -ne 'README.md' } | Copy-Item -Destination (Join-Path $Candidate 'assets') -Recurse -Force
    # Keep the manifest-owned README from the exact source ZIP.
    python (Join-Path $Candidate 'scripts\test-hf130-source-contracts.py') --mode local-build --root $Candidate
    if ($LASTEXITCODE) { throw 'HF130 candidate with restored local assets failed' }

    # Publish only the fully validated/prepared candidate. Preserve any current
    # v1.8.5 tree under a unique name and restore it if the final move fails.
    $PreviousWork = $null
    try {
        if (Test-Path $Work) {
            $PreviousWork = Join-Path $ProjectRoot ("qubd-v1.8.5-before-hf130-r7-" + (Get-Date -Format 'yyyyMMdd-HHmmss-fff'))
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

    # Cargo rechecks fingerprints for the changed source. Reuse its local cache
    # after the verified source swap; keep it in the backup if transfer fails.
    $CargoCacheStatus = 'No previous target cache found'
    if ($PreviousWork) {
        $PreviousTarget = Join-Path $PreviousWork 'target'
        if (Test-Path -LiteralPath $PreviousTarget) {
            $TargetItem = Get-Item -LiteralPath $PreviousTarget -Force
            if ($TargetItem.PSIsContainer -and -not ($TargetItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                try {
                    Move-Item -LiteralPath $PreviousTarget -Destination (Join-Path $Work 'target') -ErrorAction Stop
                    $CargoCacheStatus = 'Reused at ' + (Join-Path $Work 'target')
                } catch {
                    $CargoCacheStatus = 'Transfer failed; inspect the previous backup target directory'
                    Write-Warning "Optional Cargo cache transfer failed. The verified new source is installed; Cargo can rebuild. $($_.Exception.Message)"
                }
            } else {
                $CargoCacheStatus = 'Non-regular target retained in the previous backup'
            }
        }
    }

    # ZIP timestamps can predate the cached check. Mark the corrected modules
    # as freshly changed without altering their manifest-verified contents.
    foreach ($ChangedModule in @('src\p2p.rs', 'src\bin\qub_core.rs', 'src\fast_storage.rs', 'src\main.rs')) {
        [IO.File]::SetLastWriteTimeUtc((Join-Path $Work $ChangedModule), [DateTime]::UtcNow)
    }

    $Source = $Work
    Set-Location $Source
    "HF130 ZIP SHA256: $actual"
    "HF130 source: $Source"
    "Assets copied from: $AssetSource"
    "Previous local v1.8.5 backup: $PreviousWork"
    "Cargo build cache: $CargoCacheStatus"
    "Repository: $Repo"
    "SSH key: $Key"
    Write-Host 'HF130 r7 PATH / ZIP / PRIVACY / EXTRACTION GATE: PASS'
}
```

## Gate 1 - locked native build and source contracts

Check the accompanying test report for the exact preparation environment and actual results. This Windows-native gate
is mandatory and must be recorded on the established stable-Rust Windows host
before any server upload. It deliberately does not invoke Bash, WSL or Git Bash;
the publisher shell self-test runs against the exact uploaded source on both
Linux seed staging hosts in Gate 4. Do not run `cargo fmt` on this release tree:
that would change manifest-verified bytes. A read-only formatting audit is
optional, separately reported, and may show inherited style differences; it is
not a compilation test or a mandatory release gate.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $env:QUB_BUILD_CONFIG = 'mainnet'
    python .\scripts\test-hf130-source-contracts.py --mode local-build --root $Source
    if ($LASTEXITCODE) { throw 'HF130 source contracts failed' }

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
    Write-Host 'HF130 NATIVE BUILD GATE: PASS'
}
```

Run the required regression registry. The runner lists the real Rust tests,
requires every registered test name and a nonzero count, then runs them and
checks the number actually passed. A Cargo exit code from a zero-match filter
cannot satisfy this gate. The r7 registry expects 26 tests (14 seed, 12 GUI).

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    python .\scripts\test-hf130-native-tests.py --scope all
    if ($LASTEXITCODE) { throw 'HF130 required native regression suite failed' }
    Write-Host 'HF130 FOCUSED TEST GATE: PASS'
}
```

Run the existing real-process regressions against the exact executables built
above. These scripts use disposable regtest directories. Explicit paths avoid
falling back to an unrelated debug executable from a previous attempt. The FCE
script checks committed files before invoking status/export; it does not require
optional derived files to exist immediately after init or wait for an exited
process. Repeated finite CLI writes must return with no storage/export lease.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $RegtestQubd = Join-Path $Source 'target\release\qubd.exe'
    $RegtestMiner = Join-Path $Source 'target\x86_64-pc-windows-msvc\release\qub-rpc-miner.exe'
    $RegtestConfig = Join-Path $Source 'config\regtest.toml'
    foreach ($File in @($RegtestQubd, $RegtestMiner, $RegtestConfig)) {
        if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { throw "Required regression input missing: $File" }
    }
    python .\scripts\test-hf123-fast-chain-regtest.py --qubd $RegtestQubd --config $RegtestConfig
    if ($LASTEXITCODE) { throw 'HF123 fast-chain real-process regression failed' }
    python .\scripts\test-hf123-rpc-regtest.py --qubd $RegtestQubd --miner $RegtestMiner --config-template $RegtestConfig
    if ($LASTEXITCODE) { throw 'HF123 RPC/miner real-process regression failed' }
    foreach ($Regression in @('.\scripts\test-hf125-block-relay-regtest.py', '.\scripts\test-hf126-equal-height-fork-regtest.py', '.\scripts\test-hf127-tx-delivery-regtest.py')) {
        python $Regression --qubd $RegtestQubd --config-template $RegtestConfig
        if ($LASTEXITCODE) { throw "Real-process regression failed: $Regression" }
    }
    Write-Host 'HF123-HF127 REAL-PROCESS REGRESSION GATE: PASS'
}
```

### Existing HF128 mainnet data compatibility

This restores HF128 A8 before any upload. Close all QUB Core, daemon and miner
processes first. The known-working data path is preserved below. Copy only its
`chain-v2` directory into a unique local temporary directory and validate that
copy; wallets, settings and the original chain remain in place. Allow enough
free space for one FCE copy. Keep this local evidence private and outside source.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $Qubd = Join-Path $Source 'target\release\qubd.exe'
    $RealDataDir = 'C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet'
    $OriginalFce = Join-Path $RealDataDir 'chain-v2'
    $OriginalCurrent = Join-Path $OriginalFce 'CURRENT.json'
    foreach ($File in @($Qubd, $OriginalCurrent)) {
        if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { throw "Existing mainnet validation input missing: $File" }
    }
    $QubProcesses = @(Get-Process -Name 'QUB-Core','qub-core','qubd','qub-rpc-miner' -ErrorAction SilentlyContinue)
    if ($QubProcesses.Count) { throw 'Close all QUB Core, daemon and miner processes before copying mainnet state.' }
    $CurrentBefore = (Get-FileHash -LiteralPath $OriginalCurrent -Algorithm SHA256).Hash
    $MainnetEvidence = Join-Path ([IO.Path]::GetTempPath()) ('qub-hf130-mainnet-' + [guid]::NewGuid().ToString('N'))
    $CopyData = Join-Path $MainnetEvidence 'data'
    $CopyFce = Join-Path $CopyData 'chain-v2'
    New-Item -ItemType Directory -Path $CopyFce -Force | Out-Null
    robocopy.exe $OriginalFce $CopyFce /E /COPY:DAT /DCOPY:DAT /XJ /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Mainnet FCE copy failed: $LASTEXITCODE" }
    if ((Get-FileHash -LiteralPath $OriginalCurrent -Algorithm SHA256).Hash -ne $CurrentBefore -or
        (Get-FileHash -LiteralPath (Join-Path $CopyFce 'CURRENT.json') -Algorithm SHA256).Hash -ne $CurrentBefore) {
        throw 'Mainnet CURRENT changed during copy; close the writer and repeat with a new temporary directory.'
    }
    $ConfigText = Get-Content -LiteralPath (Join-Path $Source 'config\mainnet.toml') -Raw -Encoding UTF8
    if ([regex]::Matches($ConfigText, '(?m)^data_dir\s*=').Count -ne 1) { throw 'Expected one mainnet data_dir setting.' }
    $CopyTomlPath = $CopyData.Replace('\', '/')
    $ConfigText = [regex]::Replace($ConfigText, '(?m)^data_dir\s*=.*$', ('data_dir = "' + $CopyTomlPath + '"'))
    $MainnetCheckConfig = Join-Path $MainnetEvidence 'mainnet-validation.toml'
    [IO.File]::WriteAllText($MainnetCheckConfig, $ConfigText, [Text.UTF8Encoding]::new($false))
    $StatusText = & $Qubd --config $MainnetCheckConfig status-fast
    if ($LASTEXITCODE) { throw 'Existing mainnet status-fast failed.' }
    $StatusText | Set-Content -LiteralPath (Join-Path $MainnetEvidence 'status-fast.json') -Encoding UTF8
    $Status = $StatusText | ConvertFrom-Json
    if ($Status.ok -ne $true -or $Status.network -ne 'mainnet' -or $Status.version -ne '1.8.5' -or
        [int64]$Status.height -lt 24000 -or [int]$Status.tip_block_version -ne 2 -or [int]$Status.next_block_expected_version -ne 2) {
        throw 'Existing mainnet identity/version gate failed.'
    }
    foreach ($Command in @('storage-stats', 'validate', 'block-relay-status', 'tx-delivery-status')) {
        & $Qubd --config $MainnetCheckConfig $Command
        if ($LASTEXITCODE) { throw "Existing mainnet command failed: $Command" }
    }
    $PreflightText = & $Qubd --config $MainnetCheckConfig preflight
    if ($LASTEXITCODE) { throw 'Existing mainnet preflight failed.' }
    $PreflightText | Set-Content -LiteralPath (Join-Path $MainnetEvidence 'preflight.json') -Encoding UTF8
    $Preflight = $PreflightText | ConvertFrom-Json
    if ($Preflight.ok -ne $true -or $Preflight.network -ne 'mainnet' -or [int64]$Preflight.height -lt 24000) { throw 'Existing mainnet preflight identity failed.' }
    $Status | Format-List network,version,height,tip_hash,tip_block_version,next_block_expected_version
    Write-Host "HF130 EXISTING MAINNET COPY VALIDATION: PASS - $MainnetEvidence"
}
```

The installer helper's disposable genesis preflight later checks the shipping
configuration; it is not a substitute for this existing-chain validation.
Because only chain state is copied, relay/outbox commands here test CLI loading,
not the user's original pending transactions. Test those through the GUI matrix.

### Required post-commit evidence

The required native test registry includes the HF130 storage regressions for
maintenance failure, delayed maintenance, pointer pinning, active readers and
busy writers. Run those shipped tests through the Gate 1 registry; do not edit
production source, introduce environment fault hooks or regenerate the source
manifest locally to satisfy this release gate. See the exact test names and
coverage limitations in `HF130-v185-TEST-REPORT.md`.

On disposable regtest/LAN data, also record successful block commit return,
`CURRENT.json` identity, `status-fast`, and in-process/API identity and latency
while background maintenance/export runs. The API freshness target is two
seconds after a stable committed head. This is separate real-process evidence;
a helper unit-test PASS does not prove the API timing target on production
hardware. No chain/wallet data is deleted to perform this check.

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
| newer full payload carries an older final head | retain the newer head and its QUB balances; only independent non-head fields may advance |
| catch-up runs while blocks advance | Recent Blocks advances within one 20-second mining refresh interval |

Also start a pool miner 100-150+ blocks behind, disconnect and reconnect it,
and verify recovery and resume in the same pool with the same selected address.
Exercise Stop during share preparation, network guards, relay and CPU/GPU drain;
reconfigure power while resuming and trigger an update while resume is queued.
A manual Join has a separate bounded share search. Pool share preparation remains
a prerequisite for pool hashing; a very slow CPU can still take time to find a
valid share. Cancellation checks do not create an absolute Stop bound for disk
commit or a stalled GPU driver.

Use process/controller logs to prove there was never simultaneous old/new GPU
dispatch. Manual Sync is not complete merely because a local wallet snapshot
returned; require the typed network outcome `progress` or `fresh-at-tip`.

## Gate 3 - consensus-diff freeze

Compare the verified HF128 r3 deployed source and HF130. Product-version labels may change; operational
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
    Write-Host 'HF130 POOL TEMPLATE COMPATIBILITY GATE: PASS'
}
```

Confirm mainnet #24000/version 2 and the historic QNS/JIN/pool/checkpoint/DAA/
Blast activation constants in the source-contract output. Do not update a
constant simply to match v1.8.5 branding.

## Gate 4 - build and stage both seeds

Export only the manifest-listed source bytes. The exporter validates the local
source and excludes local assets, target outputs and checkout metadata. Its
destination must be new; it never cleans an existing working tree.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $StageId = [guid]::NewGuid().ToString('N')
    $Deploy = Join-Path $ProjectRoot ('_hf130_seed_source_' + $StageId)
    $Tar = Join-Path $ProjectRoot ('qubd-hf130-v185-source-' + $StageId + '.tar.gz')
    if (-not (Test-Path $Key -PathType Leaf)) { throw "SSH key missing: $Key" }
    python (Join-Path $Source 'scripts\export-hf130-source.py') --source $Source --destination $Deploy
    if ($LASTEXITCODE) { throw 'Exact HF130 source export failed' }
    python (Join-Path $Deploy 'scripts\test-hf130-source-contracts.py') --mode archive --root $Deploy
    if ($LASTEXITCODE) { throw 'Seed export archive contract failed' }
    tar.exe -czf $Tar -C $Deploy .
    if ($LASTEXITCODE) { throw 'Seed source tar creation failed' }
    $TarHash = (Get-FileHash -LiteralPath $Tar -Algorithm SHA256).Hash.ToLowerInvariant()
    scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf130-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'NYC3 source upload failed' }
    scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf130-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'AMS3 source upload failed' }
    "Seed tar SHA256: $TarHash"
    Write-Host 'HF130 CLEAN SEED SOURCE UPLOAD GATE: PASS'
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
export QUB_BUILD_CONFIG=mainnet
rustc --version --verbose
cargo --version

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf130-v185-source.tar.gz
EXPECTED_HASH=__EXPECTED_TAR_HASH__
CFG=/opt/qub/mainnet/mainnet-seed.toml

test "$(sha256sum "$ARCHIVE" | awk '{print $1}')" = "$EXPECTED_HASH"
if [ -e "$SRC" ]; then sudo mv "$SRC" "$SRC.before-hf130-$(date +%Y%m%d-%H%M%S)-$$"; fi
sudo install -d -m0755 -o deploy -g deploy "$SRC"
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"
chmod 0755 deploy/digitalocean/*.sh

python3 scripts/test-hf130-source-contracts.py --mode archive --root "$SRC"
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh
cargo check --locked --lib --tests --bin qubd
cargo test --locked --lib --tests --bin qubd
python3 scripts/test-hf130-native-tests.py --scope seed
cargo build --locked --release --bin qubd --bin qub-rpc-miner
python3 scripts/test-hf123-fast-chain-regtest.py --qubd "$SRC/target/release/qubd" --config "$SRC/config/regtest.toml"
python3 scripts/test-hf123-rpc-regtest.py --qubd "$SRC/target/release/qubd" --miner "$SRC/target/release/qub-rpc-miner" --config-template "$SRC/config/regtest.toml"
for regression in scripts/test-hf125-block-relay-regtest.py scripts/test-hf126-equal-height-fork-regtest.py scripts/test-hf127-tx-delivery-regtest.py; do
    python3 "$regression" --qubd "$SRC/target/release/qubd" --config-template "$SRC/config/regtest.toml"
done
LC_ALL=C grep -aFq '/QUB Core:1.8.5/' target/release/qubd
sha256sum target/release/qubd

timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > /tmp/hf130-staged-status.json
python3 -I - <<'PY'
import json
s=json.load(open('/tmp/hf130-staged-status.json'))
assert s['ok'] is True and s['network']=='mainnet'
assert s['version']=='1.8.5' and int(s['height'])>=24000
assert int(s['tip_block_version'])==2 and int(s['next_block_expected_version'])==2
print(s['height'],s['tip_hash'])
PY
'@
    $SeedBuild = $SeedBuild.Replace('__EXPECTED_TAR_HASH__', $TarHash)

    $SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF130 build gate failed' }
    $SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF130 build gate failed' }

    $NycBuildHash = (ssh.exe -i $Key $NYC3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'NYC3 staged binary hash read failed' }
    $AmsBuildHash = (ssh.exe -i $Key $AMS3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'AMS3 staged binary hash read failed' }
    if ($NycBuildHash -notmatch '^[0-9a-f]{64}$' -or $AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Invalid remote build hash' }
    "NYC3 tested Linux qubd SHA256: $NycBuildHash"
    "AMS3 tested Linux qubd SHA256: $AmsBuildHash"
    Write-Host 'HF130 PER-HOST VERIFIED SEED BUILD GATE: PASS'
}
```

Both hosts build the same manifest-verified source and pass their own native
tests. Each installation must match that host's recorded staged binary hash;
the helper also checks the running `/proc/PID/exe` bytes. As in HF128, the hosts
do not need identical compiler/linker environments. Equality between their
binary hashes is not claimed as a reproducibility result. Preserve both build
logs, including the printed Rust/Cargo versions and each binary hash.

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
    Write-Host 'HF130 DISTINCT SEED IDENTITY GATE: PASS'
}
```

Do not regenerate identity as part of an ordinary release. If duplicated, use
the explicit HF128 identity-recovery procedure with backups and only NYC3
offline, then repeat every gate.

## Gate 5 - NYC3 canary first

Keep AMS3 serving. The included installer validates the staged source inventory
and exact binary hash, records binary/config/identity backups, and stops each
active QUB service with a checked deadline before replacing its binary. It
restarts testnet only if testnet was active before the operation. Failure
returns nonzero and attempts recovery; inspect that recovery before proceeding.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($NycBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 4 in this session first' }
    $NycDeploy = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf130-seed.sh nyc3 __EXPECTED_BINARY_SHA256__
'@
    $NycDeploy = $NycDeploy.Replace('__EXPECTED_BINARY_SHA256__', $NycBuildHash)
    $NycDeploy | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF130 canary install failed; inspect recovery output' }
    Write-Host 'HF130 NYC3 CANARY INSTALL GATE: PASS'
}
```

Record the exact `/opt/qub/bin/backups/hf130-nyc3-*` directory printed by the
installer; never select a rollback backup with a wildcard.

Keep NYC3 canary online for at least 60 minutes and five new blocks. Sample
`status-fast` every five seconds. Height may stay flat without new blocks. Any head change must be explained
by validated chain selection; a real reorganization can lower height. A GUI
rollback to an older capture without a canonical reorganization is a failure. Compare AMS3/NYC3 cumulative-work-selected height/hash before AMS3.

## Gate 6 - AMS3 seed, publisher and separate Explorer API

### 6.1 Install AMS3 seed and publisher units

Only after the NYC3 canary passes. Check free disk space before copying the
snapshot backup or starting migration: retained generations, the new export,
staging, migration and backups coexist. Allow at least five current full-snapshot
sizes of free margin plus journal growth and the actual backup size. Measure
allocated bytes on this host; this estimate is not a universal capacity proof.
The first HF130 run imports existing public bytes. A mismatch between legacy
`canonical-chain.json` and `snapshots/chain.json` fails closed for diagnosis.
Do not delete or replace one of them to bypass that check.

The installer stops the publisher timer and waits for the publisher to stop.
It copies the entire existing snapshot tree with `cp -a`, preserving symlinks
and hidden generation directories, alongside exact binary/config/unit backups.
It installs the same checked binary separately as
`/opt/qub/bin/qubd-snapshot-export-hf130`; the publisher unit uses this versioned
exporter, which must remain available even if the seed later rolls back to
HF128. HF128's exporter does not emit the report fields required by HF130.

The publisher runs as `deploy`. The helper records and sets ownership/mode only
on `/srv/qub-updates/mainnet` and its `snapshots` directory, with no recursive
ownership change. The parent must be writable to replace canonical aliases.
It checks read access to FCE CURRENT before stopping the seed, preserves the
original directory metadata in the backup, and restores it if installation fails.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 4 in this session first' }
    $AmsDeploy = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf130-seed.sh ams3 __EXPECTED_BINARY_SHA256__
'@
    $AmsDeploy = $AmsDeploy.Replace('__EXPECTED_BINARY_SHA256__', $AmsBuildHash)
    $AmsDeploy | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF130 install failed; inspect recovery output' }
    Write-Host 'HF130 AMS3 SEED / PUBLISHER UNIT INSTALL GATE: PASS'
}
```

Record the exact `/opt/qub/bin/backups/hf130-ams3-*` directory. The snapshot timer
stays stopped until the following first-publication check. No state file,
wallet or node identity is rolled back by the installer.

Repeat the NYC3 status/version/hash/listener checks. Require AMS3 and NYC3 have
different `node_id`, independent regional configs and exactly the same selected
height/hash after allowing normal network propagation before enabling publication.

Verify the installed unit definitions are exactly the reviewed files:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsUnitVerify = @'
set -euo pipefail
systemctl cat qub-mainnet-snapshot-publish.service
systemctl cat qub-mainnet-snapshot-publish.timer
cmp /opt/jinex/staging/src/qubd/deploy/digitalocean/qub-mainnet-snapshot-publish.service /etc/systemd/system/qub-mainnet-snapshot-publish.service
cmp /opt/jinex/staging/src/qubd/deploy/digitalocean/qub-mainnet-snapshot-publish.timer /etc/systemd/system/qub-mainnet-snapshot-publish.timer
cmp /opt/jinex/staging/src/qubd/target/release/qubd /opt/qub/bin/qubd-snapshot-export-hf130
sha256sum /etc/systemd/system/qub-mainnet-snapshot-publish.{service,timer}
systemd-analyze verify /etc/systemd/system/qub-mainnet-snapshot-publish.service /etc/systemd/system/qub-mainnet-snapshot-publish.timer
'@
    $AmsUnitVerify | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 publisher unit verification failed' }
    Write-Host 'HF130 AMS3 PUBLISHER UNIT VERIFICATION: PASS'
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
PreviousGeneration=$(readlink /srv/qub-updates/mainnet/snapshots/.current || true)
sudo systemctl start qub-mainnet-snapshot-publish.service
systemctl show qub-mainnet-snapshot-publish.service -p Result -p ExecMainStatus -p ActiveEnterTimestamp -p InactiveEnterTimestamp
[ "$(systemctl show -p Result --value qub-mainnet-snapshot-publish.service)" = success ]
[ "$(systemctl show -p ExecMainStatus --value qub-mainnet-snapshot-publish.service)" = 0 ]
PublishedGeneration=$(readlink /srv/qub-updates/mainnet/snapshots/.current)
[ -n "$PublishedGeneration" ] && [ "$PublishedGeneration" != "$PreviousGeneration" ]
journalctl -u qub-mainnet-snapshot-publish.service -n 200 --no-pager
sudo systemctl enable --now qub-mainnet-snapshot-publish.timer
systemctl is-active --quiet qub-mainnet-snapshot-publish.timer
systemctl list-timers qub-mainnet-snapshot-publish.timer --all
'@
    $AmsPublisherStart | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 first publication/timer start failed' }
    Write-Host 'HF130 AMS3 FIRST PUBLICATION / TIMER START: PASS'
}
```

The publisher switches `.current` once per complete immutable generation.
The fixed public URLs and JSON formats remain available to HF128. Multiple
HTTP requests can cross a switch: compare tip identity and checksums and retry
a bounded number of times. Do not call one symlink rename a multi-request
transaction. Keep `.current`, `.generations` and the fixed symlinks intact.

For at least ten consecutive cycles require:

- `Result=success`, `ExecMainStatus=0`, no lock-contention/skip line;
- each complete cycle is below approximately 115 seconds;
- `tip.json.source == "fce"`, `recovered_from_previous == false`;
- `tip.json` height/hash exactly equals the exporter report logged by that
  publication invocation; compare live `status-fast` separately with allowed
  propagation/publication lag, since blocks can arrive between captures;
- public `published_at_unix` age always stays below 180 seconds;
- snapshot height/hash converges to AMS3 FCE within two configured cycles;
- service last/next trigger changes and timer remains active.

The 20-minute service timeout is only a kill bound. A repeated cycle over 115
seconds is NO-GO because Core stops treating the HTTP tip as fresh before the
next run.

### 6.2 Caddy cache policy

Copy the existing active Caddy configuration to a timestamped backup and a
candidate file. Merge only the snapshot cache rules from
`deploy/digitalocean/Caddyfile.qub-updates` into that candidate, preserving the
live API, Explorer, TLS and other routes. The supplied file is a route template,
not a replacement for an unknown full server configuration. Determine the
active config path from `systemctl cat caddy`; run `caddy validate --config`
against the candidate, install that candidate at the actual active path, and
reload Caddy. If reload fails, restore the exact backup and reload it. Record
both paths and the validation/reload output. Test origin and public responses:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsCaddyHeaders = @'
set -euo pipefail
# The established HF128 origin check permits the origin certificate on this
# pinned loopback connection only. No redirects or external insecure requests.
curl --noproxy '*' --connect-timeout 5 --max-time 15 -kfsSI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tip.json
curl --noproxy '*' --connect-timeout 5 --max-time 15 -kfsSI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
'@
    $AmsCaddyHeaders | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Caddy snapshot header request failed' }
}
```

`tip.json`, every tail, chain and canonical-chain URL and their sidecars must
be `no-store, max-age=0`; none may also receive bulk max-age 60. The loopback
diagnostic preserves HF128's origin-certificate handling. Public checks must
use normal certificate verification (never `-k`) and the ordinary URLs without
a query parameter. Record `Cache-Control`, `Age`, `CF-Cache-Status` and response
height/hash. A cache-busted URL is additional diagnostic evidence only.

### 6.3 Separate versioned Explorer API binary

Do **not** run `deploy/digitalocean/install-explorer-api-service.sh`: it builds
without the reviewed staged artifact and overwrites shared `/opt/qub/bin/qubd`.

Install the already-tested binary as
`/opt/qub/bin/qubd-explorer-api-hf130`, and override only the Explorer API unit.
The helper records a timestamped backup of the previous drop-in and any existing
versioned API binary, verifies the running API hash and unchanged seed PID/hash,
and attempts to restore the previous API configuration if installation fails.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 4 in this session first' }
    $AmsApiInstall = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf130-explorer-api.sh __EXPECTED_BINARY_SHA256__
'@
    $AmsApiInstall = $AmsApiInstall.Replace('__EXPECTED_BINARY_SHA256__', $AmsBuildHash)
    $AmsApiInstall | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 API install failed; inspect recovery output' }
    Write-Host 'HF130 AMS3 VERSIONED EXPLORER API INSTALL: PASS'
}
```

Gate the root and status separately-the old runbook incorrectly required
`version` from an older status schema:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsApiGate = @'
set -euo pipefail
systemctl is-active --quiet qub-explorer-api-caddy.socket
test -S /srv/qub-updates/run/qub-explorer-api.sock
curl --fail --silent --show-error --connect-timeout 2 --max-time 10 --unix-socket /srv/qub-updates/run/qub-explorer-api.sock http://localhost/api/v1/status-fast >/tmp/hf130-api-bridge-status.json
systemctl is-active --quiet qub-explorer-api-caddy.service
timeout --signal=TERM --kill-after=5s 45s python3 -I - <<'PYAPI'
import json,time,urllib.request
base='http://127.0.0.1:18765/api/v1/'
bridge=json.load(open('/tmp/hf130-api-bridge-status.json'))
assert bridge['ok'] is True and bridge['network']=='mainnet' and bridge['version']=='1.8.5'
def get(route):
    started=time.monotonic()
    with urllib.request.urlopen(base+route,timeout=2) as response:
        value=json.load(response)
        if base.startswith('https:'): assert 'no-store' in response.headers.get('Cache-Control','').lower()
    assert time.monotonic()-started<2,route+' exceeded 2s'
    return value
for base in ('http://127.0.0.1:18765/api/v1/', 'https://api.qubit-coin.io/api/v1/'):
    assert get('')['version']=='1.8.5'
    for attempt in range(5):
        before=get('status-fast'); summary=get('summary'); after=get('status-fast')
        assert before['ok'] is True and after['ok'] is True
        assert after['version']=='1.8.5' and after['network']=='mainnet'
        if (before['height'],before['tip_hash']) != (after['height'],after['tip_hash']):
            continue
        assert (summary['height'],summary['bestblockhash'])==(after['height'],after['tip_hash'])
        print('Stable API identity:',base,after['height'],after['tip_hash']); break
    else:
        raise SystemExit('No stable identity bracket in five attempts; repeat observation before marking PASS')
PYAPI
'@
    $AmsApiGate | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Explorer API root/status/summary gate failed' }
    Write-Host 'HF130 AMS3 EXPLORER API GATE: PASS'
}
```

The existing Caddy socket bridge remains installed and configured:
`qub-explorer-api-caddy.socket`, `qub-explorer-api-caddy.service` and
`/srv/qub-updates/run/qub-explorer-api.sock`. Do not replace that bridge with a
TCP-only Caddy template. The gate verifies the socket, bridge service, local API
and established public `https://api.qubit-coin.io/api/v1/` endpoint separately.

Immediately after several new blocks, require `/api/v1/summary` returns the new
height within two seconds. `/health` alone is insufficient because it does not
prove chain-route freshness. Verify the public API has `Cache-Control: no-store`
and is not served stale by a proxy/CDN.

## Gate 7 - preserve Explorer v0.7.3 and test Jinex compatibility

The bundled `explorer/public/index.html` is legacy v0.7 source. It retains HF129
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

Compatibility test the `tip.json` fields introduced in HF129 and retained by HF130 against the deployed
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
Core release at NO-GO; do not claim HF130 fixed that component.

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

## Gate 9 - Windows distribution, trust policy and public manifest

From the tested source with known-good private assets:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $BuildArgs = @{ Config = 'mainnet'; BuildInstaller = $true }
    if ($env:QUB_SIGN_CERT_THUMBPRINT) {
        $BuildArgs.Sign = $true
        $BuildArgs.CertThumbprint = $env:QUB_SIGN_CERT_THUMBPRINT
        $ReleaseVerification = @{ CertThumbprint = $env:QUB_SIGN_CERT_THUMBPRINT }
    } else {
        $BuildArgs.OfficialOriginUnsigned = $true
        $ReleaseVerification = @{ OfficialOriginUnsigned = $true }
    }
    & (Join-Path $Source 'scripts\build-windows-release.ps1') @BuildArgs
    Write-Host 'HF130 WINDOWS RELEASE BUILD: PASS'
}
```

This preserves HF128's two supported release modes. With a configured certificate,
every executable and installer must have a valid signature from that exact
thumbprint. Without one, this block explicitly selects `OfficialOriginUnsigned`;
every executable and installer must be `NotSigned`. An invalid, untrusted or
unexpected signature never falls back to unsigned mode. Both modes require the
fixed official HTTPS origin, exact hashes/sizes, version metadata and updater
smoke tests. No certificate purchase or WSL installation is required.

The helper finds Inno Setup in PATH, either Program Files location, or the
established per-user LocalAppData location, before starting Cargo. Native tests
and preflight remain required. Preflight's disposable chain state stays outside
source and dist. The helper builds fresh executables, applies the selected trust
policy, and creates the seven channel files only after successful build/installer
checks. Keep `$ReleaseVerification` for the remaining blocks in this session.

Independently verify the channel files and all three bundle executables:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $UpdateDir = Join-Path $Source 'dist\updates\mainnet\windows-x64'
    $BundleDir = Join-Path $Source 'dist\QUB-Core-v1.8.5-windows-x64-mainnet'
    if ($ReleaseVerification -isnot [hashtable]) { throw 'Run the Windows release build block in this session first.' }
    & (Join-Path $Source 'scripts\verify-hf130-windows-release.ps1') -Directory $UpdateDir -BundleDirectory $BundleDir @ReleaseVerification
    $Files = @(Get-ChildItem -LiteralPath $UpdateDir -File | Sort-Object Name)
    $PublicStage = '/tmp/qub-hf130-windows-' + [guid]::NewGuid().ToString('N')
    $PrepareUpload = @'
set -euo pipefail
mkdir -m0700 __PUBLIC_STAGE__
'@
    $PrepareUpload = $PrepareUpload.Replace('__PUBLIC_STAGE__', $PublicStage)
    $PrepareUpload | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Creating fresh AMS3 Windows stage failed' }
    foreach ($File in $Files) {
        scp.exe -i $Key $File.FullName "${AMS3}:$PublicStage/"
        if ($LASTEXITCODE) { throw "Windows upload failed: $($File.Name)" }
    }
    Write-Host 'HF130 VERIFIED WINDOWS FILES UPLOADED; PUBLIC MANIFEST UNCHANGED'
}
```

The following block publishes the release. The helper revalidates all uploaded
bytes and unchanged consensus metadata, saves the exact previous channel and
root-Latest files, stages each payload on the destination filesystem, and
renames `manifest.json` last. It rejects different bytes already present under
the versioned v1.8.5 installer URL. `Latest` URLs can change before the manifest;
manifest clients use its immutable versioned `url`. Neither checksums nor a
PE header replaces the preceding Windows trust-policy verification.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($PublicStage -notmatch '^/tmp/qub-hf130-windows-[0-9a-f]{32}$') { throw 'Run the verified Windows upload block first' }
    $PublishWindows = @'
set -euo pipefail
SRC=/opt/jinex/staging/src/qubd
python3 "$SRC/scripts/test-hf130-source-contracts.py" --mode checkout --root "$SRC"
sudo -n bash "$SRC/deploy/digitalocean/publish-hf130-windows-release.sh" __PUBLIC_STAGE__
'@
    $PublishWindows = $PublishWindows.Replace('__PUBLIC_STAGE__', $PublicStage)
    $PublishWindows | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Windows publication failed; use the printed backup and inspect manifest state' }
    Write-Host 'HF130 WINDOWS ORIGIN PUBLICATION GATE: PASS'
}
```

As in the working HF128 procedure, use Cloudflare Custom Purge for the exact
release URLs below after origin publication. Snapshot paths include their
sidecars because HF130 publishes both. Preserve the previous v1.8.4 versioned
installer on origin for older manifests and rollback; do not delete it.

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe.sha256
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/snapshots/tip.json
https://download.qubit-coin.io/mainnet/snapshots/tip.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
https://download.qubit-coin.io/mainnet/snapshots/tail-64.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-256.json
https://download.qubit-coin.io/mainnet/snapshots/tail-256.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json
https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json
https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json
https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/chain.json
https://download.qubit-coin.io/mainnet/snapshots/chain.json.sha256
https://download.qubit-coin.io/mainnet/canonical-chain.json
https://download.qubit-coin.io/mainnet/canonical-chain.json.sha256
```

Verify actual public files using a fresh directory outside source. This check
uses the ordinary URLs consumed by clients, with normal TLS verification and no
cache-busting query. If stale bytes remain, repeat the targeted purge and this
entire check; a nonce-only success is insufficient. Repeat from a second network
before announcement. Headers are retained alongside the evidence.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $PublicEvidence = Join-Path $ProjectRoot ('_hf130_public_' + [guid]::NewGuid().ToString('N'))
    $PublicDownloads = Join-Path $PublicEvidence 'files'
    New-Item -ItemType Directory -Path $PublicDownloads -Force | Out-Null
    $Base = 'https://download.qubit-coin.io/mainnet/windows-x64'
    foreach ($File in @(Get-ChildItem -LiteralPath $UpdateDir -File)) {
        $Downloaded = Join-Path $PublicDownloads $File.Name
        $Headers = Join-Path $PublicEvidence ($File.Name + '.headers.txt')
        curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 300 -D $Headers "$Base/$($File.Name)" -o $Downloaded
        if ($LASTEXITCODE) { throw "Public download failed: $($File.Name)" }
        if ((Get-FileHash -LiteralPath $Downloaded -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $File.FullName -Algorithm SHA256).Hash) { throw "Public bytes differ from the verified local release: $($File.Name)" }
    }
    & (Join-Path $Source 'scripts\verify-hf130-windows-release.ps1') -Directory $PublicDownloads @ReleaseVerification
    $RootLatest = Join-Path $PublicEvidence 'root-QUB-Core-Latest.exe'
    curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 300 -D (Join-Path $PublicEvidence 'root-Latest.headers.txt') 'https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe' -o $RootLatest
    if ($LASTEXITCODE) { throw 'Root Latest public download failed' }
    if ((Get-FileHash -LiteralPath $RootLatest -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath (Join-Path $UpdateDir 'QUB-Core-Latest.exe') -Algorithm SHA256).Hash) { throw 'Public root Latest differs from the released installer' }
    $RootSidecar = Join-Path $PublicEvidence 'root-QUB-Core-Latest.exe.sha256'
    curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 30 'https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256' -o $RootSidecar
    if ($LASTEXITCODE) { throw 'Root Latest public sidecar download failed' }
    if ((Get-Content -LiteralPath $RootSidecar -Raw -Encoding ASCII).Trim() -cne (Get-FileHash -LiteralPath $RootLatest -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Public root Latest sidecar mismatch' }
    Write-Host "HF130 PUBLIC WINDOWS BYTES / TRUST POLICY GATE: PASS - $PublicEvidence"
}
```

Perform both:

- clean install on a fresh Windows profile/device;
- update an actual HF128 v1.8.4 installation through the public manifest.

Require bounded download, size/SHA256/ProductVersion/selected trust verification,
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
    $Zip = Join-Path $ProjectRoot 'HF130-v185-source-no-assets-r7.zip'
    $Sums = Join-Path $ProjectRoot 'HF130-v185-r7-SHA256SUMS.txt'
    $Extract = Join-Path $ProjectRoot ('_hf130_repo_extract_' + [guid]::NewGuid().ToString('N'))
    $Sync = Join-Path $ProjectRoot ('_repo_sync_hf130_' + [guid]::NewGuid().ToString('N'))

    if (-not (Test-Path (Join-Path $Repo '.git') -PathType Container)) { throw "Repo missing: $Repo" }
    if (-not (Test-Path $Zip -PathType Leaf)) { throw "HF130 source ZIP missing: $Zip" }
    if (-not (Test-Path $Sums -PathType Leaf)) { throw "HF130 source sums missing: $Sums" }

    $SumLines = @(Get-Content $Sums -Encoding ASCII | Where-Object { $_ -match '^[0-9a-fA-F]{64}  HF130-v185-source-no-assets-r7\.zip$' })
    if ($SumLines.Count -ne 1) { throw 'Need one exact HF130 ZIP checksum entry' }
    $Expected = ($SumLines[0] -split '  ')[0].ToLowerInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw 'HF130 source ZIP hash mismatch before repo update' }

    Set-Location $Repo
    $InitialStatus = @(git status --porcelain)
    if ($LASTEXITCODE) { throw 'git status failed' }
    if ($InitialStatus.Count -gt 0) { throw 'Repository has uncommitted changes' }
    git fetch origin --tags --prune
    if ($LASTEXITCODE) { throw 'git fetch failed' }
    git checkout main
    if ($LASTEXITCODE) { throw 'git checkout main failed' }
    git pull --ff-only origin main
    if ($LASTEXITCODE) { throw 'git pull --ff-only failed' }

    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract
    $SourceRoot = Join-Path $Extract 'qubd-v1.8.5'
    if (-not (Test-Path (Join-Path $SourceRoot 'Cargo.toml') -PathType Leaf)) { throw 'HF130 ZIP root missing' }

    python (Join-Path $SourceRoot 'scripts\test-hf130-source-contracts.py') --mode archive --root $SourceRoot
    if ($LASTEXITCODE) { throw 'HF130 extracted source contract failed before repo mirror' }

    New-Item -ItemType Directory -Force $Sync | Out-Null
    robocopy.exe $SourceRoot $Sync /MIR /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Source mirror failed: $LASTEXITCODE" }

    # Preserve repository governance/workflow files only when not owned by the
    # release inventory. The checkout contract checks these auxiliary files;
    # manifest-only exports intentionally exclude them.
    foreach ($Name in @('LICENSE','LICENSE.md','LICENSE.txt','NOTICE','NOTICE.md','NOTICE.txt','SECURITY.md','CONTRIBUTING.md','CODE_OF_CONDUCT.md')) {
        if ((Test-Path (Join-Path $Repo $Name)) -and -not (Test-Path (Join-Path $SourceRoot $Name))) {
            Copy-Item -LiteralPath (Join-Path $Repo $Name) -Destination (Join-Path $Sync $Name)
        }
    }
    if ((Test-Path (Join-Path $Repo '.github')) -and -not (Test-Path (Join-Path $SourceRoot '.github'))) {
        Copy-Item -LiteralPath (Join-Path $Repo '.github') -Destination (Join-Path $Sync '.github') -Recurse
    }
    python (Join-Path $Sync 'scripts\test-hf130-source-contracts.py') --mode checkout --root $Sync
    if ($LASTEXITCODE) { throw 'Exact source plus preserved governance failed before repository mirror' }
    robocopy.exe $Sync $Repo /MIR /XD (Join-Path $Repo '.git') /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Repository mirror failed: $LASTEXITCODE" }

    Set-Location $Repo
    python .\scripts\test-hf130-source-contracts.py --mode checkout --root $Repo
    if ($LASTEXITCODE) { throw 'Repository source contract failed' }
    cargo check --locked --all-targets
    if ($LASTEXITCODE) { throw 'Repository cargo check failed' }
    cargo test --locked --all-targets
    if ($LASTEXITCODE) { throw 'Repository cargo test failed' }
    git diff --check
    if ($LASTEXITCODE) { throw 'git diff --check failed' }

    $Changes = @(git status --porcelain)
    if ($LASTEXITCODE) { throw 'git status failed' }
    $Changes
    if ($Changes.Count -eq 0) { throw 'Repository has no HF130 changes; do not create a tag' }
    git diff --stat
    if ($LASTEXITCODE) { throw 'git diff --stat failed' }
    git diff -- src\pools.rs src\lib.rs src\p2p.rs
    if ($LASTEXITCODE) { throw 'git source diff failed' }

    git add -A
    if ($LASTEXITCODE) { throw 'git add failed' }
    git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
    if ($LASTEXITCODE) { throw 'git publisher mode update failed' }
    git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh deploy/digitalocean/install-hf130-seed.sh deploy/digitalocean/install-hf130-explorer-api.sh deploy/digitalocean/publish-hf130-windows-release.sh deploy/digitalocean/rollback-hf130-seed.sh
    if ($LASTEXITCODE) { throw 'git publisher test mode update failed' }
    git diff --cached --check
    if ($LASTEXITCODE) { throw 'Staged diff check failed' }
    $Staged = @(git diff --cached --name-status)
    if ($LASTEXITCODE) { throw 'git staged file list failed' }
    $Staged
    foreach ($Required in @('HF130-SOURCE-MANIFEST.json','HF130-v185-DEPLOY-RUNBOOK.md','RELEASE_NOTES-v1.8.5-HF130.md')) {
        if (-not (Test-Path (Join-Path $Repo $Required) -PathType Leaf)) { throw "Missing release file: $Required" }
    }

    git commit -m 'Update QUB Core to v1.8.5 HF130'
    if ($LASTEXITCODE) { throw 'HF130 commit failed' }
    $Commit = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE) { throw 'git revision read failed' }
    git push origin main
    if ($LASTEXITCODE) { throw 'HF130 main push failed' }
    git fetch origin main
    if ($LASTEXITCODE) { throw 'git remote confirmation fetch failed' }
    $RemoteCommit = (git rev-parse origin/main).Trim()
    if ($LASTEXITCODE) { throw 'git remote revision read failed' }
    if ($RemoteCommit -ne $Commit) { throw 'Remote main does not match the HF130 commit' }

    $ExistingTag = @(git tag -l 'v1.8.5-hf130')
    if ($LASTEXITCODE) { throw 'git local tag lookup failed' }
    if ($ExistingTag.Count) { throw 'Local tag v1.8.5-hf130 already exists; never move a release tag' }
    $RemoteTag = git ls-remote --tags origin 'refs/tags/v1.8.5-hf130'
    if ($LASTEXITCODE) { throw 'git remote tag lookup failed' }
    if ($RemoteTag) { throw 'Remote tag v1.8.5-hf130 already exists; never move a release tag' }
    git tag -a v1.8.5-hf130 -m 'QUB Core v1.8.5 HF130' $Commit
    if ($LASTEXITCODE) { throw 'HF130 annotated tag creation failed' }
    git push origin v1.8.5-hf130
    if ($LASTEXITCODE) { throw 'HF130 tag push failed' }

    Write-Host "HF130 REPO/TAG GATE: PASS - $Commit"
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
        (Join-Path $ProjectRoot 'HF130-v185-source-no-assets-r7.zip'),
        (Join-Path $ProjectRoot 'HF130-v185-r7-SHA256SUMS.txt'),
        $Installer,
        $InstallerSidecar,
        (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF130.md')
    )) {
        if (-not (Test-Path $Required -PathType Leaf)) { throw "Release attachment missing: $Required" }
    }

    gh release create v1.8.5-hf130 `
        (Join-Path $ProjectRoot 'HF130-v185-source-no-assets-r7.zip') `
        (Join-Path $ProjectRoot 'HF130-v185-r7-SHA256SUMS.txt') `
        $Installer `
        $InstallerSidecar `
        --repo AlxProe/qub-core `
        --title 'QUB Core v1.8.5 HF130 - Canonical Freshness, Sync & Mining Resume Reliability' `
        --notes-file (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF130.md')
    if ($LASTEXITCODE) { throw 'GitHub release creation failed' }
    Write-Host 'HF130 GITHUB RELEASE: PASS'
}
```

## Gate 11 - Telegram post

Post `HF130-v185-TELEGRAM-POST.txt` only after the merged/tagged release and all
public download checks succeed. Replace its checksum placeholders with the
actual published SHA256 values. Verify every link in an incognito browser and
from a second network before posting.

## Post-release monitoring

For the first 24 hours, continue Gate 8 sampling and alert on:

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

HF130 introduces no consensus activation. Restore binaries and operational
configuration while retaining the current canonical data and independent seed
identities. Use only the exact timestamped paths recorded during installation.
A failed install's recovery message is an attempt, not a rollback PASS: verify
the running binary hash, services and tip after it returns.

1. Restore the previous public Windows manifest first, from the exact
   `/opt/qub/releases/windows-before-hf130-*` backup printed at publication.
   Copy it to a temporary file inside `/srv/qub-updates/mainnet/windows-x64`,
   then rename it to `manifest.json`. Restore its sidecar and previous Latest
   installers/sidecars from the same backup. Preserve versioned installers so
   clients that already read either manifest can finish a matching download.
2. Stop the snapshot timer while changing the seed. For a seed-only rollback,
   KEEP `/opt/qub/bin/qubd-snapshot-export-hf130`, the HF130 publisher script/unit,
   and the complete managed snapshot tree. HF128's exporter lacks the strict
   report fields the new publisher requires. Do not point the publisher at the
   rolled-back `/opt/qub/bin/qubd`.
3. Roll back NYC3 first using `qubd` and `mainnet-seed.toml` from its exact seed
   backup, with mainnet and any previously active testnet process stopped and
   verified absent before installation. Restart only previously active services,
   verify running `/proc/PID/exe` hash and unchanged `node_id`, then prove healthy.
4. Roll back AMS3 by the same procedure after NYC3 is healthy. Keep the versioned
   snapshot exporter. Resume the HF130 publisher through systemd, require a full
   successful cycle and public freshness, then enable its timer again.
5. If the separate Explorer API is the failing layer, stop only its unit; restore
   the previous `hf130.conf` from the exact API backup (or remove this one drop-in
   if the backup says it did not exist), restore the previous versioned API binary
   if one was backed up, daemon-reload and restart that API unit. Earlier drop-ins
   remain intact. Verify the seed PID/hash did not change.
6. A FULL publisher rollback is a separate operation: stop timer and service,
   retain the current whole managed tree as evidence, restore the complete
   pre-migration `snapshots` directory and canonical aliases/sidecars with
   symlinks preserved from the SAME backup, then restore the old publisher
   script/unit/timer and any prior Caddy cache rules. Never run the old flat-file
   publisher over HF130's managed symlinks and never restore individual files
   from different generations. If the backup is missing or inconsistent, stop
   publication and diagnose; do not fabricate a replacement chain snapshot.

A seed-only rollback can be executed from the same Windows session using the
included helper. Its backup argument must be the exact path printed for that
region; it does not touch snapshot generations, wallets or chain state.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $Region = 'nyc3'
    $RollbackHost = $NYC3
    $SeedBackup = Read-Host 'Paste the exact NYC3 seed backup path printed by Gate 5'
    if ($SeedBackup -notmatch '^/opt/qub/bin/backups/hf130-nyc3-[0-9]{8}-[0-9]{6}-[0-9]+$') { throw 'Invalid NYC3 backup path' }
    $RollbackSeed = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/rollback-hf130-seed.sh __REGION__ __BACKUP__
'@
    $RollbackSeed = $RollbackSeed.Replace('__REGION__', $Region).Replace('__BACKUP__', $SeedBackup)
    $RollbackSeed | ssh.exe -i $Key $RollbackHost "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Seed rollback failed; inspect services and exact backup' }
}
```

After NYC3 is verified healthy, repeat this block for AMS3 with `$Region='ams3'`,
`$RollbackHost=$AMS3`, the exact AMS3 backup, and `hf130-ams3-` in the path check.
The helper resumes a previously active snapshot timer only after a successful
manual publication using the retained HF130 exporter.

Rollback is complete only when the old public manifest is visible, both seed
listeners are healthy with the expected binary hashes, RPC is not public,
independent seed IDs are unchanged, the API is read-only and responsive,
publisher/timer is healthy, and Core/Explorer/Jinex tips advance again. Preserve
all chain-v2 generations, wallets, node IDs, peer state and durable outboxes.

## Evidence record

For every gate archive: UTC start/end, operator, host, source/installer SHA256,
Rust/toolchain version, exact commands/output, seed IDs, height/hash samples,
publisher cycle durations, public headers, Explorer/Jinex provenance, Windows
signature evidence, backups and rollback rehearsal result. A checklist without
command output is not release evidence.
