#!/usr/bin/env python3
"""Source preparation regressions; no Cargo, GUI or real user data."""
import importlib.util
from pathlib import Path
import sys
import shutil
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location("hf132_prepare", Path(__file__).with_name("hf132-prepare.py"))
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)


class PrepareTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.source = self.root / "extracted"
        self.source.mkdir()
        self.readme = self.source / "assets" / "README.md"
        self.readme.parent.mkdir()
        self.readme.write_bytes(b"reviewed README\n")
        self.assets = self.root / "old-work" / "assets"
        self.assets.mkdir(parents=True)
        (self.assets / "README.md").write_bytes(b"old README\n")
        (self.assets / "qubit-coin-logo.png").write_bytes(b"passive png fixture")
        self.destination = self.root / "sources" / "HF132" / "qubd-v1.8.5"

    def test_copy_preserves_original_and_reviewed_readme(self):
        (self.assets / "mining-on-muted-white.mp4").write_bytes(b"passive movie fixture")
        with patch.object(helper, "verify_inventory", return_value={"assets/README.md": self.readme}), patch.object(helper, "verify_release"):
            helper.prepare(self.source, self.destination, self.assets)
        self.assertTrue(self.source.is_dir())
        self.assertEqual((self.assets / "README.md").read_bytes(), b"old README\n")
        self.assertEqual((self.destination / "assets/README.md").read_bytes(), b"reviewed README\n")
        self.assertEqual((self.destination / "assets/mining-on-muted-white.mp4").read_bytes(), b"passive movie fixture")

    def test_bad_asset_rejected_before_destination_creation(self):
        (self.assets / "private.key").write_bytes(b"fixture only")
        with patch.object(helper, "verify_inventory"), patch.object(helper, "verify_release"):
            with self.assertRaises(ValueError):
                helper.prepare(self.source, self.destination, self.assets)
        self.assertFalse(self.destination.exists())

    def test_failed_existing_source_never_replaced(self):
        self.destination.mkdir(parents=True)
        marker = self.destination / "keep-me"
        marker.write_bytes(b"retain partial evidence")
        def verify(root, mode):
            if root == self.destination:
                raise ValueError("partial inventory")
            return {}
        with patch.object(helper, "verify_inventory", side_effect=verify), patch.object(helper, "verify_release"):
            with self.assertRaisesRegex(ValueError, "partial inventory"):
                helper.prepare(self.source, self.destination, self.assets)
        self.assertEqual(marker.read_bytes(), b"retain partial evidence")

    def test_missing_logo_is_actionable(self):
        (self.assets / "qubit-coin-logo.png").unlink()
        with self.assertRaisesRegex(ValueError, "logo.png"):
            helper.assets_to_copy(self.assets)

    def test_existing_exact_source_reused_without_overwrite(self):
        self.destination.mkdir(parents=True)
        (self.destination / "assets").mkdir()
        (self.destination / "assets/README.md").write_bytes(self.readme.read_bytes())
        shutil.copyfile(self.assets / "qubit-coin-logo.png", self.destination / "assets/qubit-coin-logo.png")
        marker = self.destination / "unchanged"
        marker.write_bytes(b"same")
        with patch.object(helper, "verify_inventory"), patch.object(helper, "verify_release"), patch.object(helper.shutil, "copyfile") as copy:
            helper.prepare(self.source, self.destination, self.assets)
            copy.assert_not_called()
        self.assertEqual(marker.read_bytes(), b"same")

    def test_interrupted_asset_copy_resumes_without_recopying_source(self):
        (self.assets / "mining-on-muted-white.mp4").write_bytes(b"passive movie fixture")
        original_copy = shutil.copyfile

        def interrupt_copy(source, target):
            if Path(source).name == "qubit-coin-logo.png":
                Path(target).write_bytes(b"partial")
                raise OSError("injected interrupted asset copy")
            return original_copy(source, target)

        inventory = {"assets/README.md": self.readme}
        with patch.object(helper, "verify_inventory", return_value=inventory), patch.object(helper, "verify_release"):
            with patch.object(helper.shutil, "copyfile", side_effect=interrupt_copy):
                with self.assertRaisesRegex(OSError, "interrupted asset"):
                    helper.prepare(self.source, self.destination, self.assets)
            self.assertEqual((self.destination / "assets/README.md").read_bytes(), b"reviewed README\n")
            self.assertFalse((self.destination / "assets/qubit-coin-logo.png").exists())
            self.assertFalse(list(self.destination.parent.glob(".hf132-asset-*")))
            with patch.object(helper.shutil, "copyfile", wraps=original_copy) as copying:
                helper.prepare(self.source, self.destination, self.assets)
                self.assertTrue(copying.called)
                for call in copying.call_args_list:
                    self.assertIn(self.assets, Path(call.args[0]).parents)
            self.assertEqual((self.destination / "assets/qubit-coin-logo.png").read_bytes(), b"passive png fixture")
            self.assertEqual((self.destination / "assets/mining-on-muted-white.mp4").read_bytes(), b"passive movie fixture")
            self.assertEqual((self.destination / "assets/README.md").read_bytes(), b"reviewed README\n")

    def test_resume_rejects_modified_asset_before_filling_missing_asset(self):
        (self.assets / "mining-on-muted-white.mp4").write_bytes(b"passive movie fixture")
        with patch.object(helper, "verify_inventory", return_value={"assets/README.md": self.readme}), patch.object(helper, "verify_release"):
            helper.prepare(self.source, self.destination, self.assets)
            (self.destination / "assets/mining-on-muted-white.mp4").unlink()
            logo = self.destination / "assets/qubit-coin-logo.png"
            logo.write_bytes(b"modified existing logo")
            with patch.object(helper.shutil, "copyfile") as copying:
                with self.assertRaisesRegex(ValueError, "not overwritten"):
                    helper.verify_assets(self.destination, self.assets)
                copying.assert_not_called()
            self.assertEqual(logo.read_bytes(), b"modified existing logo")
            self.assertFalse((self.destination / "assets/mining-on-muted-white.mp4").exists())

    def test_verify_assets_fills_missing_media_then_rejects_unexpected_file(self):
        (self.assets / "mining-on-muted-white.mp4").write_bytes(b"passive movie fixture")
        with patch.object(helper, "verify_inventory", return_value={"assets/README.md": self.readme}), patch.object(helper, "verify_release"):
            helper.prepare(self.source, self.destination, self.assets)
            movie = self.destination / "assets/mining-on-muted-white.mp4"
            movie.unlink()
            helper.verify_assets(self.destination, self.assets)
            self.assertEqual(movie.read_bytes(), b"passive movie fixture")
            unexpected = self.destination / "assets/unexpected.png"
            unexpected.write_bytes(b"unlisted passive asset")
            with self.assertRaisesRegex(ValueError, "Unexpected local runtime"):
                helper.verify_assets(self.destination, self.assets)
            self.assertEqual(unexpected.read_bytes(), b"unlisted passive asset")


if __name__ == "__main__":
    unittest.main()
