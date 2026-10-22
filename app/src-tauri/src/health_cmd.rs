//! Full check, Repair and the AI repair assistant.
//!
//! The full check inspects every subsystem (computer clock, saved brokers, the MT5 terminal
//! link, the local model, OpenRouter and the AI observer) and reports each problem with a
//! stable code, its likely cause, a fix and the repair actions that can resolve it.
//!
//! Repair runs only whitelisted, reversible actions: reconnect a saved broker, re-open the MT5
//! terminal link, correct the application clock, start the local model server, clear stale
//! issues, or (explicitly) set the MT5 server offset. It never places, closes or changes
//! orders, never changes risk, and never disables a safety check. The AI assistant (local
//! Qwen or the selected Anthropic model on OpenRouter) only explains the report and proposes
//! actions from the same whitelist; the user applies them.

use std::{collections::BTreeMap, time::Instant};

use aegis_core::{
    ai_provider, clock,
    local_ai::{Runtime, ENDPOINT},
    BrokerId, Timeframe,
};
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::{local_ai_cmd::LocalAiState, observer_cmd::ObserverState, AppState};

static CHECK_LOCK: Mutex<()> = Mutex::const_new(());
static REPAIR_LOCK: Mutex<()> = Mutex::const_new(());

const MAX_STALE_MS: i64 = 10_000;
const MAX_FUTURE_MS: i64 = 2_000;

#[derive(Clone, Serialize)]
pub struct HealthItem {
    pub id: String,
    pub group: &'static str,
    /// Stable message code; the window renders localized text from it.
    pub code: String,
    pub params: BTreeMap<String, String>,
    /// ok | warn | fail | skip
    pub status: &'static str,
    pub title: String,
    pub detail: String,
    pub fix: String,
    pub actions: Vec<String>,
    /// Window element to highlight: `broker:<id>`, `models:local`, `models:cloud`, `trading`, `clock`.
    pub target: Option<String>,
    pub latency_ms: u64,
}

#[derive(Clone, Serialize)]
pub struct HealthReport {
    pub at_ms: u64,
    pub overall: &'static str,
    pub counts: BTreeMap<&'static str, usize>,
    pub items: Vec<HealthItem>,
    /// Safe automatic actions Repair will run for this report.
    pub actions: Vec<String>,
    pub deep: bool,
    pub elapsed_ms: u64,
}

struct Item(HealthItem);

impl Item {
    fn new(id: &str, group: &'static str, code: &str, status: &'static str, title: &str) -> Self {
        Item(HealthItem {
            id: id.into(),
            group,
            code: code.into(),
            params: BTreeMap::new(),
            status,
            title: title.into(),
            detail: String::new(),
            fix: String::new(),
            actions: Vec::new(),
            target: None,
            latency_ms: 0,
        })
    }
    fn detail(mut self, text: impl Into<String>) -> Self {
        self.0.detail = text.into();
        self
    }
    fn fix(mut self, text: impl Into<String>) -> Self {
        self.0.fix = text.into();
        self
    }
    fn param(mut self, key: &str, value: impl ToString) -> Self {
        self.0.params.insert(key.into(), value.to_string());
        self
    }
    fn action(mut self, action: impl Into<String>) -> Self {
        self.0.actions.push(action.into());
        self
    }
    fn target(mut self, target: impl Into<String>) -> Self {
        self.0.target = Some(target.into());
        self
    }
    fn latency(mut self, at: Instant) -> Self {
        self.0.latency_ms = at.elapsed().as_millis() as u64;
        self
    }
}

fn seconds(ms: i64) -> String {
    format!("{:.1}", ms as f64 / 1000.0)
}

fn short(text: &str, max: usize) -> String {
    let mut out: String = text.chars().take(max).collect();
    if text.chars().count() > max {
        out.push('…');
    }
    out
}

/// Whole half hours, as accepted by the MT5 offset field.
fn round_half_hour_s(ms: i64) -> i64 {
    ((ms as f64 / 1_800_000.0).round() as i64) * 1800
}

