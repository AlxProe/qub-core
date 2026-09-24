#!/usr/bin/env python3
"""Retired HF129 gate: prevents accidental use of an obsolete deployment runbook."""
import sys

print("HF129 gates are superseded. Use HF130-v185-DEPLOY-RUNBOOK.md and "
      "scripts/test-hf130-source-contracts.py with --mode archive, local-build or checkout.",
      file=sys.stderr)
raise SystemExit(1)
