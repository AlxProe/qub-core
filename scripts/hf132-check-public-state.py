#!/usr/bin/env python3
"""Read-only HF132 snapshot/API checks; no new block or manual lab is required."""

import argparse
import hashlib
import json
import queue
import re
import struct
import sys
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

SNAPSHOTS = 'https://download.qubit-coin.io/mainnet/snapshots/'
API = 'https://api.qubit-coin.io/api/v1/'
MAX_AGE = 180
MAX_ATTEMPTS = 3
TOTAL_TIMEOUT = 90
REQUEST_TIMEOUT = 10
HASH = re.compile(r'[0-9a-f]{64}')
LIMITS = {'tip.json': 64 * 1024, 'tail-64.json': 64 * 1024 * 1024}


class CheckFailed(RuntimeError):
    pass


def require(condition, message):
    if not condition:
        raise CheckFailed(message)


def integer(value, name, minimum=0):
    require(type(value) is int and value >= minimum, 'invalid integer: ' + name)
    return value


def hash_value(value, name):
    require(isinstance(value, str) and HASH.fullmatch(value), 'invalid hash: ' + name)
    return value


def parse_json(data, name):
    try:
        value = json.loads(data)
    except (ValueError, UnicodeError) as error:
        raise CheckFailed('invalid JSON: ' + name) from error
    require(isinstance(value, dict), 'expected JSON object: ' + name)
    return value


def verify_sidecar(data, sidecar, name):
    try:
        expected = sidecar.decode('ascii').strip()
    except UnicodeError as error:
        raise CheckFailed('invalid sidecar encoding: ' + name) from error
    actual = hashlib.sha256(data).hexdigest()
    require(expected == actual + '  ' + name, 'checksum/filename mismatch: ' + name)


def verify_tip(tip, now):
    require(tip.get('network') == 'mainnet', 'snapshot is not mainnet')
    integer(tip.get('height'), 'height', 24000)
    hash_value(tip.get('tip_hash'), 'tip_hash')
    hash_value(tip.get('chain_sha256'), 'chain_sha256')
    require(tip.get('source') == 'fce', 'snapshot source is not fce')
    require(tip.get('recovered_from_previous') is False, 'snapshot used PREVIOUS recovery')
    integer(tip.get('generation'), 'generation', 1)
    integer(tip.get('state_revision'), 'state_revision')
    integer(tip.get('committed_at_unix'), 'committed_at_unix', 1)
    published = integer(tip.get('published_at_unix'), 'published_at_unix', 1)
    require(-30 <= now - published < MAX_AGE, 'snapshot publication is stale or in the future')


def verify_tail(tail, tip):
    require(set(tail) == {'network', 'start_height', 'tip_hash', 'tip_height', 'blocks'}, 'tail-64 schema mismatch')
    require(tail['network'] == 'mainnet', 'tail is not mainnet')
    require(type(tail['tip_height']) is int and tail['tip_height'] == tip['height'], 'tail height differs from tip')
    require(tail['tip_hash'] == tip['tip_hash'], 'tail hash differs from tip')
    start = integer(tail['start_height'], 'tail start_height')
    blocks = tail['blocks']
    require(isinstance(blocks, list) and len(blocks) == 64, 'tail-64 must contain 64 blocks')
    require(start + len(blocks) - 1 == tip['height'], 'tail height range is inconsistent')
    previous = None
    for offset, block in enumerate(blocks):
        require(isinstance(block, dict) and isinstance(block.get('header'), dict), 'invalid tail block header')
        require(isinstance(block.get('transactions'), list), 'invalid tail transactions schema')
        header = block['header']
        values = [integer(header.get(name), name) for name in ('version', 'time', 'bits', 'nonce')]
        require(all(value <= 0xffffffff for value in values), 'header integer exceeds u32')
        require(values[0] == (2 if start + offset >= 24000 else 1), 'tail epoch block version differs')
        parent = hash_value(header.get('prev_block_hash'), 'prev_block_hash')
        merkle = hash_value(header.get('merkle_root'), 'merkle_root')
        if previous is not None:
            require(parent == previous, 'tail header hash-link mismatch')
        encoded = struct.pack('<I', values[0]) + bytes.fromhex(parent) + bytes.fromhex(merkle) + struct.pack('<III', *values[1:])
        previous = hashlib.sha256(hashlib.sha256(encoded).digest()).hexdigest()
    require(previous == tip['tip_hash'], 'tail last header hash differs from advertised tip')


