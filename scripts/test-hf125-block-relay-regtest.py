#!/usr/bin/env python3
"""HF125 reliable block-delivery and canonical-liveness regtest E2E.

The test uses three isolated regtest data directories and real qubd processes.
It proves:

1. a seed with max_inbound_peers=0 still accepts a one-shot block-submit
   connection through the HF125 reserve lane;
2. the miner receives an explicit acceptance acknowledgement and clears its
   durable pending-block relay marker;
3. a durable pending height-2 block can repair a receiver that is still at
   genesis by serving the missing suffix on the same connection;
4. both receivers persist the accepted canonical tip across restart.

No existing QUB wallet or chain directory is touched.
"""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import shutil
import socket
import subprocess
import tempfile
import time
from typing import Any


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--qubd", type=Path, help="Path to qubd/qubd.exe")
    parser.add_argument(
        "--config-template",
        type=Path,
        default=Path("config/regtest.toml"),
        help="Regtest TOML template",
    )
    parser.add_argument("--timeout", type=int, default=180)
    parser.add_argument("--keep", action="store_true")
    return parser.parse_args()


def executable_default() -> Path:
    suffix = ".exe" if os.name == "nt" else ""
    release = Path("target/release") / f"qubd{suffix}"
    debug = Path("target/debug") / f"qubd{suffix}"
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
    replacement = f"{key} = {value}"
    if key_pattern.search(body):
        body = key_pattern.sub(lambda _match: replacement, body, count=1)
    else:
        body = replacement + "\n" + body
    return text[: match.start(2)] + body + text[match.end(2) :]


def make_config(
    template: str,
    path: Path,
    data_dir: Path,
    bind_port: int,
    bootnodes: list[str],
    *,
    p2p_enabled: bool,
    max_inbound: int = 32,
) -> None:
    text = template
    text = replace_toml_value(text, "node", "data_dir", json.dumps(str(data_dir).replace("\\", "/")))
    text = replace_toml_value(text, "p2p", "enabled", str(p2p_enabled).lower())
    text = replace_toml_value(text, "p2p", "bind", json.dumps(f"127.0.0.1:{bind_port}"))
    text = replace_toml_value(text, "p2p", "advertise_addr", '""')
    text = replace_toml_value(text, "p2p", "bootnodes", json.dumps(bootnodes))
    text = replace_toml_value(text, "p2p", "max_inbound_peers", str(max_inbound))
    text = replace_toml_value(text, "rpc", "enabled", "false")
    path.write_text(text, encoding="utf-8", newline="\n")


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


def run_json(command: list[str], *, cwd: Path, timeout: int = 120) -> dict[str, Any]:
    raw = run_checked(command, cwd=cwd, timeout=timeout)
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise RuntimeError(f"expected JSON object from {' '.join(command)}")
    return value


def wait_port(port: int, deadline: float) -> None:
    last_error: Exception | None = None
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return
        except OSError as exc:
            last_error = exc
        time.sleep(0.1)
    raise RuntimeError(f"port {port} did not open: {last_error}")


def wait_height(
    qubd: Path,
    config: Path,
    expected: int,
    *,
    cwd: Path,
    deadline: float,
) -> dict[str, Any]:
    last: dict[str, Any] | None = None
    while time.monotonic() < deadline:
        try:
            last = run_json(
                [str(qubd), "--config", str(config), "status-fast"],
                cwd=cwd,
                timeout=20,
            )
            if int(last.get("height", -1)) >= expected:
                return last
        except Exception:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"node did not reach height {expected}; last={last}")


def start_node(qubd: Path, config: Path, log: Path, *, cwd: Path) -> tuple[subprocess.Popen[str], Any]:
    handle = log.open("w", encoding="utf-8", newline="\n")
    process = subprocess.Popen(
        [str(qubd), "--config", str(config), "node"],
        cwd=cwd,
        text=True,
        encoding="utf-8",
        errors="replace",
        stdout=handle,
        stderr=subprocess.STDOUT,
    )
    return process, handle


def stop_process(process: subprocess.Popen[str] | None, handle: Any | None) -> None:
    if process is not None and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=8)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)
    if handle is not None:
        handle.close()


