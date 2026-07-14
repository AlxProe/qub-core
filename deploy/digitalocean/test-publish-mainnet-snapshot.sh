#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PUBLISHER="$SCRIPT_DIR/publish-mainnet-snapshot.sh"
TMP=$(mktemp -d /tmp/qub-hf121-snapshot-test.XXXXXX)
VALID_LOG="$TMP/valid.log"
INVALID_LOG="$TMP/invalid.log"
VALID_LOCK="$TMP/publisher-valid.lock"
INVALID_LOCK="$TMP/publisher-invalid.lock"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT INT TERM

mkdir -p "$TMP/data" "$TMP/out"
cat >"$TMP/config.toml" <<EOF
[node]
data_dir = "$TMP/data"
EOF

python3 - "$TMP/data/chain.json" <<'PY'
import hashlib
import json
import struct
import sys
from pathlib import Path

path = Path(sys.argv[1])

def u32(v):
    return struct.pack("<I", v)

def block_hash(block):
    h = block["header"]
    raw = (
        u32(h["version"])
        + bytes.fromhex(h["prev_block_hash"])
        + bytes.fromhex(h["merkle_root"])
        + u32(h["time"])
        + u32(h["bits"])
        + u32(h["nonce"])
    )
    return hashlib.sha256(hashlib.sha256(raw).digest()).hexdigest()

blocks = []
prev = "00" * 32
for height in range(100):
    block = {
        "header": {
            "version": 1 if height < 50 else 2,
            "prev_block_hash": prev,
            "merkle_root": hashlib.sha256(f"merkle-{height}".encode()).hexdigest(),
            "time": 1_700_000_000 + height,
            "bits": 0x1F00FFFF,
            "nonce": height,
        },
        "transactions": [],
    }
    blocks.append(block)
    prev = block_hash(block)

path.write_text(
    json.dumps({"network": "mainnet", "blocks": blocks, "utxos": [], "mempool": []}, indent=2) + "\n",
    encoding="utf-8",
)
PY

CFG="$TMP/config.toml" \
OUT_DIR="$TMP/out" \
ALT="$TMP/canonical-chain.json" \
LOCK_FILE="$VALID_LOCK" \
EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$VALID_LOG"

python3 - "$TMP" <<'PY'
import hashlib
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
out = root / "out"
tip = json.loads((out / "tip.json").read_text(encoding="utf-8"))
assert tip["height"] == 99
assert len(json.loads((out / "tail-64.json").read_text(encoding="utf-8"))["blocks"]) == 64
assert len(json.loads((out / "tail-4096.json").read_text(encoding="utf-8"))["blocks"]) == 100
assert hashlib.sha256((out / "chain.json").read_bytes()).hexdigest() == tip["chain_sha256"]
assert (root / "canonical-chain.json").read_bytes() == (out / "chain.json").read_bytes()
for window in (64, 256, 1024, 2048, 4096):
    obj = json.loads((out / f"tail-{window}.json").read_text(encoding="utf-8"))
    assert set(obj) == {"network", "start_height", "tip_hash", "tip_height", "blocks"}
    assert obj["tip_height"] == 99
print("valid snapshot generation: OK")
PY

python3 - "$TMP/data/chain.json" <<'PY'
import json
import sys
from pathlib import Path
p = Path(sys.argv[1])
o = json.loads(p.read_text(encoding="utf-8"))
o["blocks"][50]["header"]["version"] = 1
p.write_text(json.dumps(o, indent=2) + "\n", encoding="utf-8")
PY

if CFG="$TMP/config.toml" OUT_DIR="$TMP/bad-out" ALT="$TMP/bad-canonical.json" LOCK_FILE="$INVALID_LOCK" EPOCH2_HEIGHT=50 bash "$PUBLISHER" >"$INVALID_LOG" 2>&1; then
    echo "ERROR: publisher accepted an invalid post-activation block version" >&2
    exit 1
fi

grep -q 'version mismatch' "$INVALID_LOG"

echo "invalid epoch-version rejection: OK"
echo "HF121 snapshot publisher self-test: PASS"
