param(
    [ValidateSet('mainnet')]
    [string]$Config = 'mainnet',
    [switch]$SkipTests,
    [switch]$Sign,
    [string]$CertSubject = '',
    [string]$CertThumbprint = '',
    [string]$TimestampUrl = 'http://timestamp.digicert.com',
    [switch]$BuildInstaller,
    [switch]$SkipPreflight,
    [switch]$OfficialOriginUnsigned,
    [string]$NativeReceipt = '',
    [string]$DistRoot = ''
)

throw 'HF134 candidate: historical publisher disabled. Run the HF134 native verification package first; release packaging and publication require separate verified receipts.'

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$Root = Split-Path -Parent $PSScriptRoot
Set-Location $Root
if ($SkipTests -or $SkipPreflight) { throw 'HF132 requires native evidence and shipping preflight. Reuse verified evidence with -NativeReceipt; skip switches are not supported.' }
if (-not $DistRoot) { $DistRoot = Join-Path $Root 'dist' }
if (-not [IO.Path]::IsPathRooted($DistRoot)) { throw '-DistRoot must be an absolute path.' }
$DistRoot = [IO.Path]::GetFullPath($DistRoot)
if (Test-Path -LiteralPath $DistRoot) { throw "Distribution destination already exists; use a new empty -DistRoot path. Nothing was removed: $DistRoot" }

$Version = '1.8.5'
$Platform = 'windows-x64'
$UpdateBaseUrl = 'https://download.qubit-coin.io'
$env:QUB_BUILD_CONFIG = $Config

$RequiredReleaseDocs = @(
    'RELEASE_NOTES-v1.8.5-HF132.md',
    'HF132-v185-RELIABILITY-REVIEW.md',
    'HF132-v185-DEPLOY-RUNBOOK.md',
    'HF132-v185-TEST-REPORT.md'
)
foreach ($Doc in $RequiredReleaseDocs) {
    if (-not (Test-Path (Join-Path $Root $Doc) -PathType Leaf)) {
        throw "Required HF132 release document missing before build: $Doc"
    }
}

function Assert-NativeSuccess {
    param(
        [Parameter(Mandatory)][string]$Operation,
        [Parameter(Mandatory)][int]$ExitCode
    )
    if ($ExitCode -ne 0) {
        throw "$Operation failed with native exit code $ExitCode"
    }
}

# Avoid VCRUNTIME140.dll deployment failures on clean Windows installs.
# .cargo/config.toml enables crt-static. Keep this explicit for direct script builds,
# but avoid duplicating the flag when the caller already set it.
if ($env:CARGO_ENCODED_RUSTFLAGS) { throw 'CARGO_ENCODED_RUSTFLAGS overrides RUSTFLAGS; unset it before this release build.' }
if (($env:RUSTFLAGS -as [string]) -match '-crt-static') { throw 'RUSTFLAGS disables the required static Windows CRT.' }
if (($env:RUSTFLAGS -as [string]) -notmatch '\+crt-static') {
    $env:RUSTFLAGS = (($env:RUSTFLAGS, '-C target-feature=+crt-static') -join ' ').Trim()
}
$env:CARGO_BUILD_TARGET = 'x86_64-pc-windows-msvc'
if (-not $env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR = Join-Path $Root 'target' }
if (-not [IO.Path]::IsPathRooted($env:CARGO_TARGET_DIR)) { $env:CARGO_TARGET_DIR = Join-Path $Root $env:CARGO_TARGET_DIR }
$env:CARGO_TARGET_DIR = [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)

if ($OfficialOriginUnsigned -and ($Sign -or $Config -ne 'mainnet' -or -not $BuildInstaller)) {
    throw '-OfficialOriginUnsigned is only for an unsigned mainnet installer; it cannot be combined with -Sign.'
}
if ($Config -eq 'mainnet' -and $BuildInstaller) {
    if (-not $Sign -and -not $OfficialOriginUnsigned) { throw 'Select -Sign with an exact certificate thumbprint, or the established -OfficialOriginUnsigned mainnet policy.' }
    if ($SkipTests -or $SkipPreflight) { throw 'Public mainnet installer cannot skip tests or preflight.' }
    if ($Sign -and $CertThumbprint -notmatch '^[0-9a-fA-F]{40}$') { throw 'Public mainnet signing requires the exact 40-digit certificate thumbprint.' }
    if ($OfficialOriginUnsigned -and ($CertThumbprint -or $CertSubject)) { throw 'Unsigned mode cannot include a signing certificate selector.' }
}

