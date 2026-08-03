# HF128 / QUB Core v1.8.4 End-to-End Deployment Runbook

## Release identity

```text
QUB Core version: 1.8.4
Hotfix: HF128
Package generation: v184
Canary correction: r2 (learned self-alias quarantine)
Windows release correction: r3 (v1.8.4 file metadata and fail-closed native builds)
Consensus activation introduced by HF128: none
Mandatory update: yes
Fast Chain Engine schema: QUB-FCE-1 unchanged
P2P protocol number: 2 unchanged
Protocol Epoch 2 activation: #24000 unchanged
Post-activation block version: 2 unchanged
Mainnet pool-share consensus cap: 128 unchanged
```

HF128 is a non-consensus reliability release. It corrects real CPU/GPU power application and telemetry, removes pool-mining deadlocks without changing HF127 validators, repairs pool+QUB-Melt template incompatibility, makes automatic catch-up single-flight and less redundant, separates AMS3/NYC3 seed identity/configuration, quarantines a DNS alias after it is proven to resolve back to the local node ID, publishes Explorer snapshots atomically, and makes the Windows updater bounded and recoverable. HF123-HF127 remain included.

There is no new activation height, block version, protocol number, checkpoint, reward rule, DAA rule, pool consensus rule, QUB/JIN economic rule or Fast Chain Engine schema. HF127 nodes remain wire/chain compatible during the rolling deployment.

Do not delete wallet files or `chain-v2/`. Do not run two state-changing QUB processes against the same data directory. Stop immediately on any real compile failure, failed test, non-v2 mainnet tip, seed listener failure, manifest mismatch, hash mismatch or clean-install failure.

---

# A. Windows extraction and local release gates

## A1. Verify the source archive

Place these files in:

```text
C:\Users\proes\Desktop\qub-node
```

```text
HF128-v184-source-no-assets-r3.zip
HF128-v184-r3-SHA256SUMS.txt
```

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = "Stop"

    $ProjectRoot = "C:\Users\proes\Desktop\qub-node"
    $Zip = Join-Path $ProjectRoot "HF128-v184-source-no-assets-r3.zip"
    $Sums = Join-Path $ProjectRoot "HF128-v184-r3-SHA256SUMS.txt"

    if (-not (Test-Path $Zip)) { throw "Missing source ZIP: $Zip" }
    if (-not (Test-Path $Sums)) { throw "Missing SHA256SUMS: $Sums" }

    $Line = Get-Content $Sums -Encoding ASCII |
        Where-Object { $_ -match '\sHF128-v184-source-no-assets-r3\.zip$' } |
        Select-Object -First 1

    if (-not $Line) { throw "Source ZIP entry missing from SHA256SUMS." }

    $Expected = ($Line -split '\s+')[0].ToUpperInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToUpperInvariant()

    "Expected: $Expected"
    "Actual:   $Actual"

    if ($Actual -ne $Expected) { throw "HF128 source ZIP SHA mismatch." }

    Write-Host "HF128 SOURCE ZIP GATE: PASS"
}
```

## A2. Clean extraction

```powershell
$ProjectRoot = "C:\Users\proes\Desktop\qub-node"
$Zip = Join-Path $ProjectRoot "HF128-v184-source-no-assets-r3.zip"
$Work = Join-Path $ProjectRoot "qubd-v1.8.4"
$Extract = Join-Path $ProjectRoot "_hf128_extract"
$Backup = Join-Path $ProjectRoot "qubd-v1.8.4-before-hf128-reextract"

Remove-Item -Recurse -Force $Extract -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force $Backup -ErrorAction SilentlyContinue

if (Test-Path $Work) { Rename-Item $Work $Backup }

