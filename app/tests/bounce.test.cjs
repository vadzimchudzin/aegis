const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { test } = require("node:test");

function fixture() {
  const lines = [];
  const removed = [];
  const requests = [];
  const S = { chart: { broker: "roboforex", tf: "15m", generation: 1 }, bars: [{ close: 100 }] };
  const window = { I18N: { t: (s) => s }, AEGIS: {
    S, log() {}, esc: String,
    invoke: () => new Promise((resolve, reject) => requests.push({ resolve, reject })),
    candles: {
      createPriceLine(options) { lines.push(options); return options; },
      removePriceLine(line) { removed.push(line); },
      priceToCoordinate: (price) => price * 10,
    },
  } };
  const source = fs.readFileSync(path.join(__dirname, "../ui/bounce.js"), "utf8").replace(
    "A.bounce = { render, leaveBacktestView, onLiveChart };",
    "B.info = {}; B.params = { min_prob: 0.4 }; A.bounce = { refreshLive, onLiveChart };"
  );
  vm.runInNewContext(source, {
    window, document: { addEventListener() {} },
    getComputedStyle: () => ({ getPropertyValue: () => "#ffffff" }),
    setTimeout: () => 1, clearTimeout() {}, console,
  });
  return { api: window.AEGIS.bounce, S, lines, removed, requests };
}
const result = (entries) => ({ fresh: true, live: { price: 100, entries } });
const entry = (price) => ({ entry: price, dir: 1, ok: true, prob: 0.5 });

test("dense bounce levels retain lines without stacking axis labels", async () => {
  const f = fixture();
  const pending = f.api.refreshLive();
  f.requests[0].resolve(result(Array.from({ length: 30 }, (_, i) => entry(100 + i / 10))));
  await pending;
  assert.equal(f.lines.length, 30);
  assert.equal(f.lines.filter(x => x.axisLabelVisible).length, 1);
  assert.ok(f.lines.filter(x => !x.axisLabelVisible).every(x => x.title === ""));
});

test("sparse bounce levels label at most four nearest prices", async () => {
  const f = fixture();
  const pending = f.api.refreshLive();
  f.requests[0].resolve(result(Array.from({ length: 12 }, (_, i) => entry(100 + i * 5))));
  await pending;
  assert.equal(f.lines.filter(x => x.axisLabelVisible).length, 4);
  assert.deepEqual(f.lines.filter(x => x.axisLabelVisible).map(x => x.price), [100, 105, 110, 115]);
});

test("chart switch discards an outstanding bounce response", async () => {
  const f = fixture();
  const pending = f.api.refreshLive();
  f.S.chart.tf = "4h";
  f.api.onLiveChart();
  f.requests[0].resolve(result([entry(100)]));
  await pending;
  assert.equal(f.lines.length, 0);
});

test("a slower old request cannot overwrite newer overlay levels", async () => {
  const f = fixture();
  const old = f.api.refreshLive();
  const recent = f.api.refreshLive();
  f.requests[1].resolve(result([entry(101)]));
  await recent;
  f.requests[0].resolve(result([entry(99)]));
  await old;
  assert.deepEqual(f.lines.map(x => x.price), [101]);
  assert.equal(f.removed.length, 0);
});

test("failed overlay refresh removes obsolete expected entries", async () => {
  const f = fixture();
  const first = f.api.refreshLive();
  f.requests[0].resolve(result([entry(100)]));
  await first;
  const next = f.api.refreshLive();
  f.requests[1].reject(new Error("feed unavailable"));
  await next;
  assert.equal(f.removed.length, 1);
});

test("invalid prices and probabilities never reach the chart", async () => {
  const f = fixture();
  const pending = f.api.refreshLive();
  f.requests[0].resolve(result([entry(NaN), { ...entry(100), prob: Infinity }, entry(101)]));
  await pending;
  assert.deepEqual(f.lines.map(x => x.price), [101]);
});

test("archive fallback never appears as current expected entries", async () => {
  const f = fixture();
  const pending = f.api.refreshLive();
  f.requests[0].resolve({ ...result([entry(100)]), fresh: false, note: "using archive" });
  await pending;
  assert.equal(f.lines.length, 0);
});
