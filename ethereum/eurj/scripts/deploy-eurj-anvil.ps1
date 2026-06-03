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
Write-Host "Build EURJ contracts..."
forge build
Write-Host "Deploy mock EURC..."
$eurcOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock EUR Coin" "EURC" 6
$eurcOut
$eurc = Get-DeployedAddress $eurcOut "EURC"
Write-Host "Deploy mock EURS..."
$eursOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock STASIS EURS" "EURS" 2
$eursOut
$eurs = Get-DeployedAddress $eursOut "EURS"
Write-Host "Mint mock stablecoins to deployer..."
cast send $eurc "mint(address,uint256)" $DeployerAddress 1000000000000 --rpc-url $RpcUrl --private-key $PrivateKey
cast send $eurs "mint(address,uint256)" $DeployerAddress 100000000 --rpc-url $RpcUrl --private-key $PrivateKey
Write-Host "Deploy EURJ token..."
$eurjOut = forge create src/JinexEUR.sol:JinexEUR --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex EUR" "EURJ" $DeployerAddress
$eurjOut
$eurj = Get-DeployedAddress $eurjOut "EURJ"
Write-Host "Deploy EURJ ReserveVault..."
$vaultOut = forge create src/EURJReserveVault.sol:EURJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $eurj $eurc $eurs $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "EURJ vault"
Write-Host "Configure EURJ vault minter..."
cast send $eurj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey
Write-Host ""
Write-Host "ANVIL_CHAIN_ID=31337"
Write-Host "ETH_RPC_URL=$RpcUrl"
Write-Host "DEPLOYER_ADDRESS=$DeployerAddress"
Write-Host "MOCK_EURC_ADDRESS=$eurc"
Write-Host "MOCK_EURS_ADDRESS=$eurs"
Write-Host "EURJ_ADDRESS=$eurj"
Write-Host "EURJ_VAULT_ADDRESS=$vault"
