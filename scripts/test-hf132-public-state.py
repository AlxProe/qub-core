#!/usr/bin/env python3
"""Fixture checks for the read-only public checker; never contacts mainnet."""

import hashlib
import importlib.util
import io
import json
import struct
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch

sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location('hf132_public_state', Path(__file__).with_name('hf132-check-public-state.py'))
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)
NOW = 1_789_330_000


def encoded(value):
    return (json.dumps(value, separators=(',', ':')) + '\n').encode()


def fixture():
    blocks = []
    parent = '00' * 32
    height = 25000
    for number in range(height - 63, height + 1):
        merkle = hashlib.sha256(str(number).encode()).hexdigest()
        header = {'version': 2, 'prev_block_hash': parent, 'merkle_root': merkle,
                  'time': NOW - 10000 + number, 'bits': 0x1f00ffff, 'nonce': number}
        raw = struct.pack('<I', 2) + bytes.fromhex(parent) + bytes.fromhex(merkle) + struct.pack('<III', header['time'], header['bits'], number)
        parent = hashlib.sha256(hashlib.sha256(raw).digest()).hexdigest()
        blocks.append({'header': header, 'transactions': []})
    chain = encoded({'network': 'mainnet', 'blocks': blocks})
    tip = {'network': 'mainnet', 'height': height, 'tip_hash': parent,
           'chain_sha256': hashlib.sha256(chain).hexdigest(), 'published_at_unix': NOW,
           'source': 'fce', 'generation': 13, 'state_revision': 27,
           'committed_at_unix': NOW - 10000, 'recovered_from_previous': False}
    tail = {'network': 'mainnet', 'start_height': height - 63,
            'tip_hash': parent, 'tip_height': height, 'blocks': blocks}
    payloads = {'chain.json': chain, 'tip.json': encoded(tip), 'tail-64.json': encoded(tail)}
    for name in list(payloads):
        payloads[name + '.sha256'] = (hashlib.sha256(payloads[name]).hexdigest() + '  ' + name + '\n').encode()
    status = {'ok': True, 'version': '1.8.5', 'network': 'mainnet', 'height': height,
              'tip_hash': parent, 'tip_block_version': 2, 'next_block_expected_version': 2}
    api = {'': {'version': '1.8.5'}, 'status-fast': status,
           'summary': {'network': 'mainnet', 'height': height, 'bestblockhash': parent},
           'block/' + str(height): {'network': 'mainnet', 'height': height, 'hash': parent}}
    return payloads, api


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.payloads, self.api = fixture()
        self.headers = {'Cache-Control': 'no-store, max-age=0', 'CF-Cache-Status': 'DYNAMIC'}
        self.calls = []

    def fetch(self, url, maximum, deadline):
        self.calls.append(url)
        self.assertNotIn('?', url)
        self.assertGreater(deadline, time.monotonic())
        if url.startswith(CHECK.SNAPSHOTS):
            body = self.payloads[url.removeprefix(CHECK.SNAPSHOTS)]
        else:
            body = encoded(self.api[url.removeprefix(CHECK.API)])
        self.assertLessEqual(len(body), maximum)
        return body, self.headers

    def update_json(self, name, change):
        value = json.loads(self.payloads[name])
        change(value)
        self.payloads[name] = encoded(value)
        self.payloads[name + '.sha256'] = (hashlib.sha256(self.payloads[name]).hexdigest() + '  ' + name + '\n').encode()

    def check(self):
        return CHECK.check_public(fetch=self.fetch, now=NOW)

    def test_quiet_mainnet_with_old_commit_and_fresh_publication_passes(self):
        result = self.check()
        self.assertTrue(result['ok'])
        self.assertFalse(result['new_block_observation_required'])

    def test_newer_api_tip_accepts_matching_snapshot_ancestor(self):
        self.api['status-fast']['height'] += 1
        self.api['status-fast']['tip_hash'] = 'ab' * 32
        self.api['summary']['height'] += 1
        self.api['summary']['bestblockhash'] = 'ab' * 32
        self.assertTrue(self.check()['ok'])

    def test_snapshot_ancestor_must_match_api_block(self):
        self.api['block/25000']['hash'] = 'ab' * 32
        with self.assertRaisesRegex(CHECK.CheckFailed, 'not the API canonical block'):
            self.check()

    def test_tail_header_corruption_is_rejected_even_with_matching_sidecar(self):
        self.update_json('tail-64.json', lambda value: value['blocks'][20]['header'].__setitem__('nonce', 7))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'hash-link mismatch'):
            self.check()

    def test_mismatched_tail_generation_is_rejected(self):
        self.update_json('tail-64.json', lambda value: value.__setitem__('tip_hash', 'ab' * 32))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'tail hash differs'):
            self.check()

    def test_wrong_sidecar_filename_is_rejected(self):
        self.payloads['tip.json.sha256'] = self.payloads['tip.json.sha256'].replace(b'tip.json', b'other.json')
        with self.assertRaisesRegex(CHECK.CheckFailed, 'checksum/filename mismatch'):
            self.check()

    def test_stale_publication_fails(self):
        self.update_json('tip.json', lambda value: value.__setitem__('published_at_unix', NOW - 180))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'stale'):
            self.check()

    def test_future_publication_fails(self):
        self.update_json('tip.json', lambda value: value.__setitem__('published_at_unix', NOW + 31))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'future'):
            self.check()

    def test_previous_recovery_fails(self):
        self.update_json('tip.json', lambda value: value.__setitem__('recovered_from_previous', True))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'PREVIOUS'):
            self.check()

    def test_legacy_source_fails(self):
        self.update_json('tip.json', lambda value: value.__setitem__('source', 'legacy'))
        with self.assertRaisesRegex(CHECK.CheckFailed, 'not fce'):
            self.check()

    def test_old_api_version_fails(self):
        self.api['status-fast']['version'] = '1.8.4'
        with self.assertRaisesRegex(CHECK.CheckFailed, 'status/version/network'):
            self.check()

    def test_stale_summary_fails(self):
        self.api['summary']['height'] -= 1
        with self.assertRaisesRegex(CHECK.CheckFailed, 'summary differs'):
            self.check()

    def test_cache_control_is_required(self):
        self.headers = {'Cache-Control': 'public, max-age=60'}
        with self.assertRaisesRegex(CHECK.CheckFailed, 'missing Cache-Control no-store'):
            self.check()

    def test_conflicting_positive_max_age_fails(self):
        self.headers['Cache-Control'] = 'no-store, max-age=60'
        with self.assertRaisesRegex(CHECK.CheckFailed, 'positive cache max-age'):
            self.check()

    def test_nonzero_age_fails(self):
        self.headers['Age'] = '50'
        with self.assertRaisesRegex(CHECK.CheckFailed, 'Age is nonzero'):
            self.check()

    def test_snapshot_switch_retries_and_recovers(self):
        count = 0

        def switching(url, maximum, deadline):
            nonlocal count
            body, headers = self.fetch(url, maximum, deadline)
            if url == CHECK.SNAPSHOTS + 'tip.json':
                count += 1
                if count == 2:
                    changed = json.loads(body)
                    changed['published_at_unix'] += 1
                    body = encoded(changed)
            return body, headers

        result = CHECK.check_public(fetch=switching, now=NOW)
        self.assertEqual(result['attempt'], 2)
        self.assertIn('generation changed', result['previous_attempts'][0])

    def test_permanent_request_failure_is_bounded_to_three_attempts(self):
        count = 0

        def broken(*args):
            nonlocal count
            count += 1
            raise CHECK.CheckFailed('fixture network failure')

        with self.assertRaises(CHECK.CheckFailed):
            CHECK.check_public(fetch=broken, now=NOW)
        self.assertEqual(count, 3)

    def test_local_generation_and_chain_hash(self):
        with tempfile.TemporaryDirectory(prefix='hf132-public-state-') as temp:
            directory = Path(temp)
            for name, body in self.payloads.items():
                (directory / name).write_bytes(body)
            self.assertTrue(CHECK.check_snapshot_directory(directory, now=NOW)['ok'])
            (directory / 'chain.json').write_bytes(b'corrupt chain')
            with self.assertRaisesRegex(CHECK.CheckFailed, 'local chain bytes differ'):
                CHECK.check_snapshot_directory(directory, now=NOW)

    def test_forbidden_external_and_nonce_urls_do_not_start_requests(self):
        for url in ('http://download.qubit-coin.io/mainnet/snapshots/tip.json',
                    CHECK.SNAPSHOTS + 'tip.json?nonce=1', 'https://example.invalid/tip.json'):
            with self.assertRaises(CHECK.CheckFailed):
                CHECK.fetch_official(url, 1000, time.monotonic() + 1)

    def test_redirect_is_rejected(self):
        request = CHECK.urllib.request.Request(CHECK.SNAPSHOTS + 'tip.json')
        with self.assertRaisesRegex(CHECK.CheckFailed, 'redirect rejected'):
            CHECK.NoRedirect().redirect_request(request, None, 302, '', {}, 'https://example.invalid/')

    def test_transport_response_size_bound(self):
        class Response(io.BytesIO):
            status = 200
            headers = {}

        class Opener:
            def open(self, *args, **kwargs):
                return Response(b'over-sized')

        with patch.object(CHECK.urllib.request, 'build_opener', return_value=Opener()):
            with self.assertRaisesRegex(CHECK.CheckFailed, 'exceeds inspection bound'):
                CHECK.fetch_official(CHECK.SNAPSHOTS + 'tip.json', 3, time.monotonic() + 1)

    def test_stalled_transport_cannot_hold_the_operator_helper(self):
        release = threading.Event()

        class Opener:
            def open(self, *args, **kwargs):
                release.wait(2)
                raise OSError('fixture transport released')

        started = time.monotonic()
        try:
            with patch.object(CHECK.urllib.request, 'build_opener', return_value=Opener()), patch.object(CHECK, 'REQUEST_TIMEOUT', 0.02):
                with self.assertRaisesRegex(CHECK.CheckFailed, 'request deadline'):
                    CHECK.fetch_official(CHECK.SNAPSHOTS + 'tip.json', 1000, time.monotonic() + 1)
        finally:
            release.set()
        self.assertLess(time.monotonic() - started, 0.5)


if __name__ == '__main__':
    unittest.main(verbosity=2)
