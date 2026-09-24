#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
PUBLISHER="$SCRIPT_DIR/publish-mainnet-snapshot.sh"
TMP=$(mktemp -d /tmp/qub-hf130-snapshot-test.XXXXXX)
VALID_LOG="$TMP/valid.log"
INVALID_LOG="$TMP/invalid.log"
FAIL_CLOSED_LOG="$TMP/fail-closed.log"
FCE_LOG="$TMP/fce.log"
RECOVERED_LOG="$TMP/recovered.log"
LOCKED_LOG="$TMP/locked.log"
VALID_LOCK="$TMP/publisher-valid.lock"
INVALID_LOCK="$TMP/publisher-invalid.lock"
FAIL_CLOSED_LOCK="$TMP/publisher-fail-closed.lock"
FCE_LOCK="$TMP/publisher-fce.lock"
RECOVERED_LOCK="$TMP/publisher-recovered.lock"
LOCKED_LOCK="$TMP/publisher-locked.lock"
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

if BIN="$TMP/no-qubd" \
CFG="$TMP/config.toml" \
OUT_DIR="$TMP/fail-closed-out" \
ALT="$TMP/fail-closed-canonical.json" \
LOCK_FILE="$FAIL_CLOSED_LOCK" \
EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$FAIL_CLOSED_LOG" 2>&1; then
    echo "ERROR: production publisher silently accepted legacy chain.json" >&2
    exit 1
fi
grep -q 'refused legacy chain.json' "$FAIL_CLOSED_LOG"
echo "production legacy-source rejection: OK"

exec 8>"$LOCKED_LOCK"
flock -n 8
set +e
BIN="$TMP/no-qubd" \
CFG="$TMP/config.toml" \
OUT_DIR="$TMP/locked-out" \
ALT="$TMP/locked-canonical.json" \
LOCK_FILE="$LOCKED_LOCK" \
EPOCH2_HEIGHT=50 \
ALLOW_LEGACY_SOURCE=1 \
bash "$PUBLISHER" >"$LOCKED_LOG" 2>&1
LOCKED_RC=$?
set -e
flock -u 8
exec 8>&-
if [ "$LOCKED_RC" -ne 75 ]; then
    echo "ERROR: lock contention returned $LOCKED_RC instead of 75" >&2
    exit 1
fi
grep -q 'lock contention' "$LOCKED_LOG"
echo "lock-contention nonzero result: OK"

BIN="$TMP/no-qubd" \
CFG="$TMP/config.toml" \
OUT_DIR="$TMP/out" \
ALT="$TMP/canonical-chain.json" \
LOCK_FILE="$VALID_LOCK" \
EPOCH2_HEIGHT=50 \
ALLOW_LEGACY_SOURCE=1 \
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

mkdir -p "$TMP/data/chain-v2"
printf '{}\n' >"$TMP/data/chain-v2/CURRENT.json"
cat >"$TMP/fake-qubd" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
destination="${@: -1}"
cp "$FAKE_CHAIN_SOURCE" "$destination"
printf '%s\n' "$FAKE_EXPORT_REPORT"
SH
chmod +x "$TMP/fake-qubd"

TIP_ID=$(python3 - "$TMP/out/tip.json" <<'PY'
import json
import sys
tip = json.load(open(sys.argv[1], encoding="utf-8"))
print(f'{tip["height"]}\t{tip["tip_hash"]}')
PY
)
IFS=$'\t' read -r FCE_HEIGHT FCE_TIP <<<"$TIP_ID"
FCE_REPORT=$(python3 - "$FCE_HEIGHT" "$FCE_TIP" <<'PY'
import json
import sys
print(json.dumps({
    "ok": True,
    "network": "mainnet",
    "height": int(sys.argv[1]),
    "tip_hash": sys.argv[2],
    "bytes": 1,
    "output": "test",
    "source": "fce",
    "storage_engine": "QUB-FCE-1",
    "generation": 42,
    "state_revision": 7,
    "committed_at_unix": 1700000000,
    "recovered_from_previous": False,
}))
PY
)

