# HF131 / QUB Core v1.8.5 deployment runbook

Release preparation: HF131 r4, activation-aware mainnet preflight. Baseline: deployed HF128/v1.8.4.
Product remains v1.8.5. Protocol epoch 2, block version 2, activation #24000,
genesis, difficulty rules, rewards, pool/share rules and FCE format are unchanged.
Explorer 0.7.3 stays deployed. Jinex Wallet is not deployed and is not a gate.

Use this runbook only with these exact downloaded files in
`C:\Users\proes\Desktop\qub-node`:

- HF131-v185-source-no-assets-r4.zip
- HF131-v185-r4-SHA256SUMS.txt
- HF131-v185-PREPARE-r4.ps1
- HF131-v185-DEPLOY-RUNBOOK-r4.md

The preparation script pins this ZIP's exact SHA256. Never rename an HF130 ZIP
to these filenames. Keep previous downloads, working sources and lab data.
This setup uses a new source directory and does not need any old directory to
move. After the r3 height-zero preflight failure, start this r4 Gate 0 then
Gate 1. The old Cargo cache stays in place; previous failure logs remain in their
old evidence path. r4 corrects only the CLI reserve preflight and adds six
internal tests. All other Rust, Cargo files and configs are byte-identical to r3.
The actual JIN ledger, bootstrap, mining and chain validation are unchanged.

The route is: internal/native tests -> isolated GUI mainnet smoke -> NYC3 then
AMS3 seeds/API -> package dist -> test a copied portable bundle -> public dist
-> GitHub -> Telegram. Compiling test executables necessarily precedes GUI smoke;
installer/distribution packaging comes after the seed checks.

There is no operator regtest/LAN lab, exact-gap exercise, or six-hour release
soak. Normal unit tests retain isolated fixture networks internally; they start
no manual lab and use no real mainnet wallet. Keep post-release monitoring.

Use PowerShell 5.1 or newer, Python 3.11+, the existing Windows Rust/MSVC setup,
OpenSSH, Git, and Inno Setup. No Bash or WSL is needed on Windows. Linux Bash
checks run on the seeds. Do not run cargo fmt over this archived source: it
changes its manifest bytes. Native checks and tests are mandatory.

Copy each complete PowerShell block, including its outer `. {` or `& {`.
A red error/nonzero exit means stop that gate; do not paste later blocks.
Prefer the same PowerShell. In a new one, rerun Gate 0 with `-Resume` as shown
below; it restores paths and verifies existing source without copying it again.
Record manual smoke results before marking their gates complete. Code blocks
alone cannot prove GPU behavior, GUI actions or the public updater.

Seeds are discovery/relay peers, not consensus authorities. HF131 gives healthy
direct peers time to answer even when seeds time out. The existing independent
peer proof and delivery rules remain: an isolated miner cannot prove that its
private tip is the network tip. Do not stop both production seeds to simulate
an outage; the internal fault tests cover that scheduling condition.

## Gate 0 - prepare a separate source directory

Close the old lab GUI and enter `quit` in its helper console. Close any QUB GUI,
qubd, RPC miner or Cargo build using this project's target cache before Gate 1.
Do not delete lock files, kill unrelated processes or move old data.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    $Prepare = Join-Path $ProjectRoot 'HF131-v185-PREPARE-r4.ps1'
    $Sums = Join-Path $ProjectRoot 'HF131-v185-r4-SHA256SUMS.txt'
    foreach ($File in @($Prepare, $Sums)) {
        if (-not (Test-Path -LiteralPath $File -PathType Leaf)) { throw "Missing r4 file: $File" }
    }
    $PrepareLines = @(Get-Content -LiteralPath $Sums -Encoding ASCII | Where-Object {
        $_ -match '^[0-9a-f]{64}  HF131-v185-PREPARE-r4\.ps1$'
    })
    if ($PrepareLines.Count -ne 1) { throw 'Missing or duplicate r4 PREPARE checksum' }
    $PrepareHash = (Get-FileHash -LiteralPath $Prepare -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($PrepareLines[0] -cne "$PrepareHash  HF131-v185-PREPARE-r4.ps1") { throw 'Wrong r4 PREPARE bytes' }
    if (@('RemoteSigned', 'Unrestricted', 'Bypass') -notcontains [string](Get-ExecutionPolicy)) {
        Get-ExecutionPolicy -List | Out-Host
        throw 'Current policy requires approved signing or administrator review; no policy was changed'
    }
    Unblock-File -LiteralPath $Prepare
    . $Prepare
}
```

This checks PREPARE against the r4 sums before removing that one downloaded
file's Internet-zone marker. It does not change any execution policy. AllSigned,
Restricted or a stricter effective organization policy stops this block.

This copies checked local assets from the existing `qubd-v1.8.5\assets` into
`sources\HF131-v185-<ZIP hash prefix>\qubd-v1.8.5`. PNG, MP3, MP4 and other
passive local assets remain supported. It preserves the archive README, rejects
private keys/scripts/state/binaries, and never exports these runtime assets.
The old Cargo target cache is reused in place, without deletion or renaming.

For a new PowerShell later, use this instead of repeating finished gates:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    . (Join-Path $ProjectRoot 'HF131-v185-PREPARE-r4.ps1') -Resume
}
```

