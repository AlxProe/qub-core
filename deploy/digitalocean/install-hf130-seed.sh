#!/usr/bin/env bash
# Reviewed HF130 candidate install. Does not touch canonical data or node identity.
set -euo pipefail
REGION=${1:?usage: install-hf130-seed.sh nyc3|ams3 EXPECTED_BINARY_SHA256}
EXPECTED=${2:?expected staged binary SHA256 required}
[[ "$REGION" == nyc3 || "$REGION" == ams3 ]]
[[ "$EXPECTED" =~ ^[0-9a-f]{64}$ ]]
SRC=/opt/jinex/staging/src/qubd
STAGED=$SRC/target/release/qubd
BIN=/opt/qub/bin/qubd
CFG=/opt/qub/mainnet/mainnet-seed.toml
DATA=/opt/qub/mainnet/data
MAIN=qub-seed-mainnet.service
TEST=qub-seed-testnet.service
TIMER=qub-mainnet-snapshot-publish.timer
PUBLISHER=qub-mainnet-snapshot-publish.service
REGIONAL=$SRC/deploy/digitalocean/mainnet-seed-$REGION.toml
BACKUP=/opt/qub/bin/backups/hf130-$REGION-$(date -u +%Y%m%d-%H%M%S)-$$
TEST_ACTIVE=0
TIMER_ACTIVE=0
BACKUP_READY=0
sudo -n true
python3 "$SRC/scripts/test-hf130-source-contracts.py" --mode checkout --root "$SRC"
[ -x "$STAGED" ] && [ -f "$REGIONAL" ]
[ "$(sha256sum "$STAGED" | awk '{print $1}')" = "$EXPECTED" ]
systemctl is-active --quiet "$MAIN"
[ "$(systemctl show -p MainPID --value "$MAIN")" -gt 0 ]
sudo test -f "$DATA/chain-v2/CURRENT.json"
if [ "$REGION" = ams3 ]; then
    # Check the new publisher's read identity before stopping a live seed.
    sudo -u deploy test -r "$DATA/chain-v2/CURRENT.json"
    for directory in /srv/qub-updates/mainnet /srv/qub-updates/mainnet/snapshots; do
        sudo test -d "$directory"
        ! sudo test -L "$directory"
    done
fi
IDENTITY_BEFORE=$(sudo sha256sum "$DATA/node_id" | awk '{print $1}')
grep -Fq "advertise_addr = \"seed-$REGION.qubit-coin.io:17444\"" "$REGIONAL"
OTHER=ams3; [ "$REGION" != ams3 ] || OTHER=nyc3
grep -Fq "bootnodes = [\"seed-$OTHER.qubit-coin.io:17444\"]" "$REGIONAL"
if systemctl is-active --quiet "$TEST"; then TEST_ACTIVE=1; fi
if [ "$REGION" = ams3 ] && systemctl is-active --quiet "$TIMER"; then TIMER_ACTIVE=1; fi
sudo install -d -m0755 -o root -g root "$BACKUP"
sudo cp -a "$BIN" "$BACKUP/qubd"
sudo cp -a "$CFG" "$BACKUP/mainnet-seed.toml"
printf '%s\n' "$REGION" | sudo tee "$BACKUP/region" >/dev/null
printf '%s\n' "$TEST_ACTIVE" | sudo tee "$BACKUP/test-was-active" >/dev/null
printf '%s\n' "$IDENTITY_BEFORE" | sudo tee "$BACKUP/node-id-sha256" >/dev/null
printf 'Exact seed rollback backup: %s\n' "$BACKUP"
stop_unit() {
    sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$1" || return $?
    [ "$(systemctl show -p MainPID --value "$1")" = 0 ] || return 1
    ! systemctl is-active --quiet "$1"
}
restore_optional() {
    local name=$1 path=$2 mode=$3
    if sudo test -f "$BACKUP/$name"; then sudo install -m"$mode" "$BACKUP/$name" "$path"; else sudo rm -f "$path"; fi
}
recover() {
    local rc=$?
    trap - EXIT
    if [ "$rc" -ne 0 ] && [ "$BACKUP_READY" = 1 ]; then
        set +e
        if ! stop_unit "$MAIN"; then
            printf 'Recovery stopped: mainnet process did not stop; no binary overwrite attempted. Backup: %s\n' "$BACKUP" >&2
            exit "$rc"
        fi
        if [ "$TEST_ACTIVE" = 1 ] && ! stop_unit "$TEST"; then
            printf 'Recovery stopped: testnet process did not stop; no binary overwrite attempted. Backup: %s\n' "$BACKUP" >&2
            exit "$rc"
        fi
        sudo install -m0755 -o root -g root "$BACKUP/qubd" "$BIN"
        sudo install -m0644 -o root -g root "$BACKUP/mainnet-seed.toml" "$CFG"
        if [ "$REGION" = ams3 ]; then
            # No HF130 publication is started by this installer, so managed
            # snapshot migration has not happened here. Restore old units safely.
            restore_optional publish-mainnet-snapshot.sh /opt/qub/bin/publish-mainnet-snapshot.sh 0755
            restore_optional qub-mainnet-snapshot-publish.service /etc/systemd/system/qub-mainnet-snapshot-publish.service 0644
            restore_optional qub-mainnet-snapshot-publish.timer /etc/systemd/system/qub-mainnet-snapshot-publish.timer 0644
            if sudo test -f "$BACKUP/publication-directory-modes.txt"; then
                while read -r original_uid original_gid original_mode directory; do
                    sudo chown "$original_uid:$original_gid" "$directory"
                    sudo chmod "$original_mode" "$directory"
                done < <(sudo cat "$BACKUP/publication-directory-modes.txt")
            fi
            sudo systemctl daemon-reload
        fi
        sudo systemctl start "$MAIN"
        if [ "$TEST_ACTIVE" = 1 ]; then sudo systemctl start "$TEST"; fi
        if [ "$TIMER_ACTIVE" = 1 ]; then sudo systemctl start "$TIMER"; fi
        printf 'Install failed; recovery attempted from %s. Verify service status before proceeding.\n' "$BACKUP" >&2
    fi
    exit "$rc"
}
trap recover EXIT
if [ "$REGION" = ams3 ]; then
    # Back up only the two directories' own metadata; never recursively change
    # ownership of public payloads, private data, or the Explorer webroot.
    for directory in /srv/qub-updates/mainnet /srv/qub-updates/mainnet/snapshots; do
        sudo stat -c '%u %g %a %n' "$directory"
    done | sudo tee "$BACKUP/publication-directory-modes.txt" >/dev/null
    for name in publish-mainnet-snapshot.sh qub-mainnet-snapshot-publish.service qub-mainnet-snapshot-publish.timer; do
        case "$name" in *.sh) source=/opt/qub/bin/$name;; *) source=/etc/systemd/system/$name;; esac
        if sudo test -f "$source"; then sudo cp -a "$source" "$BACKUP/$name"; fi
    done
    BACKUP_READY=1
    if [ "$(systemctl show -p LoadState --value "$TIMER")" != not-found ]; then sudo systemctl stop "$TIMER"; fi
    if [ "$(systemctl show -p LoadState --value "$PUBLISHER")" != not-found ]; then stop_unit "$PUBLISHER"; fi
    # Preserve symlinks and hidden generation directories, never dereference.
    if sudo test -d /srv/qub-updates/mainnet/snapshots; then sudo cp -a /srv/qub-updates/mainnet/snapshots "$BACKUP/snapshots"; fi
    for name in canonical-chain.json canonical-chain.json.sha256; do
        if sudo test -e "/srv/qub-updates/mainnet/$name"; then sudo cp -a "/srv/qub-updates/mainnet/$name" "$BACKUP/$name"; fi
    done
    # Canonical aliases live one level above snapshots. Child-only write access
    # cannot replace those aliases during the first managed-generation migration.
    sudo install -d -m0755 -o deploy -g deploy /srv/qub-updates/mainnet /srv/qub-updates/mainnet/snapshots
    sudo -u deploy test -w /srv/qub-updates/mainnet
    sudo -u deploy test -w /srv/qub-updates/mainnet/snapshots
