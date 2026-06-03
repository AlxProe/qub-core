#!/usr/bin/env bash
set -euo pipefail
SRC_DIR="${1:-/opt/jinex/staging/src/qubd}"
NETWORK="${2:-mainnet}"
BIND="${3:-127.0.0.1:18765}"
if [[ "$NETWORK" != "mainnet" && "$NETWORK" != "testnet" ]]; then
  echo "usage: $0 /path/to/qubd [mainnet|testnet] [127.0.0.1:18765]" >&2
  exit 2
fi
cd "$SRC_DIR"
if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust stable first." >&2
  exit 1
fi
cargo build --release --bin qubd
sudo install -D -m 0755 target/release/qubd /opt/qub/bin/qubd
CONFIG="/opt/qub/$NETWORK/${NETWORK}-seed.toml"
if [[ ! -f "$CONFIG" ]]; then
  echo "missing $CONFIG; install seed services first" >&2
  exit 1
fi
SERVICE="qub-explorer-api-${NETWORK}.service"
sudo tee "/etc/systemd/system/$SERVICE" >/dev/null <<EOF
[Unit]
Description=Qubit Coin ${NETWORK} read-only explorer API
After=network-online.target qub-seed-${NETWORK}.service
Wants=network-online.target

[Service]
User=deploy
Group=deploy
WorkingDirectory=/opt/qub/${NETWORK}
ExecStart=/opt/qub/bin/qubd --config ${CONFIG} explorer-api ${BIND}
Restart=always
RestartSec=3
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=full
ProtectHome=true
ReadWritePaths=/opt/qub/${NETWORK}

[Install]
WantedBy=multi-user.target
EOF
sudo systemctl daemon-reload
sudo systemctl enable --now "$SERVICE"
sudo systemctl status "$SERVICE" --no-pager
