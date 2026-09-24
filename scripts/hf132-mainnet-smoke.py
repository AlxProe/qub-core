#!/usr/bin/env python3
"""Prepare a private mainnet GUI copy for candidate and verified portable smoke.

The helper never starts mining, submits transactions, or copies private data
into source/dist. Manual GUI actions use the REAL MAINNET and real wallet keys.
Keep the resulting lab private. COPY_VALIDATED is not a mining-test PASS.
"""
from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shutil
import socket
import stat
import subprocess
import sys
import time
import tomllib
import uuid

SCHEMA = "QUB-HF132-MAINNET-SMOKE-1"
PREFS = "data/qub-core-gui-settings.json"
QUIET_PREFS = {
    "setup_complete": True, "setup_profile": "Mainnet",
    "setup_bootnodes": "", "setup_advertise_addr": "",
    "setup_listen_for_peers": False, "setup_seed_node_mode": False,
    "auto_start_windows": False, "start_mining_on_open": False,
    "start_mining_after_update_restart": False,
    "hf105_auto_restart_default_applied": True,
    "auto_check_updates": False, "auto_download_updates": False,
    "auto_install_updates": False, "stop_miner_on_update_available": False,
    "update_url": "http://127.0.0.1:1/disabled",
    "confirm_plaintext_wallet_risk": False,
    "allow_isolated_regtest_mining": False,
    "auto_sync_wallet_balances": False,
}


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def safe_path(path, *, must_exist=True):
    """Check lexical ancestors before resolving, including Windows junctions."""
    path = Path(os.path.abspath(path))
    for item in (path, *path.parents):
        try:
            info = item.lstat()
        except FileNotFoundError:
            continue
        require(not stat.S_ISLNK(info.st_mode)
                and not (getattr(info, "st_file_attributes", 0) & 0x400),
                f"Symlink/reparse point is not allowed: {item}")
    require(not must_exist or path.exists(), f"Missing path: {path}")
    return path


def regular(path):
    path = safe_path(path)
    require(stat.S_ISREG(path.stat().st_mode), f"Not a regular file: {path}")
    return path


def sha(path):
    with regular(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def read_json(path):
    return json.loads(regular(path).read_text(encoding="utf-8-sig"))


def write_json(path, value):
    path = safe_path(path, must_exist=False)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + "." + uuid.uuid4().hex + ".tmp")
    try:
        with temporary.open("x", encoding="utf-8", newline="\n") as output:
            json.dump(value, output, indent=2)
            output.write("\n")
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def require_stopped(extra_names=()):
    names = {"qub-core.exe", "qubd.exe", "qub-rpc-miner.exe",
             "qub-core", "qubd", "qub-rpc-miner"}
    names.update(str(name).lower() for name in extra_names)
    if os.name == "nt":
        result = subprocess.run(["tasklist.exe", "/FO", "CSV", "/NH"],
                                capture_output=True, text=True, timeout=30)
        require(result.returncode == 0, "Cannot verify QUB processes are stopped")
        running = [row[0] for row in csv.reader(io.StringIO(result.stdout))
                   if row and row[0].lower() in names]
    else:
        running = []
        for item in Path("/proc").glob("[0-9]*/comm"):
            try:
                name = item.read_text().strip()
                if name.lower() in names:
                    running.append(name)
            except (FileNotFoundError, PermissionError):
                continue
    require(not running, "Close all QUB GUI/daemon/miner processes first: " + ", ".join(running))


def no_leases(data):
    for name in ("WRITE.lock", "LEGACY-EXPORT.lock"):
        require(not (data / "chain-v2" / name).exists(),
                f"Writer/export lease exists; close its owner first: {data / 'chain-v2' / name}")



def require_closed_copy_source(data):
    """Old smoke data is reusable only as a checked input to a NEW lab."""
    owner = data.parent
    require(not (owner / "SMOKE.lock").exists(),
            "Smoke helper session is open; close its GUI and helper before copying")
    marker = owner / "smoke.json"
    if marker.exists():
        record = read_json(marker)
        require(data.name == "data" and record.get("schema") in
                {"QUB-HF131-MAINNET-SMOKE-1", SCHEMA},
                "Unrecognized old smoke directory; do not reuse it in place")
        config = regular(owner / "gui/config/mainnet-smoke.toml")
        require(sha(config) == record.get("config_sha256"), "Old smoke configuration changed")
        parsed = tomllib.loads(config.read_text(encoding="utf-8"))
        require(parsed["network"]["name"] == "mainnet"
                and Path(parsed["node"]["data_dir"]) == data,
                "Old smoke source is not its recorded isolated mainnet data")
    no_leases(data)


