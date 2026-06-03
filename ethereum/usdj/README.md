# Jinex USD (USDJ) Ethereum pooled reserve contracts - HF107

This package contains the Ethereum-side contracts for **Option B: pooled reserve redemption**.

Public user-facing assets:

- One Ethereum ERC-20 `USDJ`
- One future QUB-chain `USDJ` asset

Internal accounting buckets:

- USDT reserve bucket
- USDC reserve bucket

There are no public transferable `USDJ-usdt_backed` or `USDJ-usdc_backed` tokens. Users see one fungible USDJ balance. The vault tracks pooled reserve liquidity and lets a user melt USDJ into either USDT or USDC if that selected reserve bucket has enough liquidity.

## Contracts

- `src/JinexUSD.sol` — ERC-20 USDJ, 6 decimals. Mint/burn restricted to one configured vault.
- `src/USDJReserveVault.sol` — holds USDT/USDC reserves, infuse/melt, pause/unpause, no owner withdrawal power over reserves.

## Mainnet stablecoin addresses

- USDT: `0xdAC17F958D2ee523a2206206994597C13D831ec7`
- USDC: `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48`

## Build and test

```bash
forge test
forge build
```

## Deploy manually with forge create

1. Deploy USDJ:

```bash
forge create src/JinexUSD.sol:JinexUSD \
  --rpc-url "$ETH_RPC_URL" \
  --private-key "$DEPLOYER_PRIVATE_KEY" \
  --constructor-args "Jinex USD" "USDJ" "$DEPLOYER_ADDRESS"
```

2. Deploy vault:

```bash
forge create src/USDJReserveVault.sol:USDJReserveVault \
  --rpc-url "$ETH_RPC_URL" \
  --private-key "$DEPLOYER_PRIVATE_KEY" \
  --constructor-args "$USDJ_ADDRESS" "$USDT_ADDRESS" "$USDC_ADDRESS" "$DEPLOYER_ADDRESS"
```

3. Configure USDJ mint/burn vault exactly once:

```bash
cast send "$USDJ_ADDRESS" "configureVaultMinter(address)" "$VAULT_ADDRESS" \
  --rpc-url "$ETH_RPC_URL" \
  --private-key "$DEPLOYER_PRIVATE_KEY"
```

4. Paste token/vault addresses into QUB Core:

`Create / import address -> Ethereum -> USDJ Ethereum contracts`.

For Ethereum mainnet, leave USDT/USDC override blank to use the canonical mainnet addresses. For Anvil/Sepolia/mock testing, paste the mock USDT and mock USDC addresses too, and set the QUB Core Ethereum Chain ID to match the RPC. Anvil default is `31337`; Sepolia is `11155111`; Ethereum mainnet is `1`.

## Important

This package does not include the cross-chain bridge yet. It only tests Ethereum-side USDJ infuse/melt.

The owner can pause/unpause the vault and rescue unsupported tokens accidentally sent to the vault, but cannot withdraw USDT, USDC, or USDJ reserves. After the vault is configured as the USDJ minter, the USDJ token has no alternate minter path.
