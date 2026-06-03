# QUB Hotfix22 / v1.1.0 — QNS + Explorer + Caddy

Hotfix22 is cumulative from the released Hotfix20/v1.0.1 line and includes the Hotfix21 block explorer.

## Is it a fork?

QNS is a consensus extension, activated by height. It does not require a chain reset.

Default activation heights:

- mainnet: block #5000
- testnet: block #120
- regtest: block #10
- regtest-lan: block #20

Mainnet was around #702 when this package was prepared, so #5000 leaves a large upgrade window. Testnet can exercise QNS quickly.

Old nodes can continue syncing ordinary blocks, but miners should upgrade before QNS activation. Once QNS is active, upgraded miners/nodes enforce deterministic name uniqueness, payment, and marker rules.

## QNS rules

- Name suffix: `.qub`
- Accepted label chars: latin lowercase letters and digits (`a-z`, `0-9`)
- Case input is normalized to lowercase.
- Max label length: 32 chars.
- Names are permanent and frozen to the registered address.
- One transaction registers one name.
- First valid registration wins.
- `qns.qub` is reserved as the first protocol name and resolves to the QNS protocol treasury address.
- Registration payment goes to the configured protocol address.
- A tiny marker output stores the deterministic name/address binding on-chain.

## Pricing

Price is deterministic and length-sensitive:

```text
price_atoms = base_registration_atoms + price_coefficient_atoms * (max_label_chars - label_len + 1)^2
```

With the shipped config:

```text
base_registration_atoms = 1 QUB
price_coefficient_atoms = 1 QUB
max_label_chars = 32
```

So shorter names are more expensive and long names are cheaper. Examples:

- `a.qub`: 1025 QUB
- 32-char label: 2 QUB

## Important treasury note

The shipped `protocol_address` is a deterministic protocol-reserve address. If you want QNS payments to be spendable by a treasury wallet before activation, generate a dedicated treasury address and update `qns.protocol_address` consistently in mainnet/testnet seed/client configs before the activation height. After activation, changing it is a consensus change.

## CLI

```bash
qubd --config config/mainnet.toml qns-price alice.qub
qubd --config config/mainnet.toml qns-resolve qns.qub
qubd --config config/mainnet.toml qns-list
qubd --config config/mainnet.toml qns-list qub1...
qubd --config config/mainnet.toml qns-register alice.qub qub1TARGET_ADDRESS 0.00001
```

`send` now accepts `.qub` names after they are registered:

```bash
qubd --config config/mainnet.toml send alice.qub 1.25 0.00001
```

## GUI

The GUI shows QNS names in:

- peers / miner list
- recent block list
- send dialog resolved address field
- QNS registration popup
- live chain summary

`Send` accepts either a normal address or a registered `.qub` name and shows the resolved address before signing.

## Explorer

New API endpoints:

```text
GET /api/v1/qns?limit=25&offset=0
GET /api/v1/qns/<name.qub>
GET /api/v1/resolve/<name.qub>
```

The explorer remains read-only and scans `chain.json` directly. No explorer database is used.

## Caddy setup

Start the explorer API:

```bash
cd /opt/jinex/staging/src/qubd
. "$HOME/.cargo/env"
bash deploy/digitalocean/install-seed-services.sh /opt/jinex/staging/src/qubd
bash deploy/digitalocean/install-explorer-api-service.sh /opt/jinex/staging/src/qubd mainnet 127.0.0.1:18765
```

Add this to Caddy:

```caddy
api.qubit-coin.io {
    encode zstd gzip

    header {
        X-Content-Type-Options "nosniff"
        Referrer-Policy "no-referrer"
        Access-Control-Allow-Origin "*"
        Access-Control-Allow-Methods "GET, OPTIONS"
        Access-Control-Allow-Headers "Content-Type"
    }

    reverse_proxy 127.0.0.1:18765
}
```

Then:

```bash
sudo caddy fmt --overwrite /etc/caddy/Caddyfile
sudo systemctl reload caddy
curl -s https://api.qubit-coin.io/api/v1/summary
```

## Cloudflare Pages

Build the static explorer:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-explorer-pages.ps1 `
  -ApiBase "https://api.qubit-coin.io/api/v1"
```

Deploy `dist\x-qubit-coin-io` to Cloudflare Pages and attach the custom domain:

```text
x.qubit-coin.io
```

## Rollout

Recommended order:

1. Update testnet seed.
2. Test QNS at/after testnet activation.
3. Update mainnet seed.
4. Build and publish v1.1.0 client before mainnet #5000.
5. Ensure active miners upgrade before #5000.

No chain reset is needed.