def child_file(root, name):
    require(isinstance(name, str) and re.fullmatch(r"[A-Za-z0-9_.-]+", name)
            and name not in (".", ".."), "Unsafe FCE child filename")
    return regular(root / name)


def committed_files(data):
    fce = safe_path(data / "chain-v2")
    no_leases(data)
    files = {}
    current = None
    for name in ("CURRENT.json", "PREVIOUS.json"):
        if name == "PREVIOUS.json" and not (fce / name).exists():
            continue
        pointer_path = regular(fce / name)
        pointer = read_json(pointer_path)
        require(pointer.get("magic") == "QUB-FCE-1" and pointer.get("schema_version") == 1
                and pointer.get("network") == "mainnet", "Expected mainnet QUB-FCE-1 pointer")
        require(isinstance(pointer.get("committed_height"), int)
                and pointer["committed_height"] >= (24000 if name == "CURRENT.json" else 0)
                and re.fullmatch(r"[0-9a-f]{64}", str(pointer.get("tip_hash", ""))),
                "Expected existing mainnet history at height >= 24000")
        state_path = child_file(fce, pointer["state_file"])
        journal_path = child_file(fce, pointer["blocks_file"])
        require(sha(state_path) == pointer["state_sha256"], "FCE state checksum mismatch")
        state = read_json(state_path)
        for key in ("schema_version", "magic", "network", "generation", "state_revision",
                    "committed_height", "tip_hash", "journal_bytes", "mempool_digest", "total_work_hex"):
            require(state.get(key) == pointer.get(key), f"FCE pointer/state mismatch: {key}")
        require(isinstance(pointer.get("journal_bytes"), int)
                and pointer["journal_bytes"] > 0
                and journal_path.stat().st_size >= pointer["journal_bytes"], "Truncated FCE journal")
        for path in (pointer_path, state_path, journal_path):
            files[path.relative_to(data).as_posix()] = sha(path)
        if name == "CURRENT.json":
            current = pointer
    require(current is not None, "CURRENT is required; implicit PREVIOUS recovery is not allowed")
    return current, files


def valid_payout(text):
    require(isinstance(text, str) and re.fullmatch(r"qub1[0-9a-fA-F]{48}", text),
            "Use a complete mainnet qub1 address; QNS names are not resolved by this helper")
    raw = bytes.fromhex(text[4:])
    expected = hashlib.sha256(hashlib.sha256(b"qub" + raw[:20]).digest()).digest()[:4]
    require(raw[20:] == expected, "Mainnet payout address checksum mismatch")
    return "qub1" + raw.hex()


def wallet_payout(wallet, explicit=None):
    require(wallet.get("network") == "mainnet" and isinstance(wallet.get("keys"), list),
            "Expected a mainnet wallet.json")
    addresses = [valid_payout(key["address"]) for key in wallet["keys"]]
    if explicit:
        return valid_payout(explicit), valid_payout(explicit) in addresses
    index = wallet.get("default_index")
    require(isinstance(index, int) and not isinstance(index, bool) and 0 <= index < len(addresses),
            "Wallet has no default address; provide --payout with your mainnet address")
    return addresses[index], True


def set_toml(text, section, key, value):
    lines, active, count = text.splitlines(), "", 0
    for index, line in enumerate(lines):
        if line.strip().startswith("["):
            active = line.strip().strip("[]")
        if active == section and re.match(r"^\s*" + re.escape(key) + r"\s*=", line):
            lines[index], count = key + " = " + json.dumps(value), count + 1
    require(count == 1, f"Expected exactly one [{section}] {key}")
    return "\n".join(lines) + "\n"


def quiet_prefs(lab, record, *, fresh=False, runtime=None):
    path = (runtime or lab / "gui") / PREFS
    prefs = {} if fresh or not path.exists() else read_json(path)
    prefs.update(QUIET_PREFS)
    prefs.update(config_path=str(lab / "gui/config/mainnet-smoke.toml"),
                 payout_address=record["payout"])
    prefs.setdefault("cpu_percent", 1)
    prefs.setdefault("gpu_percent", 0)
    prefs.setdefault("cpu_worker_override", 0)
    prefs.setdefault("gpu_work_items_override", 0)
    prefs.setdefault("last_pool_id", "")
    write_json(path, prefs)


