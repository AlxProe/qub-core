# QUB Explorer v0.2 deploy notes

Explorer v0.2 is a static, read-only explorer. It should be served from explorer.qubit-coin.io and same-origin snapshot paths should be exposed at /mainnet/snapshots/*.

New required assets:
- qub.webp
- jin.webp
- banner.png
- favicon.png

Deploy by uploading index.html and assets/ to /srv/qub-explorer, then mirroring to /srv/qub-updates/explorer if using the Docker Caddy setup.
