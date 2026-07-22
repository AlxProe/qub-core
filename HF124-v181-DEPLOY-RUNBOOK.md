# HF124 / QUB Core v1.8.1 End-to-End Deployment Runbook

## Release identity

```text
QUB Core: v1.8.1
Hotfix: HF124
Package generation: v181
Consensus activation introduced by HF124: none
Active protocol epoch: 2
Protocol Epoch 2 activation height: #24000
Post-activation block version: 2
Mainnet pool-share consensus cap: 128 per block
```

HF124 is mandatory because older official template builders can construct a block containing more pool shares than consensus accepts, and GUI miners can repeatedly abandon proof-of-work on mempool-only changes.

Do not delete wallet files or `chain-v2/`. Do not run two state-changing QUB processes against one data directory.

---

# A. Windows extraction and local release gates

## A1. Verify the source archive

Place these files in:

```text
C:\Users\proes\Desktop\qub-node
```

```text
HF124-v181-source-no-assets.zip
HF124-v181-SHA256SUMS.txt
```

PowerShell:

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = "Stop"

    $ProjectRoot = "C:\Users\proes\Desktop\qub-node"
    $Zip = Join-Path $ProjectRoot "HF124-v181-source-no-assets.zip"
    $Sums = Join-Path $ProjectRoot "HF124-v181-SHA256SUMS.txt"

    if (-not (Test-Path $Zip)) {
        throw "HF124 source ZIP not found: $Zip"
    }

    if (-not (Test-Path $Sums)) {
        throw "HF124 SHA256SUMS not found: $Sums"
    }

    $Line = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF124-v181-source-no-assets\.zip$' } |
        Select-Object -First 1

    if (-not $Line) {
        throw "HF124 source ZIP entry missing from SHA256SUMS."
    }

    $Expected = ($Line -split '\s+')[0].ToUpperInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

    "Expected: $Expected"
    "Actual:   $Actual"

    if ($Actual -ne $Expected) {
        throw "HF124 source ZIP SHA mismatch."
    }

    Write-Host "HF124 SOURCE ZIP GATE: PASS"
}
```

## A2. Clean extraction

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF124-v181-source-no-assets.zip"
$Work = Join-Path $ProjectRoot "qubd-v1.8.1"
$Extract = Join-Path $ProjectRoot "_hf124_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.8.1-before-hf124"

Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) {
    Rename-Item $Work $Backup
}

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.8.1") $Work

Set-Location $Work
```

## A3. Copy known-good runtime assets

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.0\assets",
    "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9\assets",
    "$env:LOCALAPPDATA\Programs\Qubit Coin Core\assets"
)

$AssetSource = $AssetSources |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1

if (-not $AssetSource) {
    throw "No runtime assets source found."
}

Remove-Item -Recurse -Force .\assets -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force .\assets | Out-Null
Copy-Item (Join-Path $AssetSource "*") .\assets -Recurse -Force

"Assets copied from: $AssetSource"
```

## A4. Source marker gate

```powershell
Select-String -Path .\Cargo.toml `
    -Pattern 'version = "1.8.1"|HF124|mining liveness'

Select-String -Path .\build.rs `
    -Pattern 'ProductVersion", "1.8.1"|FileVersion", "1.8.1.0"'

Select-String -Path .\src\p2p.rs `
    -Pattern 'QUB Core:1.8.1|hf124_fair_mempool_relay_batch|relay_mempool_batch_to_known_peers|HF124 coalesced mempool persistence|capture one immutable Fast Chain Engine snapshot'

Select-String -Path .\src\lib.rs `
    -Pattern 'hf124_pool_share_mempool_limit|accept_transactions_to_mempool_batch|selected_pool_shares|non_pool_transactions.is_empty'

Select-String -Path .\src\pools.rs `
    -Pattern 'HF124_POOL_REGISTRY_CACHE_ENTRIES|HF124_POOL_WINDOW_CACHE_ENTRIES|cached_pools_registry_from_blocks|declared_share_txs|leave room for coinbase'

Select-String -Path .\src\bin\qub_core.rs `
    -Pattern 'APP_VERSION: &str = "v1.8.1"|one local share per canonical parent|HF124 stable solo template|HF124 stable pool template|pool shares are frequent, short-lived work markers'

Select-String -Path .\tests\v1_core.rs `
    -Pattern 'hf124_candidate_parts_are_reused_without_rebuilding_transaction_selection|hf124_candidate_caps_pool_shares_and_keeps_ordinary_transactions|hf124_pool_share_mempool_policy_is_bounded_to_confirmable_horizon|hf124_consensus_still_rejects_more_than_128_pool_shares|hf124_candidate_drains_oldest_confirmable_shares_first|hf124_same_tip_persistence_merges_concurrent_mempool_entries|hf124_stale_pool_share_does_not_block_ordinary_mempool_admission|hf124_rebuild_skips_stale_legacy_prefix_and_retains_newer_valid_shares|hf124_rebuild_caps_legacy_share_queue_before_general_mempool_limit|hf124_pool_shares_are_not_persisted_or_resurrected_by_wallet_outbox'
```

Forbidden old hot-path gate:

```powershell
$OldPatterns = @(
    'Mempool changed; rebuilding block template',
    'Mempool changed; rebuilding pool block template',
    'base_mempool_fingerprint',
    'fn mempool_fingerprint'
)

$Hits = foreach ($Pattern in $OldPatterns) {
    Select-String `
        -Path .\src\bin\qub_core.rs `
        -Pattern $Pattern `
        -SimpleMatch `
        -ErrorAction SilentlyContinue
}

if ($Hits) {
    $Hits | Format-Table Path, LineNumber, Line -AutoSize
    throw "Pre-HF124 mempool-triggered mining restart code remains."
}

"HF124 SOURCE MARKER GATE: PASS"
```

## A5. Real Cargo gates

```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

STOP on any real compile error or failed test.

## A6. HF124-specific tests

```powershell
cargo test hf124_ -- --nocapture
```

Expected tests include:

```text
hf124_candidate_parts_are_reused_without_rebuilding_transaction_selection
hf124_candidate_caps_pool_shares_and_keeps_ordinary_transactions
hf124_pool_share_mempool_policy_is_bounded_to_confirmable_horizon
hf124_consensus_still_rejects_more_than_128_pool_shares
hf124_candidate_drains_oldest_confirmable_shares_first
hf124_same_tip_persistence_merges_concurrent_mempool_entries
hf124_stale_pool_share_does_not_block_ordinary_mempool_admission
hf124_rebuild_skips_stale_legacy_prefix_and_retains_newer_valid_shares
hf124_rebuild_caps_legacy_share_queue_before_general_mempool_limit
hf124_pool_shares_are_not_persisted_or_resurrected_by_wallet_outbox
hf124_relay_batch_reserves_space_for_ordinary_transactions
hf124_relay_batch_never_relays_more_than_one_block_of_shares
```

## A7. Retained Fast Chain Engine and RPC E2E

```powershell
py .\scripts\test-hf123-fast-chain-regtest.py `
    --qubd .\target\release\qubd.exe

py .\scripts\test-hf123-rpc-regtest.py `
    --qubd .\target\release\qubd.exe `
    --miner .\target\release\qub-rpc-miner.exe
```

Expected endings:

```text
HF123 FAST CHAIN ENGINE REGTEST E2E: PASS
HF123 RPC REGTEST E2E: PASS
```

## A8. Real mainnet post-#24000 gate

Close the running GUI before full validation.

```powershell
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.1"
$QUBD = Join-Path $Root "target\release\qubd.exe"
$SourceCfg = Join-Path $Root "config\mainnet.toml"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf124-v181.toml"

if (-not (Test-Path $QUBD)) { throw "qubd.exe missing." }
if (-not (Test-Path $RealDataDir)) { throw "Mainnet data missing." }

$DataDirToml = $RealDataDir -replace '\\', '/'
$Cfg = Get-Content $SourceCfg -Raw -Encoding UTF8

if ($Cfg -match '(?m)^data_dir\s*=') {
    $Cfg = [regex]::Replace(
        $Cfg,
        '(?m)^data_dir\s*=.*$',
        "data_dir = `"$DataDirToml`""
    )
}
elseif ($Cfg -match '(?m)^\[node\]\s*$') {
    $Cfg = [regex]::Replace(
        $Cfg,
        '(?m)^\[node\]\s*$',
        "[node]`r`ndata_dir = `"$DataDirToml`""
    )
}
else {
    $Cfg = "[node]`r`ndata_dir = `"$DataDirToml`"`r`n`r`n" + $Cfg
}

[System.IO.File]::WriteAllText(
    $TmpCfg,
    $Cfg,
    [System.Text.UTF8Encoding]::new($false)
)
```

Status/storage:

```powershell
$Status = & $QUBD --config $TmpCfg status-fast | ConvertFrom-Json
$Status | Format-List network,height,tip_hash,tip_block_version,next_block_expected_version,storage_engine,mempool_tx_count

if ($Status.ok -ne $true) { throw "status-fast ok=false" }
if ($Status.network -ne "mainnet") { throw "wrong network" }
if ([int]$Status.height -lt 24000) { throw "height below #24000" }
if ([int]$Status.tip_block_version -ne 2) { throw "tip is not v2" }
if ([int]$Status.next_block_expected_version -ne 2) { throw "next version is not v2" }

& $QUBD --config $TmpCfg storage-stats
if ($LASTEXITCODE -ne 0) { throw "storage-stats failed" }
```

Full validation/preflight:

```powershell
& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) { throw "mainnet validate failed" }

$Preflight = & $QUBD --config $TmpCfg preflight | ConvertFrom-Json
if ($Preflight.ok -ne $true) { throw "preflight ok=false" }

"HF124 LOCAL MAINNET GATE: PASS"
```

## A9. GUI smoke

```powershell
cargo run --release --bin qub-core -- --config $TmpCfg
```

Verify:

```text
1. QUB Core displays v1.8.1.
2. Existing QUB/Ethereum wallets and pending outbox remain.
3. Mainnet tip is v2 and sync works.
4. Melt/Infuse remain responsive.
5. Start mining while mempool activity changes.
6. The mining round does not restart merely because the mempool count changes.
7. A pool miner reports one local share for the current parent, not one per round restart.
8. No RPC port opens in the standard mainnet config.
```

---

# B. Prepare and upload seed source

## B1. Variables

```powershell
$Key = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"

$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.1"
$Deploy = "C:\temp\qubd_hf124_v181_seed_deploy"
$Tar = "C:\temp\qubd-hf124-v181-source.tar.gz"
```

## B2. Clean source archive

```powershell
Remove-Item -Recurse -Force $Deploy -ErrorAction SilentlyContinue
Remove-Item -Force $Tar -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Deploy | Out-Null