def prepare(source, data_source, lab, payout=None):
    source, origin = safe_path(source), safe_path(data_source)
    lab = safe_path(lab, must_exist=False)
    require(not lab.exists(), "Use a NEW smoke directory; existing evidence is never overwritten")
    for other in (source, origin):
        require(lab != other and other not in lab.parents and lab not in other.parents,
                "Smoke directory must be outside source, dist and original data")
    require("dist" not in (part.lower() for part in lab.parts), "Private smoke data must stay outside dist")
    require_stopped()
    require_closed_copy_source(origin)
    current, inventory = committed_files(origin)
    wallet_file = origin / "wallet.json"
    if wallet_file.exists():
        chosen, owned = wallet_payout(read_json(wallet_file), payout)
        inventory["wallet.json"] = sha(wallet_file)
    else:
        require(payout, "No wallet.json found; provide --payout for solo-only smoke")
        chosen, owned = valid_payout(payout), False
    if (origin / "peers.json").exists():
        inventory["peers.json"] = sha(origin / "peers.json")
    assets = safe_path(source / "assets")
    require(assets.is_dir(), "Local runtime assets are required")
    asset_files = []
    for path in assets.rglob("*"):
        safe_path(path)
        if path.is_file():
            asset_files.append(path)
    require(asset_files, "Local runtime assets are empty")
    template = regular(source / "config/mainnet.toml").read_text(encoding="utf-8-sig")
    require(tomllib.loads(template)["network"]["name"] == "mainnet", "Wrong source mainnet config")
    lab.mkdir(parents=True, mode=0o700)
    for relative, digest in inventory.items():
        target = lab / "data" / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(regular(origin / relative), target)
        require(sha(target) == digest, "Mainnet file changed during copy")
    if (lab / "data/wallet.json").exists():
        os.chmod(lab / "data/wallet.json", 0o600)
    for path in asset_files:
        target = lab / "gui/assets" / path.relative_to(assets)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    require_stopped()
    require_closed_copy_source(origin)
    require(all(sha(origin / relative) == digest for relative, digest in inventory.items()),
            "Original data changed during copy; keep this incomplete lab and stop the original writer")
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    for section, key, value in [
        ("node", "data_dir", (lab / "data").as_posix()),
        ("p2p", "bind", f"127.0.0.1:{port}"), ("p2p", "advertise_addr", ""),
        ("rpc", "enabled", False), ("mining", "enabled", False),
        ("mining", "auto_mine_mempool", False),
    ]:
        template = set_toml(template, section, key, value)
    parsed = tomllib.loads(template)
    require(parsed["network"]["address_prefix"] == "qub" and parsed["p2p"]["enabled"],
            "Smoke requires mainnet prefix and outbound P2P")
    config = lab / "gui/config/mainnet-smoke.toml"
    config.parent.mkdir(parents=True)
    config.write_text(template, encoding="utf-8", newline="\n")
    record = {"schema": SCHEMA, "status": "PREPARED", "source": str(source),
              "original_data": str(origin), "payout": chosen, "payout_key_copied": owned,
              "wallet_copied": "wallet.json" in inventory, "config_sha256": sha(config),
              "original_inventory": inventory, "initial_height": current["committed_height"],
              "initial_tip": current["tip_hash"], "created_at_unix": int(time.time()),
              "mining_started_by_helper": False, "native_chain_validated": False}
    quiet_prefs(lab, record, fresh=True)
    write_json(lab / "smoke.json", record)
    print(json.dumps({"lab": str(lab), "config": str(config), "height": current["committed_height"],
                      "payout": chosen, "wallet_copied": record["wallet_copied"],
                      "pool_key_available": owned, "status": "PREPARED"}, indent=2))
    return record


