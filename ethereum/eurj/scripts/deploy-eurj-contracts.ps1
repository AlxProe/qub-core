param(
  [Parameter(Mandatory=$true)][string]$RpcUrl,
  [Parameter(Mandatory=$true)][string]$PrivateKey,
  [Parameter(Mandatory=$true)][string]$DeployerAddress,
  [string]$EurcAddress = "0x1aBaEA1f7C830bD89Acc67eC4Af516284b1bC33c",
  [string]$EursAddress = "0xdB25f211AB05b1c97D595516F45794528a807ad8"
)

$ErrorActionPreference = "Stop"

function Get-DeployedAddress($Output, $Label) {
  $text = ($Output | Out-String)
  if ($text -match "Deployed to:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  if ($text -match "Contract Address:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  throw "Could not parse $Label deployed address. forge output:`n$text"
}

Write-Host "Building EURJ contracts..."
forge build

Write-Host "Deploy EURJ token..."
$eurjOut = forge create src/JinexEUR.sol:JinexEUR --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex EUR" "EURJ" $DeployerAddress
$eurjOut
$eurj = Get-DeployedAddress $eurjOut "EURJ"

Write-Host "Deploy EURJ ReserveVault..."
$vaultOut = forge create src/EURJReserveVault.sol:EURJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $eurj $EurcAddress $EursAddress $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "EURJ vault"

Write-Host "Configure EURJ vault minter..."
cast send $eurj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey

Write-Host ""
Write-Host "EURJ_ADDRESS=$eurj"
Write-Host "EURJ_VAULT_ADDRESS=$vault"
Write-Host "EURC_ADDRESS=$EurcAddress"
Write-Host "EURS_ADDRESS=$EursAddress"
Write-Host ""
Write-Host "Paste EURJ_ADDRESS and EURJ_VAULT_ADDRESS into QUB Core -> Create / import address -> Ethereum -> Stablecoin Ethereum contracts."