fn fail_reason(report: &Value) -> String {
    report["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|c| c["status"] == "fail"))
        .map(|c| {
            let detail = c["detail"].as_str().unwrap_or("");
            if detail.is_empty() {
                format!("{} failed", c["label"].as_str().unwrap_or("check"))
            } else {
                detail.to_string()
            }
        })
        .unwrap_or_else(|| "not connected".into())
}

// ---- checks ------------------------------------------------------------------------------

async fn check_clock(items: &mut Vec<HealthItem>) {
    let at = Instant::now();
    let check = clock::measure().await;
    let item = match check.residual_ms {
        None => Item::new(
            "system.clock",
            "system",
            "system.clock.unknown",
            "warn",
            "Computer clock",
        )
        .detail(format!(
            "Internet time could not be measured: {}",
            check.failures.join("; ")
        ))
        .fix("Check the internet connection; the clock is checked again on the next full check."),
        Some(residual) if residual.abs() < clock::SIGNIFICANT_SKEW_MS => {
            if check.correction_ms != 0 {
                Item::new(
                    "system.clock",
                    "system",
                    "system.clock.corrected",
                    "ok",
                    "Computer clock",
                )
                .param("correction_s", seconds(check.correction_ms))
                .detail(format!(
                    "AEGIS corrects the Windows clock by {} s; synchronize Windows time to remove the correction",
                    seconds(check.correction_ms)
                ))
            } else {
                Item::new("system.clock", "system", "system.clock.ok", "ok", "Computer clock")
                    .param("offset_s", seconds(residual))
                    .detail(format!("Within {} s of internet time", seconds(residual.abs())))
            }
        }
        Some(residual) => {
            let ahead = residual < 0;
            let status = if residual.abs() >= 10_000 { "fail" } else { "warn" };
            Item::new("system.clock", "system", "system.clock.skew", status, "Computer clock")
                .param("offset_s", seconds(residual.abs()))
                .param("direction", if ahead { "ahead" } else { "behind" })
                .detail(format!(
                    "The computer clock is {} s {} internet time: fresh MT5 quotes look stale or from the future and exchange signatures fail",
                    seconds(residual.abs()),
                    if ahead { "ahead of" } else { "behind" }
                ))
                .fix("Windows Settings → Time & language → Date & time → Sync now. Repair corrects the AEGIS clock immediately.")
                .action("sync_clock")
                .target("clock")
        }
    };
    items.push(item.latency(at).0);
}

async fn check_brokers(app: &AppHandle, items: &mut Vec<HealthItem>) {
    let state = app.state::<AppState>();
    let (public, key_error) = {
        let settings = state.settings.lock().await;
        (settings.public(), settings.key_error().map(str::to_string))
    };
    if let Some(error) = key_error {
        items.push(
            Item::new("system.settings", "system", "system.settings.fail", "fail", "Saved settings")
                .detail(error)
                .fix("Saved secrets cannot be decrypted on this Windows account. Enter the broker passwords and the OpenRouter key again.")
                .target("broker:roboforex")
                .0,
        );
    } else {
        items.push(
            Item::new(
                "system.settings",
                "system",
                "system.settings.ok",
                "ok",
                "Saved settings",
            )
            .detail("Encrypted settings are readable")
            .0,
        );
    }
    for broker in BrokerId::ALL {
        let id = format!(
            "broker.{}",
            serde_json::to_value(broker)
                .ok()
                .and_then(|v| v.as_str().map(String::from))
                .unwrap_or_default()
        );
        let key = id.trim_start_matches("broker.").to_string();
        let title = broker.name();
        let saved = public.brokers.get(&broker);
        let connector = state.sessions.lock().await.get(&broker).map(|s| s.connector.clone());
        let Some(connector) = connector else {
            let complete = saved.is_some_and(|b| b.complete);
            if !complete {
                items.push(
                    Item::new(&id, "brokers", "broker.skip", "skip", title)
                        .detail("Not configured")
                        .target(format!("broker:{key}"))
                        .0,
                );
                continue;
            }
            let reason = saved
                .and_then(|b| b.last_report.as_ref())
                .and_then(|r| serde_json::to_value(r).ok())
                .map(|r| fail_reason(&r))
                .unwrap_or_else(|| "saved but not connected".into());
            items.push(
                Item::new(&id, "brokers", "broker.fail", "fail", title)
                    .param("reason", short(&reason, 240))
                    .detail(format!("Saved but not connected: {}", short(&reason, 240)))
                    .fix(
                        "Repair reconnects with the saved credentials. If it fails again, check the fields in Brokers.",
                    )
                    .action(format!("reconnect:{key}"))
                    .target(format!("broker:{key}"))
                    .0,
            );
            continue;
        };
        if broker == BrokerId::Roboforex {
            check_mt5(app, &connector, items).await;
            continue;
        }
        let at = Instant::now();
        match connector.candles(Timeframe::M1, 2).await {
            Ok(_) => items.push(
                Item::new(&id, "brokers", "broker.ok", "ok", title)
                    .detail(format!("Connected · {} answers", connector.symbol()))
                    .latency(at)
                    .0,
            ),
            Err(error) => items.push(
                Item::new(&id, "brokers", "broker.ping_fail", "fail", title)
                    .param("error", short(&error.to_string(), 240))
                    .detail(format!("Connected, but market data fails: {error}"))
                    .fix("Repair reconnects the session. Check the internet connection and the API key permissions.")
                    .action(format!("reconnect:{key}"))
                    .target(format!("broker:{key}"))
                    .latency(at)
                    .0,
            ),
        }
    }
}

async fn check_mt5(app: &AppHandle, connector: &aegis_core::Connector, items: &mut Vec<HealthItem>) {
    let at = Instant::now();
    let money = crate::observer_cmd::health_snapshot(&app.state::<ObserverState>())
        .await
        .0
        .mode
        == "money";
    if !connector.alive().await {
        items.push(
            Item::new("mt5.bridge", "mt5", "mt5.bridge.dead", "fail", "MT5 bridge")
                .detail("The Python bridge to MetaTrader 5 has stopped")
                .fix("Repair restarts the bridge and logs in with the saved RoboForex credentials.")
                .action("reconnect:roboforex")
                .target("broker:roboforex")
                .0,
        );
        return;
    }
    let report = match connector.mt5_health().await {
        Some(Ok(report)) => report,
        Some(Err(error)) => {
            items.push(
                Item::new("mt5.bridge", "mt5", "mt5.bridge.error", "fail", "MT5 bridge")
                    .param("error", short(&error.to_string(), 240))
                    .detail(format!("The bridge does not answer: {error}"))
                    .fix("Repair restarts the bridge. If Python or the MetaTrader5 package is missing, install them (see Brokers → RoboForex).")
                    .action("reconnect:roboforex")
                    .target("broker:roboforex")
                    .latency(at)
                    .0,
            );
            return;
        }
        None => return,
    };
    items.push(
        Item::new("mt5.bridge", "mt5", "mt5.bridge.ok", "ok", "MT5 bridge")
            .detail("Python bridge answers")
            .latency(at)
            .0,
    );
    let recovered = report["recovery"]["recovered"].as_u64().unwrap_or(0);
    if report["ipc"] != true {
        let error = report["ipc_error"].as_str().unwrap_or("").to_string();
        items.push(
            Item::new("mt5.link", "mt5", "mt5.link.fail", "fail", "MT5 terminal link (IPC)")
                .param("error", short(&error, 240))
                .detail(format!("The link to the MetaTrader 5 terminal is broken: {error}"))
                .fix("Start the RoboForex MetaTrader 5 terminal and log in, then press Repair. AEGIS also retries automatically every few seconds.")
                .action("mt5_reinit")
                .target("broker:roboforex")
                .0,
        );
        return;
    }
    items.push(
        Item::new("mt5.link", "mt5", "mt5.link.ok", "ok", "MT5 terminal link (IPC)")
            .param("recovered", recovered)
            .detail(if recovered > 0 {
                format!("Working; restored automatically {recovered} time(s) after the terminal restarted")
            } else {
                "Working".into()
            })
            .0,
    );
    let terminal = &report["terminal"];
    let connected = terminal["connected"] == true;
    if connected {
        items.push(
            Item::new(
                "mt5.terminal",
                "mt5",
                "mt5.terminal.ok",
                "ok",
                "MT5 terminal ↔ trade server",
            )
            .detail(format!(
                "{} · build {}",
                terminal["company"].as_str().unwrap_or("MetaTrader 5"),
                terminal["build"]
            ))
            .0,
        );
    } else {
        items.push(
            Item::new("mt5.terminal", "mt5", "mt5.terminal.fail", "fail", "MT5 terminal ↔ trade server")
                .detail("The terminal is running but not connected to the RoboForex trade server")
                .fix("Look at the connection indicator in the terminal's bottom-right corner: check the internet, the server name and the password. Repair re-logs in.")
                .action("mt5_reinit")
                .target("broker:roboforex")
                .0,
        );
    }
    match report["account"].as_object() {
        None => items.push(
            Item::new("mt5.account", "mt5", "mt5.account.fail", "fail", "MT5 account")
                .detail("MT5 returns no account information")
                .fix("Log in to the account in the MetaTrader 5 terminal, then press Repair.")
                .action("mt5_reinit")
                .target("broker:roboforex")
                .0,
        ),
        Some(account) if account.get("same_identity") == Some(&json!(false)) => items.push(
            Item::new("mt5.account", "mt5", "mt5.account.changed", "fail", "MT5 account")
                .detail("The terminal is logged in to a different account than AEGIS connected")
                .fix("Reconnect RoboForex in Brokers with the account you want AEGIS to use.")
                .action("reconnect:roboforex")
                .target("broker:roboforex")
                .0,
        ),
        Some(account) => {
            let trading =
                account.get("trade_allowed") == Some(&json!(true)) && account.get("trade_expert") == Some(&json!(true));
            let item = Item::new(
                "mt5.account",
                "mt5",
                if trading {
                    "mt5.account.ok"
                } else {
                    "mt5.account.readonly"
                },
                if trading || !money { "ok" } else { "fail" },
                "MT5 account",
            )
            .detail(format!(
                "{} · {} · {} {:.2}{}",
                account.get("login").map(|v| v.to_string()).unwrap_or_default(),
                account.get("server").and_then(Value::as_str).unwrap_or(""),
                account.get("currency").and_then(Value::as_str).unwrap_or(""),
                account.get("balance").and_then(Value::as_f64).unwrap_or(0.0),
                if trading {
                    ""
                } else {
                    " · trading not allowed (investor password or EAs disabled)"
                }
            ));
            items.push(if trading {
                item.0
            } else {
                item.fix(
                    "Use the trading password, not the investor password, and allow automated trading for the account.",
                )
                .target("broker:roboforex")
                .0
            });
        }
    }
    let algo = terminal["trade_allowed"] == true;
    items.push(
        if algo {
            Item::new("mt5.algo", "mt5", "mt5.algo.ok", "ok", "Algo Trading button").detail("Enabled")
        } else {
            Item::new(
                "mt5.algo",
                "mt5",
                "mt5.algo.off",
                if money { "fail" } else { "warn" },
                "Algo Trading button",
            )
            .detail("Algo Trading is off in the terminal: Paper works, Money mode cannot send orders")
            .fix("Press the Algo Trading button on the MetaTrader 5 toolbar (it turns green).")
            .target("broker:roboforex")
        }
        .0,
    );
    let offset_s = report["offset_seconds"].as_i64().unwrap_or(0);
    match report["quote"].as_object() {
        None => items.push(
            Item::new("mt5.quote", "mt5", "mt5.quote.none", "fail", "XAUUSD quote")
                .detail("MT5 returns no XAUUSD quote")
                .fix("Open XAUUSD in the terminal's Market Watch, then press Repair.")
                .action("mt5_reinit")
                .target("broker:roboforex")
                .0,
        ),
        Some(quote) => {
            let age = quote.get("age_ms").and_then(Value::as_i64).unwrap_or(0);
            let raw = quote.get("raw_ms").and_then(Value::as_i64).unwrap_or(0);
            let item = if age < -MAX_FUTURE_MS {
                // Quote newer than now: the MT5 server offset (or the computer clock) is wrong.
                let suggest = offset_s - round_half_hour_s(age);
                let mut item = Item::new("mt5.quote", "mt5", "mt5.quote.future", "fail", "XAUUSD quote")
                    .param("ahead_s", seconds(-age))
                    .param("offset", offset_s)
                    .param("suggest", suggest)
                    .detail(format!(
                        "The quote is {} s in the future (configured MT5 server offset {offset_s:+} s)",
                        seconds(-age)
                    ))
                    .fix(format!(
                        "If the computer clock is correct, the RoboForex server clock offset is wrong: confirm it and set {suggest:+} s in Brokers → RoboForex → MT5 server clock offset."
                    ))
                    .target("broker:roboforex");
                if suggest != offset_s && suggest.abs() <= 50_400 && -age >= 1_800_000 - 120_000 {
                    item = item.action(format!("set_offset:{suggest}"));
                }
                item
            } else if age > 1_800_000 - 120_000 && connected {
                // Hours "stale" while the terminal is connected: an offset of whole hours is wrong
                // or the market is closed (weekend). Both are reported; neither is guessed silently.
                let suggest = offset_s - round_half_hour_s(age);
                let mut item = Item::new("mt5.quote", "mt5", "mt5.quote.hours", "warn", "XAUUSD quote")
                    .param("age_s", seconds(age))
                    .param("offset", offset_s)
                    .param("suggest", suggest)
                    .detail(format!(
                        "Last XAUUSD tick is {} h old (MT5 server offset {offset_s:+} s)",
                        format_args!("{:.1}", age as f64 / 3_600_000.0)
                    ))
                    .fix(format!(
                        "On a weekend this is normal. On a trading day the MT5 server offset is probably wrong: confirm it with RoboForex and set {suggest:+} s."
                    ))
                    .target("broker:roboforex");
                if suggest != offset_s && suggest.abs() <= 50_400 {
                    item = item.action(format!("set_offset:{suggest}"));
                }
                item
            } else if age > MAX_STALE_MS {
                if connected {
                    Item::new("mt5.quote", "mt5", "mt5.quote.closed", "warn", "XAUUSD quote")
                        .param("age_s", seconds(age))
                        .param("raw_ms", raw)
                        .detail(format!(
                            "No new XAUUSD ticks for {} s while the terminal is connected: the market is closed or paused (daily break about 23:55–01:05 server time, weekend or holiday)",
                            seconds(age)
                        ))
                        .fix("Nothing to repair: AI entries pause and resume automatically when ticks return.")
                } else {
                    Item::new("mt5.quote", "mt5", "mt5.quote.stale", "fail", "XAUUSD quote")
                        .param("age_s", seconds(age))
                        .detail(format!(
                            "The quote is {} s old because the terminal is offline",
                            seconds(age)
                        ))
                        .fix("Restore the terminal's connection to the trade server, then press Repair.")
                        .action("mt5_reinit")
                        .target("broker:roboforex")
                }
            } else {
                Item::new("mt5.quote", "mt5", "mt5.quote.ok", "ok", "XAUUSD quote")
                    .param("age_s", seconds(age.max(0)))
                    .detail(format!(
                        "Fresh: {} s old · bid {} · ask {}",
                        seconds(age.max(0)),
                        quote.get("bid").map(|v| v.to_string()).unwrap_or_default(),
                        quote.get("ask").map(|v| v.to_string()).unwrap_or_default()
                    ))
            };
            items.push(item.0);
        }
    }
    if report["rates_ok"] == true {
        items.push(
            Item::new("mt5.rates", "mt5", "mt5.rates.ok", "ok", "XAUUSD candles")
                .detail("Candle history available")
                .0,
        );
    } else {
        let error = report["rates_error"].as_str().unwrap_or("").to_string();
        items.push(
            Item::new("mt5.rates", "mt5", "mt5.rates.fail", "fail", "XAUUSD candles")
                .param("error", short(&error, 200))
                .detail(format!("copy_rates_from_pos failed {error}"))
                .fix("Repair re-opens the terminal link. Make sure XAUUSD is in Market Watch.")
                .action("mt5_reinit")
                .target("broker:roboforex")
                .0,
        );
    }
}

async fn check_local_ai(app: &AppHandle, deep: bool, items: &mut Vec<HealthItem>) {
    let at = Instant::now();
    let local = app.state::<LocalAiState>();
    let status = crate::local_ai_cmd::status(&local).await;
    let (installed, downloaded, ready, version) = match &status {
        Ok(s) => (
            s.runtime.installed,
            s.runtime.model_downloaded,
            s.runtime.server_ready,
            s.runtime.version.clone(),
        ),
        Err(_) => {
            let s = Runtime::inspect(ENDPOINT).await;
            (s.installed, s.model_downloaded, s.server_ready, s.version)
        }
    };
    let item = if !installed && !ready {
        Item::new("ai.local", "ai", "ai.local.not_installed", "warn", "Local model (Qwen)")
            .detail("The local runtime is not installed; AI trading needs it")
            .fix("Settings → Models → Local model → confirm the download and press Install locally.")
            .target("models:local")
    } else if !ready {
        Item::new("ai.local", "ai", "ai.local.stopped", "fail", "Local model (Qwen)")
            .detail("Installed, but the local model server is not running")
            .fix("Repair starts the server (or press Start server in Settings → Models).")
            .action("start_local_ai")
            .target("models:local")
    } else if !downloaded {
        Item::new("ai.local", "ai", "ai.local.no_model", "fail", "Local model (Qwen)")
            .detail("The server runs but Qwen3 8B is not downloaded")
            .fix("Settings → Models → Local model → Install locally downloads the model (resumable).")
            .target("models:local")
    } else {
        Item::new("ai.local", "ai", "ai.local.ok", "ok", "Local model (Qwen)")
            .param("version", version.clone().unwrap_or_default())
            .detail(format!("Server ready · Ollama {}", version.unwrap_or_default()))
    };
    let ok = item.0.status == "ok";
    items.push(item.latency(at).0);
    if deep && ok {
        let at = Instant::now();
        match crate::local_ai_cmd::local_ai_ping(app.state::<LocalAiState>()).await {
            Ok(answer) => items.push(
                Item::new("ai.local.inference", "ai", "ai.local.inference_ok", "ok", "Local model inference")
                    .param("ms", answer.elapsed_ms)
                    .detail(format!("Answered in {} ms", answer.elapsed_ms))
                    .latency(at)
                    .0,
            ),
            Err(error) if error.contains("Another local AI operation") => items.push(
                Item::new("ai.local.inference", "ai", "ai.local.inference_busy", "ok", "Local model inference")
                    .detail("Busy with an analysis right now; inference is working")
                    .0,
            ),
            Err(error) => items.push(
                Item::new("ai.local.inference", "ai", "ai.local.inference_fail", "fail", "Local model inference")
                    .param("error", short(&error, 240))
                    .detail(format!("Inference failed: {error}"))
                    .fix("Repair restarts the local server. Close other GPU-heavy programs; server.log in the local-ai folder has details.")
                    .action("start_local_ai")
                    .target("models:local")
                    .latency(at)
                    .0,
            ),
        }
    }
}

async fn check_cloud(app: &AppHandle, deep_cloud: bool, items: &mut Vec<HealthItem>) {
    let key = app.state::<AppState>().settings.lock().await.ai_key();
    let model = crate::observer_cmd::selected_model(app).await;
    let at = Instant::now();
    match &key {
        None => items.push(
            Item::new("ai.cloud.key", "ai", "ai.cloud.key_missing", "warn", "OpenRouter key")
                .detail("No OpenRouter API key is saved; AI trading needs the cloud check")
                .fix("Settings → Models → OpenRouter: paste the key and press Save.")
                .target("models:cloud")
                .0,
        ),
        Some(key) => match ai_provider::check_key(key).await {
            Ok(info) => items.push(
                Item::new("ai.cloud.key", "ai", "ai.cloud.key_ok", "ok", "OpenRouter key")
                    .param("label", info.label.clone())
                    .detail(format!(
                        "Accepted{}{}",
                        if info.label.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", info.label)
                        },
                        info.usage.map(|u| format!(" · used ${u:.2}")).unwrap_or_default()
                    ))
                    .latency(at)
                    .0,
            ),
            Err(error) => items.push(
                Item::new("ai.cloud.key", "ai", "ai.cloud.key_fail", "fail", "OpenRouter key")
                    .param("error", short(&error, 200))
                    .detail(error)
                    .fix("Check the key at openrouter.ai/keys and save it again in Settings → Models.")
                    .target("models:cloud")
                    .latency(at)
                    .0,
            ),
        },
    }
    let at = Instant::now();
    match ai_provider::list_models().await {
        Ok(models) => {
            let found = models.iter().find(|m| m.id == model);
            items.push(
                match found {
                    Some(m) if m.json_mode => {
                        Item::new("ai.cloud.model", "ai", "ai.cloud.model_ok", "ok", "OpenRouter model")
                            .param("model", &model)
                            .detail(format!("{} · {}", m.name, model))
                    }
                    Some(_) => Item::new(
                        "ai.cloud.model",
                        "ai",
                        "ai.cloud.model_nojson",
                        "warn",
                        "OpenRouter model",
                    )
                    .param("model", &model)
                    .detail(format!(
                        "{model} does not advertise JSON mode; decisions may be rejected"
                    ))
                    .fix("Choose another Anthropic model in Settings → Models.")
                    .target("models:cloud"),
                    None => Item::new(
                        "ai.cloud.model",
                        "ai",
                        "ai.cloud.model_missing",
                        "fail",
                        "OpenRouter model",
                    )
                    .param("model", &model)
                    .detail(format!("{model} is not offered by OpenRouter any more"))
                    .fix("Choose an available Anthropic model in Settings → Models.")
                    .target("models:cloud"),
                }
                .latency(at)
                .0,
            );
        }
        Err(error) => items.push(
            Item::new(
                "ai.cloud.model",
                "ai",
                "ai.cloud.catalog_fail",
                "warn",
                "OpenRouter model",
            )
            .param("error", short(&error, 200))
            .detail(format!("{model}; catalog unavailable: {error}"))
            .fix("Check the internet connection or a firewall blocking openrouter.ai.")
            .target("models:cloud")
            .latency(at)
            .0,
        ),
    }
    if deep_cloud && key.is_some() {
        let at = Instant::now();
        let result = crate::observer_cmd::observer_provider_test(app.clone(), app.state::<ObserverState>()).await;
        items.push(
            match result {
                Ok(diag) => Item::new(
                    "ai.cloud.test",
                    "ai",
                    "ai.cloud.test_ok",
                    "ok",
                    "OpenRouter answer (billable)",
                )
                .param("ms", diag.latency_ms)
                .detail(diag.detail),
                Err(error) => Item::new(
                    "ai.cloud.test",
                    "ai",
                    "ai.cloud.test_fail",
                    "fail",
                    "OpenRouter answer (billable)",
                )
                .param("error", short(&error, 200))
                .detail(error)
                .fix("Check the OpenRouter balance and the selected model.")
                .target("models:cloud"),
            }
            .latency(at)
            .0,
        );
    }
}