function Find-InnoSetup {
    $Command = Get-Command 'iscc.exe' -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($Command) { return $Command.Source }
    foreach ($Candidate in @(
        "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe",
        "$env:ProgramFiles\Inno Setup 6\ISCC.exe",
        "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
    )) {
        if (Test-Path -LiteralPath $Candidate -PathType Leaf) { return $Candidate }
    }
    throw 'Inno Setup 6 compiler not found in PATH or the established machine/user installation paths.'
}

# Resolve the existing compiler before lengthy Cargo work. No WSL dependency.
$InnoSetupCompiler = $null
if ($BuildInstaller) { $InnoSetupCompiler = Find-InnoSetup }
if (-not (Test-Path -LiteralPath (Join-Path $Root 'assets\qubit-coin-logo.png') -PathType Leaf)) { throw 'Restore known-good runtime assets first: qubit-coin-logo.png is required for Windows resource/version metadata.' }
$NativeHelper = Join-Path $Root 'scripts\hf132-native-build.py'
if ($NativeReceipt) {
    python $NativeHelper verify --source $Root --receipt $NativeReceipt
    Assert-NativeSuccess -Operation 'HF132 existing native evidence verification' -ExitCode $LASTEXITCODE
} else {
    $EvidenceRoot = Join-Path (Split-Path -Parent $Root) ('hf132-native-' + [guid]::NewGuid().ToString('N'))
    $NativeReceipt = Join-Path $EvidenceRoot 'native-build.json'
    python $NativeHelper build --source $Root --receipt $NativeReceipt
    Assert-NativeSuccess -Operation 'HF132 native build and required tests' -ExitCode $LASTEXITCODE
}
$NativeReceipt = (Resolve-Path -LiteralPath $NativeReceipt).Path
$NativeEvidence = Get-Content -LiteralPath $NativeReceipt -Raw -Encoding UTF8 | ConvertFrom-Json
$ReleaseDir = Join-Path $env:CARGO_TARGET_DIR 'x86_64-pc-windows-msvc\release'
$ExpectedReleaseExecutables = @(
    (Join-Path $ReleaseDir 'qub-core.exe'),
    (Join-Path $ReleaseDir 'qubd.exe'),
    (Join-Path $ReleaseDir 'qub-rpc-miner.exe')
)
foreach ($Exe in $ExpectedReleaseExecutables) {
    if (-not (Test-Path -LiteralPath $Exe -PathType Leaf)) { throw "Verified native output missing: $Exe" }
}

function Find-SignTool {
    $candidates = @(
        "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe",
        "$env:ProgramFiles\Windows Kits\10\bin\*\x64\signtool.exe"
    )
    foreach ($pattern in $candidates) {
        $match = Get-ChildItem $pattern -ErrorAction SilentlyContinue | Sort-Object FullName -Descending | Select-Object -First 1
        if ($match) { return $match.FullName }
    }
    return (Get-Command 'signtool.exe' -ErrorAction Stop).Source
}

# Reserve the destination without -Force. A concurrent creation is a failure,
# never permission to merge into or replace an existing distribution.
New-Item -ItemType Directory -Path $DistRoot -ErrorAction Stop | Out-Null
$Dist = Join-Path $DistRoot "QUB-Core-v$Version-$Platform-$Config"
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dist 'config') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dist 'assets') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dist 'assets\fonts') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dist 'assets\flags') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $Dist 'tools') | Out-Null
New-Item -ItemType Directory -Force -Path (Join-Path $DistRoot 'updates') | Out-Null
$ChannelUpdateDir = Join-Path $DistRoot "updates\$Config\windows-x64"
New-Item -ItemType Directory -Force -Path $ChannelUpdateDir | Out-Null

