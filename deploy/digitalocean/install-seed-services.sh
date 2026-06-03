#!/usr/bin/env bash
set -euo pipefail

SRC_DIR="${1:-/opt/jinex/staging/src/qubd}"
QUB_ROOT="/opt/qub"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo not found. Install Rust stable first: https://rustup.rs" >&2
  exit 1
fi

cd "$SRC_DIR"
cargo test
cargo build --release --bin qubd

sudo install -d -o deploy -g deploy "$QUB_ROOT/bin" "$QUB_ROOT/mainnet" "$QUB_ROOT/testnet"
sudo install -m 0755 target/release/qubd "$QUB_ROOT/bin/qubd"
sudo install -m 0644 deploy/digitalocean/mainnet-seed.toml "$QUB_ROOT/mainnet/mainnet-seed.toml"
sudo install -m 0644 deploy/digitalocean/testnet-seed.toml "$QUB_ROOT/testnet/testnet-seed.toml"
sudo install -m 0644 deploy/digitalocean/qub-seed-mainnet.service /etc/systemd/system/qub-seed-mainnet.service
sudo install -m 0644 deploy/digitalocean/qub-seed-testnet.service /etc/systemd/system/qub-seed-testnet.service
sudo systemctl daemon-reload
sudo systemctl enable --now qub-seed-mainnet.service
sudo systemctl enable --now qub-seed-testnet.service
sudo systemctl --no-pager status qub-seed-mainnet.service qub-seed-testnet.service
