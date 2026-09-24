#!/usr/bin/env python3
"""Create a reproducible HF130 no-assets source ZIP from a clean release tree.

This intentionally regenerates the source inventory; it is a release-preparation
tool, not a way to approve modified bytes during a deployment gate. The deployed
candidate must match its previously verified external ZIP SHA256.
"""
import argparse
import json
from pathlib import Path
import sys
import tempfile
import zipfile

sys.dont_write_bytecode = True
from hf130_source_policy import MANIFEST, sha256, source_files, verify_inventory, verify_release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    output_dir = args.output_dir.absolute()
    if output_dir == source or source in output_dir.parents:
        raise ValueError("output directory must be outside the source tree")
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / "HF130-v185-source-no-assets-r7.zip"
    if output.exists() or output.is_symlink():
        raise ValueError("output exists; use a new output directory to preserve the previous candidate")
    files = source_files(source, "archive")
    verify_release(source)
    entries = {relative: {"bytes": path.stat().st_size, "sha256": sha256(path)}
               for relative, path in sorted(files.items()) if relative != MANIFEST}
    manifest = {
        "schema": "QUB-HF130-SOURCE-1",
        "release": "HF130-v185",
        "product_version": "1.8.5",
        "preparation_revision": "r7",
        "baseline_zip": {
            "HF128-v184-source-no-assets-r3.zip": "85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3",
            "HF129-v185-source-no-assets-r6.zip": "85160e0f72ed7308ab05793b4981863d8fcfc6b7ebff61f29cbdd1c68afad33c",
        },
        "files": entries,
    }
    (source / MANIFEST).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    files = verify_inventory(source, "archive")
    with tempfile.TemporaryDirectory(prefix="hf130-package-", dir=output_dir) as temp:
        staged = Path(temp) / output.name
        with zipfile.ZipFile(staged, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for relative, path in sorted(files.items()):
                info = zipfile.ZipInfo("qubd-v1.8.5/" + relative, date_time=(2026, 9, 13, 0, 0, 0))
                info.create_system = 3
                info.external_attr = 0o100644 << 16
                info.compress_type = zipfile.ZIP_DEFLATED
                archive.writestr(info, path.read_bytes(), compress_type=zipfile.ZIP_DEFLATED, compresslevel=9)
        extracted = Path(temp) / "verified"
        with zipfile.ZipFile(staged) as archive:
            if archive.testzip() is not None:
                raise ValueError("ZIP CRC verification failed")
            archive.extractall(extracted)
        verify_inventory(extracted / "qubd-v1.8.5", "archive")
        verify_release(extracted / "qubd-v1.8.5")
        staged.replace(output)
    print(f"HF130 source package: PASS ({len(files)} source files)")
    print(f"{sha256(output)}  {output.name}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, zipfile.BadZipFile) as error:
        print(f"HF130 source package: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
