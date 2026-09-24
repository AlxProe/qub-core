#!/usr/bin/env python3
"""Behavioral privacy/inventory regressions using isolated temporary trees."""
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from types import SimpleNamespace
import stat
import shutil

sys.dont_write_bytecode = True
from hf132_source_policy import (
    FROZEN_SHA256, FROZEN_SOURCE_SLICES, MANIFEST, MANIFEST_SCHEMA, PREPARATION_REVISION,
    PRODUCT_VERSION, RELEASE, regular_metadata, require_outside_source,
    safe_relative, sha256, source_files, source_slice, verify_frozen_sources,
    verify_inventory, verify_runbook_text,
)


class PackagePolicyTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hf132-policy-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.put("src/sample.rs", b"fn main() {}\n")
        self.put("assets/README.md", b"Runtime assets are supplied locally.\n")
        self.put("explorer/public/assets/README.md", b"Private assets excluded.\n")
        entries = {name: {"sha256": sha256(path), "bytes": path.stat().st_size}
                   for name, path in source_files(self.root, "archive").items()}
        self.put(MANIFEST, json.dumps({"schema": MANIFEST_SCHEMA, "release": RELEASE,
                                      "product_version": PRODUCT_VERSION,
                                      "preparation_revision": PREPARATION_REVISION,
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

    def test_obsolete_root_manifest_cannot_enter_hf132_export(self):
        self.put("HF130-SOURCE-MANIFEST.json", b"{}\n")
        for mode in ("archive", "local-build", "checkout"):
            with self.assertRaisesRegex(ValueError, "obsolete source manifest"):
                verify_inventory(self.root, mode)

    def test_manifest_product_revision_and_entry_types_are_checked(self):
        manifest_path = self.root / MANIFEST
        original = json.loads(manifest_path.read_text())
        for key, value in (("product_version", "1.8.4"), ("preparation_revision", "r7"),
                           ("release", "HF130-v185"), ("schema", "QUB-HF130-SOURCE-1")):
            with self.subTest(key=key):
                modified = dict(original, **{key: value})
                manifest_path.write_text(json.dumps(modified))
                with self.assertRaisesRegex(ValueError, "manifest identity"):
                    verify_inventory(self.root, "archive")
        for details in ({"bytes": True, "sha256": "0" * 64},
                        {"bytes": 1, "sha256": "not-a-hash"},
                        {"bytes": -1, "sha256": "0" * 64}):
            modified = json.loads(json.dumps(original))
            modified["files"]["src/sample.rs"] = details
            manifest_path.write_text(json.dumps(modified))
            with self.assertRaisesRegex(ValueError, "invalid manifest size/hash"):
                verify_inventory(self.root, "archive")

    def test_windows_reserved_and_ambiguous_paths_are_rejected(self):
        for name in ("src/CON.rs", "assets/AUX", "src/com1.txt", "assets/LPT9.png",
                     "assets/COM\u00b9.png", "src/trailing.", "src/trailing ",
                     "assets/bad?.png", "src/control\x01.rs"):
            with self.subTest(name=name), self.assertRaises(ValueError):
                safe_relative(name)

    def test_junction_attribute_is_rejected_without_symlink_mode(self):
        directory = SimpleNamespace(st_mode=stat.S_IFDIR | 0o755, st_file_attributes=0x400)
        with patch.object(Path, "lstat", return_value=directory):
            with self.assertRaisesRegex(ValueError, "reparse point"):
                regular_metadata(self.root)
            with self.assertRaisesRegex(ValueError, "reparse point"):
                source_files(self.root, "archive")

    def test_resolved_destination_must_remain_outside_source(self):
        require_outside_source(self.root, self.root.parent / (self.root.name + "-output"))
        with self.assertRaisesRegex(ValueError, "outside source"):
            require_outside_source(self.root, self.root / "new-output")
        with tempfile.TemporaryDirectory(prefix="hf132-output-alias-") as other:
            alias = Path(other) / "source-alias"
            try:
                alias.symlink_to(self.root, target_is_directory=True)
            except (OSError, NotImplementedError):
                self.skipTest("symlinks unavailable for this OS/account")
            with self.assertRaisesRegex(ValueError, "outside source"):
                require_outside_source(self.root, alias / "nested-output")


class FrozenCompatibilityTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hf132-frozen-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        project = Path(__file__).resolve().parents[1]
        names = set(FROZEN_SHA256) | {entry[1] for entry in FROZEN_SOURCE_SLICES}
        for name in names:
            destination = self.root / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(project / name, destination)

    def test_current_frozen_compatibility_bytes_match_deployed_hf128(self):
        verify_frozen_sources(self.root)

    def test_each_pinned_slice_rejects_changed_bytes(self):
        for label, relative, start, end, _expected in FROZEN_SOURCE_SLICES:
            with self.subTest(label=label):
                path = self.root / relative
                original = path.read_bytes()
                position = original.index(start) + len(start)
                path.write_bytes(original[:position] + b" " + original[position:])
                with self.assertRaisesRegex(ValueError, "compatibility bytes changed"):
                    verify_frozen_sources(self.root)
                path.write_bytes(original)

    def test_unrelated_gpu_ownership_text_can_change_without_unfreezing_kernel(self):
        path = self.root / "src/gpu_miner.rs"
        path.write_bytes(b"// Ownership implementation outside frozen mining blocks.\n" + path.read_bytes())
        verify_frozen_sources(self.root)

    def test_pool_and_config_files_remain_frozen_in_full(self):
        for relative in ("src/pools.rs", "config/mainnet.toml"):
            with self.subTest(relative=relative):
                path = self.root / relative
                original = path.read_bytes()
                path.write_bytes(original + b"\n")
                with self.assertRaisesRegex(ValueError, "compatibility bytes changed"):
                    verify_frozen_sources(self.root)
                path.write_bytes(original)

    def test_slice_extraction_rejects_duplicate_or_missing_boundaries(self):
        self.assertEqual(source_slice(b"before START body END after", b"START", b"END", "fixture"),
                         b"START body END")
        for data in (b"START body END START", b"body END", b"START body"):
            with self.subTest(data=data), self.assertRaises(ValueError):
                source_slice(data, b"START", b"END", "fixture")


class RunbookPolicyTests(unittest.TestCase):
    GOOD = ("HF132-v185-source-no-assets.zip\nHF132-v185-SHA256SUMS.txt\n"
            "```powershell\n& {\n    $ErrorActionPreference = 'Stop'\n}\n```\n")

    def test_exact_names_and_fail_stop_block_pass(self):
        verify_runbook_text(self.GOOD)
        verify_runbook_text(self.GOOD.replace("```powershell", "```PowerShell"))

    def test_all_powershell_blocks_must_be_fail_stop(self):
        bad_blocks = ("Write-Host 'PASS'\n", "& {\nWrite-Host 'PASS'\n}\n",
                      "& {\n$ErrorActionPreference = 'Stop'\n}\nWrite-Host 'PASS'\n")
        for fence in bad_blocks:
            with self.subTest(fence=fence), self.assertRaises(ValueError):
                verify_runbook_text(self.GOOD + "```powershell\n" + fence + "```\n")

    def test_non_ascii_or_wrong_release_filenames_fail(self):
        for text in (self.GOOD + "\u2014", self.GOOD.replace("HF132-v185-source-no-assets.zip", "old.zip"),
                     self.GOOD.replace("HF132-v185-SHA256SUMS.txt", "old-sums.txt")):
            with self.subTest(text=text), self.assertRaises(ValueError):
                verify_runbook_text(text)


if __name__ == "__main__":
    unittest.main(verbosity=2)
