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
Write-Host "Build XAUJ contracts..."
forge build
Write-Host "Deploy mock PAXG..."
$paxgOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock PAX Gold" "PAXG" 18
$paxgOut
$paxg = Get-DeployedAddress $paxgOut "PAXG"
Write-Host "Deploy mock XAUt..."
$xautOut = forge create src/MockERC20.sol:MockERC20 --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Mock Tether Gold" "XAUt" 6
$xautOut
$xaut = Get-DeployedAddress $xautOut "XAUt"
Write-Host "Mint mock gold tokens to deployer..."
cast send $paxg "mint(address,uint256)" $DeployerAddress 1000000000000000000000000 --rpc-url $RpcUrl --private-key $PrivateKey
cast send $xaut "mint(address,uint256)" $DeployerAddress 1000000000000 --rpc-url $RpcUrl --private-key $PrivateKey
Write-Host "Deploy XAUJ token..."
$xaujOut = forge create src/JinexGold.sol:JinexGold --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args "Jinex Gold" "XAUJ" $DeployerAddress
$xaujOut
$xauj = Get-DeployedAddress $xaujOut "XAUJ"
Write-Host "Deploy XAUJ ReserveVault..."
$vaultOut = forge create src/XAUJReserveVault.sol:XAUJReserveVault --rpc-url $RpcUrl --private-key $PrivateKey --broadcast --constructor-args $xauj $paxg $xaut $DeployerAddress
$vaultOut
$vault = Get-DeployedAddress $vaultOut "XAUJ vault"
Write-Host "Configure XAUJ vault minter..."
cast send $xauj "configureVaultMinter(address)" $vault --rpc-url $RpcUrl --private-key $PrivateKey
Write-Host ""
Write-Host "ANVIL_CHAIN_ID=31337"
Write-Host "ETH_RPC_URL=$RpcUrl"
Write-Host "DEPLOYER_ADDRESS=$DeployerAddress"
Write-Host "MOCK_PAXG_ADDRESS=$paxg"
Write-Host "MOCK_XAUT_ADDRESS=$xaut"
Write-Host "XAUJ_ADDRESS=$xauj"
Write-Host "XAUJ_VAULT_ADDRESS=$vault"