Copy-Item (Join-Path $ReleaseDir 'qub-core.exe') (Join-Path $Dist 'QUB-Core.exe')
Copy-Item (Join-Path $ReleaseDir 'qubd.exe') (Join-Path $Dist 'tools\qubd.exe')
Copy-Item (Join-Path $ReleaseDir 'qub-rpc-miner.exe') (Join-Path $Dist 'tools\qub-rpc-miner.exe')
# Confirm the copied bytes, not only the earlier shared-cache verification.
$CopiedExecutables = @{
    'qub-core.exe' = (Join-Path $Dist 'QUB-Core.exe')
    'qubd.exe' = (Join-Path $Dist 'tools\qubd.exe')
    'qub-rpc-miner.exe' = (Join-Path $Dist 'tools\qub-rpc-miner.exe')
}
foreach ($Name in $CopiedExecutables.Keys) {
    $Entry = $NativeEvidence.binaries.PSObject.Properties[$Name].Value
    if ((Get-FileHash -LiteralPath $CopiedExecutables[$Name] -Algorithm SHA256).Hash.ToLowerInvariant() -cne $Entry.sha256) {
        throw "Copied distribution executable differs from verified native evidence: $Name"
    }
}
Copy-Item (Join-Path $Root 'config\*.toml') (Join-Path $Dist 'config')
if (Test-Path (Join-Path $Root 'README-MINER-WINDOWS.md')) { Copy-Item (Join-Path $Root 'README-MINER-WINDOWS.md') $Dist }
# End-user documentation only. Historical deploy/runbook evidence stays in source.
foreach ($Doc in @('README-FAST-CHAIN-ENGINE.md', 'README-RPC-MINER.md', 'RELEASE_NOTES-v1.8.5-HF132.md')) {
    $DocPath = Join-Path $Root $Doc
    if (Test-Path -LiteralPath $DocPath -PathType Leaf) { Copy-Item -LiteralPath $DocPath -Destination $Dist }
}
foreach ($LegalFile in @(Get-ChildItem -LiteralPath $Root -File | Where-Object { $_.Name -match '^(LICENSE|COPYING|NOTICE)([.-].*)?$' })) {
    Copy-Item -LiteralPath $LegalFile.FullName -Destination $Dist
}
$AssetsReadme = Join-Path $Root 'assets\README.md'
if (Test-Path $AssetsReadme) {
    Copy-Item $AssetsReadme (Join-Path $Dist 'assets')
} else {
    @"
QUB Core runtime assets

Optional files copied into release bundles when present:
- qubit-coin-logo.png
- jin-coin-logo.png
- mined.mp3
- network-mined.mp3
- mining-on.mp3
- mining-off.png
- mining-off-white.png
- mining-prep.gif
- mining-prep-white.gif
- mining-on.gif
- mining-on-white.gif
- fonts/Ubuntu-BoldItalic.ttf

This fallback README was generated by the release build script because source assets/README.md was missing.
"@ | Set-Content -Encoding UTF8 -Path (Join-Path $Dist 'assets\README.md')
}

