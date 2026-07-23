# HF126 / QUB Core v1.8.2 End-to-End Deployment Runbook

## Release identity

```text
QUB Core version: 1.8.2
Hotfix: HF126
Package generation: v182
Consensus activation introduced by HF126: none
Mandatory update: yes
Fast Chain Engine schema: QUB-FCE-1 unchanged
P2P protocol number: 2 unchanged
Protocol Epoch 2 activation: #24000 unchanged
Post-activation block version: 2 unchanged
Mainnet pool-share consensus cap: 128 unchanged
```

HF126 is a mandatory non-consensus liveness release for equal-height competing tips. It includes HF123 Fast Chain Engine, HF124 mining-liveness controls and HF125 acknowledged block delivery. Ordinary same-height proof-of-work ties remain mineable; a locally mined unacknowledged tip can use only the strict verified re-anchor path.

Do not delete wallet files or `chain-v2/`. Do not run two state-changing QUB processes against the same data directory. Stop immediately on any real compile failure, failed test, non-v2 mainnet tip, seed listener failure, manifest mismatch, hash mismatch or clean-install failure.

---

# A. Windows extraction and local release gates

## A1. Verify the source archive

Place these files in:

```text
C:\Users\proes\Desktop\qub-node
```

```text
HF126-v182-source-no-assets.zip
HF126-v182-SHA256SUMS.txt
```

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = "Stop"

    $ProjectRoot = "C:\Users\proes\Desktop\qub-node"
    $Zip = Join-Path $ProjectRoot "HF126-v182-source-no-assets.zip"
    $Sums = Join-Path $ProjectRoot "HF126-v182-SHA256SUMS.txt"

    if (-not (Test-Path $Zip)) { throw "Missing source ZIP: $Zip" }
    if (-not (Test-Path $Sums)) { throw "Missing SHA256SUMS: $Sums" }

    $Line = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF126-v182-source-no-assets\.zip$' } |
        Select-Object -First 1

    if (-not $Line) { throw "Source ZIP entry missing from SHA256SUMS." }

    $Expected = ($Line -split '\s+')[0].ToUpperInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

    "Expected: $Expected"
    "Actual:   $Actual"

    if ($Actual -ne $Expected) { throw "HF126 source ZIP SHA mismatch." }

    Write-Host "HF126 SOURCE ZIP GATE: PASS"
}
```

## A2. Clean extraction

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF126-v182-source-no-assets.zip"
$Work = Join-Path $ProjectRoot "qubd-v1.8.2"
$Extract = Join-Path $ProjectRoot "_hf126_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.8.2-before-hf126"

Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) { Rename-Item $Work $Backup }

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.8.2") $Work
Set-Location $Work
```

## A3. Copy known-good runtime assets

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.2-before-hf126\assets",
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.1\assets",
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.0\assets",
    "$env:LOCALAPPDATA\Programs\Qubit Coin Core\assets"
)

$AssetSource = $AssetSources |
    Where-Object { Test-Path $_ } |
    Select-Object -First 1

if (-not $AssetSource) { throw "No runtime assets source found." }

Remove-Item -Recurse -Force .\assets -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force .\assets | Out-Null
Copy-Item (Join-Path $AssetSource "*") .\assets -Recurse -Force
"Assets copied from: $AssetSource"
```

## A4. Source marker and forbidden-path gate

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = "Stop"

    Select-String -Path .\Cargo.toml -Pattern 'version = "1.8.2"'
    Select-String -Path .\src\p2p.rs -Pattern 'QUB Core:1.8.2|HF126|SubmitBlock|BlockAck|pending-block-relay.json|HF125_PRIORITY_INBOUND_RESERVE|HF125_PRIORITY_MAX_SUBMISSIONS|ack_after_overlap|hf126_verified_equal_height_competing_tip'
    Select-String -Path .\src\lib.rs -Pattern 'connect_block_persist_atomic|save_chain_verified_equal_work_reanchor|pub\(crate\) fn write_text_replace'
    Select-String -Path .\src\fast_storage.rs -Pattern 'FastCommitPolicy|VerifiedEqualWorkReanchor|publish_live_chain_verified_equal_work_reanchor|stale Fast Chain Engine persistence rejected'
    Select-String -Path .\src\main.rs -Pattern 'block-relay-status|relay-pending-block|hf125_reliable_block_delivery|hf126_equal_height_recovery|hf126_equal_height_ties_mineable'
    Select-String -Path .\src\bin\qub_core.rs -Pattern 'APP_VERSION: &str = "v1.8.2"|WAITING|PREPARING|Equal-height competing tips|delivery acknowledged|automatic retry active'
    Select-String -Path .\tests\v1_core.rs -Pattern 'hf125_atomic_block_connect_persists_before_publishing_caller_state|hf125_mainnet_storage_rejects_equal_work_same_height_tip_overwrite|hf126_verified_equal_work_reanchor_persists_and_updates_live_owner'
    Select-String -Path .\scripts\test-hf125-block-relay-regtest.py -Pattern 'HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\test-hf126-equal-height-fork-regtest.py -Pattern 'HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS'

    $SafeEnvTemplates = @('.env.example','.env.sample','.env.template')

    $Forbidden = @(
        Get-ChildItem -Recurse -Force -File |
            Where-Object {
                $_.FullName -notmatch '\\target\\|\\dist\\|\\.git\\' -and
                (
                    $_.Name -in @(
                        'wallet.json',
                        'ethereum-wallets.json',
                        'wallet-pending-txs.json',
                        'chain.json',
                        'chain-status.json',
                        'pending-block-relay.json',
                        '.env'
                    ) -or
                    (
                        $_.Name -like '.env.*' -and
                        $_.Name -notin $SafeEnvTemplates
                    ) -or
                    $_.Extension -in @('.exe','.dll','.pdb','.zip','.tar','.gz','.7z')
                )
            }
    )

    if ($Forbidden.Count -gt 0) {
        $Forbidden | Select-Object FullName | Format-Table -AutoSize
        throw "Runtime/private/build files exist in the source tree."
    }

    if (-not (Test-Path .\ethereum\usdj\.env.example)) {
        throw "Expected safe USDJ .env.example template is missing."
    }

    Write-Host "HF126 SOURCE MARKER GATE: PASS"
}
```

