param(
  [Parameter(Mandatory=$true)][string]$RpcUrl,
  [Parameter(Mandatory=$true)][string]$PrivateKey,
  [Parameter(Mandatory=$true)][string]$DeployerAddress,
  [Parameter(Mandatory=$true)][string]$USDJAddress
)
$ErrorActionPreference = "Stop"
function Get-DeployedAddress([string]$text) {
  $m = [regex]::Match($text, '(?im)Deployed to:\s*(0x[a-fA-F0-9]{40})')
  if (-not $m.Success) { throw "Could not parse deployed address from forge output:`n$text" }
  $m.Groups[1].Value
}
Write-Host "Deploying USDJBridgeGateway to Ethereum mainnet..."
$out = forge create --broadcast --rpc-url $RpcUrl --private-key $PrivateKey src/USDJBridgeGateway.sol:USDJBridgeGateway --constructor-args $USDJAddress $DeployerAddress 2>&1 | Out-String
Write-Host $out
$gw = Get-DeployedAddress $out
Write-Host "USDJ_BRIDGE_GATEWAY_ADDRESS=$gw"
Write-Host "NOTE: configureQubVerifier() is one-way and should only be called after the QUB proof verifier is deployed and reviewed."
