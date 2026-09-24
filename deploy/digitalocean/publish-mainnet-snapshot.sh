#!/usr/bin/env bash
set -euo pipefail

# HF130 / v1.8.5: committed-generation, bounded-memory exact-schema publisher.
#
# Safety properties:
# - never runs full `qubd validate` inside the timer path;
# - exports one committed QUB-FCE-1 generation through qubd when available;
# - fails closed on production unless the source is an exact CURRENT QUB-FCE-1 export;
# - allows legacy/PREVIOUS recovery only through explicit operator/test flags;
# - scans blocks incrementally and keeps only the latest 4096 in memory;
# - verifies network, full hash-link continuity, and the HF120 #24000 version gate;
# - writes every artifact through a staging directory;
# - commits every public artifact through one atomic generation-pointer switch;
# - migrates legacy flat files without changing their bytes before that switch;
# - preserves the previous generations for diagnostics;
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
ALLOW_LEGACY_SOURCE=${ALLOW_LEGACY_SOURCE:-0}
ALLOW_PREVIOUS_RECOVERY=${ALLOW_PREVIOUS_RECOVERY:-0}

umask 022
LOCK_DIR=$(dirname -- "$LOCK_FILE")
mkdir -p "$LOCK_DIR"
exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  echo "snapshot publish lock contention: another generation is still running" >&2
  exit 75
fi

DATA_DIR=$(grep -E '^[[:space:]]*data_dir[[:space:]]*=' "$CFG" | head -n1 | cut -d= -f2- | tr -d ' "')
if [ -z "${DATA_DIR:-}" ]; then
  DATA_DIR=/opt/qub/mainnet/data
fi

mkdir -p "$OUT_DIR" "$(dirname "$ALT")"
STAGE=$(mktemp -d "$OUT_DIR/.publish-hf130.XXXXXX")
cleanup() {
  rm -rf "$STAGE"
}
trap cleanup EXIT INT TERM

LEGACY_SRC="$DATA_DIR/chain.json"
FAST_POINTER="$DATA_DIR/chain-v2/CURRENT.json"
EXPORT_SRC="$STAGE/export-source.json"
SOURCE_KIND=""
EXPORT_HEIGHT="0"
EXPORT_TIP_HASH=""
EXPORT_GENERATION="0"
EXPORT_STATE_REVISION="0"
EXPORT_COMMITTED_AT_UNIX="0"
EXPORT_RECOVERED_FROM_PREVIOUS="false"

if [ -f "$FAST_POINTER" ]; then
  if [ ! -x "$BIN" ]; then
    echo "Fast Chain Engine exists but qubd exporter is unavailable: $BIN" >&2
    exit 1
  fi
  echo "exporting committed Fast Chain Engine generation via qubd"
  EXPORT_REPORT=$(timeout \
    --signal=TERM \
    --kill-after=30s \
    "${EXPORT_TIMEOUT_SECS}s" \
    "$BIN" --config "$CFG" export-chain-json "$EXPORT_SRC")
  echo "$EXPORT_REPORT"
  IFS=$'\t' read -r EXPORT_HEIGHT EXPORT_TIP_HASH EXPORT_GENERATION EXPORT_STATE_REVISION EXPORT_COMMITTED_AT_UNIX EXPORT_RECOVERED_FROM_PREVIOUS < <(
    python3 - "$EXPORT_REPORT" <<'PY'
import json
import sys

report = json.loads(sys.argv[1])
if report.get("ok") is not True:
    raise SystemExit("exporter did not report ok=true")
if report.get("source") != "fce" or report.get("storage_engine") != "QUB-FCE-1":
    raise SystemExit("production exporter did not use QUB-FCE-1")
tip = str(report.get("tip_hash", "")).strip().lower()
if int(report.get("height", -1)) < 0 or len(tip) != 64:
    raise SystemExit("exporter returned an invalid height/tip identity")
print(
    int(report["height"]),
    tip,
    int(report.get("generation", 0)),
    int(report.get("state_revision", 0)),
    int(report.get("committed_at_unix", 0)),
    "true" if report.get("recovered_from_previous") else "false",
    sep="\t",
)
PY
  )
  if [ "$EXPORT_RECOVERED_FROM_PREVIOUS" = "true" ] && [ "$ALLOW_PREVIOUS_RECOVERY" != "1" ]; then
    echo "publisher refused a PREVIOUS-recovered export; repair CURRENT or set ALLOW_PREVIOUS_RECOVERY=1 for explicit recovery" >&2
    exit 1
  fi
  SOURCE_KIND="fce"
  SRC="$EXPORT_SRC"
