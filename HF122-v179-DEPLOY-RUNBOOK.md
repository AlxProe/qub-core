# HF122 / QUB Core v1.7.9 End-to-End Deployment Runbook

This runbook deploys HF122 without changing consensus. Protocol Epoch 2 remains active at #24000.

## Release artifacts

```text
HF122-v179-source-no-assets.zip
HF122-v179-patch-no-assets.patch
HF122-v179-SHA256SUMS.txt
HF122-v179-SECURITY-REVIEW.md
QUB_Explorer_Static_v0_7-no-assets.zip
QUB_Explorer_Static_v0_7-SHA256SUMS.txt
```

## Hard stop conditions

Stop immediately on:

```text
source ZIP SHA mismatch
cargo test/build failure
regtest RPC E2E failure
real-mainnet validate/preflight failure
post-#24000 tip block version != 2
seed branch mismatch or port failure
RPC unexpectedly listening on a public seed
headless token permission failure
snapshot tip/tail mismatch
bad Windows EXE header
manifest/SHA mismatch
Explorer JavaScript syntax failure
```

---

# A. Windows local release gate

## A1. Extract and copy assets

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF122-v179-source-no-assets.zip"
$Work = Join-Path $ProjectRoot "qubd-v1.7.9"
$Extract = Join-Path $ProjectRoot "_hf122_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.7.9-before-hf122"

$Sums = Join-Path $ProjectRoot "HF122-v179-SHA256SUMS.txt"

if (-not (Test-Path $Sums)) {
    throw "Checksum file not found: $Sums"
}

$SumLine = Get-Content $Sums -Encoding ASCII |
    Where-Object { $_ -match 'HF122-v179-source-no-assets\.zip$' } |
    Select-Object -First 1

if (-not $SumLine) {
    throw "Source ZIP checksum entry not found in $Sums"
}

$Expected = ($SumLine -split '\s+')[0].ToUpperInvariant()
$Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

"HF122 ZIP SHA256: $Actual"

if ($Actual -ne $Expected) {
    throw "HF122 source ZIP SHA mismatch. Expected $Expected got $Actual"
}

Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) {
    Rename-Item $Work $Backup
}

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.7.9") $Work

cd $Work
```

Copy runtime assets from the current deployed tree:

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.7.8\assets",
    "$env:LOCALAPPDATA\Programs\Qubit Coin Core\assets"
)

$AssetSource = $AssetSources |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1

if (-not $AssetSource) {
    throw "No QUB Core assets source found."
}

Remove-Item -Recurse -Force .\assets -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force .\assets | Out-Null
Copy-Item (Join-Path $AssetSource "*") .\assets -Recurse -Force

"Assets copied from: $AssetSource"
```

## A2. Marker gate

```powershell
Get-Content .\Cargo.toml -Encoding UTF8 |
    Select-String 'version = "1.7.9"'

Select-String -Path .\src\bin\qub_core.rs `
    -Pattern 'APP_VERSION: &str = "v1.7.9"'

Select-String -Path .\src\p2p.rs `
    -Pattern 'QUB Core:1.7.9|start_embedded'

Select-String -Path .\src\rpc.rs `
    -Pattern 'RPC_VERSION: &str = "1.7.9"|constant_time_token_eq|template-batch|submit-block|allowed_cidrs'

Select-String -Path .\src\lib.rs `
    -Pattern 'MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT|PROTOCOL_EPOCH_2_BLOCK_VERSION|mining_stats_json|max_cached_jobs must be >= max_template_batch'

Select-String -Path .\src\bin\qub_rpc_miner.rs `
    -Pattern 'APP_VERSION: &str = "v1.7.9"|Stock Bitcoin Stratum/AxeOS devices require a separate QUB adapter'

Select-String -Path .\tests\v1_core.rs `
    -Pattern 'hf122_mining_observability_detects_exact_two_label_alternation|hf122_rpc_config_is_bounded_and_protocol_epoch_2_is_unchanged'
```

## A3. Real Cargo build/test gate

```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

Warnings alone do not stop release. Any `error[...]` stops release.

## A4. Regtest RPC E2E

```powershell
py .\scripts\test-hf122-rpc-regtest.py `
    --qubd .\target\release\qubd.exe `
    --miner .\target\release\qub-rpc-miner.exe
```

Mandatory ending:

```text
HF122 RPC REGTEST E2E: PASS
```

## A5. Real-mainnet consensus/status gate

Close the GUI for this gate.

```powershell
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9"
$QUBD = "$Root\target\release\qubd.exe"
$SourceCfg = "$Root\config\mainnet.toml"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf122-v179.toml"

$DataDirToml = $RealDataDir -replace "\\", "/"
$Text = Get-Content $SourceCfg -Raw -Encoding UTF8

