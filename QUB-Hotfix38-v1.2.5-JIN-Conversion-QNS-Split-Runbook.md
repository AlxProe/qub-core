# QUB Hotfix38 / v1.2.6 — JIN Conversion + QNS Miner Split

## Scope

This is a chain-upgrade capable release, but the activation heights are intentionally disabled in packaged configs.
Before deploying to any live network, patch the network config section-safely:

- `[qns].miner_split_activation_height`
- `[jin].conversion_activation_height`

Do **not** change:

- `[qns].activation_height`
- `[jin].activation_height`

## Features

- JIN Coin -> JIN Token conversion request on Qubit Chain.
- Whole-JIN only conversion, because JIN Token on Enjin Matrixchain is integer-only.
- Conversion locks native JIN back into the JIN bridge/protocol address.
- Enjin Matrixchain payout/claim flow is external and must be implemented separately.
- JIN fees from JIN transfers/conversions are credited to the miner who mined the block.
- Recent block rewards and mined-block cards show `QUB + JIN` rewards.
- Address Activity supports Conversion entries and JIN fee details.
- QNS registration split can activate by config: 50% protocol output, 50% miner share as block fee.

## QNS split design note

A user cannot put the future block miner's address into a signed QNS registration transaction, because the miner is not known at tx creation time. The safe consensus design is:

- QNS tx pays 50% of the deterministic price to the QNS protocol address as a normal output.
- The remaining 50% is intentionally left as block fee.
- The miner receives that fee in the coinbase of the same block.

The total QNS price paid by the user is unchanged.

## Testnet activation

For testnet, use current height + 10:

```bash
H=$(/opt/qub/bin/qubd-v1.2.6-testnet --config /opt/qub/testnet/testnet-seed.toml info | python3 -c 'import sys,json; print(json.load(sys.stdin)["height"])')
ACT=$((H + 10))
echo "testnet ACT=$ACT"
```

Patch section-safely:

```bash
cat >/tmp/fix-v125-activation.awk <<'AWK'
BEGIN { section = "" }
$0 ~ /^\[[^]]+\]$/ { section = $0 }
section == "[qns]" && $1 == "miner_split_activation_height" { print "miner_split_activation_height = " act_height; next }
section == "[jin]" && $1 == "conversion_activation_height" { print "conversion_activation_height = " act_height; next }
{ print }
AWK

awk -v act_height="$ACT" -f /tmp/fix-v125-activation.awk /opt/qub/testnet/testnet-seed.toml > /tmp/testnet-seed.toml.fixed
sudo install -m 0644 /tmp/testnet-seed.toml.fixed /opt/qub/testnet/testnet-seed.toml
```

Verify:

```bash
grep -n "^\[qns\]\|^\[jin\]\|activation_height\|miner_split_activation_height\|conversion_activation_height" /opt/qub/testnet/testnet-seed.toml
```

Expected:

- QNS activation stays as existing QNS activation.
- JIN activation stays as existing JIN activation.
- miner split activation = ACT.
- conversion activation = ACT.

## Mainnet activation

Only after public testnet passes activation, use current height + 100:

```bash
H=$(/opt/qub/bin/qubd-v1.2.6-mainnet --config /opt/qub/mainnet/mainnet-seed.toml info | python3 -c 'import sys,json; print(json.load(sys.stdin)["height"])')
ACT=$((H + 100))
echo "mainnet ACT=$ACT"
```

Patch `[qns].miner_split_activation_height` and `[jin].conversion_activation_height` to ACT on seed and miner configs.
Do not overwrite the existing mainnet JIN/QNS activation heights.

## Verification after activation

- Register a QNS name. Verify protocol receives half and miner receives half as QUB fee.
- Send JIN with JIN fee. Verify miner JIN balance increases by fee after block.
- Send JIN with QUB fee. Verify miner receives QUB fee only.
- Create JIN Coin -> Token conversion request. Verify sender JIN decreases and protocol JIN reserve increases by conversion amount.
- Verify Address Activity shows Conversion entry.
- Verify Recent global blocks reward column shows `QUB + JIN`.

## Mainnet warning

Hotfix36 / v1.2.1 was testnet-only and must never be mixed into mainnet. v1.2.6 is based on the v1.2.0/v1.2.4 mainline.