robocopy $Src $Deploy /E `
    /XD .git target dist data .gradle .idea node_modules `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json CURRENT.json PREVIOUS.json WRITE.lock `
        .env .env.*

if ($LASTEXITCODE -gt 7) {
    throw "robocopy failed with $LASTEXITCODE"
}

tar -czf $Tar -C $Deploy .
Get-FileHash $Tar -Algorithm SHA256
```

## B3. Upload

```powershell
scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf124-v181-source.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "AMS3 source upload failed" }

scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf124-v181-source.tar.gz"
if ($LASTEXITCODE -ne 0) { throw "NYC3 source upload failed" }
```

---

# C. Build/test on both seeds

```powershell
$SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf124-v181-source.tar.gz
CFG=/opt/qub/mainnet/mainnet-seed.toml

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 deploy/digitalocean/*.sh 2>/dev/null || true

echo "=== HF124 markers ==="
grep -n 'version = "1.8.1"' Cargo.toml
grep -n 'QUB Core:1.8.1' src/p2p.rs
grep -n 'hf124_pool_share_mempool_limit' src/lib.rs
grep -n 'hf124_fair_mempool_relay_batch' src/p2p.rs
grep -n 'relay_mempool_batch_to_known_peers' src/p2p.rs
grep -n 'one local share per canonical parent' src/bin/qub_core.rs
grep -n 'hf124_candidate_parts_are_reused_without_rebuilding_transaction_selection' tests/v1_core.rs
grep -n 'hf124_candidate_caps_pool_shares' tests/v1_core.rs
grep -n 'hf124_candidate_drains_oldest_confirmable_shares_first' tests/v1_core.rs
grep -nE 'hf124_same_tip_persistence_merges_concurrent_mempool_entries|hf124_rebuild_skips_stale_legacy_prefix_and_retains_newer_valid_shares|hf124_rebuild_caps_legacy_share_queue_before_general_mempool_limit|hf124_pool_shares_are_not_persisted_or_resurrected_by_wallet_outbox' tests/v1_core.rs
grep -n 'hf124_relay_batch_reserves_space_for_ordinary_transactions' src/p2p.rs

echo "=== Rust versions ==="
rustc --version
cargo --version

echo "=== Tests ==="
cargo test --lib
cargo test --test v1_core hf124_ -- --nocapture

echo "=== Release binaries ==="
cargo build --release --bin qubd
cargo build --release --bin qub-rpc-miner

ls -lah target/release/qubd target/release/qub-rpc-miner
file target/release/qubd target/release/qub-rpc-miner

if ! LC_ALL=C grep -aFq '/QUB Core:1.8.1/' target/release/qubd; then
  echo "STOP: v1.8.1 binary marker missing"
  exit 1
fi

echo "=== Real post-#24000 status ==="
timeout --signal=TERM --kill-after=20s 300s \
  target/release/qubd --config "$CFG" status-fast \
  > /tmp/hf124-seed-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf124-seed-status.json'))
print('height',sf.get('height'),'tip',sf.get('tip_hash'),'version',sf.get('tip_block_version'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
PY

echo "HF124 SEED BUILD GATE: PASS"
'@
```

AMS3:

```powershell
$SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "AMS3 HF124 build gate failed" }
```

NYC3:

```powershell
$SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "NYC3 HF124 build gate failed" }
```

---

# D. NYC3 canary

Install mainnet first. Start testnet only after the mainnet canary passes.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
UNIT=qub-seed-mainnet.service
TEST_UNIT=qub-seed-testnet.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.1-hf124-$STAMP"

restore_on_error() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    echo "=== NYC3 canary failed rc=$rc; restoring previous binary ==="
    sudo systemctl stop "$UNIT" 2>/dev/null || true
    [ -f "$BACKUP" ] && sudo install -m 0755 -o root -g root "$BACKUP" "$BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT" || true
    sudo systemctl start "$TEST_UNIT" 2>/dev/null || true
  fi
  exit "$rc"
}
trap restore_on_error EXIT

test -x "$STAGED"
test -f "$CFG"
STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"

if ! LC_ALL=C grep -aFq '/QUB Core:1.8.1/' "$STAGED"; then
  echo "STOP: staged binary is not v1.8.1"
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"

sudo systemctl stop "$TEST_UNIT" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=15s 90s systemctl stop "$UNIT" || true
if systemctl is-active --quiet "$UNIT"; then
  sudo systemctl kill --kill-who=all "$UNIT" || true
  sleep 3
fi
if systemctl is-active --quiet "$UNIT"; then
  echo "STOP: mainnet unit did not stop"
  exit 1
fi

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$UNIT"

for i in $(seq 1 120); do
  sleep 5
  ACTIVE="$(systemctl is-active "$UNIT" || true)"
  PID="$(systemctl show -p MainPID --value "$UNIT")"
  echo "wait=$i active=$ACTIVE pid=$PID"
  if [ "$ACTIVE" = active ] && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$UNIT" --no-pager -l || true
    sudo journalctl -u "$UNIT" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT")"
RUN_HASH="$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')"
[ "$RUN_HASH" = "$STAGED_HASH" ]

if sudo ss -ltnp | grep -q ':17445'; then
  echo "STOP: public-seed RPC is listening"
  exit 1
fi

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast \
  > /tmp/hf124-nyc3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf124-nyc3-status.json'))
assert sf.get('ok') is True
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
print('NYC3',sf.get('height'),sf.get('tip_hash'))
PY

