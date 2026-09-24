#!/usr/bin/env bash
# Installs a separately versioned read-only API binary; seed binary is untouched.
set -euo pipefail
EXPECTED=${1:?usage: install-hf130-explorer-api.sh EXPECTED_BINARY_SHA256}
[[ "$EXPECTED" =~ ^[0-9a-f]{64}$ ]]
SRC=/opt/jinex/staging/src/qubd
SEED_UNIT=qub-seed-mainnet.service
API_UNIT=qub-explorer-api-mainnet.service
API_BIN=/opt/qub/bin/qubd-explorer-api-hf130
CFG=/opt/qub/mainnet/mainnet-seed.toml
DROPIN=/etc/systemd/system/$API_UNIT.d/hf130.conf
BACKUP=/opt/qub/bin/backups/hf130-explorer-api-$(date -u +%Y%m%d-%H%M%S)-$$
sudo -n true
python3 "$SRC/scripts/test-hf130-source-contracts.py" --mode checkout --root "$SRC"
[ "$(sha256sum "$SRC/target/release/qubd" | awk '{print $1}')" = "$EXPECTED" ]
systemctl is-active --quiet "$API_UNIT"
SEED_PID=$(systemctl show -p MainPID --value "$SEED_UNIT")
[ "$SEED_PID" -gt 0 ]
SEED_HASH=$(sudo sha256sum /proc/$SEED_PID/exe | awk '{print $1}')
sudo install -d -m0755 -o root -g root "$BACKUP"
if sudo test -f "$DROPIN"; then sudo cp -a "$DROPIN" "$BACKUP/hf130.conf"; fi
if sudo test -f "$API_BIN"; then sudo cp -a "$API_BIN" "$BACKUP/qubd-explorer-api-hf130"; fi
systemctl cat "$API_UNIT" | sudo tee "$BACKUP/effective-unit.txt" >/dev/null
printf 'Exact API rollback backup: %s\n' "$BACKUP"
recover() {
    rc=$?; trap - EXIT
    if [ "$rc" -ne 0 ]; then
        set +e
        sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$API_UNIT"
        if [ "$(systemctl show -p MainPID --value "$API_UNIT")" != 0 ]; then
            printf 'API recovery stopped: process still active; no executable overwrite attempted. Backup: %s\n' "$BACKUP" >&2
            exit "$rc"
        fi
        if sudo test -f "$BACKUP/hf130.conf"; then sudo install -m0644 "$BACKUP/hf130.conf" "$DROPIN"; else sudo rm -f "$DROPIN"; fi
        if sudo test -f "$BACKUP/qubd-explorer-api-hf130"; then sudo install -m0755 "$BACKUP/qubd-explorer-api-hf130" "$API_BIN"; fi
        sudo systemctl daemon-reload
        sudo systemctl start "$API_UNIT"
        printf 'API install failed; recovery attempted from %s. Verify API health.\n' "$BACKUP" >&2
    fi
    exit "$rc"
}
trap recover EXIT
sudo timeout --signal=TERM --kill-after=10s 60s systemctl stop "$API_UNIT"
[ "$(systemctl show -p MainPID --value "$API_UNIT")" = 0 ]
sudo install -m0755 -o root -g root "$SRC/target/release/qubd" "$API_BIN"
sudo install -d /etc/systemd/system/$API_UNIT.d
sudo tee "$DROPIN" >/dev/null <<UNIT
[Service]
# Enforce read-only canonical data even if an exceptional loader fallback runs.
ReadWritePaths=
ReadOnlyPaths=/opt/qub
ExecStart=
ExecStart=$API_BIN --config $CFG explorer-api 127.0.0.1:18765
UNIT
sudo systemctl daemon-reload
sudo systemctl start "$API_UNIT"
for attempt in $(seq 1 60); do
    if systemctl is-active --quiet "$API_UNIT" && curl --fail --silent --show-error --connect-timeout 1 --max-time 2 http://127.0.0.1:18765/api/v1/ >/dev/null; then break; fi
    [ "$attempt" -lt 60 ]; sleep 2
done
API_PID=$(systemctl show -p MainPID --value "$API_UNIT")
[ "$(sudo sha256sum /proc/$API_PID/exe | awk '{print $1}')" = "$EXPECTED" ]
[ "$(systemctl show -p MainPID --value "$SEED_UNIT")" = "$SEED_PID" ]
[ "$(sudo sha256sum /proc/$SEED_PID/exe | awk '{print $1}')" = "$SEED_HASH" ]
trap - EXIT
printf 'HF130 VERSIONED EXPLORER API INSTALL: PASS\nExact backup: %s\n' "$BACKUP"