foreach ($Asset in @(
    'qubit-coin-logo.png',
    'opening-banner.png',
    'jin-coin-logo.png',
    'mined.mp3',
    'network-mined.mp3',
    'mining-on.mp3',
    'mining-off.png',
    'mining-off-white.png',
    'mining-prep.gif',
    'mining-prep-white.gif',
    'mining-on.gif',
    'mining-on-white.gif',
    'online.png',
    'offline.png',
    'mining-controls.png',
    'mining-controls-white.png',
    'start-mining.png',
    'stop-mining.png',
    'cpu.png',
    'cpu-white.png',
    'gpu.png',
    'gpu-white.png',
    'wallet-address.png',
    'wallet-address-white.png',
    'send.png',
    'success.png',
    'success-white.png',
    'failed.png',
    'failed-white.png',
    'qub.png',
    'jin.png',
    'register-qub.png',
    'danger-zone.png',
    'delete-local-private-keys.png',
    'benchmark.png',
    'benchmark-white.png',
    'settings.png',
    'settings-white.png',
    'dark.png',
    'dark-white.png',
    'light.png',
    'light-white.png',
    'sync.png',
    'qr.png',
    'qr-white.png',
    'qr-scan.png',
    'qr-scan-white.png',
    'live-chain-data.png',
    'live-chain-data-white.png',
    'network.png',
    'network-white.png',
    'height.png',
    'height-white.png',
    'best-block.png',
    'best-block-white.png',
    'mempool-tx.png',
    'mempool-tx-white.png',
    'spendable.png',
    'immature.png',
    'immature-white.png',
    'total-wallet.png',
    'total-wallet-white.png',
    'wallet-keys.png',
    'wallet-keys-white.png',
    'coinbase-maturity.png',
    'coinbase-maturity-white.png',
    'block-target.png',
    'block-target-white.png',
    'next-reward.png',
    'next-reward-white.png',
    'halving-interval.png',
    'halving-interval-white.png',
    'pow-bits.png',
    'pow-bits-white.png',
    'p2p-peers.png',
    'p2p-peers-white.png',
    'qns.png',
    'qns-white.png',
    'data-dir.png',
    'data-dir-white.png',
    'miner-telemetry.png',
    'miner-telemetry-white.png',
    'hashrate.png',
    'hashrate-white.png',
    'total-hashes.png',
    'total-hashes-white.png',
    'cpu-workers.png',
    'cpu-workers-white.png',
    'duty.png',
    'duty-white.png',
    'target-block.png',
    'target-block-white.png',
    'peers-block-stream.png',
    'peers-block-stream-white.png',
    'web-map.png',
    'web-map-white.png',
    'list.png',
    'list-white.png',
    'peers-other-miners.png',
    'peers-other-miners-white.png',
    'recent-global-blocks.png',
    'recent-global-blocks-white.png',
    'your-pending-mined-block.png',
    'your-pending-mined-block-white.png',
    'your-confirmed-mined-block.png',
    'your-confirmed-mined-block-white.png',
    'your-confirmed-mined-block.png.png',
    'your-confirmed-mined-block-white.png.png',
    'updates-available.png',
    'check-now.png',
    'install-and-restart.png',
    'i.png',
    'i-white.png',
    'address-balances.png',
    'address-balances-white.png',
    'crypto.png',
    'crypto-white.png',
    'import-private-key.png',
    'import-private-key-white.png',
    'buy.png',
    'buy-white.png',
    'sell.png',
    'sell-white.png',
    'offer.png',
    'offer-white.png',
    'melt.png',
    'melt-white.png',
    'infuse.png',
    'infuse-white.png',
    'convert.png',
    'convert-white.png',
    'jin-token.png',
    'jin-token-white.png',
    'auction.png',
    'auction-white.png'
)) {
    $Path = Join-Path $Root "assets\$Asset"
    if (Test-Path $Path) { Copy-Item $Path (Join-Path $Dist 'assets') }
}


# Hotfix37 extra GUI assets.
# Duplicates are harmless; SHA256SUMS.txt is generated from actual files in dist.
foreach ($Asset in @(
    'i.png',
    'i-white.png',
    'address-balances.png',
    'address-balances-white.png',
    'crypto.png',
    'crypto-white.png',
    'import-private-key.png',
    'import-private-key-white.png',
    'buy.png',
    'buy-white.png',
    'sell.png',
    'sell-white.png',
    'offer.png',
    'offer-white.png',
    'melt.png',
    'melt-white.png',
    'infuse.png',
    'infuse-white.png',
    'convert.png',
    'convert-white.png',
    'jin-token.png',
    'jin-token-white.png',
    'auction.png',
    'auction-white.png',
    'list-asset.png',
    'list-asset-white.png'
)) {
    $Path = Join-Path $Root "assets\$Asset"
    if (Test-Path $Path) { Copy-Item -Force $Path (Join-Path $Dist 'assets') }
}

