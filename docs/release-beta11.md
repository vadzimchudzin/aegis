# AEGIS 0.7.0-beta.11 — Journal rotation

Windows installer only (`AEGIS_0.7.0-beta.11_x64-setup.exe`).

## Fixed
- **`journal append would exceed the 32 MiB limit`.** The AI observer journal (`decisions.jsonl`)
  had a hard 32 MiB cap and no rotation: once full, every record was refused and Paper/Money
  trading could not start. The journal now rotates itself before it reaches 24 MiB:
  - the full journal is compressed with zstd into `journal-archive/decisions-<time>.jsonl.zst`
    next to it and read back with a SHA-256 check before anything is replaced;
  - the new journal starts with a `journal_checkpoint` record (last simulator state, daily
    baseline, decision/outcome totals moved to the archive), followed by every unresolved broker
    intent and the newest ~4 MiB of records unchanged; the file is replaced atomically;
  - after a restart the app recovers the same Paper balance, position, daily baseline, pending
    broker operations and counters as from the whole history;
  - a corrupt journal is never rotated or modified.
- Upgrading is enough: the first record written after the update rotates an already full journal.
  Do not delete or move `decisions.jsonl` by hand — that resets the Paper account and counters.

## Verification
- `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace` (incl. 4 new rotation
  tests: full journal, exact archive round-trip, repeated rotations, corrupt journal untouched),
  JS and Python test suites. Live RoboForex demo acceptance on Windows is still required.
