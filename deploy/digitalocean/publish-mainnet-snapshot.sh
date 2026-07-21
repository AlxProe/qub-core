#!/usr/bin/env bash
set -euo pipefail

# HF123 / v1.8.0: Fast Chain Engine aware, bounded-memory exact-schema publisher.
#
# Safety properties:
# - never runs full `qubd validate` inside the timer path;
# - exports one committed QUB-FCE-1 generation through qubd when available;
# - retains the legacy chain.json fallback for isolated tests and pre-migration recovery;
# - scans blocks incrementally and keeps only the latest 4096 in memory;
# - verifies network, full hash-link continuity, and the HF120 #24000 version gate;
# - writes every artifact through a staging directory;
# - publishes tip.json last as the generation commit marker;
# - preserves the exact JSON schemas consumed by QUB Core and Explorer.
# - supports an overridable LOCK_FILE so isolated self-tests never touch the live timer lock.

CFG=${CFG:-/opt/qub/mainnet/mainnet-seed.toml}
BIN=${BIN:-/opt/qub/bin/qubd}
EXPORT_TIMEOUT_SECS=${EXPORT_TIMEOUT_SECS:-900}
OUT_DIR=${OUT_DIR:-/srv/qub-updates/mainnet/snapshots}
ALT=${ALT:-/srv/qub-updates/mainnet/canonical-chain.json}
EXPECTED_NETWORK=${EXPECTED_NETWORK:-mainnet}
EPOCH2_HEIGHT=${EPOCH2_HEIGHT:-24000}
EPOCH1_VERSION=${EPOCH1_VERSION:-1}
EPOCH2_VERSION=${EPOCH2_VERSION:-2}
LOCK_FILE=${LOCK_FILE:-/tmp/qub-mainnet-snapshot-publish.lock}

umask 022
LOCK_DIR=$(dirname -- "$LOCK_FILE")
mkdir -p "$LOCK_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  echo "snapshot publish already running; skipping"
  exit 0
fi

DATA_DIR=$(grep -E '^[[:space:]]*data_dir[[:space:]]*=' "$CFG" | head -n1 | cut -d= -f2- | tr -d ' "')
if [ -z "${DATA_DIR:-}" ]; then
  DATA_DIR=/opt/qub/mainnet/data
fi

mkdir -p "$OUT_DIR" "$(dirname "$ALT")"
STAGE=$(mktemp -d "$OUT_DIR/.publish-hf123.XXXXXX")
cleanup() {
  rm -rf "$STAGE"
}
trap cleanup EXIT INT TERM

LEGACY_SRC="$DATA_DIR/chain.json"
FAST_POINTER="$DATA_DIR/chain-v2/CURRENT.json"
EXPORT_SRC="$STAGE/export-source.json"

if [ -f "$FAST_POINTER" ]; then
  if [ ! -x "$BIN" ]; then
    echo "Fast Chain Engine exists but qubd exporter is unavailable: $BIN" >&2
    exit 1
  fi
  echo "exporting committed Fast Chain Engine generation via qubd"
  timeout \
    --signal=TERM \
    --kill-after=30s \
    "${EXPORT_TIMEOUT_SECS}s" \
    "$BIN" --config "$CFG" export-chain-json "$EXPORT_SRC"
  SRC="$EXPORT_SRC"
elif [ -f "$LEGACY_SRC" ]; then
  echo "using pre-migration legacy chain source: $LEGACY_SRC"
  SRC="$LEGACY_SRC"
else
  echo "canonical chain source not found under $DATA_DIR" >&2
  exit 1
fi

python3 - "$SRC" "$STAGE" "$OUT_DIR" "$ALT" "$EXPECTED_NETWORK" "$EPOCH2_HEIGHT" "$EPOCH1_VERSION" "$EPOCH2_VERSION" <<'PY'
import collections
import hashlib
import json
import os
import re
import shutil
import struct
import sys
import time
from pathlib import Path

src = Path(sys.argv[1])
stage = Path(sys.argv[2])
out_dir = Path(sys.argv[3])
alt_path = Path(sys.argv[4])
expected_network = sys.argv[5]
epoch2_height = int(sys.argv[6])
epoch1_version = int(sys.argv[7])
epoch2_version = int(sys.argv[8])

