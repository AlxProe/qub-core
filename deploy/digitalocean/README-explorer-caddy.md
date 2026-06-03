# QUB Explorer API with Caddy

Hotfix22 / QUB Core v1.1.0 ships the read-only explorer API in `qubd`.

Run the API service after seed services are installed:

```bash
cd /opt/jinex/staging/src/qubd
. "$HOME/.cargo/env"
bash deploy/digitalocean/install-explorer-api-service.sh /opt/jinex/staging/src/qubd mainnet 127.0.0.1:18765
```

Add this to your existing Caddy config:

```bash
sudo cp deploy/digitalocean/Caddyfile.qub-explorer-api /etc/caddy/Caddyfile.d/qub-explorer-api.caddy
sudo caddy fmt --overwrite /etc/caddy/Caddyfile.d/qub-explorer-api.caddy
sudo systemctl reload caddy
```

If your Caddy setup does not import `/etc/caddy/Caddyfile.d/*.caddy`, paste the block from
`deploy/digitalocean/Caddyfile.qub-explorer-api` into `/etc/caddy/Caddyfile`.

Check:

```bash
curl -s http://127.0.0.1:18765/api/v1/health
curl -s https://api.qubit-coin.io/api/v1/summary
```
