#!/usr/bin/env bash
set -euo pipefail

BIN=${BIN:-/opt/qub/bin/qubd}
CFG=${CFG:-/opt/qub/mainnet/mainnet-seed.toml}
OUT_DIR=${OUT_DIR:-/srv/qub-updates/mainnet/snapshots}
ALT=${ALT:-/srv/qub-updates/mainnet/canonical-chain.json}

DATA_DIR=$(grep -E '^[[:space:]]*data_dir[[:space:]]*=' "$CFG" | head -n1 | cut -d= -f2- | tr -d ' "')
if [ -z "${DATA_DIR:-}" ]; then
  DATA_DIR=/opt/qub/mainnet/data
fi
SRC="$DATA_DIR/chain.json"
test -f "$SRC"

mkdir -p "$OUT_DIR"

# Validate the source chain before publishing anything.
"$BIN" --config "$CFG" validate >/dev/null

TMP="$OUT_DIR/chain.json.tmp"
cp "$SRC" "$TMP"
chmod 644 "$TMP"
mv "$TMP" "$OUT_DIR/chain.json"
cp "$OUT_DIR/chain.json" "$ALT"

python3 - <<'PY'
import json, hashlib, struct, time
from pathlib import Path

out_dir = Path('/srv/qub-updates/mainnet/snapshots')
chain_path = out_dir / 'chain.json'
alt_path = Path('/srv/qub-updates/mainnet/canonical-chain.json')
chain = json.load(open(chain_path, encoding='utf-8'))
blocks = chain.get('blocks', [])
network = chain.get('network', 'mainnet')

def u32(x):
    if isinstance(x, str):
        x = int(x, 0) if x.startswith('0x') else int(x)
    return struct.pack('<I', int(x) & 0xffffffff)

def hb(x):
    return bytes.fromhex(str(x))

def bh(block):
    h = block['header']
    raw = u32(h['version']) + hb(h['prev_block_hash']) + hb(h['merkle_root']) + u32(h['time']) + u32(h['bits']) + u32(h['nonce'])
    return hashlib.sha256(hashlib.sha256(raw).digest()).hexdigest()

height = max(0, len(blocks) - 1)
tip_hash = bh(blocks[-1]) if blocks else ''
sha = hashlib.sha256(chain_path.read_bytes()).hexdigest()
now = int(time.time())

(chain_path.with_suffix(chain_path.suffix + '.sha256')).write_text(f'{sha}  {chain_path.name}\n', encoding='ascii')
alt_path.with_suffix(alt_path.suffix + '.sha256').write_text(f'{sha}  {alt_path.name}\n', encoding='ascii')

tip = {
    'network': network,
    'height': height,
    'tip_hash': tip_hash,
    'chain_sha256': sha,
    'published_at_unix': now,
}
(out_dir / 'tip.json').write_text(json.dumps(tip, separators=(',', ':')) + '\n', encoding='utf-8')
(out_dir / 'tip.json.sha256').write_text(hashlib.sha256((out_dir / 'tip.json').read_bytes()).hexdigest() + '  tip.json\n', encoding='ascii')

for window in (64, 256, 1024, 2048, 4096):
    start = max(0, height - window + 1)
    tail = {
        'network': network,
        'start_height': start,
        'tip_height': height,
        'tip_hash': tip_hash,
        'blocks': blocks[start:],
    }
    path = out_dir / f'tail-{window}.json'
    path.write_text(json.dumps(tail, separators=(',', ':')) + '\n', encoding='utf-8')
    path.with_suffix(path.suffix + '.sha256').write_text(hashlib.sha256(path.read_bytes()).hexdigest() + f'  tail-{window}.json\n', encoding='ascii')
PY

chmod 644 "$OUT_DIR"/*.json "$OUT_DIR"/*.sha256 "$ALT" "$ALT.sha256" 2>/dev/null || true
ls -lh "$OUT_DIR"/tip.json "$OUT_DIR"/tail-64.json "$OUT_DIR"/tail-256.json "$OUT_DIR"/tail-1024.json "$OUT_DIR"/tail-2048.json "$OUT_DIR"/tail-4096.json "$OUT_DIR"/chain.json
