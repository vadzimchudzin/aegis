# AEGIS 0.7.0-beta.10 — Full check, Repair, AI repair, Anthropic model choice

Windows installer only (`AEGIS_0.7.0-beta.10_x64-setup.exe`).

## Fixed
- **Errors that never disappeared.** The AI observer kept the last MT5 error forever: the account
  loop set `status.error` on failure and never cleared it. Every problem now has a source
  (account, market, frames, events, journal, decision, recovery) and is cleared by the source
  itself as soon as it works again. One-off click errors (e.g. "Connect RoboForex MT5 in Brokers
  first") vanish after 20 s or when observation starts.
- **`copy_rates_from_pos failed (-10001): IPC send failed`, `MT5 terminal is disconnected`,
  `MT5 account or symbol information is unavailable`.** The MT5 bridge never re-opened the
  terminal link after the terminal restarted, updated or Windows slept. It now detects IPC errors
  (-10001…-10005), re-initializes with the same login (never another account) and retries;
  failed attempts are rate-limited to one per 3 s. A crashed Python bridge is restarted by the app
  with the saved credentials within 15 s.
- **`quote timestamp is more than 10 seconds stale` at ~23:55 server time.** This is the daily gold
  break / weekend, not a fault. It is now shown as an amber "market closed or paused" notice;
  Paper/Money start is allowed and waits for fresh quotes (entries still require a fresh,
  validated quote; a future timestamp remains a hard error).
- Auto-connect no longer tries half-filled forms (no more "MT5 login is empty" on every start).

## New
- **Full check** (Settings → Diagnostics, rail, ⋯ menu, lamp, and the AI-trading status card):
  computer clock vs internet time, saved settings, every broker, MT5 bridge / IPC link / terminal /
  account / Algo Trading / XAUUSD quote / candles, local Qwen (server + real inference), OpenRouter
  key (free check), selected model, optional billable OpenRouter test, observer configuration and
  live issues. Failing items are red with cause and fix; **Show** opens and pulses the element
  that causes the problem; the rail and broker icons get a failure badge.
- **Repair**: runs only whitelisted, reversible actions — correct the AEGIS clock from internet
  time, reconnect a saved broker, re-open the MT5 link, start the local model server, clear stale
  messages. Changing the MT5 server offset is offered only as an explicit, confirmed action.
  Repair never places, closes or modifies orders and never changes risk.
- **Repair with AI**: local Qwen (free, offline) or the selected Anthropic model on OpenRouter
  explains the report in the interface language and proposes actions from the same whitelist; the
  user applies them. Account numbers and keys are masked before anything is sent to the cloud.
- **Anthropic model choice** (Settings → Models): live OpenRouter catalog of real-time
  `anthropic/*` models with prices; the selection is saved and used for decisions, tests and AI
  repair.
- Settings reorganized into **Appearance · AI trading · Models · Diagnostics**; the ⋯ menu lists
  Brokers, AI trading, Models, Diagnostics, Appearance, Densities. The header Stop button is shown
  only while AI runs or protects a position.

## CI
- Release builds and publishes the Windows NSIS installer only; every push to `main` uploads the
  installer as the `AEGIS-windows-installer` artifact.

## Verification
- Rust 216 tests, Python 104 tests, JS 39 tests, clippy `-D warnings`.
- Desktop E2E of the real binary (tauri-driver, mock Binance/Bybit, fake MT5 with fault
  injection, mock Ollama/OpenRouter): every new control clicked, MT5 outage → red cause → Repair →
  green, automatic IPC recovery, issues clearing after recovery, closed-market notice, model
  selection, local and cloud AI repair. Live RoboForex demo acceptance on Windows is still required.
