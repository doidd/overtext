import { test, expect } from "@playwright/test";
import { build } from "vite";
import react from "@vitejs/plugin-react";

let bundle: string;
test.beforeAll(async () => {
  const result = await build({ configFile: false, logLevel: "error", define: { "process.env.NODE_ENV": '"production"' }, plugins: [react(), {
    name: "settings-test-mocks",
    enforce: "pre",
    resolveId(id) {
      if (id.startsWith("@tauri-apps/api/") || id.endsWith("useAppLocale")) return "\0" + id;
    },
    load(id) {
      if (id === "\0@tauri-apps/api/core") return `export async function invoke(command,args){window.calls.push({command,args}); if(command==='get_settings')return structuredClone(window.fixture); if(command==='install_rapidocr'){if(window.failInstall)throw Error('download failed'); window.fixture[args.model==='mobile'?'rapidMobileInstalled':'rapidServerInstalled']=true;} if(command==='save_settings')window.saved=args.settings; return null;}`;
      if (id === "\0@tauri-apps/api/window") return `export const getCurrentWindow=()=>({setTitle:async()=>{},close:async()=>{}});`;
      if (id.endsWith("useAppLocale")) return `export const useAppLocale=()=>({systemLocale:'en'});`;
    },
  }], build: { write: false, lib: { entry: process.cwd() + "/tests/render/settings-entry.ts", formats: ["iife"], name: "SettingsTest" } } });
  const output = (Array.isArray(result) ? result[0] : result).output;
  bundle = output.find(item => item.type === "chunk")!.code;
});

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", error => errors.push(error.message));
  await page.setContent('<div id="root"></div>');
  await page.evaluate(() => Object.assign(window, { calls: [], fixture: {
    settings: { uiLang: "vi", targetLang: "vi", ocrLang: "", provider: "free", baseUrl: "", model: "" },
    languages: [["vi", "Vietnamese"]], ocrLanguages: ["en-US", "ja"],
    paddleocrInstalled: true, rapidMobileInstalled: false, rapidServerInstalled: false,
  } }));
  await page.addScriptTag({ content: bundle });
  await expect(page.locator("select").first()).toBeVisible({ timeout: 4000 }).catch(error => {
    throw new Error(errors.join("; ") || String(error));
  });
});

test("old settings default to Mobile; downloaded Server can be selected and saved", async ({ page }) => {
  await expect(page.getByLabel("Engine OCR")).toHaveValue("rapid-mobile");
  await page.getByRole("button", { name: "Tải và cài RapidOCR Mobile", exact: true }).click();
  await expect(page.getByRole("button", { name: "Tải và cài RapidOCR Mobile", exact: true })).toHaveCount(0);
  await page.getByLabel("Engine OCR").selectOption("rapid-server");
  await page.getByRole("button", { name: "Tải và cài RapidOCR Server", exact: true }).click();
  await expect(page.getByRole("button", { name: "Tải và cài RapidOCR Server", exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "Lưu", exact: true }).click();
  await expect.poll(() => page.evaluate(() => (window as any).saved?.ocrEngine)).toBe("rapid-server");
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === "install_rapidocr").map((c: any) => c.args.model))).toEqual(["mobile", "server"]);
});

test("failed download keeps install action available; locale changes labels", async ({ page }) => {
  await page.evaluate(() => { (window as any).failInstall = true; });
  await page.getByRole("button", { name: "Tải và cài RapidOCR Server", exact: true }).click();
  await expect(page.getByText(/Không cài được OCR:.*download failed/)).toBeVisible();
  await expect(page.getByRole("button", { name: "Tải và cài RapidOCR Server", exact: true })).toBeEnabled();
  await page.getByLabel("Ngôn ngữ giao diện").selectOption("en");
  await expect(page.getByRole("button", { name: "Download and install RapidOCR Server", exact: true })).toBeVisible();
  await page.getByLabel("Interface language").selectOption("ja");
  await expect(page.getByRole("button", { name: "RapidOCR Server をダウンロードしてインストール", exact: true })).toBeVisible();
});