Expand-Archive -Path $Zip -DestinationPath $Extract -Force
Move-Item (Join-Path $Extract "qubd-v1.8.4") $Work
Set-Location $Work
```

## A3. Copy known-good runtime assets

```powershell
$AssetSources = @(
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.4-before-hf128-reextract\assets",
    "C:\Users\proes\Desktop\qub-node\qubd-v1.8.3\assets",
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

    Select-String -Path .\Cargo.toml -Pattern 'version = "1.8.4"'
    Select-String -Path .\build.rs -Pattern 'ProductVersion", "1.8.4"|FileVersion", "1.8.4.0"'
    Select-String -Path .\src\p2p.rs -Pattern 'QUB Core:1.8.4|remote_node_id_is_self|hf128_quarantine_self_alias|sync_from_height|HF128|hf127_transaction_parent_guard'
    Select-String -Path .\src\lib.rs -Pattern 'PoolCandidateMode|MeltCompatibility|ShareBootstrapCompatibility|preferred_pool_share|legacy_pool_marker_compatible'
    Select-String -Path .\src\pools.rs -Pattern 'POOLCOMPAT1|expected_pool_compatibility_coinbase_outputs'
    Select-String -Path .\src\fast_storage.rs -Pattern 'load_committed_chain_with_identity|FastCommitPolicy|VerifiedEqualWorkReanchor'
    Select-String -Path .\src\main.rs -Pattern 'hf128_live_mining_power_application|hf128_pool_liveness_compatibility|hf128_bounded_chain_sync|hf128_seed_identity_dedupe|hf128_self_alias_quarantine|hf128_atomic_explorer_snapshot|hf128_updater_recovery'
    Select-String -Path .\src\bin\qub_core.rs -Pattern 'APP_VERSION: &str = "v1.8.4"|PlanRejected|gpu_interruptible_duty_sleep|GpuBatchTuner|HF128_CATCHUP_FORCE_PENDING|hf128_curl_download'
    Select-String -Path .\deploy\digitalocean\mainnet-seed-ams3.toml -Pattern 'seed-ams3.qubit-coin.io|seed-nyc3.qubit-coin.io'
    Select-String -Path .\deploy\digitalocean\mainnet-seed-nyc3.toml -Pattern 'seed-nyc3.qubit-coin.io|seed-ams3.qubit-coin.io'
    Select-String -Path .\scripts\test-hf125-block-relay-regtest.py -Pattern 'HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\test-hf126-equal-height-fork-regtest.py -Pattern 'HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\test-hf127-tx-delivery-regtest.py -Pattern 'HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS'
    Select-String -Path .\scripts\build-windows-release.ps1 -Pattern "Assert-NativeSuccess -Operation 'cargo build qub-core'|Assert-NativeSuccess -Operation 'cargo build qubd'|Assert-NativeSuccess -Operation 'cargo build qub-rpc-miner'"

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

    Write-Host "HF128 SOURCE MARKER GATE: PASS"
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

## A6. Focused regressions

```powershell
cargo test --locked hf124_ -- --nocapture
cargo test --locked hf125_ -- --nocapture
cargo test --locked hf126_ -- --nocapture
cargo test --locked hf127_ -- --nocapture
cargo test --locked hf128_ -- --nocapture
cargo test --locked pool -- --nocapture
cargo test --locked gpu -- --nocapture
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
$Root = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.4"
$QUBD = Join-Path $Root "target\release\qubd.exe"
$SourceCfg = Join-Path $Root "config\mainnet.toml"
$RealDataDir = "C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet"
$TmpCfg = Join-Path $env:TEMP "qub-mainnet-hf128-v184.toml"

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
$Status | Format-List network,height,tip_hash,tip_block_version,next_block_expected_version,hf127_reliable_transaction_delivery,hf128_live_mining_power_application,hf128_pool_liveness_compatibility,hf128_bounded_chain_sync,hf128_seed_identity_dedupe,hf128_self_alias_quarantine,hf128_atomic_explorer_snapshot,hf128_updater_recovery

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
if ($Status.hf128_live_mining_power_application -ne $true) { throw "HF128 mining marker missing" }
if ($Status.hf128_pool_liveness_compatibility -ne $true) { throw "HF128 pool marker missing" }
if ($Status.hf128_bounded_chain_sync -ne $true) { throw "HF128 sync marker missing" }
if ($Status.hf128_seed_identity_dedupe -ne $true) { throw "HF128 seed identity marker missing" }
if ($Status.hf128_self_alias_quarantine -ne $true) { throw "HF128 self-alias quarantine marker missing" }
if ($Status.hf128_atomic_explorer_snapshot -ne $true) { throw "HF128 Explorer snapshot marker missing" }
if ($Status.hf128_updater_recovery -ne $true) { throw "HF128 updater marker missing" }

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

Write-Host "HF128 LOCAL MAINNET GATE: PASS"
```

## A9. GUI transaction and full-power mining smoke

Use a copy of the real mainnet data directory. Do not point two state-changing processes at the same directory.

Verify:

1. QUB Core displays `v1.8.4`.
2. CPU accepts both slider and numeric values from 1 to 100.
3. GPU accepts both slider and numeric values from 0 to 100.
4. At 84%, mining can start normally.
5. At 85% or above, Start solo/pool remains blocked until the red mandatory checkbox is accepted.
6. Changing CPU/GPU power or an advanced override clears the session confirmation.
7. Auto CPU at 100% reports `2 x logical CPU count` nonce lanes at continuous duty. This intentionally restores the pre-HF127 throughput plan; it is not an OS CPU-utilization promise.
8. While mining, change CPU 25% -> 50% -> 100%. Each change must stop the acknowledged old plan, restart once, display the applied duty, and materially change measured H/s after a 20-second settling window.
9. GPU 0% starts no OpenCL worker. GPU 25% and 50% use the same efficiency-tuned kernel batch but real interruptible duty sleeps. GPU 100% has no duty sleep. Record 30-second averages; `100%` should materially exceed `50%` on the same device without reusing a low-power batch.
10. GPU aggregate H/s and total hashes never retain a stopped device and never move backward across a transient worker recovery.
11. An intentionally excessive manual CPU override fails once, leaves Mining Off, and does not enter a three-second restart loop.
12. Start a newly joined/empty-score pool. Its first valid pending share must allow a markerless `pool_compat_bootstrap` block; the next parent returns to ordinary `POOLBLOCK1` PPLNS.
13. Put a default-fee QUB Melt for JIN in mempool while pool mining. A markerless `pool_compat_melt` block must confirm it; there must be no `coinbase mismatch` or stopped pool worker. Then confirm ordinary marked pool mining resumes.
14. A transaction with mining disabled reaches `acknowledged`; a different miner can confirm it.
15. Press `Sync` while automatic catch-up runs. It must queue one deep repair behind the active writer, never start two writers and never repeat forever.
16. Local/network tip, block-pending state and hashrate remain accurate.

Keep the raw before/after H/s numbers with the release evidence. Do not promise a universal `2x` GPU ratio: driver, thermals and GPU model matter; the gate is that the requested duty is genuinely applied and 100% is no longer pinned by a low-% tune.


# B. Build the seed source archive and upload it

```powershell
$Key = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"

$Src = "C:\Users\proes\Desktop\qub-node\qubd-v1.8.4"
$Deploy = "C:\temp\qubd_hf128_v184_seed_deploy"
$Tar = "C:\temp\qubd-hf128-v184-source.tar.gz"

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

scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf128-v184-source.tar.gz"
scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf128-v184-source.tar.gz"
```

---

# C. Build/test the staged source on both seeds

```powershell
$SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf128-v184-source.tar.gz
CFG=/opt/qub/mainnet/mainnet-seed.toml

sudo mkdir -p "$SRC"
sudo chown -R deploy:deploy "$SRC"
find "$SRC" -mindepth 1 -maxdepth 1 -exec rm -rf -- {} +
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"

chmod 0755 deploy/digitalocean/*.sh 2>/dev/null || true

grep -n 'QUB Core:1.8.4' src/p2p.rs
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
grep -n 'PoolCandidateMode' src/lib.rs
grep -n 'HF128_CATCHUP_FORCE_PENDING' src/bin/qub_core.rs
grep -n 'load_committed_chain_with_identity' src/fast_storage.rs

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

STATUS_FILE=/tmp/hf128-staged-status.json
timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > "$STATUS_FILE"

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf128-staged-status.json'))
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
for key in (
    'hf128_live_mining_power_application',
    'hf128_pool_liveness_compatibility',
    'hf128_bounded_chain_sync',
    'hf128_seed_identity_dedupe',
    'hf128_self_alias_quarantine',
    'hf128_atomic_explorer_snapshot',
    'hf128_updater_recovery',
):
    assert sf.get(key) is True, key
PY

ls -lah target/release/qubd
file target/release/qubd
LC_ALL=C grep -aFq '/QUB Core:1.8.4/' target/release/qubd

echo 'HF128 SEED BUILD GATE: PASS'
'@

$SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "AMS3 HF128 build gate failed" }

$SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw "NYC3 HF128 build gate failed" }
```

---

## C2. Mandatory AMS3/NYC3 node-identity gate

HF128 will ignore duplicated official evidence, but production must still have two independent node identities. This gate is read-only unless the two IDs are actually identical.

```powershell
$AmsId = (ssh.exe -i $Key $AMS3 "sudo cat /opt/qub/mainnet/data/node_id").Trim()
$NycId = (ssh.exe -i $Key $NYC3 "sudo cat /opt/qub/mainnet/data/node_id").Trim()

"AMS3 node_id: $AmsId"
"NYC3 node_id: $NycId"

if ($AmsId -notmatch '^[0-9a-fA-F]{16,}$') { throw 'AMS3 node_id is invalid' }
if ($NycId -notmatch '^[0-9a-fA-F]{16,}$') { throw 'NYC3 node_id is invalid' }
if ($AmsId -eq $NycId) {
    throw 'STOP: AMS3 and NYC3 have a duplicated node_id. Run the recovery block below before deploying.'
}
Write-Host 'HF128 SEED IDENTITY GATE: PASS'
```

Only if the gate reports an identical ID, regenerate NYC3 recoverably while AMS3 stays online:

```powershell
@'
set -euo pipefail
UNIT=qub-seed-mainnet.service
ID=/opt/qub/mainnet/data/node_id
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/mainnet/data/node_id.duplicate-backup-$STAMP"

OLD="$(sudo cat "$ID")"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$UNIT"
sudo mv "$ID" "$BACKUP"
sudo systemctl start "$UNIT"
for i in $(seq 1 60); do
  sleep 2
  [ -s "$ID" ] && systemctl is-active --quiet "$UNIT" && break
  [ "$i" = 60 ] && exit 1
done
NEW="$(sudo cat "$ID")"
[ "$NEW" != "$OLD" ]
echo "old=$OLD new=$NEW backup=$BACKUP"
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw 'NYC3 node_id regeneration failed; restore the printed backup before continuing.' }

$AmsId = (ssh.exe -i $Key $AMS3 "sudo cat /opt/qub/mainnet/data/node_id").Trim()
$NycId = (ssh.exe -i $Key $NYC3 "sudo cat /opt/qub/mainnet/data/node_id").Trim()
if ($AmsId -eq $NycId) { throw 'STOP: node IDs are still identical' }
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
REGIONAL_CFG="$SRC/deploy/digitalocean/mainnet-seed-nyc3.toml"
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.4-hf128-$STAMP"
CFG_BACKUP="$CFG.backup-before-v1.8.4-hf128-$STAMP"

recover() {
  rc=$?
  trap - EXIT
  if [ "$rc" -ne 0 ]; then
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$MAIN" 2>/dev/null || true
    sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$TEST" 2>/dev/null || true
    [ -f "$BACKUP" ] && sudo install -m 0755 -o root -g root "$BACKUP" "$BIN"
    [ -f "$CFG_BACKUP" ] && sudo install -m 0644 -o root -g root "$CFG_BACKUP" "$CFG"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$MAIN" || true
    sudo systemctl start "$TEST" 2>/dev/null || true
  fi
  exit "$rc"
}
trap recover EXIT

echo '[NYC3 1/6] Verifying staged HF128 r2 binary and regional config'
test -x "$STAGED"
test -f "$REGIONAL_CFG"
LC_ALL=C grep -aFq '/QUB Core:1.8.4/' "$STAGED"
grep -Fq 'advertise_addr = "seed-nyc3.qubit-coin.io:17444"' "$REGIONAL_CFG"
grep -Fq 'bootnodes = ["seed-ams3.qubit-coin.io:17444"]' "$REGIONAL_CFG"

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"
echo '[NYC3 2/6] Creating recoverable binary/config backups'
sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BACKUP"
sudo cp "$CFG" "$CFG_BACKUP"

echo '[NYC3 3/6] Stopping seed services and installing HF128 r2'
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true
sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
sudo install -m 0644 -o root -g root "$REGIONAL_CFG" "$CFG"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo timeout --signal=TERM --kill-after=10s 60s systemctl daemon-reload
sudo timeout --signal=TERM --kill-after=10s 60s systemctl start "$MAIN"

echo '[NYC3 4/6] Waiting up to 10 minutes for mainnet service + port 17444'
for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ $((i % 6)) -eq 0 ]; then echo "NYC3 wait: $((i * 5))/600 seconds"; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$MAIN" --no-pager -l || true
    sudo journalctl -u "$MAIN" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

echo '[NYC3 5/6] Running bounded status-fast and block-relay gates (up to 5 minutes)'
timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast > /tmp/hf128-nyc3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf128-nyc3-status.json'))
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
assert sf.get('hf128_pool_liveness_compatibility') is True
assert sf.get('hf128_bounded_chain_sync') is True
assert sf.get('hf128_seed_identity_dedupe') is True
assert sf.get('hf128_self_alias_quarantine') is True
print('NYC3',sf.get('height'),sf.get('tip_hash'))
PY

"$BIN" --config "$CFG" block-relay-status

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC 17445 is listening.'
  exit 1
fi

echo '[NYC3 6/6] Restoring testnet service'
sudo timeout --signal=TERM --kill-after=10s 60s systemctl start "$TEST" 2>/dev/null || true

trap - EXIT
echo 'NYC3 HF128 CANARY: PASS'
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "NYC3 HF128 canary failed" }

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
REGIONAL_CFG="$SRC/deploy/digitalocean/mainnet-seed-ams3.toml"
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
SNAP_TIMER=qub-mainnet-snapshot-publish.timer
SNAP_SERVICE=qub-mainnet-snapshot-publish.service
STAMP="$(date +%Y%m%d-%H%M%S)"
BIN_BACKUP="/opt/qub/bin/backups/qubd.backup-before-v1.8.4-hf128-$STAMP"
SNAP_BACKUP="$SNAP_BIN.backup-before-v1.8.4-hf128-$STAMP"
CFG_BACKUP="$CFG.backup-before-v1.8.4-hf128-$STAMP"

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
    [ -f "$CFG_BACKUP" ] && sudo install -m 0644 -o root -g root "$CFG_BACKUP" "$CFG"
    sudo systemctl daemon-reload || true
    sudo systemctl start "$MAIN" || true
    sudo systemctl start "$TEST" 2>/dev/null || true
    sudo systemctl start "$SNAP_TIMER" 2>/dev/null || true
  fi
  exit "$rc"
}
trap recover EXIT

echo '[AMS3 1/7] Verifying staged HF128 r2 binary, snapshot publisher and regional config'
test -x "$STAGED"
test -f "$SNAP_SRC"
test -f "$REGIONAL_CFG"
LC_ALL=C grep -aFq '/QUB Core:1.8.4/' "$STAGED"
grep -Fq 'advertise_addr = "seed-ams3.qubit-coin.io:17444"' "$REGIONAL_CFG"
grep -Fq 'bootnodes = ["seed-nyc3.qubit-coin.io:17444"]' "$REGIONAL_CFG"
bash -n "$SNAP_SRC"

cd "$SRC"
chmod 0755 deploy/digitalocean/*.sh
echo '[AMS3 2/7] Running snapshot publisher self-test'
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh

STAGED_HASH="$(sha256sum "$STAGED" | awk '{print $1}')"
echo '[AMS3 3/7] Creating recoverable binary/publisher/config backups'
sudo mkdir -p /opt/qub/bin/backups
sudo cp "$BIN" "$BIN_BACKUP"
[ -f "$SNAP_BIN" ] && sudo cp "$SNAP_BIN" "$SNAP_BACKUP"
sudo cp "$CFG" "$CFG_BACKUP"

echo '[AMS3 4/7] Stopping publication and seed services; installing HF128 r2'
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 30s systemctl stop "$SNAP_SERVICE" 2>/dev/null || true
sudo systemctl kill --kill-who=all "$SNAP_SERVICE" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$MAIN"
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$TEST" 2>/dev/null || true

sudo install -m 0755 -o root -g root "$STAGED" "$BIN"
sudo install -m 0755 -o root -g root "$SNAP_SRC" "$SNAP_BIN"
sudo install -m 0644 -o root -g root "$REGIONAL_CFG" "$CFG"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$STAGED_HASH" ]

sudo timeout --signal=TERM --kill-after=10s 60s systemctl daemon-reload
sudo timeout --signal=TERM --kill-after=10s 60s systemctl start "$MAIN"

echo '[AMS3 5/7] Waiting up to 10 minutes for mainnet service + port 17444'
for i in $(seq 1 120); do
  sleep 5
  if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep -q ':17444'; then break; fi
  if [ $((i % 6)) -eq 0 ]; then echo "AMS3 wait: $((i * 5))/600 seconds"; fi
  if [ "$i" = 120 ]; then
    sudo systemctl status "$MAIN" --no-pager -l || true
    sudo journalctl -u "$MAIN" -n 250 --no-pager || true
    exit 1
  fi
done

PID="$(systemctl show -p MainPID --value "$MAIN")"
[ "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" = "$STAGED_HASH" ]

echo '[AMS3 6/7] Running bounded status-fast and block-relay gates (up to 5 minutes)'
timeout --signal=TERM --kill-after=20s 300s \
  "$BIN" --config "$CFG" status-fast > /tmp/hf128-ams3-status.json

python3 - <<'PY'
import json
sf=json.load(open('/tmp/hf128-ams3-status.json'))
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
assert sf.get('hf128_pool_liveness_compatibility') is True
assert sf.get('hf128_bounded_chain_sync') is True
assert sf.get('hf128_seed_identity_dedupe') is True
assert sf.get('hf128_self_alias_quarantine') is True
print('AMS3',sf.get('height'),sf.get('tip_hash'))
PY

"$BIN" --config "$CFG" block-relay-status

if sudo ss -ltnp | grep -q ':17445'; then
  echo 'STOP: public seed RPC 17445 is listening.'
  exit 1
fi

echo '[AMS3 7/7] Restoring testnet and snapshot timer with bounded starts'
sudo timeout --signal=TERM --kill-after=10s 60s systemctl start "$TEST" 2>/dev/null || true
sudo systemctl reset-failed "$SNAP_TIMER" 2>/dev/null || true
sudo timeout --signal=TERM --kill-after=10s 60s systemctl start "$SNAP_TIMER"

trap - EXIT
echo 'AMS3 HF128 DEPLOYMENT: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "AMS3 HF128 deployment failed" }
```

---

# F. Update only the separate Explorer API binary on AMS3

The public seed binary is already updated. The read-only Explorer API must run the exact same HF128 executable without restarting the canonical seed or creating a second state writer.

```powershell
@'
set -euo pipefail

SRC=/opt/jinex/staging/src/qubd
BUILT="$SRC/target/release/qubd"
API_BIN=/opt/qub/bin/qubd-explorer-api-hf128
API_UNIT=qub-explorer-api-mainnet.service
SEED_UNIT=qub-seed-mainnet.service
CFG=/opt/qub/mainnet/mainnet-seed.toml
DROPIN_DIR="/etc/systemd/system/$API_UNIT.d"
DROPIN="$DROPIN_DIR/hf128.conf"
STAMP="$(date +%Y%m%d-%H%M%S)"
BACKUP="$API_BIN.backup-$STAMP"
DROPIN_BACKUP_DIR="/tmp/hf128-explorer-api-dropins-$STAMP"
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
LC_ALL=C grep -aFq '/QUB Core:1.8.4/' "$BUILT"

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
  "$DROPIN_DIR/hf126.conf" \
  "$DROPIN_DIR/hf127.conf" \
  "$DROPIN_DIR/hf128.conf"
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

RESULT="$(curl -sS --connect-timeout 5 --max-time 30 -o /tmp/hf128-api-mempool.json -w '%{http_code}|%{time_total}' http://127.0.0.1:18765/api/v1/mempool)"
echo "mempool result: $RESULT"

python3 - "$RESULT" <<'PY'
import json,sys
code,elapsed=sys.argv[1].split('|',1)
data=json.load(open('/tmp/hf128-api-mempool.json'))
assert code == '200'
assert float(elapsed) < 15
assert data.get('network') == 'mainnet'
assert isinstance(data.get('transactions'),list)
assert int(data.get('count',-1)) == len(data['transactions'])
print('API count',data['count'],'elapsed',elapsed)
PY

curl -fsS --connect-timeout 5 --max-time 30 \
  http://127.0.0.1:18765/api/v1/status-fast > /tmp/hf128-api-status.json
python3 - <<'PY'
import json
s=json.load(open('/tmp/hf128-api-status.json'))
assert s.get('ok') is True
assert s.get('network') == 'mainnet'
assert s.get('version') == '1.8.4'
assert s.get('hf128_atomic_explorer_snapshot') is True
assert s.get('hf128_self_alias_quarantine') is True
print('Explorer API',s.get('height'),s.get('tip_hash'))
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
echo 'HF128 EXPLORER API UPDATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"

if ($LASTEXITCODE -ne 0) { throw "HF128 Explorer API update failed" }
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

echo 'HF128 SNAPSHOT GATE: PASS'
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

Stage I must use the r3 source correction. It changes only Windows resource
metadata and release-build failure handling. The already-deployed r2 Linux
seed/API binaries and all consensus/network behavior are unchanged.

```powershell
Set-Location "C:\Users\proes\Desktop\qub-node\qubd-v1.8.4"
Remove-Item -Recurse -Force .\dist -ErrorAction SilentlyContinue

$ISCC = @(
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe"
) | Where-Object { Test-Path $_ } | Select-Object -First 1

if (-not $ISCC) { throw "ISCC.exe not found" }
$env:Path = "$(Split-Path $ISCC);$env:Path"

$BuildArgs=@{
    Config='mainnet'
    BuildInstaller=$true
    SkipTests=$true
    SkipPreflight=$true
}
if ($env:QUB_SIGN_CERT_THUMBPRINT) {
    $BuildArgs.Sign=$true
    $BuildArgs.CertThumbprint=$env:QUB_SIGN_CERT_THUMBPRINT
}
& .\scripts\build-windows-release.ps1 @BuildArgs
```

Verify:

```powershell
$Outputs = @(
    '.\dist\installer\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\manifest.json',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe',
    '.\dist\updates\mainnet\windows-x64\QUB-Core-Latest.exe',
    '.\dist\updates\QUB-Core-Latest.exe'
)
foreach ($Path in $Outputs) { if (-not (Test-Path $Path)) { throw "Missing output: $Path" } }

foreach ($Path in $Outputs | Where-Object { $_ -like '*.exe' }) {
    $Bytes=[System.IO.File]::ReadAllBytes((Resolve-Path $Path).Path)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw "Not an EXE: $Path" }
}

