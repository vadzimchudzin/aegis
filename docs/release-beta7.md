# AEGIS 0.7.0-beta.7

Experimental prerelease, not a stable release and not acceptance for real-money trading.
Paper is the default; Money remains off unless explicitly armed for the current session.

## Changes since beta.6

- **Weekend gaps no longer block 4h and 1d frames.** The observer required every pair of the
  last 21 closed bars to be exactly one period apart, so any broker calendar failed: D1
  Friday to Monday is three days and H4 Friday 20:00 to Monday 00:00 is thirteen slots. Both
  frames reported `contains gaps` on real RoboForex history. A gap is now accepted when it is
  a whole number of periods and the skipped span contains a Saturday or Sunday. A hole in the
  middle of the week, a non-multiple timestamp, or a gap longer than four days is still
  reported as gaps.
- **A busy local model is no longer reported as missing.** Readiness was a single 3-second
  probe, so a server loading a model or finishing a CPU generation looked absent; `Start` then
  spawned a second process that could not bind the port and exited, producing
  `Ollama exited` while a healthy server was serving the endpoint. Readiness is retried three
  times, and a child that exits on bind is treated as success when the endpoint already
  answers.
- **A stale snapshot is retried before failing.** A quiet market can go longer than the
  10-second staleness budget without a tick. The snapshot is requested up to three times,
  700 ms apart, and only staleness is retried; every other validation error still fails
  immediately and the thresholds are unchanged.
- **A model server that dies mid-session is restarted once.** A refused connection to 11435
  used to surface as `error sending request for url (...)`, which reads like a network fault,
  and it ended the run until a manual Start even when the server had exited after passing the
  readiness check. A connect failure now says that the local model server is not running, and
  the decision loop restarts the runtime once and retries the same snapshot. Timeouts are not
  retried, and the event scoring path still rejects an event on failure instead of retrying
  it, because a retry there could duplicate tool dispatch.

## Verification and limitations

- 176 `aegis-core` unit tests, 6 MT5 bridge integration tests, 3 partial-PnL and 3 venue
  tests pass; strict `cargo clippy -p aegis-core --all-targets -D warnings` and
  `cargo fmt --all --check` are clean.
- New tests: a D1 series that crosses weekends summarizes without gaps, a direct gap helper
  covering daily and four-hour weekend spans, a mid-week hole that must still be rejected, and
  unchanged rejection of non-multiple timestamps. The pre-existing mid-week gap test passes
  unchanged.
- **No live broker acceptance was performed.** The gap rule was validated against synthetic
  calendars, not against a recorded RoboForex history. If an H4 frame still reports gaps after
  this build, the broker has holes in the middle of the week (a holiday or an empty session
  window); the JSONL journal is needed to confirm which bars are missing. A trading-holiday
  calendar is not part of this release.
- The AI readiness change addresses false negatives only. If the endpoint genuinely does not
  answer, the same error is reported after the retries; the legacy runtime on port 11434 is a
  separate path and is not started by this service.

## Distribution and release gate

Expected installers: Windows NSIS setup `.exe`, Linux `.deb`, and macOS universal `.dmg`
(Apple Silicon + Intel). Packaging on three runners is not an installation smoke test.
The macOS bundle uses the existing ad-hoc signing setting; notarization is not claimed.

All three platform test jobs must pass before packaging starts. Each packaging job reruns
Rust, Python and Node tests before the Tauri action. All three Tauri jobs upload to the same
draft release. Only a final job depending on the entire successful build matrix may publish:
it finds the draft by listing releases (a draft has no git tag yet), checks that the draft
targets the tested commit and that nonempty, uploaded `.exe`, `.deb`, and `.dmg` assets
exist, then publishes beta.7 with `prerelease: true`. A failed matrix or missing asset leaves
the release draft; there is no publicly partial installer release.
