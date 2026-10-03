import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import ts from "typescript";

async function pipeline(blocks, translations) {
  const source = readFileSync(new URL("../src/translation.ts", import.meta.url), "utf8");
  const mock = `export const calls = [];
    async function invoke(command, args) {
      calls.push({ command, args });
      if (command === "recognize_capture") return ${JSON.stringify({ width: 700, height: 280, blocks })};
      if (command === "translate_texts") return ${JSON.stringify(translations)};
      throw new Error(command);
    }`;
  const { outputText } = ts.transpileModule(source.replace('import { invoke } from "@tauri-apps/api/core";', mock), {
    compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 },
  });
  return import(`data:text/javascript;base64,${Buffer.from(outputText).toString("base64")}`);
}

test("metadata and code are preserved while titles and body reach the translator", async () => {
  const blocks = [
    { kind: "metadata", text: "example.com\nhttps://example.com/path" },
    { kind: "heading", text: "Original title" },
    { kind: "code", text: "fn main() {}" },
    { kind: "paragraph", text: "Original body" },
  ];
  const module = await pipeline(blocks, ["Tiêu đề", "Nội dung"]);
  const result = await module.translateCapture("sample.png", () => {});
  assert.deepEqual(module.calls.find(call => call.command === "translate_texts").args.texts, ["Original title", "Original body"]);
  assert.deepEqual(result.blocks.map(block => block.translated), [blocks[0].text, "Tiêu đề", blocks[2].text, "Nội dung"]);
  assert.ok(module.toMarkdown(result.blocks).includes("example.com\nhttps://example.com/path"));
});

test("a capture containing only links never contacts a translation provider", async () => {
  const module = await pipeline([{ kind: "metadata", text: "https://example.com" }], []);
  const result = await module.translateCapture("link.png", () => {});
  assert.equal(module.calls.length, 1);
  assert.equal(result.blocks[0].translated, "https://example.com");
});
