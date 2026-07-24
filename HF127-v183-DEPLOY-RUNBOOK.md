# HF127 / QUB Core v1.8.3 End-to-End Deployment Runbook — source revision r2

## Release identity

```text
QUB Core version: 1.8.3
Hotfix: HF127
Package generation: v183
Consensus activation introduced by HF127: none
Mandatory update: yes
Fast Chain Engine schema: QUB-FCE-1 unchanged
P2P protocol number: 2 unchanged
Protocol Epoch 2 activation: #24000 unchanged
Post-activation block version: 2 unchanged
Mainnet pool-share consensus cap: 128 unchanged
```

HF127 is a mandatory non-consensus reliability release for acknowledged transaction delivery, low-latency block/transaction propagation and full-range mining controls. It includes every HF123-HF126 reliability layer unchanged.

Source revision `r2` is a compile-only correction. The public identity remains QUB Core v1.8.3 / HF127 / v183. The initial HF127 source archive is superseded.

Do not delete wallet files or `chain-v2/`. Do not run two state-changing QUB processes against the same data directory. Stop immediately on any real compile failure, failed test, non-v2 mainnet tip, seed listener failure, manifest mismatch, hash mismatch or clean-install failure.

---

# A. Windows extraction and local release gates

## A1. Verify the source archive

Place these files in:

```text
C:\Users\proes\Desktop\qub-node
```

```text
HF127-v183-source-no-assets-r2.zip
HF127-v183-SHA256SUMS-r2.txt
```

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = "Stop"

    $ProjectRoot = "C:\Users\proes\Desktop\qub-node"
    $Zip = Join-Path $ProjectRoot "HF127-v183-source-no-assets-r2.zip"
    $Sums = Join-Path $ProjectRoot "HF127-v183-SHA256SUMS-r2.txt"

    if (-not (Test-Path $Zip)) { throw "Missing source ZIP: $Zip" }
    if (-not (Test-Path $Sums)) { throw "Missing SHA256SUMS: $Sums" }

    $Line = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF127-v183-source-no-assets-r2\.zip$' } |
        Select-Object -First 1

    if (-not $Line) { throw "Source ZIP entry missing from SHA256SUMS." }

    $Expected = ($Line -split '\s+')[0].ToUpperInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

    "Expected: $Expected"
    "Actual:   $Actual"

    if ($Actual -ne $Expected) { throw "HF127 source ZIP SHA mismatch." }

    Write-Host "HF127 SOURCE ZIP GATE: PASS"
}
```

## A2. Clean extraction

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF127-v183-source-no-assets-r2.zip"
$Work = Join-Path $ProjectRoot "qubd-v1.8.3"
$Extract = Join-Path $ProjectRoot "_hf127_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.8.3-before-hf127"

Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) { Rename-Item $Work $Backup }

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.8.3") $Work
Set-Location $Work
```

## A3. Copy known-good runtime assets

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.3-before-hf127\assets",
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

    Select-String -Path .\Cargo.toml -Pattern 'version = "1.8.3"'
    Select-String -Path .\src\p2p.rs -Pattern 'QUB Core:1.8.3|SubmitTx|TxAck|hf127_transaction_parent_guard|hf127_is_submit_role|HF127_TX_RELAY_MAX_PEERS|HF127_OUTBOUND_DIALS_PER_TICK'
    Select-String -Path .\src\lib.rs -Pattern 'connect_block_persist_atomic|save_chain_verified_equal_work_reanchor|PendingTxRecord|pending_tx_delivery_json'
    Select-String -Path .\src\fast_storage.rs -Pattern 'FastCommitPolicy|VerifiedEqualWorkReanchor|publish_live_chain_verified_equal_work_reanchor|stale Fast Chain Engine persistence rejected'
    Select-String -Path .\src\main.rs -Pattern 'tx-delivery-status|relay-pending-txs|hf127_reliable_transaction_delivery|hf127_tx_submit_reserve_lane|hf127_low_latency_propagation|hf126_equal_height_recovery'
    Select-String -Path .\src\bin\qub_core.rs -Pattern 'APP_VERSION: &str = "v1.8.3"|HF127_HIGH_POWER_WARNING_PERCENT|confirm_unrestricted_mining_risk|cpu_worker_override|gpu_work_items_override|No partial CPU nonce-lane plan will be used'
    Select-String -Path .\tests\v1_core.rs -Pattern 'hf126_verified_equal_work_reanchor_persists_and_updates_live_owner|hf127_pending_tx_v1_json_defaults_delivery_fields'
    Select-String -Path .\scripts\test-hf125-block-relay-regtest.py -Pattern 'HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\test-hf126-equal-height-fork-regtest.py -Pattern 'HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\test-hf127-tx-delivery-regtest.py -Pattern 'HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS'

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

    Write-Host "HF127 SOURCE MARKER GATE: PASS"
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

