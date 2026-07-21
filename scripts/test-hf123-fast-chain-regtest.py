#!/usr/bin/env python3
"""HF123 Fast Chain Engine end-to-end regression test.

This test uses an isolated regtest data directory and a real qubd binary. It
verifies one-time initialization, append-only commits, crash-suffix truncation,
CURRENT/PREVIOUS recovery, explicit legacy export, and consensus validation.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile


def run(command: list[str], *, expect: int = 0) -> subprocess.CompletedProcess[str]:
    print("+", " ".join(command), flush=True)
    result = subprocess.run(command, text=True, capture_output=True)
    if result.stdout:
        print(result.stdout, end="" if result.stdout.endswith("\n") else "\n")
    if result.stderr:
        print(result.stderr, file=sys.stderr, end="" if result.stderr.endswith("\n") else "\n")
    if result.returncode != expect:
        raise RuntimeError(
            f"command returned {result.returncode}, expected {expect}: {' '.join(command)}"
        )
    return result


def run_json(command: list[str]) -> dict:
    result = run(command)
    try:
        value = json.loads(result.stdout)
    except json.JSONDecodeError as exc:
        raise RuntimeError(f"command did not return JSON: {' '.join(command)}") from exc
    if not isinstance(value, dict):
        raise RuntimeError("expected a JSON object")
    return value


def replace_toml_value(text: str, section: str, key: str, value: str) -> str:
    lines = text.splitlines()
    current = ""
    replaced = False
    output: list[str] = []
    for line in lines:
        stripped = line.strip()
        if stripped.startswith("[") and stripped.endswith("]"):
            current = stripped[1:-1].strip()
        if current == section and stripped.startswith(f"{key} ="):
            output.append(f"{key} = {value}")
            replaced = True
        else:
            output.append(line)
    if not replaced:
        raise RuntimeError(f"missing [{section}] {key} in source config")
    return "\n".join(output) + "\n"


def u32(value: int | str) -> bytes:
    if isinstance(value, str):
        value = int(value, 0)
    return struct.pack("<I", int(value) & 0xFFFFFFFF)


def h32(value: str) -> bytes:
    raw = bytes.fromhex(value)
    if len(raw) != 32:
        raise ValueError("expected 32-byte hash")
    return raw


def block_hash(block: dict) -> str:
    header = block["header"]
    raw = (
        u32(header["version"])
        + h32(header["prev_block_hash"])
        + h32(header["merkle_root"])
        + u32(header["time"])
        + u32(header["bits"])
        + u32(header["nonce"])
    )
    return hashlib.sha256(hashlib.sha256(raw).digest()).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--qubd", required=True, help="Path to qubd/qubd.exe")
    parser.add_argument(
        "--config",
        default=str(Path(__file__).resolve().parents[1] / "config" / "regtest.toml"),
        help="Source regtest TOML",
    )
    parser.add_argument("--keep", action="store_true", help="Keep temporary directory")
    args = parser.parse_args()

    qubd = str(Path(args.qubd).resolve())
    source_config = Path(args.config).resolve()
    if not Path(qubd).is_file():
        raise SystemExit(f"qubd not found: {qubd}")
    if not source_config.is_file():
        raise SystemExit(f"config not found: {source_config}")

    temp_root = Path(tempfile.mkdtemp(prefix="qub-hf123-fce-e2e-"))
    try:
        data_dir = temp_root / "data"
        config_path = temp_root / "regtest-hf123.toml"
        config_text = source_config.read_text(encoding="utf-8")
        data_value = json.dumps(str(data_dir).replace("\\", "/"))
        config_text = replace_toml_value(config_text, "node", "data_dir", data_value)
        config_text = replace_toml_value(config_text, "p2p", "enabled", "false")
        config_text = replace_toml_value(config_text, "rpc", "enabled", "false")
        config_path.write_text(config_text, encoding="utf-8", newline="\n")

        base = [qubd, "--config", str(config_path)]
        run(base + ["init"])

        fast_dir = data_dir / "chain-v2"
        current_path = fast_dir / "CURRENT.json"
        previous_path = fast_dir / "PREVIOUS.json"
        status_path = data_dir / "chain-status.json"
        legacy_path = data_dir / "chain.json"
        for required in (current_path, status_path, legacy_path):
            if not required.is_file():
                raise RuntimeError(f"missing initialized artifact: {required}")

        initial = run_json(base + ["status-fast"])
        if initial.get("storage_engine") != "QUB-FCE-1" or int(initial.get("height", -1)) != 0:
            raise RuntimeError(f"unexpected initialized status: {initial}")

        run(base + ["mine", "3"])
        status3 = run_json(base + ["status-fast"])
        if int(status3.get("height", -1)) != 3:
            raise RuntimeError("height did not reach 3")
        pointer3 = json.loads(current_path.read_text(encoding="utf-8"))
        if int(pointer3["committed_height"]) != 3:
            raise RuntimeError("CURRENT pointer height is not 3")
        journal = fast_dir / pointer3["blocks_file"]
        committed3 = int(pointer3["journal_bytes"])
        if journal.stat().st_size != committed3:
            raise RuntimeError("journal length differs from committed prefix")

        # Simulate a process crash after journal append but before pointer commit.
        with journal.open("ab") as handle:
            handle.write(b"{uncommitted-hf123-crash-suffix}\n")
            handle.flush()
            os.fsync(handle.fileno())
        if journal.stat().st_size <= committed3:
            raise RuntimeError("could not append synthetic crash suffix")

        run(base + ["mine", "1"])
        status4 = run_json(base + ["status-fast"])
        if int(status4.get("height", -1)) != 4:
            raise RuntimeError("height did not reach 4 after suffix recovery")
        pointer4 = json.loads(current_path.read_text(encoding="utf-8"))
        committed4 = int(pointer4["journal_bytes"])
        current_journal = fast_dir / pointer4["blocks_file"]
        if current_journal.stat().st_size != committed4:
            raise RuntimeError("uncommitted journal suffix was not truncated")
        if not previous_path.is_file():
            raise RuntimeError("PREVIOUS pointer was not created")

        export_path = temp_root / "canonical-export.json"
        export_report = run_json(base + ["export-chain-json", str(export_path)])
        if int(export_report.get("height", -1)) != 4:
            raise RuntimeError("explicit export height mismatch")
        exported = json.loads(export_path.read_text(encoding="utf-8"))
        blocks = exported.get("blocks")
        if not isinstance(blocks, list) or len(blocks) != 5:
            raise RuntimeError("explicit export block count mismatch")
        if block_hash(blocks[-1]) != status4.get("tip_hash"):
            raise RuntimeError("explicit export tip hash mismatch")

        run(base + ["validate"])
        stats = run_json(base + ["storage-stats"])
        if stats.get("storage_engine") != "QUB-FCE-1" or int(stats.get("height", -1)) != 4:
            raise RuntimeError("storage-stats mismatch")

        # Corrupt CURRENT and prove bounded PREVIOUS recovery, then restore the
        # exact current pointer so the isolated test ends at the canonical tip.
        current_backup = current_path.read_bytes()
        current_path.write_bytes(b"{broken-current")
        recovered = run_json(base + ["status-fast"])
        if int(recovered.get("height", -1)) != 3:
            raise RuntimeError(
                f"PREVIOUS recovery did not return prior committed height: {recovered}"
            )
        current_path.write_bytes(current_backup)
        restored = run_json(base + ["status-fast"])
        if int(restored.get("height", -1)) != 4 or restored.get("tip_hash") != status4.get("tip_hash"):
            raise RuntimeError("CURRENT pointer restoration failed")

        print("HF123 FAST CHAIN ENGINE REGTEST E2E: PASS")
        print(f"temporary data: {temp_root}")
        return 0
    finally:
        if args.keep:
            print(f"kept temporary directory: {temp_root}")
        else:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
