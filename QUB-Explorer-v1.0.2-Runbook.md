# Qubit Coin Explorer v1.0.2 — x.qubit-coin.io

This package adds a lightweight read-only explorer API to `qubd` and a static frontend for Cloudflare Pages.

## Architecture

```
QUB seed/full node on DigitalOcean
  qubd node                 -> P2P: 17444 / 18444
  qubd explorer-api         -> localhost HTTP: 127.0.0.1:18765
  nginx api.qubit-coin.io   -> HTTPS via Cloudflare -> localhost:18765

Cloudflare Pages
  x.qubit-coin.io           -> static explorer frontend
  browser fetches           -> https://api.qubit-coin.io/api/v1/...
```

The explorer API does **not** use an explorer database. Every request reloads the node chain state from `chain.json` and scans blocks/transactions/UTXOs on demand.

## Why not frontend-only?

A pure frontend cannot accurately read the live chain directly because browsers cannot speak the QUB raw TCP P2P protocol and cannot read the seed node filesystem. The smallest accurate setup is a static frontend plus a read-only HTTP API that loads the canonical chain state directly from the node.

## API endpoints

```
GET /api/v1/summary
GET /api/v1/blocks?limit=25&offset=0
GET /api/v1/block/<height-or-hash>
GET /api/v1/tx/<txid>
GET /api/v1/address/<address>?limit=25&offset=0
GET /api/v1/search?q=<height|hash|txid|address>
GET /api/v1/mempool
GET /api/v1/health
```

## Seed/API install on DigitalOcean

Upload clean source to the droplet, then:

```bash
cd /opt/jinex/staging/src/qubd
. "$HOME/.cargo/env"
bash deploy/digitalocean/install-seed-services.sh /opt/jinex/staging/src/qubd
bash deploy/digitalocean/install-explorer-api-service.sh /opt/jinex/staging/src/qubd mainnet 127.0.0.1:18765
```

Check:

```bash
systemctl status qub-explorer-api-mainnet.service --no-pager
curl -s http://127.0.0.1:18765/api/v1/summary | head
```

## Nginx API proxy

```bash
sudo apt-get update
sudo apt-get install -y nginx
sudo cp deploy/digitalocean/nginx-qub-explorer-api.conf /etc/nginx/sites-available/qub-explorer-api.conf
sudo ln -sf /etc/nginx/sites-available/qub-explorer-api.conf /etc/nginx/sites-enabled/qub-explorer-api.conf
sudo nginx -t
sudo systemctl reload nginx
```

Cloudflare DNS:

```
A api 159.223.222.103 Proxied / orange cloud
```

Then test from your PC:

```powershell
Invoke-RestMethod https://api.qubit-coin.io/api/v1/summary
```

If your Cloudflare SSL mode is Full/Strict, install an origin certificate on nginx or use certbot. If you use Flexible during the first test, Cloudflare terminates HTTPS at the edge and connects to nginx over HTTP.

## Build Cloudflare Pages static explorer

From Windows source folder:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\build-explorer-pages.ps1 `
  -ApiBase "https://api.qubit-coin.io/api/v1"
```

Deploy with Wrangler direct upload:

```powershell
npx wrangler pages project create qub-explorer --production-branch main
npx wrangler pages deploy .\dist\x-qubit-coin-io --project-name qub-explorer
```

In Cloudflare Pages, add custom domain:

```
x.qubit-coin.io
```

Cloudflare will create/guide the CNAME record to the Pages project.

## Safe rollout

1. Update seed source to v1.0.2 and restart seed services.
2. Install explorer API service.
3. Configure nginx + Cloudflare DNS for `api.qubit-coin.io`.
4. Deploy static frontend to Pages and attach `x.qubit-coin.io`.
5. Open `https://x.qubit-coin.io` and search a known block/address/tx.

No chain reset, no hard fork, and no balance migration are required.