# HF123/v1.8.0: copy every runtime asset that exists, not only the legacy allow-list.
# This prevents new bridge/UI icons (eth/usdt/usdc/usdj/enj/subscan/nftio/verified/etc.)
# from appearing in local smoke but missing from live update bundles.
$AssetRoot = Join-Path $Root 'assets'
if (Test-Path $AssetRoot) {
    Get-ChildItem $AssetRoot -Recurse -File | Where-Object {
        $_.Extension.ToLowerInvariant() -in @('.png','.webp','.gif','.mp3','.ttf','.otf','.md')
    } | ForEach-Object {
        $rel = $_.FullName.Substring($AssetRoot.Length).TrimStart('\','/')
        $dest = Join-Path (Join-Path $Dist 'assets') $rel
        $parent = Split-Path -Parent $dest
        if (-not (Test-Path $parent)) { New-Item -ItemType Directory -Force -Path $parent | Out-Null }
        Copy-Item -Force $_.FullName $dest
    }
}

foreach ($Flag in @('en-US.png','el-GR.png','fr-FR.png','de-DE.png','es-ES.png','zh-CN.png','ja-JP.png','ko-KR.png')) {
    $FlagPath = Join-Path $Root "assets\flags\$Flag"
    if (Test-Path $FlagPath) { Copy-Item $FlagPath (Join-Path $Dist 'assets\flags') }
}

$Font = Join-Path $Root 'assets\fonts\Ubuntu-BoldItalic.ttf'
if (Test-Path $Font) { Copy-Item $Font (Join-Path $Dist 'assets\fonts') }

$Cli = @"
@echo off
set "QUB_HOME=%~dp0.."
cd /d "%QUB_HOME%"
"%~dp0qubd.exe" --config "%QUB_HOME%\config\$Config.toml" %*
"@
Set-Content -Encoding ASCII -Path (Join-Path $Dist 'tools\qubd-cli.cmd') -Value $Cli

$SeedLauncher = @"
@echo off
set "QUB_HOME=%~dp0.."
cd /d "%QUB_HOME%"
"%~dp0qubd.exe" --config "%QUB_HOME%\config\$Config.toml" node
"@
Set-Content -Encoding ASCII -Path (Join-Path $Dist 'tools\Start-QUB-Seed-Node.cmd') -Value $SeedLauncher

$SyncOnce = @"
@echo off
set "QUB_HOME=%~dp0.."
cd /d "%QUB_HOME%"
"%~dp0qubd.exe" --config "%QUB_HOME%\config\$Config.toml" sync
"@
Set-Content -Encoding ASCII -Path (Join-Path $Dist 'tools\Sync-QUB-Once.cmd') -Value $SyncOnce

& {
    $PreflightConfig = Join-Path $Dist "config\$Config.toml"
    # preflight calls load_or_init_chain; isolate its relative data_dir so it
    # cannot create state in the source inventory or the shipping bundle.
    $PreflightRoot = Join-Path ([IO.Path]::GetTempPath()) ('qub-hf132-preflight-' + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $PreflightRoot | Out-Null
    Push-Location $PreflightRoot
    try {
        & (Join-Path $Dist 'tools\qubd.exe') --config $PreflightConfig preflight
        if ($LASTEXITCODE -ne 0) { throw "QUB preflight failed for $Config in disposable data directory $PreflightRoot" }
    } finally {
        Pop-Location
        Remove-Item -LiteralPath $PreflightRoot -Recurse -Force
    }
}

if ($Sign) {
    $SignTool = Find-SignTool
    $Description = "Qubit Coin Core v$Version"
    $ExeFiles = @((Join-Path $Dist 'QUB-Core.exe'), (Join-Path $Dist 'tools\qubd.exe'), (Join-Path $Dist 'tools\qub-rpc-miner.exe'))
    foreach ($Exe in $ExeFiles) {
        if (-not (Test-Path $Exe)) { throw "Missing executable to sign: $Exe" }
        $SignArgs = @('sign', '/fd', 'SHA256', '/td', 'SHA256', '/tr', $TimestampUrl, '/d', $Description)
        if ($CertThumbprint -ne '') { $SignArgs += @('/sha1', $CertThumbprint) }
        elseif ($CertSubject -ne '') { $SignArgs += @('/n', $CertSubject) }
        else { $SignArgs += @('/a') }
        $SignArgs += $Exe
        & $SignTool @SignArgs
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $Exe" }
        & $SignTool verify /pa /v $Exe
        if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for $Exe" }
        $Signature = Get-AuthenticodeSignature -LiteralPath $Exe
        if ($Signature.Status -ne 'Valid') { throw "Authenticode chain invalid: $Exe" }
        if ($CertThumbprint -and $Signature.SignerCertificate.Thumbprint -ne $CertThumbprint) { throw "Unexpected signing certificate: $Exe" }
    }
}
if ($OfficialOriginUnsigned) {
    foreach ($Exe in @((Join-Path $Dist 'QUB-Core.exe'), (Join-Path $Dist 'tools\qubd.exe'), (Join-Path $Dist 'tools\qub-rpc-miner.exe'))) {
        if ((Get-AuthenticodeSignature -LiteralPath $Exe).Status -ne 'NotSigned') { throw "Official-origin unsigned mode requires NotSigned; it never accepts an invalid signature: $Exe" }
    }
}

$HashFile = Join-Path $Dist 'SHA256SUMS.txt'
Get-ChildItem $Dist -Recurse -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    $hash = Get-FileHash -Algorithm SHA256 $_.FullName
    "$($hash.Hash.ToLower())  $($_.FullName.Substring($Dist.Length + 1))"
} | Set-Content -Encoding ASCII $HashFile

if ($BuildInstaller) {
    $ISCC = $InnoSetupCompiler
    $InstallerName = "QUB-Core-v$Version-Windows-x64-$Config-Setup.exe"
    $InstallerPath = Join-Path $DistRoot "installer\$InstallerName"
    if (Test-Path -LiteralPath $InstallerPath) { throw "Installer destination unexpectedly exists: $InstallerPath" }
    $Iss = Join-Path $Root 'installer\QUB-Core.iss'
    if (-not (Test-Path $Iss)) { throw "Missing installer script: $Iss" }
    & $ISCC ("/O" + (Join-Path $DistRoot 'installer')) "/DMyAppSource=$Dist" "/DMyAppConfig=$Config" $Iss
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup compiler failed' }

    $InstallerName = "QUB-Core-v$Version-Windows-x64-$Config-Setup.exe"
    $InstallerPath = Join-Path $DistRoot "installer\$InstallerName"
    if (-not (Test-Path $InstallerPath)) { throw "Installer not found after ISCC: $InstallerPath" }

    if ($Sign) {
        $SignTool = Find-SignTool
        $Description = "Qubit Coin Core v$Version Installer"
        $SignArgs = @('sign', '/fd', 'SHA256', '/td', 'SHA256', '/tr', $TimestampUrl, '/d', $Description)
        if ($CertThumbprint -ne '') { $SignArgs += @('/sha1', $CertThumbprint) }
        elseif ($CertSubject -ne '') { $SignArgs += @('/n', $CertSubject) }
        else { $SignArgs += @('/a') }
        $SignArgs += $InstallerPath
        & $SignTool @SignArgs
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for installer $InstallerPath" }
        & $SignTool verify /pa /v $InstallerPath
        if ($LASTEXITCODE -ne 0) { throw "signtool verify failed for installer $InstallerPath" }
        $Signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
        if ($Signature.Status -ne 'Valid') { throw 'Installer Authenticode chain invalid' }
        if ($CertThumbprint -and $Signature.SignerCertificate.Thumbprint -ne $CertThumbprint) { throw 'Installer signing certificate mismatch' }
    }
    if ($OfficialOriginUnsigned -and (Get-AuthenticodeSignature -LiteralPath $InstallerPath).Status -ne 'NotSigned') {
        throw 'Official-origin unsigned installer must be NotSigned; an invalid signature is never accepted.'
    }

    $LatestExe = Join-Path $DistRoot 'updates\QUB-Core-Latest.exe'
    Copy-Item $InstallerPath $LatestExe -Force
    $LatestHash = (Get-FileHash -Algorithm SHA256 $LatestExe).Hash.ToLower()
    Set-Content -Encoding ASCII (Join-Path $DistRoot 'updates\QUB-Core-Latest.exe.sha256') $LatestHash

    $VersionedUpdateInstaller = Join-Path $ChannelUpdateDir $InstallerName
    $ChannelLatest = Join-Path $ChannelUpdateDir 'QUB-Core-Latest.exe'
    Copy-Item $InstallerPath $VersionedUpdateInstaller -Force
    Copy-Item $InstallerPath $ChannelLatest -Force
    $VersionedHash = (Get-FileHash -Algorithm SHA256 $VersionedUpdateInstaller).Hash.ToLower()
    $ChannelLatestHash = (Get-FileHash -Algorithm SHA256 $ChannelLatest).Hash.ToLower()
    Set-Content -Encoding ASCII (Join-Path $ChannelUpdateDir "$InstallerName.sha256") $VersionedHash
    Set-Content -Encoding ASCII (Join-Path $ChannelUpdateDir 'QUB-Core-Latest.exe.sha256') $ChannelLatestHash

    $ConsensusFamily = if ($Config -eq 'mainnet') { 'qns-1000' } elseif ($Config -eq 'testnet') { 'qns-50' } else { $Config }
    $Manifest = [ordered]@{
        channel = "$Config-windows-x64"
        network = $Config
        version = $Version
        platform = 'windows-x64'
        kind = 'installer'
        mandatory = ($Config -eq 'mainnet')
        chain_upgrade = $false
        hotfix = 'HF132'
        activation_feature = 'none'
        activation_height = 0
        protocol_epoch = 2
        protocol_epoch_2_activation_height = 24000
        post_activation_block_version = 2
        checkpoint_height = 10367
        checkpoint_hash = '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000'
        consensus_family = if ($Config -eq 'mainnet') { 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000' } else { $ConsensusFamily }
        hotfix_family = 'hf132-sync-catchup-reliability'
        published_at = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
        url = "$UpdateBaseUrl/$Config/windows-x64/$InstallerName"
        latest_url = "$UpdateBaseUrl/$Config/windows-x64/QUB-Core-Latest.exe"
        sha256 = $VersionedHash
        size = [int64](Get-Item $VersionedUpdateInstaller).Length
        notes = 'HF132 sync catch-up completion, moving-tip progress and pending solo/pool Start/Stop reliability, retaining HF131 fixes and unchanged mainnet consensus.'
    }
    $ManifestJson = $Manifest | ConvertTo-Json -Depth 8
    $ManifestPath = Join-Path $ChannelUpdateDir 'manifest.json'
    $Utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($ManifestPath, $ManifestJson, $Utf8NoBom)
    $ManifestHash = (Get-FileHash -Algorithm SHA256 $ManifestPath).Hash.ToLower()
    Set-Content -Encoding ASCII (Join-Path $ChannelUpdateDir 'manifest.json.sha256') $ManifestHash
    Get-ChildItem -LiteralPath $ChannelUpdateDir -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | Sort-Object Name | ForEach-Object {
        "$((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant())  $($_.Name)"
    } | Set-Content -Encoding ASCII (Join-Path $ChannelUpdateDir 'SHA256SUMS.txt')
}

Write-Host "Native evidence: $NativeReceipt"
Write-Host "Built release bundle: $Dist"
Write-Host "Normal users run: $Dist\QUB-Core.exe"
Write-Host "Advanced tools: $Dist\tools"
if ($BuildInstaller) {
    Write-Host "Installer ready: $DistRoot\installer\QUB-Core-v$Version-Windows-x64-$Config-Setup.exe"
    Write-Host "Legacy latest payload: $DistRoot\updates\QUB-Core-Latest.exe"
    Write-Host "Manifest channel output: $DistRoot\updates\$Config\windows-x64\manifest.json"
}
if ($Sign) { Write-Host "Executables were Authenticode-signed before SHA256SUMS.txt was generated." }
if ($OfficialOriginUnsigned) { Write-Host 'Unsigned official-origin release: NotSigned verified before checksums; distribute only via the established download.qubit-coin.io HTTPS URLs.' }