$VersionedExecutables = @(
    '.\dist\QUB-Core-v1.8.4-windows-x64-mainnet\QUB-Core.exe',
    '.\dist\QUB-Core-v1.8.4-windows-x64-mainnet\tools\qubd.exe',
    '.\dist\QUB-Core-v1.8.4-windows-x64-mainnet\tools\qub-rpc-miner.exe',
    '.\dist\installer\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe'
)

foreach ($Path in $VersionedExecutables) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Missing versioned executable: $Path"
    }

    $ProductVersion = [string](Get-Item -LiteralPath $Path).VersionInfo.ProductVersion
    if (-not $ProductVersion.StartsWith('1.8.4')) {
        throw "Wrong ProductVersion for $Path: $ProductVersion"
    }

    Write-Host "[VERSION] $Path -> $ProductVersion"
}

$Installer=(Resolve-Path '.\dist\installer\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe').Path
$Sig=Get-AuthenticodeSignature $Installer
if ($env:QUB_SIGN_CERT_THUMBPRINT) {
    if ($Sig.Status -ne 'Valid') { throw "Signed installer verification failed: $($Sig.Status)" }
    if ($Sig.SignerCertificate.Subject -notlike '*Alexander Proestakis*') { throw 'Unexpected installer signer' }
} elseif ($Sig.Status -ne 'NotSigned') {
    throw "Unexpected unsigned-private-build signature state: $($Sig.Status)"
} else {
    Write-Warning 'Private unsigned build: keep manifest and installer on the exact official download.qubit-coin.io HTTPS origin. Custom/cross-origin updates will be rejected.'
}