## Gate 1 - one native build and preflight receipt

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if (-not $Source -or -not $NativeReceipt) { throw 'Run Gate 0 first' }
    $Busy = @(Get-CimInstance Win32_Process | Where-Object {
        $_.ExecutablePath -and $_.ExecutablePath.StartsWith($env:CARGO_TARGET_DIR, [StringComparison]::OrdinalIgnoreCase)
    })
    if ($Busy.Count) { $Busy | Select-Object ProcessId, Name, ExecutablePath; throw 'Close these project processes before building; no process was killed' }
    foreach ($Check in @('test-hf131-package-policy.py','test-hf131-prepare.py','test-hf131-native-build.py','test-hf131-mainnet-smoke.py','test-hf131-public-state.py')) {
        python (Join-Path $Source ('scripts\' + $Check))
        if ($LASTEXITCODE) { throw "Internal helper test failed: $Check" }
    }
    $NativeAction = 'build'
    if (Test-Path -LiteralPath $NativeReceipt -PathType Leaf) { $NativeAction = 'verify' }
    python (Join-Path $Source 'scripts\hf131-native-build.py') $NativeAction --source $Source --receipt $NativeReceipt
    if ($LASTEXITCODE) { throw 'HF131 native build/evidence failed' }
    Write-Host 'HF131 INTERNAL / NATIVE / PREFLIGHT GATE: PASS'
}
```

This runs the full locked native suite, the exact HF130+HF131 regression
registry, builds all three release executables, and runs disposable mainnet
preflight. Missing/zero/ignored required tests fail. Successful evidence binds
source, Cargo.lock, toolchain, flags, logo and binary hashes plus phase logs.
A repeat verifies that receipt. Packaging reuses it and still performs shipping
preflight. If inputs change, build a new candidate; never edit a receipt to pass.

The disposable preflight starts at mainnet genesis (#0). Its JIN reserve check
now reports `ok: true, applicable: false` before native JIN activation #5555.
No reserve exists at that stage. From #5555 through #16776 the check requires
the actual 42,000,000 JIN protocol reserve; from #16777 it requires replayed
bootstrap history. Unreadable or insufficient applicable state still fails.
This diagnostic also runs during dist packaging. A genesis preflight PASS is
configuration evidence, not evidence of present mainnet synchronization.
The subsequent isolated mainnet GUI smoke still checks a real chain copy.

## Gate 2 - isolated local GUI mainnet smoke

Close every application writing your existing mainnet data before the copy.
The known path below is your existing mainnet installation, not the old regtest
lab. If your current data moved, change only `$MainnetData` to its actual path.
The helper copies FCE and wallet keys into a new PRIVATE local smoke directory;
those keys are needed to sign pool shares. Never upload the smoke directory.
It does not move or alter your original wallet/chain, and it does not start mining.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $MainnetData = 'C:\Users\proes\Desktop\Qubit Coin Core\data\mainnet'
    if (-not (Test-Path -LiteralPath $SmokeLab)) {
        python (Join-Path $Source 'scripts\hf131-mainnet-smoke.py') prepare --source $Source --data-source $MainnetData --lab $SmokeLab
        if ($LASTEXITCODE) { throw 'Isolated mainnet smoke preparation failed' }
    }
    python (Join-Path $Source 'scripts\hf131-mainnet-smoke.py') launch --lab $SmokeLab --gui (Join-Path $NativeDir 'qub-core.exe') --qubd (Join-Path $NativeDir 'qubd.exe') --receipt $NativeReceipt
    if ($LASTEXITCODE) { throw 'Mainnet GUI launch/validation failed' }
}
```

Check Settings > Config file points to the isolated smoke config and the title
says mainnet. The helper disables automatic update/install and mining autostart.
Keep the original miner closed while using this wallet copy. Mainnet mining
and pool shares are real; use your own payout, not the old `rqub` lab address.
Do not send funds, create pools, import keys or change wallet accounts for smoke.

An old copied height such as #44423 is valid historical data; it is not evidence
of present synchronization. Let Sync catch up before mining. Compare full tip
hash and height to both seeds/API after normal propagation. A high block number
alone is insufficient. If Sync remains stuck, retain logs and stop this release.

Perform this short observed matrix, normally 1-2 minutes per mining mode after
sync (no fixed number of blocks to mine):

| Action | Required observation |
|---|---|
| Sync | Current chain and wallet refresh; no stale capture rollback or endless queued target |
| Solo CPU | Correct payout, active work/hashrate; Stop stops and Start resumes |
| Solo GPU | Correct selected device, stable work/hashrate; repeated Stop/Start succeeds |
| Pool CPU | Existing pool selected, owned payout, share work and mining progress |
| Pool GPU | Pool/share work progresses and GPU work starts; repeated Stop/Start succeeds |
| Power/reconfigure | Low/high CPU and GPU settings take effect; no duplicate controller |
| New network tip | Work refreshes to the new parent; submitted accepted blocks retain relay evidence |

Pool share preparation can temporarily show no GPU block work; record the
phase/log, not just one hashrate sample. Do not require a found block in a fixed
time window. If no candidate is found, explicitly record block submission as
not observed; native tests cover the worker handoff but do not prove your driver.
Record mode, payout, device, observed tip, Stop/Start and any accepted/share logs
in `$Evidence\mainnet-smoke-notes.txt`. Then close the smoke GUI before seeds.

## Gate 3 - stage and build both seeds

Use the established infrastructure: NYC3 `deploy@167.99.57.45`, AMS3
`deploy@159.223.222.103`, source `/opt/jinex/staging/src/qubd`, binary
`/opt/qub/bin/qubd`, config `/opt/qub/mainnet/mainnet-seed.toml`.
Each host keeps its own tested binary hash. Never require both build hashes to
match across different compiler/linker environments.

Export only the manifest-listed source bytes. The exporter validates the local
source and excludes local assets, target outputs and checkout metadata. Its
destination must be new; it never cleans an existing working tree.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $StageId = [guid]::NewGuid().ToString('N')
    $Deploy = Join-Path $ProjectRoot ('_hf131_seed_source_' + $StageId)
    $Tar = Join-Path $ProjectRoot ('qubd-hf131-v185-source-' + $StageId + '.tar.gz')
    if (-not (Test-Path $Key -PathType Leaf)) { throw "SSH key missing: $Key" }
    python (Join-Path $Source 'scripts\export-hf131-source.py') --source $Source --destination $Deploy
    if ($LASTEXITCODE) { throw 'Exact HF131 source export failed' }
    python (Join-Path $Deploy 'scripts\test-hf131-source-contracts.py') --mode archive --root $Deploy
    if ($LASTEXITCODE) { throw 'Seed export archive contract failed' }
    tar.exe -czf $Tar -C $Deploy .
    if ($LASTEXITCODE) { throw 'Seed source tar creation failed' }
    $TarHash = (Get-FileHash -LiteralPath $Tar -Algorithm SHA256).Hash.ToLowerInvariant()
    scp.exe -i $Key $Tar "${NYC3}:/tmp/qubd-hf131-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'NYC3 source upload failed' }
    scp.exe -i $Key $Tar "${AMS3}:/tmp/qubd-hf131-v185-source.tar.gz"
    if ($LASTEXITCODE) { throw 'AMS3 source upload failed' }
    "Seed tar SHA256: $TarHash"
    Write-Host 'HF131 CLEAN SEED SOURCE UPLOAD GATE: PASS'
}
```

Build and test the exact uploaded tar on both established staging trees:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $SeedBuild = @'
set -euo pipefail

if [ -f "$HOME/.cargo/env" ]; then . "$HOME/.cargo/env"; fi
export PATH="$HOME/.cargo/bin:$PATH"
export QUB_BUILD_CONFIG=mainnet
rustc --version --verbose
cargo --version

SRC=/opt/jinex/staging/src/qubd
ARCHIVE=/tmp/qubd-hf131-v185-source.tar.gz
EXPECTED_HASH=__EXPECTED_TAR_HASH__
CFG=/opt/qub/mainnet/mainnet-seed.toml

test "$(sha256sum "$ARCHIVE" | awk '{print $1}')" = "$EXPECTED_HASH"
CACHE=
if [ -e "$SRC" ]; then
    PREVIOUS="$SRC.before-hf131-$(date +%Y%m%d-%H%M%S)-$$"
    sudo mv "$SRC" "$PREVIOUS"
    if [ -d "$PREVIOUS/target" ]; then CACHE=$(readlink -f "$PREVIOUS/target"); fi
fi
sudo install -d -m0755 -o deploy -g deploy "$SRC"
tar -xzf "$ARCHIVE" -C "$SRC"
cd "$SRC"
chmod 0755 deploy/digitalocean/*.sh

python3 scripts/test-hf131-source-contracts.py --mode archive --root "$SRC"
if [ -n "$CACHE" ]; then ln -s "$CACHE" "$SRC/target"; fi
bash deploy/digitalocean/test-publish-mainnet-snapshot.sh
python3 -B deploy/digitalocean/test-hf131-deployment.py
cargo test --locked --lib --bin qubd
python3 scripts/test-hf131-native-tests.py --scope seed
cargo build --locked --release --bin qubd
LC_ALL=C grep -aFq '/QUB Core:1.8.5/' target/release/qubd
sha256sum target/release/qubd

timeout --signal=TERM --kill-after=20s 300s \
    target/release/qubd --config "$CFG" status-fast > /tmp/hf131-staged-status.json
python3 -I - <<'PY'
import json
s=json.load(open('/tmp/hf131-staged-status.json'))
assert s['ok'] is True and s['network']=='mainnet'
assert s['version']=='1.8.5' and int(s['height'])>=24000
assert int(s['tip_block_version'])==2 and int(s['next_block_expected_version'])==2
print(s['height'],s['tip_hash'])
PY
'@
    $SeedBuild = $SeedBuild.Replace('__EXPECTED_TAR_HASH__', $TarHash)

    $SeedBuild | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF131 build gate failed' }
    $SeedBuild | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF131 build gate failed' }

    $NycBuildHash = (ssh.exe -i $Key $NYC3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'NYC3 staged binary hash read failed' }
    $AmsBuildHash = (ssh.exe -i $Key $AMS3 "sha256sum /opt/jinex/staging/src/qubd/target/release/qubd | awk '{print `$1}'").Trim()
    if ($LASTEXITCODE) { throw 'AMS3 staged binary hash read failed' }
    if ($NycBuildHash -notmatch '^[0-9a-f]{64}$' -or $AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Invalid remote build hash' }
    "NYC3 tested Linux qubd SHA256: $NycBuildHash"
    "AMS3 tested Linux qubd SHA256: $AmsBuildHash"
    @{ nyc3 = $NycBuildHash; ams3 = $AmsBuildHash } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Evidence 'seed-builds.json') -Encoding ASCII
    Write-Host 'HF131 PER-HOST VERIFIED SEED BUILD GATE: PASS'
}
```

Both hosts build the same manifest-verified source and pass their own native
tests. Each installation must match that host's recorded staged binary hash;
the helper also checks the running `/proc/PID/exe` bytes. As in HF128, the hosts
do not need identical compiler/linker environments. Equality between their
binary hashes is not claimed as a reproducibility result. Preserve both build
logs, including the printed Rust/Cargo versions and each binary hash.

Verify seed identities are non-empty and different:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsId = (ssh.exe -i $Key $AMS3 'sudo cat /opt/qub/mainnet/data/node_id').Trim()
    if ($LASTEXITCODE) { throw 'AMS3 node_id read failed' }
    $NycId = (ssh.exe -i $Key $NYC3 'sudo cat /opt/qub/mainnet/data/node_id').Trim()
    if ($LASTEXITCODE) { throw 'NYC3 node_id read failed' }
    if ($AmsId -notmatch '^[0-9a-fA-F]{16,}$' -or $NycId -notmatch '^[0-9a-fA-F]{16,}$') { throw 'Invalid seed identity' }
    if ($AmsId -eq $NycId) { throw 'STOP: duplicated AMS3/NYC3 node_id' }
    Write-Host 'HF131 DISTINCT SEED IDENTITY GATE: PASS'
}
```

