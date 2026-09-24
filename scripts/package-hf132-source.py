#!/usr/bin/env python3
"""Create a reproducible HF132 no-assets source ZIP from a clean release tree.

This intentionally regenerates the source inventory; it is a release-preparation
tool, not a way to approve modified bytes during a deployment gate. The deployed
candidate must match its previously verified external ZIP SHA256.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib
import zipfile

sys.dont_write_bytecode = True
from hf132_source_policy import (
    MANIFEST, MANIFEST_SCHEMA, PREPARATION_REVISION, PRODUCT_VERSION, RELEASE,
    require_outside_source, sha256, source_files, verify_inventory, verify_release,
)


def verify_rust_syntax(source, files, rustfmt):
    """Parse every Rust file before publishing an inventory; never format in place.

    rustfmt uses Rust's parser but does not expand macros, type-check or run
    tests. Those remain mandatory in the native build gate.
    """
    edition = str(tomllib.loads((source / "Cargo.toml").read_text(encoding="utf-8"))["package"]["edition"])
    rust_files = [(relative, path) for relative, path in sorted(files.items())
                  if path.suffix == ".rs"]
    if not rust_files:
        raise ValueError("no Rust files found for the source syntax gate")
    for relative, path in rust_files:
        before = sha256(path)
        parsed = subprocess.run(
            [rustfmt, "--edition", edition, "--emit", "stdout", "--config", "skip_children=true", str(path)],
            stdout=subprocess.DEVNULL, stderr=subprocess.PIPE,
            text=True, encoding="utf-8", errors="replace", timeout=60,
        )
        if sha256(path) != before:
            raise ValueError(f"Rust parser changed source bytes: {relative}")
        if parsed.returncode:
            raise ValueError(f"Rust syntax check failed in {relative}:\n{parsed.stderr.strip()}")
    print(f"HF132 Rust syntax: PASS ({len(rust_files)} files; parsing only, no native test claim)")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--rustfmt", default=os.environ.get("HF132_RUSTFMT", "rustfmt"),
                        help="Rust parser executable used read-only before creating the source manifest")
    args = parser.parse_args()
    source = args.source.absolute()
    output_dir = args.output_dir.absolute()
    require_outside_source(source, output_dir)
    output_dir.mkdir(parents=True, exist_ok=True)
    output = output_dir / "HF132-v185-source-no-assets.zip"
    if output.exists() or output.is_symlink():
        raise ValueError("output exists; use a new output directory to preserve the previous candidate")
    files = source_files(source, "archive")
    verify_release(source)
    verify_rust_syntax(source, files, args.rustfmt)
    entries = {relative: {"bytes": path.stat().st_size, "sha256": sha256(path)}
               for relative, path in sorted(files.items()) if relative != MANIFEST}
    manifest = {
        "schema": MANIFEST_SCHEMA,
        "release": RELEASE,
        "product_version": PRODUCT_VERSION,
        "preparation_revision": PREPARATION_REVISION,
        "baseline_zip": {
            "HF131-v185-source-no-assets-r4.zip": "d26ad61009be5e51d99b606f3e761b400abcfe0243658baa316d5f6d1a9d379e",
            "HF128-v184-source-no-assets-r3.zip": "85fa369110e7923e2790cd270f0385b4ceb47d6fde28dc17492a5b9ef8128ba3",
        },
        "files": entries,
    }
    (source / MANIFEST).write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    files = verify_inventory(source, "archive")
    with tempfile.TemporaryDirectory(prefix="hf132-package-", dir=output_dir) as temp:
        staged = Path(temp) / output.name
        with zipfile.ZipFile(staged, "w", compression=zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
            for relative, path in sorted(files.items()):
                info = zipfile.ZipInfo("qubd-v1.8.5/" + relative, date_time=(2026, 9, 14, 0, 0, 0))
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
    print(f"HF132 source package: PASS ({len(files)} source files)")
    print(f"{sha256(output)}  {output.name}")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, zipfile.BadZipFile, subprocess.TimeoutExpired) as error:
        print(f"HF132 source package: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
