param(
  [string]$RpcUrl = "http://127.0.0.1:8545",
  [string]$PrivateKey = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  [string]$DeployerAddress = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
)

$ErrorActionPreference = "Stop"

function Get-DeployedAddress($Output, $Label) {
  $text = ($Output | Out-String)
  if ($text -match "Deployed to:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  if ($text -match "Contract Address:\s*(0x[a-fA-F0-9]{40})") { return $Matches[1] }
  throw "Could not parse $Label deployed address. forge output:`n$text"
}

Write-Host "Build contracts..."
forge build

Write-Host "Deploy mock USDT..."
$usdtOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock Tether USD" "USDT"
$usdtOut
$usdt = Get-DeployedAddress $usdtOut "USDT"

Write-Host "Deploy mock USDC..."
$usdcOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock USD Coin" "USDC"
$usdcOut
$usdc = Get-DeployedAddress $usdcOut "USDC"

Write-Host "Mint mock stablecoins to deployer..."
cast send $usdt "mint(address,uint256)" $DeployerAddress 1000000000000 --rpc-url $RpcUrl --private-key $PrivateKey
cast send $usdc "mint(address,uint256)" $DeployerAddress 1000000000000 --rpc-url $RpcUrl --private-key $PrivateKey

Write-Host "Deploy USDJ token..."
$usdjOut = forge create src/JinexUSD.sol:JinexUSD --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex USD" "USDJ" $DeployerAddress
$usdjOut
$usdj = Get-DeployedAddress $usdjOut "USDJ"

Write-Host "Deploy ReserveVault..."
$vaultOut = forge create src/USDJReserveVault.sol:USDJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $usdj $usdt $usdc $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "vault"

Write-Host "Configure USDJ vault minter..."
cast send $usdj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey

Write-Host ""
Write-Host "ANVIL_CHAIN_ID=31337"
Write-Host "ETH_RPC_URL=$RpcUrl"
Write-Host "DEPLOYER_ADDRESS=$DeployerAddress"
Write-Host "MOCK_USDT_ADDRESS=$usdt"
Write-Host "MOCK_USDC_ADDRESS=$usdc"
Write-Host "USDJ_ADDRESS=$usdj"
Write-Host "VAULT_ADDRESS=$vault"
Write-Host ""
Write-Host "QUB Core settings: RPC=$RpcUrl, Chain ID=31337, USDT token=$usdt, USDC token=$usdc, USDJ token=$usdj, Reserve vault=$vault"
