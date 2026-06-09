param(
  [string]$RpcUrl = "http://127.0.0.1:8545",
  [string]$PrivateKey = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  [string]$DeployerAddress = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
  [Parameter(Mandatory=$true)][string]$USDJAddress
)
$ErrorActionPreference = "Stop"
function Get-DeployedAddress([string]$text) {
  $m = [regex]::Match($text, '(?im)Deployed to:\s*(0x[a-fA-F0-9]{40})')
  if (-not $m.Success) { throw "Could not parse deployed address from forge output:`n$text" }
  $m.Groups[1].Value
}
Write-Host "Deploying USDJBridgeGateway to Anvil..."
$out = forge create --broadcast --rpc-url $RpcUrl --private-key $PrivateKey src/USDJBridgeGateway.sol:USDJBridgeGateway --constructor-args $USDJAddress $DeployerAddress 2>&1 | Out-String
Write-Host $out
$gw = Get-DeployedAddress $out
Write-Host "USDJ_BRIDGE_GATEWAY_ADDRESS=$gw"
