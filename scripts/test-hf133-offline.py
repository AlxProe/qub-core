#!/usr/bin/env python3
"""Offline arithmetic and real-mainnet-header checks. No node or lab network."""
import hashlib
import json
from pathlib import Path
import random
import struct
import unittest
from decimal import Decimal
import hf133_daa_reference as r

ROOT = Path(__file__).resolve().parents[1]


class DaaChecks(unittest.TestCase):
    def test_mainnet_fixture_hashes_links_and_107_v2_targets(self):
        rows = json.loads((ROOT/'tests/fixtures/hf133-mainnet-headers.json').read_text(encoding='utf-8'))
        history = []
        for row in rows:
            header = struct.pack('<I', row['version']) + bytes.fromhex(row['prev_block_hash']) + bytes.fromhex(row['merkle_root']) + struct.pack('<III', row['time'], row['bits'], row['nonce'])
            hashed = hashlib.sha256(hashlib.sha256(header).digest()).digest().hex()
            self.assertEqual(hashed, row['hash'])
            if history:
                self.assertEqual(row['prev_block_hash'], history[-1]['hash'])
                self.assertEqual(row['height'], history[-1]['height']+1)
            if len(history) >= 21:
                self.assertEqual(r.v2(history), row['bits'])
            history.append(row)
        self.assertEqual(len(rows)-21, 107)

    def test_all_65536_polynomial_fractions_are_monotone_and_bounded(self):
        # Decimal ideal exponential is independent of the integer polynomial.
        previous = 0
        base = 0x1c100000
        max_error = Decimal(0)
        for fraction in range(65536):
            actual = r.target(r.asert(base, fraction, 65536))
            ideal = r.decimal_oracle_target(base, fraction, 65536)
            self.assertGreaterEqual(actual, previous)
            max_error = max(max_error, abs(Decimal(actual)/Decimal(ideal)-1))
            previous = actual
        self.assertLess(max_error, Decimal('0.00017'))
        print('Exhaustive fractional exponent relative error bound:', max_error)

    def test_signed_boundary_oracle_and_truncation(self):
        self.assertEqual(r.trunc_div(-1, 2), 0)
        for half in (1800, 3600, 7200, 172800):
            for error in (-half-1, -half, -half+1, -1, 0, 1, half-1, half, half+1):
                actual = r.target(r.asert(0x1c100000, error, half))
                ideal = r.decimal_oracle_target(0x1c100000, error, half)
                self.assertLess(abs(Decimal(actual)/Decimal(ideal)-1), Decimal('0.00018'))

    def test_cross_language_fixture_is_current(self):
        rows = json.loads((ROOT/'tests/fixtures/hf133-asert-vectors.json').read_text(encoding='utf-8'))['vectors']
        self.assertEqual(len(rows), 4408)
        for v in rows:
            self.assertEqual(r.asert(v['anchor_bits'], int(v['time_error']), v['half_life'], v['pow_limit_bits']), v['expected_bits'])

    def test_compact_roundtrip_and_extreme_saturation(self):
        rng = random.Random(133)
        limit = r.target(r.POW_LIMIT_BITS)
        for _ in range(5000):
            n = rng.randrange(1, limit+1)
            bits = r.compact(n)
            self.assertEqual(r.compact(r.target(bits)), bits)
            self.assertLessEqual(r.target(bits), n)
            self.assertEqual(r.asert(bits, 10**10), r.POW_LIMIT_BITS)
            self.assertEqual(r.asert(bits, -10**10), r.compact(1))
        for invalid in (0, 0x1d80ffff, 0x22010000, 0x01000001):
            with self.assertRaises(ValueError): r.target(invalid)

    def test_anchor_carry_and_reorg(self):
        rows = [{'time':1000000+i*60, 'bits':0x1c100000} for i in range(21)]
        self.assertEqual(r.anchored(rows, 20), rows[-1]['bits'])
        for i in range(2000):
            self.assertEqual(r.anchored(rows, 20), 0x1c100000)
            rows.append({'time':rows[-1]['time']+60, 'bits':0x1c100000})
        before = r.anchored(rows, 20)
        branch = [dict(x) for x in rows]
        branch[20]['bits'] = 0x1c200000
        self.assertNotEqual(r.anchored(branch, 20), before)
        self.assertEqual(r.anchored(rows, 20), before)

    def test_single_pre_activation_time_outlier(self):
        rows = [{'time':1000000+i*60, 'bits':0x1c100000} for i in range(21)]
        rows[-1]['time'] = 0
        self.assertEqual(r.anchored(rows, 20), 0x1c100000)
        median = r.mtp(rows)
        rows.append({'time':median+1, 'bits':r.anchored(rows, 20)})
        self.assertLess(abs(r.target(r.anchored(rows, 20))/r.target(0x1c100000)-1), 0.02)

    def test_time_manipulation_and_return_to_history(self):
        rows = [{'time':1000000+i*60, 'bits':0x1c100000} for i in range(50)]
        before = r.mtp(rows)
        rows[-1]['time'] += 7200
        self.assertEqual(r.mtp(rows), before)
        for row in rows[-6:]: row['time'] += 7200
        self.assertGreater(r.mtp(rows), before)
        # MTP limits a single outlier, not majority timestamp manipulation.

if __name__ == '__main__':
    unittest.main(verbosity=2)