## A5. Authoritative Cargo gates

```powershell
cargo test --locked
cargo build --locked --release --bin qubd
cargo build --locked --release --bin qub-core
cargo build --locked --release --bin qub-rpc-miner
```

Warnings alone do not stop the release. Stop on any `error[E...]`, `could not compile`, panic or failed test.

## A6. HF124, HF125 and HF126 focused regressions

```powershell
cargo test --locked hf124_ -- --nocapture
cargo test --locked hf125_ -- --nocapture
cargo test --locked hf126_ -- --nocapture
```

## A7. Real-process E2E gates

The retained test harnesses write temporary Windows paths as forward-slash TOML strings. The HF126 test creates two equal-height sibling branches and proves that the next higher-work suffix is delivered and acknowledged through bounded overlap repair.

```powershell
py .\scripts\test-hf123-fast-chain-regtest.py `
    --qubd .\target\release\qubd.exe

py .\scripts\test-hf123-rpc-regtest.py `
    --qubd .\target\release\qubd.exe `
    --miner .\target\release\qub-rpc-miner.exe

py .\scripts\test-hf125-block-relay-regtest.py `
    --qubd .\target\release\qubd.exe

py .\scripts\test-hf126-equal-height-fork-regtest.py `
    --qubd .\target\release\qubd.exe `
    --timeout 240
```

Expected endings:

```text
HF123 FAST CHAIN ENGINE REGTEST E2E: PASS
HF123 RPC REGTEST E2E: PASS
HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS
HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS
```

## A8. Real-mainnet local validation

Close every QUB Core instance first.

```powershell
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.2"
$QUBD = Join-Path $Root "target\release\qubd.exe"
$SourceCfg = Join-Path $Root "config\mainnet.toml"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf126-v182.toml"

if (-not (Test-Path $QUBD)) { throw "qubd.exe missing: $QUBD" }
if (-not (Test-Path $RealDataDir)) { throw "Mainnet data missing: $RealDataDir" }

$DataDirToml = $RealDataDir -replace '\\','/'
$Cfg = Get-Content $SourceCfg -Raw -Encoding UTF8

if ($Cfg -match '(?m)^data_dir\s*=') {
    $Cfg = [regex]::Replace($Cfg,'(?m)^data_dir\s*=.*$',"data_dir = `"$DataDirToml`"")
} elseif ($Cfg -match '(?m)^\[node\]\s*$') {
    $Cfg = [regex]::Replace($Cfg,'(?m)^\[node\]\s*$',"[node]`r`ndata_dir = `"$DataDirToml`"")
} else {
    $Cfg = "[node]`r`ndata_dir = `"$DataDirToml`"`r`n`r`n" + $Cfg
}

[System.IO.File]::WriteAllText($TmpCfg,$Cfg,[System.Text.UTF8Encoding]::new($false))

$Status = & $QUBD --config $TmpCfg status-fast | ConvertFrom-Json
$Status | Format-List network,height,tip_hash,tip_block_version,next_block_expected_version,hf125_reliable_block_delivery,hf125_fork_safe_publication,hf126_equal_height_recovery,hf126_equal_height_ties_mineable

if ($Status.ok -ne $true) { throw "status-fast ok=false" }
if ($Status.network -ne 'mainnet') { throw "wrong network" }
if ([int]$Status.height -lt 24000) { throw "mainnet below #24000" }
if ([int]$Status.tip_block_version -ne 2) { throw "post-#24000 tip is not v2" }
if ([int]$Status.next_block_expected_version -ne 2) { throw "next block version is not 2" }

& $QUBD --config $TmpCfg storage-stats
if ($LASTEXITCODE -ne 0) { throw "storage-stats failed" }

& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) { throw "validate failed" }

