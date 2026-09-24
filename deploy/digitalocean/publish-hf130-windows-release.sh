#!/usr/bin/env bash
# Execute only after native/trust-policy/soak gates. Publishes the manifest last.
set -euo pipefail
[ "$(id -u)" -eq 0 ] || { echo 'Run through sudo after reviewing the staged release.' >&2; exit 1; }
STAGE=${1:?usage: publish-hf130-windows-release.sh /absolute/stage-directory}
case "$STAGE" in /tmp/qub-hf130-windows-*) ;; *) echo 'Unexpected stage directory' >&2; exit 1;; esac
[ -d "$STAGE" ] && [ ! -L "$STAGE" ]
exec 9>/run/qub-hf130-windows-publish.lock
flock -n 9 || { echo 'Another Windows publication is active' >&2; exit 75; }
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
for k,v in dict(version='1.8.5',hotfix='HF130',network='mainnet',platform='windows-x64',channel='mainnet-windows-x64',kind='installer',activation_feature='none',activation_height=0,protocol_epoch=2,protocol_epoch_2_activation_height=24000,post_activation_block_version=2,checkpoint_height=10367,checkpoint_hash='21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000',consensus_family='qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000').items(): assert m[k]==v,k
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
BACKUP=/opt/qub/releases/windows-before-hf130-$STAMP
mkdir -p "$DEST" "$BACKUP"
# Immutable versioned URL: never replace different bytes under a published name.
if [ -e "$DEST/$NAME" ]; then cmp -s "$STAGE/$NAME" "$DEST/$NAME" || { echo 'Different v1.8.5 installer already exists; do not overwrite its immutable URL' >&2; exit 1; }; fi
cp -a "$DEST" "$BACKUP/windows-x64"
for name in QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256; do
    if [ -e "$ROOT/$name" ]; then cp -a "$ROOT/$name" "$BACKUP/$name"; fi
done
printf 'Exact Windows rollback backup: %s\n' "$BACKUP"
trap 'rc=$?; if [ "$rc" -ne 0 ]; then printf "Publication failed; inspect manifest and restore from %s\n" "$BACKUP" >&2; fi' EXIT
# All validation occurs before publication. Copy to same-filesystem temp files
# then rename; readers cannot download a half-written installer.
install_file() {
    local name=$1 directory=$2
    install -m0644 "$STAGE/$name" "$directory/.hf130-$STAMP-$name"
    mv -fT "$directory/.hf130-$STAMP-$name" "$directory/$name"
}
for name in "$NAME" "$NAME.sha256" QUB-Core-Latest.exe QUB-Core-Latest.exe.sha256 SHA256SUMS.txt manifest.json.sha256; do
    install_file "$name" "$DEST"
done
install_file QUB-Core-Latest.exe "$ROOT"
install_file QUB-Core-Latest.exe.sha256 "$ROOT"
(cd "$DEST" && tr -d '\r' < "$STAGE/SHA256SUMS.txt" | sed '/  manifest.json$/d' | sha256sum -c -)
install_file manifest.json "$DEST"
(cd "$DEST" && tr -d '\r' < SHA256SUMS.txt | sha256sum -c -)
printf 'HF130 ORIGIN WINDOWS PUBLICATION: PASS\nExact previous release backup: %s\n' "$BACKUP"
