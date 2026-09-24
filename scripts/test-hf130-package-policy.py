#!/usr/bin/env python3
"""Behavioral privacy/inventory regressions using isolated temporary trees."""
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.dont_write_bytecode = True
from hf130_source_policy import MANIFEST, sha256, source_files, verify_inventory


class PackagePolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hf130-policy-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.put("src/sample.rs", b"fn main() {}\n")
        self.put("assets/README.md", b"Runtime assets are supplied locally.\n")
        self.put("explorer/public/assets/README.md", b"Private assets excluded.\n")
        entries = {name: {"sha256": sha256(path), "bytes": path.stat().st_size}
                   for name, path in source_files(self.root, "archive").items()}
        self.put(MANIFEST, json.dumps({"schema": "QUB-HF130-SOURCE-1", "release": "HF130-v185",
                                      "files": entries}).encode())

    def put(self, relative, data=b"fixture\n"):
        path = self.root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return path

    def test_clean_archive_and_checkout(self):
        for mode in ("archive", "local-build", "checkout"):
            self.assertEqual(len(verify_inventory(self.root, mode)), 4)

    def test_repeat_build_and_repository_exclude_only_workspace_roots(self):
        self.put("target/debug/qubd.exe", b"MZbuild-output")
        self.put("dist/installer.exe", b"MZsigned-build-output")
        self.put(".git/objects/metadata", b"git-local-metadata")
        for mode in ("local-build", "checkout"):
            self.assertEqual(len(verify_inventory(self.root, mode)), 4)
        with self.assertRaisesRegex(ValueError, "runtime/build directory"):
            verify_inventory(self.root, "archive")

    def test_empty_build_directory_is_rejected_in_archive(self):
        (self.root / "target").mkdir()
        with self.assertRaisesRegex(ValueError, "runtime/build directory"):
            verify_inventory(self.root, "archive")

    def test_repository_governance_is_preserved_but_not_exported(self):
        self.put("LICENSE", b"Existing repository license\n")
        self.put(".github/workflows/release.yml", b"name: Existing workflow\n")
        for mode in ("local-build", "checkout"):
            files = verify_inventory(self.root, mode)
            self.assertEqual(len(files), 4)
            self.assertNotIn("LICENSE", files)
            self.assertNotIn(".github/workflows/release.yml", files)
        with self.assertRaisesRegex(ValueError, "inventory mismatch"):
            verify_inventory(self.root, "archive")
        self.put(".github/private.key", b"fixture")
        with self.assertRaisesRegex(ValueError, "private/binary file"):
            verify_inventory(self.root, "checkout")

    def test_nested_build_directory_is_never_excluded(self):
        self.put("src/target/hidden.txt")
        for mode in ("archive", "local-build", "checkout"):
            with self.assertRaisesRegex(ValueError, "runtime/build directory"):
                verify_inventory(self.root, mode)

    def test_real_legacy_asset_names_and_future_passive_format(self):
        for name in (".gitkeep", "jin-token.png", "mined.mp3", "mining-on-muted-white.mp4",
                     "flags/gr.png", "font.ttf", "future.passive"):
            self.put("assets/" + name, b"passive fixture")
        self.assertEqual(len(verify_inventory(self.root, "local-build")), 4)
        for mode in ("archive", "checkout"):
            with self.assertRaisesRegex(ValueError, "runtime asset"):
                verify_inventory(self.root, mode)

    def test_local_mode_does_not_allow_explorer_assets(self):
        self.put("explorer/public/assets/logo.png")
        with self.assertRaisesRegex(ValueError, "runtime asset"):
            verify_inventory(self.root, "local-build")

    def test_local_mode_rejects_key_state_script_archive(self):
        for relative in ("assets/wallet.json", "assets/node_id", "assets/peers.json", "assets/private.key",
                         "assets/api-secret.txt", "assets/start.ps1", "assets/save.zip", "wallet.json"):
            with self.subTest(relative=relative):
                path = self.put(relative)
                with self.assertRaises(ValueError):
                    verify_inventory(self.root, "local-build")
                path.unlink()

    def test_local_mode_rejects_disguised_executable_and_private_key(self):
        for data in (b"MZbinary", b"\x7fELFbinary", b"PK\x03\x04archive", b"#!/bin/sh\n",
                     b"-----BEGIN OPENSSH " + b"PRIVATE KEY-----\nfixture"):
            with self.subTest(prefix=data[:8]):
                path = self.put("assets/unrecognized.passive", data)
                with self.assertRaises(ValueError):
                    verify_inventory(self.root, "local-build")
                path.unlink()

    def test_missing_extra_and_modified_sources_fail(self):
        source = self.root / "src/sample.rs"
        original = source.read_bytes()
        for data in (b"fn main() { }\n", b"fn Main() {}\n"):
            source.write_bytes(data)
            with self.assertRaisesRegex(ValueError, "source (size|hash) changed"):
                verify_inventory(self.root, "checkout")
        source.unlink()
        with self.assertRaisesRegex(ValueError, "inventory mismatch"):
            verify_inventory(self.root, "checkout")
        source.write_bytes(original)
        extra = self.put("src/unreviewed.rs")
        with self.assertRaisesRegex(ValueError, "inventory mismatch"):
            verify_inventory(self.root, "checkout")
        extra.unlink()

    def test_manifest_paths_cannot_escape_destination(self):
        manifest = json.loads((self.root / MANIFEST).read_text())
        for name in ("../outside", "/absolute", "C:/drive", "a/../b", "a\\b", "a//b"):
            with self.subTest(name=name):
                manifest["files"] = {name: {"sha256": "0" * 64, "bytes": 0}}
                (self.root / MANIFEST).write_text(json.dumps(manifest))
                with self.assertRaises(ValueError):
                    verify_inventory(self.root, "archive")

    def test_symlink_asset_is_rejected(self):
        link = self.root / "assets/linked.png"
        try:
            link.symlink_to(self.root / "src/sample.rs")
        except (OSError, NotImplementedError):
            self.skipTest("symlinks unavailable for this OS/account")
        with self.assertRaisesRegex(ValueError, "symlink"):
            verify_inventory(self.root, "local-build")

    def test_case_collision_is_rejected_before_windows_extraction(self):
        path = self.root / "src/SAMPLE.rs"
        if path.exists():
            self.skipTest("case-insensitive filesystem cannot create fixture")
        path.write_bytes(b"fixture")
        with self.assertRaisesRegex(ValueError, "case collision"):
            verify_inventory(self.root, "archive")


if __name__ == "__main__":
    unittest.main(verbosity=2)