Do not regenerate identity as part of an ordinary release. If duplicated, use
the explicit HF128 identity-recovery procedure with backups and only NYC3
offline, then repeat every gate.

## Gate 4 - NYC3 canary, then AMS3 and Explorer API

Keep AMS3 serving. The included installer validates the staged source inventory
and exact binary hash, records binary/config/identity backups, and stops each
active QUB service with a checked deadline before replacing its binary. It
restarts testnet only if testnet was active before the operation. Failure
returns nonzero and attempts recovery; inspect that recovery before proceeding.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($NycBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 3 in this session first' }
    $NycDeploy = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf131-seed.sh nyc3 __EXPECTED_BINARY_SHA256__
'@
    $NycDeploy = $NycDeploy.Replace('__EXPECTED_BINARY_SHA256__', $NycBuildHash)
    $NycDeploy | ssh.exe -i $Key $NYC3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'NYC3 HF131 canary install failed; inspect recovery output' }
    Write-Host 'HF131 NYC3 CANARY INSTALL GATE: PASS'
}
```

Record the exact backup path printed by the installer. Keep AMS3 serving.
Observe NYC3 for 5-10 minutes while it receives normal mainnet traffic. Compare
its height AND hash with AMS3. Do not require five newly mined blocks on a quiet
network. If no new block occurs, record that limitation and confirm matching
current tips plus active P2P; do not claim observed new-block relay.

Use the following read-only check before moving on. A propagation race may
produce a temporary mismatch; repeat this block until both observations match.
Persistent divergence is a stop condition. The installer already verifies the
running binary hash, owned P2P listener and closed public RPC.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $Tips = @()
    foreach ($HostName in @($NYC3,$AMS3)) {
        $Text = ssh.exe -i $Key $HostName 'timeout 60s /opt/qub/bin/qubd --config /opt/qub/mainnet/mainnet-seed.toml status-fast'
        if ($LASTEXITCODE) { throw "Seed status failed: $HostName" }
        $Tip = ($Text -join "`n") | ConvertFrom-Json
        if (-not $Tip.ok -or $Tip.network -ne 'mainnet' -or $Tip.tip_block_version -ne 2 -or $Tip.next_block_expected_version -ne 2) { throw "Invalid seed status: $HostName" }
        $Tips += $Tip
        $Tip | Select-Object network,version,height,tip_hash
    }
    if ($Tips[0].height -ne $Tips[1].height -or $Tips[0].tip_hash -ne $Tips[1].tip_hash) { throw 'Seed tips differ: allow propagation, inspect and repeat before AMS3' }
    Write-Host 'HF131 SEED TIP COMPARISON: PASS'
}
```

### AMS3 seed, publisher and separate Explorer API

### Install AMS3 seed and publisher units

Only after the NYC3 canary passes. Check free disk space before copying the
snapshot backup or starting migration: retained generations, the new export,
staging, migration and backups coexist. Allow at least five current full-snapshot
sizes of free margin plus journal growth and the actual backup size. Measure
allocated bytes on this host; this estimate is not a universal capacity proof.
The first HF131 run imports existing public bytes. A mismatch between legacy
`canonical-chain.json` and `snapshots/chain.json` fails closed for diagnosis.
Do not delete or replace one of them to bypass that check.

The installer stops the publisher timer and waits for the publisher to stop.
It copies the entire existing snapshot tree with `cp -a`, preserving symlinks
and hidden generation directories, alongside exact binary/config/unit backups.
It installs the same checked binary separately as
`/opt/qub/bin/qubd-snapshot-export-hf131`; the publisher unit uses this versioned
exporter, which must remain available even if the seed later rolls back to
HF128. HF128's exporter does not emit the report fields required by HF131.

The publisher runs as `deploy`. The helper records and sets ownership/mode only
on `/srv/qub-updates/mainnet` and its `snapshots` directory, with no recursive
ownership change. The parent must be writable to replace canonical aliases.
It checks read access to FCE CURRENT before stopping the seed, preserves the
original directory metadata in the backup, and restores it if installation fails.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 3 in this session first' }
    $AmsDeploy = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf131-seed.sh ams3 __EXPECTED_BINARY_SHA256__
'@
    $AmsDeploy = $AmsDeploy.Replace('__EXPECTED_BINARY_SHA256__', $AmsBuildHash)
    $AmsDeploy | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 HF131 install failed; inspect recovery output' }
    Write-Host 'HF131 AMS3 SEED / PUBLISHER UNIT INSTALL GATE: PASS'
}
```

Record the exact `/opt/qub/bin/backups/hf131-ams3-*` directory. The snapshot timer
stays stopped until the following first-publication check. No state file,
wallet or node identity is rolled back by the installer.

Repeat the NYC3 status/version/hash/listener checks. Require AMS3 and NYC3 have
different `node_id`, independent regional configs and exactly the same selected
height/hash after allowing normal network propagation before enabling publication.

