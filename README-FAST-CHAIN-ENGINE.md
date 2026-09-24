# QUB Fast Chain Engine (`QUB-FCE-1`)

HF123 moves QUB Core persistence away from rewriting and reparsing one growing `chain.json` file during normal operation.

## Layout

```text
<data_dir>/
├── chain-v2/
│   ├── CURRENT.json
│   ├── PREVIOUS.json
│   ├── WRITE.lock
│   ├── blocks-<generation>.jsonl
│   ├── state-<generation>-<revision>.json
│   └── legacy-export-status.json
├── chain-status.json
├── chain.json                  # compatibility export, not the hot path
├── wallet.json
└── wallet-pending-txs.json
```

`CURRENT.json` identifies one committed generation and state revision. `PREVIOUS.json` preserves the immediately preceding committed pointer for bounded recovery.

## Commit protocol

A normal append commit performs this sequence:

1. Acquire the per-data-directory writer lease.
2. Verify the candidate is not behind the committed total work.
3. Truncate any bytes beyond the committed journal prefix.
4. Append newly connected blocks and synchronize the journal.
5. Write and synchronize an immutable state snapshot.
6. Write `PREVIOUS.json` from the old committed pointer.
7. Atomically replace `CURRENT.json` with the new pointer.
8. Publish the committed live chain after releasing the canonical storage lock.
9. Run derived status/cleanup maintenance after live publication. Desktop Core
   and persistent node/API services use detached workers and schedule periodic
   compatibility exports. Finite qubd commands finish a non-fatal maintenance
   pass in their own thread and use explicit full-chain exports when needed.

A branch replacement creates a new journal generation rather than overwriting the current committed branch.

## Startup and migration

When `chain-v2/` does not exist but a legacy `chain.json` exists:

1. QUB Core fully validates the legacy chain.
2. It creates the initial Fast Chain Engine generation.
3. The original `chain.json` remains available as the first compatibility export.

A successful finite `init` requires CURRENT and its referenced committed state
and journal. `chain-status.json` is derived metadata; `status-fast` can reconstruct
it. `chain.json` is an optional compatibility export. Do not wait for detached
threads after a CLI process has exited or treat either derived file as the
canonical initialization marker.

Migration is local storage work only. It does not change block hashes, transaction IDs, consensus state or wallet keys.

Do not interrupt the first validated migration. Take a backup of the data directory before a production seed upgrade.

## Recovery

- A journal suffix written before a pointer commit is ignored and truncated by the next writer.
- If the complete `CURRENT.json` commit cannot be read or verified, including its referenced state or journal, QUB Core attempts `PREVIOUS.json`.
- If both Fast Chain Engine commits are unusable, startup fails closed. QUB Core never silently replaces them from a potentially stale compatibility export; an operator may perform explicit recovery from a separately verified export and then resynchronize.
- State files are SHA-256 checked and cross-checked against the pointer.
- Journal records are parsed in order and checked for genesis, height-specific block version, hash-link continuity, checkpoint consistency and committed tip equality.

## Operator commands

Fast status:

```bash
qubd --config <config.toml> status-fast
```

Storage metrics:

```bash
qubd --config <config.toml> storage-stats
```

Explicit compatibility export:

```bash
qubd --config <config.toml> export-chain-json /path/to/chain.json
```

Full consensus replay remains available through:

```bash
qubd --config <config.toml> validate
```

## Process model

Only one state-changing QUB process may own a data directory. The Fast Chain Engine uses both a process-local mutex and an on-disk writer lease. Embedded P2P, RPC and GUI readers share immutable in-memory snapshots and do not use `chain.json` as an inter-thread communication channel.

A separate headless node must use a separate data directory, as demonstrated by `config/headless-mainnet.toml`.