def inspect_snapshot(payloads, now):
    for name in ('tip.json', 'tail-64.json'):
        verify_sidecar(payloads[name], payloads[name + '.sha256'], name)
    tip = parse_json(payloads['tip.json'], 'tip.json')
    verify_tip(tip, now)
    verify_tail(parse_json(payloads['tail-64.json'], 'tail-64.json'), tip)
    return tip


def check_snapshot_directory(directory, now=None):
    # Resolve once: subsequent reads stay in this immutable generation even if
    # the publisher switches the public .current link during this check.
    pinned = Path(directory).resolve(strict=True)
    require(pinned.is_dir(), 'snapshot generation is not a directory')
    payloads = {}
    for name in ('tip.json', 'tip.json.sha256', 'tail-64.json', 'tail-64.json.sha256'):
        path = pinned / name
        require(path.is_file() and not path.is_symlink(), 'not a regular generation file: ' + name)
        require(path.stat().st_size <= LIMITS.get(name, 1024), 'generation file exceeds inspection bound: ' + name)
        payloads[name] = path.read_bytes()
    tip = inspect_snapshot(payloads, time.time() if now is None else now)
    # Also verify the complete local chain bytes without loading its JSON tree.
    chain_path = pinned / 'chain.json'
    sidecar_path = pinned / 'chain.json.sha256'
    require(chain_path.is_file() and not chain_path.is_symlink(), 'missing regular chain.json')
    require(sidecar_path.is_file() and not sidecar_path.is_symlink(), 'missing regular chain sidecar')
    digest = hashlib.sha256()
    with chain_path.open('rb') as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b''):
            digest.update(chunk)
    require(digest.hexdigest() == tip['chain_sha256'], 'local chain bytes differ from tip chain_sha256')
    require(sidecar_path.read_text(encoding='ascii').strip() == digest.hexdigest() + '  chain.json', 'local chain sidecar mismatch')
    return {'ok': True, 'mode': 'origin-generation', 'directory': str(pinned), 'snapshot': tip,
            'new_block_observation_required': False}


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        raise CheckFailed('redirect rejected for ordinary official URL: ' + req.full_url)


def fetch_official(url, maximum, deadline):
    require(url.startswith(SNAPSHOTS) or url.startswith(API), 'non-official URL')
    require('?' not in url and '#' not in url, 'query/fragment is forbidden')
    remaining = min(REQUEST_TIMEOUT, deadline - time.monotonic())
    require(remaining > 0, 'public-check deadline exhausted')
    result = queue.Queue(maxsize=1)

    def work():
        try:
            # Default HTTPS handling verifies certificates. No redirect, custom
            # unverified context, query nonce, or cache-bypassing request header.
            opener = urllib.request.build_opener(NoRedirect())
            with opener.open(url, timeout=remaining) as response:
                require(response.status == 200, 'HTTP response is not 200')
                headers = {}
                for key, value in response.headers.items():
                    key = key.lower()
                    headers[key] = headers[key] + ', ' + value if key in headers else value
                data = bytearray()
                while True:
                    require(time.monotonic() < deadline, 'public-check deadline exhausted')
                    part = response.read1(min(65536, maximum + 1 - len(data)))
                    if not part:
                        break
                    data.extend(part)
                    require(len(data) <= maximum, 'response exceeds inspection bound: ' + url)
                result.put((bytes(data), headers))
        except Exception as error:
            result.put(error)

    # A stalled platform DNS lookup must not make the operator helper hang.
    # At most one timed-out daemon request is created per bounded retry.
    threading.Thread(target=work, daemon=True).start()
    try:
        value = result.get(timeout=remaining)
    except queue.Empty as error:
        raise CheckFailed('request deadline: ' + url) from error
    if isinstance(value, Exception):
        raise CheckFailed(str(value)) from value
    return value


def check_cache(headers, url):
    controls = [part.strip().lower() for part in headers.get('cache-control', '').split(',')]
    require('no-store' in controls, 'missing Cache-Control no-store: ' + url)
    for part in controls:
        key, separator, value = part.partition('=')
        if key.strip() in {'max-age', 's-maxage'}:
            require(separator and value.strip().strip('"') == '0', 'positive cache max-age: ' + url)
    if 'age' in headers:
        require(headers['age'].strip().isdigit() and int(headers['age']) == 0, 'cached response Age is nonzero: ' + url)
    statuses = {part.strip().upper() for part in headers.get('cf-cache-status', '').split(',')}
    require(not statuses.intersection({'HIT', 'STALE', 'UPDATING'}), 'cached/stale Cloudflare response: ' + url)