if ($Text -match '(?m)^data_dir\s*=') {
    $Text = [regex]::Replace(
        $Text,
        '(?m)^data_dir\s*=.*$',
        "data_dir = `"$DataDirToml`""
    )
}
elseif ($Text -match '(?m)^\[node\]\s*$') {
    $Text = [regex]::Replace(
        $Text,
        '(?m)^\[node\]\s*$',
        "[node]`r`ndata_dir = `"$DataDirToml`""
    )
}
else {
    $Text = "[node]`r`ndata_dir = `"$DataDirToml`"`r`n`r`n" + $Text
}

[System.IO.File]::WriteAllText(
    $TmpCfg,
    $Text,
    [System.Text.UTF8Encoding]::new($false)
)

$StatusText = (& $QUBD --config $TmpCfg status-fast 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw "status-fast failed:`n$StatusText" }
$Status = $StatusText | ConvertFrom-Json

$Status | Format-List height,tip_hash,tip_block_version,next_block_expected_version,protocol_epoch_2_activation_height,status_source

if ([int]$Status.height -lt 24000) { throw "Local mainnet is below #24000." }
if ([int]$Status.tip_block_version -ne 2) { throw "Canonical tip is not v2." }
if ([int]$Status.next_block_expected_version -ne 2) { throw "Next block is not v2." }
if ([int]$Status.protocol_epoch_2_activation_height -ne 24000) { throw "Epoch anchor moved." }

& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) { throw "Full validate failed." }

$PreflightText = (& $QUBD --config $TmpCfg preflight 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw "Preflight failed:`n$PreflightText" }
$Preflight = $PreflightText | ConvertFrom-Json
if ($Preflight.ok -ne $true) { throw "Preflight ok=false." }

& $QUBD --config $TmpCfg peers
if ($LASTEXITCODE -ne 0) { throw "peers failed." }

& $QUBD --config $TmpCfg mempool
if ($LASTEXITCODE -ne 0) { throw "mempool failed." }

"HF122 LOCAL MAINNET GATE: PASS"
```

## A6. GUI smoke

```powershell
cargo run --release --bin qub-core -- --config $TmpCfg
```

Check:

```text
v1.7.9 displayed
wallets preserved
sync beyond #24000
canonical v2 tip
Melt/Infuse responsive
mining still works
no new activation/rollback/address rule
```

---

# B. Source upload and seed builds

## B1. Variables and source archive

```powershell
$Key = "$env:USERPROFILE\.ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"
$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9"
$Deploy = "C:\temp\qubd_hf122_seed_deploy"
$Tar = "C:\temp\qubd-hf122-v179-source.tar.gz"

Remove-Item -Recurse -Force $Deploy -ErrorAction SilentlyContinue
Remove-Item -Force $Tar -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Deploy | Out-Null

robocopy $Src $Deploy /E `
    /XD .git target dist data .gradle .idea node_modules `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json .env .env.*

if ($LASTEXITCODE -gt 7) { throw "robocopy failed with $LASTEXITCODE" }

tar -czf $Tar -C $Deploy .
Get-FileHash $Tar -Algorithm SHA256

scp -i $Key $Tar "${AMS3}:/tmp/qubd-hf122-v179-source.tar.gz"
scp -i $Key $Tar "${NYC3}:/tmp/qubd-hf122-v179-source.tar.gz"
```

## B2. Build/test script for both seeds

```powershell
$BuildSeed = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf122-v179-source.tar.gz
MAINCFG=/opt/qub/mainnet/mainnet-seed.toml

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 \
  deploy/digitalocean/publish-mainnet-snapshot.sh \
  deploy/digitalocean/test-publish-mainnet-snapshot.sh \
  deploy/digitalocean/generate-rpc-token.sh \
  scripts/test-hf122-rpc-regtest.py

grep -R 'QUB Core:1.7.9' -n src/p2p.rs
grep -R 'RPC_VERSION: &str = "1.7.9"' -n src/rpc.rs
grep -R 'MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT' -n src/lib.rs
grep -R 'hf122_mining_observability_detects_exact_two_label_alternation' -n tests/v1_core.rs

cargo test --lib --tests
cargo build --release --bin qubd
cargo build --release --bin qub-rpc-miner

ls -lah target/release/qubd target/release/qub-rpc-miner
file target/release/qubd target/release/qub-rpc-miner

# The seed config keeps RPC disabled. This is only a canonical-state gate.
timeout --signal=TERM --kill-after=20s 600s \
  target/release/qubd --config "$MAINCFG" status-fast \
  > /tmp/hf122-seed-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf122-seed-status.json'))
print(sf)
if sf.get('ok') is not True: raise SystemExit('status-fast ok=false')
if int(sf.get('height',0)) < 24000: raise SystemExit('height below #24000')
if int(sf.get('tip_block_version',0)) != 2: raise SystemExit('tip not v2')
if int(sf.get('next_block_expected_version',0)) != 2: raise SystemExit('next not v2')
if int(sf.get('protocol_epoch_2_activation_height',0)) != 24000: raise SystemExit('epoch anchor moved')
PY

