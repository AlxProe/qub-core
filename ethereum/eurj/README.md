# Jinex EUR (EURJ) Ethereum pooled reserve contracts - HF108

One public Ethereum ERC-20 `EURJ`, internally backed by pooled reserve buckets:

- EURC reserve bucket
- EURS reserve bucket

Users see one fungible EURJ balance. Melt chooses either EURC or EURS if that reserve bucket has enough liquidity.

Build/test:

```bash
forge test
forge build
```

Anvil deploy:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\deploy-eurj-anvil.ps1
```

## Mainnet deployment

Use `scripts/deploy-eurj-contracts.ps1` with an Ethereum mainnet RPC and deployer key. Defaults use Ethereum EURC and EURS contracts:

- EURC: `0x1aBaEA1f7C830bD89Acc67eC4Af516284b1bC33c`
- EURS: `0xdB25f211AB05b1c97D595516F45794528a807ad8`

The deployer can pause/unpause and transfer ownership, but the vault cannot rescue EURJ/EURC/EURS reserves.