## A6. HF124, HF125, HF126 and HF127 focused regressions

```powershell
cargo test --locked hf124_ -- --nocapture
cargo test --locked hf125_ -- --nocapture
cargo test --locked hf126_ -- --nocapture
cargo test --locked hf127_ -- --nocapture
```

## A7. Real-process E2E gates

The retained test harnesses write temporary Windows paths as forward-slash TOML strings. HF126 retains equal-height overlap delivery. HF127 adds sender-mining-off transaction delivery through the tx-submit reserve lane, seed-to-independent-miner propagation, independent confirmation, two-confirmation outbox cleanup and restart persistence.

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

```powershell
py .\scripts\test-hf127-tx-delivery-regtest.py `
    --qubd .\target\release\qubd.exe `
    --timeout 300
```

Mandatory ending:

```text
HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS
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
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.3"
$QUBD = Join-Path $Root "target\release\qubd.exe"
$SourceCfg = Join-Path $Root "config\mainnet.toml"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf127-v183.toml"

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
$Status | Format-List network,height,tip_hash,tip_block_version,next_block_expected_version,hf125_reliable_block_delivery,hf125_fork_safe_publication,hf126_equal_height_recovery,hf126_equal_height_ties_mineable,hf127_reliable_transaction_delivery,hf127_tx_submit_reserve_lane,hf127_low_latency_propagation,hf127_full_power_mining_controls

if ($Status.ok -ne $true) { throw "status-fast ok=false" }
if ($Status.network -ne 'mainnet') { throw "wrong network" }
if ([int]$Status.height -lt 24000) { throw "mainnet below #24000" }
if ([int]$Status.tip_block_version -ne 2) { throw "post-#24000 tip is not v2" }
if ([int]$Status.next_block_expected_version -ne 2) { throw "next block version is not 2" }
if ($Status.hf126_equal_height_recovery -ne $true) { throw "HF126 equal-height recovery marker missing" }
if ($Status.hf127_reliable_transaction_delivery -ne $true) { throw "HF127 reliable transaction delivery marker missing" }
if ($Status.hf127_tx_submit_reserve_lane -ne $true) { throw "HF127 tx-submit reserve marker missing" }
if ($Status.hf127_low_latency_propagation -ne $true) { throw "HF127 propagation marker missing" }
if ($Status.hf127_full_power_mining_controls -ne $true) { throw "HF127 mining-control marker missing" }

& $QUBD --config $TmpCfg storage-stats
if ($LASTEXITCODE -ne 0) { throw "storage-stats failed" }

& $QUBD --config $TmpCfg validate
if ($LASTEXITCODE -ne 0) { throw "validate failed" }

$Preflight = & $QUBD --config $TmpCfg preflight | ConvertFrom-Json
if ($Preflight.ok -ne $true) { throw "preflight ok=false" }

$Relay = & $QUBD --config $TmpCfg block-relay-status | ConvertFrom-Json
$Relay | Format-List

$TxDelivery = & $QUBD --config $TmpCfg tx-delivery-status | ConvertFrom-Json
$TxDelivery | Format-List ok,network,count,delivery_rule
if ($TxDelivery.ok -ne $true) { throw "tx-delivery-status ok=false" }
if ($TxDelivery.network -ne 'mainnet') { throw "tx-delivery-status wrong network" }

Write-Host "HF127 LOCAL MAINNET GATE: PASS"
```

## A9. GUI transaction and full-power mining smoke

Use a copy of the real mainnet data directory. Do not point two state-changing processes at the same directory.

Verify:

1. QUB Core displays `v1.8.3`.
2. CPU accepts both slider and numeric values from 1 to 100.
3. GPU accepts both slider and numeric values from 0 to 100.
4. At 84%, mining can start normally.
5. At 85% or above, Start solo/pool remains blocked until the red mandatory checkbox is accepted.
6. Changing CPU/GPU power or an advanced override clears the session confirmation.
7. At 100% CPU, the plan uses all logical processors at 100% duty.
8. An intentionally excessive manual CPU worker override fails clearly if the operating system cannot create every requested worker; the round never reports a partial active nonce-lane plan.
9. A transaction with mining disabled reaches `acknowledged` delivery state.
10. `tx-delivery-status <txid>` shows peer ACK metadata and retry state.
11. GUI status polling does not open repeated blind relay loops.
12. A different miner can confirm the transaction.
13. Local and network tips, block pending state and hashrate remain accurate.