fi
BACKUP_READY=1
stop_unit "$MAIN"
if [ "$TEST_ACTIVE" = 1 ]; then stop_unit "$TEST"; fi
sudo install -m0755 -o root -g root "$STAGED" "$BIN"
sudo install -m0644 -o root -g root "$REGIONAL" "$CFG"
[ "$(sha256sum "$BIN" | awk '{print $1}')" = "$EXPECTED" ]
if [ "$REGION" = ams3 ]; then
    sudo install -m0755 -o root -g root "$STAGED" /opt/qub/bin/qubd-snapshot-export-hf130
    [ "$(sha256sum /opt/qub/bin/qubd-snapshot-export-hf130 | awk '{print $1}')" = "$EXPECTED" ]
    sudo install -m0755 -o root -g root "$SRC/deploy/digitalocean/publish-mainnet-snapshot.sh" /opt/qub/bin/publish-mainnet-snapshot.sh
    sudo install -m0644 -o root -g root "$SRC/deploy/digitalocean/$PUBLISHER" "/etc/systemd/system/$PUBLISHER"
    sudo install -m0644 -o root -g root "$SRC/deploy/digitalocean/$TIMER" "/etc/systemd/system/$TIMER"
    sudo systemctl daemon-reload
fi
sudo systemctl start "$MAIN"
for attempt in $(seq 1 120); do
    if systemctl is-active --quiet "$MAIN" && sudo ss -ltnp | grep ':17444' >/dev/null; then break; fi
    [ "$attempt" -lt 120 ]; sleep 5
done
PID=$(systemctl show -p MainPID --value "$MAIN")
[ "$(sudo sha256sum /proc/$PID/exe | awk '{print $1}')" = "$EXPECTED" ]
[ "$(sudo sha256sum "$DATA/node_id" | awk '{print $1}')" = "$IDENTITY_BEFORE" ]
STATUS=$(mktemp /tmp/hf130-seed-status.XXXXXX.json)
timeout --signal=TERM --kill-after=20s 300s "$BIN" --config "$CFG" status-fast > "$STATUS"
python3 -I - "$STATUS" <<'PY'
import json,sys
s=json.load(open(sys.argv[1]))
assert s['ok'] is True and s['network']=='mainnet' and s['version']=='1.8.5'
assert s['height']>=24000 and s['tip_block_version']==2 and s['next_block_expected_version']==2
print('Selected canonical identity:',s['height'],s['tip_hash'])
PY
# Any configured RPC listener must stay loopback-only.
sudo ss -ltnp | python3 -I -c 'import sys; rows=[r.split() for r in sys.stdin if ":17445 " in r]; assert all(r[3].rsplit(":",1)[0] in ("127.0.0.1","[::1]","::1") for r in rows),"RPC exposed beyond loopback"'
if [ "$TEST_ACTIVE" = 1 ]; then sudo systemctl start "$TEST"; fi
trap - EXIT
printf 'HF130 %s SEED INSTALL: PASS\nExact backup: %s\n' "$REGION" "$BACKUP"
if [ "$REGION" = ams3 ]; then printf '%s\n' 'Publisher timer intentionally stopped pending explicit publication/cache/API gates.'; fi