FAKE_CHAIN_SOURCE="$TMP/data/chain.json" \
FAKE_EXPORT_REPORT="$FCE_REPORT" \
BIN="$TMP/fake-qubd" \
CFG="$TMP/config.toml" \
OUT_DIR="$TMP/fce-out" \
ALT="$TMP/fce-canonical.json" \
LOCK_FILE="$FCE_LOCK" \
EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$FCE_LOG"

python3 - "$TMP/fce-out/tip.json" <<'PY'
import json
import sys
tip = json.load(open(sys.argv[1], encoding="utf-8"))
assert tip["source"] == "fce"
assert tip["generation"] == 42
assert tip["state_revision"] == 7
assert tip["recovered_from_previous"] is False
print("exact FCE export identity publication: OK")
PY

# HF128's exporter reports storage_engine but lacks authoritative source /
# recovery metadata. A seed-binary rollback must retain the versioned HF130
# exporter; accepting the old report would silently weaken the source gate.
HF128_REPORT=$(python3 - "$FCE_REPORT" <<'PYOLD'
import json
import sys
report = json.loads(sys.argv[1])
for key in ("source", "generation", "state_revision", "committed_at_unix", "recovered_from_previous"):
    report.pop(key)
print(json.dumps(report))
PYOLD
)
cp "$TMP/fce-out/tip.json" "$TMP/fce-tip-before-old-report.json"
if FAKE_CHAIN_SOURCE="$TMP/data/chain.json" FAKE_EXPORT_REPORT="$HF128_REPORT" \
BIN="$TMP/fake-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/fce-out" \
ALT="$TMP/fce-canonical.json" LOCK_FILE="$FCE_LOCK" EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$TMP/old-report.log" 2>&1; then
    echo "ERROR: publisher accepted legacy exporter report without source identity" >&2
    exit 1
fi
grep -q 'production exporter did not use QUB-FCE-1' "$TMP/old-report.log"
cmp -s "$TMP/fce-out/tip.json" "$TMP/fce-tip-before-old-report.json"
echo "HF128 exporter report without authoritative source is rejected: OK"

RECOVERED_REPORT=$(python3 - "$FCE_REPORT" <<'PY'
import json
import sys
report = json.loads(sys.argv[1])
report["recovered_from_previous"] = True
print(json.dumps(report))
PY
)
if FAKE_CHAIN_SOURCE="$TMP/data/chain.json" \
FAKE_EXPORT_REPORT="$RECOVERED_REPORT" \
BIN="$TMP/fake-qubd" \
CFG="$TMP/config.toml" \
OUT_DIR="$TMP/recovered-out" \
ALT="$TMP/recovered-canonical.json" \
LOCK_FILE="$RECOVERED_LOCK" \
EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$RECOVERED_LOG" 2>&1; then
    echo "ERROR: publisher accepted PREVIOUS recovery without explicit authorization" >&2
    exit 1
fi
grep -q 'refused a PREVIOUS-recovered export' "$RECOVERED_LOG"
echo "production PREVIOUS-recovery rejection: OK"

rm -f "$TMP/data/chain-v2/CURRENT.json"

# The fixtures deliberately test publisher identity/schema/IO behavior; they
# are not mined blocks and do not replace qubd consensus or live-node tests.
cat >"$TMP/verify-generation.py" <<'PYVERIFY'
import hashlib
import json
import sys
from pathlib import Path
out, alt, expected = Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3])
tip = json.loads((out / "tip.json").read_text())
assert tip["height"] == expected
for path in out.glob("*.json"):
    actual = hashlib.sha256(path.read_bytes()).hexdigest()
    checksum = Path(str(path) + ".sha256").read_text().split()[0]
    assert actual == checksum, (path, actual, checksum)
assert hashlib.sha256((out / "chain.json").read_bytes()).hexdigest() == tip["chain_sha256"]
assert alt.read_bytes() == (out / "chain.json").read_bytes()
assert hashlib.sha256(alt.read_bytes()).hexdigest() == Path(str(alt) + ".sha256").read_text().split()[0]
for window in (64, 256, 1024, 2048, 4096):
    obj = json.loads((out / f"tail-{window}.json").read_text())
    assert set(obj) == {"network", "start_height", "tip_hash", "tip_height", "blocks"}
    assert (obj["tip_height"], obj["tip_hash"]) == (tip["height"], tip["tip_hash"])
