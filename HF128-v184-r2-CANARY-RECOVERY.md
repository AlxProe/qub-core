# HF128 v1.8.4 r2 Canary Recovery

## Why r2 exists

The first HF128 AMS3 canary was chain-safe and successfully installed, but its
journal exposed repeated connections from AMS3 back to itself through the
legacy `seed.qubit-coin.io` DNS alias. Node-ID equality rejected every session,
so the alias never counted as independent seed evidence, but the alias remained
eligible for later heartbeat, sync and relay attempts.

r2 adds a transport-only learned self-alias quarantine. It changes no consensus
rule, activation height, block/transaction format, P2P protocol number, state
schema, wallet data or persisted node ID.

## Current live-state rule

Leave the already-running HF128 seed services online while building r2. Do not
roll back chain data and do not delete `node_id`, `chain-v2`, peer data, wallets,
pending block delivery or pending transaction delivery.

Do not update the standalone Explorer API, public Windows manifest or GitHub
release until the r2 journal gate below passes on NYC3 and AMS3.

## r2 rollout sequence

1. Verify and clean-extract `HF128-v184-source-no-assets-r2.zip` with sections
   A1-A4 of the main runbook.
2. Run all Cargo, regression, real-process and mainnet gates in A5-A9.
3. Recreate/upload the source tar with section B.
4. Rebuild and test both staged seed trees with section C.
5. Re-run the distinct-node-ID gate C2. Never regenerate an ID unless the gate
   proves the IDs are identical.
6. Deploy NYC3 with section D.
7. Deploy AMS3 with section E. The r2 runbook prints numbered progress and a
   heartbeat every 30 seconds during service waits. Do not interrupt while SSH
   remains connected and the bounded gate is still reporting progress.
8. Run the journal gate below. Continue to section F only after it prints PASS.

## Mandatory post-restart journal gate

Run from PowerShell after both r2 seed deployments:

```powershell
$Key  = Join-Path $env:USERPROFILE ".ssh\jinex_ed25519"
$AMS3 = "deploy@159.223.222.103"
$NYC3 = "deploy@167.99.57.45"

$Gate = @'
set -euo pipefail
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
MAIN=qub-seed-mainnet.service

systemctl is-active --quiet "$MAIN"
PID="$(systemctl show -p MainPID --value "$MAIN")"
[ "$PID" -gt 0 ]
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$(sudo sha256sum "/proc/$PID/exe" | awk '{print $1}')" ]

timeout --signal=TERM --kill-after=10s 180s \
  "$BIN" --config "$CFG" status-fast >/tmp/hf128-r2-status.json
python3 - <<'PY'
import json
s=json.load(open('/tmp/hf128-r2-status.json'))
assert s.get('ok') is True
assert s.get('network') == 'mainnet'
assert s.get('hf128_self_alias_quarantine') is True
assert s.get('hf128_seed_identity_dedupe') is True
assert s.get('hf128_bounded_chain_sync') is True
print('tip',s['height'],s['tip_hash'])
PY

SINCE="$(systemctl show -p ActiveEnterTimestamp --value "$MAIN")"
sudo journalctl -u "$MAIN" --since "$SINCE" --no-pager \
  >/tmp/hf128-r2-seed-journal.txt

LEARNED="$(grep -c 'HF128 learned self-alias quarantined for this process: seed.qubit-coin.io:17444' /tmp/hf128-r2-seed-journal.txt || true)"
REJECTS="$(grep -cE 'self connection rejected.*seed\.qubit-coin\.io:17444' /tmp/hf128-r2-seed-journal.txt || true)"
echo "learned-generic-self-alias=$LEARNED generic-self-rejects=$REJECTS"

[ "$LEARNED" -le 1 ]
[ "$REJECTS" -le 2 ]

sleep 180
LATER_REJECTS="$(sudo journalctl -u "$MAIN" --since "$SINCE" --no-pager | grep -cE 'self connection rejected.*seed\.qubit-coin\.io:17444' || true)"
echo "generic-self-rejects-after-180s=$LATER_REJECTS"
[ "$LATER_REJECTS" -le 2 ]

echo 'HF128 r2 SELF-ALIAS GATE: PASS'
'@

$Gate | ssh.exe -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -i $Key $NYC3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw 'NYC3 HF128 r2 self-alias gate failed' }

$Gate | ssh.exe -o ConnectTimeout=15 -o ServerAliveInterval=15 -o ServerAliveCountMax=3 -i $Key $AMS3 "tr -d '\r' | bash -s"
if ($LASTEXITCODE -ne 0) { throw 'AMS3 HF128 r2 self-alias gate failed' }
```

Expected AMS3 behavior is zero or one learned generic self-alias line and no
continued growth in generic-alias rejection count. NYC3 may report zero learned
lines because the generic alias currently points to AMS3, which is a different
node from NYC3.

If either count keeps growing, stop before Explorer API/public release and
capture the complete journals. Do not delete peer files or node IDs as a
workaround.
