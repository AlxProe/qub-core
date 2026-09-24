#!/usr/bin/env python3
"""Run the registered HF130 Rust regressions and reject missing/zero-test passes.

This script requires the native Rust toolchain. It does not replace the complete
Cargo suite, Windows/GPU checks, mixed-version checks, or the seed canary.
"""

from __future__ import annotations

import argparse
from collections import Counter
from pathlib import Path
import re
import subprocess
import sys


ROOT = Path(__file__).resolve().parents[1]

# Exact test basenames are kept independently of Cargo discovery. An accidental
# rename/removal must fail the gate, rather than silently reducing its coverage.
# Updated together with the HF130 implementation before release packaging.
LIB_TESTS: tuple[str, ...] = (
    "hf130_drip_feed_frame_obeys_absolute_deadline",
    "hf130_silent_peer_read_is_cancelled",
    "hf130_nested_budget_cannot_extend_parent",
    "hf130_fresh_proof_rejects_ancestor_pending_and_changed_tip",
    "hf130_dns_cache_bounds_pending_work",
    "hf130_probe_unrelated_messages_share_one_deadline",
    "hf130_snapshot_generation_pairing_rejects_mismatch",
    "hf130_blocked_write_obeys_absolute_deadline",
    "hf130_live_publish_precedes_delayed_maintenance",
    "hf130_post_current_maintenance_error_does_not_fail_save",
    "hf130_inline_cli_maintenance_failure_retains_committed_tip",
    "hf130_cleanup_rechecks_current_previous",
    "hf130_open_journal_survives_cleanup",
    "hf130_cleanup_yields_to_canonical_writer",
)
DAEMON_TESTS: tuple[str, ...] = ()
GUI_TESTS: tuple[str, ...] = (
    "hf130_gui_sync_routes_preserve_public_repair",
    "hf130_gui_payout_warning_uses_active_prefix",
    "hf130_regtest_gui_sync_adopts_configured_peer_block",
    "hf130_pool_share_cancel_during_search_never_yields_work",
    "hf130_pool_share_parent_change_rejects_found_nonce",
    "hf130_pool_share_deadline_preserves_cursor_for_next_batch",
    "hf130_failed_snapshot_recapture_cannot_overtake_newer_reader",
    "hf130_head_merge_keeps_balance_and_tip_in_same_capture",
    "hf130_automatic_catchup_ticks_cannot_queue_another_pass",
    "hf130_miner_exit_reason_distinguishes_stop_and_failure",
    "hf130_pool_resume_pins_address_and_rejects_changed_intent",
    "hf130_controller_unwind_joins_all_round_workers",
)

LIST_LINE = re.compile(r"^([A-Za-z_][A-Za-z0-9_:]*): test$")
PASS_LINE = re.compile(r"^test ([A-Za-z_][A-Za-z0-9_:]*) \.\.\. ok$")


def test_basename(name: str) -> str:
    return name.rsplit("::", 1)[-1]


def expected_tests(scope: str) -> tuple[str, ...]:
    result = LIB_TESTS + DAEMON_TESTS + (GUI_TESTS if scope == "all" else ())
    if not LIB_TESTS or (scope == "all" and not GUI_TESTS):
        raise RuntimeError("HF130 native test registry is incomplete")
    if len(result) != len(set(result)):
        raise RuntimeError("HF130 native test registry contains duplicate names")
    if any(not name.startswith("hf130_") for name in result):
        raise RuntimeError("HF130 registry contains an unexpected test name")
    return result


def selected_names(output: str, pattern: re.Pattern[str]) -> list[str]:
    return [
        match.group(1)
        for line in output.splitlines()
        if (match := pattern.fullmatch(line.strip()))
        and test_basename(match.group(1)).startswith("hf130_")
    ]


def verify_names(names: list[str], expected: tuple[str, ...], phase: str) -> None:
    counts = Counter(test_basename(name) for name in names)
    missing = sorted(set(expected) - set(counts))
    unexpected = sorted(set(counts) - set(expected))
    duplicates = sorted(name for name, count in counts.items() if count != 1)
    if not names or missing or unexpected or duplicates:
        raise RuntimeError(
            f"HF130 {phase} registry mismatch: count={len(names)}, "
            f"missing={missing}, unexpected={unexpected}, duplicates={duplicates}"
        )


def run_command(command: list[str]) -> str:
    # Capture UTF-8 with replacement: a localized Windows console must not hide
    # a native failure behind a Python UnicodeDecodeError/UnicodeEncodeError.
    print("Running: " + " ".join(command), flush=True)
    lines: list[str] = []
    process = subprocess.Popen(
        command,
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        encoding="utf-8",
        errors="replace",
        text=True,
    )
    assert process.stdout is not None
    for line in process.stdout:
        lines.append(line)
        print(line, end="", flush=True)
    exit_code = process.wait()
    if exit_code != 0:
        raise RuntimeError(f"Native command failed with exit code {exit_code}")
    return "".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scope", choices=("all", "seed"), default="all")
    parser.add_argument(
        "--list-expected",
        action="store_true",
        help="Print registry only; does not compile or execute any Rust test.",
    )
    args = parser.parse_args()
    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="backslashreplace")
    expected = expected_tests(args.scope)
    if args.list_expected:
        print("HF130 expected registry only (native tests NOT executed):")
        print("\n".join(expected))
        return 0
    if not (ROOT / "Cargo.lock").is_file():
        raise RuntimeError(f"Missing locked source root: {ROOT}")
    target_args = ["--all-targets"] if args.scope == "all" else ["--lib", "--bin", "qubd"]
    base = ["cargo", "test", "--locked", "--color", "never", *target_args, "hf130_"]
    discovered_output = run_command([*base, "--", "--list"])
    discovered = selected_names(discovered_output, LIST_LINE)
    verify_names(discovered, expected, "discovery")
    # Serialize regressions that use process-global environment/state. Every
    # registered test must report an explicit `ok`; ignored tests do not count.
    executed_output = run_command([*base, "--", "--test-threads=1"])
    executed = selected_names(executed_output, PASS_LINE)
    verify_names(executed, expected, "execution")
    if set(discovered) != set(executed):
        raise RuntimeError("HF130 qualified test identities changed after discovery")
    print(f"HF130 NATIVE REGRESSIONS: PASS ({args.scope}; {len(expected)} tests executed)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as exc:
        print(f"HF130 NATIVE REGRESSIONS: FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
