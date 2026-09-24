#!/usr/bin/env python3
"""Run the registered HF132 Rust regressions and reject missing/zero-test passes.

This script requires the native Rust toolchain. It does not replace the complete
Cargo suite or the GUI/mainnet smoke. Includes retained HF130 regressions.
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
# Updated together with the HF132 implementation before release packaging.
LIB_TESTS: tuple[str, ...] = (
    "hf132_memory_idle_peer_releases_handshake_snapshot",
    "hf132_memory_inbound_writers_drop_busy_candidates_then_retry",
    "hf132_memory_overlap_extension_matches_full_replay",
    "hf132_memory_invalid_overlap_and_suffix_preserve_parent",
    "hf132_memory_real_fork_keeps_full_validation_and_work_selection",
    "hf132_memory_streamed_chain_is_byte_identical_to_legacy_frames",
    "hf132_memory_serialization_failure_closes_partial_frame_and_restores_timeout",
    "hf132_completed_suffix_survives_expired_network_slice",
    "hf132_cancelled_suffix_keeps_committed_parent",
    "hf132_moving_tip_reuses_request_then_follows_committed_suffix",
    "hf132_duplicate_chunk_does_not_request_old_fork_window",
    "hf132_slow_peer_network_slice_leaves_healthy_peer_time",
    "hf132_reconcile_rejects_stale_capture_and_keeps_current_equal_work_commit",
    "hf132_stop_mining_waiter_does_not_cancel_active_sync_writer",
    "hf132_ui_snapshot_defers_while_live_owner_is_busy",
    "hf132_committed_readers_do_not_wait_for_storage_writer",
    "hf132_ui_genesis_initialization_defers_to_storage_writer",
    "hf132_validated_candidate_adoption_matches_public_replay",
    "hf132_verified_disk_baseline_rejects_ui_only_utxo_state",
    "hf132_validated_adoption_restores_local_tx_and_discards_peer_mempool",
    "hf132_rpc_slow_reader_releases_writer_and_keeps_captured_parent",

    "hf131_writer_lease_write_failure_releases_path",
    "hf131_writer_lease_flush_failure_releases_path",
    "hf131_rpc_block_relay_failure_preserves_identity_and_requires_retry",
    "hf131_rpc_block_relay_success_retains_ack_evidence",
    "hf131_rpc_transaction_failed_persistence_keeps_live_state_retryable",
    "hf131_silent_first_probe_does_not_hide_healthy_peers",
    "hf131_probe_cancellation_is_inherited_and_joined",
    "hf131_direct_quorum_counts_node_identity_once",
    "hf131_ahead_quorum_is_exact_pair_and_deduplicated",
    "hf131_equal_height_tie_never_creates_exact_proof",
    "hf131_repaired_block_acknowledgements_prove_direct_delivery",
    "hf131_relay_duplicates_and_legacy_are_not_delivery",
    "hf131_reachable_sync_peers_precede_silent_candidates",

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
DAEMON_TESTS: tuple[str, ...] = (
    "hf131_preflight_reserve_is_not_applicable_before_jin",
    "hf131_preflight_reserve_is_enforced_from_jin_activation",
    "hf131_preflight_bootstrap_is_enforced_from_infusion_activation",
    "hf131_preflight_history_errors_fail_closed",
    "hf131_preflight_invalid_bootstrap_parameters_fail_closed",
    "hf131_preflight_mainnet_genesis_retains_an_empty_jin_ledger",
)
GUI_TESTS: tuple[str, ...] = (
    "hf132_pending_solo_pool_intent_survives_sync_callbacks_and_stop",
    "hf132_refresh_and_manual_sync_share_one_attached_reader",
    "hf132_reader_panic_and_error_release_admission_with_real_outcome",
    "hf132_busy_gui_snapshot_defers_without_authoritative_replay_then_recovers",
    "hf132_progress_requires_observed_blocks_and_expiring_rate",
    "hf132_wait_history_coalesces_without_hiding_block_progress",

    "hf131_gpu_partial_initialization_releases_every_created_handle_once",
    "hf131_gpu_allocations_retain_library_until_reverse_cleanup_finishes",
    "hf131_gpu_api_retains_library_after_resource_cleanup",
    "hf131_gpu_cleanup_continues_after_driver_release_error",
    "hf131_gpu_null_allocations_never_call_release",
    "hf131_gpu_library_load_scope_error_releases_module",
    "hf131_found_candidate_survives_stop_before_publication",
    "hf131_found_candidate_honors_manual_stop_and_first_result",
    "hf131_gui_peer_roles_cannot_promote_stale_or_gossip_tips",
    "hf131_mining_qns_resolution_is_pinned_across_retries",
    "hf131_mining_invalid_or_cross_network_payout_is_permanent",
    "hf131_pool_mining_requires_owned_resolved_address",
    "hf131_resolved_payout_preserves_restart_mode_and_generation",

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
        raise RuntimeError("HF132 native test registry is incomplete")
    if len(result) != len(set(result)):
        raise RuntimeError("HF132 native test registry contains duplicate names")
    if any(not name.startswith(("hf130_", "hf131_", "hf132_")) for name in result):
        raise RuntimeError("HF132 registry contains an unexpected test name")
    return result


def selected_names(output: str, pattern: re.Pattern[str]) -> list[str]:
    return [
        match.group(1)
        for line in output.splitlines()
        if (match := pattern.fullmatch(line.strip()))
        and test_basename(match.group(1)).startswith(("hf130_", "hf131_", "hf132_"))
    ]


def verify_names(names: list[str], expected: tuple[str, ...], phase: str) -> None:
    counts = Counter(test_basename(name) for name in names)
    missing = sorted(set(expected) - set(counts))
    unexpected = sorted(set(counts) - set(expected))
    duplicates = sorted(name for name, count in counts.items() if count != 1)
    if not names or missing or unexpected or duplicates:
        raise RuntimeError(
            f"HF132 {phase} registry mismatch: count={len(names)}, "
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
        print("HF132 expected registry only (native tests NOT executed):")
        print("\n".join(expected))
        return 0
    if not (ROOT / "Cargo.lock").is_file():
        raise RuntimeError(f"Missing locked source root: {ROOT}")
    target_args = ["--all-targets"] if args.scope == "all" else ["--lib", "--bin", "qubd"]
    base = ["cargo", "test", "--locked", "--color", "never", *target_args, "hf13"]
    discovered_output = run_command([*base, "--", "--list"])
    discovered = selected_names(discovered_output, LIST_LINE)
    verify_names(discovered, expected, "discovery")
    # Serialize regressions that use process-global environment/state. Every
    # registered test must report an explicit `ok`; ignored tests do not count.
    executed_output = run_command([*base, "--", "--test-threads=1"])
    executed = selected_names(executed_output, PASS_LINE)
    verify_names(executed, expected, "execution")
    if set(discovered) != set(executed):
        raise RuntimeError("HF132 qualified test identities changed after discovery")
    print(f"HF132 NATIVE REGRESSIONS: PASS ({args.scope}; {len(expected)} tests executed)")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError) as exc:
        print(f"HF132 NATIVE REGRESSIONS: FAIL: {exc}", file=sys.stderr)
        raise SystemExit(1)