echo 'HF122 SEED BUILD GATE: PASS'
'@

$BuildSeed | ssh -i $Key $NYC3 "tr -d '\r' | bash -s"
$BuildSeed | ssh -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# C. Seed canary and rollout

RPC must remain disabled in `/opt/qub/mainnet/mainnet-seed.toml` and `/opt/qub/testnet/testnet-seed.toml`.

## C1. NYC3 canary

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
MAINCFG=/opt/qub/mainnet/mainnet-seed.toml
UNIT_MAIN=qub-seed-mainnet.service
UNIT_TEST=qub-seed-testnet.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.7.9-hf122-$STAMP"

rollback() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo systemctl stop "$UNIT_MAIN" 2>/dev/null || true
    sudo systemctl stop "$UNIT_TEST" 2>/dev/null || true
    [ -f "$BACKUP" ] && sudo install -m 0755 -o root -g root "$BACKUP" "$BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT_MAIN" || true
    sudo systemctl start "$UNIT_TEST" 2>/dev/null || true
  fi
  exit "$rc"
}
trap rollback EXIT

# Public seed RPC must remain disabled.
if grep -A20 '^\[rpc\]' "$MAINCFG" 2>/dev/null | grep -qE '^enabled[[:space:]]*=[[:space:]]*true'; then
  echo 'STOP: RPC is enabled in the public NYC3 seed config.'
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"
STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"

sudo systemctl stop "$UNIT_MAIN"
sudo systemctl stop "$UNIT_TEST" 2>/dev/null || true
sudo install -m 0755 -o root -g root "$STAGED" "$BIN"

[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$UNIT_MAIN"
sudo systemctl start "$UNIT_TEST" 2>/dev/null || true

for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$UNIT_MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo journalctl -u "$UNIT_MAIN" -n 200 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]
sudo strings "/proc/$PID/exe" | grep 'QUB Core:1.7.9'

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$MAINCFG" status-fast \
  > /tmp/hf122-nyc3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf122-nyc3-status.json'))
if sf.get('ok') is not True: raise SystemExit('ok=false')
if int(sf.get('height',0)) < 24000: raise SystemExit('below #24000')
if int(sf.get('tip_block_version',0)) != 2: raise SystemExit('tip not v2')
if int(sf.get('next_block_expected_version',0)) != 2: raise SystemExit('next not v2')
print('NYC3 height',sf.get('height'),'tip',sf.get('tip_hash'))
PY

# RPC must not be listening on the public seed.
if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: RPC port 17445 is unexpectedly listening on NYC3.'
  exit 1
fi

trap - EXIT
echo 'NYC3 HF122 CANARY: PASS'
'@ | ssh -i $Key $NYC3 "tr -d '\r' | bash -s"
```

```powershell
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

## C2. AMS3 rollout

Use the same logic, additionally pausing the snapshot timer. The HF121 r3 publisher remains canonical; install the source copy only if it differs.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
SNAP_SRC="$SRC/deploy/digitalocean/publish-mainnet-snapshot.sh"
SNAP_BIN=/opt/qub/bin/publish-mainnet-snapshot.sh
MAINCFG=/opt/qub/mainnet/mainnet-seed.toml
UNIT_MAIN=qub-seed-mainnet.service
UNIT_TEST=qub-seed-testnet.service
SNAP_SERVICE=qub-mainnet-snapshot-publish.service
SNAP_TIMER=qub-mainnet-snapshot-publish.timer
STAMP="$(date +%Y%m%d-%H%M%S)"
BIN_BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.7.9-hf122-$STAMP"
SNAP_BACKUP="$SNAP_BIN.backup-before-v1.7.9-hf122-$STAMP"

rollback() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
    sudo timeout --kill-after=10s 20s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
    sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
    sudo systemctl stop "$UNIT_MAIN" 2>/dev/null || true
    sudo systemctl stop "$UNIT_TEST" 2>/dev/null || true
    [ -f "$BIN_BACKUP" ] && sudo install -m 0755 -o root -g root "$BIN_BACKUP" "$BIN"
    [ -f "$SNAP_BACKUP" ] && sudo install -m 0755 -o root -g root "$SNAP_BACKUP" "$SNAP_BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT_MAIN" || true
    sudo systemctl start "$UNIT_TEST" 2>/dev/null || true
    sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true
  fi
  exit "$rc"
}
trap rollback EXIT

if grep -A20 '^\[rpc\]' "$MAINCFG" 2>/dev/null | grep -qE '^enabled[[:space:]]*=[[:space:]]*true'; then
  echo 'STOP: RPC is enabled in the public AMS3 seed config.'
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BIN_BACKUP"
[ -f "$SNAP_BIN" ] && sudo cp "$SNAP_BIN" "$SNAP_BACKUP"
STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"

sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 20s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl stop "$UNIT_MAIN"
sudo systemctl stop "$UNIT_TEST" 2>/dev/null || true

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
if ! cmp -s "$SNAP_SRC" "$SNAP_BIN"; then
  sudo install -m 0755 -o root -g root "$SNAP_SRC" "$SNAP_BIN"