async fn check_observer(app: &AppHandle, items: &mut Vec<HealthItem>) {
    let (config, status, recovery) = crate::observer_cmd::health_snapshot(&app.state::<ObserverState>()).await;
    if let Some(error) = recovery {
        items.push(
            Item::new("observer.recovery", "observer", "observer.recovery", "fail", "AI journal reconciliation")
                .param("error", short(&error, 240))
                .detail(error)
                .fix("Safety lock, not repairable automatically: inspect positions in MetaTrader 5 and the journal; closing positions stays available.")
                .target("trading")
                .0,
        );
    }
    let invalid = config.validate().err();
    let item = if let Some(error) = invalid {
        Item::new(
            "observer.config",
            "observer",
            "observer.config.invalid",
            "fail",
            "AI trading settings",
        )
        .param("error", short(&error, 200))
        .detail(error)
        .fix("Settings → AI trading: correct the highlighted values and save.")
        .target("trading")
    } else if !config.ai_enabled {
        Item::new(
            "observer.config",
            "observer",
            "observer.config.ai_off",
            "warn",
            "AI trading settings",
        )
        .detail("AI strategy analysis is switched off")
        .fix("Settings → AI trading → enable AI strategy analysis and save.")
        .target("trading")
    } else if !config.strategies.iter().any(|s| s.enabled) {
        Item::new(
            "observer.config",
            "observer",
            "observer.config.no_strategy",
            "fail",
            "AI trading settings",
        )
        .detail("No strategy is enabled")
        .fix("Settings → AI trading → enable at least one strategy.")
        .target("trading")
    } else {
        Item::new(
            "observer.config",
            "observer",
            "observer.config.ok",
            "ok",
            "AI trading settings",
        )
        .param("mode", &config.mode)
        .detail(format!(
            "{} · {} strategies",
            if config.mode == "money" { "Money" } else { "Paper" },
            config.strategies.iter().filter(|s| s.enabled).count()
        ))
    };
    items.push(item.0);
    for issue in status.issues.iter().filter(|i| i.source != "recovery") {
        let closed = issue.source == "market" && issue.message.contains("Market closed");
        items.push(
            Item::new(
                &format!("observer.issue.{}", issue.source),
                "observer",
                if closed {
                    "observer.issue.market_closed"
                } else {
                    "observer.issue"
                },
                if closed { "warn" } else { "fail" },
                "AI observer",
            )
            .param("source", &issue.source)
            .param("message", short(&issue.message, 300))
            .detail(format!("{}: {}", issue.source, short(&issue.message, 300)))
            .fix(if closed {
                "Nothing to repair: observation continues and resumes with fresh quotes."
            } else {
                "Repair clears the stale message; a problem that still exists is reported again within seconds."
            })
            .action("clear_issues")
            .target("trading")
            .0,
        );
    }
    items.push(
        Item::new("observer.state", "observer", "observer.state", "ok", "AI observer")
            .param("running", status.running)
            .param("phase", &status.phase)
            .detail(format!(
                "{} · {}",
                if status.running { "running" } else { "stopped" },
                status.phase
            ))
            .0,
    );
}