$InstallerHash=(Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash.ToLowerInvariant()
$VersionedUpdate=(Resolve-Path '.\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe').Path
$ChannelLatest=(Resolve-Path '.\dist\updates\mainnet\windows-x64\QUB-Core-Latest.exe').Path
$RootLatest=(Resolve-Path '.\dist\updates\QUB-Core-Latest.exe').Path

foreach ($Copy in @($VersionedUpdate,$ChannelLatest,$RootLatest)) {
    $CopyHash=(Get-FileHash -LiteralPath $Copy -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($CopyHash -ne $InstallerHash) {
        throw "Installer copy hash mismatch: $Copy"
    }
}

Write-Host "Installer SHA-256: $InstallerHash"
Write-Host 'HF128 r3 WINDOWS DISTRIBUTION GATE: PASS'
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
Set-JsonProp $m 'hotfix' 'HF128'
Set-JsonProp $m 'activation_feature' 'none'
Set-JsonProp $m 'activation_height' 0
Set-JsonProp $m 'protocol_epoch' 2
Set-JsonProp $m 'protocol_epoch_2_activation_height' 24000
Set-JsonProp $m 'post_activation_block_version' 2
Set-JsonProp $m 'checkpoint_height' 10367
Set-JsonProp $m 'checkpoint_hash' '21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000'
Set-JsonProp $m 'consensus_family' 'qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000'
$Installer='QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe'
$InstallerPath=Join-Path (Split-Path $Manifest) $Installer
$InstallerHash=(Get-FileHash $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
$InstallerBytes=[int64](Get-Item $InstallerPath).Length
Set-JsonProp $m 'url' "https://download.qubit-coin.io/mainnet/windows-x64/$Installer"
Set-JsonProp $m 'sha256' $InstallerHash
Set-JsonProp $m 'size' $InstallerBytes
Set-JsonProp $m 'hotfix_family' 'hf128-mining-pool-sync-seed-explorer-updater-reliability'
Set-JsonProp $m 'notes' 'QUB Core v1.8.4 HF128 is a non-consensus reliability update: true CPU/GPU duty application and truthful telemetry; pool bootstrap and QUB Melt compatibility under unchanged HF127 validators; single-flight catch-up; independent seed identity; atomic Explorer snapshots; and bounded recoverable Windows updates. No activation height, protocol bump, block-version change, checkpoint change, DAA/reward/economic change or QUB-FCE schema change.'

$Json=$m | ConvertTo-Json -Depth 40
[System.IO.File]::WriteAllText($Manifest,$Json+"`r`n",[System.Text.UTF8Encoding]::new($false))

$Check=Get-Content $Manifest -Raw -Encoding UTF8 | ConvertFrom-Json
if ($Check.version -ne '1.8.4') { throw 'bad version' }
if ($Check.mandatory -ne $true) { throw 'mandatory=false' }
if ($Check.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
if ($Check.hotfix -ne 'HF128') { throw 'bad hotfix' }
if ($Check.activation_feature -ne 'none') { throw 'bad activation feature' }
if ([int]$Check.activation_height -ne 0) { throw 'bad activation height' }
if ([int]$Check.protocol_epoch_2_activation_height -ne 24000) { throw 'bad epoch2 height' }
if ([int]$Check.post_activation_block_version -ne 2) { throw 'bad block version' }
if ($Check.sha256 -ne $InstallerHash) { throw 'bad installer hash' }
if ([int64]$Check.size -ne $InstallerBytes) { throw 'bad installer size' }
if (([uri]$Check.url).Host -ne 'download.qubit-coin.io') { throw 'installer URL must use official origin' }

$UpdateDir='.\dist\updates\mainnet\windows-x64'
Push-Location $UpdateDir
(Get-FileHash manifest.json -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII manifest.json.sha256
(Get-FileHash QUB-Core-Latest.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-Latest.exe.sha256
(Get-FileHash QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe -Algorithm SHA256).Hash.ToLowerInvariant() | Set-Content -Encoding ASCII QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe.sha256
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
sudo cp -a "$DIR" "$DIR.backup-before-v1.8.4-hf128-$STAMP" 2>/dev/null || true
[ -f "$ROOT/QUB-Core-Latest.exe" ] && sudo cp "$ROOT/QUB-Core-Latest.exe" "$ROOT/QUB-Core-Latest.exe.backup-before-v1.8.4-hf128-$STAMP"
[ -f "$ROOT/QUB-Core-Latest.exe.sha256" ] && sudo cp "$ROOT/QUB-Core-Latest.exe.sha256" "$ROOT/QUB-Core-Latest.exe.sha256.backup-before-v1.8.4-hf128-$STAMP"

# Publish installers and sidecars first. The manifest is the release pointer
# and is atomically renamed last, so no client can observe a manifest whose
# artifact has not arrived yet.
sudo rsync -av --exclude manifest.json /tmp/qub-mainnet-windows-x64/ "$DIR/"
sudo install -m 0644 /tmp/QUB-Core-Latest-mainnet.exe "$ROOT/QUB-Core-Latest.exe"
sudo install -m 0644 /tmp/QUB-Core-Latest-mainnet.exe.sha256 "$ROOT/QUB-Core-Latest.exe.sha256"
sudo install -m 0644 /tmp/qub-mainnet-windows-x64/manifest.json "$DIR/.manifest.json.hf128-$STAMP.tmp"
sudo mv "$DIR/.manifest.json.hf128-$STAMP.tmp" "$DIR/manifest.json"
sudo find "$ROOT" -type f -exec chmod 0644 {} \;

python3 - <<'PY'
import json
m=json.load(open('/srv/qub-updates/mainnet/windows-x64/manifest.json'))
assert m['version']=='1.8.4'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
assert m['hotfix']=='HF128'
assert m['activation_feature']=='none'
assert int(m['activation_height'])==0
assert int(m['protocol_epoch_2_activation_height'])==24000
assert int(m['post_activation_block_version'])==2
assert len(m['sha256'])==64
assert int(m['size']) > 0
PY

[ "$(head -c 2 "$DIR/QUB-Core-Latest.exe" | xxd -p)" = '4d5a' ]
test "$(sha256sum "$DIR/QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe" | awk '{print $1}')" = "$(python3 -c 'import json; print(json.load(open("/srv/qub-updates/mainnet/windows-x64/manifest.json"))["sha256"])')"
echo 'HF128 ORIGIN FILE INSTALL: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

---

# L. Origin verification, Cloudflare purge and public gate

Origin:

```powershell
@'
set -euo pipefail
curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/manifest.json > /tmp/hf128-origin-manifest.json
python3 - <<'PY'
import json
m=json.load(open('/tmp/hf128-origin-manifest.json'))
print(m['version'],m['hotfix'],m['mandatory'],m['chain_upgrade'])
assert m['version']=='1.8.4'
assert m['hotfix']=='HF128'
assert m['mandatory'] is True
assert m['chain_upgrade'] is False
assert len(m['sha256']) == 64
assert int(m['size']) > 0
PY
[ "$(curl -ksL --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe | head -c 2 | xxd -p)" = '4d5a' ]
echo 'HF128 ORIGIN GATE: PASS'
'@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Cloudflare Custom Purge:

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe.sha256
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
    $m=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/manifest.json?verify=hf128-$Nonce" | ConvertFrom-Json
    if ($m.version -ne '1.8.4') { throw 'bad public version' }
    if ($m.hotfix -ne 'HF128') { throw 'bad public hotfix' }
    if ($m.mandatory -ne $true) { throw 'mandatory=false' }
    if ($m.chain_upgrade -ne $false) { throw 'chain_upgrade must be false' }
    if ($m.activation_feature -ne 'none') { throw 'bad activation feature' }
    if ([int]$m.activation_height -ne 0) { throw 'bad activation height' }

    if ([int64]$m.size -le 0) { throw 'manifest size missing' }
    if (([uri]$m.url).Host -ne 'download.qubit-coin.io') { throw 'cross-origin installer URL' }

    $Exe=Join-Path $env:TEMP 'QUB-Core-v1.8.4-HF128-Latest.exe'
    curl.exe -fsSL "https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe?verify=hf128-$Nonce" -o $Exe
    $Bytes=[System.IO.File]::ReadAllBytes($Exe)
    if ($Bytes[0] -ne 0x4D -or $Bytes[1] -ne 0x5A) { throw 'public Latest is not EXE' }
    $Actual=(Get-FileHash $Exe -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $m.sha256.ToLowerInvariant()) { throw 'public installer SHA mismatch' }
    if ((Get-Item $Exe).Length -ne [int64]$m.size) { throw 'public installer size mismatch' }

    $Tip=curl.exe -fsSL "https://download.qubit-coin.io/mainnet/snapshots/tip.json?verify=hf128-$Nonce" | ConvertFrom-Json
    if ([int]$Tip.height -lt 24000) { throw 'snapshot below #24000' }

    Write-Host 'HF128 PUBLIC RELEASE GATE: PASS'
}
```

---

# M. Clean-install and auto-update smoke

Install the public versioned installer on a clean test Windows profile/device, and update one existing v1.8.3/HF127 installation through the public manifest. Do not announce the release until the actual old client completes the whole download/install/relaunch path.

```text
1. QUB Core displays v1.8.4.
2. Existing wallets, chain-v2 state and pending transaction outbox survive the upgrade.
3. Fresh installation downloads canonical history and reaches the public version-2 tip.
4. `Check now` downloads into an operation-unique `.part`, verifies size, SHA256, ProductVersion and Authenticode/private-origin policy, then reaches Ready.
5. Install re-verifies the same properties immediately before launch. Installer exit 0 writes the restart marker only after success and relaunches v1.8.4. A simulated nonzero installer exit writes `data/updater/install-result.txt` and relaunches the old executable.
6. Turning Auto check/download/install off survives restart; HF128 never turns those preferences back on.
7. A temporary manifest with a bad SHA, oversize response, HTTP URL, cross-origin unsigned artifact or modified staged EXE must fail visibly and must not stop mining or launch anything. Restore the official URL afterwards.
8. CPU accepts 1–100%; GPU accepts 0–100%.
9. At 85% or above, or with an active manual override, solo/pool start remains blocked until the red session-only hardware-risk checkbox is accepted.
10. If a manual CPU override exceeds operating-system thread capacity, mining fails once, remains Off and does not continue with a partial plan or restart loop.
11. Changing CPU/GPU % while active waits for the old controller's explicit `Stopped` acknowledgement; no second OpenCL generation starts on a timer.
12. CPU/GPU H/s and session totals respond to the applied plan, do not retain stopped workers, and do not move backward except when a clearly new session resets to zero.
13. A new/empty-score pool bootstraps from a valid pending target share even when other pools fill the global share cap.
14. Pool mining confirms a default-QUB-fee Melt via `pool_compat_melt`, then resumes ordinary PPLNS on the next parent without `coinbase mismatch` or worker stop.
15. `block-relay-status`, `tx-delivery-status` and `relay-pending-txs` are available.
16. A locally found block is durable plus acknowledged, or durable plus automatic retry; it is never presented as publicly delivered from a socket write alone.
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

Finally, leave Core, Explorer and Jinex Wallet open together for at least 30 minutes. At every sample, compare Core tip, AMS3 API `/api/v1/status-fast`, public Explorer tip and Jinex Wallet gateway tip. Explorer/Wallet may trail by one publication interval, but must not remain frozen while both independent seeds advance. Record timestamps/heights; if both clients freeze at the same height again, stop the release and inspect the shared AMS3 publication/API/gateway path rather than changing consensus.

---

# N. GitHub repository, tag and release

Do not tag an unchanged repository. Mirror the clean HF128 source ZIP into the repository before commit/tag.

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference='Stop'

    $ProjectRoot='C:\Users\proes\Desktop\qub-node'
    $Repo=Join-Path $ProjectRoot 'qub-core-opensource'
    $Zip=Join-Path $ProjectRoot 'HF128-v184-source-no-assets-r3.zip'
    $Extract=Join-Path $ProjectRoot '_hf128_repo_extract'
    $Sync=Join-Path $ProjectRoot '_repo_sync_hf128'

    if (-not (Test-Path (Join-Path $Repo '.git'))) { throw "Repo missing: $Repo" }
    if (-not (Test-Path $Zip)) { throw "HF128 source ZIP missing: $Zip" }

    Set-Location $Repo
    if (@(git status --porcelain).Count -gt 0) { throw 'Repository has uncommitted changes.' }
    git fetch origin
    git checkout main
    git pull --ff-only origin main

    Remove-Item -Recurse -Force $Extract,$Sync -ErrorAction SilentlyContinue
    Expand-Archive -Path $Zip -DestinationPath $Extract -Force
    $SourceRoot=Join-Path $Extract 'qubd-v1.8.4'
    if (-not (Test-Path $SourceRoot)) { throw 'HF128 ZIP root missing.' }

    $ReadmeText=Get-Content (Join-Path $SourceRoot 'README.md') -Raw -Encoding UTF8
    $CargoText=Get-Content (Join-Path $SourceRoot 'Cargo.toml') -Raw -Encoding UTF8
    if (-not $ReadmeText.Contains('# QUB Core v1.8.4 HF128')) { throw 'HF128 README marker missing.' }
    if (-not $CargoText.Contains('version = "1.8.4"')) { throw 'Cargo version 1.8.4 missing.' }
    foreach ($Required in @('HF128-v184-DEPLOY-RUNBOOK.md','HF128-v184-RELIABILITY-REVIEW.md','RELEASE_NOTES-v1.8.4-HF128.md','deploy\digitalocean\mainnet-seed-ams3.toml','deploy\digitalocean\mainnet-seed-nyc3.toml','scripts\test-hf127-tx-delivery-regtest.py')) {
        if (-not (Test-Path (Join-Path $SourceRoot $Required))) { throw "Required HF128 source file missing: $Required" }
    }

    $SafeEnvTemplates=@('.env.example','.env.sample','.env.template')
    $Forbidden=@(Get-ChildItem $SourceRoot -Recurse -Force -File | Where-Object {
        $_.FullName -notmatch '\\target\\|\\dist\\|\\.git\\|\\__pycache__\\' -and (
            $_.Name -in @('wallet.json','ethereum-wallets.json','wallet-pending-txs.json','chain.json','chain-status.json','pending-block-relay.json','CURRENT.json','PREVIOUS.json','WRITE.lock','.env') -or
            ($_.Name -like '.env.*' -and $_.Name -notin $SafeEnvTemplates) -or
            $_.Extension -in @('.exe','.dll','.pdb','.ilk','.zip','.tar','.gz','.7z')
        )
    })
    if ($Forbidden.Count -gt 0) { $Forbidden | Select-Object FullName; throw 'Forbidden runtime/private/build files exist in canonical HF128 source.' }

    $AssetFiles=@(Get-ChildItem (Join-Path $SourceRoot 'assets') -Recurse -Force -File)
    if ($AssetFiles.Count -ne 1 -or $AssetFiles[0].Name -ne 'README.md') { $AssetFiles | Select-Object FullName; throw 'HF128 source archive is not no-assets.' }

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
    if ($Changes.Count -eq 0) { throw 'Repository has no HF128 changes. Do not create the tag.' }

    git add -A
    git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
    git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh
    git diff --cached --check
    git diff --cached --stat
    $Staged=@(git diff --cached --name-status)
    if ($Staged.Count -eq 0) { throw 'Nothing staged.' }
    if (-not ($Staged -match 'README\.md')) { throw 'HF128 README is not staged.' }
    if (-not ($Staged -match 'RELEASE_NOTES-v1\.8\.4-HF128\.md')) { throw 'HF128 release notes are not staged.' }
    if (-not ($Staged -match 'mainnet-seed-ams3\.toml')) { throw 'AMS3 regional seed config is not staged.' }
    if (-not ($Staged -match 'mainnet-seed-nyc3\.toml')) { throw 'NYC3 regional seed config is not staged.' }

    git commit -m 'Update QUB Core to v1.8.4 HF128'
    $Commit=(git rev-parse HEAD).Trim()
    git push origin main
    git fetch origin main
    if ((git rev-parse origin/main).Trim() -ne $Commit) { throw 'Remote main mismatch.' }

    if (git tag -l 'v1.8.4-hf128') { throw 'Tag v1.8.4-hf128 already exists.' }
    git tag -a v1.8.4-hf128 -m 'QUB Core v1.8.4 HF128' $Commit
    git push origin v1.8.4-hf128

    Write-Host 'HF128 GITHUB SOURCE/TAG GATE: PASS'
}
```

Create the GitHub Release from tag `v1.8.4-hf128` with title:

```text
QUB Core v1.8.4 HF128 - Mining, Pool, Sync & Updater Reliability
```

Use `RELEASE_NOTES-v1.8.4-HF128.md` as the release body and attach the source ZIP, its SHA256SUMS file, the versioned Windows installer and installer sidecar. If `gh` is available:

```powershell
gh release create v1.8.4-hf128 `
  "C:\Users\proes\Desktop\qub-node\HF128-v184-source-no-assets-r3.zip" `
  "C:\Users\proes\Desktop\qub-node\HF128-v184-r3-SHA256SUMS.txt" `
  ".\dist\installer\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe" `
  ".\dist\updates\mainnet\windows-x64\QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe.sha256" `
  --repo AlxProe/qub-core `
  --title "QUB Core v1.8.4 HF128 - Mining, Pool, Sync & Updater Reliability" `
  --notes-file ".\RELEASE_NOTES-v1.8.4-HF128.md"
```

Do not create or move a tag until the source commit exists on `origin/main`.


# O. Telegram announcement

```text
🆕 QUB Core v1.8.4 HF128 is live.

🔽 Download:
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.4-Windows-x64-mainnet-Setup.exe

🧑‍💻 Source:
https://github.com/AlxProe/qub-core/releases/tag/v1.8.4-hf128

HF128 is a reliability update with no chain upgrade, no new activation height and no protocol/block-version change.

⛏ Mining fixes
- CPU/GPU % changes now apply to the live mining plan
- GPU % is real interruptible duty control; 100% has no QUB duty sleep
- GPU auto-tuning can no longer reuse an incompatible low-power batch at 100%
- CPU 100% restores the full automatic 2x-logical nonce-lane plan
- H/s and session totals are measured/reset consistently
- Plan changes wait for the old CPU/GPU controller to stop—no overlapping OpenCL generations

🏊 Pool + Melt liveness
- New/empty pools bootstrap deterministically from a valid pending share
- Pool mining can confirm default-fee QUB Melt transactions without the old coinbase mismatch
- Ordinary PPLNS resumes automatically on the next parent
- Consensus validators and pool rules remain unchanged

🌐 Sync / seeds / Explorer
- One staged suffix/overlap sync instead of duplicate from-genesis request floods
- Manual Sync/Repair queues safely behind automatic catch-up
- AMS3 and NYC3 use independent regional configs and node identities
- Duplicate seed identity can no longer count as independent evidence
- Explorer API chain snapshot + Fast Chain Engine identity are loaded atomically

⬆ Updater
- Bounded downloads with visible failures and retry
- Mandatory size + SHA256 verification and pre-install re-verification
- Atomic staging; manifest is published last
- Failed installer relaunches the previous Core and reports the error
- Your Auto update checkbox choices are preserved

HF123–HF127 remain included. Protocol Epoch 2 stays at #24000; block version 2, DAA, rewards, checkpoints, genesis, QUB/JIN economics, pool consensus, P2P protocol 2 and QUB-FCE-1 are unchanged.

After installation, verify that QUB Core shows v1.8.4.
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
echo "node_id=$(cat /opt/qub/mainnet/data/node_id)"
curl -fsS --max-time 20 http://127.0.0.1:18765/api/v1/status-fast
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
echo "node_id=$(cat /opt/qub/mainnet/data/node_id)"
free -h
cat /proc/swaps || true
sudo journalctl -u qub-seed-mainnet.service -n 160 --no-pager
'@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
```

For the first locally sent transaction and locally found post-HF128 block, record:

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

---

# Q. Rollback (only if a post-deploy gate fails)

HF128 introduces no consensus activation, so binary rollback is wire/chain safe. Never roll back `chain-v2/`, wallets, pending outboxes or `node_id`. First restore the previous public manifest so additional clients stop receiving HF128, then roll back NYC3, then AMS3. Use the exact timestamped paths printed/created during sections D/E/K—do not use a wildcard.

Set these to exact existing paths:

```powershell
$NycOldBin='/opt/qub/bin/backups/qubd.backup-before-v1.8.4-hf128-YYYYMMDD-HHMMSS'
$NycOldCfg='/opt/qub/mainnet/mainnet-seed.toml.backup-before-v1.8.4-hf128-YYYYMMDD-HHMMSS'
$AmsOldBin='/opt/qub/bin/backups/qubd.backup-before-v1.8.4-hf128-YYYYMMDD-HHMMSS'
$AmsOldCfg='/opt/qub/mainnet/mainnet-seed.toml.backup-before-v1.8.4-hf128-YYYYMMDD-HHMMSS'
$UpdateBackup='/srv/qub-updates/mainnet/windows-x64.backup-before-v1.8.4-hf128-YYYYMMDD-HHMMSS'
```

Restore the update channel first:

```powershell
@"
set -euo pipefail
test -d '$UpdateBackup'
sudo rsync -a '$UpdateBackup/' /srv/qub-updates/mainnet/windows-x64/
python3 -m json.tool /srv/qub-updates/mainnet/windows-x64/manifest.json >/dev/null
echo 'UPDATE CHANNEL ROLLBACK: PASS'
"@ | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
```

Rollback NYC3 canary:

```powershell
@"
set -euo pipefail
test -f '$NycOldBin'
test -f '$NycOldCfg'
sudo systemctl stop qub-seed-mainnet.service
sudo install -m 0755 -o root -g root '$NycOldBin' /opt/qub/bin/qubd
sudo install -m 0644 -o root -g root '$NycOldCfg' /opt/qub/mainnet/mainnet-seed.toml
sudo systemctl start qub-seed-mainnet.service
systemctl is-active qub-seed-mainnet.service
sudo ss -ltnp | grep ':17444'
/opt/qub/bin/qubd --config /opt/qub/mainnet/mainnet-seed.toml status-fast
"@ | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
```

After NYC3 is healthy, run the same explicit restore on AMS3 using `$AmsOldBin` and `$AmsOldCfg`, while also stopping/restarting the snapshot timer around the seed restart. Restore the Explorer API from the exact `$API_BIN.backup-...` and drop-in backup created in section F if its gate was the failure. Re-publish one snapshot only after AMS3 and NYC3 again report the same cumulative-work-selected tip with different node IDs.

Rollback is complete only when the public manifest points to the previous release, both seed listeners are healthy, RPC 17445 is not public, the Explorer API is read-only/healthy, the snapshot timer is active, and Core/Explorer/Wallet tips advance again.