elif [ -f "$LEGACY_SRC" ]; then
  if [ "$ALLOW_LEGACY_SOURCE" != "1" ]; then
    echo "publisher refused legacy chain.json without ALLOW_LEGACY_SOURCE=1; production publication requires QUB-FCE-1 CURRENT" >&2
    exit 1
  fi
  echo "using explicitly authorized recovery/test legacy chain source: $LEGACY_SRC"
  SOURCE_KIND="legacy"
  SRC="$LEGACY_SRC"
else
  echo "canonical chain source not found under $DATA_DIR" >&2
  exit 1
fi

python3 - "$SRC" "$STAGE" "$OUT_DIR" "$ALT" "$EXPECTED_NETWORK" "$EPOCH2_HEIGHT" "$EPOCH1_VERSION" "$EPOCH2_VERSION" "$SOURCE_KIND" "$EXPORT_HEIGHT" "$EXPORT_TIP_HASH" "$EXPORT_GENERATION" "$EXPORT_STATE_REVISION" "$EXPORT_COMMITTED_AT_UNIX" "$EXPORT_RECOVERED_FROM_PREVIOUS" <<'PY'
import atexit
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
source_kind = sys.argv[9]
export_height = int(sys.argv[10])
export_tip_hash = sys.argv[11].strip().lower()
export_generation = int(sys.argv[12])
export_state_revision = int(sys.argv[13])
export_committed_at_unix = int(sys.argv[14])
export_recovered_from_previous = sys.argv[15].lower() == "true"

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

if source_kind == "fce" and (height != export_height or tip_hash != export_tip_hash):
    raise ValueError(
        "export identity mismatch: "
        f"report #{export_height} {export_tip_hash}, bytes #{height} {tip_hash}"
    )


def write_text(path, text, encoding="utf-8"):
    path.write_text(text, encoding=encoding)


write_text(stage / "chain.json.sha256", f"{chain_sha}  chain.json\n", "ascii")

