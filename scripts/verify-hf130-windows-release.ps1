[CmdletBinding(DefaultParameterSetName = 'Signed')]
param(
    [Parameter(Mandatory)][string]$Directory,
    [Parameter(Mandatory, ParameterSetName = 'Signed')][ValidatePattern('^[0-9a-fA-F]{40}$')][string]$CertThumbprint,
    [Parameter(Mandatory, ParameterSetName = 'Unsigned')][switch]$OfficialOriginUnsigned,
    [string]$BundleDirectory = ''
)
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
if ($PSCmdlet.ParameterSetName -eq 'Unsigned' -and -not $OfficialOriginUnsigned) { throw 'Unsigned policy must be explicitly selected.' }
$TrustPolicy = if ($OfficialOriginUnsigned) { 'official-origin unsigned (NotSigned)' } else { 'signed with exact certificate thumbprint' }
$Directory = (Resolve-Path -LiteralPath $Directory).Path
$InstallerName = 'QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
$ExpectedFiles = @($InstallerName, "$InstallerName.sha256", 'QUB-Core-Latest.exe', 'QUB-Core-Latest.exe.sha256', 'manifest.json', 'manifest.json.sha256', 'SHA256SUMS.txt')
$ActualFiles = @(Get-ChildItem -LiteralPath $Directory -Force)
if ($ActualFiles.Count -ne $ExpectedFiles.Count) { throw 'Update directory must contain exactly the seven HF130 release files.' }
foreach ($Item in $ActualFiles) {
    if ($Item.PSIsContainer -or $Item.Name -notin $ExpectedFiles -or ($Item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { throw "Unexpected update entry: $($Item.Name)" }
}
$Entries = @{}
foreach ($Line in Get-Content -LiteralPath (Join-Path $Directory 'SHA256SUMS.txt') -Encoding ASCII) {
    if ($Line -notmatch '^([0-9a-fA-F]{64})  ([A-Za-z0-9._-]+)$') { throw 'Malformed release checksum line.' }
    $Name = $Matches[2]
    if ($Entries.ContainsKey($Name) -or $Name -eq 'SHA256SUMS.txt' -or $Name -notin $ExpectedFiles) { throw "Invalid/duplicate checksum entry: $Name" }
    $Entries[$Name] = $Matches[1].ToLowerInvariant()
}
if ($Entries.Count -ne 6) { throw 'Release checksum inventory must cover all six payload/metadata files.' }
foreach ($Name in $Entries.Keys) {
    $Hash = (Get-FileHash -LiteralPath (Join-Path $Directory $Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Hash -ne $Entries[$Name]) { throw "Release checksum mismatch: $Name" }
}
$Manifest = Get-Content -LiteralPath (Join-Path $Directory 'manifest.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$Fields = @{
    channel = 'mainnet-windows-x64'; network = 'mainnet'; version = '1.8.5'; platform = 'windows-x64'; kind = 'installer';
    hotfix = 'HF130'; activation_feature = 'none'; activation_height = 0; protocol_epoch = 2;
    protocol_epoch_2_activation_height = 24000; post_activation_block_version = 2; checkpoint_height = 10367;
    checkpoint_hash = '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000';
    consensus_family = 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000';
    url = "https://download.qubit-coin.io/mainnet/windows-x64/$InstallerName";
    latest_url = 'https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe'
}
foreach ($Name in @('activation_height','protocol_epoch','protocol_epoch_2_activation_height','post_activation_block_version','checkpoint_height','size')) {
    if ($null -eq $Manifest.PSObject.Properties[$Name] -or ($Manifest.$Name -isnot [int] -and $Manifest.$Name -isnot [long])) { throw "Manifest integer field has wrong JSON type: $Name" }
}
foreach ($Name in $Fields.Keys) {
    if ($null -eq $Manifest.PSObject.Properties[$Name] -or $Manifest.$Name -cne $Fields[$Name]) { throw "Manifest field mismatch: $Name" }
}
if ($Manifest.mandatory -isnot [bool] -or -not $Manifest.mandatory -or $Manifest.chain_upgrade -isnot [bool] -or $Manifest.chain_upgrade) { throw 'Manifest mandatory/chain_upgrade mismatch.' }
$Installer = Join-Path $Directory $InstallerName
$InstallerHash = (Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
if ($Manifest.sha256 -cne $InstallerHash -or [int64]$Manifest.size -ne (Get-Item -LiteralPath $Installer).Length) { throw 'Installer does not match manifest hash/size.' }
if ((Get-FileHash -LiteralPath (Join-Path $Directory 'QUB-Core-Latest.exe') -Algorithm SHA256).Hash.ToLowerInvariant() -ne $InstallerHash) { throw 'Latest installer differs from versioned installer.' }
foreach ($Name in @($InstallerName, 'QUB-Core-Latest.exe', 'manifest.json')) {
    $Expected = (Get-Content -LiteralPath (Join-Path $Directory "$Name.sha256") -Raw -Encoding ASCII).Trim()
    $Actual = (Get-FileHash -LiteralPath (Join-Path $Directory $Name) -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Expected -cne $Actual) { throw "Sidecar mismatch: $Name" }
}
$Executables = @($Installer, (Join-Path $Directory 'QUB-Core-Latest.exe'))
if ($BundleDirectory) {
    $Executables += @((Join-Path $BundleDirectory 'QUB-Core.exe'), (Join-Path $BundleDirectory 'tools\qubd.exe'), (Join-Path $BundleDirectory 'tools\qub-rpc-miner.exe'))
}
foreach ($Exe in $Executables) {
    $Signature = Get-AuthenticodeSignature -LiteralPath $Exe
    if ($OfficialOriginUnsigned) {
        if ($Signature.Status -ne 'NotSigned') { throw "Unsigned official-origin mode requires NotSigned; invalid signatures are rejected: $Exe" }
    } elseif ($Signature.Status -ne 'Valid' -or $Signature.SignerCertificate.Thumbprint -ne $CertThumbprint) {
        throw "Invalid or unexpected Authenticode signer: $Exe"
    }
    $Version = (Get-Item -LiteralPath $Exe).VersionInfo.ProductVersion
    if ($Version -notmatch '^1\.8\.5(?:\.0)?$') { throw "Unexpected ProductVersion '$Version': $Exe" }
}
Write-Output "HF130 WINDOWS DISTRIBUTION VERIFICATION: PASS - $InstallerHash - $TrustPolicy"
