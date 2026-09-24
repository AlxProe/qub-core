#!/usr/bin/env python3
"""Fixture tests for HF132 evidence reuse; these do not compile or run Rust."""
from __future__ import annotations

import copy
import contextlib
import importlib.util
import io
from pathlib import Path
import tempfile
import unittest
from unittest import mock

SCRIPT = Path(__file__).with_name("hf132-native-build.py")
SPEC = importlib.util.spec_from_file_location("hf132_native_build", SCRIPT)
assert SPEC and SPEC.loader
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


class NativeEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source = self.root / "source"
        self.source.mkdir()
        (self.source / "src").mkdir()
        for name in ("build.rs", "Cargo.toml", "src/lib.rs"):
            (self.source / name).write_text("fixture only", encoding="utf-8")
        self.env = native.build_environment(self.source, {})
        self.identity = {"source": str(self.source), "manifest_sha256": "a" * 64,
                         "cargo_lock_sha256": "b" * 64, "target": native.TARGET,
                         "target_root": self.env["CARGO_TARGET_DIR"], "environment": {"RUSTFLAGS": self.env["RUSTFLAGS"]},
                         "rustc": "test-rustc", "cargo": "test-cargo", "build_logo_sha256": "c" * 64}
        self.binaries = {}
        release = Path(self.env["CARGO_TARGET_DIR"]) / native.TARGET / "release"
        release.mkdir(parents=True)
        for name in native.BINS:
            data = bytearray(256)
            data[:2] = b"MZ"
            data[0x3c:0x40] = (0x80).to_bytes(4, "little")
            data[0x80:0x86] = b"PE\0\0\x64\x86"
            (release / name).write_bytes(data)
        self.binaries = native.executable_inventory(self.env)
        phases = []
        for name in native.PHASES:
            path = self.root / (name + ".log")
            path.write_text("Native fixture evidence; not real execution\n", encoding="utf-8")
            phases.append({"name": name, "exit_code": 0, "log": str(path), "log_sha256": native.sha256(path)})
        self.receipt = {"schema": native.SCHEMA, "ok": True, "identity": self.identity,
                        "binaries": self.binaries, "phases": phases}

    def check(self, receipt=None, identity=None, binaries=None):
        native.check_receipt(self.receipt if receipt is None else receipt,
                             self.identity if identity is None else identity,
                             self.binaries if binaries is None else binaries)

    def test_identical_completed_evidence_is_accepted(self):
        self.check()

    def test_changed_source_lock_toolchain_flags_target_and_logo_are_rejected(self):
        for key in self.identity:
            with self.subTest(key=key):
                changed = copy.deepcopy(self.identity)
                changed[key] = "changed"
                with self.assertRaisesRegex(RuntimeError, "inputs changed"):
                    self.check(identity=changed)

    def test_modified_executable_is_rejected(self):
        path = Path(self.binaries["qubd.exe"]["path"])
        with path.open("ab") as output:
            output.write(b"modified")
        with self.assertRaisesRegex(RuntimeError, "executable identity changed"):
            self.check(binaries=native.executable_inventory(self.env))

    def test_missing_phase_and_zero_phase_receipts_are_rejected(self):
        for phases in ([], self.receipt["phases"][:-1], None, [None]):
            with self.subTest(phases=phases):
                receipt = dict(self.receipt, phases=phases)
                with self.assertRaisesRegex(RuntimeError, "missing required"):
                    self.check(receipt=receipt)

    def test_failed_native_command_cannot_be_reused(self):
        receipt = copy.deepcopy(self.receipt)
        receipt["phases"][1]["exit_code"] = 1
        with self.assertRaisesRegex(RuntimeError, "failed phase"):
            self.check(receipt=receipt)

    def test_changed_or_missing_log_is_rejected(self):
        path = Path(self.receipt["phases"][1]["log"])
        path.write_text("replacement", encoding="utf-8")
        with self.assertRaisesRegex(RuntimeError, "log changed"):
            self.check()
        path.unlink()
        with self.assertRaises(OSError):
            self.check()

    def test_false_pass_or_wrong_schema_is_rejected(self):
        for receipt in (dict(self.receipt, ok=False), dict(self.receipt, schema="old")):
            with self.assertRaisesRegex(RuntimeError, "Missing or failed"):
                self.check(receipt=receipt)

    def test_receipt_inside_source_is_rejected(self):
        with self.assertRaisesRegex(RuntimeError, "outside"):
            native.ensure_external_receipt(self.source, self.source / "receipt.json")
        native.ensure_external_receipt(self.source, self.root / "evidence" / "receipt.json")

    def test_build_defaults_and_shared_target_are_consistent(self):
        self.assertEqual(self.env["QUB_BUILD_CONFIG"], "mainnet")
        self.assertEqual(self.env["CARGO_BUILD_TARGET"], native.TARGET)
        self.assertEqual(self.env["RUSTFLAGS"], "-C target-feature=+crt-static")
        env = native.build_environment(self.source, {"CARGO_TARGET_DIR": str(self.root / "old-target")})
        self.assertEqual(Path(env["CARGO_TARGET_DIR"]), self.root / "old-target")
        relative = native.build_environment(self.source, {"CARGO_TARGET_DIR": "cache"})
        self.assertEqual(Path(relative["CARGO_TARGET_DIR"]), self.source / "cache")

    def test_wrong_config_hidden_flags_and_disabled_crt_are_rejected(self):
        for env in ({"QUB_BUILD_CONFIG": "regtest"}, {"CARGO_ENCODED_RUSTFLAGS": "hidden"},
                    {"RUSTFLAGS": "-C target-feature=-crt-static"}):
            with self.subTest(env=env), self.assertRaises(RuntimeError):
                native.build_environment(self.source, env)

    def test_effective_static_flags_are_not_duplicated(self):
        flags = "-C opt-level=3 -C target-feature=+crt-static"
        env = native.build_environment(self.source, {"RUSTFLAGS": flags})
        self.assertEqual(env["RUSTFLAGS"], flags)

    def test_invalid_pe_and_wrong_architecture_are_rejected(self):
        path = Path(self.binaries["qubd.exe"]["path"])
        valid = path.read_bytes()
        for content in (b"not a PE", valid[:63], valid[:0x84] + b"\x4c\x01" + valid[0x86:]):
            with self.subTest(size=len(content)):
                path.write_bytes(content)
                with self.assertRaises(RuntimeError):
                    native.executable_inventory(self.env)

    def test_symlink_binary_is_rejected(self):
        path = Path(self.binaries["qubd.exe"]["path"])
        other = path.with_name("other.exe")
        path.rename(other)
        try:
            path.symlink_to(other)
        except OSError:
            self.skipTest("Symlink creation unavailable on this fixture host")
        with self.assertRaisesRegex(RuntimeError, "regular file"):
            native.executable_inventory(self.env)

    def test_failed_build_does_not_publish_receipt(self):
        receipt = self.root / "failed-native.json"
        def fake_run(command, cwd, env, log, timeout=None):
            if log.stem == "all-native-tests":
                raise RuntimeError("injected native command failure")
            log.write_text("fixture success", encoding="utf-8")
        with mock.patch.object(native.sys, "platform", "win32"), \
             mock.patch.object(native, "identity", return_value=self.identity), \
             mock.patch.object(native, "run", side_effect=fake_run):
            with self.assertRaisesRegex(RuntimeError, "injected"):
                native.build(self.source, receipt, self.env)
        self.assertFalse(receipt.exists())

    def test_full_build_phases_and_verify_reuse_do_not_repeat_native_work(self):
        receipt = self.root / "native.json"
        commands = []
        def fake_run(command, cwd, env, log, timeout=None):
            commands.append(command)
            log.write_text("fixture success; no Rust execution", encoding="utf-8")
        with mock.patch.object(native.sys, "platform", "win32"), \
             mock.patch.object(native, "identity", return_value=self.identity), \
             mock.patch.object(native, "run", side_effect=fake_run), contextlib.redirect_stdout(io.StringIO()):
            native.build(self.source, receipt, self.env)
            self.assertEqual(len(commands), len(native.PHASES))
            self.assertEqual(sum(command[:2] == ["cargo", "test"] for command in commands), 1)
            self.assertEqual(sum(command[:2] == ["cargo", "build"] for command in commands), 1)
            self.assertEqual(commands[-3].count("--bin"), 3)
            commands.clear()
            native.verify(self.source, receipt, self.env)
            self.assertEqual(len(commands), 1)
            self.assertIn("test-hf132-source-contracts.py", commands[0][1])


if __name__ == "__main__":
    unittest.main(verbosity=2)