tip = {
    "network": network,
    "height": height,
    "tip_hash": tip_hash,
    "chain_sha256": chain_sha,
    "published_at_unix": now,
    "source": source_kind,
    "generation": export_generation,
    "state_revision": export_state_revision,
    "committed_at_unix": export_committed_at_unix,
    "recovered_from_previous": export_recovered_from_previous,
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

# All existing URLs resolve through ONE generation pointer. A failed alias
# preparation or pointer switch cannot expose a mixture of new and old files.
# Separate HTTP requests can still straddle a successful switch: consumers must
# compare the advertised tip/hash/checksum and retry an inconsistent fetch.
artifact_names = ["chain.json", "chain.json.sha256", "tip.json", "tip.json.sha256"]
for window in (64, 256, 1024, 2048, 4096):
    artifact_names.extend([f"tail-{window}.json", f"tail-{window}.json.sha256"])
alt_sha_name = "canonical-chain.json.sha256"
write_text(stage / alt_sha_name, f"{chain_sha}  canonical-chain.json\n", "ascii")

out_dir = out_dir.resolve()
alt_path = Path(os.path.abspath(alt_path))
alt_sha_path = alt_path.with_suffix(alt_path.suffix + ".sha256")
generations = out_dir / ".generations"
current = out_dir / ".current"
public = [(out_dir / name, name) for name in artifact_names]
public.extend([(alt_path, "chain.json"), (alt_sha_path, alt_sha_name)])
if len({str(path) for path, _ in public}) != len(public):
    raise ValueError("snapshot and canonical artifact paths must be distinct")

# Reject the exact historical ALT-directory failure before changing public
# files. Also reject non-regular legacy files and a foreign pointer layout.
for path, _ in public:
    if path.exists() and not path.is_file():
        raise ValueError(f"publication target is not a regular file: {path}")
    if path.is_symlink() and not path.exists():
        # Missing links are permitted only when they already have our expected
        # target (e.g. recovery from an interrupted first installation).
        target_name = next(name for item, name in public if item == path)
        expected = os.path.relpath(current / target_name, path.parent)
        if os.readlink(path) != expected:
            raise ValueError(f"publication target is an unexpected broken symlink: {path}")
if generations.is_symlink() or (generations.exists() and not generations.is_dir()):
    raise ValueError("invalid snapshot generation directory")
generations.mkdir(exist_ok=True)
if current.is_symlink():
    old_generation = current.resolve(strict=True)
    if old_generation.parent != generations or not old_generation.is_dir():
        raise ValueError("snapshot pointer does not reference a managed generation")
elif current.exists():
    raise ValueError("snapshot .current must be a managed symlink")
else:
    old_generation = None


def fsync_dir(path):
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


def prepared_link(path, target):
    tmp = path.with_name(path.name + f".hf130-{os.getpid()}-{time.time_ns()}.tmp")
    os.symlink(os.path.relpath(target, path.parent), tmp)
    return tmp


# A unique immutable generation is complete and durable before it is selected.
new_generation = generations / f"gen-{time.time_ns()}-{height}-{chain_sha[:16]}"
new_generation.mkdir(mode=0o755)


def discard_uncommitted_generation():
    try:
        if current.resolve(strict=False) != new_generation:
            shutil.rmtree(new_generation)
    except OSError as error:
        print(f"snapshot uncommitted generation cleanup deferred: {error}", file=sys.stderr)


atexit.register(discard_uncommitted_generation)
for name in artifact_names + [alt_sha_name]:
    os.replace(stage / name, new_generation / name)
    os.chmod(new_generation / name, 0o644)
    with (new_generation / name).open("rb") as file_obj:
        os.fsync(file_obj.fileno())
fsync_dir(new_generation)
fsync_dir(generations)

# During first-install migration, .current initially exposes copies of exactly
# the old public bytes. Replacing a legacy file with its alias therefore changes
# no observable payload, even if a later ALT replacement fails. Missing files
# stay missing until the final commit. The legacy canonical file is normally
# identical to chain.json; refuse a pre-existing mismatch rather than rewrite it
# as a side effect of migration.
if old_generation is None:
    old_generation = generations / f"legacy-{time.time_ns()}"
    old_generation.mkdir(mode=0o755)
    for path, name in public:
        if not path.exists():
            continue
        saved = old_generation / name
        if saved.exists():
            # chain and canonical aliases intentionally share one generation
            # payload. Their pre-migration bytes must agree.
            if path.stat().st_size != saved.stat().st_size:
                raise ValueError("legacy canonical chain differs from snapshot chain")
            with path.open("rb") as a, saved.open("rb") as b:
                while True:
                    left, right = a.read(1024 * 1024), b.read(1024 * 1024)
                    if left != right:
                        raise ValueError("legacy canonical chain differs from snapshot chain")
                    if not left:
                        break
        else:
            try:
                if path.is_symlink():
                    raise OSError("copy legacy symlink contents")
                os.link(path, saved)
            except OSError:
                shutil.copyfile(path, saved)
            with saved.open("rb") as file_obj:
                os.fsync(file_obj.fileno())
    fsync_dir(old_generation)
    fsync_dir(generations)
    initial_link = prepared_link(current, old_generation)
    try:
        os.replace(initial_link, current)
        fsync_dir(out_dir)
    finally:
        if initial_link.is_symlink():
            initial_link.unlink()

# Prepare all aliases first (including ALT's parent/filesystem), before any
# replacement. Normally no aliases change after the initial migration.
aliases = []
try:
    for path, name in public:
        expected = os.path.relpath(current / name, path.parent)
        if path.is_symlink() and os.readlink(path) == expected:
            continue
        if (current / name).exists() and path.exists():
            # A managed publication must not import an out-of-band edit back
            # into its current generation. Repair it only by a new commit.
            if old_generation.name.startswith("gen-"):
                raise ValueError(f"managed snapshot alias was replaced externally: {path}")
        aliases.append((prepared_link(path, current / name), path))
    for tmp, path in aliases:
        os.replace(tmp, path)
    for parent in {path.parent for path, _ in public}:
        fsync_dir(parent)
    next_link = prepared_link(current, new_generation)
    try:
        os.replace(next_link, current)
        # After this rename the entire generation is committed. A directory
        # fsync failure must be reported, but does not revert an already visible
        # coherent generation or remove files that readers can now reference.
        fsync_dir(out_dir)
    finally:
        if next_link.is_symlink():
            next_link.unlink()
finally:
    for tmp, _ in aliases:
        if tmp.is_symlink():
            tmp.unlink()

# Maintenance is after commit. Keep the selected and previous generation plus
# the newest third generation; already-open HTTP responses retain their inode.
# Never turn a successful coherent publication into a failed service result
# merely because old-generation deletion was unavailable.
try:
    retained = {new_generation, old_generation}
    known = sorted(
        (path for path in generations.iterdir()
         if path.is_dir() and not path.is_symlink()
         and (path.name.startswith("gen-") or path.name.startswith("legacy-"))),
        key=lambda path: path.stat().st_mtime_ns,
        reverse=True,
    )
    for candidate in known:
        if len(retained) >= 3:
            break
        retained.add(candidate)
    for obsolete in known:
        if obsolete not in retained:
            shutil.rmtree(obsolete)
    fsync_dir(generations)
except OSError as error:
    print(f"snapshot generation cleanup deferred: {error}", file=sys.stderr)

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