fn finish(items: Vec<HealthItem>, started: Instant, deep: bool) -> HealthReport {
    let mut counts = BTreeMap::from([("ok", 0), ("warn", 0), ("fail", 0), ("skip", 0)]);
    for item in &items {
        *counts.entry(item.status).or_default() += 1;
    }
    let overall = if counts["fail"] > 0 {
        "fail"
    } else if counts["warn"] > 0 {
        "warn"
    } else {
        "ok"
    };
    let mut actions: Vec<String> = Vec::new();
    for item in items.iter().filter(|i| i.status == "fail" || i.status == "warn") {
        for action in &item.actions {
            // Changing the MT5 offset is a configuration decision; it is never automatic.
            if !action.starts_with("set_offset:") && !actions.contains(action) {
                actions.push(action.clone());
            }
        }
    }
    HealthReport {
        at_ms: clock::now_ms_u64(),
        overall,
        counts,
        items,
        actions,
        deep,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

pub(crate) async fn run_check(app: &AppHandle, deep: bool, cloud: bool) -> HealthReport {
    let _guard = CHECK_LOCK.lock().await;
    let started = Instant::now();
    let mut system = Vec::new();
    let mut brokers = Vec::new();
    let mut local = Vec::new();
    let mut remote = Vec::new();
    // Independent groups run together; each keeps its own order.
    tokio::join!(
        check_clock(&mut system),
        check_brokers(app, &mut brokers),
        check_local_ai(app, deep, &mut local),
        check_cloud(app, deep && cloud, &mut remote),
    );
    let mut observer = Vec::new();
    check_observer(app, &mut observer).await;
    let mut items = system;
    items.extend(brokers);
    items.extend(local);
    items.extend(remote);
    items.extend(observer);
    finish(items, started, deep)
}

/// Full check. `deep` adds a local inference test; `cloud` (with deep) adds one billable
/// OpenRouter request. Nothing here places or changes orders.
#[tauri::command]
pub async fn health_check(app: AppHandle, deep: Option<bool>, cloud: Option<bool>) -> Result<HealthReport, String> {
    Ok(run_check(&app, deep.unwrap_or(true), cloud.unwrap_or(false)).await)
}

// ---- repair -------------------------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct RepairStep {
    pub action: String,
    pub ok: bool,
    pub detail: String,
}

#[derive(Clone, Serialize)]
pub struct RepairReport {
    pub steps: Vec<RepairStep>,
    pub report: HealthReport,
}

fn broker_from(key: &str) -> Option<BrokerId> {
    BrokerId::ALL.into_iter().find(|b| {
        serde_json::to_value(b)
            .ok()
            .and_then(|v| v.as_str().map(|s| s == key))
            .unwrap_or(false)
    })
}

/// Accepts only whitelisted repair actions.
pub(crate) fn valid_action(action: &str) -> bool {
    match action.split_once(':') {
        None => matches!(action, "sync_clock" | "mt5_reinit" | "start_local_ai" | "clear_issues"),
        Some(("reconnect", broker)) => broker_from(broker).is_some(),
        Some(("set_offset", value)) => value
            .parse::<i64>()
            .is_ok_and(|v| (-50_400..=50_400).contains(&v) && v % 1800 == 0),
        _ => false,
    }
}

fn action_rank(action: &str) -> u8 {
    match action.split_once(':').map_or(action, |(a, _)| a) {
        "sync_clock" => 0,
        "set_offset" => 1,
        "reconnect" => 2,
        "mt5_reinit" => 3,
        "start_local_ai" => 4,
        _ => 5,
    }
}

async fn reconnect(app: &AppHandle, broker: BrokerId) -> Result<String, String> {
    let state = app.state::<AppState>();
    let fields = {
        let settings = state.settings.lock().await;
        if !settings.has_credentials(broker) {
            return Err(format!(
                "{} has no complete saved credentials; fill them in Brokers",
                broker.name()
            ));
        }
        settings.credentials(broker)
    };
    let report = state.open(broker, fields).await;
    let value = serde_json::to_value(&report).unwrap_or(Value::Null);
    if report.connected {
        if let Some(session) = state.sessions.lock().await.get(&broker) {
            let _ = session.connector.sync_clock(clock::correction_ms()).await;
        }
        Ok(format!(
            "{} reconnected · {}",
            broker.name(),
            report.account.clone().unwrap_or_default()
        ))
    } else {
        Err(format!("{}: {}", broker.name(), fail_reason(&value)))
    }
}

async fn sync_clock(app: &AppHandle) -> Result<String, String> {
    let check = clock::measure().await;
    let offset = check
        .offset_ms
        .ok_or_else(|| format!("internet time unavailable: {}", check.failures.join("; ")))?;
    let correction = if offset.abs() >= clock::SIGNIFICANT_SKEW_MS {
        offset
    } else {
        0
    };
    clock::set_correction_ms(correction)?;
    let sessions: Vec<_> = app
        .state::<AppState>()
        .sessions
        .lock()
        .await
        .values()
        .map(|s| s.connector.clone())
        .collect();
    for connector in sessions {
        connector.sync_clock(correction).await.map_err(|e| e.to_string())?;
    }
    Ok(if correction == 0 {
        format!(
            "Clock is within {} s of internet time; no correction needed",
            seconds(offset.abs())
        )
    } else {
        format!(
            "AEGIS clock corrected by {} s (Windows clock is {} s {})",
            seconds(correction),
            seconds(correction.abs()),
            if correction < 0 { "ahead" } else { "behind" }
        )
    })
}

async fn run_action(app: &AppHandle, action: &str) -> Result<String, String> {
    match action.split_once(':') {
        None => match action {
            "sync_clock" => sync_clock(app).await,
            "mt5_reinit" => {
                let connector = app
                    .state::<AppState>()
                    .sessions
                    .lock()
                    .await
                    .get(&BrokerId::Roboforex)
                    .map(|s| s.connector.clone());
                match connector {
                    Some(c) if c.alive().await => match c.mt5_reinit().await {
                        Ok(_) => Ok("MT5 terminal link re-opened".into()),
                        // A dead link or another account: a full reconnect is the next step.
                        Err(error) => reconnect(app, BrokerId::Roboforex)
                            .await
                            .map(|done| format!("{done} (re-open failed: {error})")),
                    },
                    _ => reconnect(app, BrokerId::Roboforex).await,
                }
            }
            "start_local_ai" => crate::local_ai_cmd::local_ai_start(app.state::<LocalAiState>())
                .await
                .map(|_| "Local model server started and answered".into()),
            "clear_issues" => {
                crate::observer_cmd::clear_issues(&app.state::<ObserverState>()).await;
                Ok("Stale AI observer messages cleared".into())
            }
            _ => Err("unknown action".into()),
        },
        Some(("reconnect", broker)) => reconnect(app, broker_from(broker).ok_or("unknown broker")?).await,
        Some(("set_offset", value)) => {
            let value: i64 = value.parse().map_err(|_| "invalid offset")?;
            {
                let state = app.state::<AppState>();
                let mut settings = state.settings.lock().await;
                settings.set_plain_field(BrokerId::Roboforex, "server_offset_seconds", Some(&value.to_string()))?;
                settings.save().map_err(|e| e.to_string())?;
            }
            reconnect(app, BrokerId::Roboforex)
                .await
                .map(|done| format!("MT5 server offset set to {value:+} s · {done}"))
        }
        _ => Err("unknown action".into()),
    }
}

pub(crate) async fn run_repair(app: &AppHandle, actions: Option<Vec<String>>) -> Result<RepairReport, String> {
    let _guard = REPAIR_LOCK
        .try_lock()
        .map_err(|_| "Repair is already running".to_string())?;
    let mut actions = match actions {
        Some(list) => list,
        None => run_check(app, false, false).await.actions,
    };
    actions.retain(|a| valid_action(a));
    actions.dedup();
    actions.sort_by_key(|a| action_rank(a));
    // A full reconnect supersedes re-opening the same terminal link.
    if actions
        .iter()
        .any(|a| a == "reconnect:roboforex" || a.starts_with("set_offset:"))
    {
        actions.retain(|a| a != "mt5_reinit");
    }
    if actions.iter().any(|a| a.starts_with("set_offset:")) {
        actions.retain(|a| a != "reconnect:roboforex");
    }
    let mut steps = Vec::new();
    for action in &actions {
        let result = run_action(app, action).await;
        steps.push(RepairStep {
            action: action.clone(),
            ok: result.is_ok(),
            detail: result.unwrap_or_else(|e| e),
        });
    }
    if !actions.is_empty() {
        // Repaired loops re-report anything still wrong; old messages must not linger.
        crate::observer_cmd::clear_issues(&app.state::<ObserverState>()).await;
    }
    let _ = app.emit("health_changed", json!({"reason": "repair", "steps": steps}));
    let report = run_check(app, false, false).await;
    Ok(RepairReport { steps, report })
}

/// Repair: runs the given whitelisted actions, or the safe actions of a fresh full check.
#[tauri::command]
pub async fn health_repair(app: AppHandle, actions: Option<Vec<String>>) -> Result<RepairReport, String> {
    if let Some(list) = &actions {
        if let Some(bad) = list.iter().find(|a| !valid_action(a)) {
            return Err(format!("Action {bad} is not an allowed repair"));
        }
    }
    run_repair(&app, actions).await
}

// ---- AI repair assistant ----------------------------------------------------------------

#[derive(Clone, Serialize)]
pub struct AiCause {
    pub check: String,
    pub explanation: String,
}

#[derive(Clone, Serialize)]
pub struct AiDiagnosis {
    pub engine: String,
    pub model: String,
    pub summary: String,
    pub causes: Vec<AiCause>,
    pub steps: Vec<String>,
    pub actions: Vec<String>,
    pub elapsed_ms: u64,
}

/// Masks account numbers and anything key-like before text leaves the computer.
fn redact(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut digits = String::new();
    let flush = |digits: &mut String, out: &mut String| {
        if digits.len() >= 5 {
            out.push_str("•••");
            out.push_str(&digits[digits.len() - 2..]);
        } else {
            out.push_str(digits);
        }
        digits.clear();
    };
    for ch in text.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else {
            flush(&mut digits, &mut out);
            out.push(ch);
        }
    }
    flush(&mut digits, &mut out);
    out.split_whitespace()
        .map(|word| {
            if word.starts_with("sk-") && word.len() > 12 {
                "[key]"
            } else {
                word
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn language_name(lang: &str) -> &'static str {
    match lang {
        "ru" => "Russian",
        "kk" => "Kazakh",
        _ => "English",
    }
}

pub(crate) fn diagnosis_prompt(report: &HealthReport, log: &[String], cloud: bool) -> (String, Vec<String>) {
    let clean = |text: &str, max: usize| {
        let text = short(text, max);
        if cloud {
            redact(&text)
        } else {
            text
        }
    };
    let problems: Vec<Value> = report
        .items
        .iter()
        .filter(|i| i.status == "fail" || i.status == "warn")
        .take(14)
        .map(|i| {
            json!({"check": i.id, "status": i.status, "title": i.title, "detail": clean(&i.detail, 260),
                   "suggested_fix": clean(&i.fix, 200), "actions": i.actions})
        })
        .collect();
    let healthy: Vec<&str> = report
        .items
        .iter()
        .filter(|i| i.status == "ok")
        .map(|i| i.id.as_str())
        .collect();
    let mut allowed: Vec<String> = Vec::new();
    for item in report.items.iter().filter(|i| i.status == "fail" || i.status == "warn") {
        for action in &item.actions {
            if !allowed.contains(action) {
                allowed.push(action.clone());
            }
        }
    }
    let log: Vec<String> = log.iter().rev().take(12).map(|l| clean(l, 180)).collect();
    let prompt = json!({
        "app": format!("AEGIS {}", env!("CARGO_PKG_VERSION")),
        "os": std::env::consts::OS,
        "problems": problems,
        "healthy_checks": healthy,
        "recent_log": log,
        "allowed_actions": allowed,
        "action_meaning": {
            "sync_clock": "correct the AEGIS clock from internet time",
            "reconnect:<broker>": "log in again with the saved credentials",
            "mt5_reinit": "re-open the link to the running MetaTrader 5 terminal",
            "start_local_ai": "start the local Qwen server",
            "clear_issues": "clear stale AI observer messages",
            "set_offset:<seconds>": "store a new MT5 server clock offset and reconnect"
        }
    })
    .to_string();
    (prompt, allowed)
}

fn system_prompt(lang: &str) -> String {
    format!(
        "You are the AEGIS repair assistant inside a Windows desktop gold-trading terminal. AEGIS reads \
         RoboForex MetaTrader 5 through a local Python bridge, Binance and Bybit through their APIs, runs a local \
         Qwen model through Ollama and checks decisions with an Anthropic model on OpenRouter. You receive a JSON \
         health report. Explain the most likely root cause of each problem in plain words, give short concrete \
         steps a non-programmer can follow, and choose repair actions only from allowed_actions. Facts: a stale \
         quote while the MT5 terminal is connected means the market is closed (daily break about 23:55-01:05 \
         server time, weekend) and needs no repair; IPC errors (-10001..-10005) mean the terminal was closed or \
         restarted; a quote in the future means a wrong MT5 server offset or computer clock. Never suggest \
         placing or closing orders, increasing risk, disabling safety checks, or sharing passwords and keys. \
         Answer in {}. Return one JSON object only: {{\"summary\":string,\"causes\":[{{\"check\":string,\
         \"explanation\":string}}],\"steps\":[string],\"actions\":[string]}}.",
        language_name(lang)
    )
}

/// Summary, causes, steps and whitelisted actions parsed from a model answer.
type Diagnosis = (String, Vec<AiCause>, Vec<String>, Vec<String>);

fn parse_diagnosis(text: &str, allowed: &[String]) -> Result<Diagnosis, String> {
    let start = text.find('{').ok_or("The model returned no JSON")?;
    let end = text.rfind('}').ok_or("The model returned no JSON")?;
    let value: Value =
        serde_json::from_str(&text[start..=end]).map_err(|_| "The model returned invalid JSON".to_string())?;
    let summary = short(value["summary"].as_str().unwrap_or("").trim(), 800);
    if summary.is_empty() {
        return Err("The model returned an empty summary".into());
    }
    let causes = value["causes"]
        .as_array()
        .into_iter()
        .flatten()
        .take(8)
        .filter_map(|c| {
            Some(AiCause {
                check: short(c["check"].as_str()?, 60),
                explanation: short(c["explanation"].as_str()?, 500),
            })
        })
        .collect();
    let steps = value["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .take(8)
        .map(|s| short(s, 300))
        .collect();
    let mut actions = Vec::new();
    for action in value["actions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        if allowed.iter().any(|a| a == action) && valid_action(action) && !actions.iter().any(|a: &String| a == action)
        {
            actions.push(action.to_string());
        }
    }
    Ok((summary, causes, steps, actions))
}

/// Asks the local model or the selected OpenRouter model to explain the full-check result.
/// Proposed actions are filtered to the repair whitelist; nothing is executed here.
#[tauri::command]
pub async fn health_ai_diagnose(
    app: AppHandle,
    engine: String,
    lang: Option<String>,
    log: Option<Vec<String>>,
) -> Result<AiDiagnosis, String> {
    let cloud = match engine.as_str() {
        "local" => false,
        "cloud" => true,
        _ => return Err("engine must be local or cloud".into()),
    };
    let report = run_check(&app, false, false).await;
    let lang = lang.unwrap_or_else(|| "ru".into());
    let (prompt, allowed) = diagnosis_prompt(&report, &log.unwrap_or_default(), cloud);
    let system = system_prompt(&lang);
    let started = Instant::now();
    let (text, model) = if cloud {
        let key = app
            .state::<AppState>()
            .settings
            .lock()
            .await
            .ai_key()
            .ok_or("Save an OpenRouter key in Settings → Models first")?;
        let model = crate::observer_cmd::selected_model(&app).await;
        let answer = ai_provider::consult_model(&key, &model, &prompt, &system, 60).await?;
        (answer.response, model)
    } else {
        let mut action_schema = json!({"type": "string"});
        if !allowed.is_empty() {
            action_schema = json!({"type": "string", "enum": allowed});
        }
        let schema = json!({"type":"object","properties":{
            "summary":{"type":"string","maxLength":700},
            "causes":{"type":"array","maxItems":6,"items":{"type":"object","properties":{
                "check":{"type":"string"},"explanation":{"type":"string","maxLength":400}},"required":["check","explanation"]}},
            "steps":{"type":"array","maxItems":8,"items":{"type":"string","maxLength":300}},
            "actions":{"type":"array","maxItems":6,"items":action_schema}},
            "required":["summary","causes","steps","actions"]});
        let answer = app.state::<LocalAiState>().diagnose(&prompt, &system, &schema).await?;
        (answer.response, aegis_core::local_ai::MODEL.to_string())
    };
    let (summary, causes, steps, actions) = parse_diagnosis(&text, &allowed)?;
    Ok(AiDiagnosis {
        engine,
        model,
        summary,
        causes,
        steps,
        actions,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

// ---- OpenRouter catalog -----------------------------------------------------------------

#[derive(Serialize)]
pub struct ModelCatalog {
    pub selected: String,
    pub models: Vec<ai_provider::ModelInfo>,
    /// Set when the live catalog could not be loaded; `models` then holds known defaults.
    pub error: Option<String>,
}

fn fallback_models() -> Vec<ai_provider::ModelInfo> {
    [
        ("anthropic/claude-fable-5", "Anthropic: Claude Fable 5"),
        ("anthropic/claude-opus-5.5", "Anthropic: Claude Opus 5.5"),
        ("anthropic/claude-sonnet-5.5", "Anthropic: Claude Sonnet 5.5"),
        ("anthropic/claude-haiku-5.5", "Anthropic: Claude Haiku 5.5"),
        ("anthropic/claude-sonnet-4.5", "Anthropic: Claude Sonnet 4.5"),
    ]
    .into_iter()
    .map(|(id, name)| ai_provider::ModelInfo {
        id: id.into(),
        name: name.into(),
        context_length: 0,
        prompt_per_million: None,
        completion_per_million: None,
        json_mode: true,
    })
    .collect()
}

/// Anthropic models offered by OpenRouter right now (free catalog request, no key).
#[tauri::command]
pub async fn provider_models(app: AppHandle) -> Result<ModelCatalog, String> {
    let selected = crate::observer_cmd::selected_model(&app).await;
    Ok(match ai_provider::list_models().await {
        Ok(models) => ModelCatalog {
            selected,
            models,
            error: None,
        },
        Err(error) => ModelCatalog {
            selected,
            models: fallback_models(),
            error: Some(error),
        },
    })
}

// ---- automatic recovery -------------------------------------------------------------------

/// Restarts a dead MT5 bridge with the saved credentials. A broken terminal link inside a
/// live bridge is recovered by the bridge itself; this covers a crashed Python process.
pub async fn run(app: AppHandle) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(15));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut backoff_until = 0u64;
    loop {
        interval.tick().await;
        let connector = app
            .state::<AppState>()
            .sessions
            .lock()
            .await
            .get(&BrokerId::Roboforex)
            .map(|s| s.connector.clone());
        let Some(connector) = connector else { continue };
        if connector.alive().await || clock::now_ms_u64() < backoff_until {
            continue;
        }
        let result = reconnect(&app, BrokerId::Roboforex).await;
        if result.is_err() {
            backoff_until = clock::now_ms_u64() + 60_000;
        }
        let _ = app.emit(
            "health_changed",
            json!({"reason": "auto", "ok": result.is_ok(), "message": result.unwrap_or_else(|e| e)}),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_whitelist_is_strict() {
        for ok in [
            "sync_clock",
            "mt5_reinit",
            "start_local_ai",
            "clear_issues",
            "reconnect:roboforex",
            "reconnect:binance",
            "set_offset:10800",
            "set_offset:-1800",
        ] {
            assert!(valid_action(ok), "{ok}");
        }
        for bad in [
            "rm -rf",
            "reconnect:mt4",
            "set_offset:10801",
            "set_offset:99999",
            "place_order",
            "sync_clock:1",
            "",
        ] {
            assert!(!valid_action(bad), "{bad}");
        }
    }

    #[test]
    fn half_hour_rounding_suggests_broker_offsets() {
        assert_eq!(round_half_hour_s(-10_800_000 + 900), -10_800);
        assert_eq!(round_half_hour_s(7_200_000 - 120_000), 7_200);
        assert_eq!(round_half_hour_s(42_485), 0);
    }

    #[test]
    fn cloud_text_is_redacted() {
        let text = redact("login 68012345 · RoboForex-ECN key sk-or-v1-abcdefghijkl balance 1000");
        assert!(!text.contains("68012345"));
        assert!(text.contains("•••45"));
        assert!(!text.contains("abcdefghijkl"));
        assert!(text.contains("1000"));
    }

    #[test]
    fn diagnosis_parser_filters_actions() {
        let allowed = vec!["mt5_reinit".to_string(), "clear_issues".to_string()];
        let (summary, causes, steps, actions) = parse_diagnosis(
            "```json\n{\"summary\":\"Terminal restarted\",\"causes\":[{\"check\":\"mt5.link\",\"explanation\":\"IPC\"}],\"steps\":[\"Open MT5\"],\"actions\":[\"mt5_reinit\",\"place_order\",\"sync_clock\",\"mt5_reinit\"]}\n```",
            &allowed,
        )
        .unwrap();
        assert_eq!(summary, "Terminal restarted");
        assert_eq!(causes.len(), 1);
        assert_eq!(steps, ["Open MT5"]);
        assert_eq!(actions, ["mt5_reinit"]);
        assert!(parse_diagnosis("no json", &allowed).is_err());
    }

    #[test]
    fn report_collects_safe_actions_only() {
        let item = |status: &'static str, actions: &[&str]| HealthItem {
            id: "x".into(),
            group: "mt5",
            code: "c".into(),
            params: BTreeMap::new(),
            status,
            title: String::new(),
            detail: String::new(),
            fix: String::new(),
            actions: actions.iter().map(|a| a.to_string()).collect(),
            target: None,
            latency_ms: 0,
        };
        let report = finish(
            vec![
                item("fail", &["mt5_reinit", "set_offset:10800"]),
                item("warn", &["sync_clock", "mt5_reinit"]),
                item("ok", &["start_local_ai"]),
            ],
            Instant::now(),
            false,
        );
        assert_eq!(report.overall, "fail");
        assert_eq!(report.actions, ["mt5_reinit", "sync_clock"]);
        let (prompt, allowed) = diagnosis_prompt(&report, &["20:51 Feed: IPC send failed 6801234".into()], true);
        assert!(allowed.contains(&"set_offset:10800".to_string()));
        assert!(!prompt.contains("6801234"));
    }
}