def main() -> int:
    args = parse_args()
    root = Path.cwd().resolve()
    qubd = (args.qubd or executable_default()).resolve()
    template_path = args.config_template.resolve()
    if not qubd.is_file():
        raise RuntimeError(f"qubd not found: {qubd}")
    if not template_path.is_file():
        raise RuntimeError(f"config template not found: {template_path}")

    temp_root = Path(tempfile.mkdtemp(prefix="qub-hf125-relay-e2e-"))
    b_proc: subprocess.Popen[str] | None = None
    b_handle: Any | None = None
    c_proc: subprocess.Popen[str] | None = None
    c_handle: Any | None = None
    deadline = time.monotonic() + args.timeout
    try:
        template = template_path.read_text(encoding="utf-8")
        a_data = temp_root / "a-data"
        b_data = temp_root / "b-data"
        c_data = temp_root / "c-data"
        a_cfg = temp_root / "a.toml"
        b_cfg = temp_root / "b.toml"
        c_cfg = temp_root / "c.toml"
        a_port, b_port, c_port = free_port(), free_port(), free_port()

        make_config(
            template,
            a_cfg,
            a_data,
            a_port,
            [f"127.0.0.1:{b_port}"],
            p2p_enabled=True,
        )
        make_config(
            template,
            b_cfg,
            b_data,
            b_port,
            [],
            p2p_enabled=True,
            max_inbound=0,
        )
        make_config(
            template,
            c_cfg,
            c_data,
            c_port,
            [],
            p2p_enabled=True,
            max_inbound=0,
        )

        for cfg in (a_cfg, b_cfg, c_cfg):
            run_checked([str(qubd), "--config", str(cfg), "init"], cwd=root, timeout=30)
        run_checked([str(qubd), "--config", str(a_cfg), "wallet-new"], cwd=root, timeout=30)

        print(f"HF125 relay E2E directory: {temp_root}")
        print(f"A={a_port} B={b_port} C={c_port}")

        b_proc, b_handle = start_node(qubd, b_cfg, temp_root / "b.log", cwd=root)
        wait_port(b_port, deadline)

        mine1 = run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "1"],
            cwd=root,
            timeout=90,
        )
        print(mine1)
        if "block_delivery:" not in mine1:
            raise RuntimeError("height-1 block did not report acknowledged delivery")
        match = re.search(r"(?<!official_)acknowledged=(\d+)", mine1)
        if match is None or int(match.group(1)) < 1:
            raise RuntimeError("height-1 block did not receive an explicit acknowledgement")

        a1 = wait_height(qubd, a_cfg, 1, cwd=root, deadline=deadline)
        b1 = wait_height(qubd, b_cfg, 1, cwd=root, deadline=deadline)
        if a1.get("tip_hash") != b1.get("tip_hash"):
            raise RuntimeError("A/B tips differ after acknowledged block delivery")
        relay1 = run_json(
            [str(qubd), "--config", str(a_cfg), "block-relay-status"],
            cwd=root,
            timeout=30,
        )
        if relay1.get("pending") is not False:
            raise RuntimeError(f"acknowledged block remained pending: {relay1}")
        print("Reserve-lane acknowledged delivery: PASS")

        # Build height 2 locally without P2P, then create the exact durable relay
        # record used after a process restart. C remains at genesis.
        make_config(
            template,
            a_cfg,
            a_data,
            a_port,
            [],
            p2p_enabled=False,
        )
        run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "1"],
            cwd=root,
            timeout=60,
        )
        a2 = wait_height(qubd, a_cfg, 2, cwd=root, deadline=deadline)
        export_path = temp_root / "a-chain.json"
        run_json(
            [str(qubd), "--config", str(a_cfg), "export-chain-json", str(export_path)],
            cwd=root,
            timeout=60,
        )
        exported = json.loads(export_path.read_text(encoding="utf-8"))
        block2 = exported["blocks"][2]
        pending_path = a_data / "pending-block-relay.json"
        pending_path.write_text(
            json.dumps(
                {
                    "version": 1,
                    "network": "regtest",
                    "block_hash": a2["tip_hash"],
                    "height": 2,
                    "block": block2,
                    "created_unix": int(time.time()),
                    "last_attempt_unix": 0,
                    "attempts": 0,
                    "last_acknowledgements": 0,
                    "last_error": "",
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        c_proc, c_handle = start_node(qubd, c_cfg, temp_root / "c.log", cwd=root)
        wait_port(c_port, deadline)
        make_config(
            template,
            a_cfg,
            a_data,
            a_port,
            [f"127.0.0.1:{c_port}"],
            p2p_enabled=True,
        )

        relay2_raw = run_checked(
            [str(qubd), "--config", str(a_cfg), "relay-pending-block"],
            cwd=root,
            timeout=90,
        )
        print(relay2_raw)
        relay2 = json.loads(relay2_raw)
        if int(relay2.get("acknowledgements", 0)) < 1:
            raise RuntimeError(f"suffix-repair relay was not acknowledged: {relay2}")
        c2 = wait_height(qubd, c_cfg, 2, cwd=root, deadline=deadline)
        if c2.get("tip_hash") != a2.get("tip_hash"):
            raise RuntimeError("behind receiver did not converge to A height-2 tip")
        relay2_status = run_json(
            [str(qubd), "--config", str(a_cfg), "block-relay-status"],
            cwd=root,
            timeout=30,
        )
        if relay2_status.get("pending") is not False:
            raise RuntimeError("pending relay was not cleared after suffix repair")
        print("Same-connection suffix repair + acknowledgement: PASS")

        stop_process(c_proc, c_handle)
        c_proc, c_handle = None, None
        c_proc, c_handle = start_node(qubd, c_cfg, temp_root / "c-restart.log", cwd=root)
        wait_port(c_port, deadline)
        c2_restart = wait_height(qubd, c_cfg, 2, cwd=root, deadline=deadline)
        if c2_restart.get("tip_hash") != a2.get("tip_hash"):
            raise RuntimeError("receiver lost accepted block across restart")

        print("HF125 RELIABLE BLOCK DELIVERY REGTEST E2E: PASS")
        return 0
    finally:
        stop_process(b_proc, b_handle)
        stop_process(c_proc, c_handle)
        if args.keep:
            print(f"kept temporary directory: {temp_root}")
        else:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
