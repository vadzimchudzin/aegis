# AEGIS 0.7.0-beta.9 — explicit MT5 clock configuration

## What changed

Beta.9 adds a persistent, optional, nonsecret **MT5 server clock offset** setting to the RoboForex broker connection form. It is an explicit timestamp correction, not automatic timezone detection. The field is stored with that broker's settings and restored at startup. The dropdown offers environment/UTC fallback, explicit UTC (0), and UTC−14 through UTC+14 in one-hour steps. The underlying `server_offset_seconds` field accepts signed integer seconds from −50400 to +50400 inclusive.

Precedence: explicit connection setting (including 0) → `AEGIS_MT5_SERVER_OFFSET_SECONDS` when the connection field is unset → standard UTC (0). Invalid explicit values fail validation even if the environment is valid. A connection resolves its correction once; changing the environment or form does not change an existing session. **Reconnect to apply changes.** Old saved connections without the new field keep environment/UTC fallback.

For every quote, candle and tick timestamp, `normalized UTC = raw timestamp − configured offset`. Historical tick query bounds are converted to the same raw timestamp domain before requesting rows, then returned rows are normalized back to UTC. A cached tick is never used to infer an offset or made fresh by changing its timestamp to now. Existing freshness checks remain enabled: quotes more than 2 seconds in the future or more than 10 seconds old remain invalid for live sampling.

## Exact Windows setup steps (including a true UTC+5 locale)

1. Stop any AI strategy and leave real-money mode disabled. No live trading is needed for this check. Use a demo account if connecting to verify your own installation.
2. In Windows **Settings → Time & language → Date & time**, enable **Set time automatically**, keep your true local timezone (for example **UTC+05:00**), and click **Sync now**. If automatic timezone chooses the wrong locale, turn only automatic timezone selection off and select your true timezone manually. Do not change your timezone to UTC or shift the Windows clock by three hours to silence AEGIS.
3. Open your MT5 terminal and confirm the intended account and server. Check that the terminal is connected and the market is open; a weekend/closed-market cached quote can legitimately be stale. Windows local time display and the broker timestamp convention are different settings.
4. Open AEGIS **Brokers → RoboForex**. For the first diagnostic connection, leave **MT5 server clock offset** at **Environment / UTC default (not auto-detected)** if no override is intended. Be aware an existing environment variable still applies. To explicitly test standard UTC and ignore that variable, choose **UTC (+0)** instead.
5. Click **Connect** (or **Reconnect** for an existing session). In the checklist read **Raw quote timestamp (UTC)**, **Computer UTC time**, **Configured broker offset (seconds)**, **Clock configuration source**, and **Normalized quote age**. Raw quote UTC means the raw Unix timestamp rendered as UTC, before any correction. A negative age is in the future; a positive age is old. The checklist explicitly warns future/stale data. It does not claim that timezone was automatically detected.
6. If the raw quote appears approximately **three hours ahead**, **ask your broker to confirm the timestamp convention for this exact MT5 server/account and date**, including daylight-saving behavior. An observed +3h difference is evidence to investigate, not proof that +3h is the right correction. An incorrect Windows clock or an old quote must not be compensated with a guessed offset. Your local UTC+5 timezone does not imply a +5h or +3h broker offset.
7. **Only if the broker confirms raw timestamps are UTC+3**, choose **UTC+3** (10800 seconds), then click **Reconnect**. For confirmed UTC+2 choose UTC+2 (7200); for standard UTC choose UTC (+0). Positive means raw timestamps are ahead of UTC and the configured seconds are subtracted. Never choose +3 merely because your locale is UTC+5 or because the quote looked future.
8. Recheck the diagnostics. On an open market with fresh ticks, normalized quote age should be near zero. If it remains future/stale, stop and investigate Windows synchronization, terminal connectivity, market hours, account/server identity and broker convention. Do not disable freshness checks or keep adjusting offsets until an old tick appears fresh.
9. Restart AEGIS to confirm the selected connection setting persists. The offset is not a password and is shown in saved broker values. Revisit it when the broker changes timestamp convention or DST; AEGIS deliberately does not guess or auto-adjust it.

## Russian / Қазақша

**Русский:** сохраняйте настоящий местный часовой пояс Windows (UTC+5, если он верный) и синхронизируйте часы. В форме подключения RoboForex выберите подтверждённое брокером смещение времени сервера MT5 и переподключитесь. Разница +3 часа требует подтверждения брокера; это не автоопределённый часовой пояс. Исходное время котировки, UTC компьютера и возраст после коррекции видны в проверках. Старые и будущие котировки не становятся допустимыми автоматически.

**Қазақша:** Windows жүйесінде нақты жергілікті уақыт белдеуін (дұрыс болса UTC+5) сақтап, уақытты синхрондаңыз. RoboForex қосылымында брокер растаған MT5 сервері уақытының ығысуын таңдап, қайта қосылыңыз. +3 сағат айырмасы брокердің растауын қажет етеді; бұл автоматты анықталған уақыт белдеуі емес. Бастапқы баға уақыты, компьютердің UTC уақыты және түзетілген жас тексерулерде көрсетіледі. Ескірген және болашақтағы бағалар автоматты түрде жарамды болмайды.

## Verification and publication policy

Regression coverage includes explicit/environment/UTC precedence; signed integer and range rejection; immutable explicit zero; stale quote preservation under correction; timezone independence; quotes/candles/live and historical ticks in the same time domain; Rust form parsing, bridge JSON forwarding and saved-setting round trips; frontend dropdown and ru/kk guidance.

The Linux local-AI stop test now publishes its child PID using a closed temporary file plus atomic replacement, then parses a positive PID before checking `/proc/<pid>/stat`. The actual SIGTERM-ignoring runner-kill assertion remains intact. No production stop behavior was weakened.

This branch starts at beta.8 commit `5376a3adffe03f9b86a88f3c77b0226f18ae7f99`. Published beta.8 is unchanged. Release gates retain full workspace tests on Linux/Windows/macOS, core lint and formatting, Python and frontend tests, desktop installer builds, immutable SHA/tag validation and nonempty `.exe`, `.deb`, `.dmg` asset checks before publication. Merge is permitted only after both full PR CI and release gates succeed. No live broker or Money-mode trading was used to implement or test this release.
