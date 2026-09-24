#!/usr/bin/env python3
"""Export only the verified HF130 inventory; never copy runtime or git files."""
import argparse
from pathlib import Path
import shutil
import sys

sys.dont_write_bytecode = True
from hf130_source_policy import verify_inventory, verify_release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--destination", type=Path, required=True)
    args = parser.parse_args()
    source = args.source.absolute()
    destination = args.destination.absolute()
    if destination.exists() or destination.is_symlink():
        raise ValueError("destination must not exist; choose a fresh staging path")
    if destination == source or source in destination.parents:
        raise ValueError("destination must be outside source")
    files = verify_inventory(source, "local-build")
    verify_release(source)
    destination.mkdir(parents=True, exist_ok=False)
    # Leave a failed partial export for inspection. It has no PASS marker and
    # the next invocation must use a fresh destination.
    for relative, path in sorted(files.items()):
        target = destination / relative
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, target)
    verify_inventory(destination, "archive")
    verify_release(destination)
    print(f"HF130 source export: PASS ({len(files)} files; {destination})")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"HF130 source export: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