Verify the installed unit definitions are exactly the reviewed files:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsUnitVerify = @'
set -euo pipefail
systemctl cat qub-mainnet-snapshot-publish.service
systemctl cat qub-mainnet-snapshot-publish.timer
cmp /opt/jinex/staging/src/qubd/deploy/digitalocean/qub-mainnet-snapshot-publish-hf131.service /etc/systemd/system/qub-mainnet-snapshot-publish.service
cmp /opt/jinex/staging/src/qubd/deploy/digitalocean/qub-mainnet-snapshot-publish-hf131.timer /etc/systemd/system/qub-mainnet-snapshot-publish.timer
cmp /opt/jinex/staging/src/qubd/target/release/qubd /opt/qub/bin/qubd-snapshot-export-hf131
sha256sum /etc/systemd/system/qub-mainnet-snapshot-publish.{service,timer}
systemd-analyze verify /etc/systemd/system/qub-mainnet-snapshot-publish.service /etc/systemd/system/qub-mainnet-snapshot-publish.timer
'@
    $AmsUnitVerify | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 publisher unit verification failed' }
    Write-Host 'HF131 AMS3 PUBLISHER UNIT VERIFICATION: PASS'
}
```

Start publication only through systemd so it uses the same user,
environment and `/run/qub-snapshot-publisher/publish.lock` as the timer. Never
direct-run the script with its different default `/tmp` lock while the timer may
run:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsPublisherStart = @'
set -euo pipefail
PreviousGeneration=$(readlink /srv/qub-updates/mainnet/snapshots/.current || true)
sudo systemctl start qub-mainnet-snapshot-publish.service
systemctl show qub-mainnet-snapshot-publish.service -p Result -p ExecMainStatus -p ActiveEnterTimestamp -p InactiveEnterTimestamp
[ "$(systemctl show -p Result --value qub-mainnet-snapshot-publish.service)" = success ]
[ "$(systemctl show -p ExecMainStatus --value qub-mainnet-snapshot-publish.service)" = 0 ]
PublishedGeneration=$(readlink /srv/qub-updates/mainnet/snapshots/.current)
[ -n "$PublishedGeneration" ] && [ "$PublishedGeneration" != "$PreviousGeneration" ]
journalctl -u qub-mainnet-snapshot-publish.service -n 200 --no-pager
'@
    $AmsPublisherStart | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 first publication/timer start failed' }
    Write-Host 'HF131 AMS3 FIRST PUBLICATION: PASS; TIMER REMAINS STOPPED'
}
```

The publisher switches `.current` once per complete immutable generation.
The fixed public URLs and JSON formats remain available to HF128. Multiple
HTTP requests can cross a switch: compare tip identity and checksums and retry
a bounded number of times. Do not call one symlink rename a multi-request
transaction. Keep `.current`, `.generations` and the fixed symlinks intact.

Before public Windows publication, observe two consecutive successful publisher
cycles; continue observing after release. Require:

- `Result=success`, `ExecMainStatus=0`, no lock-contention/skip line;
- each complete cycle is below 115 seconds;
- `tip.json.source == "fce"`, `recovered_from_previous == false`;
- `tip.json` height/hash exactly equals the exporter report logged by that
  publication invocation; compare live `status-fast` separately with allowed
  propagation/publication lag, since blocks can arrive between captures;
- public `published_at_unix` age always stays below 180 seconds;
- snapshot height/hash converges to AMS3 FCE within two configured cycles;
- service last/next trigger changes and timer remains active.

The 20-minute service timeout is only a kill bound. A repeated cycle over 115
seconds is NO-GO because Core stops treating the HTTP tip as fresh before the
next run.

### Caddy cache policy

Preserve the existing working HF128 Caddy configuration and Unix socket routes.
Run the header check below first. If the existing rules already provide the
required no-store headers, no Caddy edit is needed. If they do not, back up the
actual active config identified by `systemctl cat caddy`, merge only the snapshot
cache rules from `deploy/digitalocean/Caddyfile.qub-updates`, validate the candidate
with `caddy validate --config`, then install/reload it with the exact backup ready.
Do not replace the live config with that partial route template. Preserve API,
Explorer and TLS routes. A failed validation/reload is a stop condition.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsCaddyHeaders = @'
set -euo pipefail
# The established HF128 origin check permits the origin certificate on this
# pinned loopback connection only. No redirects or external insecure requests.
curl --noproxy '*' --connect-timeout 5 --max-time 15 -kfsSI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tip.json
curl --noproxy '*' --connect-timeout 5 --max-time 15 -kfsSI --resolve download.qubit-coin.io:443:127.0.0.1 https://download.qubit-coin.io/mainnet/snapshots/tail-64.json
'@
    $AmsCaddyHeaders | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Caddy snapshot header request failed' }
}
```

`tip.json`, every tail, chain and canonical-chain URL and their sidecars must
be `no-store, max-age=0`; none may also receive bulk max-age 60. The loopback
diagnostic preserves HF128's origin-certificate handling. Public checks must
use normal certificate verification (never `-k`) and the ordinary URLs without
a query parameter. Record `Cache-Control`, `Age`, `CF-Cache-Status` and response
height/hash. A cache-busted URL is additional diagnostic evidence only.

### Separate versioned Explorer API binary

Do **not** run `deploy/digitalocean/install-explorer-api-service.sh`: it builds
without the reviewed staged artifact and overwrites shared `/opt/qub/bin/qubd`.

Install the already-tested binary as
`/opt/qub/bin/qubd-explorer-api-hf131`, and override only the Explorer API unit.
The helper records a timestamped backup of the previous drop-in and any existing
versioned API binary, verifies the running API hash and unchanged seed PID/hash,
and attempts to restore the previous API configuration if installation fails.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($AmsBuildHash -notmatch '^[0-9a-f]{64}$') { throw 'Run Gate 3 in this session first' }
    $AmsApiInstall = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/install-hf131-explorer-api.sh __EXPECTED_BINARY_SHA256__
'@
    $AmsApiInstall = $AmsApiInstall.Replace('__EXPECTED_BINARY_SHA256__', $AmsBuildHash)
    $AmsApiInstall | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 API install failed; inspect recovery output' }
    Write-Host 'HF131 AMS3 VERSIONED EXPLORER API INSTALL: PASS'
}
```

Gate the root and status separately-the old runbook incorrectly required
`version` from an older status schema:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsApiGate = @'
set -euo pipefail
systemctl is-active --quiet qub-explorer-api-caddy.socket
test -S /srv/qub-updates/run/qub-explorer-api.sock
curl --fail --silent --show-error --connect-timeout 2 --max-time 10 --unix-socket /srv/qub-updates/run/qub-explorer-api.sock http://localhost/api/v1/status-fast >/tmp/hf131-api-bridge-status.json
systemctl is-active --quiet qub-explorer-api-caddy.service
timeout --signal=TERM --kill-after=5s 45s python3 -I - <<'PYAPI'
import json,time,urllib.request
base='http://127.0.0.1:18765/api/v1/'
bridge=json.load(open('/tmp/hf131-api-bridge-status.json'))
assert bridge['ok'] is True and bridge['network']=='mainnet' and bridge['version']=='1.8.5'
def get(route):
    started=time.monotonic()
    with urllib.request.urlopen(base+route,timeout=2) as response:
        value=json.load(response)
        if base.startswith('https:'): assert 'no-store' in response.headers.get('Cache-Control','').lower()
    assert time.monotonic()-started<2,route+' exceeded 2s'
    return value
for base in ('http://127.0.0.1:18765/api/v1/', 'https://api.qubit-coin.io/api/v1/'):
    assert get('')['version']=='1.8.5'
    for attempt in range(5):
        before=get('status-fast'); summary=get('summary'); after=get('status-fast')
        assert before['ok'] is True and after['ok'] is True
        assert after['version']=='1.8.5' and after['network']=='mainnet'
        if (before['height'],before['tip_hash']) != (after['height'],after['tip_hash']):
            continue
        assert (summary['height'],summary['bestblockhash'])==(after['height'],after['tip_hash'])
        print('Stable API identity:',base,after['height'],after['tip_hash']); break
    else:
        raise SystemExit('No stable identity bracket in five attempts; repeat observation before marking PASS')
PYAPI
'@
    $AmsApiGate | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'AMS3 Explorer API root/status/summary gate failed' }
    Write-Host 'HF131 AMS3 EXPLORER API GATE: PASS'
}
```

The existing Caddy socket bridge remains installed and configured:
`qub-explorer-api-caddy.socket`, `qub-explorer-api-caddy.service` and
`/srv/qub-updates/run/qub-explorer-api.sock`. Do not replace that bridge with a
TCP-only Caddy template. The gate verifies the socket, bridge service, local API
and established public `https://api.qubit-coin.io/api/v1/` endpoint separately.