fi

[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$UNIT_MAIN"
sudo systemctl start "$UNIT_TEST" 2>/dev/null || true

for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$UNIT_MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo journalctl -u "$UNIT_MAIN" -n 200 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]
sudo strings "/proc/$PID/exe" | grep 'QUB Core:1.7.9'

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$MAINCFG" status-fast \
  > /tmp/hf122-ams3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf122-ams3-status.json'))
if sf.get('ok') is not True: raise SystemExit('ok=false')
if int(sf.get('height',0)) < 24000: raise SystemExit('below #24000')
if int(sf.get('tip_block_version',0)) != 2: raise SystemExit('tip not v2')
if int(sf.get('next_block_expected_version',0)) != 2: raise SystemExit('next not v2')
print('AMS3 height',sf.get('height'),'tip',sf.get('tip_hash'))
PY

sudo timeout --signal=TERM --kill-after=30s 900s "$SNAP_BIN"
sudo systemctl reset-failed "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl reset-failed "$SNAP_TIMER" 2>/dev/null || true
sudo systemctl start "$SNAP_TIMER"

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: RPC port 17445 is unexpectedly listening on AMS3 seed.'
  exit 1
fi

systemctl is-active "$UNIT_MAIN"
systemctl is-active "$SNAP_TIMER"
cat /srv/qub-updates/mainnet/snapshots/tip.json

trap - EXIT
echo 'AMS3 HF122 SEED DEPLOYMENT: PASS'
'@ | ssh -i $Key $AMS3 "tr -d '\r' | bash -s"
```

## C3. Public seed checks

```powershell
Test-NetConnection seed.qubit-coin.io -Port 17444
Test-NetConnection seed-ams3.qubit-coin.io -Port 17444
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

---

# D. Separate production headless node

A dedicated droplet/VM with at least 4 GiB RAM and adequate disk is recommended. Do not use NYC3 because it already has resource pressure history. Do not expose RPC port 17445 publicly.

Set:

```powershell
$HEADLESS = "deploy@<HEADLESS_IPV4>"
```

Upload the same source archive:

```powershell
scp -i $Key $Tar "${HEADLESS}:/tmp/qubd-hf122-v179-source.tar.gz"
```

Install Rust/build dependencies if absent, then build:

```powershell
@'
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  sudo apt-get update
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y build-essential pkg-config libssl-dev curl ca-certificates
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
  sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain stable --no-modify-path
fi

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/qub/headless/src
sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy /opt/qub/headless
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf /tmp/qubd-hf122-v179-source.tar.gz -C "$SRC"
cd "$SRC"

cargo test --lib --tests
cargo build --release --bin qubd
cargo build --release --bin qub-rpc-miner

sudo install -d -m 0755 -o root -g root /opt/qub/bin
sudo install -m 0755 -o root -g root target/release/qubd /opt/qub/bin/qubd
sudo install -m 0755 -o root -g root target/release/qub-rpc-miner /opt/qub/bin/qub-rpc-miner

sudo install -d -m 0750 -o deploy -g deploy /opt/qub/headless/config /opt/qub/headless/data/mainnet
sudo install -m 0640 -o deploy -g deploy config/headless-mainnet.toml /opt/qub/headless/config/mainnet.toml
sudo install -m 0755 -o root -g root deploy/digitalocean/generate-rpc-token.sh /opt/qub/bin/generate-rpc-token.sh
sudo -u deploy /opt/qub/bin/generate-rpc-token.sh /opt/qub/headless/config/rpc.token
sudo install -m 0644 -o root -g root deploy/digitalocean/qub-headless-mainnet.service /etc/systemd/system/qub-headless-mainnet.service

sudo systemctl daemon-reload
sudo systemctl enable --now qub-headless-mainnet.service
sudo systemctl status qub-headless-mainnet.service --no-pager -l
'@ | ssh -i $Key $HEADLESS "tr -d '\r' | bash -s"
```

Firewall: open P2P only; never open RPC.

```powershell
@'
set -e
sudo ufw allow OpenSSH
sudo ufw allow 17446/tcp
sudo ufw deny 17445/tcp
sudo ufw --force enable
sudo ufw status verbose
'@ | ssh -i $Key $HEADLESS "tr -d '\r' | bash -s"
```

RPC smoke on the host:

```powershell
@'
set -euo pipefail
TOKEN="$(tr -d '\r\n' < /opt/qub/headless/config/rpc.token)"

curl -sS -H "X-QUB-RPC-Token: $TOKEN" http://127.0.0.1:17445/rpc/v1/status | python3 -m json.tool
curl -sS -H "X-QUB-RPC-Token: $TOKEN" http://127.0.0.1:17445/rpc/v1/mining/status | python3 -m json.tool
curl -sS -H "X-QUB-RPC-Token: $TOKEN" 'http://127.0.0.1:17445/rpc/v1/mining/stats?window=256' | python3 -m json.tool

if sudo ss -ltnp | grep ':17445' | grep -q '0.0.0.0\|\[::\]'; then
  echo 'STOP: RPC is not loopback-only.'
  exit 1
fi

echo 'HEADLESS RPC LOOPBACK SMOKE: PASS'
'@ | ssh -i $Key $HEADLESS "tr -d '\r' | bash -s"
```

