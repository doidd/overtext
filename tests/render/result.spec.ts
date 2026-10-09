import { test, expect } from "@playwright/test";
import { build } from "vite";
import react from "@vitejs/plugin-react";
import { readFileSync } from "node:fs";
const messages = JSON.parse(readFileSync("src/locales/messages.json", "utf8"));

const png = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRZkAAAAASUVORK5CYII=";
const source = ["データ加工・整理", "・共通指摘IDの統合", "https://example.com"];
let bundle: string;

test.beforeAll(async () => {
  const result = await build({ configFile: false, logLevel: "error", define: { "process.env.NODE_ENV": '"production"' }, plugins: [react(), {
    name: "result-test-mocks", enforce: "pre",
    resolveId(id) {
      if (id.startsWith("@tauri-apps/api/") || id.endsWith("useAppLocale") || id.endsWith("renderImage")) return "\0" + id;
    },
    load(id) {
      if (id === "\0@tauri-apps/api/core") return `export const convertFileSrc=()=>${JSON.stringify(png)}; export async function invoke(command,args){window.calls.push({command,args}); if(command==='recognize_capture')return window.recognition; if(command==='translate_texts')return ['Xử lý dữ liệu','Hợp nhất mã chung']; return null;}`;
      if (id === "\0@tauri-apps/api/window") return `export const getCurrentWindow=()=>({close:async()=>{}});`;
      if (id.endsWith("useAppLocale")) return `const messages=${JSON.stringify(messages)}; export const useAppLocale=()=>({t:messages[window.locale]});`;
      if (id.endsWith("renderImage")) return `export const renderTranslatedImage=async()=>${JSON.stringify(png)};`;
    },
  }], build: { write: false, lib: { entry: process.cwd() + "/tests/render/result-entry.ts", formats: ["iife"], name: "ResultTest" } } });
  bundle = (Array.isArray(result) ? result[0] : result).output.find(item => item.type === "chunk")!.code;
});

async function mount(page: import("@playwright/test").Page, locale: string, empty = false) {
  await page.setContent(`<style>${readFileSync("src/styles.css", "utf8")}</style><div id="root"></div>`);
  await page.evaluate(({ locale, source, empty }) => {
    Object.assign(window, { locale, calls: [], recognition: { width: 408, height: 204, blocks: empty ? [] : source.map((text, i) => ({
      text, kind: ["heading", "list", "metadata"][i], x: 0, y: i * 40, width: 400, height: 30,
      lineHeight: 20, lineCount: 1, align: "left", color: "#111", background: "#fff",
    })) } });
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText: async (text: string) => { (window as any).copied = text; } } });
  }, { locale, source, empty });
  await page.addScriptTag({ content: bundle });
  await page.locator(".result").hover();
}

for (const locale of ["vi", "en", "ja"] as const) {
  test(`${locale}: OCR text copies the source; translated text and original image remain available`, async ({ page }) => {
    const t = messages[locale];
    await mount(page, locale);
    await page.getByRole("button", { name: t.ocrText, exact: true }).click();
    await expect(page.locator(".text-view pre")).toHaveText(source.join("\n\n"));
    await page.getByRole("button", { name: t.copySource, exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as any).copied)).toBe(source.join("\n\n"));
    await page.getByRole("button", { name: t.textView, exact: true }).click();
    await expect(page.locator(".text-view pre")).toContainText("Xử lý dữ liệu");
    await page.getByRole("button", { name: t.copyText, exact: true }).click();
    await expect.poll(() => page.evaluate(() => (window as any).copied)).toBe("## Xử lý dữ liệu\n\n- Hợp nhất mã chung\n\nhttps://example.com");
    await page.getByRole("button", { name: t.original, exact: true }).click();
    await expect(page.locator(".text-view")).toHaveCount(0);
    await expect(page.locator(".result img")).toHaveAttribute("src", png);
    expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === "recognize_capture").length)).toBe(1);
  });
}

test("empty OCR shows the localized empty state in the source tab", async ({ page }) => {
  await mount(page, "vi", true);
  await page.getByRole("button", { name: messages.vi.ocrText, exact: true }).click();
  await expect(page.locator(".text-view")).toHaveText(messages.vi.noText);
  await page.getByRole("button", { name: messages.vi.copySource, exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).copied)).toBe("");
});