def launch(lab, gui, receipt_path=None, qubd=None, portable=False):
    lab, gui = safe_path(lab), regular(gui)
    record = read_json(lab / "smoke.json")
    require(record.get("schema") == SCHEMA, "Wrong smoke record")
    source = safe_path(record["source"])
    require_stopped((gui.name,))
    config = regular(lab / "gui/config/mainnet-smoke.toml")
    require(sha(config) == record["config_sha256"], "Smoke configuration changed")
    parsed = tomllib.loads(config.read_text(encoding="utf-8"))
    require(Path(parsed["node"]["data_dir"]) == lab / "data"
            and parsed["network"]["name"] == "mainnet", "Smoke config is not isolated mainnet")
    committed_files(lab / "data")
    receipt_hash = None
    if portable:
        # Authenticode changes bytes after the native receipt. The runbook
        # verifies this exact copied bundle against the dist manifest/signature
        # immediately before this command; do not relabel it native-byte-bound.
        require(qubd is not None, "Portable smoke requires explicit --qubd from the copied bundle")
        qubd = regular(qubd)
        provenance = "external-portable-verifier-required"
        print("Portable mode requires the runbook's dist manifest/signature verifier first.")
    else:
        require(receipt_path is not None, "Candidate smoke requires --receipt from the native build")
        receipt_path = regular(receipt_path)
        checked = subprocess.run([sys.executable, str(source / "scripts/hf132-native-build.py"),
                                  "verify", "--source", str(source), "--receipt", str(receipt_path)],
                                 cwd=source, check=False)
        require(checked.returncode == 0, "Native build receipt verification failed")
        receipt = read_json(receipt_path)
        expected = receipt["binaries"]
        qubd = regular(qubd or expected["qubd.exe"]["path"])
        require(sha(gui) == expected["qub-core.exe"]["sha256"], "GUI bytes differ from the tested native build")
        require(sha(qubd) == expected["qubd.exe"]["sha256"], "Daemon bytes differ from the tested native build")
        receipt_hash, provenance = sha(receipt_path), "native-receipt-verified"
    runtime = gui.parent if portable else lab / "gui"
    if portable:
        require(source != runtime and source not in runtime.parents
                and "dist" not in (part.lower() for part in runtime.parts),
                "Use a separate portable COPY outside source/dist")
        original = safe_path(record["original_data"])
        require(runtime != original and runtime not in original.parents and original not in runtime.parents,
                "Portable smoke must not reuse the original installed application's directory")
        for name in ("assets", "config"):
            require(safe_path(runtime / name).is_dir(), f"Portable copy is missing {name}/")
        require(runtime in qubd.parents, "Portable daemon must come from the same copied bundle")
    quiet_prefs(lab, record, runtime=runtime)
    log_dir = lab / "reports" / (str(time.time_ns()) + "-" + uuid.uuid4().hex[:8])
    log_dir.mkdir(parents=True)
    report = {"status": "RUNNING", "gui": str(gui), "gui_sha256": sha(gui),
              "qubd": str(qubd), "qubd_sha256": sha(qubd), "config_sha256": sha(config),
              "receipt_sha256": receipt_hash, "binary_provenance": provenance,
              "runtime": str(runtime), "portable": portable,
              "manual_mining_verdict": "NOT_RECORDED"}
    write_json(log_dir / "session.json", report)
    lock_path = lab / "SMOKE.lock"
    with lock_path.open("x", encoding="ascii") as lock:
        lock.write(str(os.getpid()))
    process = None
    try:
        print("MAINNET COPY: helper starts no mining. Verify payout and choose Solo/Pool manually.")
        print("Stop mining and CLOSE the GUI when finished. Keep this console open.")
        print("Private data: " + str(lab / "data"))
        with (log_dir / "gui.log").open("w", encoding="utf-8") as log:
            process = subprocess.Popen([str(gui), "--config", str(config)], cwd=runtime,
                                       stdout=log, stderr=subprocess.STDOUT)
            try:
                code = process.wait()
            except KeyboardInterrupt:
                print("Close the GUI manually to finish safely; no process was force-killed.", flush=True)
                raise
        require(code == 0, f"GUI exited with code {code}; retain {log_dir}")
        no_leases(lab / "data")
        with (log_dir / "validate.log").open("w", encoding="utf-8") as log:
            checked = subprocess.run([str(qubd), "--config", str(config), "validate"],
                                     cwd=lab / "gui", stdout=log, stderr=subprocess.STDOUT, timeout=900)
        require(checked.returncode == 0, f"Copied mainnet validation failed; see {log_dir}")
        current, _ = committed_files(lab / "data")
        report.update(status="COPY_VALIDATED", height=current["committed_height"], tip=current["tip_hash"])
    except BaseException as error:
        report.update(status="INTERRUPTED_OR_FAILED", error=str(error) or type(error).__name__)
        raise
    finally:
        if process is not None:
            report["gui_pid"] = process.pid
            report["gui_still_running"] = process.poll() is None
        try:
            write_json(log_dir / "session.json", report)
        finally:
            lock_path.unlink(missing_ok=True)
    print("COPY_VALIDATED: " + str(log_dir / "session.json"))
    print("This validates stored chain only. Record actual GUI/mining observations separately.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    setup = commands.add_parser("prepare")
    setup.add_argument("--source", type=Path, required=True)
    setup.add_argument("--data-source", type=Path, required=True)
    setup.add_argument("--lab", type=Path, required=True)
    setup.add_argument("--payout")
    run = commands.add_parser("launch")
    run.add_argument("--lab", type=Path, required=True)
    run.add_argument("--gui", type=Path, required=True)
    run.add_argument("--receipt", type=Path, help="Required for candidate mode; portable trust uses the runbook's dist verifier")
    run.add_argument("--qubd", type=Path)
    run.add_argument("--portable", action="store_true", help="Use a separate portable copy's runtime assets/config directory")
    args = parser.parse_args()
    if args.command == "prepare":
        prepare(args.source, args.data_source, args.lab, args.payout)
    else:
        launch(args.lab, args.gui, args.receipt, args.qubd, args.portable)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        print("HF132 MAINNET SMOKE STOPPED: " + str(error), file=sys.stderr)
        raise SystemExit(1)