$Preflight = & $QUBD --config $TmpCfg preflight | ConvertFrom-Json
if ($Preflight.ok -ne $true) { throw "preflight ok=false" }

$Relay = & $QUBD --config $TmpCfg block-relay-status | ConvertFrom-Json
$Relay | Format-List

Write-Host "HF126 LOCAL MAINNET GATE: PASS"
```

## A9. GUI equal-height smoke

Back up the real local state before launching the candidate build:

```powershell
$SmokeBackup = Join-Path `
    "C:\Users\proes\Desktop\qub-node" `
    ("_hf126-local-smoke-backup-" + (Get-Date -Format "yyyyMMdd-HHmmss"))

New-Item -ItemType Directory -Force $SmokeBackup | Out-Null

foreach ($Name in @(
    "chain-v2",
    "pending-block-relay.json",
    "wallet.json",
    "ethereum-wallets.json",
    "wallet-pending-txs.json"
)) {
    $Source = Join-Path $RealDataDir $Name
    if (Test-Path $Source) {
        Copy-Item $Source (Join-Path $SmokeBackup $Name) -Recurse -Force
    }
}

"Smoke backup: $SmokeBackup"
```

Launch:

```powershell
cargo run --release --bin qub-core -- --config $TmpCfg
```

Verify the exact failure state shown in the supplied video no longer deadlocks the client:

```text
1. QUB Core displays v1.8.2.
2. Wallets and ordinary pending transactions remain present.
3. The top header says MINING only with non-zero measured hashrate.
4. Template startup says PREPARING; sync/acknowledgement work says WAITING.
5. If local #N and network #N have different hashes, Live Chain displays both.
6. Recent chain blocks show separate “local #N” and “network #N” rows.
7. The client does not remain permanently on “equal-height competing tips” green-light wait.
8. A local unacknowledged tip either:
   - re-anchors after strict matching evidence and full replay, or
   - remains mineable so the next valid block can create the higher-work branch.
9. A block found on one side of the tie stays in pending delivery until explicit acknowledgement.
10. Background catch-up/mining-wait history does not alternate duplicate rows every few seconds.
11. No second state-changing QUB process uses the same data directory.
```

Operator checks while the GUI is closed:

```powershell
& $QUBD --config $TmpCfg block-relay-status
& $QUBD --config $TmpCfg status-fast
```

If `block-relay-status` is pending, retain the file; do not delete it manually. HF126 uses its exact local-tip and competing-tip evidence for recovery.

---

# B. Build the seed source archive and upload it

```powershell
$Key = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"

$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.2"
$Deploy = "C:\temp\qubd_hf126_v182_seed_deploy"
$Tar = "C:\temp\qubd-hf126-v182-source.tar.gz"

Remove-Item -Recurse -Force $Deploy -ErrorAction SilentlyContinue
Remove-Item -Force $Tar -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Deploy | Out-Null

robocopy $Src $Deploy /E `
    /XD .git target dist data .idea node_modules `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json pending-block-relay.json `
        CURRENT.json PREVIOUS.json WRITE.lock .env .env.*

if ($LASTEXITCODE -gt 7) { throw "robocopy failed with $LASTEXITCODE" }

tar -czf $Tar -C $Deploy .
Get-FileHash $Tar -Algorithm SHA256

scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf126-v182-source.tar.gz"
scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf126-v182-source.tar.gz"
```

---

# C. Build/test the staged source on both seeds

```powershell
$SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf126-v182-source.tar.gz
CFG=/opt/qub/mainnet/mainnet-seed.toml

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 deploy/digitalocean/*.sh 2>/dev/null || true

grep -n 'QUB Core:1.8.2' src/p2p.rs
grep -n 'SubmitBlock' src/p2p.rs
grep -n 'BlockAck' src/p2p.rs
grep -n 'connect_block_persist_atomic' src/lib.rs
grep -n 'pending-block-relay.json' src/p2p.rs
grep -n 'HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS' scripts/test-hf125-block-relay-regtest.py
grep -n 'HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS' scripts/test-hf126-equal-height-fork-regtest.py

rustc --version
cargo --version

cargo test --locked --lib --tests --bin qubd
cargo build --locked --release --bin qubd

python3 scripts/test-hf125-block-relay-regtest.py \
    --qubd target/release/qubd \
    --timeout 240

python3 scripts/test-hf126-equal-height-fork-regtest.py \
    --qubd target/release/qubd \
    --timeout 240

STATUS_FILE=/tmp/hf126-staged-status.json
timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > "$STATUS_FILE"

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf126-staged-status.json'))
print('height',sf.get('height'),'tip',sf.get('tip_hash'),'version',sf.get('tip_block_version'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('hf125_reliable_block_delivery') is True
assert sf.get('hf125_fork_safe_publication') is True
assert sf.get('hf126_equal_height_recovery') is True
assert sf.get('hf126_equal_height_ties_mineable') is True
PY

ls -lah target/release/qubd
file target/release/qubd
LC_ALL=C grep -aFq '/QUB Core:1.8.2/' target/release/qubd

echo 'HF126 SEED BUILD GATE: PASS'
'@

$SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "AMS3 HF126 build gate failed" }

$SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "NYC3 HF126 build gate failed" }
```

