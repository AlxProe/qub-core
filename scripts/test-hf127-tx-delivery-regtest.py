#!/usr/bin/env python3
"""HF127 reliable transaction delivery and independent-miner regtest E2E.

The test uses three isolated regtest nodes and proves:

1. a sender with mining disabled receives an explicit transaction ACK;
2. a seed whose single normal inbound slot is occupied still accepts the
   transaction through the bounded tx-submit reserve lane;
3. the seed relays the accepted transaction to an already-connected,
   independent miner;
4. the independent miner includes and confirms the transaction;
5. the sender never mines, receives two confirmations from another miner,
   clears its durable delivery record, and the receiver preserves the result
   across restart.

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
    parser.add_argument("--timeout", type=int, default=240)
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


def phase_deadline(timeout: int) -> float:
    """Give each independent E2E phase its full bounded timeout budget."""
    return time.monotonic() + max(1, timeout)


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
    raw = run_checked(command, cwd=cwd, timeout=timeout)
    value = json.loads(raw)
    if not isinstance(value, dict):
        raise RuntimeError(f"expected JSON object from {' '.join(command)}")
    return value


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


def wait_port(port: int, deadline: float) -> None:
    while time.monotonic() < deadline:
        try:
            with socket.create_connection(("127.0.0.1", port), timeout=0.5):
                return
        except OSError:
            time.sleep(0.1)
    raise RuntimeError(f"port {port} did not open")


def wait_height(
    qubd: Path, config: Path, expected: int, *, cwd: Path, deadline: float
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


def wait_mempool_tx(
    qubd: Path, config: Path, txid: str, *, cwd: Path, deadline: float
) -> None:
    last = ""
    while time.monotonic() < deadline:
        try:
            last = run_checked(
                [str(qubd), "--config", str(config), "mempool"],
                cwd=cwd,
                timeout=20,
            )
            if txid in last:
                return
        except Exception:
            pass
        time.sleep(0.25)
    raise RuntimeError(f"transaction {txid} did not reach mempool; last={last}")


def copy_chain_state(source: Path, target: Path) -> None:
    source_fast = source / "chain-v2"
    target_fast = target / "chain-v2"
    if not source_fast.is_dir():
        raise RuntimeError(f"source Fast Chain Engine directory missing: {source_fast}")
    shutil.rmtree(target_fast, ignore_errors=True)
    shutil.copytree(source_fast, target_fast)
    for name in ("chain-status.json", "chain.json"):
        src = source / name
        if src.is_file():
            shutil.copy2(src, target / name)


def parse_address(output: str) -> str:
    match = re.search(r"(?m)^address:\s*(\S+)\s*$", output)
    if not match:
        raise RuntimeError(f"wallet-new did not return an address:\n{output}")
    return match.group(1)


def parse_txid(output: str) -> str:
    match = re.search(r"(?m)^txid:\s*([0-9a-fA-F]{64})\s*$", output)
    if not match:
        raise RuntimeError(f"send did not return a txid:\n{output}")
    return match.group(1).lower()


def main() -> int:
    args = parse_args()
    root = Path.cwd().resolve()
    qubd = (args.qubd or executable_default()).resolve()
    template_path = args.config_template.resolve()
    if not qubd.is_file():
        raise RuntimeError(f"qubd not found: {qubd}")
    if not template_path.is_file():
        raise RuntimeError(f"config template not found: {template_path}")

    temp_root = Path(tempfile.mkdtemp(prefix="qub-hf127-tx-e2e-"))
    a_proc: subprocess.Popen[str] | None = None
    a_handle: Any | None = None
    b_proc: subprocess.Popen[str] | None = None
    b_handle: Any | None = None
    c_proc: subprocess.Popen[str] | None = None
    c_handle: Any | None = None

    try:
        template = template_path.read_text(encoding="utf-8")
        a_data, b_data, c_data = (
            temp_root / "a-data",
            temp_root / "b-data",
            temp_root / "c-data",
        )
        a_cfg, b_cfg, c_cfg = (
            temp_root / "a.toml",
            temp_root / "b.toml",
            temp_root / "c.toml",
        )
        a_port, b_port, c_port = free_port(), free_port(), free_port()

        make_config(template, a_cfg, a_data, a_port, [], p2p_enabled=False)
        make_config(
            template,
            b_cfg,
            b_data,
            b_port,
            [],
            p2p_enabled=True,
            max_inbound=1,
        )
        make_config(
            template,
            c_cfg,
            c_data,
            c_port,
            [f"127.0.0.1:{b_port}"],
            p2p_enabled=True,
        )

        for cfg in (a_cfg, b_cfg, c_cfg):
            run_checked([str(qubd), "--config", str(cfg), "init"], cwd=root, timeout=30)

        run_checked([str(qubd), "--config", str(a_cfg), "wallet-new"], cwd=root, timeout=30)
        b_wallet = run_checked(
            [str(qubd), "--config", str(b_cfg), "wallet-new"], cwd=root, timeout=30
        )
        recipient = parse_address(b_wallet)
        run_checked([str(qubd), "--config", str(c_cfg), "wallet-new"], cwd=root, timeout=30)

        # Fund A and mature its coinbase entirely offline. A never runs a miner
        # after the transaction is created.
        run_checked(
            [str(qubd), "--config", str(a_cfg), "mine", "3"],
            cwd=root,
            timeout=120,
        )
        a3 = wait_height(qubd, a_cfg, 3, cwd=root, deadline=phase_deadline(args.timeout))
        copy_chain_state(a_data, b_data)
        copy_chain_state(a_data, c_data)

        # Enable A's P2P only after the common chain is prepared.
        make_config(
            template,
            a_cfg,
            a_data,
            a_port,
            [f"127.0.0.1:{b_port}", f"127.0.0.1:{c_port}"],
            p2p_enabled=True,
        )

        print(f"HF127 transaction E2E directory: {temp_root}")
        print(f"A={a_port} B={b_port} C={c_port} common_tip={a3.get('tip_hash')}")

        b_proc, b_handle = start_node(qubd, b_cfg, temp_root / "b.log", cwd=root)
        wait_port(b_port, phase_deadline(args.timeout))
        c_proc, c_handle = start_node(qubd, c_cfg, temp_root / "c.log", cwd=root)
        wait_port(c_port, phase_deadline(args.timeout))
        time.sleep(2.0)

        send_output = run_checked(
            [
                str(qubd),
                "--config",
                str(a_cfg),
                "send",
                recipient,
                "1",
                "0.00001",
            ],
            cwd=root,
            timeout=90,
        )
        print(send_output)
        txid = parse_txid(send_output)
        ack_match = re.search(r"(?m)^acknowledged_by_peers:\s*(\d+)\s*$", send_output)
        if ack_match is None or int(ack_match.group(1)) < 1:
            raise RuntimeError("sender did not receive an explicit transaction acknowledgement")

        delivery = run_json(
            [str(qubd), "--config", str(a_cfg), "tx-delivery-status", txid],
            cwd=root,
            timeout=30,
        )
        rows = delivery.get("transactions")
        if not isinstance(rows, list) or len(rows) != 1:
            raise RuntimeError(f"durable delivery record missing: {delivery}")
        if not bool(rows[0].get("network_delivered")):
            raise RuntimeError(f"transaction was not marked delivered: {rows[0]}")
        print("Reserve-lane transaction acknowledgement: PASS")

        wait_mempool_tx(
            qubd, b_cfg, txid, cwd=root, deadline=phase_deadline(args.timeout)
        )
        wait_mempool_tx(
            qubd, c_cfg, txid, cwd=root, deadline=phase_deadline(args.timeout)
        )
        print("Seed-to-independent-miner mempool propagation: PASS")

        # Stop C's node process before its CLI miner takes ownership of C's data
        # directory. A is already gone and never mines the transaction.
        stop_process(c_proc, c_handle)
        c_proc, c_handle = None, None
        mine_output = run_checked(
            [str(qubd), "--config", str(c_cfg), "mine", "2"],
            cwd=root,
            timeout=180,
        )
        print(mine_output)
        if "block_delivery:" not in mine_output:
            raise RuntimeError("independent miner did not receive block acknowledgement")

        c5 = wait_height(qubd, c_cfg, 5, cwd=root, deadline=phase_deadline(args.timeout))
        c_proc, c_handle = start_node(qubd, c_cfg, temp_root / "c-restart.log", cwd=root)
        wait_port(c_port, phase_deadline(args.timeout))
        b5 = wait_height(qubd, b_cfg, 5, cwd=root, deadline=phase_deadline(args.timeout))
        if b5.get("tip_hash") != c5.get("tip_hash"):
            raise RuntimeError("seed and independent miner tips differ after confirmation")
        if txid in run_checked(
            [str(qubd), "--config", str(b_cfg), "mempool"], cwd=root, timeout=30
        ):
            raise RuntimeError("confirmed transaction remained in seed mempool")
        print("Independent miner two-confirmation result with sender offline: PASS")

        # Bring the sender online only after both blocks exist. It must sync and
        # reconcile the durable outbox without performing any mining itself.
        a_proc, a_handle = start_node(qubd, a_cfg, temp_root / "a-reconcile.log", cwd=root)
        wait_port(a_port, phase_deadline(args.timeout))
        a5 = wait_height(qubd, a_cfg, 5, cwd=root, deadline=phase_deadline(args.timeout))
        if a5.get("tip_hash") != c5.get("tip_hash"):
            raise RuntimeError("sender did not converge to the independent miner tip")
        outbox_deadline = phase_deadline(args.timeout)
        while time.monotonic() < outbox_deadline:
            delivery = run_json(
                [str(qubd), "--config", str(a_cfg), "tx-delivery-status", txid],
                cwd=root,
                timeout=30,
            )
            rows = delivery.get("transactions")
            if isinstance(rows, list) and not rows:
                break
            time.sleep(0.5)
        else:
            raise RuntimeError(f"sender outbox did not clear after two confirmations: {delivery}")
        print("Sender durable outbox cleared after independent confirmations: PASS")

        stop_process(b_proc, b_handle)
        b_proc, b_handle = None, None
        b_proc, b_handle = start_node(qubd, b_cfg, temp_root / "b-restart.log", cwd=root)
        wait_port(b_port, phase_deadline(args.timeout))
        b5_restart = wait_height(qubd, b_cfg, 5, cwd=root, deadline=phase_deadline(args.timeout))
        if b5_restart.get("tip_hash") != c5.get("tip_hash"):
            raise RuntimeError("receiver lost confirmed transaction block across restart")

        print("HF127 RELIABLE TRANSACTION DELIVERY REGTEST E2E: PASS")
        return 0
    finally:
        stop_process(a_proc, a_handle)
        stop_process(b_proc, b_handle)
        stop_process(c_proc, c_handle)
        if args.keep:
            print(f"kept temporary directory: {temp_root}")
        else:
            shutil.rmtree(temp_root, ignore_errors=True)


if __name__ == "__main__":
    raise SystemExit(main())
