#!/usr/bin/env python3
"""Install verified source beside existing work; never rename an existing tree."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import stat
import sys
import tempfile

sys.dont_write_bytecode = True
from hf132_source_policy import check_file, sha256, verify_inventory, verify_release


def regular(path: Path, directory: bool) -> None:
    info = path.lstat()
    if path.is_symlink() or getattr(info, "st_file_attributes", 0) & 0x400:
        raise ValueError(f"Link/reparse point is not allowed: {path}")
    if not (stat.S_ISDIR(info.st_mode) if directory else stat.S_ISREG(info.st_mode)):
        raise ValueError(f"Not a regular {'directory' if directory else 'file'}: {path}")


def assets_to_copy(assets: Path, *, require_logo: bool = True) -> list[tuple[Path, str]]:
    regular(assets, True)
    files = []
    folded = set()
    for directory, subdirs, names in os.walk(assets, followlinks=False):
        for name in subdirs:
            regular(Path(directory) / name, True)
        for name in names:
            path = Path(directory) / name
            relative = path.relative_to(assets).as_posix()
            regular(path, False)
            if relative == "README.md":
                continue
            if relative.casefold() == "readme.md":
                raise ValueError("Runtime asset collides with the reviewed assets/README.md")
            if relative.casefold() in folded:
                raise ValueError(f"Case collision in assets: {relative}")
            folded.add(relative.casefold())
            check_file(path, "assets/" + relative, True)
            files.append((path, relative))
    if require_logo and not any(relative == "qubit-coin-logo.png" for _, relative in files):
        raise ValueError(f"Required qubit-coin-logo.png is missing from {assets}")
    return files


def regular_ancestors(path: Path) -> None:
    for parent in [path.parent, *path.parent.parents]:
        regular(parent, True)


def fill_missing_assets(destination: Path, asset_files: list[tuple[Path, str]]) -> None:
    """Verify all existing bytes first; only install missing passive files."""
    expected = {relative: (path, sha256(path)) for path, relative in asset_files}
    asset_root = destination / "assets"
    regular(asset_root, True)
    actual = {relative: path for path, relative in assets_to_copy(asset_root, require_logo=False)}
    unexpected = set(actual) - set(expected)
    if unexpected:
        raise ValueError("Unexpected local runtime assets; none overwritten: " + ", ".join(sorted(unexpected)))
    for relative, path in actual.items():
        if sha256(path) != expected[relative][1]:
            raise ValueError(f"Existing runtime asset differs from checked original; not overwritten: {relative}")
    for relative, (source, digest) in expected.items():
        if relative in actual:
            continue
        target = asset_root / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        regular_ancestors(target)
        # Staging is outside the manifest tree. Interrupted copy leaves no
        # partial final asset that a later resume could mistake for user edits.
        descriptor, temporary_name = tempfile.mkstemp(prefix=".hf132-asset-", dir=destination.parent)
        os.close(descriptor)
        temporary = Path(temporary_name)
        try:
            shutil.copyfile(source, temporary)
            if sha256(source) != digest or sha256(temporary) != digest:
                raise ValueError(f"Original asset changed while copying: {relative}")
            if os.name == "nt":
                # Windows rename refuses an existing destination. Unlike
                # replace(), this never overwrites a concurrently added file.
                os.rename(temporary, target)
            else:
                os.link(temporary, target)
        finally:
            temporary.unlink(missing_ok=True)
    # Include already-existing files in the final recheck, not only the newly
    # copied subset; logo and media must all match the same checked originals.
    for relative, (source, digest) in expected.items():
        target = asset_root / relative
        regular(target, False)
        if sha256(source) != digest or sha256(target) != digest:
            raise ValueError(f"Runtime asset identity changed during preparation: {relative}")


def verify_assets(destination: Path, assets: Path) -> None:
    destination, assets = destination.absolute(), assets.absolute()
    regular_ancestors(destination)
    regular_ancestors(assets)
    regular(destination, True)
    asset_files = assets_to_copy(assets)
    verify_inventory(destination, "local-build")
    verify_release(destination)
    fill_missing_assets(destination, asset_files)
    verify_inventory(destination, "local-build")
    verify_release(destination)
    print(f"HF132 RUNTIME ASSETS VERIFIED: {destination}")


def prepare(source: Path, destination: Path, assets: Path) -> None:
    source = source.absolute()
    destination = destination.absolute()
    regular(source, True)
    regular_ancestors(source)
    regular_ancestors(assets.absolute())
    verify_inventory(source, "archive")
    verify_release(source)
    # Check every input asset before writing anything. Keep the archive README.
    asset_files = assets_to_copy(assets.absolute())
    if destination.exists():
        regular_ancestors(destination)
        regular(destination, True)
        verify_inventory(destination, "local-build")
        verify_release(destination)
        fill_missing_assets(destination, asset_files)
        verify_inventory(destination, "local-build")
        verify_release(destination)
        print(f"Existing exact HF132 source reused: {destination}")
        return
    if source == destination or source in destination.parents:
        raise ValueError("Destination must be outside the extracted archive")
    destination.parent.mkdir(parents=True, exist_ok=True)
    for parent in [destination.parent, *destination.parent.parents]:
        regular(parent, True)
    destination.mkdir()
    # Incomplete source fails verification on retry; complete source with
    # missing assets can resume. No existing source/asset/cache is overwritten.
    for relative, path in verify_inventory(source, "archive").items():
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    fill_missing_assets(destination, asset_files)
    verify_inventory(destination, "local-build")
    verify_release(destination)
    print(f"HF132 SOURCE READY: {destination}")
    print(f"Assets copied from: {assets}")
    print("Existing source and target cache retained in place.")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path)
    parser.add_argument("--verify-assets", action="store_true")
    parser.add_argument("--destination", type=Path, required=True)
    parser.add_argument("--assets", type=Path, required=True)
    args = parser.parse_args()
    if args.verify_assets:
        verify_assets(args.destination, args.assets)
    else:
        if args.source is None:
            parser.error("--source is required unless --verify-assets is selected")
        prepare(args.source, args.destination, args.assets)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError) as exc:
        print(f"HF132 SOURCE PREPARATION FAILED: {exc}", file=sys.stderr)
        raise SystemExit(1)
