param(
  [Parameter(Mandatory=$true)][string]$RpcUrl,
  [Parameter(Mandatory=$true)][string]$PrivateKey,
  [Parameter(Mandatory=$true)][string]$DeployerAddress,
  [string]$UsdtAddress = "0xdAC17F958D2ee523a2206206994597C13D831ec7",
  [string]$UsdcAddress = "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"
)

$ErrorActionPreference = "Stop"

function Get-DeployedAddress($Output, $Label) {
  $text = ($Output | Out-String)
  if ($text -match "Deployed to:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  if ($text -match "Contract Address:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  throw "Could not parse $Label deployed address. forge output:`n$text"
}

Write-Host "Building..."
forge build

Write-Host "Deploy USDJ token..."
$usdjOut = forge create src/JinexUSD.sol:JinexUSD --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex USD" "USDJ" $DeployerAddress
$usdjOut
$usdj = Get-DeployedAddress $usdjOut "USDJ"

Write-Host "Deploy ReserveVault..."
$vaultOut = forge create src/USDJReserveVault.sol:USDJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $usdj $UsdtAddress $UsdcAddress $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "vault"

Write-Host "Configure USDJ vault minter..."
cast send $usdj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey

Write-Host ""
Write-Host "USDJ_ADDRESS=$usdj"
Write-Host "VAULT_ADDRESS=$vault"
Write-Host "USDT_ADDRESS=$UsdtAddress"
Write-Host "USDC_ADDRESS=$UsdcAddress"
Write-Host ""
Write-Host "Paste USDJ_ADDRESS and VAULT_ADDRESS into QUB Core -> Create / import address -> Ethereum -> USDJ Ethereum contracts."
