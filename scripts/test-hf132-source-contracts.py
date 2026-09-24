#!/usr/bin/env python3
"""Check the exact HF132 source inventory and its explicitly selected context."""
import argparse
from pathlib import Path
import sys

sys.dont_write_bytecode = True
from hf132_source_policy import MODES, verify_inventory, verify_release


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=MODES, required=True)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    args = parser.parse_args()
    files = verify_inventory(args.root.absolute(), args.mode)
    verify_release(args.root.absolute())
    print(f"HF132 source contracts: PASS ({args.mode}; {len(files)} manifest files)")


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError) as error:
        print(f"HF132 source contracts: FAIL: {error}", file=sys.stderr)
        raise SystemExit(1)