sudo systemctl start "$TEST_UNIT" 2>/dev/null || true

trap - EXIT
echo "NYC3 HF124 CANARY: PASS"
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "NYC3 HF124 canary failed" }
```

Public canary reachability:

```powershell
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

---

# E. AMS3 deployment

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
UNIT=qub-seed-mainnet.service
TEST_UNIT=qub-seed-testnet.service
SNAP_TIMER=qub-mainnet-snapshot-publish.timer
SNAP_SERVICE=qub-mainnet-snapshot-publish.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.1-hf124-$STAMP"

restore_on_error() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    echo "=== AMS3 deployment failed rc=$rc; restoring previous binary ==="
    sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
    sudo systemctl stop "$UNIT" 2>/dev/null || true
    [ -f "$BACKUP" ] && sudo install -m 0755 -o root -g root "$BACKUP" "$BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT" || true
    sudo systemctl start "$TEST_UNIT" 2>/dev/null || true
    sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true
  fi
  exit "$rc"
}
trap restore_on_error EXIT

test -x "$STAGED"
STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"
if ! LC_ALL=C grep -aFq '/QUB Core:1.8.1/' "$STAGED"; then
  echo "STOP: staged binary is not v1.8.1"
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"

sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl stop "$TEST_UNIT" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=15s 90s systemctl stop "$UNIT" || true
if systemctl is-active --quiet "$UNIT"; then
  sudo systemctl kill --kill-who=all "$UNIT" || true
  sleep 3
fi
if systemctl is-active --quiet "$UNIT"; then
  echo "STOP: mainnet unit did not stop"
  exit 1
fi

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$UNIT"

for i in $(seq 1 120); do
  sleep 5
  ACTIVE="$(systemctl is-active "$UNIT" || true)"
  PID="$(systemctl show -p MainPID --value "$UNIT")"
  echo "wait=$i active=$ACTIVE pid=$PID"
  if [ "$ACTIVE" = active ] && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$UNIT" --no-pager -l || true
    sudo journalctl -u "$UNIT" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

if sudo ss -ltnp | grep -q ':17445'; then
  echo "STOP: public-seed RPC is listening"
  exit 1
fi

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast \
  > /tmp/hf124-ams3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf124-ams3-status.json'))
assert sf.get('ok') is True
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
print('AMS3',sf.get('height'),sf.get('tip_hash'),'mempool',sf.get('mempool_tx_count'))
PY

sudo systemctl start "$TEST_UNIT" 2>/dev/null || true
sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true

systemctl is-active "$UNIT"
systemctl is-active "$SNAP_TIMER" || true
sudo ss -ltnp | grep -E ':(17444|18765)\b' || true

trap - EXIT
echo "AMS3 HF124 DEPLOYMENT: PASS"
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "AMS3 HF124 deployment failed" }
```

Public seed checks:

```powershell
Test-NetConnection seed.qubit-coin.io -Port 17444
Test-NetConnection seed-ams3.qubit-coin.io -Port 17444
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

---

# F. Update the separate Explorer API binary on AMS3

The Explorer API remains a read-only service and must not replace the canonical seed process.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
BUILT="$SRC/target/release/qubd"
API_BIN=/opt/qub/bin/qubd-explorer-api-hf124
API_UNIT=qub-explorer-api-mainnet.service
SEED_UNIT=qub-seed-mainnet.service
CFG=/opt/qub/mainnet/mainnet-seed.toml
DROPIN_DIR="/etc/systemd/system/$API_UNIT.d"
DROPIN="$DROPIN_DIR/hf124-explorer-api.conf"
STAMP="$(date +%Y%m%d-%H%M%S)"
DROPIN_BACKUP_DIR="/etc/systemd/system/${API_UNIT}.d.backup-before-hf124-$STAMP"
HAD_DROPIN_DIR=0

restore_on_error() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    echo "=== HF124 Explorer API update failed rc=$rc; restoring previous drop-ins ==="
    sudo systemctl stop "$API_UNIT" 2>/dev/null || true
    sudo rm -rf "$DROPIN_DIR"
    if [ "$HAD_DROPIN_DIR" -eq 1 ] && [ -d "$DROPIN_BACKUP_DIR" ]; then
      sudo cp -a "$DROPIN_BACKUP_DIR" "$DROPIN_DIR"
    fi
    sudo systemctl daemon-reload || true
    sudo systemctl reset-failed "$API_UNIT" 2>/dev/null || true
    sudo systemctl restart "$API_UNIT" || true
    echo "=== Canonical seed remains untouched ==="
    systemctl is-active "$SEED_UNIT" || true
    sudo ss -ltnp | grep ':17444' || true
  fi
  exit "$rc"
}
trap restore_on_error EXIT

test -x "$BUILT"
test -f "$CFG"

if ! LC_ALL=C grep -aFq '/QUB Core:1.8.1/' "$BUILT"; then
  echo "STOP: built Explorer API binary is not v1.8.1"
  exit 1
fi

SEED_PID_BEFORE="$(systemctl show -p MainPID --value "$SEED_UNIT")"
SEED_HASH_BEFORE="$(sudo sha256sum "/proc/$SEED_PID_BEFORE/exe" | awk '{print $1}')"

sudo install -m 0755 -o root -g root "$BUILT" "$API_BIN"
if ! LC_ALL=C grep -aFq '/QUB Core:1.8.1/' "$API_BIN"; then
  echo "STOP: installed API binary version marker missing"
  exit 1
fi