When a new block occurs, check that `/api/v1/summary` returns the new height
within two seconds. If none occurs during observation, record new-block freshness
as unobserved and keep monitoring. `/health` alone is insufficient because it does not
prove chain-route freshness. Verify the public API has `Cache-Control: no-store`
and is not served stale by a proxy/CDN.

### Final snapshot/API check, then enable the timer

If Cloudflare still returns pre-upgrade cached snapshot bytes, purge the affected
ordinary snapshot URLs listed in Gate 7 and repeat this block. It pins a complete
origin generation, verifies its payload checksums, then checks ordinary public
URLs and API ancestry with normal TLS. A newer API head is allowed if the
snapshot tip is an actual block on that chain. No new mined block is required.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $AmsPublicReady = @'
set -euo pipefail
SRC=/opt/jinex/staging/src/qubd
sudo systemctl start qub-mainnet-snapshot-publish.service
[ "$(systemctl show -p Result --value qub-mainnet-snapshot-publish.service)" = success ]
[ "$(systemctl show -p ExecMainStatus --value qub-mainnet-snapshot-publish.service)" = 0 ]
python3 -B "$SRC/scripts/hf131-check-public-state.py" --snapshot-dir /srv/qub-updates/mainnet/snapshots/.current
python3 -B "$SRC/scripts/hf131-check-public-state.py" --public
sudo systemctl enable --now qub-mainnet-snapshot-publish.timer
systemctl is-active --quiet qub-mainnet-snapshot-publish.timer
systemctl list-timers qub-mainnet-snapshot-publish.timer --all
'@
    $AmsPublicReady | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Public snapshot/API readiness failed; inspect timer state before proceeding' }
    Write-Host 'HF131 SNAPSHOT / PUBLIC API / TIMER ENABLE: PASS'
}
```

Observe the next two timer cycles as specified above while continuing with dist
packaging. Check Result, ExecMainStatus, age and changing generation. The gate
requires successful publications, not a fixed count of newly mined blocks.

## Gate 5 - preserve Explorer 0.7.3

Do not deploy the source ZIP's `explorer/public` tree. It is not the deployed
0.7.3 release. Do not run build-explorer-pages or change its assets in this Core
release. The reviewed deployed 0.7.3 r4 source ZIP SHA256 is
`8eb8ac79c503b564c1a2d49152840ac03c85b4fff5e7a1cfa18565d694dfe749`.
Fourteen chain/API serializers remain byte-identical to HF128; API status and
snapshot diagnostics retain their documented compatible fields.

Open the existing Explorer and check current height/hash, latest blocks, one
existing transaction, your address balances, and pool/QNS views. Repeat after
a new network block if one occurs. Record actual observations. The preceding
API block checks the local API, existing Unix socket bridge and public URL.
Jinex Wallet is not deployed: no Jinex test, deployment or approval is required.

## Gate 6 - build dist and test a copied portable bundle

From the tested source with known-good private assets:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    Set-Location $Source
    $DistRoot = Join-Path $ProjectRoot ('dist\HF131-' + (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0,8))
    [IO.File]::WriteAllText((Join-Path $Evidence 'dist-root.txt'), $DistRoot, [Text.UTF8Encoding]::new($false))
    $BuildArgs = @{ Config = 'mainnet'; BuildInstaller = $true; NativeReceipt = $NativeReceipt; DistRoot = $DistRoot }
    if ($env:QUB_SIGN_CERT_THUMBPRINT) {
        $BuildArgs.Sign = $true
        $BuildArgs.CertThumbprint = $env:QUB_SIGN_CERT_THUMBPRINT
        $ReleaseVerification = @{ CertThumbprint = $env:QUB_SIGN_CERT_THUMBPRINT }
    } else {
        $BuildArgs.OfficialOriginUnsigned = $true
        $ReleaseVerification = @{ OfficialOriginUnsigned = $true }
    }
    & (Join-Path $Source 'scripts\build-windows-release.ps1') @BuildArgs
    $ReleaseVerification | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $Evidence 'release-trust.json') -Encoding ASCII
    Write-Host 'HF131 WINDOWS RELEASE BUILD: PASS'
}
```

This preserves HF128's two supported release modes. With a configured certificate,
every executable and installer must have a valid signature from that exact
thumbprint. Without one, this block explicitly selects `OfficialOriginUnsigned`;
every executable and installer must be `NotSigned`. An invalid, untrusted or
unexpected signature never falls back to unsigned mode. Both modes require the
fixed official HTTPS origin, exact hashes/sizes, version metadata and updater
smoke tests. No certificate purchase or WSL installation is required.

The helper finds Inno Setup in PATH, either Program Files location, or the
established per-user LocalAppData location, before starting Cargo. Native tests
and preflight remain required. Preflight's disposable chain state stays outside
source and dist. The helper verifies the native receipt, copies those exact executables, applies
the selected trust policy, and creates the seven channel files only after successful build/installer
checks. Keep `$ReleaseVerification` for the remaining blocks in this session.

Test the finished portable bytes before any public Windows upload. This copies
the bundle to a separate directory; FCE/wallet data stays in the private smoke
lab, outside the shipping source/dist. Do not copy that test folder back to dist.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $UpdateDir = Join-Path $DistRoot 'updates\mainnet\windows-x64'
    $BundleDir = Join-Path $DistRoot 'QUB-Core-v1.8.5-windows-x64-mainnet'
    $PortableTest = Join-Path ([Environment]::GetFolderPath('Desktop')) ('QUB-HF131-Portable-Test-' + [guid]::NewGuid().ToString('N').Substring(0,8))
    Copy-Item -LiteralPath $BundleDir -Destination $PortableTest -Recurse
    & (Join-Path $Source 'scripts\verify-hf131-windows-release.ps1') -Directory $UpdateDir -BundleDirectory $PortableTest @ReleaseVerification
    python (Join-Path $Source 'scripts\hf131-mainnet-smoke.py') launch --lab $SmokeLab --gui (Join-Path $PortableTest 'QUB-Core.exe') --qubd (Join-Path $PortableTest 'tools\qubd.exe') --portable
    if ($LASTEXITCODE) { throw 'Copied portable mainnet smoke launch failed' }
}
```

Repeat Sync, solo CPU/GPU and pool CPU/GPU Start/Stop briefly on the copied
bundle. Verify logos, fonts, sound, version, device selection and the intended
mainnet payout, then close it. Packaging verifies the installer identity, hash
and trust policy. The portable GUI observations are manual, not implied by a
packaging PASS. Existing installed config files are preserved on upgrade.
An optional fresh-install UI test should use a spare Windows profile, because
Inno registers the same AppId even when selecting another destination folder;
it is not necessary to replace your current installation for the portable test.

## Gate 7 - upload and publish the Windows channel

Independently verify the channel files and all three bundle executables:

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $UpdateDir = Join-Path $DistRoot 'updates\mainnet\windows-x64'
    $BundleDir = Join-Path $DistRoot 'QUB-Core-v1.8.5-windows-x64-mainnet'
    if ($ReleaseVerification -isnot [hashtable]) { throw 'Run the Windows release build block in this session first.' }
    & (Join-Path $Source 'scripts\verify-hf131-windows-release.ps1') -Directory $UpdateDir -BundleDirectory $BundleDir @ReleaseVerification
    $Files = @(Get-ChildItem -LiteralPath $UpdateDir -File | Sort-Object Name)
    $PublicStage = '/tmp/qub-hf131-windows-' + [guid]::NewGuid().ToString('N')
    $PrepareUpload = @'
set -euo pipefail
mkdir -m0700 __PUBLIC_STAGE__
'@
    $PrepareUpload = $PrepareUpload.Replace('__PUBLIC_STAGE__', $PublicStage)
    $PrepareUpload | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Creating fresh AMS3 Windows stage failed' }
    foreach ($File in $Files) {
        scp.exe -i $Key $File.FullName "${AMS3}:$PublicStage/"
        if ($LASTEXITCODE) { throw "Windows upload failed: $($File.Name)" }
    }
    [IO.File]::WriteAllText((Join-Path $Evidence 'public-stage.txt'), $PublicStage, [Text.UTF8Encoding]::new($false))
    Write-Host 'HF131 VERIFIED WINDOWS FILES UPLOADED; PUBLIC MANIFEST UNCHANGED'
}
```

