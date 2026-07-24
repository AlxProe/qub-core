# HF127 / v1.8.3 source revision r2 — compile correction

This source revision keeps the public release identity unchanged:

```text
QUB Core v1.8.3
HF127
v183
```

It corrects two compile-time issues discovered by the first dependency-backed Windows `cargo test` run.

## Corrections

1. `src/main.rs` now declares `#![recursion_limit = "512"]` so the expanded `serde_json::json!` status object compiles reliably. This changes only macro expansion limits at compile time and has no runtime or consensus effect.

2. `src/bin/qub_core.rs` computes the CPU worker-override prefix before mutably borrowing `self.prefs.cpu_worker_override` for `egui::DragValue`. This resolves Rust borrow error `E0503` without changing the control's behavior.

## Unchanged

No changes were made to consensus, block/transaction serialization, hashes, Protocol Epoch 2, DAA, rewards, checkpoints, Fast Chain Engine schema, P2P protocol, transaction-delivery protocol, mining algorithms, or network configuration.

The first HF127 source archive is superseded. Use the r2 source archive or the r2 replacement files, then rerun the complete locked Cargo/test gate.