if [ -d "$DROPIN_DIR" ]; then
  HAD_DROPIN_DIR=1
  sudo cp -a "$DROPIN_DIR" "$DROPIN_BACKUP_DIR"
fi
sudo mkdir -p "$DROPIN_DIR"

# Remove the old HF123 API-only override and install one exact HF124 override.
sudo rm -f "$DROPIN_DIR/hf123-explorer-api-r3.conf"
sudo tee "$DROPIN" >/dev/null <<EOF2
[Service]
ExecStart=
ExecStart=$API_BIN --config $CFG explorer-api 127.0.0.1:18765
EOF2

sudo systemctl daemon-reload
sudo systemctl restart "$API_UNIT"

for i in $(seq 1 90); do
  sleep 2
  if systemctl is-active --quiet "$API_UNIT" && sudo ss -ltnp | grep -q ':18765'; then break; fi
  if [ "$i" = 90 ]; then
    sudo systemctl status "$API_UNIT" --no-pager -l || true
    sudo journalctl -u "$API_UNIT" -n 200 --no-pager || true
    exit 1
  fi
done

API_PID="$(systemctl show -p MainPID --value "$API_UNIT")"
[ "$(sudo sha256sum "/proc/$API_PID/exe" | awk '{print $1}')" = "$(sha256sum "$API_BIN" | awk '{print $1}')" ]

RESULT="$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/hf124-api-mempool.json -w '%{http_code}|%{time_total}' http://127.0.0.1:18765/api/v1/mempool)"
echo "API mempool: $RESULT"
[ "${RESULT%%|*}" = 200 ]

python3 - "${RESULT##*|}" <<'PY'
import json
import sys
elapsed=float(sys.argv[1])
m=json.load(open('/tmp/hf124-api-mempool.json'))
assert m.get('network') == 'mainnet'
assert isinstance(m.get('transactions'), list)
assert int(m.get('count',-1)) == len(m['transactions'])
assert elapsed < 15, f'mempool endpoint too slow: {elapsed}s'
print('mempool',m['count'],'elapsed',elapsed)
PY

SEED_PID_AFTER="$(systemctl show -p MainPID --value "$SEED_UNIT")"
SEED_HASH_AFTER="$(sudo sha256sum "/proc/$SEED_PID_AFTER/exe" | awk '{print $1}')"
[ "$SEED_PID_AFTER" = "$SEED_PID_BEFORE" ]
[ "$SEED_HASH_AFTER" = "$SEED_HASH_BEFORE" ]

systemctl is-active "$SEED_UNIT"
sudo ss -ltnp | grep ':17444'
systemctl is-active qub-explorer-api-caddy.socket
systemctl is-active qub-explorer-api-caddy.service

trap - EXIT
echo "HF124 EXPLORER API UPDATE: PASS"
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "HF124 Explorer API update failed" }
```
Public same-origin API gate:

```powershell
$Mempool = curl.exe -fsSL "https://explorer.qubit-coin.io/api/v1/mempool?hf124=$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())" | ConvertFrom-Json
$Mempool | Format-List network,count
if ($Mempool.network -ne 'mainnet') { throw 'wrong API network' }
if ([int]$Mempool.count -ne $Mempool.transactions.Count) { throw 'mempool count mismatch' }
```

---

# G. Publish a fresh snapshot

```powershell
@'
set -euo pipefail

sudo timeout --signal=TERM --kill-after=30s 900s \
  /opt/qub/bin/publish-mainnet-snapshot.sh

cat /srv/qub-updates/mainnet/snapshots/tip.json

python3 - <<'PY'
import json
tip=json.load(open('/srv/qub-updates/mainnet/snapshots/tip.json'))
tail=json.load(open('/srv/qub-updates/mainnet/snapshots/tail-64.json'))
assert tip.get('network') == 'mainnet'
assert int(tip.get('height',0)) >= 24000
assert int(tail.get('tip_height',0)) == int(tip['height'])
assert tail.get('tip_hash') == tip.get('tip_hash')
assert int(tail['blocks'][-1]['header']['version']) == 2
print('snapshot',tip['height'],tip['tip_hash'])
PY

echo "HF124 SNAPSHOT GATE: PASS"
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Purge the snapshot URLs only if the public snapshot does not immediately show the new tip.

---

# H. Build the Windows distribution

```powershell
Set-Location "C:\Users\proes\Desktop\qub-node\qubd-v1.8.1"

Remove-Item -Recurse -Force .\dist -ErrorAction SilentlyContinue

$ISCC = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found" }
$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass `
    -File .\scripts\build-windows-release.ps1 `
    -Config mainnet `
    -BuildInstaller `
    -SkipTests `
    -SkipPreflight
```

Expected files:

```powershell
$Expected = @(
    '.\dist\installer\QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\manifest.json',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-Latest.exe',
    '.\dist\updates\QUB-Core-Latest.exe'
)
foreach ($Path in $Expected) {
    if (-not (Test-Path $Path)) { throw "Missing output: $Path" }
}
```

EXE headers:

```powershell
foreach ($Path in @(
    '.\dist\installer\QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-Latest.exe',
    '.\dist\updates\QUB-Core-Latest.exe'
)) {
    $Bytes=[System.IO.File]::ReadAllBytes((Resolve-Path $Path).Path)
    $Header='{0:X2} {1:X2}' -f $Bytes[0],$Bytes[1]
    "$Path $Header"
    if ($Header -ne '4D 5A') { throw "$Path is not an EXE" }
}
```

---

# I. Patch the public manifest

```powershell
$Manifest=(Resolve-Path '.\dist\updates\mainnet\windows-x64\manifest.json').Path
$m=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json

