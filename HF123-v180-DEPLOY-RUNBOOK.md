# HF123 / QUB Core v1.8.0 End-to-End Deployment Runbook

## Release identity

```text
QUB Core: v1.8.0
Hotfix: HF123
Package generation: v180
Consensus activation introduced by HF123: none
Active protocol epoch: 2
Protocol Epoch 2 activation height: #24000
Post-activation block version: 2
```

HF123 migrates local persistence to `QUB-FCE-1`. Back up every production data directory before the first HF123 startup. Do not run two state-changing QUB processes against the same data directory.

---

# A. Windows source extraction and local gates

## A1. Variables and ZIP verification

Place `HF123-v180-source-no-assets.zip` in:

```text
C:\Users\proes\Desktop\qub-node
```

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF123-v180-source-no-assets.zip"
$Sums = Join-Path $ProjectRoot "HF123-v180-SHA256SUMS.txt"
$Work = Join-Path $ProjectRoot "qubd-v1.8.0"
$Extract = Join-Path $ProjectRoot "_hf123_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.8.0-before-hf123"

if (-not (Test-Path $Sums)) {
    throw "Companion SHA256SUMS file not found: $Sums"
}

$ZipSumLine = Get-Content $Sums -Encoding ASCII |
    Where-Object { $_ -match '\sHF123-v180-source-no-assets\.zip$' } |
    Select-Object -First 1

if (-not $ZipSumLine) {
    throw "Source ZIP entry not found in $Sums"
}

$ExpectedZipHash = ($ZipSumLine -split '\s+')[0].ToUpperInvariant()
$ActualZipHash = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

"HF123 ZIP SHA256: $ActualZipHash"

if ($ActualZipHash -ne $ExpectedZipHash) {
    throw "HF123 ZIP SHA mismatch. Expected $ExpectedZipHash got $ActualZipHash"
}
```

## A2. Clean extraction

```powershell
Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) {
    Rename-Item $Work $Backup
}

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.8.0") $Work

cd $Work
```

## A3. Copy runtime assets

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.7.9\assets",
    "C:\Users\proes\Desktop\qub-node\qubd-v1.7.8\assets",
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

## A4. Marker gate

```powershell
Get-Content .\Cargo.toml -Encoding UTF8 |
    Select-String 'version = "1.8.0"|features = \["derive", "rc"\]'

Select-String -Path .\src\fast_storage.rs `
    -Pattern 'HF123_FAST_STORAGE_MAGIC|CURRENT.json|PREVIOUS.json|append_journal|commit_chain|export_chain_json'

Select-String -Path .\src\lib.rs `
    -Pattern 'Arc<Vec<Block>>|Arc<HashMap<OutPoint, CoinRecord>>|MAINNET_PROTOCOL_EPOCH_2_ACTIVATION_HEIGHT|PROTOCOL_EPOCH_2_BLOCK_VERSION'

Select-String -Path .\src\p2p.rs `
    -Pattern 'QUB Core:1.8.0|Fast Chain Engine|register_live_chain'

Select-String -Path .\src\bin\qub_core.rs `
    -Pattern 'APP_VERSION: &str = "v1.8.0"|hf123_gui_derived_cache|status_history'

Select-String -Path .\scripts\test-hf123-fast-chain-regtest.py `
    -Pattern 'HF123 FAST CHAIN ENGINE REGTEST E2E: PASS'
```

Forbidden analytics gate:

```powershell
$ForbiddenAnalytics = Select-String `
    -Path .\src\*.rs,.\src\bin\*.rs,.\tests\*.rs,.\explorer\public\index.html `
    -Pattern 'exact two-label|two_label_alternation|same-label streak|address-order' `
    -ErrorAction SilentlyContinue

if ($ForbiddenAnalytics) {
    $ForbiddenAnalytics | Format-Table Path, LineNumber, Line -AutoSize
    throw "Address-order analytics remain in an active product surface."
}
```

## A5. Real Cargo gates

```powershell
cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-core
cargo build --release --bin qub-rpc-miner
```

STOP on any real compile or test error.

## A6. Fast Chain Engine regtest E2E

```powershell
py .\scripts\test-hf123-fast-chain-regtest.py `
    --qubd .\target\release\qubd.exe
```

Expected ending:

```text
HF123 FAST CHAIN ENGINE REGTEST E2E: PASS
```