For Windows operator access, use an SSH tunnel instead of public RPC:

```powershell
ssh -i $Key -L 17445:127.0.0.1:17445 $HEADLESS
```

Do not enable the reference-miner systemd example until a deliberate payout address and worker count are configured. It is a CPU reference, not the Bitaxe adapter.

---

# E. Explorer v0.7

## E1. Extract and restore existing assets

```powershell
$ExplorerZip = "C:\Users\proes\Desktop\qub-node\QUB_Explorer_Static_v0_7-no-assets.zip"
$ExplorerWork = "C:\temp\QUB_Explorer_Static_v0_7"

Remove-Item -Recurse -Force $ExplorerWork -ErrorAction SilentlyContinue
Expand-Archive -Path $ExplorerZip -DestinationPath "C:\temp" -Force

# Copy the existing v0.6 runtime assets into the v0.7 assets folder before deploy.
$ExistingExplorerAssets = "C:\Users\proes\Desktop\qub-node\QUB_Explorer_Static_v0_6_FINAL2\assets"
if (Test-Path $ExistingExplorerAssets) {
    Copy-Item "$ExistingExplorerAssets\*" "$ExplorerWork\assets" -Recurse -Force
}
```

## E2. Static syntax marker gate

```powershell
Select-String -Path "$ExplorerWork\index.html" -Pattern 'Explorer v0.7|Mining|exact two-label alternation|effective label count|coinbase-only'
```

## E3. AMS3 static deployment (preserve live assets)

```powershell
scp -i $Key "$ExplorerWork\index.html" "${AMS3}:/tmp/qub-explorer-v0.7-index.html"
```

```powershell
@'
set -euo pipefail
STAMP="$(date +%Y%m%d-%H%M%S)"
ROOT=/srv/qub-explorer
sudo cp "$ROOT/index.html" "$ROOT/index.html.backup-before-v0.7-$STAMP"
sudo install -m 0644 -o root -g root /tmp/qub-explorer-v0.7-index.html "$ROOT/index.html"
ls -lah "$ROOT/index.html"
echo 'QUB Explorer v0.7 origin installed.'
'@ | ssh -i $Key $AMS3 "tr -d '\r' | bash -s"
```

No seed restart is required for a static `index.html` replacement.

---

# F. Windows distribution and update manifest

## F1. Build distribution

```powershell
cd "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9"
Remove-Item -Recurse -Force .\dist -ErrorAction SilentlyContinue

$ISCC = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found." }
$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
    -Config mainnet -BuildInstaller -SkipTests -SkipPreflight
```

Verify outputs include:

```text
QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe
QUB-Core-Latest.exe
tools/qubd.exe
tools/qub-rpc-miner.exe
```

## F2. Patch manifest

HF122 is mandatory infrastructure software but **not** a new chain upgrade:

```powershell
$Manifest = (Resolve-Path ".\dist\updates\mainnet\windows-x64\manifest.json").Path
$m = Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json

function Set-JsonProp($Object, [string]$Name, $Value) {
    if ($Object.PSObject.Properties.Name -contains $Name) { $Object.$Name = $Value }
    else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

Set-JsonProp $m "mandatory" $true
Set-JsonProp $m "chain_upgrade" $false
Set-JsonProp $m "hotfix" "HF122"
Set-JsonProp $m "activation_feature" "none"
Set-JsonProp $m "activation_height" 0
Set-JsonProp $m "protocol_epoch" 2
Set-JsonProp $m "protocol_epoch_2_activation_height" 24000
Set-JsonProp $m "post_activation_block_version" 2
Set-JsonProp $m "checkpoint_height" 10367
Set-JsonProp $m "checkpoint_hash" "21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000"
Set-JsonProp $m "consensus_family" "qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000"
Set-JsonProp $m "hotfix_family" "hf122-headless-authenticated-rpc-mining-worker-observability"
Set-JsonProp $m "notes" "Mandatory QUB Core v1.7.9 HF122 infrastructure release. HF122 introduces no new consensus activation; Protocol Epoch 2 remains active at #24000 with block version 2 required thereafter. HF122 completes authenticated QUB-native headless RPC, tracked solo and existing on-chain pool mining templates, compact parallel template batches, guarded tracked block submission, validated transaction submission, long-poll tip events, the qub-rpc-miner reference CPU worker, and objective mining distribution/streak/alternation/interval observability. Standard mainnet/testnet RPC remains disabled by default. The supplied headless configuration is loopback-only and token authenticated. Raw RPC has no built-in TLS and must not be exposed directly to the public Internet. Stock Bitcoin Stratum/AxeOS/Bitaxe hardware is not directly compatible yet and requires a separately reviewed QUB adapter. No rollback, address-specific rule, DAA change, checkpoint/genesis/economics change, or QUB/JIN change."

[System.IO.File]::WriteAllText($Manifest, ($m | ConvertTo-Json -Depth 40) + "`r`n", [System.Text.UTF8Encoding]::new($false))
```

