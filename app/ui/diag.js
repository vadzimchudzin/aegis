"use strict";
// Full check, Repair, AI repair assistant and the Models tab (OpenRouter + local model).
(() => {
  const A = () => window.AEGIS;
  const invoke = (cmd, args) => A().invoke(cmd, args);
  const esc = (s) => String(s ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);
  const lang = () => (window.I18N?.lang === "ru" || window.I18N?.lang === "kk" ? window.I18N.lang : "en");

  const UI = {
    ru: {
      diagnostics: "Диагностика", intro: "Полная проверка проходит по всем узлам AEGIS: часы компьютера, брокеры, связь с терминалом MT5, котировки, локальная модель, OpenRouter и AI-наблюдатель. Ни одна проверка не отправляет ордера.",
      run: "Полная проверка", running: "Проверяю…", repair: "Починить", repairing: "Чиню…", repair_all: "Починить всё", nothing: "Автоматически чинить нечего",
      paid: "Включить платный тест OpenRouter (1 запрос)", deep_note: "Локальная модель проверяется настоящим ответом.",
      ok: "Норма", warn: "Внимание", fail: "Ошибка", skip: "Не настроено", last: "Последняя проверка", took: "за {s} с",
      all_ok: "Всё работает.", has_fail: "Найдены ошибки: {n}. Причина подсвечена красным.", has_warn: "Есть предупреждения: {n}.",
      show: "Показать", fix_it: "Исправить", fix: "Как исправить", cause: "Причина", detail: "Подробности",
      groups: { system: "Система", brokers: "Брокеры", mt5: "RoboForex MT5", ai: "Модели ИИ", observer: "AI-торговля" },
      steps_done: "Выполнено", step_fail: "Не удалось",
      ai_title: "Починить с помощью ИИ", ai_intro: "ИИ читает отчёт проверки и журнал, объясняет причину простыми словами и предлагает исправления только из безопасного списка. Применяете их вы.",
      engine: "Модель", engine_local: "Локальная Qwen (бесплатно, офлайн)", engine_cloud: "Облако OpenRouter · {model} (платно)",
      ask: "Спросить ИИ", asking: "ИИ анализирует…", ai_summary: "Вывод ИИ", ai_steps: "Что сделать", ai_actions: "Предложенные исправления", apply: "Применить выбранные", no_ai_actions: "ИИ не предлагает автоматических исправлений.",
      confirm_offset: "Изменить смещение часов сервера MT5 на {v} с и переподключить RoboForex? Сделайте это, только если подтвердили смещение у брокера.",
      confirm_paid: "Будет отправлен платный запрос в OpenRouter. Продолжить?",
      actions: { sync_clock: "Исправить часы AEGIS по интернет-времени", mt5_reinit: "Переоткрыть связь с терминалом MT5", start_local_ai: "Запустить сервер локальной модели", clear_issues: "Сбросить устаревшие ошибки", "reconnect:roboforex": "Переподключить RoboForex", "reconnect:binance": "Переподключить Binance", "reconnect:bybit": "Переподключить Bybit", set_offset: "Установить смещение сервера MT5 {v} с" },
      models: "Модели", cloud_title: "Облачная модель · OpenRouter (Anthropic)", cloud_intro: "Облачная модель независимо перепроверяет решения локальной модели и помогает в ремонте. Доступны только модели семейства Anthropic в реальном времени.",
      key: "API-ключ OpenRouter", key_ph: "Оставьте пустым, чтобы сохранить текущий ключ", key_stored: "Ключ сохранён", key_missing: "Ключ не сохранён",
      model: "Модель Anthropic", refresh: "Обновить список", save: "Сохранить", check_key: "Проверить ключ (бесплатно)", test_model: "Тест модели (платно)",
      saved: "Сохранено.", catalog_offline: "Каталог OpenRouter недоступен, показан базовый список: {e}", per_m: "$/1M токенов: вход {p} · выход {c}", nojson: "без JSON-режима",
      local_title: "Локальная модель · Qwen3 8B", stop_first: "Сначала остановите AI-наблюдение.",
      market_closed: "Рынок закрыт или на паузе: новых тиков XAUUSD нет. Торговля возобновится автоматически.",
    },
    en: {
      diagnostics: "Diagnostics", intro: "The full check walks through every AEGIS subsystem: computer clock, brokers, the MT5 terminal link, quotes, the local model, OpenRouter and the AI observer. No check sends orders.",
      run: "Full check", running: "Checking…", repair: "Repair", repairing: "Repairing…", repair_all: "Repair all", nothing: "Nothing to repair automatically",
      paid: "Include the billable OpenRouter test (1 request)", deep_note: "The local model is tested with a real answer.",
      ok: "OK", warn: "Warning", fail: "Error", skip: "Not configured", last: "Last check", took: "in {s} s",
      all_ok: "Everything works.", has_fail: "Errors found: {n}. The cause is highlighted in red.", has_warn: "Warnings: {n}.",
      show: "Show", fix_it: "Fix", fix: "How to fix", cause: "Cause", detail: "Details",
      groups: { system: "System", brokers: "Brokers", mt5: "RoboForex MT5", ai: "AI models", observer: "AI trading" },
      steps_done: "Done", step_fail: "Failed",
      ai_title: "Repair with AI", ai_intro: "The AI reads the check report and the log, explains the cause in plain words and proposes fixes from a safe list only. You apply them.",
      engine: "Model", engine_local: "Local Qwen (free, offline)", engine_cloud: "OpenRouter cloud · {model} (billable)",
      ask: "Ask AI", asking: "AI is analysing…", ai_summary: "AI conclusion", ai_steps: "What to do", ai_actions: "Proposed fixes", apply: "Apply selected", no_ai_actions: "The AI proposes no automatic fixes.",
      confirm_offset: "Set the MT5 server clock offset to {v} s and reconnect RoboForex? Only do this after confirming the offset with your broker.",
      confirm_paid: "This sends a billable OpenRouter request. Continue?",
      actions: { sync_clock: "Correct the AEGIS clock from internet time", mt5_reinit: "Re-open the MT5 terminal link", start_local_ai: "Start the local model server", clear_issues: "Clear stale errors", "reconnect:roboforex": "Reconnect RoboForex", "reconnect:binance": "Reconnect Binance", "reconnect:bybit": "Reconnect Bybit", set_offset: "Set MT5 server offset to {v} s" },
      models: "Models", cloud_title: "Cloud model · OpenRouter (Anthropic)", cloud_intro: "The cloud model independently re-checks the local model's decisions and helps with repairs. Only real-time models of the Anthropic family are available.",
      key: "OpenRouter API key", key_ph: "Leave empty to keep the stored key", key_stored: "Key stored", key_missing: "No key stored",
      model: "Anthropic model", refresh: "Refresh list", save: "Save", check_key: "Check key (free)", test_model: "Test model (billable)",
      saved: "Saved.", catalog_offline: "OpenRouter catalog unavailable, showing the basic list: {e}", per_m: "$/1M tokens: in {p} · out {c}", nojson: "no JSON mode",
      local_title: "Local model · Qwen3 8B", stop_first: "Stop AI observation first.",
      market_closed: "Market closed or paused: no new XAUUSD ticks. Trading resumes automatically.",
    },
  };
  UI.kk = { ...UI.ru, diagnostics: "Диагностика", run: "Толық тексеру", repair: "Жөндеу", models: "Модельдер", groups: { ...UI.ru.groups, brokers: "Брокерлер", ai: "ЖИ модельдері", observer: "ЖИ сауда" } };
  const tr = (key, vars) => {
    let out = UI[lang()][key] ?? UI.en[key] ?? key;
    if (vars && typeof out === "string") out = out.replace(/\{(\w+)\}/g, (m, k) => (k in vars ? String(vars[k]) : m));
    return out;
  };

  // Localized titles and explanations per stable code; backend English text is the fallback.
  const CODES = {
    ru: {
      "system.clock.ok": ["Часы компьютера", "Расхождение с интернет-временем {offset_s} с — норма."],
      "system.clock.corrected": ["Часы компьютера", "AEGIS поправляет часы Windows на {correction_s} с. Синхронизируйте время Windows, чтобы поправка не требовалась."],
      "system.clock.skew": ["Часы компьютера", "Часы компьютера отстают/спешат на {offset_s} с: свежие котировки MT5 выглядят устаревшими или «из будущего», подписи бирж не проходят.", "Параметры Windows → Время и язык → Дата и время → «Синхронизировать». Кнопка «Починить» сразу поправит часы AEGIS."],
      "system.clock.unknown": ["Часы компьютера", "Не удалось измерить интернет-время (нет сети?)."],
      "system.settings.ok": ["Сохранённые настройки", "Зашифрованные настройки читаются."],
      "system.settings.fail": ["Сохранённые настройки", "Сохранённые пароли не расшифровываются на этой учётной записи Windows.", "Заново введите пароли брокеров и ключ OpenRouter."],
      "broker.skip": [null, "Не настроен."],
      "broker.ok": [null, null],
      "broker.fail": [null, "Сохранён, но не подключён: {reason}", "«Починить» переподключит с сохранёнными данными. Если не поможет — проверьте поля в «Брокерах»."],
      "broker.ping_fail": [null, "Подключён, но рыночные данные не приходят: {error}", "«Починить» переподключит сессию. Проверьте интернет и права ключа."],
      "mt5.bridge.ok": ["Мост MT5 (Python)", "Отвечает."],
      "mt5.bridge.dead": ["Мост MT5 (Python)", "Процесс моста к MetaTrader 5 остановился.", "«Починить» перезапустит мост и войдёт с сохранёнными данными RoboForex."],
      "mt5.bridge.error": ["Мост MT5 (Python)", "Мост не отвечает: {error}", "«Починить» перезапустит мост. Если нет Python или пакета MetaTrader5 — установите их (Брокеры → RoboForex)."],
      "mt5.link.ok": ["Связь с терминалом MT5 (IPC)", "Работает."],
      "mt5.link.fail": ["Связь с терминалом MT5 (IPC)", "Связь с терминалом MetaTrader 5 разорвана (IPC send failed): терминал закрыт, перезапущен или компьютер уходил в сон. {error}", "Запустите терминал RoboForex MT5 и войдите в счёт, затем «Починить». AEGIS также сам переподключается каждые несколько секунд."],
      "mt5.terminal.ok": ["Терминал MT5 ↔ торговый сервер", null],
      "mt5.terminal.fail": ["Терминал MT5 ↔ торговый сервер", "Терминал запущен, но не подключён к торговому серверу RoboForex.", "Посмотрите индикатор связи в правом нижнем углу терминала: интернет, сервер, пароль. «Починить» выполнит повторный вход."],
      "mt5.account.ok": ["Счёт MT5", null],
      "mt5.account.readonly": ["Счёт MT5", "Торговля по счёту запрещена (инвесторский пароль или запрет советников).", "Используйте торговый пароль и разрешите автоторговлю для счёта."],
      "mt5.account.fail": ["Счёт MT5", "MT5 не возвращает данные счёта («account or symbol information is unavailable»).", "Войдите в счёт в терминале MetaTrader 5 и нажмите «Починить»."],
      "mt5.account.changed": ["Счёт MT5", "В терминале выполнен вход в другой счёт.", "Переподключите RoboForex в «Брокерах» нужным счётом."],
      "mt5.algo.ok": ["Кнопка «Алго-трейдинг»", "Включена."],
      "mt5.algo.off": ["Кнопка «Алго-трейдинг»", "Алго-трейдинг выключен: Paper работает, денежный режим не сможет отправлять ордера.", "Нажмите «Алго-трейдинг» на панели MetaTrader 5 (станет зелёной)."],
      "mt5.quote.ok": ["Котировка XAUUSD", "Свежая: {age_s} с."],
      "mt5.quote.closed": ["Котировка XAUUSD", "Новых тиков нет {age_s} с, терминал на связи — рынок закрыт или на паузе (ежедневный перерыв ≈23:55–01:05 по серверу, выходные). Это не ошибка.", "Ничего чинить не нужно: входы ИИ на паузе и возобновятся сами."],
      "mt5.quote.stale": ["Котировка XAUUSD", "Котировке {age_s} с, потому что терминал офлайн.", "Восстановите связь терминала с сервером и нажмите «Починить»."],
      "mt5.quote.future": ["Котировка XAUUSD", "Котировка на {ahead_s} с «в будущем» (смещение сервера MT5 {offset} с).", "Если часы компьютера верны — неверно смещение сервера. Подтвердите у брокера и установите {suggest} с."],
      "mt5.quote.hours": ["Котировка XAUUSD", "Последний тик очень старый ({age_s} с) при смещении {offset} с.", "В выходные это нормально. В торговый день, вероятно, неверно смещение сервера MT5: подтвердите и установите {suggest} с."],
      "mt5.quote.none": ["Котировка XAUUSD", "MT5 не отдаёт котировку XAUUSD.", "Добавьте XAUUSD в «Обзор рынка» терминала и нажмите «Починить»."],
      "mt5.rates.ok": ["Свечи XAUUSD", "История доступна."],
      "mt5.rates.fail": ["Свечи XAUUSD", "copy_rates_from_pos не работает {error}", "«Починить» переоткроет связь. Проверьте, что XAUUSD есть в «Обзоре рынка»."],
      "ai.local.ok": ["Локальная модель (Qwen)", "Сервер готов · Ollama {version}."],
      "ai.local.not_installed": ["Локальная модель (Qwen)", "Локальная модель не установлена — она нужна для AI-торговли.", "Настройки → Модели → Локальная модель → подтвердите загрузку и нажмите «Установить локально»."],
      "ai.local.stopped": ["Локальная модель (Qwen)", "Установлена, но сервер модели не запущен.", "«Починить» запустит сервер (или «Запустить сервер» в Настройки → Модели)."],
      "ai.local.no_model": ["Локальная модель (Qwen)", "Сервер работает, но Qwen3 8B не скачана.", "Настройки → Модели → «Установить локально» докачает модель."],
      "ai.local.inference_ok": ["Ответ локальной модели", "Ответила за {ms} мс."],
      "ai.local.inference_busy": ["Ответ локальной модели", "Сейчас занята анализом — значит работает."],
      "ai.local.inference_fail": ["Ответ локальной модели", "Ошибка ответа: {error}", "«Починить» перезапустит сервер. Закройте тяжёлые программы на видеокарте; подробности в server.log."],
      "ai.cloud.key_missing": ["Ключ OpenRouter", "Ключ OpenRouter не сохранён — для AI-торговли нужна облачная перепроверка.", "Настройки → Модели → OpenRouter: вставьте ключ и нажмите «Сохранить»."],
      "ai.cloud.key_ok": ["Ключ OpenRouter", "Принят."],
      "ai.cloud.key_fail": ["Ключ OpenRouter", "{error}", "Проверьте ключ на openrouter.ai/keys и сохраните заново."],
      "ai.cloud.model_ok": ["Модель OpenRouter", "{model} доступна."],
      "ai.cloud.model_nojson": ["Модель OpenRouter", "{model} не поддерживает JSON-режим — решения могут отклоняться.", "Выберите другую модель Anthropic в Настройки → Модели."],
      "ai.cloud.model_missing": ["Модель OpenRouter", "{model} больше не предлагается OpenRouter.", "Выберите доступную модель Anthropic в Настройки → Модели."],
      "ai.cloud.catalog_fail": ["Модель OpenRouter", "Каталог недоступен: {error}", "Проверьте интернет или блокировку openrouter.ai."],
      "ai.cloud.test_ok": ["Ответ OpenRouter (платно)", "Ответ за {ms} мс."],
      "ai.cloud.test_fail": ["Ответ OpenRouter (платно)", "{error}", "Проверьте баланс OpenRouter и выбранную модель."],
      "observer.recovery": ["Сверка журнала ИИ", "{error}", "Защитная блокировка, автоматически не чинится: проверьте позиции в MetaTrader 5 и журнал; закрытие позиций доступно."],
      "observer.config.ok": ["Настройки AI-торговли", null],
      "observer.config.ai_off": ["Настройки AI-торговли", "AI-анализ стратегий выключен.", "Настройки → AI-торговля → включите AI-анализ и сохраните."],
      "observer.config.no_strategy": ["Настройки AI-торговли", "Не включено ни одной стратегии.", "Настройки → AI-торговля → включите хотя бы одну стратегию."],
      "observer.config.invalid": ["Настройки AI-торговли", "{error}", "Исправьте значения в Настройки → AI-торговля и сохраните."],
      "observer.issue": ["AI-наблюдатель", "{source}: {message}", "«Починить» сбросит устаревшее сообщение; если проблема ещё есть, она появится снова через секунды."],
      "observer.issue.market_closed": ["AI-наблюдатель", "Рынок закрыт или на паузе — наблюдение ждёт свежих котировок.", "Ничего чинить не нужно."],
      "observer.state": ["AI-наблюдатель", null],
    },
  };
  CODES.kk = CODES.ru;

  const STATUS_ICON = { ok: "✓", warn: "!", fail: "✕", skip: "–" };
  const D = { report: null, busy: false, ai: null, aiBusy: false, steps: null, paid: false, engine: "local", catalog: null };

  function itemText(item) {
    const table = CODES[lang()];
    const entry = table?.[item.code];
    const fill = (text) => String(text).replace(/\{(\w+)\}/g, (m, k) => (k in item.params ? item.params[k] : m));
    return {
      title: (entry && entry[0]) || item.title,
      text: entry && entry[1] ? fill(entry[1]) : item.detail,
      fix: entry && entry[2] ? fill(entry[2]) : item.fix,
      detail: entry && entry[1] && item.detail && fill(entry[1]) !== item.detail ? item.detail : "",
    };
  }
  function actionLabel(action) {
    const labels = tr("actions");
    if (action.startsWith("set_offset:")) return String(labels.set_offset).replace("{v}", action.split(":")[1]);
    return labels[action] || action;
  }

  // ---- highlight the cause in the window ------------------------------------------------

  function flash(node) {
    if (!node) return;
    node.scrollIntoView({ behavior: "smooth", block: "center" });
    node.classList.remove("hl-cause");
    void node.offsetWidth;
    node.classList.add("hl-cause");
    setTimeout(() => node.classList.remove("hl-cause"), 6000);
  }
  function waitFor(selector, done, tries = 40) {
    const node = document.querySelector(selector);
    if (node) return done(node);
    if (tries > 0) setTimeout(() => waitFor(selector, done, tries - 1), 75);
  }
  function show(target) {
    if (!target) return;
    const [kind, value] = target.split(":");
    if (kind === "broker") {
      A().openPanel("brokers", value);
      waitFor(`.broker-card[data-broker="${value}"]`, flash);
    } else if (kind === "models") {
      A().openSettings("models");
      waitFor(value === "local" ? "[data-models-local]" : "[data-models-cloud]", flash);
    } else if (kind === "trading") {
      A().openSettings("trading");
      waitFor(".obs-status-card", flash);
    } else if (kind === "clock") {
      flash(document.querySelector('[data-item="system.clock"]'));
    }
  }

  // Marks header controls that belong to a failing check so the cause is visible everywhere.
  function markChrome(report) {
    const failing = new Set((report?.items || []).filter((i) => i.status === "fail").map((i) => i.target).filter(Boolean));
    document.querySelectorAll(".broker-dot").forEach((dot) => dot.classList.remove("hl-fail"));
    document.querySelectorAll('.rail-item[data-panel="diagnostics"]').forEach((item) => {
      const fails = report?.counts?.fail || 0;
      item.dataset.badge = fails ? String(fails) : "";
      item.classList.toggle("has-fail", fails > 0);
    });
    const order = A()?.S?.order || [];
    order.forEach((id, index) => {
      if (failing.has(`broker:${id}`)) document.querySelectorAll(".broker-dot")[index]?.classList.add("hl-fail");
    });
  }

  // ---- API used by other panels --------------------------------------------------------

  async function runCheck({ deep = true, cloud = false } = {}) {
    if (D.busy) return D.report;
    D.busy = true;
    rerender();
    try {
      D.report = await invoke("health_check", { deep, cloud });
      A().S.health = D.report;
      markChrome(D.report);
      const fails = D.report.items.filter((i) => i.status === "fail");
      A().log(`${tr("run")}: ${fails.length ? fails.map((i) => itemText(i).title).join(", ") : tr("all_ok")}`, fails.length ? "bad" : D.report.counts.warn ? "warn" : "ok");
      return D.report;
    } catch (error) {
      A().log(`${tr("run")}: ${error}`, "bad");
      throw error;
    } finally {
      D.busy = false;
      rerender();
    }
  }

  async function repair(actions) {
    if (D.busy) return null;
    D.busy = true;
    rerender();
    try {
      const result = await invoke("health_repair", actions ? { actions } : {});
      D.steps = result.steps;
      D.report = result.report;
      A().S.health = D.report;
      markChrome(D.report);
      if (!result.steps.length) A().log(tr("nothing"), "warn");
      result.steps.forEach((step) => A().log(`${tr("repair")} · ${actionLabel(step.action)}: ${step.detail}`, step.ok ? "ok" : "bad"));
      await A().afterRepair?.();
      return result;
    } catch (error) {
      A().log(`${tr("repair")}: ${error}`, "bad");
      throw error;
    } finally {
      D.busy = false;
      rerender();
    }
  }

  // In-app confirmation (native dialogs are not reliable inside every WebView).
  function confirmBox(message) {
    return new Promise((resolve) => {
      const shade = document.createElement("div");
      shade.className = "dg-confirm";
      shade.innerHTML = `<div class="dg-confirm-box glass" role="alertdialog" aria-modal="true"><p>${esc(message)}</p><div class="obs-actions"><button type="button" class="cta" data-confirm="yes">${esc(lang() === "en" ? "Continue" : "Продолжить")}</button><button type="button" class="ghost" data-confirm="no">${esc(lang() === "en" ? "Cancel" : "Отмена")}</button></div></div>`;
      const done = (value) => { shade.remove(); resolve(value); };
      shade.addEventListener("click", (event) => {
        const button = event.target.closest("[data-confirm]");
        if (button) done(button.dataset.confirm === "yes");
        else if (event.target === shade) done(false);
      });
      document.body.append(shade);
      shade.querySelector('[data-confirm="yes"]').focus();
    });
  }

  async function confirmActions(actions) {
    for (const action of actions) {
      if (action.startsWith("set_offset:") && !(await confirmBox(tr("confirm_offset", { v: action.split(":")[1] })))) return false;
    }
    return true;
  }

  // ---- diagnostics tab -----------------------------------------------------------------

  let mounted = null;
  function rerender() {
    if (mounted && mounted.isConnected) paint(mounted);
    window.AEGIS?.observer?.paintQuickHealth?.();
  }

  function summaryLine(report) {
    if (!report) return "";
    const fail = report.counts.fail || 0;
    const warn = report.counts.warn || 0;
    const text = fail ? tr("has_fail", { n: fail }) : warn ? tr("has_warn", { n: warn }) : tr("all_ok");
    return `<div class="dg-summary dg-${esc(report.overall)}"><strong>${esc(text)}</strong><span>${esc(tr("last"))}: ${esc(new Date(report.at_ms).toLocaleTimeString())} · ${esc(tr("took", { s: (report.elapsed_ms / 1000).toFixed(1) }))}</span>
      <span class="dg-counts">${["fail", "warn", "ok", "skip"].map((k) => `<b class="dg-pill dg-${k}">${STATUS_ICON[k]} ${report.counts[k] || 0}</b>`).join("")}</span></div>`;
  }

  function itemHtml(item) {
    const text = itemText(item);
    const actions = item.status === "fail" || item.status === "warn" ? item.actions : [];
    return `<article class="dg-item dg-${esc(item.status)}" data-item="${esc(item.id)}">
      <span class="dg-ico">${STATUS_ICON[item.status] || ""}</span>
      <div class="dg-body"><div class="dg-head"><strong>${esc(text.title)}</strong><span class="dg-state">${esc(tr(item.status))}${item.latency_ms ? ` · ${esc(item.latency_ms)} ms` : ""}</span></div>
        ${text.text ? `<p class="dg-text">${esc(text.text)}</p>` : ""}
        ${text.detail ? `<p class="dg-detail">${esc(text.detail)}</p>` : ""}
        ${text.fix && item.status !== "ok" && item.status !== "skip" ? `<p class="dg-fix"><b>${esc(tr("fix"))}:</b> ${esc(text.fix)}</p>` : ""}
        ${actions.length || (item.target && item.status !== "ok") ? `<div class="dg-actions">${actions.map((a) => `<button type="button" class="ghost sm" data-dg-action="${esc(a)}">${esc(tr("fix_it"))}: ${esc(actionLabel(a))}</button>`).join("")}${item.target && item.status !== "ok" ? `<button type="button" class="ghost sm" data-dg-show="${esc(item.target)}">${esc(tr("show"))}</button>` : ""}</div>` : ""}
      </div></article>`;
  }

  function aiHtml() {
    const model = D.catalog?.selected || "anthropic/claude-fable-5";
    const ai = D.ai;
    let result = "";
    if (ai && ai.error) result = `<p class="dg-error" role="alert">${esc(ai.error)}</p>`;
    else if (ai) {
      result = `<div class="dg-ai-result"><h4>${esc(tr("ai_summary"))} <small>${esc(ai.model)} · ${esc((ai.elapsed_ms / 1000).toFixed(1))} s</small></h4><p>${esc(ai.summary)}</p>
        ${ai.causes.length ? `<ul class="dg-causes">${ai.causes.map((c) => `<li><b>${esc(c.check)}</b> — ${esc(c.explanation)}</li>`).join("")}</ul>` : ""}
        ${ai.steps.length ? `<h4>${esc(tr("ai_steps"))}</h4><ol>${ai.steps.map((s) => `<li>${esc(s)}</li>`).join("")}</ol>` : ""}
        <h4>${esc(tr("ai_actions"))}</h4>${ai.actions.length ? `<div class="dg-ai-actions">${ai.actions.map((a) => `<label class="obs-toggle"><input type="checkbox" data-ai-action="${esc(a)}" ${a.startsWith("set_offset:") ? "" : "checked"}><span>${esc(actionLabel(a))}</span></label>`).join("")}</div>
        <button type="button" class="cta sm" data-dg="apply-ai" ${D.busy ? "disabled" : ""}>${esc(tr("apply"))}</button>` : `<p class="obs-muted">${esc(tr("no_ai_actions"))}</p>`}</div>`;
    }
    return `<section class="obs-card dg-ai" data-dg-ai><h3>${esc(tr("ai_title"))}</h3><p class="obs-help">${esc(tr("ai_intro"))}</p>
      <div class="dg-row"><label class="obs-label">${esc(tr("engine"))}<select data-dg-engine><option value="local" ${D.engine === "local" ? "selected" : ""}>${esc(tr("engine_local"))}</option><option value="cloud" ${D.engine === "cloud" ? "selected" : ""}>${esc(tr("engine_cloud", { model }))}</option></select></label>
      <button type="button" class="ghost" data-dg="ask" ${D.aiBusy ? "disabled" : ""}>${esc(D.aiBusy ? tr("asking") : tr("ask"))}</button></div>${result}</section>`;
  }

  function paint(root) {
    const report = D.report;
    const groups = ["system", "brokers", "mt5", "ai", "observer"];
    const steps = D.steps?.length ? `<div class="dg-steps">${D.steps.map((s) => `<div class="dg-step ${s.ok ? "dg-ok" : "dg-fail"}"><b>${esc(s.ok ? tr("steps_done") : tr("step_fail"))}</b> ${esc(actionLabel(s.action))} — ${esc(s.detail)}</div>`).join("")}</div>` : "";
    const scroll = root.closest("#panelBody")?.scrollTop;
    root.innerHTML = `<div class="dg">
      <div class="obs-heading"><div><span class="obs-kicker">AEGIS</span><h2>${esc(tr("diagnostics"))}</h2><p>${esc(tr("intro"))}</p></div></div>
      <section class="obs-card dg-controls"><div class="dg-row">
        <button type="button" class="cta" data-dg="run" ${D.busy ? "disabled" : ""}>${esc(D.busy ? tr("running") : tr("run"))}</button>
        <button type="button" class="ghost dg-repair" data-dg="repair" ${D.busy || (report && !report.actions.length) ? "disabled" : ""}>${esc(D.busy ? tr("repairing") : report && !report.actions.length ? tr("nothing") : tr("repair_all"))}</button>
        <label class="obs-toggle dg-paid"><input type="checkbox" data-dg-paid ${D.paid ? "checked" : ""}><span>${esc(tr("paid"))}</span></label></div>
        <p class="obs-help">${esc(tr("deep_note"))}</p>${summaryLine(report)}${steps}</section>
      ${report ? groups.map((g) => {
        const items = report.items.filter((i) => i.group === g);
        if (!items.length) return "";
        const worst = items.some((i) => i.status === "fail") ? "fail" : items.some((i) => i.status === "warn") ? "warn" : "ok";
        return `<section class="obs-card dg-group dg-group-${worst}"><h3>${esc(tr("groups")[g] || g)}</h3>${items.map(itemHtml).join("")}</section>`;
      }).join("") : ""}
      ${aiHtml()}</div>`;
    if (scroll != null) root.closest("#panelBody").scrollTop = scroll;
  }

  function bind(root) {
    root.addEventListener("click", async (event) => {
      const button = event.target.closest("button");
      if (!button || button.disabled) return;
      try {
        if (button.dataset.dg === "run") {
          if (D.paid && !(await confirmBox(tr("confirm_paid")))) return;
          D.steps = null;
          await runCheck({ deep: true, cloud: D.paid });
        } else if (button.dataset.dg === "repair") {
          await repair(null);
        } else if (button.dataset.dgAction) {
          const action = button.dataset.dgAction;
          if (!(await confirmActions([action]))) return;
          await repair([action]);
        } else if (button.dataset.dgShow) {
          show(button.dataset.dgShow);
        } else if (button.dataset.dg === "ask") {
          if (D.engine === "cloud" && !(await confirmBox(tr("confirm_paid")))) return;
          D.aiBusy = true;
          D.ai = null;
          rerender();
          try {
            const log = [...document.querySelectorAll("#log .log-row")].slice(0, 15).map((row) => row.textContent.trim());
            D.ai = await invoke("health_ai_diagnose", { engine: D.engine, lang: lang(), log });
          } catch (error) {
            D.ai = { error: String(error) };
          } finally {
            D.aiBusy = false;
            rerender();
          }
        } else if (button.dataset.dg === "apply-ai") {
          const actions = [...root.querySelectorAll("[data-ai-action]:checked")].map((n) => n.dataset.aiAction);
          if (!actions.length || !(await confirmActions(actions))) return;
          await repair(actions);
        }
      } catch {
        /* errors are logged */
      }
    });
    root.addEventListener("change", (event) => {
      if (event.target.matches("[data-dg-paid]")) D.paid = event.target.checked;
      if (event.target.matches("[data-dg-engine]")) D.engine = event.target.value;
    });
  }

  function render(container) {
    const root = document.createElement("div");
    root.className = "dg-root";
    root.dataset.testid = "diagnostics";
    container.replaceChildren(root);
    mounted = root;
    bind(root);
    paint(root);
    if (!D.catalog) loadCatalog().then(rerender).catch(() => {});
    if (!D.report && !D.busy) runCheck({ deep: false }).catch(() => {});
  }

  // ---- models tab ------------------------------------------------------------------------

  async function loadCatalog() {
    D.catalog = await invoke("provider_models");
    return D.catalog;
  }
  function price(value) {
    return value == null ? "—" : Number(value).toFixed(value < 1 ? 2 : 1);
  }
  function modelOptions(catalog) {
    const list = catalog?.models || [];
    const selected = catalog?.selected;
    const known = list.some((m) => m.id === selected);
    const options = list.map((m) => `<option value="${esc(m.id)}" ${m.id === selected ? "selected" : ""}>${esc(m.name.replace(/^Anthropic:\s*/, ""))} — ${esc(m.id)}${m.json_mode ? "" : ` (${esc(tr("nojson"))})`}</option>`);
    if (selected && !known) options.unshift(`<option value="${esc(selected)}" selected>${esc(selected)}</option>`);
    return options.join("");
  }
  function modelInfo(root) {
    const id = root.querySelector("[data-model-select]")?.value;
    const model = D.catalog?.models?.find((m) => m.id === id);
    const node = root.querySelector("[data-model-info]");
    if (!node) return;
    node.textContent = model ? `${tr("per_m", { p: price(model.prompt_per_million), c: price(model.completion_per_million) })}${model.context_length ? ` · ${Math.round(model.context_length / 1000)}k ctx` : ""}` : "";
  }

  function renderModels(container) {
    container.innerHTML = `<div class="dg-models">
      <section class="obs-card" data-models-cloud><h3>${esc(tr("cloud_title"))}</h3><p class="obs-help">${esc(tr("cloud_intro"))}</p>
        <label class="obs-label">${esc(tr("key"))}<input type="password" autocomplete="new-password" data-model-key placeholder="${esc(tr("key_ph"))}"></label>
        <label class="obs-label">${esc(tr("model"))}<select data-model-select><option>…</option></select></label>
        <p class="obs-help" data-model-info></p><p class="obs-help dg-warn-text" data-model-offline hidden></p>
        <div class="obs-actions"><button type="button" class="cta" data-model="save">${esc(tr("save"))}</button><button type="button" class="ghost" data-model="refresh">${esc(tr("refresh"))}</button><button type="button" class="ghost" data-model="check">${esc(tr("check_key"))}</button><button type="button" class="ghost" data-model="test">${esc(tr("test_model"))}</button><span data-model-key-state></span></div>
        <div class="obs-inline-error" data-model-message role="status"></div></section>
      <section class="obs-card settings-local-ai" data-models-local><h3>${esc(tr("local_title"))}</h3><div data-local-ai></div></section></div>`;
    window.AEGIS.localAI?.render(container.querySelector("[data-local-ai]"));
    const message = container.querySelector("[data-model-message]");
    const fill = async (refresh) => {
      const select = container.querySelector("[data-model-select]");
      try {
        if (refresh || !D.catalog) await loadCatalog();
        select.innerHTML = modelOptions(D.catalog);
        const offline = container.querySelector("[data-model-offline]");
        offline.hidden = !D.catalog.error;
        offline.textContent = D.catalog.error ? tr("catalog_offline", { e: D.catalog.error }) : "";
        modelInfo(container);
      } catch (error) {
        message.textContent = String(error);
      }
    };
    const keyState = async () => {
      try {
        const info = await invoke("observer_info");
        container.querySelector("[data-model-key-state]").textContent = info.provider_key_stored ? tr("key_stored") : tr("key_missing");
      } catch { /* status is optional */ }
    };
    fill(false);
    keyState();
    container.querySelector("[data-model-select]").addEventListener("change", () => modelInfo(container));
    container.addEventListener("click", async (event) => {
      const button = event.target.closest("[data-model]");
      if (!button || button.disabled) return;
      button.disabled = true;
      message.textContent = "…";
      try {
        if (button.dataset.model === "refresh") {
          await fill(true);
          message.textContent = "";
        } else if (button.dataset.model === "save") {
          const key = container.querySelector("[data-model-key]").value.trim();
          const model = container.querySelector("[data-model-select]").value;
          await invoke("observer_provider_save", { key, model });
          container.querySelector("[data-model-key]").value = "";
          if (D.catalog) D.catalog.selected = model;
          message.textContent = tr("saved");
          await keyState();
          A().log(`OpenRouter · ${model}`, "ok");
        } else if (button.dataset.model === "check") {
          const report = await invoke("health_check", { deep: false, cloud: false });
          const item = report.items.find((i) => i.id === "ai.cloud.key");
          const model = report.items.find((i) => i.id === "ai.cloud.model");
          message.textContent = [item, model].filter(Boolean).map((i) => `${STATUS_ICON[i.status]} ${itemText(i).title}: ${itemText(i).text}`).join(" · ");
        } else if (button.dataset.model === "test") {
          if (!(await confirmBox(tr("confirm_paid")))) { message.textContent = ""; return; }
          const result = await invoke("observer_provider_test");
          message.textContent = `${result.ok ? "✓" : "!"} ${result.detail} · ${result.latency_ms} ms`;
        }
      } catch (error) {
        message.textContent = String(error?.message || error);
      } finally {
        button.disabled = false;
      }
    });
  }

  window.AEGIS = window.AEGIS || {};
  window.AEGIS.diag = { render, renderModels, runCheck, repair, show, tr, itemText, actionLabel, state: D, markChrome, confirmBox };
})();
