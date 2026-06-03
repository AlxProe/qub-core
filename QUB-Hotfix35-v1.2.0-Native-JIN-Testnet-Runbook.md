# QUB Hotfix35 / v1.2.0 — Native JIN Testnet Activation

This release adds the first native Jinex Coin (JIN) protocol support behind a chain activation height.

## Activation heights

- testnet: #3365
- mainnet: #5555

## JIN parameters

- Symbol: JIN
- Decimals: 18
- Fixed supply: 105,000,000 JIN
- Supply units: 105000000000000000000000000
- Additional minting: impossible by consensus; the supply is a deterministic one-time protocol balance.

## Protocol bridge addresses

These addresses are protocol bridge treasury addresses and are **not QNS names**.
Do not send directly to these addresses expecting Enjin Matrixchain conversion. Two-way conversion will require a future explicit bridge transaction type.

- mainnet: `qub1bbc4abd368bf9f7a840938205b1bc1ca211fe3346933717b`
- testnet: `tqub175276ed4eb15b444cf98c4cac5ae8e66a6847226da697acf`

## Native JIN tx model

JIN transfer transactions use JIN marker outputs and are anchored by standard signed QUB inputs. This preserves the existing QUB transaction serialization and avoids changing historical block hashes.

- QUB transfers stay unchanged.
- JIN transfers are active only after the JIN activation height.
- JIN balances can be read from a public address.
- JIN send uses the same local wallet key model as QUB.
- JIN fees can be encoded as JIN for JIN transfers; QUB fee fallback is also supported.
- GPU/CPU mining rules are unchanged.

## Testnet first

Deploy v1.2.0 to testnet first. Do not publish mainnet until:

1. testnet activation reaches #3365,
2. chain replay validates,
3. JIN protocol address balance appears as 105,000,000 JIN after activation,
4. JIN send safely fails for users without JIN,
5. QUB send/QNS/mining remain unchanged.