PYVERIFY

# Keep genuine regular-file layouts to exercise the one-time HF128 migration.
python3 - "$TMP" <<'PYSETUP'
import json
import shutil
import sys
from pathlib import Path
root = Path(sys.argv[1])
for name in ("alt-failure", "migration-failure"):
    out = root / name
    out.mkdir()
    for path in (root / "out").glob("*.json*"):
        shutil.copyfile(path, out / path.name)
    shutil.copyfile(root / "canonical-chain.json", root / f"{name}-canonical.json")
    shutil.copyfile(root / "canonical-chain.json.sha256", root / f"{name}-canonical.json.sha256")
    (root / f"{name}-before.json").write_text(json.dumps({
        path.name: path.read_text() for path in out.glob("*.json*")
    }))
# Make the exact historically failing ALT target: an existing directory.
(root / "alt-failure-canonical.json").unlink()
(root / "alt-failure-canonical.json").mkdir()
# Extend the valid fixture from height 99 to 100.
p = root / "data" / "chain.json"
chain = json.loads(p.read_text())
block = json.loads(json.dumps(chain["blocks"][-1]))
block["header"]["prev_block_hash"] = json.loads((root / "out" / "tip.json").read_text())["tip_hash"]
block["header"]["time"] += 1
block["header"]["nonce"] += 1
chain["blocks"].append(block)
p.write_text(json.dumps(chain, indent=2) + "\n")
PYSETUP

if BIN="$TMP/no-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/alt-failure" \
ALT="$TMP/alt-failure-canonical.json" LOCK_FILE="$TMP/alt-failure.lock" \
EPOCH2_HEIGHT=50 ALLOW_LEGACY_SOURCE=1 bash "$PUBLISHER" >"$TMP/alt-failure.log" 2>&1; then
    echo "ERROR: publisher accepted ALT directory" >&2
    exit 1
fi
grep -q 'publication target is not a regular file' "$TMP/alt-failure.log"
python3 - "$TMP" <<'PYCHECK'
import json
import sys
from pathlib import Path
root = Path(sys.argv[1])
for name, text in json.loads((root / "alt-failure-before.json").read_text()).items():
    assert (root / "alt-failure" / name).read_text() == text
assert not (root / "alt-failure" / ".current").exists()
print("ALT directory failure preserves old public bytes: OK")
PYCHECK

# Inject an actual os.replace failure from test-only Python startup code, with
# no fault-injection flag or branch added to the production publisher.
mkdir "$TMP/fault-python"
cat >"$TMP/fault-python/sitecustomize.py" <<'PYFAULT'
import os
original_replace = os.replace

def fail_selected_replace(src, dst, *args, **kwargs):
    if os.fspath(dst) == os.environ.get("QUB_TEST_REPLACE_FAILURE_DEST"):
        raise PermissionError("injected publication rename failure")
    return original_replace(src, dst, *args, **kwargs)

os.replace = fail_selected_replace
PYFAULT

if PYTHONPATH="$TMP/fault-python" QUB_TEST_REPLACE_FAILURE_DEST="$TMP/migration-failure-canonical.json" \
BIN="$TMP/no-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/migration-failure" \
ALT="$TMP/migration-failure-canonical.json" LOCK_FILE="$TMP/migration-failure.lock" \
EPOCH2_HEIGHT=50 ALLOW_LEGACY_SOURCE=1 bash "$PUBLISHER" >"$TMP/migration-failure.log" 2>&1; then
    echo "ERROR: publisher ignored injected ALT rename failure" >&2
    exit 1
fi
grep -q 'injected publication rename failure' "$TMP/migration-failure.log"
python3 "$TMP/verify-generation.py" "$TMP/migration-failure" "$TMP/migration-failure-canonical.json" 99
python3 - "$TMP" <<'PYCHECK'
import json
import sys
from pathlib import Path
root = Path(sys.argv[1])
for name, text in json.loads((root / "migration-failure-before.json").read_text()).items():
    assert (root / "migration-failure" / name).read_text() == text
assert (root / "migration-failure" / ".current").is_symlink()
assert (root / "migration-failure" / "chain.json").is_symlink()
print("mid-migration ALT rename failure preserves readable old generation: OK")
PYCHECK

