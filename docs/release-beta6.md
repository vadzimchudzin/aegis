# AEGIS 0.7.0-beta.6

Experimental prerelease, not a stable release and not acceptance for real-money trading.
Paper is the default; Money remains off unless explicitly armed for the current session.

## Changes since beta.5

- MT5 broker clock is normalized instead of being rejected. `MarketSample::validate()`
  refuses a quote more than 2 s ahead of the computer clock, and the bridge subtracted an
  offset that `_server_offset()` always returned as `0`. A broker running on EET
  (RoboForex, UTC+2/+3) therefore produced quotes three hours "in the future", and Paper
  start failed with `quote timestamp is more than 2 seconds in the future` before the model
  was consulted at all.
- The offset is measured from a live tick: a small skew, or a timezone-shaped difference of
  whole or half hours. A stale tick never produces an offset — the honest failure for it
  remains `stale`, so a closed market is not rewritten into a timezone correction.
- `history_ticks()` now subtracts the offset too. It never did, so with a shifted server
  clock every row fell outside the requested UTC slice and the reply came back empty.
- The connect checklist gains a `clock` entry showing the measured correction
  (`server time +3h 00m, normalized to the computer clock`), so the correction is visible
  rather than implicit.
- The 2 s future and 10 s stale thresholds are unchanged. Nothing here relaxes a safety
  check; the timestamps are simply converted to the clock the app compares against.
- Tests: a +3 h broker server, a +3 s skew, and a stale tick that must stay stale. The Rust
  integration checklist assertion gains the `clock` entry.

## Verification and limitations

- Local verification of this change: 173 `aegis-core` unit tests, 6 MT5 bridge integration
  tests, 3 partial-PnL tests and 3 venue tests pass; strict `cargo clippy -p aegis-core
  --all-targets -D warnings` and `cargo fmt --all --check` are clean. The Python bridge
  suite passes with the four failures that pre-date this change on a machine that has the
  real `MetaTrader5` package installed and applies Windows symlink semantics.
- **No live broker acceptance was performed for this fix.** The measurement logic is covered
  by fixtures only. Confirming that Paper starts against a real RoboForex terminal, and that
  the `clock` entry reports the expected correction, remains a separate check on Windows.
- `event_cycle` test isolation is fixed: per-test temporary directories were keyed by process
  id and a `now()`-derived stamp, so two tests reading clocks a millisecond apart shared a
  directory, deleted each other's journal and failed intermittently in the parallel full-suite
  run. A per-process counter makes the name unique, with a regression test asserting that two
  setups with the same stamp get different directories. Eight consecutive full-suite runs pass.
- The published beta.5 installers contain the old code: checking updated sources does not
  upgrade an existing installation. Only verified beta.6 assets contain these changes.

## Distribution and release gate

Expected installers: Windows NSIS setup `.exe`, Linux `.deb`, and macOS universal `.dmg`
(Apple Silicon + Intel). Packaging on three runners is not an installation smoke test.
The macOS bundle uses the existing ad-hoc signing setting; notarization is not claimed.

All three platform test jobs must pass before packaging starts. Each packaging job reruns
Rust, Python and Node tests before the Tauri action. All three Tauri jobs upload to the same
draft release. Only a final job depending on the entire successful build matrix may publish:
it checks that the tag matches the tested checkout and nonempty, uploaded `.exe`, `.deb`,
and `.dmg` assets exist, then publishes beta.6 with `prerelease: true`. A failed matrix or
missing asset leaves the release draft; there is no publicly partial installer release.