## F3. Generate sidecars

```powershell
$UpdateDir = ".\dist\updates\mainnet\windows-x64"

Push-Location $UpdateDir

(Get-FileHash "manifest.json" -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII "manifest.json.sha256"

(Get-FileHash "QUB-Core-Latest.exe" -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII "QUB-Core-Latest.exe.sha256"

(Get-FileHash "QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe" -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII "QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe.sha256"

Get-ChildItem -File |
    Where-Object { $_.Name -ne "SHA256SUMS.txt" } |
    ForEach-Object {
        $Hash = (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
        "$Hash  $($_.Name)"
    } |
    Set-Content -Encoding ASCII "SHA256SUMS.txt"

Get-Content "SHA256SUMS.txt"
Pop-Location

(Get-FileHash ".\dist\updates\QUB-Core-Latest.exe" -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII ".\dist\updates\QUB-Core-Latest.exe.sha256"
```

## F4. Upload and install update files on AMS3

```powershell
ssh -i $Key $AMS3 `
    "rm -rf /tmp/qub-mainnet-windows-x64 && mkdir -p /tmp/qub-mainnet-windows-x64"

scp -i $Key -r `
    .\dist\updates\mainnet\windows-x64\* `
    "${AMS3}:/tmp/qub-mainnet-windows-x64/"

scp -i $Key `
    .\dist\updates\QUB-Core-Latest.exe `
    "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe"

scp -i $Key `
    .\dist\updates\QUB-Core-Latest.exe.sha256 `
    "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe.sha256"
```

```powershell
@'
set -euo pipefail

STAMP="$(date +%Y%m%d-%H%M%S)"
UPDATE_DIR=/srv/qub-updates/mainnet/windows-x64
LATEST=/srv/qub-updates/mainnet/QUB-Core-Latest.exe
LATEST_SHA=/srv/qub-updates/mainnet/QUB-Core-Latest.exe.sha256

sudo mkdir -p "$UPDATE_DIR"
sudo cp -a "$UPDATE_DIR" "/srv/qub-updates/mainnet/windows-x64.backup-before-v1.7.9-hf122-$STAMP"

[ ! -f "$LATEST" ] || sudo cp "$LATEST" "$LATEST.backup-before-v1.7.9-hf122-$STAMP"
[ ! -f "$LATEST_SHA" ] || sudo cp "$LATEST_SHA" "$LATEST_SHA.backup-before-v1.7.9-hf122-$STAMP"

sudo rsync -av --delete /tmp/qub-mainnet-windows-x64/ "$UPDATE_DIR/"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe "$LATEST"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe.sha256 "$LATEST_SHA"

sudo chmod -R 755 /srv/qub-updates/mainnet
sudo find /srv/qub-updates/mainnet -type f -exec chmod 644 {} \;

ls -lah "$UPDATE_DIR"
ls -lah "$LATEST" "$LATEST_SHA"
echo 'HF122 update files installed on origin.'
'@ | ssh -i $Key $AMS3 "tr -d '\r' | bash -s"
```

## F5. Origin update verification

```powershell
@'
set -euo pipefail

curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/windows-x64/manifest.json \
  | grep -E '"version"|"mandatory"|"chain_upgrade"|"hotfix"|"activation_feature"|"activation_height"|"protocol_epoch"|"protocol_epoch_2_activation_height"|"post_activation_block_version"|"hotfix_family"'

echo
echo 'Latest EXE header:'
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe \
  | head -c 2 | xxd -p

curl -kI --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe

curl -kI --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe

curl -kI --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
'@ | ssh -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Expected:

```text
version 1.7.9
mandatory true
chain_upgrade false
hotfix HF122
activation_feature none
activation_height 0
protocol_epoch 2
post_activation_block_version 2
EXE header 4d5a
HTTP 200
```

---

# G. Cloudflare purge and public verification