## A7. RPC/miner regtest E2E

```powershell
py .\scripts\test-hf123-rpc-regtest.py `
    --qubd .\target\release\qubd.exe `
    --miner .\target\release\qub-rpc-miner.exe
```

Expected ending:

```text
HF123 RPC REGTEST E2E: PASS
```

---

# B. Local mainnet backup, migration and smoke

Close QUB Core before the migration gate.

## B1. Back up the real mainnet data directory

```powershell
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.0"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$DataBackup = "C:\Users\proes\Desktop\qub-node\mainnet-before-hf123-$(Get-Date -Format yyyyMMdd-HHmmss)"

if (-not (Test-Path $RealDataDir)) {
    throw "Mainnet data directory not found: $RealDataDir"
}

robocopy $RealDataDir $DataBackup /E /COPY:DAT /DCOPY:DAT
if ($LASTEXITCODE -gt 7) {
    throw "Mainnet backup failed with robocopy code $LASTEXITCODE"
}

"Mainnet backup: $DataBackup"
```

## B2. Create real-data temporary config

```powershell
$QUBD = "$Root\target\release\qubd.exe"
$SourceCfg = "$Root\config\mainnet.toml"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf123-v180.toml"
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
    throw "[node] section not found in mainnet config."
}

[System.IO.File]::WriteAllText(
    $TmpCfg,
    $Text,
    [System.Text.UTF8Encoding]::new($false)
)
```

## B3. Full pre-migration validation

```powershell
& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) {
    throw "Pre-migration mainnet validation failed."
}
```

## B4. Trigger migration

The first state-changing/startup load validates the legacy chain and creates `chain-v2`. Do not interrupt it.

```powershell
& $QUBD --config $TmpCfg init
if ($LASTEXITCODE -ne 0) {
    throw "HF123 mainnet migration failed."
}
```

Verify files:

```powershell
$FastDir = Join-Path $RealDataDir "chain-v2"

foreach ($Path in @(
    (Join-Path $FastDir "CURRENT.json"),
    (Join-Path $RealDataDir "chain-status.json"),
    (Join-Path $RealDataDir "chain.json")
)) {
    if (-not (Test-Path $Path)) {
        throw "Missing migrated state artifact: $Path"
    }
}
```

## B5. Post-migration status and storage metrics

```powershell
$Status = & $QUBD --config $TmpCfg status-fast | ConvertFrom-Json
$Stats = & $QUBD --config $TmpCfg storage-stats | ConvertFrom-Json

$Status | Format-List network,height,tip_hash,tip_block_version,next_block_expected_version,storage_engine,status_source
$Stats | Format-List storage_engine,height,generation,state_revision,journal_bytes,state_bytes,legacy_chain_bytes,last_load_millis,last_commit_millis

if ($Status.ok -ne $true) { throw "status-fast ok=false" }
if ($Status.network -ne "mainnet") { throw "Wrong network" }
if ([int]$Status.height -lt 24000) { throw "Mainnet is below Protocol Epoch 2" }
if ([int]$Status.tip_block_version -ne 2) { throw "Mainnet tip is not version 2" }
if ([int]$Status.next_block_expected_version -ne 2) { throw "Next block version is not 2" }
if ($Status.storage_engine -ne "QUB-FCE-1") { throw "Fast Chain Engine is not active" }
if ($Stats.storage_engine -ne "QUB-FCE-1") { throw "storage-stats engine mismatch" }
```

## B6. Export and validate

```powershell
$Export = Join-Path $env:TEMP "qub-hf123-mainnet-export.json"
& $QUBD --config $TmpCfg export-chain-json $Export
if ($LASTEXITCODE -ne 0 -or -not (Test-Path $Export)) {
    throw "Explicit chain export failed."
}

& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) { throw "Post-migration validation failed." }

$PreflightText = (& $QUBD --config $TmpCfg preflight 2>&1 | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw "Post-migration preflight failed:`n$PreflightText" }
$Preflight = $PreflightText | ConvertFrom-Json
if ($Preflight.ok -ne $true) { throw "Preflight ok=false" }
```

## B7. GUI smoke

```powershell
cargo run --release --bin qub-core -- --config $TmpCfg
```

Check:

```text
v1.8.0 displayed
wallets preserved
canonical height/tip loads
status history is readable and stable
Sync Now does not create repeated concurrent workers
Melt/Infuse remains responsive
mining starts only on the canonical tip
chain-v2 remains active
RPC remains disabled in standard mainnet config
```

---

# C. Prepare seed source upload

```powershell
$Key = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"
$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.0"
$Deploy = "C:\temp\qubd_hf123_seed_deploy"
$Tar = "C:\temp\qubd-hf123-v180-source.tar.gz"