def check_public(fetch=fetch_official, now=None):
    deadline = time.monotonic() + TOTAL_TIMEOUT
    failures = []
    observed_headers = {}

    def get(url, maximum=4 * 1024 * 1024):
        body, headers = fetch(url, maximum, deadline)
        headers = {key.lower(): value for key, value in headers.items()}
        check_cache(headers, url)
        observed_headers[url] = {key: headers[key] for key in ('cache-control', 'age', 'cf-cache-status', 'date') if key in headers}
        return body

    for attempt in range(1, MAX_ATTEMPTS + 1):
        try:
            require(time.monotonic() < deadline, 'public-check deadline exhausted')
            payloads = {}
            for name in ('tip.json', 'tip.json.sha256', 'tail-64.json', 'tail-64.json.sha256'):
                payloads[name] = get(SNAPSHOTS + name, LIMITS.get(name, 1024))
            after_tip = get(SNAPSHOTS + 'tip.json', LIMITS['tip.json'])
            require(payloads['tip.json'] == after_tip, 'snapshot generation changed during request bracket')
            tip = inspect_snapshot(payloads, time.time() if now is None else now)
            root = parse_json(get(API), 'API root')
            require(root.get('version') == '1.8.5', 'API root version differs from 1.8.5')
            before = parse_json(get(API + 'status-fast'), 'API status-fast')
            summary = parse_json(get(API + 'summary'), 'API summary')
            block = parse_json(get(API + 'block/' + str(tip['height'])), 'API snapshot-height block')
            after = parse_json(get(API + 'status-fast'), 'API status-fast')
            for status in (before, after):
                require(status.get('ok') is True and status.get('network') == 'mainnet' and status.get('version') == '1.8.5', 'API status/version/network mismatch')
                integer(status.get('height'), 'API height', 24000)
                hash_value(status.get('tip_hash'), 'API tip_hash')
                require(type(status.get('tip_block_version')) is int and status['tip_block_version'] == 2, 'API tip block version differs')
                require(type(status.get('next_block_expected_version')) is int and status['next_block_expected_version'] == 2, 'API next block version differs')
            require((before['height'], before['tip_hash']) == (after['height'], after['tip_hash']), 'API tip changed during request bracket')
            require(summary.get('network') == 'mainnet' and type(summary.get('height')) is int and
                    (summary['height'], summary.get('bestblockhash')) == (after['height'], after['tip_hash']), 'API summary differs from stable status')
            require(after['height'] >= tip['height'], 'API is behind the published snapshot')
            require(block.get('network') == 'mainnet' and type(block.get('height')) is int and
                    (block['height'], block.get('hash')) == (tip['height'], tip['tip_hash']), 'snapshot hash is not the API canonical block at its height')
            # No block-age threshold: a quiet mainnet still has a valid selected
            # head. Publication age is checked independently above.
            return {'ok': True, 'mode': 'public', 'attempt': attempt, 'snapshot': tip,
                    'api': {'height': after['height'], 'tip_hash': after['tip_hash'], 'version': after['version']},
                    'headers': observed_headers, 'new_block_observation_required': False,
                    'previous_attempts': failures}
        except (CheckFailed, OSError, ValueError, KeyError, TypeError) as error:
            failures.append(str(error))
            if time.monotonic() >= deadline:
                break
    raise CheckFailed('public snapshot/API checks failed after bounded attempts: ' + ' | '.join(failures))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group(required=True)
    mode.add_argument('--snapshot-dir', type=Path, help='pin and inspect an existing immutable generation or .current directory')
    mode.add_argument('--public', action='store_true', help='check ordinary official snapshot and API URLs with verified TLS')
    args = parser.parse_args()
    try:
        report = check_public() if args.public else check_snapshot_directory(args.snapshot_dir)
        print(json.dumps(report, indent=2, sort_keys=True))
        return 0
    except (CheckFailed, OSError, ValueError, KeyError, TypeError) as error:
        print('HF132 PUBLIC STATE: FAIL: ' + str(error), file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
