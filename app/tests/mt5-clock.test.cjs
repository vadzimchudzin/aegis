const {test} = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

function fixture(value = '') {
  const window = {__TAURI__: {core: {invoke() {}}}, I18N: {t: s => s}};
  let source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  source = source.replace(/  boot\(\)\.catch\([^\n]+\);/, '  window.clockTest = { S, cardHtml };');
  // Isolate the real card renderer without constructing the chart or booting Tauri.
  source = source.slice(0, source.indexOf("  const chart ="))
    + source.slice(source.indexOf("  function brokerState("), source.indexOf("  function renderChartChrome("))
    + source.slice(source.indexOf("  const ICON ="), source.indexOf("  function bindCard("))
    + "window.clockTest = { S, cardHtml }; })();";
  vm.runInNewContext(source, {window, document: {}, console});
  const api = window.clockTest;
  api.S.infos.roboforex = {name: 'RoboForex', venue: 'MT5', symbol: 'XAUUSD', icon: '', keys_url: '', requirements: [],
    fields: [{key: 'server_offset_seconds', label: 'MT5 server clock offset', optional: true, secret: false, hint: 'Confirm with broker and reconnect.'}]};
  api.S.settings = {brokers: {roboforex: {stored: [], values: {server_offset_seconds: value}}}};
  return api.cardHtml('roboforex');
}

test('MT5 clock selector defaults to environment/UTC, never silently to +3h', () => {
  const html = fixture();
  assert.match(html, /<select[^>]+data-key="server_offset_seconds"/);
  assert.match(html, /value="" selected>Environment \/ UTC default \(not auto-detected\)/);
  assert.doesNotMatch(html, /value="10800" selected/);
  for (const seconds of ['-50400', '0', '10800', '50400']) assert.ok(html.includes(`value="${seconds}"`));
});
test('MT5 clock selector restores explicit UTC and custom signed seconds', () => {
  for (const value of ['0', '10800', '-18000', '3', '+10800']) {
    assert.ok(fixture(value).includes(`value="${value}" selected`), value);
  }
});
test('broker form collection and draft preservation include select controls', () => {
  const source = fs.readFileSync(path.join(__dirname, '../ui/app.js'), 'utf8');
  assert.ok(source.includes('card.querySelectorAll("[data-key]").forEach((i) => (form[i.dataset.key] = i.value.trim()))'));
  assert.ok(source.includes('values: [...card.querySelectorAll("[data-key]")]'));
});
test('Russian and Kazakh clock guidance is translated', () => {
  const window = {};
  vm.runInNewContext(fs.readFileSync(path.join(__dirname, '../ui/i18n-dict.js'), 'utf8'), {window});
  for (const language of ['ru', 'kk']) {
    const dictionary = window.I18N_DICT[language];
    for (const key of ['MT5 server clock offset', 'Raw quote timestamp (UTC)', 'Computer UTC time', 'Normalized quote age', 'MT5 clock guidance', 'Environment / UTC default (not auto-detected)']) {
      assert.ok(dictionary[key], `${language}: ${key}`);
    }
    assert.ok(Object.keys(dictionary).some(key => key.includes('UTC+5') && key.includes('reconnect')));
  }
});