The following block publishes the release. The helper revalidates all uploaded
bytes and unchanged consensus metadata, saves the exact previous channel and
root-Latest files, stages each payload on the destination filesystem, and
renames `manifest.json` last. It rejects different bytes already present under
the versioned v1.8.5 installer URL. `Latest` URLs can change before the manifest;
manifest clients use its immutable versioned `url`. Neither checksums nor a
PE header replaces the preceding Windows trust-policy verification.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    if ($PublicStage -notmatch '^/tmp/qub-hf131-windows-[0-9a-f]{32}$') { throw 'Run the verified Windows upload block first' }
    $PublishWindows = @'
set -euo pipefail
SRC=/opt/jinex/staging/src/qubd
python3 "$SRC/scripts/test-hf131-source-contracts.py" --mode checkout --root "$SRC"
sudo -n bash "$SRC/deploy/digitalocean/publish-hf131-windows-release.sh" __PUBLIC_STAGE__
'@
    $PublishWindows = $PublishWindows.Replace('__PUBLIC_STAGE__', $PublicStage)
    $PublishWindows | ssh.exe -i $Key $AMS3 "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Windows publication failed; use the printed backup and inspect manifest state' }
    Write-Host 'HF131 WINDOWS ORIGIN PUBLICATION GATE: PASS'
}
```

As in the working HF128 procedure, use Cloudflare Custom Purge for the exact
release URLs below after origin publication. Snapshot paths include their
sidecars because HF131 publishes both. Preserve the previous v1.8.4 versioned
installer on origin for older manifests and rollback; do not delete it.

```text
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json
https://download.qubit-coin.io/mainnet/windows-x64/manifest.json.sha256
https://download.qubit-coin.io/mainnet/windows-x64/SHA256SUMS.txt
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe
https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe.sha256
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
```

Verify actual public files using a fresh directory outside source. This check
uses the ordinary URLs consumed by clients, with normal TLS verification and no
cache-busting query. If stale bytes remain, repeat the targeted purge and this
entire check; a nonce-only success is insufficient. Repeat from a second network
before announcement. Headers are retained alongside the evidence.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $UpdateDir = Join-Path $DistRoot 'updates\mainnet\windows-x64'
    $PublicEvidence = Join-Path $ProjectRoot ('_hf131_public_' + [guid]::NewGuid().ToString('N'))
    $PublicDownloads = Join-Path $PublicEvidence 'files'
    New-Item -ItemType Directory -Path $PublicDownloads -Force | Out-Null
    $Base = 'https://download.qubit-coin.io/mainnet/windows-x64'
    foreach ($File in @(Get-ChildItem -LiteralPath $UpdateDir -File)) {
        $Downloaded = Join-Path $PublicDownloads $File.Name
        $Headers = Join-Path $PublicEvidence ($File.Name + '.headers.txt')
        curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 300 -D $Headers "$Base/$($File.Name)" -o $Downloaded
        if ($LASTEXITCODE) { throw "Public download failed: $($File.Name)" }
        if ((Get-FileHash -LiteralPath $Downloaded -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath $File.FullName -Algorithm SHA256).Hash) { throw "Public bytes differ from the verified local release: $($File.Name)" }
    }
    & (Join-Path $Source 'scripts\verify-hf131-windows-release.ps1') -Directory $PublicDownloads @ReleaseVerification
    $RootLatest = Join-Path $PublicEvidence 'root-QUB-Core-Latest.exe'
    curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 300 -D (Join-Path $PublicEvidence 'root-Latest.headers.txt') 'https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe' -o $RootLatest
    if ($LASTEXITCODE) { throw 'Root Latest public download failed' }
    if ((Get-FileHash -LiteralPath $RootLatest -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath (Join-Path $UpdateDir 'QUB-Core-Latest.exe') -Algorithm SHA256).Hash) { throw 'Public root Latest differs from the released installer' }
    $RootSidecar = Join-Path $PublicEvidence 'root-QUB-Core-Latest.exe.sha256'
    curl.exe --fail --silent --show-error --connect-timeout 5 --max-time 30 'https://download.qubit-coin.io/mainnet/QUB-Core-Latest.exe.sha256' -o $RootSidecar
    if ($LASTEXITCODE) { throw 'Root Latest public sidecar download failed' }
    if ((Get-Content -LiteralPath $RootSidecar -Raw -Encoding ASCII).Trim() -cne (Get-FileHash -LiteralPath $RootLatest -Algorithm SHA256).Hash.ToLowerInvariant()) { throw 'Public root Latest sidecar mismatch' }
    Write-Host "HF131 PUBLIC WINDOWS BYTES / TRUST POLICY GATE: PASS - $PublicEvidence"
}
```

Perform both:

- clean install on a fresh Windows profile/device;
- update an actual HF128 v1.8.4 installation through the public manifest.

Require bounded download, size/SHA256/ProductVersion/selected trust verification,
successful relaunch, preserved wallet/settings/data, correct assets, Sync,
Solo, Pool, Stop and Recent Blocks behavior. Corrupt SHA/size, HTTP/cross-origin
and modified staged EXE tests must fail visibly and keep the old Core runnable.

## Gate 8 - repository, immutable tag and GitHub release

