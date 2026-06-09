# Jinex Gold (XAUJ) pooled reserve contracts

One public Ethereum-side XAUJ token backed by pooled PAXG and XAUt reserve buckets.

- XAUJ decimals: 18
- PAXG decimals: 18
- XAUt decimals: 6
- XAUt native units are normalized to XAUJ with a 10^12 scale.
- Melt to XAUt requires an XAUJ amount divisible by 10^12 native units.

No bridge mint/burn logic is included here. These contracts only handle Ethereum-side mint/melt against pooled gold-token reserves.
