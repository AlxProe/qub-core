#!/usr/bin/env python3
"""Build once and verify exact HF132 Windows native evidence before packaging.

Receipts are local evidence, not signatures or a replacement for GUI/mainnet
observations. Every reuse rechecks source contracts, toolchain, build settings
and all executable hashes. No lab network or real wallet is created here.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tempfile
import time
from typing import Any

TARGET = "x86_64-pc-windows-msvc"
SCHEMA = "QUB-HF132-NATIVE-BUILD-1"
MANIFEST = "HF132-SOURCE-MANIFEST.json"
BINS = ("qub-core.exe", "qubd.exe", "qub-rpc-miner.exe")
PHASES = ("source-before", "all-native-tests", "required-native-tests", "release-build",
          "mainnet-preflight", "source-after")
ENV_KEYS = ("QUB_BUILD_CONFIG", "RUSTFLAGS", "RUSTDOCFLAGS", "RUSTC", "RUSTDOC",
            "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS",
            "CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER", "CC", "CXX", "AR")


def regular(path: Path) -> None:
    info = path.lstat()
    if not stat.S_ISREG(info.st_mode) or path.is_symlink() or (
        getattr(info, "st_file_attributes", 0) & getattr(stat, "FILE_ATTRIBUTE_REPARSE_POINT", 1024)
    ):
        raise RuntimeError(f"Expected a regular file without reparse points: {path}")


def sha256(path: Path) -> str:
    regular(path)
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def build_environment(source: Path, original: dict[str, str] | None = None) -> dict[str, str]:
    env = dict(os.environ if original is None else original)
    if env.get("QUB_BUILD_CONFIG", "mainnet") != "mainnet":
        raise RuntimeError("HF132 release native evidence requires QUB_BUILD_CONFIG=mainnet")
    if env.get("CARGO_ENCODED_RUSTFLAGS"):
        raise RuntimeError("CARGO_ENCODED_RUSTFLAGS overrides RUSTFLAGS; unset it for this release build")
    flags = env.get("RUSTFLAGS", "").strip()
    if "-crt-static" in flags:
        raise RuntimeError("RUSTFLAGS explicitly disables the required static Windows CRT")
    if "+crt-static" not in flags:
        flags = (flags + " -C target-feature=+crt-static").strip()
    raw_target = Path(env.get("CARGO_TARGET_DIR") or source / "target")
    target = (raw_target if raw_target.is_absolute() else source / raw_target).resolve()
    env.update(QUB_BUILD_CONFIG="mainnet", RUSTFLAGS=flags,
               CARGO_BUILD_TARGET=TARGET, CARGO_TARGET_DIR=str(target), CARGO_TERM_COLOR="never")
    return env


def version(command: str, args: list[str], source: Path, env: dict[str, str]) -> dict[str, str]:
    executable = shutil.which(command, path=env.get("PATH"))
    if not executable:
        raise RuntimeError(f"Required native command is unavailable: {command}")
    process = subprocess.run([executable, *args], cwd=source, env=env, capture_output=True,
                             text=True, encoding="utf-8", errors="replace", timeout=30)
    if process.returncode:
        raise RuntimeError(f"{command} identity failed ({process.returncode}): {process.stderr}")
    return {"path": str(Path(executable).resolve()), "identity": process.stdout.strip()}


def identity(source: Path, env: dict[str, str]) -> dict[str, Any]:
    return {
        "source": str(source), "manifest_sha256": sha256(source / MANIFEST),
        "cargo_lock_sha256": sha256(source / "Cargo.lock"),
        "build_logo_sha256": sha256(source / "assets" / "qubit-coin-logo.png"),
        "target": TARGET, "target_root": env["CARGO_TARGET_DIR"],
        "environment": {key: env.get(key, "") for key in ENV_KEYS},
        "rustc": version("rustc", ["--version", "--verbose"], source, env),
        "cargo": version("cargo", ["--version", "--verbose"], source, env),
    }


def executable_inventory(env: dict[str, str]) -> dict[str, dict[str, Any]]:
    release = Path(env["CARGO_TARGET_DIR"]) / TARGET / "release"
    result = {}
    for name in BINS:
        path = release / name
        regular(path)
        with path.open("rb") as stream:
            if stream.read(2) != b"MZ":
                raise RuntimeError(f"Native output is not a Windows executable: {path}")
            stream.seek(0x3c)
            offset_bytes = stream.read(4)
            if len(offset_bytes) != 4:
                raise RuntimeError(f"Native output has an incomplete PE header: {path}")
            stream.seek(int.from_bytes(offset_bytes, "little"))
            if stream.read(6) != b"PE\x00\x00\x64\x86":
                raise RuntimeError(f"Native output is not PE AMD64: {path}")
        result[name] = {"path": str(path), "sha256": sha256(path), "size": path.stat().st_size}
    return result


def run(command: list[str], cwd: Path, env: dict[str, str], log: Path,
        timeout: int | None = None) -> None:
    print("+ " + subprocess.list2cmdline(command), flush=True)
    with log.open("w", encoding="utf-8", newline="\n") as evidence:
        evidence.write("Command: " + subprocess.list2cmdline(command) + "\n")
        evidence.flush()
        if timeout is not None:
            result = subprocess.run(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                    stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                                    errors="replace", timeout=timeout)
            evidence.write(result.stdout)
            print(result.stdout, end="", flush=True)
            code = result.returncode
        else:
            process = subprocess.Popen(command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                                       stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                                       errors="replace")
            assert process.stdout is not None
            try:
                for line in process.stdout:
                    evidence.write(line)
                    print(line, end="", flush=True)
                code = process.wait()
            except BaseException:
                process.terminate()
                try:
                    process.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                raise
        evidence.write(f"\nExit code: {code}\n")
    if code:
        raise RuntimeError(f"Native command failed ({code}); evidence: {log}")


def source_command(source: Path) -> list[str]:
    return [sys.executable, str(source / "scripts" / "test-hf132-source-contracts.py"),
            "--mode", "local-build", "--root", str(source)]


def ensure_external_receipt(source: Path, receipt: Path) -> None:
    if receipt == source or source in receipt.parents:
        raise RuntimeError("Native evidence must be outside the manifest-verified source tree")


def check_receipt(receipt: dict[str, Any], current: dict[str, Any],
                  binaries: dict[str, dict[str, Any]]) -> None:
    if receipt.get("schema") != SCHEMA or receipt.get("ok") is not True:
        raise RuntimeError("Missing or failed HF132 native-build evidence")
    if receipt.get("identity") != current:
        raise RuntimeError("Native evidence inputs changed; run the full HF132 native build again")
    if receipt.get("binaries") != binaries:
        raise RuntimeError("Native executable identity changed; run the full HF132 native build again")
    phases = receipt.get("phases")
    if (not isinstance(phases, list) or not all(isinstance(item, dict) for item in phases)
            or [item.get("name") for item in phases] != list(PHASES)):
        raise RuntimeError("Native receipt is missing required completed phases")
    for phase in phases:
        if phase.get("exit_code") != 0 or not phase.get("log_sha256"):
            raise RuntimeError("Native receipt contains an incomplete or failed phase")
        if sha256(Path(phase["log"])) != phase["log_sha256"]:
            raise RuntimeError(f"Native evidence log changed: {phase['name']}")


def build(source: Path, receipt_path: Path, env: dict[str, str]) -> None:
    if sys.platform != "win32":
        raise RuntimeError("HF132 native Windows build must run on the Windows Rust host")
    ensure_external_receipt(source, receipt_path)
    if receipt_path.exists():
        raise RuntimeError(f"Receipt already exists; verify it or use a new evidence path: {receipt_path}")
    receipt_path.parent.mkdir(parents=True, exist_ok=True)
    logs = Path(tempfile.mkdtemp(prefix="hf132-native-logs-", dir=receipt_path.parent))
    phases: list[dict[str, Any]] = []

    def phase(name: str, command: list[str], cwd: Path = source, timeout: int | None = None) -> None:
        log = logs / (name + ".log")
        run(command, cwd, env, log, timeout)
        phases.append({"name": name, "exit_code": 0, "log": str(log), "log_sha256": sha256(log)})

    before = identity(source, env)
    phase("source-before", source_command(source))
    # Archive mtimes are deterministic and may predate a reused Cargo cache.
    # Touch only build inputs; contents remain manifest-identical.
    for path in [source / "build.rs", source / "Cargo.toml", *sorted((source / "src").rglob("*.rs"))]:
        regular(path)
        os.utime(path, None)
    phase("all-native-tests", ["cargo", "test", "--locked", "--all-targets", "--target", TARGET])
    phase("required-native-tests", [sys.executable, str(source / "scripts" / "test-hf132-native-tests.py"), "--scope", "all"])
    phase("release-build", ["cargo", "build", "--locked", "--release", "--target", TARGET,
                            "--bin", "qub-core", "--bin", "qubd", "--bin", "qub-rpc-miner"])
    binaries = executable_inventory(env)
    with tempfile.TemporaryDirectory(prefix="qub-hf132-native-preflight-") as temporary:
        phase("mainnet-preflight", [binaries["qubd.exe"]["path"], "--config",
                                    str(source / "config" / "mainnet.toml"), "preflight"],
              Path(temporary), 180)
    phase("source-after", source_command(source))
    after = identity(source, env)
    if after != before or executable_inventory(env) != binaries:
        raise RuntimeError("Build inputs or outputs changed during validation; no successful receipt written")
    receipt = {"schema": SCHEMA, "ok": True, "created_at_unix": int(time.time()),
               "identity": after, "binaries": binaries, "phases": phases,
               "scope": "native tests and disposable mainnet preflight; no GUI/GPU/mainnet mining evidence"}
    check_receipt(receipt, after, binaries)
    # On this Windows-only build path rename is atomic and refuses an existing
    # destination. A partial JSON file is never exposed as completed evidence.
    descriptor, pending_name = tempfile.mkstemp(prefix=".hf132-native-", suffix=".tmp", dir=receipt_path.parent)
    pending = Path(pending_name)
    try:
        with os.fdopen(descriptor, "w", encoding="utf-8", newline="\n") as output:
            json.dump(receipt, output, indent=2)
            output.write("\n")
            output.flush()
            os.fsync(output.fileno())
        os.rename(pending, receipt_path)
    finally:
        pending.unlink(missing_ok=True)
    print(f"HF132 NATIVE BUILD: PASS - {receipt_path}")


def verify(source: Path, receipt_path: Path, env: dict[str, str]) -> None:
    ensure_external_receipt(source, receipt_path)
    regular(receipt_path)
    receipt = json.loads(receipt_path.read_text(encoding="utf-8-sig"))
    with tempfile.TemporaryDirectory(prefix="qub-hf132-receipt-verify-") as temporary:
        run(source_command(source), source, env, Path(temporary) / "source-contract.log")
    check_receipt(receipt, identity(source, env), executable_inventory(env))
    print(f"HF132 NATIVE EVIDENCE VERIFIED: PASS - {receipt_path}")


def main() -> int:
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="backslashreplace")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("build", "verify"))
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--receipt", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.resolve(strict=True)
    receipt = args.receipt.resolve()
    env = build_environment(source)
    {"build": build, "verify": verify}[args.action](source, receipt, env)
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError, KeyError, subprocess.SubprocessError) as error:
        print(f"HF132 NATIVE BUILD: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