if PYTHONPATH="$TMP/fault-python" QUB_TEST_REPLACE_FAILURE_DEST="$TMP/out/.current" \
BIN="$TMP/no-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/out" \
ALT="$TMP/canonical-chain.json" LOCK_FILE="$VALID_LOCK" \
EPOCH2_HEIGHT=50 ALLOW_LEGACY_SOURCE=1 bash "$PUBLISHER" >"$TMP/pointer-failure.log" 2>&1; then
    echo "ERROR: publisher ignored injected pointer rename failure" >&2
    exit 1
fi
grep -q 'injected publication rename failure' "$TMP/pointer-failure.log"
python3 "$TMP/verify-generation.py" "$TMP/out" "$TMP/canonical-chain.json" 99
echo "generation commit failure preserves every old checksum and payload: OK"

# The interrupted migration must be resumable without a special repair step.
OLD_GENERATION=$(readlink -f "$TMP/migration-failure/.current")
BIN="$TMP/no-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/migration-failure" \
ALT="$TMP/migration-failure-canonical.json" LOCK_FILE="$TMP/migration-failure.lock" \
EPOCH2_HEIGHT=50 ALLOW_LEGACY_SOURCE=1 bash "$PUBLISHER" >"$TMP/migration-retry.log"
python3 "$TMP/verify-generation.py" "$TMP/migration-failure" "$TMP/migration-failure-canonical.json" 100
python3 - "$OLD_GENERATION" "$TMP/migration-failure" <<'PYCHECK'
import hashlib
import json
import sys
from pathlib import Path
old, out = map(Path, sys.argv[1:])
assert old != (out / ".current").resolve()
tip = json.loads((old / "tip.json").read_text())
assert tip["height"] == 99
assert hashlib.sha256((old / "chain.json").read_bytes()).hexdigest() == tip["chain_sha256"]
assert json.loads((out / "tip.json").read_text())["height"] == 100
for path in out.glob("*.json*"):
    assert path.is_symlink(), path
print("legacy migration retry commits coherent new generation and retains old bytes: OK")
PYCHECK

# Re-exporting a CURRENT fixture with a mismatched report must not touch the
# existing published FCE generation.
printf '{}\n' >"$TMP/data/chain-v2/CURRENT.json"
if FAKE_CHAIN_SOURCE="$TMP/data/chain.json" FAKE_EXPORT_REPORT="$FCE_REPORT" \
BIN="$TMP/fake-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/fce-out" \
ALT="$TMP/fce-canonical.json" LOCK_FILE="$FCE_LOCK" EPOCH2_HEIGHT=50 \
bash "$PUBLISHER" >"$TMP/identity-mismatch.log" 2>&1; then
    echo "ERROR: publisher accepted mismatched export report" >&2
    exit 1
fi
grep -q 'export identity mismatch' "$TMP/identity-mismatch.log"
python3 "$TMP/verify-generation.py" "$TMP/fce-out" "$TMP/fce-canonical.json" 99
echo "FCE report mismatch preserves previous publication: OK"
rm -f "$TMP/data/chain-v2/CURRENT.json"

python3 - "$TMP/data/chain.json" <<'PY'
import json
import sys
from pathlib import Path
p = Path(sys.argv[1])
o = json.loads(p.read_text(encoding="utf-8"))
o["blocks"][50]["header"]["version"] = 1
p.write_text(json.dumps(o, indent=2) + "\n", encoding="utf-8")
PY

if BIN="$TMP/no-qubd" CFG="$TMP/config.toml" OUT_DIR="$TMP/bad-out" ALT="$TMP/bad-canonical.json" LOCK_FILE="$INVALID_LOCK" EPOCH2_HEIGHT=50 ALLOW_LEGACY_SOURCE=1 bash "$PUBLISHER" >"$INVALID_LOG" 2>&1; then
    echo "ERROR: publisher accepted an invalid post-activation block version" >&2
    exit 1
fi

grep -q 'version mismatch' "$INVALID_LOG"

echo "invalid epoch-version rejection: OK"
echo "HF130 snapshot publisher self-test: PASS"
