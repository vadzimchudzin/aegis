# AEGIS 0.7.0-beta.8

Experimental prerelease. Not acceptance for real-money trading. Paper remains the default; no trading safety gate is removed.

## Fixes

- Bounce screener keeps all expected-entry lines but labels at most four nearby levels, separated by at least 36 screen pixels. Hidden axis labels have no long title covering candles.
- Responses from superseded chart/timeframe requests or slower overlapping refreshes no longer overwrite the current chart. Failed refreshes remove obsolete overlays.
- Archive fallback is no longer rendered as live expected entries. Invalid entry prices/probabilities are discarded.
- Weekend gaps are accepted only when every skipped bar is a UTC weekend slot. Missing Monday or Friday bars are still rejected.
- Closed-candle freshness discounts bounded weekend closures at Monday reopen. Quote freshness thresholds are unchanged; broker-specific holidays are not guessed.
- Local-model restart failures are preserved rather than discarded. After restart, stopped/superseded sessions cannot submit another prompt; retry inference remains cancellable. An expired snapshot is not replayed after warmup, and its strategy key is released for the next market notification.
- Release builds retain beta.7's successful OS-specific Python interpreter fix.

## Important MT5 timestamp configuration change

Automatic clock-offset inference from a single cached tick has been removed. A timezone offset and the age of a cached quote cannot be distinguished from that tick alone. Previous inference could incorrectly mark an old quote as current.

Standard MT5 UTC timestamps require no setting (offset defaults to zero). If your terminal is independently verified to return broker-local timestamps, set `AEGIS_MT5_SERVER_OFFSET_SECONDS` in the application's environment **before launching AEGIS**. For a verified UTC+3 timestamp convention, the value is `10800`; for UTC+2 it is `7200`. Do not choose this value merely to silence a stale/future quote error. Verify terminal timestamps against UTC and check that fresh and deliberately old ticks retain their actual age. Restart AEGIS after changing the value.

Windows PowerShell example, only for an independently verified UTC+3 terminal:

```powershell
$env:AEGIS_MT5_SERVER_OFFSET_SECONDS = "10800"
# Launch the installed AEGIS executable from this same PowerShell session.
```

Invalid values and offsets beyond 14 hours are rejected. Missing/unknown offset does not cause a cached future-dated tick to be relabelled as current. This intentionally fails closed rather than bypassing freshness checks.

## Verification and limitations

Local checks: 35 frontend tests, 88 Python tests, and Rust core unit/integration tests; formatting and core Clippy are also checked. Added regressions cover dense/sparse labels, chart races, failure cleanup, invalid entries, archive fallback, Monday reopen, missing weekday bars, and stale ticks combined with a configured broker offset.

Full desktop compilation requires GTK/WebKit system libraries unavailable in the editing sandbox; GitHub's release verification/build matrix is the publication gate. Local MT5, broker connectivity, Qwen CPU/RAM behavior and the real desktop screen still need acceptance on the user's Windows machine. No real order was sent.

Remaining known limitation: local model startup/warmup is allowed to complete its cleanup path after Stop; the old session cannot submit retry inference afterwards. This change does not implement a fully cancellable model startup service, nor automatic recovery for every possible truncated-response failure.