## G1. Purge exact URLs

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe.sha256
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/snapshots/tip.json
https://download.qubit-coin.io/mainnet/snapshots/tip.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
https://download.qubit-coin.io/mainnet/snapshots/tail-64.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-256.json
https://download.qubit-coin.io/mainnet/snapshots/tail-256.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json
https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json
https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json
https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json.sha256
https://download.qubit-coin.io/mainnet/snapshots/chain.json
https://download.qubit-coin.io/mainnet/snapshots/chain.json.sha256
https://download.qubit-coin.io/mainnet/canonical-chain.json
https://download.qubit-coin.io/mainnet/canonical-chain.json.sha256
https://download.qubit-coin.io/
https://download.qubit-coin.io/index.html
https://explorer.qubit-coin.io/
```

## G2. Public manifest, installer and snapshot verification

```powershell
$m = curl.exe -L `
    "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json?verify=179-hf122" |
    ConvertFrom-Json

$m | Format-List version,mandatory,chain_upgrade,hotfix,activation_feature,activation_height,protocol_epoch,protocol_epoch_2_activation_height,post_activation_block_version,hotfix_family,notes

if ($m.version -ne "1.7.9") { throw "Bad public version." }
if ($m.hotfix -ne "HF122") { throw "Bad public hotfix." }
if ($m.mandatory -ne $true) { throw "mandatory is not true." }
if ($m.chain_upgrade -ne $false) { throw "HF122 incorrectly reports a new chain upgrade." }
if ($m.activation_feature -ne "none") { throw "Bad activation feature." }
if ([int]$m.activation_height -ne 0) { throw "Bad activation height." }
if ([int]$m.protocol_epoch -ne 2) { throw "Bad protocol epoch." }
if ([int]$m.protocol_epoch_2_activation_height -ne 24000) { throw "Epoch anchor moved." }
if ([int]$m.post_activation_block_version -ne 2) { throw "Bad post-activation version." }

$TempLatest = "$env:TEMP\QUB-Core-Latest-v179-hf122.exe"
curl.exe -L `
    "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe?verify=179-hf122" `
    -o $TempLatest

$Bytes = [System.IO.File]::ReadAllBytes($TempLatest)
if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw "Public latest is not an EXE." }

$ExpectedHash = $m.sha256.ToLowerInvariant()
$ActualHash = (Get-FileHash $TempLatest -Algorithm SHA256).Hash.ToLowerInvariant()
if ($ActualHash -ne $ExpectedHash) { throw "Public latest SHA mismatch." }

$Tip = curl.exe -L `
    "https://download.qubit-coin.io/mainnet/snapshots/tip.json?verify=179-hf122" |
    ConvertFrom-Json
$Tail = curl.exe -L `
    "https://download.qubit-coin.io/mainnet/snapshots/tail-64.json?verify=179-hf122" |
    ConvertFrom-Json

if ([int]$Tip.height -lt 24000) { throw "Snapshot is pre-activation." }
if ([int]$Tail.tip_height -ne [int]$Tip.height) { throw "Snapshot height mismatch." }
if ($Tail.tip_hash -ne $Tip.tip_hash) { throw "Snapshot hash mismatch." }
if ([int]$Tail.blocks[-1].header.version -ne 2) { throw "Snapshot tip is not v2." }

$ExplorerHtml = curl.exe -L "https://explorer.qubit-coin.io/?verify=07-hf122"
if ($ExplorerHtml -notmatch 'QUB Explorer v0\.7') { throw "Explorer v0.7 marker missing." }
if ($ExplorerHtml -notmatch 'Exact two-label alternation') { throw "Explorer Mining page marker missing." }

"HF122 PUBLIC VERIFICATION: PASS"
```

## G3. Clean-install smoke

```powershell
$Installer = "$env:TEMP\QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe"
curl.exe -L `
    "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe?final=179-hf122" `
    -o $Installer
Start-Process $Installer -Wait
```

Check:

```text
QUB Core v1.7.9
wallets/outbox preserved
sync and mining work
canonical post-#24000 v2 tip
Melt/Infuse responsive
tools/qubd.exe present
tools/qub-rpc-miner.exe present
standard mainnet RPC remains disabled
no new consensus activation
```

---

# H. GitHub repository update

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9"
$Repo = "C:\Users\proes\Desktop\qub-node\qub-core-opensource"
$Sync = Join-Path $ProjectRoot "_repo_sync_hf122"

if (-not (Test-Path $Repo)) {
    git clone https://github.com/AlxProe/qub-core.git $Repo
}

cd $Repo
if ((git status --porcelain).Length -gt 0) { throw "Repo contains uncommitted changes." }
git pull origin main

Remove-Item -Recurse -Force $Sync -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Sync | Out-Null

robocopy $Src $Sync /E `
    /XD .git target dist data .gradle .idea node_modules `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json .env .env.*

if ($LASTEXITCODE -gt 7) { throw "Source mirror failed with $LASTEXITCODE" }

$Assets = Join-Path $Sync "assets"
Remove-Item -Recurse -Force $Assets -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Assets | Out-Null
@'
# Assets

Runtime image, audio and font assets are not included in this source repository.
Never commit private keys, wallet files, chain data, installers or build artifacts.
'@ | Set-Content -Encoding UTF8 (Join-Path $Assets "README.md")

