#!/usr/bin/env python3
"""Fixture tests for the isolated mainnet-copy helper; no native GUI/network."""
import contextlib
import hashlib
import importlib.util
import io
import json
from pathlib import Path
import shutil
import tempfile
import tomllib
from types import SimpleNamespace
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("hf132_smoke", Path(__file__).with_name("hf132-mainnet-smoke.py"))
smoke = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(smoke)
TEMPLATE = (Path(__file__).resolve().parents[1] / "config/mainnet.toml").read_text(encoding="utf-8")


def address(seed=7):
    payload = bytes([seed]) * 20
    checksum = hashlib.sha256(hashlib.sha256(b"qub" + payload).digest()).digest()[:4]
    return "qub1" + (payload + checksum).hex()


class SmokeFixtures(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="qub-hf132-smoke-fixture-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source, self.origin, self.lab = (self.root / name for name in ("source", "original", "smoke"))
        (self.source / "config").mkdir(parents=True)
        (self.source / "assets").mkdir()
        (self.source / "config/mainnet.toml").write_text(TEMPLATE, encoding="utf-8")
        (self.source / "assets/icon.png").write_bytes(b"passive fixture")
        (self.origin / "chain-v2").mkdir(parents=True)
        state = {"schema_version": 1, "magic": "QUB-FCE-1", "network": "mainnet",
                 "generation": 7, "state_revision": 8, "committed_height": 70318,
                 "tip_hash": "ab" * 32, "journal_bytes": 3, "mempool_digest": "cd" * 32,
                 "total_work_hex": "ffff", "utxos": [], "mempool": []}
        smoke.write_json(self.origin / "chain-v2/state-7-8.json", state)
        (self.origin / "chain-v2/blocks-7.jsonl").write_bytes(b"{}\n")
        pointer = dict(state, state_file="state-7-8.json", blocks_file="blocks-7.jsonl",
                       state_sha256=smoke.sha(self.origin / "chain-v2/state-7-8.json"))
        smoke.write_json(self.origin / "chain-v2/CURRENT.json", pointer)
        smoke.write_json(self.origin / "wallet.json", {
            "version": 1, "network": "mainnet", "default_index": 0,
            "keys": [{"address": address(), "secret_key_hex": "fixture-private-never-log"}]})
        (self.origin / "pending-txs.json").write_text("must not copy", encoding="utf-8")
        self.stopped = patch.object(smoke, "require_stopped")
        self.stopped.start()
        self.addCleanup(self.stopped.stop)

    def prepare(self, **kwargs):
        with contextlib.redirect_stdout(io.StringIO()):
            return smoke.prepare(self.source, self.origin, self.lab, **kwargs)

    def mutate_pointer(self, **changes):
        path = self.origin / "chain-v2/CURRENT.json"
        value = smoke.read_json(path)
        value.update(changes)
        smoke.write_json(path, value)

    def test_copy_is_private_exact_and_does_not_change_consensus(self):
        before = {str(path): path.read_bytes() for path in self.origin.rglob("*") if path.is_file()}
        record = self.prepare()
        for path, data in before.items():
            self.assertEqual(Path(path).read_bytes(), data)
        self.assertEqual((self.lab / "data/wallet.json").read_bytes(), (self.origin / "wallet.json").read_bytes())
        self.assertFalse((self.lab / "data/pending-txs.json").exists())
        self.assertTrue(record["payout_key_copied"])
        config = tomllib.loads((self.lab / "gui/config/mainnet-smoke.toml").read_text())
        original = tomllib.loads(TEMPLATE)
        for section in ("network", "consensus", "pools", "jin", "jin_swap", "qns"):
            self.assertEqual(config[section], original[section])
        self.assertEqual(config["p2p"]["bootnodes"], original["p2p"]["bootnodes"])
        self.assertEqual(config["node"]["data_dir"], (self.lab / "data").as_posix())
        self.assertFalse(config["rpc"]["enabled"])
        self.assertFalse(config["mining"]["enabled"])

    def test_old_closed_hf131_smoke_can_seed_only_a_new_hf132_copy(self):
        self.prepare()
        marker = self.lab / "smoke.json"
        old = smoke.read_json(marker)
        old["schema"] = "QUB-HF131-MAINNET-SMOKE-1"
        smoke.write_json(marker, old)
        before = {str(path): path.read_bytes() for path in self.lab.rglob("*") if path.is_file()}
        new_lab = self.root / "new-hf132"
        with contextlib.redirect_stdout(io.StringIO()):
            result = smoke.prepare(self.source, self.lab / "data", new_lab)
        self.assertEqual(result["schema"], smoke.SCHEMA)
        self.assertEqual(result["original_data"], str(self.lab / "data"))
        for name, data in before.items():
            self.assertEqual(Path(name).read_bytes(), data)
        self.assertEqual((new_lab / "data/wallet.json").read_bytes(),
                         (self.lab / "data/wallet.json").read_bytes())

    def test_open_old_smoke_helper_refuses_fresh_copy_even_without_qub_process(self):
        self.prepare()
        (self.lab / "SMOKE.lock").write_text("active-helper", encoding="ascii")
        destination = self.root / "blocked-copy"
        with self.assertRaisesRegex(RuntimeError, "helper session is open"):
            smoke.prepare(self.source, self.lab / "data", destination)
        self.assertFalse(destination.exists())
        self.assertTrue((self.lab / "SMOKE.lock").exists())

    def test_old_smoke_wrong_config_identity_refuses_copy(self):
        self.prepare()
        config = self.lab / "gui/config/mainnet-smoke.toml"
        config.write_text(config.read_text() + "\n", encoding="utf-8")
        destination = self.root / "wrong-config-copy"
        with self.assertRaisesRegex(RuntimeError, "Old smoke configuration changed"):
            smoke.prepare(self.source, self.lab / "data", destination)
        self.assertFalse(destination.exists())

    def test_copy_rechecks_old_helper_lock_after_file_copy(self):
        self.prepare()
        copied = shutil.copyfile
        destination = self.root / "racing-copy"

        def starts_helper(source, target):
            result = copied(source, target)
            (self.lab / "SMOKE.lock").write_text("raced-helper", encoding="ascii")
            return result

        with patch.object(smoke.shutil, "copyfile", side_effect=starts_helper):
            with self.assertRaisesRegex(RuntimeError, "helper session is open"):
                smoke.prepare(self.source, self.lab / "data", destination)
        self.assertFalse((destination / "smoke.json").exists())

    def test_reusing_lab_refuses_overwrite(self):
        self.prepare()
        with self.assertRaisesRegex(RuntimeError, "NEW smoke"):
            self.prepare()

    def test_external_payout_is_identified_as_not_owned(self):
        record = self.prepare(payout=address(9))
        self.assertFalse(record["payout_key_copied"])
        self.assertTrue(record["wallet_copied"])

    def test_without_wallet_requires_explicit_payout(self):
        (self.origin / "wallet.json").unlink()
        with self.assertRaisesRegex(RuntimeError, "No wallet"):
            self.prepare()
        record = self.prepare(payout=address())
        self.assertFalse(record["wallet_copied"])

    def test_bad_payout_checksum_refused(self):
        bad = address()[:-1] + ("0" if address()[-1] != "0" else "1")
        with self.assertRaisesRegex(RuntimeError, "checksum"):
            self.prepare(payout=bad)

    def test_wrong_network_wallet_refused(self):
        wallet = smoke.read_json(self.origin / "wallet.json")
        wallet["network"] = "regtest"
        smoke.write_json(self.origin / "wallet.json", wallet)
        with self.assertRaisesRegex(RuntimeError, "mainnet wallet"):
            self.prepare()

    def test_state_corruption_refused_before_copy(self):
        (self.origin / "chain-v2/state-7-8.json").write_text("{}", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "checksum"):
            self.prepare()
        self.assertFalse(self.lab.exists())

    def test_wrong_network_pointer_refused(self):
        self.mutate_pointer(network="regtest")
        with self.assertRaisesRegex(RuntimeError, "mainnet QUB-FCE"):
            self.prepare()

    def test_pointer_traversal_refused(self):
        self.mutate_pointer(state_file="../wallet.json")
        with self.assertRaisesRegex(RuntimeError, "Unsafe FCE"):
            self.prepare()

    def test_truncated_journal_refused(self):
        self.mutate_pointer(journal_bytes=4)
        state = smoke.read_json(self.origin / "chain-v2/state-7-8.json")
        state["journal_bytes"] = 4
        smoke.write_json(self.origin / "chain-v2/state-7-8.json", state)
        self.mutate_pointer(state_sha256=smoke.sha(self.origin / "chain-v2/state-7-8.json"))
        with self.assertRaisesRegex(RuntimeError, "Truncated"):
            self.prepare()

    def test_existing_writer_or_exporter_lease_refused(self):
        for name in ("WRITE.lock", "LEGACY-EXPORT.lock"):
            lock = self.origin / "chain-v2" / name
            lock.write_text("pid=1", encoding="ascii")
            with self.assertRaisesRegex(RuntimeError, "lease exists"):
                self.prepare()
            self.assertTrue(lock.exists())
            lock.unlink()

    def test_source_or_dist_destination_refused(self):
        for destination in (self.source / "smoke", self.root / "dist" / "smoke", self.origin / "smoke"):
            with self.assertRaises(RuntimeError):
                smoke.prepare(self.source, self.origin, destination)

    def test_symlink_ancestor_refused(self):
        link = self.root / "linked"
        try:
            link.symlink_to(self.origin, target_is_directory=True)
        except (OSError, NotImplementedError):
            self.skipTest("Symlinks unavailable")
        with self.assertRaisesRegex(RuntimeError, "Symlink/reparse"):
            smoke.prepare(self.source, link, self.lab)

    def test_copy_detects_original_change(self):
        copy = shutil.copyfile

        def changing_copy(source, target):
            result = copy(source, target)
            if Path(source).name == "wallet.json":
                with Path(source).open("a", encoding="utf-8") as output:
                    output.write(" ")
            return result

        with patch.object(smoke.shutil, "copyfile", side_effect=changing_copy):
            with self.assertRaisesRegex(RuntimeError, "Original data changed"):
                self.prepare()
        self.assertFalse((self.lab / "smoke.json").exists())

    def test_every_launch_resets_updater_and_automatic_mining(self):
        record = self.prepare()
        path = self.lab / "gui" / smoke.PREFS
        prefs = smoke.read_json(path)
        prefs.update(start_mining_on_open=True, start_mining_after_update_restart=True,
                     auto_install_updates=True, auto_check_updates=True,
                     last_pool_id="selected-pool", cpu_percent=33)
        smoke.write_json(path, prefs)
        smoke.quiet_prefs(self.lab, record)
        actual = smoke.read_json(path)
        for key in ("start_mining_on_open", "start_mining_after_update_restart", "auto_install_updates", "auto_check_updates"):
            self.assertFalse(actual[key])
        self.assertEqual(actual["last_pool_id"], "selected-pool")
        self.assertEqual(actual["cpu_percent"], 33)

    def test_candidate_without_native_receipt_refused(self):
        self.prepare()
        gui = self.root / "qub-core.exe"
        gui.write_bytes(b"fake executable")
        with self.assertRaisesRegex(RuntimeError, "requires --receipt"):
            smoke.launch(self.lab, gui)

    def test_candidate_receipt_gui_hash_mismatch_refused(self):
        self.prepare()
        gui, daemon = self.root / "qub-core.exe", self.root / "qubd.exe"
        gui.write_bytes(b"changed GUI")
        daemon.write_bytes(b"tested daemon")
        receipt = self.root / "native.json"
        smoke.write_json(receipt, {"binaries": {
            "qub-core.exe": {"sha256": "00" * 32},
            "qubd.exe": {"path": str(daemon), "sha256": smoke.sha(daemon)}}})
        with patch.object(smoke.subprocess, "run", return_value=SimpleNamespace(returncode=0)):
            with self.assertRaisesRegex(RuntimeError, "GUI bytes differ"):
                smoke.launch(self.lab, gui, receipt)

    def test_portable_refuses_original_application_directory(self):
        self.prepare()
        gui, daemon = self.origin / "QUB-Core.exe", self.origin / "qubd.exe"
        gui.write_bytes(b"original GUI")
        daemon.write_bytes(b"original daemon")
        with contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "original installed"):
                smoke.launch(self.lab, gui, qubd=daemon, portable=True)

    def test_portable_launch_uses_copy_runtime_and_records_external_provenance(self):
        self.prepare()
        portable = self.root / "portable-copy"
        for directory in ("assets", "config", "tools"):
            (portable / directory).mkdir(parents=True, exist_ok=True)
        gui, daemon = portable / "QUB-Core.exe", portable / "tools/qubd.exe"
        gui.write_bytes(b"fixture signed GUI")
        daemon.write_bytes(b"fixture signed daemon")
        process = SimpleNamespace(pid=123, wait=lambda: 0, poll=lambda: 0)
        with patch.object(smoke.subprocess, "Popen", return_value=process) as spawn, \
             patch.object(smoke.subprocess, "run", return_value=SimpleNamespace(returncode=0)), \
             contextlib.redirect_stdout(io.StringIO()):
            smoke.launch(self.lab, gui, qubd=daemon, portable=True)
        args, kwargs = spawn.call_args
        self.assertEqual(kwargs["cwd"], portable)
        self.assertEqual(args[0], [str(gui), "--config", str(self.lab / "gui/config/mainnet-smoke.toml")])
        session = smoke.read_json(next((self.lab / "reports").glob("*/session.json")))
        self.assertEqual(session["binary_provenance"], "external-portable-verifier-required")
        self.assertIsNone(session["receipt_sha256"])
        self.assertEqual(session["manual_mining_verdict"], "NOT_RECORDED")
        self.assertFalse((portable / "data/mainnet/wallet.json").exists())
        prefs = smoke.read_json(portable / smoke.PREFS)
        self.assertFalse(prefs["start_mining_on_open"])


if __name__ == "__main__":
    unittest.main()
