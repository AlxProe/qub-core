param(
  [Parameter(Mandatory=$true)][string]$RpcUrl,
  [Parameter(Mandatory=$true)][string]$PrivateKey,
  [Parameter(Mandatory=$true)][string]$DeployerAddress,
  [string]$PaxgAddress = "0x45804880De22913dAFE09f4980848ECE6EcbAf78",
  [string]$XautAddress = "0x68749665FF8D2d112Fa859AA293F07A622782F38"
)

$ErrorActionPreference = "Stop"

function Get-DeployedAddress($Output, $Label) {
  $text = ($Output | Out-String)
  if ($text -match "Deployed to:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  if ($text -match "Contract Address:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  throw "Could not parse $Label deployed address. forge output:`n$text"
}

Write-Host "Building XAUJ contracts..."
forge build

Write-Host "Deploy XAUJ token..."
$xaujOut = forge create src/JinexGold.sol:JinexGold --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex Gold" "XAUJ" $DeployerAddress
$xaujOut
$xauj = Get-DeployedAddress $xaujOut "XAUJ"

Write-Host "Deploy XAUJ ReserveVault..."
$vaultOut = forge create src/XAUJReserveVault.sol:XAUJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $xauj $PaxgAddress $XautAddress $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "XAUJ vault"

Write-Host "Configure XAUJ vault minter..."
cast send $xauj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey

Write-Host ""
Write-Host "XAUJ_ADDRESS=$xauj"
Write-Host "XAUJ_VAULT_ADDRESS=$vault"
Write-Host "PAXG_ADDRESS=$PaxgAddress"
Write-Host "XAUT_ADDRESS=$XautAddress"
Write-Host ""
Write-Host "Patch XAUJ_ADDRESS and XAUJ_VAULT_ADDRESS into QUB Core before building public dist."