function Set-JsonProp($Object,[string]$Name,$Value) {
    if ($Object.PSObject.Properties.Name -contains $Name) { $Object.$Name=$Value }
    else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

Set-JsonProp $m 'mandatory' $true
Set-JsonProp $m 'chain_upgrade' $false
Set-JsonProp $m 'hotfix' 'HF124'
Set-JsonProp $m 'activation_feature' 'none'
Set-JsonProp $m 'activation_height' 0
Set-JsonProp $m 'protocol_epoch' 2
Set-JsonProp $m 'protocol_epoch_2_activation_height' 24000
Set-JsonProp $m 'post_activation_block_version' 2
Set-JsonProp $m 'checkpoint_height' 10367
Set-JsonProp $m 'checkpoint_hash' '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000'
Set-JsonProp $m 'consensus_family' 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000'
Set-JsonProp $m 'hotfix_family' 'hf124-mining-liveness-pool-share-template-cap-stable-pow-coalesced-mempool'
Set-JsonProp $m 'notes' 'Mandatory QUB Core v1.8.1 HF124 mining-liveness update. HF124 introduces no new chain activation and keeps Protocol Epoch 2 active at #24000 with block version 2. It makes official templates obey the existing 128 pool-share-transactions-per-block consensus limit, preserves ordinary transactions during share bursts, prevents mempool-only changes from cancelling active CPU/GPU proof-of-work, limits the GUI pool miner to one local share per canonical parent, bounds pool-share mempool retention to the confirmable stale horizon, caches and incrementally advances pool registry state, batches share admission, takes at most one copy-on-write mempool snapshot per five-second window, commits it outside the canonical mutex, preserves concurrent same-tip mempool updates, and reserves relay capacity for ordinary QUB/JIN/Library traffic. DAA, checkpoints, genesis, economics, QUB/JIN rules and the Fast Chain Engine storage format are unchanged.'

$Json=$m | ConvertTo-Json -Depth 40
[System.IO.File]::WriteAllText($Manifest,$Json+"`r`n",[System.Text.UTF8Encoding]::new($false))

$Check=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json
$Check | Format-List version,mandatory,chain_upgrade,hotfix,activation_feature,activation_height,protocol_epoch,protocol_epoch_2_activation_height,post_activation_block_version,hotfix_family

if ($Check.version -ne '1.8.1') { throw 'bad version' }
if ($Check.mandatory -ne $true) { throw 'mandatory must be true' }
if ($Check.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
if ($Check.hotfix -ne 'HF124') { throw 'bad hotfix' }
if ($Check.activation_feature -ne 'none') { throw 'activation_feature must be none' }
if ([int]$Check.activation_height -ne 0) { throw 'activation_height must be 0' }
if ([int]$Check.post_activation_block_version -ne 2) { throw 'bad block version' }
```

Regenerate sidecars:

```powershell
$UpdateDir='.\dist\updates\mainnet\windows-x64'
Push-Location $UpdateDir

(Get-FileHash manifest.json -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII manifest.json.sha256
(Get-FileHash QUB-Core-Latest.exe -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII QUB-Core-Latest.exe.sha256
(Get-FileHash QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe.sha256

Get-ChildItem -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    $Hash=(Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
    "$Hash  $($_.Name)"
} | Set-Content -Encoding ASCII SHA256SUMS.txt

Get-Content SHA256SUMS.txt
Pop-Location

(Get-FileHash '.\dist\updates\QUB-Core-Latest.exe' -Algorithm SHA256).Hash.ToLowerInvariant() |
    Set-Content -Encoding ASCII '.\dist\updates\QUB-Core-Latest.exe.sha256'
```

---

# J. Upload and install public Windows files

```powershell
$UpdateDir=(Resolve-Path '.\dist\updates\mainnet\windows-x64').Path
$RootLatest=(Resolve-Path '.\dist\updates\QUB-Core-Latest.exe').Path
$RootLatestSha=(Resolve-Path '.\dist\updates\QUB-Core-Latest.exe.sha256').Path

ssh.exe -i $Key $AMS3 'rm -rf /tmp/qub-mainnet-windows-x64 && mkdir -p /tmp/qub-mainnet-windows-x64'

$Files=Get-ChildItem $UpdateDir -File | Sort-Object Name | Select-Object -ExpandProperty FullName
$ScpArguments=@('-i',$Key)+$Files+@("${AMS3}:/tmp/qub-mainnet-windows-x64/")
scp.exe @ScpArguments
if ($LASTEXITCODE -ne 0) { throw 'windows-x64 upload failed' }

scp.exe -i $Key $RootLatest "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe"
scp.exe -i $Key $RootLatestSha "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe.sha256"
```

Origin install:

```powershell
@'
set -euo pipefail
STAMP="$(date +%Y%m%d-%H%M%S)"
DIR=/srv/qub-updates/mainnet/windows-x64
sudo cp -a "$DIR" "$DIR.backup-before-v1.8.1-hf124-$STAMP"
sudo rsync -av --delete /tmp/qub-mainnet-windows-x64/ "$DIR/"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe /srv/qub-updates/mainnet/QUB-Core-Latest.exe
sudo mv /tmp/QUB-Core-Latest-mainnet.exe.sha256 /srv/qub-updates/mainnet/QUB-Core-Latest.exe.sha256
sudo find /srv/qub-updates/mainnet -type f -exec chmod 0644 {} \;
cd "$DIR"
tr -d '\r' < SHA256SUMS.txt | sha256sum -c -
head -c 2 QUB-Core-Latest.exe | xxd -p | grep -qx 4d5a
python3 - <<'PY'
import json
m=json.load(open('/srv/qub-updates/mainnet/windows-x64/manifest.json'))
assert m['version']=='1.8.1'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
assert m['hotfix']=='HF124'
assert m['activation_feature']=='none'
assert int(m['activation_height'])==0
assert int(m['protocol_epoch'])==2
assert int(m['post_activation_block_version'])==2
print('manifest PASS')
PY
echo 'HF124 ORIGIN FILE INSTALL: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# K. Cloudflare purge and public verification

Purge:

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe.sha256
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256
```

Public gate:

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference='Stop'
    $Nonce=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $m=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json?hf124=$Nonce" | ConvertFrom-Json
    $m | Format-List version,mandatory,chain_upgrade,hotfix,activation_feature,activation_height,protocol_epoch,post_activation_block_version
    if ($m.version -ne '1.8.1') { throw 'bad public version' }
    if ($m.mandatory -ne $true) { throw 'mandatory false' }
    if ($m.chain_upgrade -ne $false) { throw 'chain_upgrade true' }
    if ($m.hotfix -ne 'HF124') { throw 'bad hotfix' }

    $Exe=Join-Path $env:TEMP 'QUB-Core-v1.8.1-HF124-Latest.exe'
    curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe?hf124=$Nonce" -o $Exe
    $Bytes=[System.IO.File]::ReadAllBytes($Exe)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw 'public latest is not EXE' }
    $Actual=(Get-FileHash $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $m.sha256.ToLowerInvariant()) { throw 'public latest SHA mismatch' }
    Write-Host 'HF124 PUBLIC ARTIFACT GATE: PASS'
}
```

---

# L. Clean installation and live liveness smoke

Install the public installer on the release workstation. Verify:

```text
1. App opens as v1.8.1.
2. Wallets and pending outbox are preserved.
3. Mainnet sync remains on block version 2.
4. Fast Chain Engine storage remains QUB-FCE-1.
5. Mining can begin with a share mempool larger than 128.
6. Mempool count changes do not repeatedly restart the mining animation/workers.
7. Pool mining submits no more than one local share for the same parent.
8. A found official block never contains more than 128 pool-share transactions.
9. Ordinary transactions remain eligible during a share backlog.
```

---

# M. GitHub repository and release

Use a clean source-only mirror. Do not commit runtime assets, wallets, Fast Chain Engine state, installers or build output.

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Src = Join-Path $ProjectRoot "qubd-v1.8.1"
$Repo = Join-Path $ProjectRoot "qub-core-opensource"
$Sync = Join-Path $ProjectRoot "_repo_sync_hf124"

if (-not (Test-Path $Repo)) {
    git clone https://github.com/AlxProe/qub-core.git $Repo
}

Set-Location $Repo

if ((git status --porcelain).Length -gt 0) {
    throw "Repository has uncommitted changes."
}

git pull origin main

Remove-Item -Recurse -Force $Sync -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Sync | Out-Null

robocopy $Src $Sync /E `
    /XD .git target dist data .gradle .idea node_modules `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json CURRENT.json PREVIOUS.json WRITE.lock `
        .env .env.*

if ($LASTEXITCODE -gt 7) {
    throw "Source mirror failed with $LASTEXITCODE"
}

$Assets = Join-Path $Sync "assets"
Remove-Item -Recurse -Force $Assets -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Assets | Out-Null

@'
# Assets

Runtime image, audio and font assets are intentionally excluded from the public source package.

Never commit private keys, wallet files, chain state, installers, build artifacts or user-specific runtime files.
'@ | Set-Content -Encoding UTF8 (Join-Path $Assets "README.md")

foreach ($File in @(
    "LICENSE",
    "NOTICE",
    "SECURITY.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    ".gitignore"
)) {
    $Existing = Join-Path $Repo $File
    if (Test-Path $Existing) {
        Copy-Item $Existing (Join-Path $Sync $File) -Force
    }
}

if ((Test-Path (Join-Path $Repo ".github")) -and
    -not (Test-Path (Join-Path $Sync ".github"))) {
    Copy-Item (Join-Path $Repo ".github") (Join-Path $Sync ".github") -Recurse -Force
}

$Forbidden = Get-ChildItem $Sync -Recurse -Force -File | Where-Object {
    $_.Extension -in @(".exe", ".dll", ".pdb", ".ilk", ".zip", ".tar", ".gz", ".7z") -or
    $_.Name -in @(
        "wallet.json",
        "ethereum-wallets.json",
        "wallet-pending-txs.json",
        "chain.json",
        "chain-status.json",
        "CURRENT.json",
        "PREVIOUS.json",
        "WRITE.lock",
        ".env"
    ) -or
    $_.Name -like ".env.*"
}

if ($Forbidden) {
    $Forbidden | Select-Object FullName
    throw "Forbidden runtime/build files found in repository mirror."
}

robocopy $Sync $Repo /MIR /XD .git
if ($LASTEXITCODE -gt 7) {
    throw "Repository mirror failed with $LASTEXITCODE"
}

Set-Location $Repo

git diff --check
git status --short

git add .

git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh

git diff --cached --check
git diff --cached --stat

git commit -m "Update QUB Core to v1.8.1 HF124"

$ExistingTag = git tag -l "v1.8.1-hf124"
if ($ExistingTag) {
    throw "Tag v1.8.1-hf124 already exists."
}

git tag -a v1.8.1-hf124 -m "QUB Core v1.8.1 HF124"
git push origin main
git push origin v1.8.1-hf124
```

If GitHub CLI is installed:

```powershell
gh release create v1.8.1-hf124 `
    --title "QUB Core v1.8.1 HF124 - Mining Liveness" `
    --notes-file ".\RELEASE_NOTES-v1.8.1-HF124.md"
```

Manual fallback:

```powershell
Get-Content .\RELEASE_NOTES-v1.8.1-HF124.md -Raw -Encoding UTF8 | Set-Clipboard
Start-Process "https://github.com/AlxProe/qub-core/releases/new?tag=v1.8.1-hf124"
```

Use:

```text
Tag: v1.8.1-hf124
Target: main
Title: QUB Core v1.8.1 HF124 - Mining Liveness
Latest release: enabled
Pre-release: disabled
Description: paste the release notes
```

The tag must point to the exact source used for the public installer. Do not move an existing tag.

---

# N. Telegram announcement

```text
🆕 QUB Core v1.8.1 HF124 is live

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.1-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.8.1-hf124

⚠️ This is a mandatory mining-liveness update.

HF124 introduces no new chain activation.
Protocol Epoch 2 remains active at block #24000, and official mainnet blocks continue to use block version 2.

A sustained pool-share burst exposed several interacting liveness problems in older QUB Core versions:

- A local block template could include more than the existing consensus limit of 128 pool-share transactions.
- CPU/GPU mining rounds restarted whenever the mempool changed, even when the canonical block parent was unchanged.
- Pool mining could create additional shares after those restarts.
- Share bursts caused repeated pool-state validation, persistence and relay pressure.

HF124 fixes the complete path:

- Official templates now include at most 128 pool shares and drain the oldest still-confirmable shares first.
- Extra shares remain pending for later blocks and do not crowd ordinary transactions out of the candidate.
- Mempool-only updates no longer cancel active proof-of-work.
- The validated candidate transaction set is assembled once and shared by CPU/GPU workers.
- Legacy GUI target-spacing and last-winner cooldown sleeps are removed; consensus/DAA and canonical-parent guards remain.
- The GUI pool miner submits at most one local share per canonical parent.
- Pool-share retention is bounded to the horizon in which shares can still be confirmed.
- Pool registry and share-window validation are cached and bounded.
- Inbound share batches reuse one validation context.
- Mempool-only Fast Chain Engine persistence snapshots state under the canonical mutex at most once per five-second window, writes after releasing the mutex, and merges same-tip GUI/P2P submissions without dropping concurrent transactions.
- Relay batches reserve capacity for ordinary QUB, JIN and Library transactions.
- Locally created shares use a bounded official-first fanout; inbound one-by-one shares are deferred to the fair heartbeat; periodic and inbound batches use one bounded `Mempool` message per peer.
- Pool shares are excluded from the durable wallet pending outbox, and legacy records are removed during reconciliation.

The 128-share consensus limit itself is unchanged.
DAA, checkpoints, genesis, economics, QUB/JIN rules and the Fast Chain Engine storage format are unchanged.

Everyone mining or running a QUB full node should update to v1.8.1.

Do not delete wallet.json.
Do not delete ethereum-wallets.json.
Do not delete wallet-pending-txs.json while transactions are pending.
Do not delete the QUB Core data directory or chain-v2 folder.
Never send private keys, wallet files or seed phrases to anyone.
```

---

# O. Post-deploy liveness monitoring

## O1. Share pressure

```powershell
$Nonce=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$Status=curl.exe -fsSL "https://explorer.qubit-coin.io/api/v1/status-fast?hf124=$Nonce" | ConvertFrom-Json
$Mempool=curl.exe -fsSL "https://explorer.qubit-coin.io/api/v1/mempool?hf124=$Nonce" | ConvertFrom-Json
$Shares=@($Mempool.transactions | Where-Object { $null -ne $_.pool_share })
"height=$($Status.height) mempool=$($Mempool.count) shares=$($Shares.Count)"
```

## O2. First post-HF124 block

After the height advances:

```powershell
$Height=(curl.exe -fsSL "https://explorer.qubit-coin.io/api/v1/status-fast?posthf124=$([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())" | ConvertFrom-Json).height
$Block=curl.exe -fsSL "https://explorer.qubit-coin.io/api/v1/block/$Height" | ConvertFrom-Json
$ShareCount=@($Block.transactions | Where-Object { $null -ne $_.pool_share }).Count
"height=$Height txs=$($Block.transactions.Count) pool_shares=$ShareCount"
if ($ShareCount -gt 128) { throw 'post-HF124 block exceeds pool share consensus cap' }
```

## O3. Service health

```powershell
@'
set -euo pipefail
systemctl is-active qub-seed-mainnet.service
systemctl is-active qub-explorer-api-mainnet.service
systemctl is-active qub-mainnet-snapshot-publish.timer
sudo ss -ltnp | grep -E ':(17444|18765)\b'
/opt/qub/bin/qubd --config /opt/qub/mainnet/mainnet-seed.toml status-fast
free -h
cat /proc/swaps || true
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

A successful rollout should show advancing canonical height, valid v2 blocks, bounded share inclusion and falling share backlog over subsequent blocks.
