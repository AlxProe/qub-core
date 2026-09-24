"""HF133 candidate source integrity; external ZIP checksum is the trust anchor."""
import argparse
import hashlib
import json
from pathlib import Path
import hf132_source_policy as core

core.MANIFEST = 'HF133-SOURCE-MANIFEST.json'
core.MANIFEST_SCHEMA = 'QUB-HF133-SOURCE-1'
core.RELEASE = 'HF133-v186'
core.PRODUCT_VERSION = '1.8.6'
core.PREPARATION_REVISION = 'daa-r4-h100000-native-candidate'
core.OBSOLETE_MANIFESTS |= {'HF132-SOURCE-MANIFEST.json'}
MANIFEST = core.MANIFEST
sha256 = core.sha256
source_files = core.source_files
require = core.require


def verify(root, mode='archive'):
    files = core.verify_inventory(root, mode)
    core.verify_frozen_sources(root)
    # Preserve all unmodified production modules, including the memory-r1 P2P
    # fixes; only public version strings may differ in these files.
    preserved = json.loads((root/'tests/fixtures/hf133-preserved-source.json').read_text(encoding='utf-8'))['files']
    for relative, expected in preserved.items():
        raw = (root/relative).read_bytes().replace(b'1.8.6', b'1.8.5')
        require(hashlib.sha256(raw).hexdigest() == expected, f'preserved module changed: {relative}')
    lib = (root/'src/lib.rs').read_bytes()
    for start, end, expected in [
        (b'fn required_work_bits_legacy(', b'pub fn target_from_compact(', '058d11d1b2536ee1911ef3b5df9a3d397301a4d97a49adaefbf5e2cbd9504796'),
        (b'fn validate_block(', b'pub fn is_immature_coinbase(', '0daf6f1505ada948a689d50fd37ee000b81a7401320cbf5498a83d90668222b6')]:
        require(lib.count(start) == lib.count(end) == 1, 'ambiguous legacy code range')
        raw = lib[lib.index(start):lib.index(end, lib.index(start))]
        require(hashlib.sha256(raw).hexdigest() == expected, 'legacy consensus kernel changed')
    daa = (root/'src/daa_v3.rs').read_text(encoding='utf-8')
    for marker in ['MAINNET_DAA_V3_ACTIVATION_HEIGHT: u32 = 100_000;', 'DAA_V3_HALF_LIFE_SECS: i64 = 3_600;', 'DAA_V3_TARGET_SPACING_SECS: u32 = 60;', 'DAA_V3_POW_LIMIT_BITS: u32 = 0x1d07_ffff;', 'DAA_V3_MTP_WINDOW: usize = 11;', 'PROTOCOL_EPOCH_3_BLOCK_VERSION: u32 = 3;']:
        require(marker in daa, 'wrong HF133 candidate parameter: '+marker)
    metadata = json.loads((root/'HF133-UPGRADE-METADATA-DRAFT.json').read_text(encoding='utf-8'))
    require(metadata.get('activation_height') == 100000 and
            metadata.get('activation_height_final') is False and
            metadata.get('public_release_approved') is False,
            'wrong HF133 candidate activation metadata')
    require('ACTIVATION_HEIGHT = 100000\n' in
            (root/'scripts/hf133_daa_reference.py').read_text(encoding='utf-8'),
            'wrong HF133 reference activation height')
    require('MAINNET_DAA_V3_ACTIVATION_HEIGHT == 100_000' in
            (root/'src/main.rs').read_text(encoding='utf-8'),
            'wrong HF133 native preflight activation height')
    require('f64' not in daa and 'f32' not in daa, 'floating point in consensus DAA')
    for relative, marker in [('Cargo.toml','version = "1.8.6"'), ('Cargo.lock','name = "qubd"\nversion = "1.8.6"'), ('installer/QUB-Core.iss','#define MyAppVersion "1.8.6"'), ('src/bin/qub_core.rs','const APP_VERSION: &str = "v1.8.6"')]:
        require(marker in (root/relative).read_text(encoding='utf-8'), 'wrong public version: '+relative)
    require(lib.count(b'daa_v3::validate_daa_v3_timestamp(') == 2, 'missing replay or connect timestamp hook')
    require(b'time: daa_v3::candidate_timestamp(' in lib, 'missing candidate timestamp hook')
    print(f'HF133 SOURCE: PASS ({len(files)-1} inventoried files, mode={mode})')
    return files

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--mode', choices=core.MODES, default='archive')
    args = parser.parse_args()
    verify(args.root, args.mode)