Remove-Item -Recurse -Force $Deploy -ErrorAction SilentlyContinue
Remove-Item -Force $Tar -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Deploy | Out-Null

robocopy $Src $Deploy /E `
    /XD .git target dist data .gradle .idea node_modules __pycache__ `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json CURRENT.json PREVIOUS.json WRITE.lock .env .env.*

if ($LASTEXITCODE -gt 7) {
    throw "Source staging failed with robocopy code $LASTEXITCODE"
}

tar -czf $Tar -C $Deploy .
Get-FileHash $Tar -Algorithm SHA256

scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf123-v180-source.tar.gz"
scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf123-v180-source.tar.gz"
```

---

# D. Build gates on AMS3 and NYC3

Use the same block for each host.

```powershell
$SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then
  . "$HOME/.cargo/env"
fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf123-v180-source.tar.gz

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 deploy/digitalocean/*.sh scripts/*.py 2>/dev/null || true

bash -n deploy/digitalocean/publish-mainnet-snapshot.sh
bash -n deploy/digitalocean/test-publish-mainnet-snapshot.sh
python3 -m py_compile scripts/test-hf123-fast-chain-regtest.py scripts/test-hf123-rpc-regtest.py

grep -n 'version = "1.8.0"' Cargo.toml
grep -n 'HF123_FAST_STORAGE_MAGIC' src/fast_storage.rs
grep -n 'QUB Core:1.8.0' src/p2p.rs

cargo_missing=0
command -v cargo >/dev/null 2>&1 || cargo_missing=1
if [ "$cargo_missing" -ne 0 ]; then
  echo 'STOP: cargo is not installed for deploy user.'
  exit 1
fi

cargo test
cargo build --release --bin qubd
cargo build --release --bin qub-rpc-miner

ls -lah target/release/qubd target/release/qub-rpc-miner
file target/release/qubd target/release/qub-rpc-miner
strings target/release/qubd | grep 'QUB Core:1.8.0' | head

echo 'HF123 SEED BUILD GATE: PASS'
'@

$SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
$SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

STOP if either build gate fails.

---

# E. NYC3 canary migration

NYC3 is the canary. Its public seed remains unavailable during validated migration. AMS3 continues serving mainnet.

## E1. Canary script

```powershell
$NYC3Canary = @'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
MAINCFG=/opt/qub/mainnet/mainnet-seed.toml
UNIT_MAIN=qub-seed-mainnet.service
UNIT_TEST=qub-seed-testnet.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP_BIN="/opt/qub/bin/backups/qubd.before-v1.8.0-hf123-$STAMP"
BACKUP_DATA="/opt/qub/backups/mainnet-before-v1.8.0-hf123-$STAMP"

restore_previous_binary() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    echo "HF123 NYC3 canary failed rc=$rc; restoring previous binary."
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$UNIT_MAIN" 2>/dev/null || true
    sudo systemctl kill --kill-who=all "$UNIT_MAIN" 2>/dev/null || true
    if [ -f "$BACKUP_BIN" ]; then
      sudo install -m 0755 -o root -g root "$BACKUP_BIN" "$BIN"
    fi
    if [ -d /opt/qub/mainnet/data/chain-v2 ]; then
      sudo mv /opt/qub/mainnet/data/chain-v2 \
        "/opt/qub/mainnet/data/chain-v2.failed-hf123-$STAMP" || true
    fi
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT_MAIN" || true
  fi
  exit "$rc"
}
trap restore_previous_binary EXIT

if grep -A20 '^\[rpc\]' "$MAINCFG" | grep -qE '^enabled[[:space:]]*=[[:space:]]*true'; then
  echo 'STOP: RPC is enabled in the public seed config.'
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups "$BACKUP_DATA"
sudo cp "$BIN" "$BACKUP_BIN"

for name in chain.json chain-status.json wallet.json wallet-pending-txs.json peers.json; do
  if [ -e "/opt/qub/mainnet/data/$name" ]; then
    sudo cp -a "/opt/qub/mainnet/data/$name" "$BACKUP_DATA/$name"
  fi
done
if [ -d /opt/qub/mainnet/data/chain-v2 ]; then
  sudo cp -a /opt/qub/mainnet/data/chain-v2 "$BACKUP_DATA/chain-v2"
fi

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"

# Stop testnet separately so it cannot add memory pressure during migration.
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$UNIT_TEST" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$UNIT_TEST" 2>/dev/null || true

sudo timeout --signal=TERM --kill-after=20s 60s systemctl stop "$UNIT_MAIN" || true
sudo systemctl kill --kill-who=all "$UNIT_MAIN" 2>/dev/null || true
if systemctl is-active --quiet "$UNIT_MAIN"; then
  echo 'STOP: mainnet service did not stop.'
  exit 1
fi

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

START="$(date +%s)"
sudo systemctl daemon-reload
sudo systemctl start "$UNIT_MAIN"

for i in $(seq 1 1440); do
  sleep 5
  ACTIVE="$(systemctl is-active "$UNIT_MAIN" || true)"
  PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
  ELAPSED="$(( $(date +%s) - START ))"

  if [ $((i % 12)) -eq 0 ]; then
    echo "migration elapsed=${ELAPSED}s active=$ACTIVE pid=$PID"
    if [ -n "$PID" ] && [ "$PID" != 0 ]; then
      ps -o pid,stat,etime,pcpu,pmem,rss,args -p "$PID" || true
    fi
    free -h | head -3
  fi

  if [ "$ACTIVE" = active ] && sudo ss -ltnp | grep -q ':17444'; then
    break
  fi

  if [ "$ACTIVE" = failed ] || [ "$ACTIVE" = inactive ]; then
    sudo systemctl status "$UNIT_MAIN" --no-pager -l || true
    sudo journalctl -u "$UNIT_MAIN" -n 250 --no-pager || true
    exit 1
  fi

  if [ "$i" = 1440 ]; then
    echo 'STOP: NYC3 migration/listener exceeded two hours.'
    sudo journalctl -u "$UNIT_MAIN" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
RUN_HASH="$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')"
[ "$RUN_HASH" = "$STAGED_HASH" ]

sudo strings "/proc/$PID/exe" | grep 'QUB Core:1.8.0' | head

timeout --signal=TERM --kill-after=20s 600s \
  "$BIN" --config "$MAINCFG" status-fast > /tmp/hf123-nyc3-status.json

timeout --signal=TERM --kill-after=20s 600s \
  "$BIN" --config "$MAINCFG" storage-stats > /tmp/hf123-nyc3-storage.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf123-nyc3-status.json'))
st=json.load(open('/tmp/hf123-nyc3-storage.json'))
print('height',sf.get('height'),'tip',sf.get('tip_hash'),'engine',sf.get('storage_engine'))
print('journal_bytes',st.get('journal_bytes'),'state_bytes',st.get('state_bytes'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('storage_engine') == 'QUB-FCE-1'
assert st.get('storage_engine') == 'QUB-FCE-1'
PY

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC port is listening.'
  exit 1
fi

# Start testnet only after mainnet canary passes.
sudo systemctl start "$UNIT_TEST" 2>/dev/null || true

trap - EXIT
echo 'NYC3 HF123 CANARY: PASS'
'@

$NYC3Canary | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
```

Expected:

```text
NYC3 HF123 CANARY: PASS
```

Public check:

```powershell
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

---

# F. AMS3 migration and snapshot publisher

## F1. Publisher self-test before live installation

```powershell
@'
set -euo pipefail
cd /opt/jinex/staging/src/qubd
chmod 0755 deploy/digitalocean/publish-mainnet-snapshot.sh deploy/digitalocean/test-publish-mainnet-snapshot.sh
bash -n deploy/digitalocean/publish-mainnet-snapshot.sh
bash -n deploy/digitalocean/test-publish-mainnet-snapshot.sh
timeout --signal=TERM --kill-after=20s 600s bash deploy/digitalocean/test-publish-mainnet-snapshot.sh
echo 'AMS3 HF123 SNAPSHOT SELF-TEST: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

## F2. AMS3 rollout

Use the same mainnet-first migration strategy and keep NYC3 online.

```powershell
$AMS3Rollout = @'
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
BACKUP_BIN="/opt/qub/bin/backups/qubd.before-v1.8.0-hf123-$STAMP"
BACKUP_SNAP="$SNAP_BIN.before-v1.8.0-hf123-$STAMP"
BACKUP_DATA="/opt/qub/backups/mainnet-before-v1.8.0-hf123-$STAMP"

restore_previous_binary() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    echo "HF123 AMS3 rollout failed rc=$rc; restoring previous executable/publisher."
    sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$UNIT_MAIN" 2>/dev/null || true
    sudo systemctl kill --kill-who=all "$UNIT_MAIN" 2>/dev/null || true
    [ -f "$BACKUP_BIN" ] && sudo install -m 0755 -o root -g root "$BACKUP_BIN" "$BIN"
    [ -f "$BACKUP_SNAP" ] && sudo install -m 0755 -o root -g root "$BACKUP_SNAP" "$SNAP_BIN"
    if [ -d /opt/qub/mainnet/data/chain-v2 ]; then
      sudo mv /opt/qub/mainnet/data/chain-v2 "/opt/qub/mainnet/data/chain-v2.failed-hf123-$STAMP" || true
    fi
    sudo systemctl daemon-reload || true
    sudo systemctl start "$UNIT_MAIN" || true
    sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true
  fi
  exit "$rc"
}
trap restore_previous_binary EXIT

if grep -A20 '^\[rpc\]' "$MAINCFG" | grep -qE '^enabled[[:space:]]*=[[:space:]]*true'; then
  echo 'STOP: RPC is enabled in the public AMS3 seed config.'
  exit 1
fi

sudo mkdir -p /opt/qub/bin/backups "$BACKUP_DATA"
sudo cp "$BIN" "$BACKUP_BIN"
[ -f "$SNAP_BIN" ] && sudo cp "$SNAP_BIN" "$BACKUP_SNAP"
for name in chain.json chain-status.json wallet.json wallet-pending-txs.json peers.json; do
  [ -e "/opt/qub/mainnet/data/$name" ] && sudo cp -a "/opt/qub/mainnet/data/$name" "$BACKUP_DATA/$name"
done
[ -d /opt/qub/mainnet/data/chain-v2 ] && sudo cp -a /opt/qub/mainnet/data/chain-v2 "$BACKUP_DATA/chain-v2"

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"

sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$UNIT_TEST" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$UNIT_TEST" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=20s 60s systemctl stop "$UNIT_MAIN" || true
sudo systemctl kill --kill-who=all "$UNIT_MAIN" 2>/dev/null || true

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
sudo install -m 0755 -o root -g root "$SNAP_SRC" "$SNAP_BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

START="$(date +%s)"
sudo systemctl daemon-reload
sudo systemctl start "$UNIT_MAIN"

for i in $(seq 1 1440); do
  sleep 5
  ACTIVE="$(systemctl is-active "$UNIT_MAIN" || true)"
  PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
  if [ $((i % 12)) -eq 0 ]; then
    echo "migration elapsed=$(( $(date +%s) - START ))s active=$ACTIVE pid=$PID"
    [ -n "$PID" ] && [ "$PID" != 0 ] && ps -o pid,stat,etime,pcpu,pmem,rss,args -p "$PID" || true
  fi
  if [ "$ACTIVE" = active ] && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$ACTIVE" = failed ] || [ "$ACTIVE" = inactive ]; then
    sudo journalctl -u "$UNIT_MAIN" -n 250 --no-pager || true
    exit 1
  fi
  if [ "$i" = 1440 ]; then
    echo 'STOP: AMS3 migration/listener exceeded two hours.'
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$UNIT_MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

timeout --signal=TERM --kill-after=20s 600s "$BIN" --config "$MAINCFG" status-fast > /tmp/hf123-ams3-status.json
timeout --signal=TERM --kill-after=20s 600s "$BIN" --config "$MAINCFG" storage-stats > /tmp/hf123-ams3-storage.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf123-ams3-status.json'))
st=json.load(open('/tmp/hf123-ams3-storage.json'))
assert sf.get('ok') is True
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('storage_engine') == 'QUB-FCE-1'
assert st.get('storage_engine') == 'QUB-FCE-1'
print('AMS3 height',sf.get('height'),'tip',sf.get('tip_hash'))
PY

sudo timeout --signal=TERM --kill-after=30s 1200s "$SNAP_BIN"

python3 - <<'PY'
import json
p='/srv/qub-updates/mainnet/snapshots/tip.json'
t='/srv/qub-updates/mainnet/snapshots/tail-64.json'
tip=json.load(open(p)); tail=json.load(open(t))
assert int(tip.get('height',0)) >= 24000
assert int(tail.get('tip_height',0)) == int(tip.get('height',0))
assert tail.get('tip_hash') == tip.get('tip_hash')
assert int(tail['blocks'][-1]['header']['version']) == 2
print('published',tip.get('height'),tip.get('tip_hash'))
PY

sudo systemctl reset-failed "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl reset-failed "$SNAP_TIMER" 2>/dev/null || true
sudo systemctl start "$SNAP_TIMER"
sudo systemctl start "$UNIT_TEST" 2>/dev/null || true

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC port is listening.'
  exit 1
fi

trap - EXIT
echo 'AMS3 HF123 DEPLOYMENT: PASS'
'@

$AMS3Rollout | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# G. Public seed and snapshot checks

```powershell
Test-NetConnection seed.qubit-coin.io -Port 17444
Test-NetConnection seed-ams3.qubit-coin.io -Port 17444
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

Origin snapshot:

```powershell
@'
set -euo pipefail
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/snapshots/tip.json
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 \
  https://download.qubit-coin.io/mainnet/snapshots/tail-64.json \
  | python3 -c 'import json,sys; o=json.load(sys.stdin); print(o["tip_height"],o["tip_hash"],o["blocks"][-1]["header"]["version"]); assert o["tip_height"]>=24000; assert o["blocks"][-1]["header"]["version"]==2'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# H. Optional separate headless node

Do not enable RPC on AMS3/NYC3 public seed configs. Use a separate service/data directory.

Install files:

```bash
sudo install -d -m 0750 -o deploy -g deploy /opt/qub/headless/config /opt/qub/headless/data/mainnet
sudo install -m 0755 -o root -g root target/release/qubd /opt/qub/bin/qubd
sudo install -m 0755 -o root -g root target/release/qub-rpc-miner /opt/qub/bin/qub-rpc-miner
sudo install -m 0644 config/headless-mainnet.toml /opt/qub/headless/config/mainnet.toml
sudo -u deploy bash deploy/digitalocean/generate-rpc-token.sh /opt/qub/headless/config/rpc.token
sudo chmod 0600 /opt/qub/headless/config/rpc.token
sudo install -m 0644 deploy/digitalocean/qub-headless-mainnet.service /etc/systemd/system/qub-headless-mainnet.service
sudo systemctl daemon-reload
sudo systemctl enable --now qub-headless-mainnet.service
```

Use SSH tunneling for operator access:

```powershell
ssh.exe -i $Key -L 17445:127.0.0.1:17445 deploy@<HEADLESS_IP>
```

---

# I. QUB Explorer v0.7 deployment

Use the corrected package that contains neutral aggregate mining statistics only:

```text
QUB_Explorer_Static_v0_7_FINAL-no-assets.zip
SHA256: 17446DAFFA4204AB016A47521A5F489FE99AD051B48F6570F99076956BBBDF61
```

```powershell
$ExplorerZip = "C:\Users\proes\Desktop\qub-node\QUB_Explorer_Static_v0_7_FINAL-no-assets.zip"
$ExplorerHash = (Get-FileHash $ExplorerZip -Algorithm SHA256).Hash.ToUpperInvariant()

if ($ExplorerHash -ne "17446DAFFA4204AB016A47521A5F489FE99AD051B48F6570F99076956BBBDF61") {
    throw "Explorer package SHA mismatch."
}

$ExplorerTemp = "C:\temp\qub-explorer-v07-final"
Remove-Item -Recurse -Force $ExplorerTemp -ErrorAction SilentlyContinue
Expand-Archive $ExplorerZip $ExplorerTemp -Force
$Index = Join-Path $ExplorerTemp "QUB_Explorer_Static_v0_7\index.html"

scp.exe -i $Key $Index "${AMS3}:/tmp/qub-explorer-v0.7-index.html"
```

```powershell
@'
set -euo pipefail
STAMP="$(date +%Y%m%d-%H%M%S)"
sudo cp /srv/qub-explorer/index.html "/srv/qub-explorer/index.html.before-v0.7-$STAMP"
sudo install -m 0644 /tmp/qub-explorer-v0.7-index.html /srv/qub-explorer/index.html
curl -ksL --resolve explorer.qubit-coin.io:443:127.0.0.1 https://explorer.qubit-coin.io/ | grep -q 'Mining'
if grep -Eqi 'exact two-label|same-label streak|address-order' /srv/qub-explorer/index.html; then
  echo 'STOP: forbidden address-order analytics found in Explorer.'
  exit 1
fi
echo 'QUB EXPLORER v0.7 DEPLOYMENT: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Purge:

```text
https://explorer.qubit-coin.io/
https://explorer.qubit-coin.io/index.html
```

---

# J. Windows distribution

## J1. Build

```powershell
cd "C:\Users\proes\Desktop\qub-node\qubd-v1.8.0"
Remove-Item -Recurse -Force .\dist -ErrorAction SilentlyContinue

$ISCC = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found" }
$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
    -Config mainnet -BuildInstaller -SkipTests -SkipPreflight
```

## J2. Manifest

```powershell
$Manifest = (Resolve-Path ".\dist\updates\mainnet\windows-x64\manifest.json").Path
$m = Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json

function Set-JsonProp($Object,[string]$Name,$Value) {
    if ($Object.PSObject.Properties.Name -contains $Name) { $Object.$Name=$Value }
    else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

Set-JsonProp $m "mandatory" $true
Set-JsonProp $m "chain_upgrade" $false
Set-JsonProp $m "hotfix" "HF123"
Set-JsonProp $m "activation_feature" "none"
Set-JsonProp $m "activation_height" 0
Set-JsonProp $m "protocol_epoch" 2
Set-JsonProp $m "protocol_epoch_2_activation_height" 24000
Set-JsonProp $m "post_activation_block_version" 2
Set-JsonProp $m "checkpoint_height" 10367
Set-JsonProp $m "checkpoint_hash" "21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000"
Set-JsonProp $m "consensus_family" "qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000"
Set-JsonProp $m "hotfix_family" "hf123-fast-chain-engine-canonical-memory-batch-persistence-gui-low-latency"
Set-JsonProp $m "notes" "Mandatory QUB Core v1.8.0 HF123 performance and storage release. HF123 introduces no new consensus activation. Protocol Epoch 2 remains active at #24000 and post-activation blocks remain version 2. HF123 introduces the QUB-FCE-1 append-only Fast Chain Engine, atomic current/previous state pointers, validated one-time legacy migration, canonical in-memory P2P/RPC/GUI state sharing, copy-on-write snapshots, batched catch-up persistence, cached GUI derived views, stable status history, Fast Chain Engine-aware snapshot publication and operator storage metrics. HF122 authenticated headless RPC and qub-rpc-miner remain included. DAA, checkpoints, genesis, economics and QUB/JIN rules are unchanged. The USDJ bridge is not part of this release."

[System.IO.File]::WriteAllText(
    $Manifest,
    ($m | ConvertTo-Json -Depth 40) + "`r`n",
    [System.Text.UTF8Encoding]::new($false)
)
```

Regenerate sidecars exactly as in previous releases. Confirm:

```text
version 1.8.0
mandatory true
chain_upgrade false
hotfix HF123
activation_feature none
activation_height 0
protocol_epoch 2
post_activation_block_version 2
```

---

# K. Upload, origin verification and Cloudflare purge

Upload the seven `windows-x64` files plus top-level Latest aliases using the verified argument-array method from HF122.

Install under:

```text
/srv/qub-updates/mainnet/windows-x64
/srv/qub-updates/mainnet/QUB-Core-Latest.exe
```

Verify on origin:

```text
manifest v1.8.0 / HF123
chain_upgrade false
EXE header 4d5a
HTTP 200
```

Purge:

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.0-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.0-Windows-x64-mainnet-Setup.exe.sha256
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe
https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256
https://download.qubit-coin.io/mainnet/snapshots/tip.json
https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
https://download.qubit-coin.io/mainnet/snapshots/tail-256.json
https://download.qubit-coin.io/mainnet/snapshots/tail-1024.json
https://download.qubit-coin.io/mainnet/snapshots/tail-2048.json
https://download.qubit-coin.io/mainnet/snapshots/tail-4096.json
https://download.qubit-coin.io/mainnet/snapshots/chain.json
https://download.qubit-coin.io/mainnet/canonical-chain.json
https://download.qubit-coin.io/
https://download.qubit-coin.io/index.html
```

Public verification must check manifest fields, EXE SHA, post-#24000 snapshot tip/version and a clean installation.

---

# L. GitHub repository and release

Mirror the clean source tree while excluding:

```text
target, dist, data, runtime assets, wallets, chain files, chain-v2 state, executables, archives, environment files
```

Before commit:

```powershell
git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh
git update-index --chmod=+x scripts/test-hf123-fast-chain-regtest.py
git update-index --chmod=+x scripts/test-hf123-rpc-regtest.py
```

Commit/tag:

```powershell
git commit -m "Update QUB Core to v1.8.0 HF123"
git tag -a v1.8.0-hf123 -m "QUB Core v1.8.0 HF123 Fast Chain Engine"
git push origin main
git push origin v1.8.0-hf123
```

Manual GitHub release if `gh` is unavailable:

```text
Tag: v1.8.0-hf123
Title: QUB Core v1.8.0 HF123 - Fast Chain Engine
Description: RELEASE_NOTES-v1.8.0-HF123.md
Set as latest release: yes
Pre-release: no
```

---

# M. Telegram post

```text
🆕 QUB Core v1.8.0 HF123 is live.

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.0-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.8.0-hf123

⚠️ This is a mandatory performance and storage update.

HF123 introduces no new consensus activation.
Protocol Epoch 2 remains active at block #24000, and mainnet blocks from #24000 onward remain block version 2.

✅ Fast Chain Engine

- Replaced the constantly rewritten monolithic chain hot path with QUB-FCE-1.
- New append-only block journal.
- Atomic CURRENT/PREVIOUS committed-state pointers.
- Immutable UTXO/mempool state snapshots.
- Validated one-time migration from the existing chain state.
- Recovery from interrupted journal writes and the previous committed pointer.
- The legacy chain.json remains only as an infrequent compatibility export.

✅ Faster node and GUI architecture

- P2P, embedded RPC and GUI snapshots now share one canonical in-memory state owner.
- Incoming P2P messages no longer reload the complete chain from disk.
- Catch-up validates suffix blocks in memory and performs a controlled persistence commit.
- GUI chain snapshots use copy-on-write immutable state instead of deep-copying the complete history.
- QNS, pools, JIN, governance and QUB/JIN views are cached per canonical tip.
- Catch-up and snapshot workers are true single-flight.
- The status area now keeps a readable timestamped history instead of rapidly replacing one line.

✅ Node operations

- Added storage-stats for Fast Chain Engine metrics.
- Added explicit committed chain export.
- The public snapshot publisher now exports one committed Fast Chain Engine generation before publishing.
- HF122 authenticated headless RPC and qub-rpc-miner remain included.

Mining statistics remain neutral and aggregate-only. A payout address or pool label is not treated as proof of a unique person, machine or operator.

DAA, checkpoints, genesis, economics and QUB/JIN consensus rules are unchanged.
The USDJ bridge is not part of this release and will be delivered separately.

Everyone running QUB Core should update to v1.8.0 HF123.

The first startup can take longer while the existing validated chain is migrated to the Fast Chain Engine. Do not interrupt that first migration and do not delete your data directory.

Do not delete wallet.json.
Do not delete ethereum-wallets.json if you created or imported an Ethereum wallet.
Do not delete wallet-pending-txs.json while transactions are pending.
Never send private keys, wallet files or seed phrases to anyone.
```

---

# Final release state

```text
Local Cargo and both E2E gates pass.
Local mainnet migration/validate/preflight pass.
NYC3 canary runs QUB-FCE-1 and serves port 17444.
AMS3 runs QUB-FCE-1 and serves port 17444.
Snapshot publisher self-test and live publication pass.
Public seed ports are reachable.
Explorer v0.7 contains neutral aggregate mining metrics only.
Windows installer and manifest hashes pass.
Public snapshot is post-#24000 and block version 2.
Clean install and migration smoke pass.
GitHub main/tag/release are updated.
Telegram announcement is published.
```
