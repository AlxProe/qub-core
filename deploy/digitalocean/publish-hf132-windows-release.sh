#!/usr/bin/env bash
# Execute after native, mainnet GUI and portable-copy/trust-policy gates.
# Publishes the manifest last; failed publication restores previous live aliases.
set -euo pipefail
[ "$(id -u)" -eq 0 ] || { echo 'Run through sudo after reviewing the staged release.' >&2; exit 1; }
STAGE=${1:?usage: publish-hf132-windows-release.sh /absolute/stage-directory}
[[ "$STAGE" =~ ^/tmp/qub-hf132-windows-[A-Za-z0-9_-]+$ ]] || { echo 'Unexpected stage directory' >&2; exit 1; }
[ -d "$STAGE" ] && [ ! -L "$STAGE" ]
exec 9>/run/qub-windows-publish.lock
flock -n 9 || { echo 'Another Windows publication is active' >&2; exit 75; }
# Also serialize with the previous, unshipped HF130 publisher if it is invoked.
exec 8>/run/qub-hf130-windows-publish.lock
flock -n 8 || { echo 'An older Windows publisher is active' >&2; exit 75; }
python3 -I - "$STAGE" <<'PY'
import hashlib,json,pathlib,re,sys
p=pathlib.Path(sys.argv[1]); installer='QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
names={installer,installer+'.sha256','QUB-Core-Latest.exe','QUB-Core-Latest.exe.sha256','manifest.json','manifest.json.sha256','SHA256SUMS.txt'}
assert {f.name for f in p.iterdir()}==names, 'unexpected stage entries'
assert all(f.is_file() and not f.is_symlink() for f in p.iterdir())
entries={}
for line in (p/'SHA256SUMS.txt').read_text(encoding='ascii').splitlines():
    match=re.fullmatch(r'([0-9a-f]{64})  ([A-Za-z0-9._-]+)',line)
    assert match, 'malformed sums'
    h,n=match.groups(); assert n in names-{'SHA256SUMS.txt'} and n not in entries
    assert hashlib.sha256((p/n).read_bytes()).hexdigest()==h, n
    entries[n]=h
assert set(entries)==names-{'SHA256SUMS.txt'}
m=json.loads((p/'manifest.json').read_text(encoding='utf-8-sig'))
for k,v in dict(version='1.8.5',hotfix='HF132',network='mainnet',platform='windows-x64',channel='mainnet-windows-x64',kind='installer',activation_feature='none',activation_height=0,protocol_epoch=2,protocol_epoch_2_activation_height=24000,post_activation_block_version=2,checkpoint_height=10367,checkpoint_hash='21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000',consensus_family='qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000').items(): assert m[k]==v,k
assert all(type(m[k]) is int for k in ('activation_height','protocol_epoch','protocol_epoch_2_activation_height','post_activation_block_version','checkpoint_height','size'))
assert m['mandatory'] is True and m['chain_upgrade'] is False
assert m['url']=='https://download.qubit-coin.io/mainnet/windows-x64/'+installer
assert m['latest_url']=='https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe'
assert m['sha256']==entries[installer]==entries['QUB-Core-Latest.exe']
assert m['size']==(p/installer).stat().st_size and m['size']>0
for n in (installer,'QUB-Core-Latest.exe','manifest.json'):
    assert (p/(n+'.sha256')).read_text(encoding='ascii').strip()==entries[n],n
assert (p/installer).open('rb').read(2)==b'MZ'
print('Staged payload, manifest and sidecars verified; Signature or explicit NotSigned evidence comes from the Windows trust-policy gate.')
PY
ROOT=/srv/qub-updates/mainnet
DEST=$ROOT/windows-x64
NAME=QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe
STAMP=$(date -u +%Y%m%d-%H%M%S)-$$
BACKUP=/opt/qub/releases/windows-before-hf132-$STAMP
[ ! -L "$ROOT" ] && [ ! -L "$DEST" ]
mkdir -p "$DEST" "$BACKUP"
# Immutable versioned URL: never replace different bytes under a published name.
if [ -e "$DEST/$NAME" ]; then cmp -s "$STAGE/$NAME" "$DEST/$NAME" || { echo 'Different v1.8.5 installer already exists; do not overwrite its immutable URL' >&2; exit 1; }; fi
cp -a "$DEST" "$BACKUP/windows-x64"
for name in QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256; do
    if [ -e "$ROOT/$name" ]; then cp -a "$ROOT/$name" "$BACKUP/$name"; fi
done
printf 'Exact Windows rollback backup: %s\n' "$BACKUP"
# All validation occurs before publication. Copy to same-filesystem temp files
# then rename; readers cannot download a half-written installer.
atomic_file() {
    local source=$1 destination=$2 temporary
    [ -f "$source" ] && [ ! -L "$source" ] && [ ! -L "$destination" ] || return 1
    temporary=$(mktemp "${destination}.hf132.XXXXXX") || return $?
    if ! install -m0644 -- "$source" "$temporary"; then rm -f -- "$temporary"; return 1; fi
    if ! mv -fT -- "$temporary" "$destination"; then rm -f -- "$temporary"; return 1; fi
}
restore_alias() {
    local name=$1 saved=$2 live=$3
    if [ -f "$saved/$name" ]; then atomic_file "$saved/$name" "$live/$name"; else rm -f -- "$live/$name"; fi
}
recover_publication() {
    local rc=$? restore_rc
    trap - EXIT
    if [ "$rc" -ne 0 ]; then
        set +e
        (
            set -e
            for name in QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256 SHA256SUMS.txt manifest.json.sha256; do
                restore_alias "$name" "$BACKUP/windows-x64" "$DEST"
            done
            for name in QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256; do restore_alias "$name" "$BACKUP" "$ROOT"; done
            # Restore the previous release pointer after its payload aliases.
            restore_alias manifest.json "$BACKUP/windows-x64" "$DEST"
        )
        restore_rc=$?
        if [ "$restore_rc" = 0 ]; then
            printf 'Publication failed; previous live aliases restored from %s. Verify public/cache results.\n' "$BACKUP" >&2
        else
            printf 'Publication and automatic restoration failed. Preserve and restore exact backup: %s\n' "$BACKUP" >&2
        fi
    fi
    exit "$rc"
}
trap recover_publication EXIT
install_file() {
    local name=$1 directory=$2
    atomic_file "$STAGE/$name" "$directory/$name"
}
for name in "$NAME" "$NAME.sha256" QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256 SHA256SUMS.txt manifest.json.sha256; do
    install_file "$name" "$DEST"
done
install_file QUB-Core-Latest.exe "$ROOT"
install_file QUB-Core-Latest.exe.sha256 "$ROOT"
(cd "$DEST" && tr -d '\r' < "$STAGE/SHA256SUMS.txt" | sed '/  manifest.json$/d' | sha256sum -c -)
install_file manifest.json "$DEST"
(cd "$DEST" && tr -d '\r' < SHA256SUMS.txt | sha256sum -c -)
printf 'HF132 ORIGIN WINDOWS PUBLICATION: PASS\nExact previous release backup: %s\n' "$BACKUP"
