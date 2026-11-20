"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { test } = require("node:test");

const UI = path.join(__dirname, "..", "ui");
const read = (file) => fs.readFileSync(path.join(UI, file), "utf8");

/**
 * Translations live in four stores keyed three different ways: the shared dictionary keyed by
 * English source text, the local model panel's own copy of it, and the densities word table keyed
 * by identifier. A new string added to only one of them is invisible until someone switches the
 * language, so every store is checked here.
 */
function sliceObject(source, startIndex) {
  let depth = 0;
  let quote = null;
  for (let i = startIndex; i < source.length; i += 1) {
    const ch = source[i];
    if (quote) {
      if (ch === "\\") i += 1;
      else if (ch === quote) quote = null;
      continue;
    }
    if (ch === '"' || ch === "'" || ch === "`") quote = ch;
    else if (ch === "{") depth += 1;
    else if (ch === "}" && (depth -= 1) === 0) return source.slice(startIndex, i + 1);
  }
  throw new Error("unbalanced object literal");
}

function objectLiteral(file, marker) {
  const source = read(file);
  const at = source.indexOf(marker);
  assert.notEqual(at, -1, `${file}: ${marker} not found`);
  const brace = source.indexOf("{", at + marker.length - 1);
  return vm.runInNewContext(`(${sliceObject(source, brace)})`);
}

function sharedDictionary(structural = true) {
  const window = {};
  vm.runInNewContext(read("i18n-dict.js"), { window });
  if (structural) vm.runInNewContext(read("structural-i18n.js"), { window });
  return window.I18N_DICT;
}

function stringsPassedTo(file, name = "t") {
  const pattern = new RegExp(String.raw`(?:^|[^\w.$])${name}\(\s*("(?:[^"\\\n]|\\.)*")`, "g");
  const found = new Set();
  let match;
  while ((match = pattern.exec(read(file)))) found.add(JSON.parse(match[1]));
  return [...found];
}

function keys(table) {
  return Object.keys(table || {}).sort();
}

function assertParity(name, table, langs) {
  const base = keys(table[langs[0]]);
  for (const lang of langs.slice(1)) {
    assert.deepEqual(keys(table[lang]), base, `${name}.${lang} has a different key set`);
  }
  for (const lang of langs) {
    const blank = Object.entries(table[lang]).filter(([, value]) => !String(value).trim());
    assert.deepEqual(blank, [], `${name}.${lang} has blank translations`);
  }
}

test("the shared dictionary keeps Russian and Kazakh key sets identical", () => {
  assertParity("I18N_DICT", sharedDictionary(), ["ru", "kk"]);
});

test("the local model panel keeps Russian and Kazakh key sets identical", () => {
  assertParity("local-ai", objectLiteral("local-ai.js", "const DICT = "), ["ru", "kk"]);
});

test("the densities word table keeps English, Russian and Kazakh key sets identical", () => {
  assertParity("densities", objectLiteral("densities.js", "const words = "), ["en", "ru", "kk"]);
});

test("structural strategy rows are complete triples", () => {
  const window = { I18N_DICT: { ru: {}, kk: {} } };
  vm.runInNewContext(read("structural-i18n.js"), { window });
  assert.deepEqual(
    keys(window.I18N_DICT.ru),
    keys(window.I18N_DICT.kk),
    "a structural row is missing its Kazakh column"
  );
  assert.ok(keys(window.I18N_DICT.ru).length > 0, "structural translations did not load");
});

test("structural strategy rows do not silently override another translation", () => {
  const before = sharedDictionary(false);
  const after = sharedDictionary(true);
  const changed = Object.keys(after.ru).filter((key) => before.ru[key] && before.ru[key] !== after.ru[key]);
  assert.deepEqual(changed, [], "structural-i18n.js overrides existing translations");
  assert.deepEqual(keys(after.ru), keys(after.kk), "the overlay left one language behind");
});

test("every string handed to the translator has a translation", () => {
  const dict = sharedDictionary();
  const stores = {
    "app.js": dict,
    "bounce.js": dict,
    "structural.js": dict,
    "local-ai.js": objectLiteral("local-ai.js", "const DICT = "),
    "densities.js": objectLiteral("densities.js", "const words = "),
  };
  const missing = [];
  for (const [file, table] of Object.entries(stores)) {
    for (const text of stringsPassedTo(file)) {
      for (const lang of ["ru", "kk"]) {
        if (!(text in table[lang])) missing.push(`${file} ${lang} ${JSON.stringify(text)}`);
      }
    }
  }
  assert.deepEqual(missing, [], "untranslated strings reach the interface");
});

test("the extracted string list matches the shipped dictionary", () => {
  const dict = sharedDictionary();
  const list = JSON.parse(read("i18n-strings.json"));
  assert.ok(Array.isArray(list) && list.length > 0, "i18n-strings.json is empty");
  const missing = list.filter((text) => !(text in dict.ru) || !(text in dict.kk));
  assert.deepEqual(missing, [], "i18n-strings.json lists strings the dictionary no longer carries");
});