# B. Build the seed source archive and upload it

```powershell
$Key = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"

$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.3"
$Deploy = "C:\temp\qubd_hf127_v183_seed_deploy"
$Tar = "C:\temp\qubd-hf127-v183-source.tar.gz"

Remove-Item -Recurse -Force $Deploy -ErrorAction SilentlyContinue
Remove-Item -Force $Tar -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $Deploy | Out-Null

robocopy $Src $Deploy /E `
    /XD .git target dist data .idea node_modules __pycache__ assets `
    /XF *.exe *.dll *.pdb *.ilk *.zip *.tar *.gz *.7z *.log `
        wallet.json ethereum-wallets.json wallet-pending-txs.json `
        chain.json chain-status.json pending-block-relay.json `
        CURRENT.json PREVIOUS.json WRITE.lock .env .env.prod .env.mainnet .env.local

if ($LASTEXITCODE -gt 7) { throw "robocopy failed with $LASTEXITCODE" }

New-Item -ItemType Directory -Force (Join-Path $Deploy 'assets') | Out-Null
Copy-Item (Join-Path $Src 'assets\README.md') (Join-Path $Deploy 'assets\README.md') -Force

$ForbiddenDeploy = @(
    Get-ChildItem $Deploy -Recurse -Force -File |
        Where-Object {
            $_.Name -in @('wallet.json','ethereum-wallets.json','wallet-pending-txs.json','chain.json','chain-status.json','pending-block-relay.json','CURRENT.json','PREVIOUS.json','WRITE.lock','.env') -or
            ($_.Name -like '.env.*' -and $_.Name -notin @('.env.example','.env.sample','.env.template')) -or
            $_.Extension -in @('.exe','.dll','.pdb','.ilk','.zip','.tar','.gz','.7z')
        }
)
if ($ForbiddenDeploy.Count -gt 0) {
    $ForbiddenDeploy | Select-Object FullName | Format-Table -AutoSize
    throw 'Forbidden runtime/private/build files exist in the seed source archive tree.'
}

tar -czf $Tar -C $Deploy .
Get-FileHash $Tar -Algorithm SHA256

scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf127-v183-source.tar.gz"
scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf127-v183-source.tar.gz"
```

---

# C. Build/test the staged source on both seeds

```powershell
$SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf127-v183-source.tar.gz
CFG=/opt/qub/mainnet/mainnet-seed.toml

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 deploy/digitalocean/*.sh 2>/dev/null || true

grep -n 'QUB Core:1.8.3' src/p2p.rs
grep -n 'SubmitBlock' src/p2p.rs
grep -n 'BlockAck' src/p2p.rs
grep -n 'SubmitTx' src/p2p.rs
grep -n 'TxAck' src/p2p.rs
grep -n 'hf127_transaction_parent_guard' src/p2p.rs
grep -n 'connect_block_persist_atomic' src/lib.rs
grep -n 'pending-block-relay.json' src/p2p.rs
grep -n 'HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS' scripts/test-hf125-block-relay-regtest.py
grep -n 'HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS' scripts/test-hf126-equal-height-fork-regtest.py
grep -n 'HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS' scripts/test-hf127-tx-delivery-regtest.py

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

STATUS_FILE=/tmp/hf127-staged-status.json
timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > "$STATUS_FILE"

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf127-staged-status.json'))
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
assert sf.get('hf127_reliable_transaction_delivery') is True
assert sf.get('hf127_tx_submit_reserve_lane') is True
assert sf.get('hf127_low_latency_propagation') is True
assert sf.get('hf127_full_power_mining_controls') is True
PY

ls -lah target/release/qubd
file target/release/qubd
LC_ALL=C grep -aFq '/QUB Core:1.8.3/' target/release/qubd

echo 'HF127 SEED BUILD GATE: PASS'
'@

$SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "AMS3 HF127 build gate failed" }

$SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "NYC3 HF127 build gate failed" }
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
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.3-hf127-$STAMP"

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
LC_ALL=C grep -aFq '/QUB Core:1.8.3/' "$STAGED"

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
  "$BIN" --config "$CFG" status-fast > /tmp/hf127-nyc3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf127-nyc3-status.json'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('hf125_reliable_block_delivery') is True
assert sf.get('hf126_equal_height_recovery') is True
assert sf.get('hf127_reliable_transaction_delivery') is True
assert sf.get('hf127_tx_submit_reserve_lane') is True
assert sf.get('hf127_low_latency_propagation') is True
assert sf.get('hf127_full_power_mining_controls') is True
print('NYC3',sf.get('height'),sf.get('tip_hash'))
PY

"$BIN" --config "$CFG" block-relay-status

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC 17445 is listening.'
  exit 1
fi

sudo systemctl start "$TEST" 2>/dev/null || true

trap - EXIT
echo 'NYC3 HF127 CANARY: PASS'
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "NYC3 HF127 canary failed" }

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
BIN_BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.3-hf127-$STAMP"
SNAP_BACKUP="$SNAP_BIN.backup-before-v1.8.3-hf127-$STAMP"

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
LC_ALL=C grep -aFq '/QUB Core:1.8.3/' "$STAGED"
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
  "$BIN" --config "$CFG" status-fast > /tmp/hf127-ams3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf127-ams3-status.json'))
assert sf.get('ok') is True
assert sf.get('network') == 'mainnet'
assert int(sf.get('height',0)) >= 24000
assert int(sf.get('tip_block_version',0)) == 2
assert int(sf.get('next_block_expected_version',0)) == 2
assert sf.get('hf125_reliable_block_delivery') is True
assert sf.get('hf126_equal_height_recovery') is True
assert sf.get('hf127_reliable_transaction_delivery') is True
assert sf.get('hf127_tx_submit_reserve_lane') is True
assert sf.get('hf127_low_latency_propagation') is True
assert sf.get('hf127_full_power_mining_controls') is True
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
echo 'AMS3 HF127 DEPLOYMENT: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "AMS3 HF127 deployment failed" }
```

---

# F. Update only the separate Explorer API binary on AMS3

The public seed binary is already updated. The read-only Explorer API must run the same HF127 code without restarting the canonical seed.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
BUILT="$SRC/target/release/qubd"
API_BIN=/opt/qub/bin/qubd-explorer-api-hf127
API_UNIT=qub-explorer-api-mainnet.service
SEED_UNIT=qub-seed-mainnet.service
CFG=/opt/qub/mainnet/mainnet-seed.toml
DROPIN_DIR="/etc/systemd/system/$API_UNIT.d"
DROPIN="$DROPIN_DIR/hf127.conf"
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="$API_BIN.backup-$STAMP"
DROPIN_BACKUP_DIR="/tmp/hf127-explorer-api-dropins-$STAMP"
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
LC_ALL=C grep -aFq '/QUB Core:1.8.3/' "$BUILT"

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
  "$DROPIN_DIR/hf127.conf"
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

RESULT="$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/hf127-api-mempool.json -w '%{http_code}|%{time_total}' http://127.0.0.1:18765/api/v1/mempool)"
echo "mempool result: $RESULT"

python3 - "$RESULT" <<'PY'
import json,sys
code,elapsed=sys.argv[1].split('|',1)
data=json.load(open('/tmp/hf127-api-mempool.json'))
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
echo 'HF127 EXPLORER API UPDATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "HF127 Explorer API update failed" }
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

echo 'HF127 SNAPSHOT GATE: PASS'
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
Set-Location "C:\Users\proes\Desktop\qub-node\qubd-v1.8.3"
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
    '.\dist\installer\QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\manifest.json',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe',
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
Set-JsonProp $m 'hotfix' 'HF127'
Set-JsonProp $m 'activation_feature' 'none'
Set-JsonProp $m 'activation_height' 0
Set-JsonProp $m 'protocol_epoch' 2
Set-JsonProp $m 'protocol_epoch_2_activation_height' 24000
Set-JsonProp $m 'post_activation_block_version' 2
Set-JsonProp $m 'checkpoint_height' 10367
Set-JsonProp $m 'checkpoint_hash' '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000'
Set-JsonProp $m 'consensus_family' 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000'
Set-JsonProp $m 'hotfix_family' 'hf127-reliable-transactions-low-latency-propagation-full-power-mining'
Set-JsonProp $m 'notes' 'Mandatory QUB Core v1.8.3 HF127 non-consensus reliability update. HF127 adds explicit SubmitTx/TxAck transaction delivery, fail-closed durable outbox retry, a bounded tx-submit reserve lane, hard pre-sign canonical checks, low-latency TCP and receiver-first diffusion, bounded peer dialing/backoff, peerbook hygiene, full 1-100% CPU and 0-100% GPU controls, mandatory red confirmation at 85%+ or with advanced overrides, and fail-closed requested-worker startup without partial nonce-lane plans. Delivery succeeds through one official ACK, two distinct direct-peer ACKs, or one explicit ACK in isolated deployments without official destinations. HF123-HF126 remain included. Protocol Epoch 2 #24000, block version 2, DAA, rewards, checkpoints, genesis, economics, QUB/JIN rules, pool rules, P2P protocol 2 and QUB-FCE-1 are unchanged.'

$Json=$m | ConvertTo-Json -Depth 40
[System.IO.File]::WriteAllText($Manifest,$Json+"`r`n",[System.Text.UTF8Encoding]::new($false))

$Check=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json
if ($Check.version -ne '1.8.3') { throw 'bad version' }
if ($Check.mandatory -ne $true) { throw 'mandatory=false' }
if ($Check.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
if ($Check.hotfix -ne 'HF127') { throw 'bad hotfix' }
if ($Check.activation_feature -ne 'none') { throw 'bad activation feature' }
if ([int]$Check.activation_height -ne 0) { throw 'bad activation height' }
if ([int]$Check.protocol_epoch_2_activation_height -ne 24000) { throw 'bad epoch2 height' }
if ([int]$Check.post_activation_block_version -ne 2) { throw 'bad block version' }

$UpdateDir='.\dist\updates\mainnet\windows-x64'
Push-Location $UpdateDir
(Get-FileHash manifest.json -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII manifest.json.sha256
(Get-FileHash QUB-Core-Latest.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-Latest.exe.sha256
(Get-FileHash QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe.sha256
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

$Files=@(Get-ChildItem $UpdateDir -File | Sort-Object Name)
if ($Files.Count -ne 7) {
    $Files | Select-Object Name,Length | Format-Table -AutoSize
    throw "Expected exactly 7 windows-x64 update files, found $($Files.Count)."
}
foreach ($File in $Files) {
    scp.exe -i $Key $File.FullName "${AMS3}:/tmp/qub-mainnet-windows-x64/"
    if ($LASTEXITCODE -ne 0) { throw "Upload failed: $($File.Name)" }
}

scp.exe -i $Key $RootLatest "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe"
if ($LASTEXITCODE -ne 0) { throw 'top-level Latest upload failed' }
scp.exe -i $Key $RootLatestSha "${AMS3}:/tmp/QUB-Core-Latest-mainnet.exe.sha256"
if ($LASTEXITCODE -ne 0) { throw 'top-level Latest sidecar upload failed' }
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
sudo cp -a "$DIR" "$DIR.backup-before-v1.8.3-hf127-$STAMP" 2>/dev/null || true
[ -f "$ROOT/QUB-Core-Latest.exe" ] && sudo cp "$ROOT/QUB-Core-Latest.exe" "$ROOT/QUB-Core-Latest.exe.backup-before-v1.8.3-hf127-$STAMP"
[ -f "$ROOT/QUB-Core-Latest.exe.sha256" ] && sudo cp "$ROOT/QUB-Core-Latest.exe.sha256" "$ROOT/QUB-Core-Latest.exe.sha256.backup-before-v1.8.3-hf127-$STAMP"

sudo rsync -av --delete /tmp/qub-mainnet-windows-x64/ "$DIR/"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe "$ROOT/QUB-Core-Latest.exe"
sudo mv /tmp/QUB-Core-Latest-mainnet.exe.sha256 "$ROOT/QUB-Core-Latest.exe.sha256"
sudo find "$ROOT" -type f -exec chmod 0644 {} \;

python3 - <<'PY'
import json
m=json.load(open('/srv/qub-updates/mainnet/windows-x64/manifest.json'))
assert m['version']=='1.8.3'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
assert m['hotfix']=='HF127'
assert m['activation_feature']=='none'
assert int(m['activation_height'])==0
assert int(m['protocol_epoch_2_activation_height'])==24000
assert int(m['post_activation_block_version'])==2
PY

[ "$(head -c 2 "$DIR/QUB-Core-Latest.exe" | xxd -p)" = '4d5a' ]
echo 'HF127 ORIGIN FILE INSTALL: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# L. Origin verification, Cloudflare purge and public gate

Origin:

```powershell
@'
set -euo pipefail
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/manifest.json > /tmp/hf127-origin-manifest.json
python3 - <<'PY'
import json
m=json.load(open('/tmp/hf127-origin-manifest.json'))
print(m['version'],m['hotfix'],m['mandatory'],m['chain_upgrade'])
assert m['version']=='1.8.3'
assert m['hotfix']=='HF127'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
PY
[ "$(curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe | head -c 2 | xxd -p)" = '4d5a' ]
echo 'HF127 ORIGIN GATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Cloudflare Custom Purge:

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe.sha256
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
    $m=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json?verify=hf127-$Nonce" | ConvertFrom-Json
    if ($m.version -ne '1.8.3') { throw 'bad public version' }
    if ($m.hotfix -ne 'HF127') { throw 'bad public hotfix' }
    if ($m.mandatory -ne $true) { throw 'mandatory=false' }
    if ($m.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
    if ($m.activation_feature -ne 'none') { throw 'bad activation feature' }
    if ([int]$m.activation_height -ne 0) { throw 'bad activation height' }

    $Exe=Join-Path $env:TEMP 'QUB-Core-v1.8.3-HF127-Latest.exe'
    curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe?verify=hf127-$Nonce" -o $Exe
    $Bytes=[System.IO.File]::ReadAllBytes($Exe)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw 'public Latest is not EXE' }
    $Actual=(Get-FileHash $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $m.sha256.ToLowerInvariant()) { throw 'public installer SHA mismatch' }

    $Tip=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/snapshots/tip.json?verify=hf127-$Nonce" | ConvertFrom-Json
    if ([int]$Tip.height -lt 24000) { throw 'snapshot below #24000' }

    Write-Host 'HF127 PUBLIC RELEASE GATE: PASS'
}
```

---

# M. Clean-install and auto-update smoke

Install the public versioned installer on a clean test Windows profile/device, and update one existing HF126 installation through the public manifest. Verify all of the following:

```text
1. QUB Core displays v1.8.3.
2. Existing wallets, chain-v2 state and pending transaction outbox survive the upgrade.
3. Fresh installation downloads canonical history and reaches the public version-2 tip.
4. CPU accepts 1–100%; GPU accepts 0–100%.
5. At 85% or above, or with an active manual override, solo/pool start remains blocked until the red session-only hardware-risk checkbox is accepted.
6. Returning both percentages to <=84% and clearing overrides removes the warning requirement.
7. If a manual CPU override exceeds operating-system thread capacity, mining fails clearly and does not continue with a partial nonce-lane plan.
8. block-relay-status, tx-delivery-status and relay-pending-txs are available.
9. A locally found block is reported as durable plus acknowledged, or durable plus automatic retry; it is never presented as publicly delivered from a socket write alone.
```

Run the critical transaction lifecycle with the sender's mining disabled:

```text
A. Sender wallet creates one small ordinary QUB transaction.
B. The GUI reports explicit peer acknowledgement and stores durable retry metadata.
C. tx-delivery-status <txid> shows network_delivered=true or an active bounded retry state with an explicit reason.
D. explorer.qubit-coin.io/api/v1/mempool shows the same txid before confirmation.
E. A different wallet/miner confirms the transaction.
F. The sender remains mining OFF for the entire test.
G. After two confirmations, the sender outbox entry clears automatically.
H. Restarting the sender and one seed preserves canonical confirmation and does not resurrect the transaction into mempool.
```

Record the txid and require the test to pass before publishing the GitHub release or Telegram announcement.

---

# N. GitHub repository, tag and release

Do not tag an unchanged repository. Mirror the clean HF127 source ZIP into the repository before commit/tag.

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference='Stop'

    $ProjectRoot='C:\Users\proes\Desktop\qub-node'
    $Repo=Join-Path $ProjectRoot 'qub-core-opensource'
    $Zip=Join-Path $ProjectRoot 'HF127-v183-source-no-assets-r2.zip'
    $Extract=Join-Path $ProjectRoot '_hf127_repo_extract'
    $Sync=Join-Path $ProjectRoot '_repo_sync_hf127'

    if (-not (Test-Path (Join-Path $Repo '.git'))) { throw "Repo missing: $Repo" }
    if (-not (Test-Path $Zip)) { throw "HF127 source ZIP missing: $Zip" }

    Set-Location $Repo
    if (@(git status --porcelain).Count -gt 0) { throw 'Repository has uncommitted changes.' }
    git fetch origin
    git checkout main
    git pull --ff-only origin main

    Remove-Item -Recurse -Force $Extract,$Sync -ErrorAction SilentlyContinue
    Expand-Archive -Path $Zip -DestinationPath $Extract -Force
    $SourceRoot=Join-Path $Extract 'qubd-v1.8.3'
    if (-not (Test-Path $SourceRoot)) { throw 'HF127 ZIP root missing.' }

    $ReadmeText=Get-Content (Join-Path $SourceRoot 'README.md') -Raw -Encoding UTF8
    $CargoText=Get-Content (Join-Path $SourceRoot 'Cargo.toml') -Raw -Encoding UTF8
    if (-not $ReadmeText.Contains('# QUB Core v1.8.3 HF127')) { throw 'HF127 README marker missing.' }
    if (-not $CargoText.Contains('version = "1.8.3"')) { throw 'Cargo version 1.8.3 missing.' }
    foreach ($Required in @('HF127-v183-DEPLOY-RUNBOOK.md','HF127-v183-RELIABILITY-REVIEW.md','RELEASE_NOTES-v1.8.3-HF127.md','scripts\test-hf127-tx-delivery-regtest.py')) {
        if (-not (Test-Path (Join-Path $SourceRoot $Required))) { throw "Required HF127 source file missing: $Required" }
    }

    $SafeEnvTemplates=@('.env.example','.env.sample','.env.template')
    $Forbidden=@(Get-ChildItem $SourceRoot -Recurse -Force -File | Where-Object {
        $_.FullName -notmatch '\\target\\|\\dist\\|\\.git\\|\\__pycache__\\' -and (
            $_.Name -in @('wallet.json','ethereum-wallets.json','wallet-pending-txs.json','chain.json','chain-status.json','pending-block-relay.json','CURRENT.json','PREVIOUS.json','WRITE.lock','.env') -or
            ($_.Name -like '.env.*' -and $_.Name -notin $SafeEnvTemplates) -or
            $_.Extension -in @('.exe','.dll','.pdb','.ilk','.zip','.tar','.gz','.7z')
        )
    })
    if ($Forbidden.Count -gt 0) { $Forbidden | Select-Object FullName; throw 'Forbidden runtime/private/build files exist in canonical HF127 source.' }

    $AssetFiles=@(Get-ChildItem (Join-Path $SourceRoot 'assets') -Recurse -Force -File)
    if ($AssetFiles.Count -ne 1 -or $AssetFiles[0].Name -ne 'README.md') { $AssetFiles | Select-Object FullName; throw 'HF127 source archive is not no-assets.' }

    New-Item -ItemType Directory -Force $Sync | Out-Null
    robocopy.exe $SourceRoot $Sync /MIR /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Source mirror failed: $LASTEXITCODE" }

    foreach ($Name in @('LICENSE','NOTICE','SECURITY.md','CONTRIBUTING.md','CODE_OF_CONDUCT.md','.gitignore')) {
        $Old=Join-Path $Repo $Name
        if (Test-Path $Old) { Copy-Item $Old (Join-Path $Sync $Name) -Force }
    }
    if (Test-Path (Join-Path $Repo '.github')) {
        Copy-Item (Join-Path $Repo '.github') (Join-Path $Sync '.github') -Recurse -Force
    }

    robocopy.exe $Sync $Repo /MIR /XD (Join-Path $Repo '.git') /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Repo mirror failed: $LASTEXITCODE" }

    Set-Location $Repo
    git diff --check
    $Changes=@(git status --porcelain)
    $Changes
    if ($Changes.Count -eq 0) { throw 'Repository has no HF127 changes. Do not create the tag.' }

    git add -A
    git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
    git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh
    git diff --cached --check
    git diff --cached --stat
    $Staged=@(git diff --cached --name-status)
    if ($Staged.Count -eq 0) { throw 'Nothing staged.' }
    if (-not ($Staged -match 'README\.md')) { throw 'HF127 README is not staged.' }
    if (-not ($Staged -match 'RELEASE_NOTES-v1\.8\.3-HF127\.md')) { throw 'HF127 release notes are not staged.' }
    if (-not ($Staged -match 'scripts/test-hf127-tx-delivery-regtest\.py')) { throw 'HF127 transaction E2E is not staged.' }

    git commit -m 'Update QUB Core to v1.8.3 HF127'
    $Commit=(git rev-parse HEAD).Trim()
    git push origin main
    git fetch origin main
    if ((git rev-parse origin/main).Trim() -ne $Commit) { throw 'Remote main mismatch.' }

    if (git tag -l 'v1.8.3-hf127') { throw 'Tag v1.8.3-hf127 already exists.' }
    git tag -a v1.8.3-hf127 -m 'QUB Core v1.8.3 HF127' $Commit
    git push origin v1.8.3-hf127

    Write-Host 'HF127 GITHUB SOURCE/TAG GATE: PASS'
}
```

Create the GitHub Release from tag `v1.8.3-hf127` with title:

```text
QUB Core v1.8.3 HF127 - Reliable Transactions & Low-Latency Mining
```

Use `RELEASE_NOTES-v1.8.3-HF127.md` as the release body. If `gh` is unavailable, create it manually from the pushed tag. Do not create or move a tag until the source commit exists on `origin/main`.


# O. Telegram announcement

```text
🆕 QUB Core v1.8.3 HF127 is live.

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.3-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.8.3-hf127

HF127 is a mandatory reliability update with no new chain activation.

✅ Reliable transaction delivery
- Explicit SubmitTx / TxAck peer acceptance
- Durable wallet-outbox retries independent from mining, GUI polling and Explorer
- Bounded tx-submit seed reserve lane
- Delivery proof from one official ACK, two distinct direct-peer ACKs, or one explicit ACK in isolated deployments with no official destination
- Hard validated-chain evidence before transaction signing
- tx-delivery-status and relay-pending-txs operator commands

✅ Faster block and transaction propagation
- TCP_NODELAY on latency-sensitive peer sessions
- Seeds begin outward diffusion immediately after durable acceptance and before returning the submit ACK
- Bounded detached peer dialing with exponential backoff
- Bootnodes remain ahead of stale peerbook entries
- Short-lived submit source ports are no longer persisted as peers
- Transaction and block delivery do not make official seeds a permanent liveness dependency

✅ Full-range mining controls
- CPU power: 1–100%
- GPU power: 0–100%
- Exact numeric percentage inputs
- 100% automatic CPU mode uses all logical processors at continuous duty
- An explicit CPU worker override requests that exact worker count; CPU power % still controls duty
- An explicit GPU work-item override removes the previous QUB software ceiling; actual device/driver limits still apply
- 85%+ or any active manual override requires an explicit red, session-only hardware-risk confirmation
- RPC miner workers use disjoint nonce lanes and no longer have the previous QUB worker-count cap
- Requested CPU worker allocation fails closed instead of silently using a partial nonce-lane plan

HF123 Fast Chain Engine, HF124 mining-liveness, HF125 acknowledged block delivery and HF126 equal-height recovery remain included.

Protocol Epoch 2 remains active from #24000 and block version 2 remains unchanged.

After installation, verify that QUB Core shows v1.8.3.
Never share wallet files, private keys or seed phrases.
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
/opt/qub/bin/qubd --config "$CFG" tx-delivery-status
/opt/qub/bin/qubd --config "$CFG" relay-pending-txs
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
/opt/qub/bin/qubd --config "$CFG" tx-delivery-status
free -h
cat /proc/swaps || true
sudo journalctl -u qub-seed-mainnet.service -n 160 --no-pager
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
```

For the first locally sent transaction and locally found post-HF127 block, record:

```text
transaction txid and sender mining state (must be OFF for the transaction test);
tx-delivery-status before ACK, after ACK and after two confirmations;
Explorer mempool appearance and confirmation height;
local height/hash;
block-relay-status before and after acknowledgement;
seed status-fast height/hash;
public snapshot height/hash;
Explorer block appearance;
relay summary accepted/already_known/already_confirmed/stale_parent/rejected/timed_out;
local and observed network hashes at the same height;
whether block delivery used ack_after_suffix or ack_after_overlap;
```

Success condition:

```text
The ordinary transaction is explicitly acknowledged while sender mining is OFF, appears in an independent node mempool, is confirmed by a different miner and clears from the sender outbox after confirmation depth.
The block is durably present locally and obtains an eligible explicit delivery acknowledgement.
Pending transaction/block records clear or become obsolete only after canonical evidence.
AMS3 and NYC3 remain version 2 and continue advertising the same cumulative-work-selected chain.
```