foreach ($File in @("LICENSE","NOTICE","SECURITY.md","CONTRIBUTING.md","CODE_OF_CONDUCT.md",".gitignore")) {
    $SourceFile = Join-Path $Repo $File
    if (Test-Path $SourceFile) { Copy-Item $SourceFile (Join-Path $Sync $File) -Force }
}
if ((Test-Path (Join-Path $Repo ".github")) -and -not (Test-Path (Join-Path $Sync ".github"))) {
    Copy-Item (Join-Path $Repo ".github") (Join-Path $Sync ".github") -Recurse -Force
}

$Forbidden = Get-ChildItem $Sync -Recurse -Force -File | Where-Object {
    $_.Extension -in @('.exe','.dll','.pdb','.ilk','.zip','.tar','.gz','.7z') -or
    $_.Name -in @('wallet.json','ethereum-wallets.json','wallet-pending-txs.json','chain.json','chain-status.json','.env') -or
    $_.Name -like '.env.*'
}
if ($Forbidden) { $Forbidden | Select-Object FullName; throw "Forbidden files in repo mirror." }

robocopy $Sync $Repo /MIR /XD ".git"
if ($LASTEXITCODE -gt 7) { throw "Repository mirror failed with $LASTEXITCODE" }

cd $Repo
git diff --check
git status --short
git add .

git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh
git update-index --chmod=+x deploy/digitalocean/generate-rpc-token.sh
git update-index --chmod=+x scripts/test-hf122-rpc-regtest.py

git diff --cached --summary
git commit -m "Update QUB Core to v1.7.9 HF122"

if (git tag -l "v1.7.9-hf122") { throw "Tag v1.7.9-hf122 already exists." }
git tag -a v1.7.9-hf122 -m "QUB Core v1.7.9 HF122"
git push origin main
git push origin v1.7.9-hf122
```

GitHub release:

```powershell
# If GitHub CLI is installed:
gh release create v1.7.9-hf122 `
    --title "QUB Core v1.7.9 HF122 - Headless RPC & Mining Infrastructure" `
    --notes-file ".\RELEASE_NOTES-v1.7.9-HF122.md"

# Manual fallback:
Get-Content .\RELEASE_NOTES-v1.7.9-HF122.md -Raw -Encoding UTF8 | Set-Clipboard
Start-Process "https://github.com/AlxProe/qub-core/releases/new?tag=v1.7.9-hf122"
```

Do not attach runtime wallets, chain state or Windows build folders. The GitHub tag is the canonical public source. The reviewed no-assets ZIP and checksum file may optionally be attached.

---

# I. Telegram post

```text
🆕 QUB Core v1.7.9 HF122 is live.

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.7.9-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.7.9-hf122

🔎 Explorer:
https://explorer.qubit-coin.io/#/mining

HF122 is a mandatory infrastructure release and does not introduce a new chain activation.

Protocol Epoch 2 remains active exactly as deployed at block #24000:
- blocks below #24000 use version 1;
- blocks from #24000 onward require version 2.

No rollback.
No address-specific mining rule.
No DAA change.
No checkpoint, genesis or economics change.
No QUB/JIN rule change.

✅ Headless node / RPC
- Production QUB-native authenticated RPC inside qubd node.
- Canonical status, block, transaction and mempool endpoints.
- Tracked solo and existing on-chain pool mining templates.
- Compact independent template batches for parallel workers.
- Canonical-parent, expiry, version and PoW guards on submit.
- Validated transaction submission and long-poll tip events.

✅ Reference miner
- Added qub-rpc-miner, the official QUB-native reference CPU worker.
- Supports solo payout addresses and existing on-chain pool IDs.
- This is the protocol reference for upcoming official pool and Dedicated QUB Miner infrastructure.

Important: stock Bitcoin Stratum/AxeOS/Bitaxe hardware is not directly compatible yet. A separate QUB adapter will be released after real hardware testing.

✅ Mining transparency
QUB Explorer v0.7 adds objective 64/256/1024/4096-block mining analytics:
- payout/pool-label distribution;
- top-label share;
- HHI and effective label count;
- same-label streaks;
- exact two-label alternation;
- coinbase-only percentage;
- average, median and p90 block intervals;
- block-version distribution.

A payout address or pool label is an observable on-chain label, not proof of a unique human, machine, cluster or operator.

✅ Security defaults
- RPC remains disabled in standard mainnet/testnet configs.
- The supplied headless config is loopback-only and token authenticated.
- Remote access requires explicit allowlisting.
- RPC has no built-in TLS and must not be exposed directly to the public Internet.
- Arbitrary untracked block submission is not available.

HF117 through HF121 remain included.

This release completes the software foundation for:
- official headless QUB nodes;
- official pool mining;
- Dedicated QUB Miner devices;
- later QUB-native infrastructure integrations.

Everyone running QUB Core should update to v1.7.9 HF122.

Do not delete wallet.json.
Do not delete ethereum-wallets.json.
Do not delete wallet-pending-txs.json while transactions are pending.
Do not delete your QUB Core data directory.
Never share private keys, wallet files, RPC tokens or seed phrases.
```
