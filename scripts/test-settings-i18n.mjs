import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

const source = readFileSync(new URL("../src/settingsI18n.ts", import.meta.url), "utf8");
const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 } });
const { resolveLocale, languageName, nativeOcrAvailable } = await import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);

test("explicit UI language overrides system preferences", () => {
  assert.equal(resolveLocale("ja", ["vi-VN", "en-US"]), "ja");
});
test("system language supports regional tags and falls back to English", () => {
  assert.equal(resolveLocale("system", ["VI-vn"]), "vi");
  assert.equal(resolveLocale("system", ["ja-JP"]), "ja");
  assert.equal(resolveLocale("system", ["de-DE", "vi-VN"]), "vi");
  assert.equal(resolveLocale("system", ["de-DE"]), "en");
  assert.equal(resolveLocale("system", []), "en");
});
test("language option labels follow the selected UI locale", () => {
  assert.equal(languageName("ja", "Japanese", "en"), "Japanese");
  assert.equal(languageName("en", "English", "ja"), "英語");
  assert.notEqual(languageName("ja", "Japanese", "vi"), "Japanese");
});
test("native OCR short tags match regional choices without confusing scripts", () => {
  assert.equal(nativeOcrAvailable("ja-JP", ["ja"]), true);
  assert.equal(nativeOcrAvailable("en-US", ["en-US"]), true);
  assert.equal(nativeOcrAvailable("zh-Hant-TW", ["zh-Hans"]), false);
  assert.equal(nativeOcrAvailable("ja-JP", []), false);
});
