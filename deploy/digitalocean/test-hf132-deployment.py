"""Actual isolated Linux dist metadata checks, not native/production evidence."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys
import tempfile
import os
import shutil

SHELL = Path(__file__).resolve().parent

def run(args, **kwargs):
    return subprocess.run(list(map(str, args)), text=True, capture_output=True, **kwargs)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def embedded(text):
    return re.findall(r"<<'PY'\n(.*?)\nPY(?:\n|$)", text, re.S)

with tempfile.TemporaryDirectory(prefix='hf132-r2-deploy-fixtures-') as tmp:
    tmp = Path(tmp)
    # Execute the real Linux dist validator against seven-file test payloads.
    validator = embedded((SHELL / 'publish-hf132-windows-release.sh').read_text())[0]
    installer = 'QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
    fields = dict(version='1.8.5', hotfix='HF132', network='mainnet', platform='windows-x64',
                  channel='mainnet-windows-x64', kind='installer', activation_feature='none',
                  activation_height=0, protocol_epoch=2, protocol_epoch_2_activation_height=24000,
                  post_activation_block_version=2, checkpoint_height=10367,
                  checkpoint_hash='21dac61d5bd98053420870a68f323da4ba84145263921036504a8a9706000000',
                  consensus_family='qns-1000-jin-5555-qnsminer-8305-jinconv-8305-pools-9999-poolrename-forkcp-10367-daa2-10500-library-10550-blast-10600-jinsale-10720-qubjin-16777-verifiedgov-21000-epoch2-24000',
                  mandatory=True, chain_upgrade=False,
                  url='https://download.qubit-coin.io/mainnet/windows-x64/' + installer,
                  latest_url='https://download.qubit-coin.io/mainnet/windows-x64/QUB-Core-Latest.exe')
    def payload(directory, changes=None):
        directory.mkdir()
        body = b'MZfixture-only-not-a-real-Windows-executable'
        for name in (installer, 'QUB-Core-Latest.exe'):
            (directory / name).write_bytes(body)
        manifest = dict(fields, size=len(body), sha256=hashlib.sha256(body).hexdigest())
        manifest.update(changes or {})
        (directory / 'manifest.json').write_text(json.dumps(manifest))
        for name in (installer, 'QUB-Core-Latest.exe', 'manifest.json'):
            (directory / (name + '.sha256')).write_text(digest(directory / name) + '\n')
        (directory / 'SHA256SUMS.txt').write_text(''.join(digest(p) + '  ' + p.name + '\n' for p in sorted(directory.iterdir())))
    cases = [
        ('valid', {}, None, True),
        ('wrong-version', {'version':'1.8.4'}, None, False),
        ('old-hotfix', {'hotfix':'HF130'}, None, False),
        ('chain-upgrade', {'chain_upgrade':True}, None, False),
        ('epoch-height', {'protocol_epoch_2_activation_height':25000}, None, False),
        ('http-url', {'url':fields['url'].replace('https:', 'http:')}, None, False),
        ('cross-origin', {'url':fields['url'].replace('download.qubit-coin.io', 'example.invalid')}, None, False),
        ('bool-size', {'size':True}, None, False),
        ('consensus-change', {'protocol_epoch':3}, None, False),
        ('corrupt-installer', {}, lambda p:(p/installer).write_bytes(b'MZtampered'), False),
        ('extra-file', {}, lambda p:(p/'unexpected.txt').write_text('extra'), False),
        ('missing-sidecar', {}, lambda p:(p/'manifest.json.sha256').unlink(), False),
    ]
    for name, changes, mutation, expected in cases:
        directory = tmp / name
        payload(directory, changes)
        if mutation: mutation(directory)
        result = run([sys.executable, '-I', '-', directory], input=validator)
        assert (result.returncode == 0) == expected, (name, result.stdout, result.stderr)
        print('Linux Windows-payload validator:', name, 'PASS' if expected else 'correctly rejected')
print('HF132 Linux publication metadata: 12 fixture cases passed.')

# Exercise the real replacement functions on disposable local files. No host
# service, /opt, /srv, deployment configuration, or canonical data is touched.
if os.name != 'posix' or shutil.which('bash') is None:
    raise SystemExit('Atomic/service helper tests require the Linux staging host; no Windows Bash requirement.')
common = (SHELL / 'hf132-release-common.sh').read_text()
publication = (SHELL / 'publish-hf132-windows-release.sh').read_text()
atomic = publication.split('atomic_file() {', 1)[1].split('\nrestore_alias() {', 1)[0]
atomic = 'atomic_file() {' + atomic

def bash(body, *args):
    return run(['bash', '-c', 'set -euo pipefail\n' + body, 'hf132-test', *args])

def sudo_stub(fail=''):
    # Strip ownership options for a non-root Linux CI user, retaining the real
    # install/mv/mktemp operations. A selected operation can fail before mutation.
    return '''
sudo() {
    local command=$1; shift
    if [ "$command" = "%s" ]; then return 77; fi
    if [ "$command" = install ]; then
        local filtered=()
        while [ "$#" -gt 0 ]; do
            case "$1" in -o|-g) shift 2;; *) filtered+=("$1"); shift;; esac
        done
        command install "${filtered[@]}"
    else command "$command" "$@"; fi
}
''' % fail

checks = 0
with tempfile.TemporaryDirectory(prefix='hf132-deployment-atomic-') as temp:
    directory = Path(temp)
    source = directory / 'candidate'
    destination = directory / 'live'
    source.write_bytes(b'new-complete-executable')
    for label, function, call in (
        ('seed/API', common, 'hf132_atomic_install "$1" "$2" 0755'),
        ('public payload', atomic, 'atomic_file "$1" "$2"'),
    ):
        destination.write_bytes(b'old-complete-executable')
        with destination.open('rb') as existing_reader:
            result = bash(function + sudo_stub() + '\n' + call, source, destination)
            assert result.returncode == 0, (label, result.stderr)
            assert destination.read_bytes() == source.read_bytes()
            assert existing_reader.read() == b'old-complete-executable'
        checks += 1
        for failure in ('install', 'mv'):
            destination.write_bytes(b'old-complete-executable')
            stub = sudo_stub(failure) if label == 'seed/API' else '\n' + failure + '() { return 77; }\n'
            result = bash(function + stub + '\n' + call, source, destination)
            assert result.returncode != 0, (label, failure)
            assert destination.read_bytes() == b'old-complete-executable'
            assert not list(directory.glob('*.hf132.*')), (label, failure, 'temporary leak')
            checks += 1
        destination.unlink()
        destination.symlink_to(source)
        result = bash(function + sudo_stub() + '\n' + call, source, destination)
        assert result.returncode != 0 and destination.is_symlink()
        destination.unlink()
        checks += 1

for pid, active, expected in ((42, True, True), (142, True, False), (42, False, False), (0, True, False)):
    stub = r'''
systemctl() {
    if [ "$1" = is-active ]; then [ "$TEST_ACTIVE" = 1 ]; else printf '%s\n' "$TEST_PID"; fi
}
sudo() { "$@"; }
ss() {
    [ "$*" = '-H -ltnp sport = :17444' ] || return 1
    printf '%s\n' 'LISTEN 0 128 0.0.0.0:17444 0.0.0.0:* users:(("qubd",pid=42,fd=7))'
}
'''
    result = bash(common + stub + f'\nTEST_PID={pid}; TEST_ACTIVE={int(active)}\nhf132_unit_owns_listener seed.service 17444')
    assert (result.returncode == 0) == expected, (pid, active, result.stderr)
    checks += 1

for stop_result, pid, active, expected in ((0, 0, 0, True), (1, 0, 0, False), (0, 42, 0, False), (0, 0, 1, False)):
    stub = r'''
sudo() { [ "$1" = timeout ] && return "$TEST_STOP_RESULT"; return 1; }
systemctl() {
    if [ "$1" = is-active ]; then [ "$TEST_ACTIVE" = 1 ]; else printf '%s\n' "$TEST_PID"; fi
}
'''
    result = bash(common + stub + f'\nTEST_STOP_RESULT={stop_result}; TEST_PID={pid}; TEST_ACTIVE={active}\nhf132_stop_unit seed.service')
    assert (result.returncode == 0) == expected, (stop_result, pid, active, result.stderr)
    checks += 1

print(f'HF132 atomic/service helper fixtures: {checks} cases passed.')

# Exercise the actual publisher EXIT handler, including its nonzero exit and
# restoration order, after a simulated publication error in a disposable tree.
recovery = 'atomic_file() {' + publication.split('atomic_file() {', 1)[1].split('\nfor name in "$NAME"', 1)[0]
for restoration_fails in (False, True):
    with tempfile.TemporaryDirectory(prefix='hf132-publication-recovery-') as temp:
        root = Path(temp)
        saved = root / 'backup'
        live = root / 'live'
        destination = live / 'windows-x64'
        (saved / 'windows-x64').mkdir(parents=True)
        destination.mkdir(parents=True)
        aliases = ('QUB-Core-Latest.exe', 'QUB-Core-Latest.exe.sha256', 'SHA256SUMS.txt', 'manifest.json.sha256', 'manifest.json')
        for name in aliases:
            (saved / 'windows-x64' / name).write_text('previous ' + name)
            (destination / name).write_text('new ' + name)
        for name in aliases[:2]:
            (saved / name).write_text('previous root ' + name)
            (live / name).write_text('new root ' + name)
        immutable = destination / 'QUB-Core-v1.8.5-Windows-x64-mainnet-Setup.exe'
        immutable.write_bytes(b'MZnew-versioned-payload')
        failure = '\ninstall() { return 77; }\n' if restoration_fails else ''
        result = bash('ROOT=$1; DEST=$2; BACKUP=$3\n' + recovery + failure + '\nfalse', live, destination, saved)
        assert result.returncode != 0
        assert immutable.read_bytes() == b'MZnew-versioned-payload'
        if restoration_fails:
            assert 'Publication and automatic restoration failed.' in result.stderr
        else:
            assert 'previous live aliases restored' in result.stderr
            for name in aliases:
                assert (destination / name).read_text() == 'previous ' + name
            for name in aliases[:2]:
                assert (live / name).read_text() == 'previous root ' + name
        checks += 1

print('HF132 publication recovery: 2 fixture cases passed.')
print('HF132 deployment fixtures: PASS (30 cases; no native/signature/live-service/production execution).')