stage.mkdir(parents=True, exist_ok=True)
out_dir.mkdir(parents=True, exist_ok=True)
alt_path.parent.mkdir(parents=True, exist_ok=True)

chain_stage = stage / "chain.json"
sha256 = hashlib.sha256()
with src.open("rb") as source, chain_stage.open("wb") as target:
    while True:
        chunk = source.read(1024 * 1024)
        if not chunk:
            break
        target.write(chunk)
        sha256.update(chunk)
    target.flush()
    os.fsync(target.fileno())
chain_sha = sha256.hexdigest()


def u32(value):
    if isinstance(value, str):
        value = int(value, 0) if value.startswith("0x") else int(value)
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def raw_hash(value):
    data = bytes.fromhex(str(value))
    if len(data) != 32:
        raise ValueError("expected 32-byte hash")
    return data


def block_hash(block):
    header = block["header"]
    raw = (
        u32(header["version"])
        + raw_hash(header["prev_block_hash"])
        + raw_hash(header["merkle_root"])
        + u32(header["time"])
        + u32(header["bits"])
        + u32(header["nonce"])
    )
    return hashlib.sha256(hashlib.sha256(raw).digest()).hexdigest()


def find_blocks_array(file_obj):
    # serde_json pretty output places network and blocks at the beginning. Keep
    # the search bounded; a missing marker means the source is not the expected
    # PersistedChainState schema and must never be published.
    prefix = file_obj.read(1024 * 1024)
    network_match = re.search(rb'"network"\s*:\s*"([^"\\]*(?:\\.[^"\\]*)*)"', prefix)
    if not network_match:
        raise ValueError("chain.json missing top-level network")
    network = json.loads(b'"' + network_match.group(1) + b'"')

    marker = re.search(rb'"blocks"\s*:\s*\[', prefix)
    if not marker:
        raise ValueError("chain.json missing top-level blocks array")
    file_obj.seek(marker.end())
    return network


def iter_block_objects(path):
    with path.open("rb") as file_obj:
        network = find_blocks_array(file_obj)
        yield ("network", network)

        current = None
        depth = 0
        in_string = False
        escaped = False
        ended = False

        while not ended:
            chunk = file_obj.read(256 * 1024)
            if not chunk:
                break
            for byte in chunk:
                if current is None:
                    if byte in b" \t\r\n,":
                        continue
                    if byte == ord("]"):
                        ended = True
                        break
                    if byte != ord("{"):
                        raise ValueError(f"unexpected byte before block object: {byte}")
                    current = bytearray([byte])
                    depth = 1
                    in_string = False
                    escaped = False
                    continue

                current.append(byte)
                if in_string:
                    if escaped:
                        escaped = False
                    elif byte == ord("\\"):
                        escaped = True
                    elif byte == ord('"'):
                        in_string = False
                    continue

                if byte == ord('"'):
                    in_string = True
                elif byte == ord("{"):
                    depth += 1
                elif byte == ord("}"):
                    depth -= 1
                    if depth == 0:
                        block = json.loads(current)
                        yield ("block", block)
                        current = None

        if current is not None:
            raise ValueError("truncated block object in chain.json")
        if not ended:
            raise ValueError("unterminated blocks array in chain.json")


network = None
block_count = 0
previous_hash = None
latest_blocks = collections.deque(maxlen=4096)

for kind, value in iter_block_objects(chain_stage):
    if kind == "network":
        network = value
        continue

    block = value
    header = block.get("header")
    transactions = block.get("transactions")
    if not isinstance(header, dict) or not isinstance(transactions, list):
        raise ValueError(f"block #{block_count} has invalid header/transactions schema")

    version = int(header.get("version"))
    expected_version = epoch2_version if block_count >= epoch2_height else epoch1_version
    if version != expected_version:
        raise ValueError(
            f"block #{block_count} version mismatch: expected {expected_version}, got {version}"
        )

    current_hash = block_hash(block)
    if block_count > 0:
        prev = str(header.get("prev_block_hash", "")).lower()
        if prev != previous_hash:
            raise ValueError(
                f"hash-link mismatch at block #{block_count}: expected prev {previous_hash}, got {prev}"
            )
    previous_hash = current_hash
    latest_blocks.append(block)
    block_count += 1

if network != expected_network:
    raise ValueError(f"network mismatch: expected {expected_network}, got {network}")
if block_count == 0 or previous_hash is None:
    raise ValueError("snapshot source has no blocks")

