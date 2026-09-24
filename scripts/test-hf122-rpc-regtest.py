#!/usr/bin/env python3
"""HF122 regtest end-to-end test for embedded RPC and the reference miner.

The script creates an isolated temporary regtest data directory, starts `qubd node`
with loopback RPC enabled, verifies authentication and header hardening, obtains a
batch of tracked mining jobs, mines one easy regtest block through
`qub-rpc-miner`, validates the new canonical height, and checks the mining
observability response.

No existing QUB data directory or wallet is touched.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any

TOKEN = "hf122-regtest-token-7f67b2dbdc714a58"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qubd", type=Path, help="Path to qubd/qubd.exe")
    parser.add_argument("--miner", type=Path, help="Path to qub-rpc-miner executable")
    parser.add_argument(
        "--config-template",
        type=Path,
        default=Path("config/regtest.toml"),
        help="Regtest TOML template (default: config/regtest.toml)",
    )
    parser.add_argument("--keep", action="store_true", help="Keep temporary test directory")
    parser.add_argument("--timeout", type=int, default=120, help="Overall test timeout seconds")
    return parser.parse_args()


def executable_default(name: str) -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    release = Path("target/release") / f"{name}{suffix}"
    debug = Path("target/debug") / f"{name}{suffix}"
    return release if release.exists() else debug


def free_port() -> int:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def replace_toml_value(text: str, section: str, key: str, value: str) -> str:
    pattern = re.compile(
        rf"(?ms)(^\[{re.escape(section)}\][ \t]*\r?\n)(.*?)(?=^\[|\Z)"
    )
    match = pattern.search(text)
    if not match:
        raise RuntimeError(f"missing TOML section [{section}]")
    body = match.group(2)
    key_pattern = re.compile(rf"(?m)^\s*{re.escape(key)}\s*=.*$")
    replacement = f'{key} = {value}'
    if key_pattern.search(body):
        body = key_pattern.sub(lambda _match: replacement, body, count=1)
    else:
        body = "\n" + replacement + body
    return text[: match.start(2)] + body + text[match.end(2) :]


def run_checked(command: list[str], *, cwd: Path, timeout: int = 120) -> str:
    result = subprocess.run(
        command,
        cwd=cwd,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        timeout=timeout,
        check=False,
    )
    if result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {' '.join(command)}\n{result.stdout}"
        )
    return result.stdout.strip()


def rpc_json(base_url: str, path: str, *, token: str | None = TOKEN) -> Any:
    headers = {"Accept": "application/json"}
    if token is not None:
        headers["X-QUB-RPC-Token"] = token
    request = urllib.request.Request(base_url + path, headers=headers, method="GET")
    with urllib.request.urlopen(request, timeout=10) as response:
        return json.loads(response.read().decode("utf-8"))


def wait_for_rpc(base_url: str, deadline: float) -> dict[str, Any]:
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            value = rpc_json(base_url, "/rpc/v1/status")
            if value.get("ok") is True:
                return value
        except Exception as exc:  # node startup races are expected here
            last_error = exc
        time.sleep(0.25)
    raise RuntimeError(f"RPC did not become ready: {last_error}")


def assert_unauthorized(base_url: str) -> None:
    try:
        rpc_json(base_url, "/rpc/v1/status", token=None)
    except urllib.error.HTTPError as exc:
        if exc.code != 401:
            raise RuntimeError(f"unauthorized request returned HTTP {exc.code}, expected 401")
        return
    raise RuntimeError("unauthorized request unexpectedly succeeded")


def assert_duplicate_token_rejected(host: str, port: int) -> None:
    request = (
        "GET /rpc/v1/status HTTP/1.1\r\n"
        f"Host: {host}:{port}\r\n"
        f"X-QUB-RPC-Token: {TOKEN}\r\n"
        f"X-QUB-RPC-Token: {TOKEN}\r\n"
        "Connection: close\r\n\r\n"
    ).encode("ascii")
    with socket.create_connection((host, port), timeout=5) as sock:
        sock.sendall(request)
        data = bytearray()
        while True:
            chunk = sock.recv(4096)
            if not chunk:
                break
            data.extend(chunk)
    status_line = bytes(data).split(b"\r\n", 1)[0].decode("ascii", "replace")
    if " 400 " not in status_line:
        raise RuntimeError(f"duplicate token headers returned {status_line!r}, expected HTTP 400")


def terminate_process(process: subprocess.Popen[str]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=8)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=5)


def main() -> int:
    args = parse_args()
    root = Path.cwd().resolve()
    qubd = (args.qubd or executable_default("qubd")).resolve()
    miner = (args.miner or executable_default("qub-rpc-miner")).resolve()
    template = args.config_template.resolve()

    for path, label in ((qubd, "qubd"), (miner, "qub-rpc-miner"), (template, "config")):
        if not path.exists():
            raise RuntimeError(f"{label} not found: {path}")

    temp_path = Path(tempfile.mkdtemp(prefix="qub-hf122-rpc-e2e-"))
    node_process: subprocess.Popen[str] | None = None
    started = time.monotonic()
    try:
        data_dir = temp_path / "data"
        config_path = temp_path / "regtest-rpc.toml"
        node_log = temp_path / "node.log"
        p2p_port = free_port()
        rpc_port = free_port()

        config = template.read_text(encoding="utf-8")
        config = replace_toml_value(config, "node", "data_dir", json.dumps(str(data_dir).replace("\\", "/")))
        config = replace_toml_value(config, "p2p", "bind", json.dumps(f"127.0.0.1:{p2p_port}"))
        config = replace_toml_value(config, "p2p", "bootnodes", "[]")
        config = replace_toml_value(config, "rpc", "enabled", "true")
        config = replace_toml_value(config, "rpc", "bind", json.dumps(f"127.0.0.1:{rpc_port}"))
        config = replace_toml_value(config, "rpc", "auth_token", json.dumps(TOKEN))
        config = replace_toml_value(config, "rpc", "auth_token_file", '""')
        config = replace_toml_value(config, "rpc", "allow_remote", "false")
        config = replace_toml_value(config, "rpc", "allowed_cidrs", "[]")
        config_path.write_text(config, encoding="utf-8", newline="\n")

        print(f"HF122 E2E directory: {temp_path}")
        print(f"P2P: 127.0.0.1:{p2p_port} | RPC: 127.0.0.1:{rpc_port}")

        init_output = run_checked(
            [str(qubd), "--config", str(config_path), "init"], cwd=root, timeout=30
        )
        print(init_output)
        wallet_output = run_checked(
            [str(qubd), "--config", str(config_path), "wallet-new"],
            cwd=root,
            timeout=30,
        )
        print(wallet_output)
        address_match = re.search(r"(?m)^address:\s*(\S+)\s*$", wallet_output)
        if not address_match:
            raise RuntimeError("wallet-new output did not contain an address")
        address = address_match.group(1)

        log_handle = node_log.open("w", encoding="utf-8", newline="\n")
        node_process = subprocess.Popen(
            [str(qubd), "--config", str(config_path), "node"],
            cwd=root,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=log_handle,
            stderr=subprocess.STDOUT,
        )

        deadline = started + args.timeout
        base_url = f"http://127.0.0.1:{rpc_port}"
        initial = wait_for_rpc(base_url, deadline)
        initial_height = int(initial.get("height", -1))
        initial_tip = str(initial.get("tip_hash", ""))
        print(f"Embedded RPC ready: height={initial_height} tip={initial_tip}")

        assert_unauthorized(base_url)
        print("Unauthorized request: HTTP 401 OK")
        assert_duplicate_token_rejected("127.0.0.1", rpc_port)
        print("Duplicate sensitive token headers: HTTP 400 OK")

        batch = rpc_json(
            base_url,
            f"/rpc/v1/mining/template-batch?address={address}&count=4",
        )
        templates = batch.get("templates", [])
        if len(templates) != 4:
            raise RuntimeError(f"expected 4 mining templates, got {len(templates)}")
        job_ids = {str(item.get("job_id", "")) for item in templates}
        if len(job_ids) != 4 or "" in job_ids:
            raise RuntimeError("template batch did not contain four unique job IDs")
        if any(int(item.get("height", -1)) != initial_height + 1 for item in templates):
            raise RuntimeError("template batch height is not canonical tip + 1")
        print("Tracked template batch: 4 unique jobs OK")

        miner_output = run_checked(
            [
                str(miner),
                "--rpc",
                base_url,
                "--token",
                TOKEN,
                "--address",
                address,
                "--workers",
                "2",
                "--batch",
                "2",
                "--refresh-secs",
                "10",
                "--once",
                "--max-rounds",
                "5",
            ],
            cwd=root,
            timeout=max(30, args.timeout),
        )
        print(miner_output)
        if "Block accepted." not in miner_output:
            raise RuntimeError("reference miner did not report an accepted block")

        final: dict[str, Any] | None = None
        while time.monotonic() < deadline:
            candidate = rpc_json(base_url, "/rpc/v1/status")
            if int(candidate.get("height", -1)) >= initial_height + 1:
                final = candidate
                break
            time.sleep(0.2)
        if final is None:
            raise RuntimeError("canonical RPC height did not advance after accepted block")
        print(f"Canonical height advanced: {initial_height} -> {final['height']}")

        stats = rpc_json(base_url, "/rpc/v1/mining/stats?window=64")
        distribution = stats.get("distribution", [])
        row = next((item for item in distribution if item.get("label") == address), None)
        if not row or int(row.get("blocks", 0)) < 1:
            raise RuntimeError("mining statistics did not attribute the block to the payout label")
        if stats.get("interpretation_note") is None:
            raise RuntimeError("mining statistics interpretation note is missing")
        print(
            "Mining observability OK: "
            f"label={address} blocks={row['blocks']} HHI={stats.get('hhi')}"
        )

        print("HF122 RPC REGTEST E2E: PASS")
        return 0
    finally:
        if node_process is not None:
            terminate_process(node_process)
        if args.keep:
            print(f"Kept test directory: {temp_path}")
        else:
            shutil.rmtree(temp_path, ignore_errors=True)


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as exc:
        print(f"HF122 RPC REGTEST E2E: FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