---

# D. NYC3 canary deployment

Deploy NYC3 first. The script updates only the seed binary and uses bounded service operations with automatic binary recovery on failure.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.2-hf126-$STAMP"

recover() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$MAIN" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$TEST" 2>/dev/null || true
    [ -f "$BACKUP" ] && sudo install -m 0755 -o root -g root "$BACKUP" "$BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$MAIN" || true
    sudo systemctl start "$TEST" 2>/dev/null || true
  fi
  exit "$rc"
}
trap recover EXIT

test -x "$STAGED"
LC_ALL=C grep -aFq '/QUB Core:1.8.2/' "$STAGED"

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"
sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"

sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true
sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$MAIN"

for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$MAIN" --no-pager -l || true
    sudo journalctl -u "$MAIN" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast > /tmp/hf126-nyc3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf126-nyc3-status.json'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('hf125_reliable_block_delivery') is True
print('NYC3',sf.get('height'),sf.get('tip_hash'))
PY

"$BIN" --config "$CFG" block-relay-status

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC 17445 is listening.'
  exit 1
fi

sudo systemctl start "$TEST" 2>/dev/null || true

trap - EXIT
echo 'NYC3 HF126 CANARY: PASS'
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "NYC3 HF126 canary failed" }

Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

---

# E. AMS3 seed deployment

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
STAGED="$SRC/target/release/qubd"
BIN=/opt/qub/bin/qubd
SNAP_SRC="$SRC/deploy/digitalocean/publish-mainnet-snapshot.sh"
SNAP_BIN=/opt/qub/bin/publish-mainnet-snapshot.sh
CFG=/opt/qub/mainnet/mainnet-seed.toml
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
SNAP_TIMER=qub-mainnet-snapshot-publish.timer
SNAP_SERVICE=qub-mainnet-snapshot-publish.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BIN_BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.2-hf126-$STAMP"
SNAP_BACKUP="$SNAP_BIN.backup-before-v1.8.2-hf126-$STAMP"

recover() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
    sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true
    [ -f "$BIN_BACKUP" ] && sudo install -m 0755 -o root -g root "$BIN_BACKUP" "$BIN"
    [ -f "$SNAP_BACKUP" ] && sudo install -m 0755 -o root -g root "$SNAP_BACKUP" "$SNAP_BIN"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$MAIN" || true
    sudo systemctl start "$TEST" 2>/dev/null || true
    sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true
  fi
  exit "$rc"
}
trap recover EXIT

test -x "$STAGED"
test -f "$SNAP_SRC"
LC_ALL=C grep -aFq '/QUB Core:1.8.2/' "$STAGED"
bash -n "$SNAP_SRC"

