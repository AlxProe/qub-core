# Qubit Coin seed nodes on one DigitalOcean droplet

Domain: `qubit-coin.io`
Seed DNS name: `seed.qubit-coin.io`
Droplet IP used in the current runbook: `159.223.222.103`

This setup runs both public seed nodes on the same machine, separated by port:

- mainnet seed: `seed.qubit-coin.io:17444`
- testnet seed: `seed.qubit-coin.io:18444`

The same DNS A record can be used because the two networks use different TCP ports.

## Cloudflare DNS

Create an A record:

```text
Type: A
Name: seed
Content: 159.223.222.103
Proxy status: DNS only / gray cloud
TTL: Auto
```

Do not proxy this record. QUB P2P is raw TCP, not HTTP(S), so Cloudflare must return the droplet IP directly.

## Upload source from Windows

From PowerShell, use the same style you already use for Jinex:

```powershell
scp -i $env:USERPROFILE\.ssh\jinex_ed25519 -r C:\temp\qubd_deploy\* deploy@159.223.222.103:/opt/jinex/staging/src/qubd/
```

If you want to reuse your current staging path exactly, put the `qubd` project folder contents under:

```text
/opt/jinex/staging/src/qubd/
```

## Droplet commands

SSH in:

```bash
ssh -i ~/.ssh/jinex_ed25519 deploy@159.223.222.103
```

Open the two seed ports. Keep SSH open before enabling UFW:

```bash
sudo ufw allow OpenSSH
sudo ufw allow 17444/tcp
sudo ufw allow 18444/tcp
sudo ufw enable
sudo ufw status verbose
```

Install/build/start both seed services:

```bash
cd /opt/jinex/staging/src/qubd
bash deploy/digitalocean/install-seed-services.sh /opt/jinex/staging/src/qubd
```

Check status:

```bash
systemctl status qub-seed-mainnet.service --no-pager
systemctl status qub-seed-testnet.service --no-pager
journalctl -u qub-seed-mainnet.service -f
journalctl -u qub-seed-testnet.service -f
```

## Public client config

Public clients ship with:

```toml
# mainnet
bootnodes = ["seed.qubit-coin.io:17444"]

# testnet
bootnodes = ["seed.qubit-coin.io:18444"]
```

Users never type the droplet IP. If the droplet IP changes, update the Cloudflare A record only.

## Important launch note

Running testnet and mainnet seeds on the same droplet is acceptable for the first controlled launch, but it is not the final decentralization target. After mainnet is stable, add at least one independent second seed location and release a config update only when it is operational.
