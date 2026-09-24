#!/usr/bin/env bash
# Restore only exact backed-up seed binary/config. Keep HF130 snapshot publisher.
set -euo pipefail
REGION=${1:?usage: rollback-hf130-seed.sh nyc3|ams3 EXACT_BACKUP_DIRECTORY}
BACKUP=${2:?exact backup directory required}
[[ "$REGION" == nyc3 || "$REGION" == ams3 ]]
[[ "$BACKUP" =~ ^/opt/qub/bin/backups/hf130-$REGION-[0-9]{8}-[0-9]{6}-[0-9]+$ ]]
sudo -n true
[ "$(cat "$BACKUP/region")" = "$REGION" ]
[ -f "$BACKUP/qubd" ] && [ -f "$BACKUP/mainnet-seed.toml" ]
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
DATA=/opt/qub/mainnet/data
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
TIMER=qub-mainnet-snapshot-publish.timer
SERVICE=qub-mainnet-snapshot-publish.service
EXPECTED=$(sha256sum "$BACKUP/qubd" | awk '{print $1}')
IDENTITY=$(cat "$BACKUP/node-id-sha256")
[ "$(sudo sha256sum "$DATA/node_id" | awk '{print $1}')" = "$IDENTITY" ]
TEST_ACTIVE=$(cat "$BACKUP/test-was-active")
[[ "$TEST_ACTIVE" == 0 || "$TEST_ACTIVE" == 1 ]]
TIMER_ACTIVE=0
stop_unit() {
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$1" || return $?
    [ "$(systemctl show -p MainPID --value "$1")" = 0 ] || return 1
    ! systemctl is-active --quiet "$1"
}
if [ "$REGION" = ams3 ]; then
    [ -x /opt/qub/bin/qubd-snapshot-export-hf130 ]
    grep -Fxq 'Environment=BIN=/opt/qub/bin/qubd-snapshot-export-hf130' "/etc/systemd/system/$SERVICE"
    if systemctl is-active --quiet "$TIMER"; then TIMER_ACTIVE=1; fi
    sudo systemctl stop "$TIMER"
    stop_unit "$SERVICE"
fi
stop_unit "$MAIN"
if systemctl is-active --quiet "$TEST"; then stop_unit "$TEST"; fi
sudo install -m0755 -o root -g root "$BACKUP/qubd" "$BIN"
sudo install -m0644 -o root -g root "$BACKUP/mainnet-seed.toml" "$CFG"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$EXPECTED" ]
sudo systemctl start "$MAIN"
for attempt in $(seq 1 120); do
    if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep ':17444' >/dev/null; then break; fi
    [ "$attempt" -lt 120 ]; sleep 5
done
PID=$(systemctl show -p MainPID --value "$MAIN")
[ "$(sudo sha256sum /proc/$PID/exe | awk '{print $1}')" = "$EXPECTED" ]
[ "$(sudo sha256sum "$DATA/node_id" | awk '{print $1}')" = "$IDENTITY" ]
STATUS=$(mktemp /tmp/hf130-rollback-status.XXXXXX.json)
timeout --signal=TERM --kill-after=20s 300s "$BIN" --config "$CFG" status-fast > "$STATUS"
python3 -I - "$STATUS" <<'PY'
import json,sys
s=json.load(open(sys.argv[1]))
assert s['ok'] is True and s['network']=='mainnet'
assert s['height']>=24000 and s['tip_block_version']==2 and s['next_block_expected_version']==2
print('Rollback selected identity:',s['version'],s['height'],s['tip_hash'])
PY
if [ "$TEST_ACTIVE" = 1 ]; then sudo systemctl start "$TEST"; fi
if [ "$REGION" = ams3 ]; then
    sudo systemctl start "$SERVICE"
    [ "$(systemctl show -p Result --value "$SERVICE")" = success ]
    [ "$(systemctl show -p ExecMainStatus --value "$SERVICE")" = 0 ]
    if [ "$TIMER_ACTIVE" = 1 ]; then sudo systemctl start "$TIMER"; fi
fi
printf 'HF130 %s SEED BINARY/CONFIG ROLLBACK: PASS - %s\nVerify public and cross-client liveness separately.\n' "$REGION" "$EXPECTED"