cd "$SRC"
chmod 0755 deploy/digitalocean/*.sh
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"
sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BIN_BACKUP"
[ -f "$SNAP_BIN" ] && sudo cp "$SNAP_BIN" "$SNAP_BACKUP"

sudo systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
sudo install -m 0755 -o root -g root "$SNAP_SRC" "$SNAP_BIN"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo systemctl daemon-reload
sudo systemctl start "$MAIN"

for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$MAIN" --no-pager -l || true
    sudo journalctl -u "$MAIN" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast > /tmp/hf126-ams3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf126-ams3-status.json'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('hf125_reliable_block_delivery') is True
print('AMS3',sf.get('height'),sf.get('tip_hash'))
PY

"$BIN" --config "$CFG" block-relay-status

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC 17445 is listening.'
  exit 1
fi

sudo systemctl start "$TEST" 2>/dev/null || true
sudo systemctl reset-failed "$SNAP_TIMER" 2>/dev/null || true
sudo systemctl start "$SNAP_TIMER"

trap - EXIT
echo 'AMS3 HF126 DEPLOYMENT: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "AMS3 HF126 deployment failed" }
```

---

# F. Update only the separate Explorer API binary on AMS3

The public seed binary is already updated. The read-only Explorer API must run the same HF126 code without restarting the canonical seed.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
BUILT="$SRC/target/release/qubd"
API_BIN=/opt/qub/bin/qubd-explorer-api-hf126
API_UNIT=qub-explorer-api-mainnet.service
SEED_UNIT=qub-seed-mainnet.service
CFG=/opt/qub/mainnet/mainnet-seed.toml
DROPIN_DIR="/etc/systemd/system/$API_UNIT.d"
DROPIN="$DROPIN_DIR/hf126.conf"
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="$API_BIN.backup-$STAMP"
DROPIN_BACKUP_DIR="/tmp/hf126-explorer-api-dropins-$STAMP"
API_BIN_EXISTED=0
DROPIN_DIR_EXISTED=0

recover() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo systemctl stop "$API_UNIT" 2>/dev/null || true

    if [ "$API_BIN_EXISTED" -eq 1 ] && [ -f "$BACKUP" ]; then
      sudo install -m 0755 -o root -g root "$BACKUP" "$API_BIN"
    else
      sudo rm -f "$API_BIN"
    fi

    sudo rm -rf "$DROPIN_DIR"
    if [ "$DROPIN_DIR_EXISTED" -eq 1 ] && [ -d "$DROPIN_BACKUP_DIR" ]; then
      sudo mkdir -p "$DROPIN_DIR"
      sudo cp -a "$DROPIN_BACKUP_DIR"/. "$DROPIN_DIR"/
    fi

    sudo systemctl daemon-reload || true
    sudo systemctl restart "$API_UNIT" || true
  fi
  exit "$rc"
}
trap recover EXIT

test -x "$BUILT"
LC_ALL=C grep -aFq '/QUB Core:1.8.2/' "$BUILT"

SEED_PID_BEFORE="$(systemctl show -p MainPID --value "$SEED_UNIT")"
SEED_HASH_BEFORE="$(sudo sha256sum "/proc/$SEED_PID_BEFORE/exe" | awk '{print $1}')"

if [ -f "$API_BIN" ]; then
  API_BIN_EXISTED=1
  sudo cp "$API_BIN" "$BACKUP"
fi

if [ -d "$DROPIN_DIR" ]; then
  DROPIN_DIR_EXISTED=1
  mkdir -p "$DROPIN_BACKUP_DIR"
  sudo cp -a "$DROPIN_DIR"/. "$DROPIN_BACKUP_DIR"/
  sudo chown -R "$(id -u):$(id -g)" "$DROPIN_BACKUP_DIR"
fi

sudo install -m 0755 -o root -g root "$BUILT" "$API_BIN"
API_HASH="$(sha256sum "$API_BIN" | awk '{print $1}')"

sudo mkdir -p "$DROPIN_DIR"
sudo rm -f \
  "$DROPIN_DIR/hf123-explorer-api-r3.conf" \
  "$DROPIN_DIR/hf124.conf" \
  "$DROPIN_DIR/hf125.conf" \
  "$DROPIN_DIR/hf126.conf"
sudo tee "$DROPIN" >/dev/null <<EOF
[Service]
ExecStart=
ExecStart=$API_BIN --config $CFG explorer-api 127.0.0.1:18765
EOF

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
[ "$(sudo sha256sum "/proc/$API_PID/exe" | awk '{print $1}')" = "$API_HASH" ]

RESULT="$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/hf126-api-mempool.json -w '%{http_code}|%{time_total}' http://127.0.0.1:18765/api/v1/mempool)"
echo "mempool result: $RESULT"

python3 - "$RESULT" <<'PY'
import json,sys
code,elapsed=sys.argv[1].split('|',1)
data=json.load(open('/tmp/hf126-api-mempool.json'))
assert code == '200'
assert float(elapsed) < 15
assert data.get('network') == 'mainnet'
assert isinstance(data.get('transactions'),list)
assert int(data.get('count',-1)) == len(data['transactions'])
print('API count',data['count'],'elapsed',elapsed)
PY

systemctl is-active qub-explorer-api-caddy.socket
systemctl is-active qub-explorer-api-caddy.service
test -S /srv/qub-updates/run/qub-explorer-api.sock

SEED_PID_AFTER="$(systemctl show -p MainPID --value "$SEED_UNIT")"
SEED_HASH_AFTER="$(sudo sha256sum "/proc/$SEED_PID_AFTER/exe" | awk '{print $1}')"
[ "$SEED_PID_AFTER" = "$SEED_PID_BEFORE" ]
[ "$SEED_HASH_AFTER" = "$SEED_HASH_BEFORE" ]

rm -rf "$DROPIN_BACKUP_DIR"
trap - EXIT
echo 'HF126 EXPLORER API UPDATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "HF126 Explorer API update failed" }
```

---

# G. Publish and verify a fresh canonical snapshot

```powershell
@'
set -euo pipefail

sudo timeout --signal=TERM --kill-after=30s 900s \
  /opt/qub/bin/publish-mainnet-snapshot.sh

cat /srv/qub-updates/mainnet/snapshots/tip.json

python3 - <<'PY'
import json
p='/srv/qub-updates/mainnet/snapshots'
tip=json.load(open(p+'/tip.json'))
tail=json.load(open(p+'/tail-64.json'))
assert tip['network']=='mainnet'
assert int(tip['height']) >= 24000
assert int(tail['tip_height']) == int(tip['height'])
assert tail['tip_hash'] == tip['tip_hash']
assert int(tail['blocks'][-1]['header']['version']) == 2
print('snapshot',tip['height'],tip['tip_hash'])
PY

systemctl is-active qub-mainnet-snapshot-publish.timer

echo 'HF126 SNAPSHOT GATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# H. Public seed checks

```powershell
Test-NetConnection seed.qubit-coin.io -Port 17444
Test-NetConnection seed-ams3.qubit-coin.io -Port 17444
Test-NetConnection seed-nyc3.qubit-coin.io -Port 17444
```

All must report `TcpTestSucceeded : True`.

---

# I. Build the Windows distribution

```powershell
Set-Location "C:\Users\proes\Desktop\qub-node\qubd-v1.8.2"
Remove-Item -Recurse -Force .\dist -ErrorAction SilentlyContinue

$ISCC = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found" }
$env:Path = "$(Split-Path $ISCC);$env:Path"

powershell -ExecutionPolicy Bypass -File .\scripts\build-windows-release.ps1 `
    -Config mainnet `
    -BuildInstaller `
    -SkipTests `
    -SkipPreflight
```

Verify:

```powershell
$Outputs = @(
    '.\dist\installer\QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\manifest.json',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-Latest.exe',
    '.\dist\updates\QUB-Core-Latest.exe'
)
foreach ($Path in $Outputs) { if (-not (Test-Path $Path)) { throw "Missing output: $Path" } }

foreach ($Path in $Outputs | Where-Object { $_ -like '*.exe' }) {
    $Bytes=[System.IO.File]::ReadAllBytes((Resolve-Path $Path).Path)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw "Not an EXE: $Path" }
}
```

---

# J. Patch the public manifest and generate sidecars

```powershell
$Manifest=(Resolve-Path '.\dist\updates\mainnet\windows-x64\manifest.json').Path
$m=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json

function Set-JsonProp($Object,[string]$Name,$Value) {
    if ($Object.PSObject.Properties.Name -contains $Name) { $Object.$Name=$Value }
    else { $Object | Add-Member -NotePropertyName $Name -NotePropertyValue $Value }
}

Set-JsonProp $m 'mandatory' $true
Set-JsonProp $m 'chain_upgrade' $false
Set-JsonProp $m 'hotfix' 'HF126'
Set-JsonProp $m 'activation_feature' 'none'
Set-JsonProp $m 'activation_height' 0
Set-JsonProp $m 'protocol_epoch' 2
Set-JsonProp $m 'protocol_epoch_2_activation_height' 24000
Set-JsonProp $m 'post_activation_block_version' 2
Set-JsonProp $m 'checkpoint_height' 10367
Set-JsonProp $m 'checkpoint_hash' '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000'
Set-JsonProp $m 'consensus_family' 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000'
Set-JsonProp $m 'hotfix_family' 'hf126-equal-height-tie-liveness-verified-reanchor-overlap-delivery-ui'
Set-JsonProp $m 'notes' 'Mandatory QUB Core v1.8.2 HF126 non-consensus liveness update. Protocol Epoch 2 remains active at #24000 and post-activation blocks remain version 2. HF126 keeps fully validated equal-height PoW ties mineable, adds a strict multi-source/full-replay re-anchor path for locally mined unacknowledged tips, extends acknowledged block delivery with bounded overlap repair for sibling branches, preserves normal Fast Chain Engine monotonicity, and separates local/network tips in the GUI with accurate MINING/WAITING/PREPARING states. HF123 Fast Chain Engine, HF124 share-pressure controls and HF125 durable acknowledged delivery remain included. DAA, rewards, checkpoints, genesis, economics, QUB/JIN rules, the 128-share consensus cap, P2P protocol 2 and QUB-FCE-1 are unchanged.'

$Json=$m | ConvertTo-Json -Depth 40
[System.IO.File]::WriteAllText($Manifest,$Json+"`r`n",[System.Text.UTF8Encoding]::new($false))

$Check=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json
if ($Check.version -ne '1.8.2') { throw 'bad version' }
if ($Check.mandatory -ne $true) { throw 'mandatory=false' }
if ($Check.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
if ($Check.hotfix -ne 'HF126') { throw 'bad hotfix' }
if ($Check.activation_feature -ne 'none') { throw 'bad activation feature' }
if ([int]$Check.activation_height -ne 0) { throw 'bad activation height' }
if ([int]$Check.protocol_epoch_2_activation_height -ne 24000) { throw 'bad epoch2 height' }
if ([int]$Check.post_activation_block_version -ne 2) { throw 'bad block version' }

$UpdateDir='.\dist\updates\mainnet\windows-x64'
Push-Location $UpdateDir
(Get-FileHash manifest.json -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII manifest.json.sha256
(Get-FileHash QUB-Core-Latest.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-Latest.exe.sha256
(Get-FileHash QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe.sha256
Get-ChildItem -File | Where-Object { $_.Name -ne 'SHA256SUMS.txt' } | ForEach-Object {
    "$( (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLowerInvariant() )  $($_.Name)"
} | Set-Content -Encoding ASCII SHA256SUMS.txt
Get-Content SHA256SUMS.txt
Pop-Location
(Get-FileHash '.\dist\updates\QUB-Core-Latest.exe' -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII '.\dist\updates\QUB-Core-Latest.exe.sha256'
```

---

# K. Upload and install the public Windows files on AMS3

```powershell
$UpdateDir=(Resolve-Path '.\dist\updates\mainnet\windows-x64').Path
$RootLatest=(Resolve-Path '.\dist\updates\QUB-Core-Latest.exe').Path
$RootLatestSha=(Resolve-Path '.\dist\updates\QUB-Core-Latest.exe.sha256').Path

ssh.exe -i $Key $AMS3 "rm -rf /tmp/qub-mainnet-windows-x64 && mkdir -p /tmp/qub-mainnet-windows-x64"

$Files=Get-ChildItem $UpdateDir -File | Sort-Object Name | Select-Object -ExpandProperty FullName
$Args=@('-i',$Key)+$Files+@("${AMS3}:/tmp/qub-mainnet-windows-x64/")
scp.exe @Args
if ($LASTEXITCODE -ne 0) { throw 'windows-x64 upload failed' }

scp.exe -i $Key $RootLatest "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe"
scp.exe -i $Key $RootLatestSha "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe.sha256"
```

```powershell
@'
set -euo pipefail
STAMP="$(date +%Y%m%d-%H%M%S)"
DIR=/srv/qub-updates/mainnet/windows-x64
ROOT=/srv/qub-updates/mainnet

cd /tmp/qub-mainnet-windows-x64
tr -d '\r' < SHA256SUMS.txt | sha256sum -c -

sudo mkdir -p "$DIR"
sudo cp -a "$DIR" "$DIR.backup-before-v1.8.2-hf126-$STAMP" 2>/dev/null || true
[ -f "$ROOT/QUB-Core-Latest.exe" ] && sudo cp "$ROOT/QUB-Core-Latest.exe" "$ROOT/QUB-Core-Latest.exe.backup-before-v1.8.2-hf126-$STAMP"
[ -f "$ROOT/QUB-Core-Latest.exe.sha256" ] && sudo cp "$ROOT/QUB-Core-Latest.exe.sha256" "$ROOT/QUB-Core-Latest.exe.sha256.backup-before-v1.8.2-hf126-$STAMP"

sudo rsync -av --delete /tmp/qub-mainnet-windows-x64/ "$DIR/"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe "$ROOT/QUB-Core-Latest.exe"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe.sha256 "$ROOT/QUB-Core-Latest.exe.sha256"
sudo find "$ROOT" -type f -exec chmod 0644 {} \;

python3 - <<'PY'
import json
m=json.load(open('/srv/qub-updates/mainnet/windows-x64/manifest.json'))
assert m['version']=='1.8.2'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
assert m['hotfix']=='HF126'
assert m['activation_feature']=='none'
assert int(m['activation_height'])==0
assert int(m['protocol_epoch_2_activation_height'])==24000
assert int(m['post_activation_block_version'])==2
PY

[ "$(head -c 2 "$DIR/QUB-Core-Latest.exe" | xxd -p)" = '4d5a' ]
echo 'HF126 ORIGIN FILE INSTALL: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# L. Origin verification, Cloudflare purge and public gate

Origin:

```powershell
@'
set -euo pipefail
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/manifest.json > /tmp/hf126-origin-manifest.json
python3 - <<'PY'
import json
m=json.load(open('/tmp/hf126-origin-manifest.json'))
print(m['version'],m['hotfix'],m['mandatory'],m['chain_upgrade'])
assert m['version']=='1.8.2'
assert m['hotfix']=='HF126'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
PY
[ "$(curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe | head -c 2 | xxd -p)" = '4d5a' ]
echo 'HF126 ORIGIN GATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Cloudflare Custom Purge:

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe.sha256
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
```

Public gate:

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference='Stop'
    $Nonce=[DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
    $m=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json?verify=hf126-$Nonce" | ConvertFrom-Json
    if ($m.version -ne '1.8.2') { throw 'bad public version' }
    if ($m.hotfix -ne 'HF126') { throw 'bad public hotfix' }
    if ($m.mandatory -ne $true) { throw 'mandatory=false' }
    if ($m.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
    if ($m.activation_feature -ne 'none') { throw 'bad activation feature' }
    if ([int]$m.activation_height -ne 0) { throw 'bad activation height' }

    $Exe=Join-Path $env:TEMP 'QUB-Core-v1.8.2-HF126-Latest.exe'
    curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe?verify=hf126-$Nonce" -o $Exe
    $Bytes=[System.IO.File]::ReadAllBytes($Exe)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw 'public Latest is not EXE' }
    $Actual=(Get-FileHash $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $m.sha256.ToLowerInvariant()) { throw 'public installer SHA mismatch' }

    $Tip=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/snapshots/tip.json?verify=hf126-$Nonce" | ConvertFrom-Json
    if ([int]$Tip.height -lt 24000) { throw 'snapshot below #24000' }

    Write-Host 'HF126 PUBLIC RELEASE GATE: PASS'
}
```

---

# M. Clean-install and auto-update smoke

Install the public versioned installer on a clean test Windows profile/device. Verify:

```text
App displays v1.8.2.
Existing installations receive the mandatory HF126 manifest.
Wallets and chain-v2 state are preserved during upgrade.
Fresh installation downloads canonical history and reaches the public v2 tip.
Mining uses HF124 stable templates.
block-relay-status is available.
A locally found block is reported as durable plus acknowledged, or durable plus automatic retry; it is never reported as publicly delivered without an eligible acknowledgement.
```

---

# N. GitHub repository, tag and release

Mirror the clean source tree exactly as in prior releases, excluding assets/runtime/build data. Preserve repository-only `.github`, license and security files. Then:

```powershell
cd C:\Users\proes\Desktop\qub-node\qub-core-opensource

git diff --check
git status --short
git add .

git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh

git commit -m "Update QUB Core to v1.8.2 HF126"
git tag -a v1.8.2-hf126 -m "QUB Core v1.8.2 HF126"
git push origin main
git push origin v1.8.2-hf126
```

GitHub release:

```powershell
gh release create v1.8.2-hf126 `
    --title "QUB Core v1.8.2 HF126 - Equal-Height Fork Recovery" `
    --notes-file ".\RELEASE_NOTES-v1.8.2-HF126.md"
```

If `gh` is unavailable, create the release manually from the pushed tag. Do not move an already published tag.

---

# O. Telegram announcement

```text
🆕 QUB Core v1.8.2 HF126 is live

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.2-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.8.2-hf126

⚠️ HF126 is a mandatory non-consensus liveness update.

HF126 introduces no new chain activation. Protocol Epoch 2 remains active at block #24000 and mainnet blocks continue to use version 2.

What changed:

- Equal-height proof-of-work branches no longer stop every updated miner. A different valid hash at the same height is treated as an unresolved PoW tie; the next valid block creates the higher-work branch.
- A locally mined, unacknowledged tip can re-anchor only through a strict path: matching durable pending-delivery identity, matching independent network evidence, full consensus/checkpoint replay and an explicit verified Fast Chain Engine commit.
- Normal Fast Chain Engine writes remain cumulative-work monotonic and still reject arbitrary equal-work sibling overwrites.
- Block delivery now repairs a receiver on the competing same-height branch by sending a bounded overlapping suffix, then resubmitting the exact found block and waiting for an explicit acknowledgement.
- Durable block-relay status records the competing height/hash reported by peers.
- The GUI now separates local and observed network candidates at the same height.
- The mining header reports MINING only when real hashrate is non-zero; coordination states display WAITING or PREPARING.
- Recent block history no longer presents a local pending candidate as a confirmed global block.
- Repetitive sync and mining-wait messages are coalesced instead of flooding the bottom status history.

HF123 Fast Chain Engine, HF124 mining-liveness protections and HF125 acknowledged block delivery remain included.

Unchanged:

- DAA and mining rewards
- Genesis and checkpoints
- QUB/JIN consensus rules
- Pool-share consensus cap
- Fast Chain Engine storage format
- P2P protocol number
- Protocol Epoch 2 activation at #24000

Everyone running QUB Core should update to v1.8.2 HF126.

Do not delete wallet.json.
Do not delete ethereum-wallets.json if you use the Ethereum wallet.
Do not delete wallet-pending-txs.json while ordinary transactions are pending.
Do not delete pending-block-relay.json while a locally found block is awaiting acknowledgement.
Do not delete the QUB Core data directory or chain-v2 folder.
Never send wallet files, private keys or seed phrases to anyone.
```

---

# P. Post-deployment monitoring

AMS3:

```powershell
@'
set -euo pipefail
CFG=/opt/qub/mainnet/mainnet-seed.toml
systemctl is-active qub-seed-mainnet.service
systemctl is-active qub-mainnet-snapshot-publish.timer
systemctl is-active qub-explorer-api-mainnet.service
sudo ss -ltnp | grep -E ':(17444|18765)\b'
/opt/qub/bin/qubd --config "$CFG" status-fast
/opt/qub/bin/qubd --config "$CFG" block-relay-status
cat /srv/qub-updates/mainnet/snapshots/tip.json
sudo journalctl -u qub-seed-mainnet.service -n 160 --no-pager
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

NYC3:

```powershell
@'
set -euo pipefail
CFG=/opt/qub/mainnet/mainnet-seed.toml
systemctl is-active qub-seed-mainnet.service
sudo ss -ltnp | grep ':17444'
/opt/qub/bin/qubd --config "$CFG" status-fast
/opt/qub/bin/qubd --config "$CFG" block-relay-status
free -h
cat /proc/swaps || true
sudo journalctl -u qub-seed-mainnet.service -n 160 --no-pager
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
```

For the first locally found post-HF126 block or observed equal-height fork, record:

```text
local height/hash;
block-relay-status before and after acknowledgement;
seed status-fast height/hash;
public snapshot height/hash;
Explorer block appearance;
relay summary accepted/already_known/stale_parent/rejected/timed_out;
local and observed network hashes at the same height;
whether delivery used ack_after_suffix or ack_after_overlap;
```

Success condition:

```text
The block is durably present locally.
At least one eligible peer or matching public canonical state confirms delivery.
The pending relay record clears or becomes obsolete after the canonical chain advances.
AMS3 and NYC3 remain version 2 and continue advertising the same cumulative-work-selected chain.
```