height = block_count - 1
tip_hash = previous_hash
now = int(time.time())
latest = list(latest_blocks)


def write_text(path, text, encoding="utf-8"):
    path.write_text(text, encoding=encoding)


write_text(stage / "chain.json.sha256", f"{chain_sha}  chain.json\n", "ascii")

tip = {
    "network": network,
    "height": height,
    "tip_hash": tip_hash,
    "chain_sha256": chain_sha,
    "published_at_unix": now,
}
write_text(stage / "tip.json", json.dumps(tip, separators=(",", ":")) + "\n")
write_text(
    stage / "tip.json.sha256",
    hashlib.sha256((stage / "tip.json").read_bytes()).hexdigest() + "  tip.json\n",
    "ascii",
)

for window in (64, 256, 1024, 2048, 4096):
    selected = latest[-window:]
    start_height = height - len(selected) + 1
    tail = {
        "network": network,
        "start_height": start_height,
        "tip_hash": tip_hash,
        "tip_height": height,
        "blocks": selected,
    }
    name = f"tail-{window}.json"
    path = stage / name
    write_text(path, json.dumps(tail, separators=(",", ":")) + "\n")
    write_text(
        stage / f"{name}.sha256",
        hashlib.sha256(path.read_bytes()).hexdigest() + f"  {name}\n",
        "ascii",
    )

# Final staged-schema verification before any public file is replaced.
for window in (64, 256, 1024, 2048, 4096):
    obj = json.loads((stage / f"tail-{window}.json").read_text(encoding="utf-8"))
    expected_keys = {"network", "start_height", "tip_hash", "tip_height", "blocks"}
    if set(obj) != expected_keys:
        raise ValueError(f"tail-{window}.json schema mismatch: {sorted(obj)}")
    if obj["tip_height"] != height or obj["tip_hash"] != tip_hash:
        raise ValueError(f"tail-{window}.json tip mismatch")

# Publish every generation file atomically. tip.json is the final commit marker.
non_tip_names = [
    "chain.json",
    "chain.json.sha256",
    "tail-64.json",
    "tail-64.json.sha256",
    "tail-256.json",
    "tail-256.json.sha256",
    "tail-1024.json",
    "tail-1024.json.sha256",
    "tail-2048.json",
    "tail-2048.json.sha256",
    "tail-4096.json",
    "tail-4096.json.sha256",
]
for name in non_tip_names:
    os.replace(stage / name, out_dir / name)

# canonical-chain.json is the same canonical byte stream as snapshots/chain.json.
alt_tmp = alt_path.with_name(alt_path.name + f".{os.getpid()}.tmp")
try:
    if alt_tmp.exists():
        alt_tmp.unlink()
    try:
        os.link(out_dir / "chain.json", alt_tmp)
    except OSError:
        shutil.copyfile(out_dir / "chain.json", alt_tmp)
    os.replace(alt_tmp, alt_path)
finally:
    if alt_tmp.exists():
        alt_tmp.unlink()

alt_sha_path = alt_path.with_suffix(alt_path.suffix + ".sha256")
alt_sha_tmp = alt_sha_path.with_name(alt_sha_path.name + f".{os.getpid()}.tmp")
write_text(alt_sha_tmp, f"{chain_sha}  canonical-chain.json\n", "ascii")
os.replace(alt_sha_tmp, alt_sha_path)

os.replace(stage / "tip.json.sha256", out_dir / "tip.json.sha256")
os.replace(stage / "tip.json", out_dir / "tip.json")

for path in list(out_dir.glob("*.json")) + list(out_dir.glob("*.sha256")) + [alt_path, alt_sha_path]:
    try:
        os.chmod(path, 0o644)
    except FileNotFoundError:
        pass

print(
    f"published snapshot height={height} tip={tip_hash} "
    f"blocks={block_count} retained={len(latest)} chain_sha256={chain_sha}"
)
PY

cat "$OUT_DIR/tip.json"
ls -lh \
  "$OUT_DIR"/tip.json \
  "$OUT_DIR"/tail-64.json \
  "$OUT_DIR"/tail-256.json \
  "$OUT_DIR"/tail-1024.json \
  "$OUT_DIR"/tail-2048.json \
  "$OUT_DIR"/tail-4096.json \
  "$OUT_DIR"/chain.json \
  "$ALT"
