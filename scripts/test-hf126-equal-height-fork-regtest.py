#!/usr/bin/env python3
"""HF126 equal-height fork and overlap-delivery regtest E2E.

The test creates two isolated nodes with a shared height-1 ancestor, mines a
competing height-2 block on each branch, mines height 3 on branch A, and proves
that HF126 delivers the higher-work A suffix to a receiver whose current tip is
the equal-height B sibling. The receiver must adopt A through the bounded
overlap repair, explicitly acknowledge the block, persist the new tip, and keep
it across restart.

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
    text = replace_toml_value(
        text,
        "node",
        "data_dir",
        json.dumps(str(data_dir).replace("\\", "/")),
    )
    text = replace_toml_value(text, "p2p", "enabled", str(p2p_enabled).lower())
    text = replace_toml_value(
        text, "p2p", "bind", json.dumps(f"127.0.0.1:{bind_port}")
    )
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
    value = json.loads(run_checked(command, cwd=cwd, timeout=timeout))
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


def start_node(
    qubd: Path, config: Path, log: Path, *, cwd: Path
) -> tuple[subprocess.Popen[str], Any]:
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

    temp_root = Path(tempfile.mkdtemp(prefix="qub-hf126-equal-tip-e2e-"))
    d_proc: subprocess.Popen[str] | None = None
    d_handle: Any | None = None
    deadline = time.monotonic() + args.timeout
    try:
        template = template_path.read_text(encoding="utf-8")
        a_data = temp_root / "a-data"
        d_data = temp_root / "d-data"
        a_cfg = temp_root / "a.toml"
        d_cfg = temp_root / "d.toml"
        a_port, d_port = free_port(), free_port()

        make_config(template, a_cfg, a_data, a_port, [], p2p_enabled=False)
        run_checked([str(qubd), "--config", str(a_cfg), "init"], cwd=root, timeout=30)
        run_checked(
            [str(qubd), "--config", str(a_cfg), "wallet-new"], cwd=root, timeout=30
        )
        run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "1"], cwd=root, timeout=90
        )
        a1 = wait_height(qubd, a_cfg, 1, cwd=root, deadline=deadline)

        # Clone the fully closed height-1 state so both branches have the exact
        # same ancestor and wallet. Different block timestamps guarantee distinct
        # height-2 headers without involving any existing user data.
        shutil.copytree(a_data, d_data)
        # Copy only committed state semantics. Runtime lease/pending files must not
        # be cloned into the sibling process.
        for relative in (
            Path("chain-v2") / "WRITE.lock",
            Path("pending-block-relay.json"),
            Path("chain-status.json"),
        ):
            candidate = d_data / relative
            if candidate.exists():
                candidate.unlink()
        make_config(template, d_cfg, d_data, d_port, [], p2p_enabled=False)

        run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "1"], cwd=root, timeout=90
        )
        a2 = wait_height(qubd, a_cfg, 2, cwd=root, deadline=deadline)
        time.sleep(1.2)
        run_checked(
            [str(qubd), "--config", str(d_cfg), "mine", "1"], cwd=root, timeout=90
        )
        d2 = wait_height(qubd, d_cfg, 2, cwd=root, deadline=deadline)
        if a2.get("tip_hash") == d2.get("tip_hash"):
            raise RuntimeError("height-2 branches unexpectedly produced the same tip")

        run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "1"], cwd=root, timeout=90
        )
        a3 = wait_height(qubd, a_cfg, 3, cwd=root, deadline=deadline)

        export_path = temp_root / "a-chain.json"
        run_json(
            [str(qubd), "--config", str(a_cfg), "export-chain-json", str(export_path)],
            cwd=root,
            timeout=60,
        )
        exported = json.loads(export_path.read_text(encoding="utf-8"))
        block3 = exported["blocks"][3]
        pending_path = a_data / "pending-block-relay.json"
        pending_path.write_text(
            json.dumps(
                {
                    "version": 1,
                    "network": "regtest",
                    "block_hash": a3["tip_hash"],
                    "height": 3,
                    "block": block3,
                    "created_unix": int(time.time()),
                    "last_attempt_unix": 0,
                    "attempts": 0,
                    "last_acknowledgements": 0,
                    "last_error": "",
                    "last_stale_parent_height": 0,
                    "last_stale_parent_tip": "",
                    "last_stale_parent_reports": 0,
                    "last_official_stale_parent_reports": 0,
                },
                indent=2,
            )
            + "\n",
            encoding="utf-8",
        )

        make_config(
            template,
            d_cfg,
            d_data,
            d_port,
            [],
            p2p_enabled=True,
            max_inbound=0,
        )
        d_proc, d_handle = start_node(qubd, d_cfg, temp_root / "d.log", cwd=root)
        wait_port(d_port, deadline)

        make_config(
            template,
            a_cfg,
            a_data,
            a_port,
            [f"127.0.0.1:{d_port}"],
            p2p_enabled=True,
        )
        relay_raw = run_checked(
            [str(qubd), "--config", str(a_cfg), "relay-pending-block"],
            cwd=root,
            timeout=120,
        )
        print(relay_raw)
        relay = json.loads(relay_raw)
        if int(relay.get("acknowledgements", 0)) < 1:
            raise RuntimeError(f"overlap relay was not acknowledged: {relay}")
        transports = {
            str(item.get("transport", ""))
            for item in relay.get("peer_results", [])
            if isinstance(item, dict)
        }
        if "ack_after_overlap" not in transports:
            raise RuntimeError(f"HF126 overlap transport was not used: {transports}")

        d3 = wait_height(qubd, d_cfg, 3, cwd=root, deadline=deadline)
        if d3.get("tip_hash") != a3.get("tip_hash"):
            raise RuntimeError("equal-height sibling receiver did not adopt A height-3 tip")
        relay_status = run_json(
            [str(qubd), "--config", str(a_cfg), "block-relay-status"],
            cwd=root,
            timeout=30,
        )
        if relay_status.get("pending") is not False:
            raise RuntimeError("pending relay was not cleared after overlap acknowledgement")

        stop_process(d_proc, d_handle)
        d_proc, d_handle = None, None
        d_proc, d_handle = start_node(qubd, d_cfg, temp_root / "d-restart.log", cwd=root)
        wait_port(d_port, deadline)
        d3_restart = wait_height(qubd, d_cfg, 3, cwd=root, deadline=deadline)
        if d3_restart.get("tip_hash") != a3.get("tip_hash"):
            raise RuntimeError("receiver lost overlap-adopted tip across restart")

        print(f"Shared ancestor height-1 tip: {a1.get('tip_hash')}")
        print(f"Competing A height-2 tip:     {a2.get('tip_hash')}")
        print(f"Competing D height-2 tip:     {d2.get('tip_hash')}")
        print("HF126 EQUAL-HEIGHT OVERLAP DELIVERY REGTEST E2E: PASS")
        return 0
    finally:
        stop_process(d_proc, d_handle)
        if args.keep:
            print(f"kept temporary directory: {temp_root}")
        else:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