Only after Gates 0-7 and the observed mainnet/portable checks pass. Use the established
`qub-core-opensource` repository and mirror the exact verified no-assets ZIP,
not the Windows working tree containing private runtime assets.

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'

    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    . (Join-Path $ProjectRoot 'HF131-v185-PREPARE-r4.ps1') -Resume
    python (Join-Path $Source 'scripts\hf131-native-build.py') verify --source $Source --receipt $NativeReceipt
    if ($LASTEXITCODE) { throw 'Native evidence does not match repository candidate' }
    $Repo = Join-Path $ProjectRoot 'qub-core-opensource'
    $Zip = Join-Path $ProjectRoot 'HF131-v185-source-no-assets-r4.zip'
    $Sums = Join-Path $ProjectRoot 'HF131-v185-r4-SHA256SUMS.txt'
    $Extract = Join-Path $ProjectRoot ('_hf131_repo_extract_' + [guid]::NewGuid().ToString('N'))
    $Sync = Join-Path $ProjectRoot ('_repo_sync_hf131_' + [guid]::NewGuid().ToString('N'))

    if (-not (Test-Path (Join-Path $Repo '.git') -PathType Container)) { throw "Repo missing: $Repo" }
    if (-not (Test-Path $Zip -PathType Leaf)) { throw "HF131 source ZIP missing: $Zip" }
    if (-not (Test-Path $Sums -PathType Leaf)) { throw "HF131 source sums missing: $Sums" }

    $SumLines = @(Get-Content $Sums -Encoding ASCII | Where-Object { $_ -match '^[0-9a-fA-F]{64}  HF131-v185-source-no-assets-r4\.zip$' })
    if ($SumLines.Count -ne 1) { throw 'Need one exact HF131 ZIP checksum entry' }
    $Expected = ($SumLines[0] -split '  ')[0].ToLowerInvariant()
    $Actual = (Get-FileHash $Zip -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Actual -ne $Expected) { throw 'HF131 source ZIP hash mismatch before repo update' }

    Set-Location $Repo
    $Origin = (git remote get-url origin).Trim()
    if ($LASTEXITCODE) { throw 'git origin lookup failed' }
    if ($Origin -notmatch '^(https://github\.com/|git@github\.com:)AlxProe/qub-core(?:\.git)?/?$') { throw "Unexpected repository origin: $Origin" }
    $InitialStatus = @(git status --porcelain)
    if ($LASTEXITCODE) { throw 'git status failed' }
    if ($InitialStatus.Count -gt 0) { throw 'Repository has uncommitted changes' }
    $env:GIT_CONFIG_COUNT = '1'
    $env:GIT_CONFIG_KEY_0 = 'core.autocrlf'
    $env:GIT_CONFIG_VALUE_0 = 'false'
    git fetch origin --tags --prune
    if ($LASTEXITCODE) { throw 'git fetch failed' }
    git checkout main
    if ($LASTEXITCODE) { throw 'git checkout main failed' }
    git pull --ff-only origin main
    if ($LASTEXITCODE) { throw 'git pull --ff-only failed' }

    Expand-Archive -LiteralPath $Zip -DestinationPath $Extract
    $SourceRoot = Join-Path $Extract 'qubd-v1.8.5'
    if (-not (Test-Path (Join-Path $SourceRoot 'Cargo.toml') -PathType Leaf)) { throw 'HF131 ZIP root missing' }
    $NativeIdentity = Get-Content -LiteralPath $NativeReceipt -Raw | ConvertFrom-Json
    if ((Get-FileHash -LiteralPath (Join-Path $SourceRoot 'HF131-SOURCE-MANIFEST.json') -Algorithm SHA256).Hash.ToLowerInvariant() -cne $NativeIdentity.identity.manifest_sha256) { throw 'Repository source differs from native-tested source' }

    python (Join-Path $SourceRoot 'scripts\test-hf131-source-contracts.py') --mode archive --root $SourceRoot
    if ($LASTEXITCODE) { throw 'HF131 extracted source contract failed before repo mirror' }

    New-Item -ItemType Directory -Force $Sync | Out-Null
    robocopy.exe $SourceRoot $Sync /MIR /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Source mirror failed: $LASTEXITCODE" }

    # Preserve repository governance/workflow files only when not owned by the
    # release inventory. The checkout contract checks these auxiliary files;
    # manifest-only exports intentionally exclude them.
    foreach ($Name in @('LICENSE','LICENSE.md','LICENSE.txt','NOTICE','NOTICE.md','NOTICE.txt','SECURITY.md','CONTRIBUTING.md','CODE_OF_CONDUCT.md')) {
        if ((Test-Path (Join-Path $Repo $Name)) -and -not (Test-Path (Join-Path $SourceRoot $Name))) {
            Copy-Item -LiteralPath (Join-Path $Repo $Name) -Destination (Join-Path $Sync $Name)
        }
    }
    if ((Test-Path (Join-Path $Repo '.github')) -and -not (Test-Path (Join-Path $SourceRoot '.github'))) {
        Copy-Item -LiteralPath (Join-Path $Repo '.github') -Destination (Join-Path $Sync '.github') -Recurse
    }
    python (Join-Path $Sync 'scripts\test-hf131-source-contracts.py') --mode checkout --root $Sync
    if ($LASTEXITCODE) { throw 'Exact source plus preserved governance failed before repository mirror' }
    robocopy.exe $Sync $Repo /MIR /XD (Join-Path $Repo '.git') /R:1 /W:1 /NFL /NDL /NP | Out-Null
    if ($LASTEXITCODE -gt 7) { throw "Repository mirror failed: $LASTEXITCODE" }

    Set-Location $Repo
    python .\scripts\test-hf131-source-contracts.py --mode checkout --root $Repo
    if ($LASTEXITCODE) { throw 'Repository source contract failed' }
    git diff --check
    if ($LASTEXITCODE) { throw 'git diff --check failed' }

    $Changes = @(git status --porcelain)
    if ($LASTEXITCODE) { throw 'git status failed' }
    $Changes
    if ($Changes.Count -eq 0) { throw 'Repository has no HF131 changes; do not create a tag' }
    git diff --stat
    if ($LASTEXITCODE) { throw 'git diff --stat failed' }
    git diff -- src\pools.rs src\lib.rs src\p2p.rs
    if ($LASTEXITCODE) { throw 'git source diff failed' }

    git add -A
    if ($LASTEXITCODE) { throw 'git add failed' }
    git update-index --chmod=+x deploy/digitalocean/publish-mainnet-snapshot.sh
    if ($LASTEXITCODE) { throw 'git publisher mode update failed' }
    git update-index --chmod=+x deploy/digitalocean/test-publish-mainnet-snapshot.sh deploy/digitalocean/install-hf131-seed.sh deploy/digitalocean/install-hf131-explorer-api.sh deploy/digitalocean/publish-hf131-windows-release.sh deploy/digitalocean/rollback-hf131-seed.sh deploy/digitalocean/hf131-release-common.sh
    if ($LASTEXITCODE) { throw 'git publisher test mode update failed' }
    git diff --cached --check
    if ($LASTEXITCODE) { throw 'Staged diff check failed' }
    $Staged = @(git diff --cached --name-status)
    if ($LASTEXITCODE) { throw 'git staged file list failed' }
    $Staged
    foreach ($Required in @('HF131-SOURCE-MANIFEST.json','HF131-v185-DEPLOY-RUNBOOK.md','RELEASE_NOTES-v1.8.5-HF131.md')) {
        if (-not (Test-Path (Join-Path $Repo $Required) -PathType Leaf)) { throw "Missing release file: $Required" }
    }

    git commit -m 'Update QUB Core to v1.8.5 HF131'
    if ($LASTEXITCODE) { throw 'HF131 commit failed' }
    $Commit = (git rev-parse HEAD).Trim()
    if ($LASTEXITCODE) { throw 'git revision read failed' }
    git push origin main
    if ($LASTEXITCODE) { throw 'HF131 main push failed' }
    git fetch origin main
    if ($LASTEXITCODE) { throw 'git remote confirmation fetch failed' }
    $RemoteCommit = (git rev-parse origin/main).Trim()
    if ($LASTEXITCODE) { throw 'git remote revision read failed' }
    if ($RemoteCommit -ne $Commit) { throw 'Remote main does not match the HF131 commit' }

    $ExistingTag = @(git tag -l 'v1.8.5-hf131')
    if ($LASTEXITCODE) { throw 'git local tag lookup failed' }
    if ($ExistingTag.Count) { throw 'Local tag v1.8.5-hf131 already exists; never move a release tag' }
    $RemoteTag = git ls-remote --tags origin 'refs/tags/v1.8.5-hf131'
    if ($LASTEXITCODE) { throw 'git remote tag lookup failed' }
    if ($RemoteTag) { throw 'Remote tag v1.8.5-hf131 already exists; never move a release tag' }
    git tag -a v1.8.5-hf131 -m 'QUB Core v1.8.5 HF131' $Commit
    if ($LASTEXITCODE) { throw 'HF131 annotated tag creation failed' }
    git push origin v1.8.5-hf131
    if ($LASTEXITCODE) { throw 'HF131 tag push failed' }

    Write-Host "HF131 REPO/TAG GATE: PASS - $Commit"
}
```

Create the GitHub release only after the pushed commit/tag and public installer
hashes are rechecked:

```powershell
& {
    Set-StrictMode -Version Latest
    $ErrorActionPreference = 'Stop'
    $ProjectRoot = 'C:\Users\proes\Desktop\qub-node'
    . (Join-Path $ProjectRoot 'HF131-v185-PREPARE-r4.ps1') -Resume
    $Installer = Join-Path $DistRoot 'installer\QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
    $InstallerSidecar = Join-Path $DistRoot 'updates\mainnet\windows-x64\QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe.sha256'

    foreach ($Required in @(
        (Join-Path $ProjectRoot 'HF131-v185-source-no-assets-r4.zip'),
        (Join-Path $ProjectRoot 'HF131-v185-r4-SHA256SUMS.txt'),
        $Installer,
        $InstallerSidecar,
        (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF131.md')
    )) {
        if (-not (Test-Path $Required -PathType Leaf)) { throw "Release attachment missing: $Required" }
    }

    $UpdateDir = Join-Path $DistRoot 'updates\mainnet\windows-x64'
    & (Join-Path $Source 'scripts\verify-hf131-windows-release.ps1') -Directory $UpdateDir @ReleaseVerification
    if ((Get-FileHash -LiteralPath $Installer -Algorithm SHA256).Hash -ne (Get-FileHash -LiteralPath (Join-Path $UpdateDir (Split-Path $Installer -Leaf)) -Algorithm SHA256).Hash) { throw 'GitHub installer differs from public channel bytes' }
    $Commit = (git -C $Repo rev-parse 'v1.8.5-hf131^{}').Trim()
    if ($LASTEXITCODE) { throw 'Local release tag missing' }
    $RemoteTag = @(git -C $Repo ls-remote origin 'refs/tags/v1.8.5-hf131^{}')
    if ($LASTEXITCODE -or $RemoteTag.Count -ne 1 -or ($RemoteTag[0] -split '\s+')[0] -cne $Commit) { throw 'Remote annotated tag differs from the local release commit' }
    gh release create v1.8.5-hf131 `
        (Join-Path $ProjectRoot 'HF131-v185-source-no-assets-r4.zip') `
        (Join-Path $ProjectRoot 'HF131-v185-r4-SHA256SUMS.txt') `
        $Installer `
        $InstallerSidecar `
        --repo AlxProe/qub-core --verify-tag `
        --title 'QUB Core v1.8.5 HF131 - Mining and Sync Reliability' `
        --notes-file (Join-Path $Source 'RELEASE_NOTES-v1.8.5-HF131.md')
    if ($LASTEXITCODE) { throw 'GitHub release creation failed' }
    Write-Host 'HF131 GITHUB RELEASE: PASS'
}
```

## Gate 9 - Telegram and monitoring

Post `HF131-v185-TELEGRAM-POST-r4.txt` only after the merged/tagged release and all
public download checks succeed. It has no unfilled checksum placeholders; publish it only after the actual
installer hash and download URLs have passed Gate 7. Verify every link in an incognito browser and
from a second network before posting.

## Post-release monitoring

For the first 24 hours, continue the seed/API/publisher observations and watch for:

- no new block for two target intervals when independent miners are visible;
- AMS3/NYC3 height/hash divergence;
- public tip age >150 seconds or source not FCE;
- publisher non-success/lock contention/cycle >115 seconds;
- `/api/v1/summary` latency >2 seconds or lag beyond two publication intervals;
- Explorer/Core GUI monotonicity violation;
- legacy exporter failure counter/commit latency spike;
- any Pool-to-Solo or duplicate mining-controller report.

Keep the exact deployment backups until at least 72 healthy hours and one full
Windows update cycle.

## Rollback

HF131 introduces no consensus activation. Restore binaries and operational
configuration while retaining the current canonical data and independent seed
identities. Use only the exact timestamped paths recorded during installation.
A failed install's recovery message is an attempt, not a rollback PASS: verify
the running binary hash, services and tip after it returns.

1. Restore the previous public Windows channel from the exact
   `/opt/qub/releases/windows-before-hf131-*` backup printed at publication.
   Restore the previous Latest installers and sidecars from that same backup.
   Copy its manifest to a temporary file inside the channel directory, restore
   the matching manifest sidecar, then rename manifest.json last. Preserve versioned installers so
   clients that already read either manifest can finish a matching download.
2. Stop the snapshot timer while changing the seed. For a seed-only rollback,
   KEEP `/opt/qub/bin/qubd-snapshot-export-hf131`, the HF131 publisher script/unit,
   and the complete managed snapshot tree. HF128's exporter lacks the strict
   report fields the new publisher requires. Do not point the publisher at the
   rolled-back `/opt/qub/bin/qubd`.
3. Roll back NYC3 first using `qubd` and `mainnet-seed.toml` from its exact seed
   backup, with mainnet and any previously active testnet process stopped and
   verified absent before installation. Restart only previously active services,
   verify running `/proc/PID/exe` hash and unchanged `node_id`, then prove healthy.
4. Roll back AMS3 by the same procedure after NYC3 is healthy. Keep the versioned
   snapshot exporter. The helper publishes through systemd and restores a
   previously active timer after a successful cycle. Immediately verify public
   freshness; if it fails, stop the timer and diagnose before proceeding.
5. If the separate Explorer API is the failing layer, stop only its unit; restore
   the previous `hf131.conf` from the exact API backup (or remove this one drop-in
   if the backup says it did not exist), restore the previous versioned API binary
   if one was backed up, daemon-reload and restart that API unit. Earlier drop-ins
   remain intact. Verify the seed PID/hash did not change.
6. A FULL publisher rollback is a separate operation: stop timer and service,
   retain the current whole managed tree as evidence, restore the complete
   pre-migration `snapshots` directory and canonical aliases/sidecars with
   symlinks preserved from the SAME backup, then restore the old publisher
   script/unit/timer and any prior Caddy cache rules. Never run the old flat-file
   publisher over HF131's managed symlinks and never restore individual files
   from different generations. If the backup is missing or inconsistent, stop
   publication and diagnose; do not fabricate a replacement chain snapshot.

A seed-only rollback can be executed from the same Windows session using the
included helper. Its backup argument must be the exact path printed for that
region; it does not touch snapshot generations, wallets or chain state.

```powershell
. {
    $ErrorActionPreference = 'Stop'
    $Region = 'nyc3'
    $RollbackHost = $NYC3
    $SeedBackup = Read-Host 'Paste the exact NYC3 seed backup path printed by Gate 4'
    if ($SeedBackup -notmatch '^/opt/qub/bin/backups/hf131-nyc3-[0-9]{8}-[0-9]{6}-[0-9]+$') { throw 'Invalid NYC3 backup path' }
    $RollbackSeed = @'
set -euo pipefail
bash /opt/jinex/staging/src/qubd/deploy/digitalocean/rollback-hf131-seed.sh __REGION__ __BACKUP__
'@
    $RollbackSeed = $RollbackSeed.Replace('__REGION__', $Region).Replace('__BACKUP__', $SeedBackup)
    $RollbackSeed | ssh.exe -i $Key $RollbackHost "tr -d '\r' | bash -s"
    if ($LASTEXITCODE) { throw 'Seed rollback failed; inspect services and exact backup' }
}
```

After NYC3 is verified healthy, repeat this block for AMS3 with `$Region='ams3'`,
`$RollbackHost=$AMS3`, the exact AMS3 backup, and `hf131-ams3-` in the path check.
The helper resumes a previously active snapshot timer only after a successful
manual publication using the retained HF131 exporter.

Rollback is complete only when the old public manifest is visible, both seed
listeners are healthy with the expected binary hashes, RPC is not public,
independent seed IDs are unchanged, the API is read-only and responsive,
publisher/timer is healthy, and Core/Explorer tips advance again. Preserve
all chain-v2 generations, wallets, node IDs, peer state and durable outboxes.

## Evidence record

Keep the exact source ZIP/SHA256, native receipt/logs, GUI/portable observations,
both Linux build logs/hashes, printed seed/API/publisher backup paths, publisher
cycle results, origin/public hashes, Git commit/tag and release link.
Do not put wallet keys, private smoke data, secrets or private runtime assets in
GitHub, Telegram or the no-assets source ZIP. No chain data rollback is part of
an ordinary binary rollback. A failed gate is never converted to PASS by moving
past it. Ask for diagnosis with that gate's output if a real failure remains.
