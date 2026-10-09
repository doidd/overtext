// Diagnose the production renderer with real engine geometry and controlled text.
// This records failures; it does not make existing defects pass by changing fixtures.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { resolve, dirname, basename } from "node:path";
import { chromium } from "playwright";
import ts from "typescript";

const rawPath = process.argv[2] ?? "logs/ocr-benchmark/raw.json";
const output = resolve(process.argv[3] ?? "logs/ocr-benchmark/renderer.json");
const raw = JSON.parse(readFileSync(rawPath, "utf8"));
const cases = new Map(raw.corpus.cases.filter(c => c.dataset === "local").map(c => [c.id, c]));
const renderer = ts.transpileModule(readFileSync("src/renderImage.ts", "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText.replace("export async function", "async function").replace(/export \{\};?/, "");
const font = readFileSync("tests/render/fonts/NotoSans-Latin-VN.woff2").toString("base64");
const reports = [];
const figures = [];
const browser = await chromium.launch({ headless: true });
try {
  for (const scale of [1, 1.5]) {
    const page = await browser.newPage({ viewport: { width: 1200, height: 800 }, deviceScaleFactor: scale });
    await page.setContent('<meta charset="utf-8">');
    await page.addScriptTag({ content: renderer });
    await page.addStyleTag({ content: `@font-face{font-family:OverTextProbe;src:url(data:font/woff2;base64,${font});font-weight:100 900}` });
    await page.evaluate(() => Promise.all([document.fonts.load('400 16px OverTextProbe'), document.fonts.load('600 16px OverTextProbe')]));
    for (const result of raw.results) {
      const fixture = cases.get(result.case);
      const run = result.runs.at(-1);
      if (!fixture || run.error) continue;
      const source = "data:image/png;base64," + readFileSync(fixture.image).toString("base64");
      const blocks = run.blocks.map(b => ({ ...b, translated: ["metadata", "code"].includes(b.kind) ? b.text
        : Array.from({ length: Math.max(2, Math.ceil(b.text.length / 7)) }, (_, i) => ["nội", "dung", "kiểm", "thử"][i % 4]).join(" ") }));
      const probe = await page.evaluate(async ({ source, fixture, blocks }) => {
        const render = window.renderTranslatedImage;
        const read = async url => {
          const image = new Image(); image.src = url; await image.decode();
          const canvas = document.createElement("canvas"); canvas.width = fixture.width; canvas.height = fixture.height;
          const ctx = canvas.getContext("2d"); ctx.drawImage(image, 0, 0); return ctx.getImageData(0, 0, canvas.width, canvas.height).data;
        };
        const draws = [];
        const original = CanvasRenderingContext2D.prototype.fillText;
        const originalRect = CanvasRenderingContext2D.prototype.rect;
        let clip;
        CanvasRenderingContext2D.prototype.rect = function(x, y, width, height) {
          clip = { x, y, width, height };
          return originalRect.call(this, x, y, width, height);
        };
        CanvasRenderingContext2D.prototype.fillText = function(text, x, y, ...rest) {
          const m = this.measureText(text);
          draws.push({ text, x, y, width: m.width, ascent: m.actualBoundingBoxAscent, descent: m.actualBoundingBoxDescent,
            fontPx: Number(this.font.match(/([\d.]+)px/)[1]), clip,
            clipped: clip && (x - m.actualBoundingBoxLeft < clip.x - 1 || x + m.actualBoundingBoxRight > clip.x + clip.width + 1
              || y - m.actualBoundingBoxAscent < clip.y - 1 || y + m.actualBoundingBoxDescent > clip.y + clip.height + 1) });
          return original.call(this, text, x, y, ...rest);
        };
        let url;
        try { url = await render(source, { ...fixture, blocks }, { fontFamily: "OverTextProbe, sans-serif" }); }
        finally { CanvasRenderingContext2D.prototype.fillText = original; CanvasRenderingContext2D.prototype.rect = originalRect; }
        const blankUrl = await render(source, { ...fixture, blocks: blocks.map(b => ["metadata", "code"].includes(b.kind) ? b : { ...b, translated: " " }) }, { fontFamily: "OverTextProbe, sans-serif" });
        const before = await read(source), blank = await read(blankUrl), translated = await read(url);
        const regions = fixture.regions.map(r => {
          let sourceInk = 0, residualInk = 0, metadataChanges = 0;
          const preserved = ["metadata", "code"].includes(r.kind);
          for (let y = Math.ceil(r.y); y < Math.floor(r.y + r.height); y++)
            for (let x = Math.ceil(r.x); x < Math.floor(r.x + r.width); x++) {
              const i = (y * fixture.width + x) * 4;
              if (preserved) {
                if ([0, 1, 2, 3].some(c => before[i + c] !== translated[i + c])) metadataChanges++;
              } else if (Math.hypot(255-before[i], 255-before[i+1], 255-before[i+2]) > 100) {
                sourceInk++;
                if (Math.hypot(255-blank[i], 255-blank[i+1], 255-blank[i+2]) > 100) residualInk++;
              }
            }
          return { kind: r.kind, text: r.text, sourceInk, residualInk, metadataChanges };
        });
        return { url, blankUrl, regions, draws, clippedDraws: draws.filter(d => d.clipped).length,
          residualInk: regions.reduce((sum, r) => sum + r.residualInk, 0),
          metadataChanges: regions.reduce((sum, r) => sum + r.metadataChanges, 0) };
      }, { source, fixture, blocks });
      const { url, blankUrl, ...metrics } = probe;
      reports.push({ case: fixture.id, engine: result.engine, scale, ...metrics });
      if (scale === 1) figures.push({ case: fixture.id, engine: result.engine, source, url, blankUrl });
    }
    await page.close();
  }
} finally { await browser.close(); }
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, JSON.stringify({ scope: "Local white-background screenshot diagnostics. Controlled Vietnamese text, not provider translations. Pinned font. Residual pixels after blank erasure are compared against independent reference regions.", reports }, null, 2) + "\n");
const escape = s => s.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll('"', "&quot;");
const rows = reports.map(r => `<tr><td>${escape(r.case)}</td><td>${r.engine}</td><td>${r.scale}</td><td>${r.residualInk}</td><td>${r.metadataChanges}</td><td>${r.clippedDraws}</td></tr>`).join("");
const panels = figures.map(f => `<section><h2>${escape(f.case)} · ${f.engine}</h2><div class="panes"><div>Ảnh gốc<img src="${f.source}"></div><div>Xóa chữ, chưa vẽ bản dịch<img src="${f.blankUrl}"></div><div>Text kiểm thử có kiểm soát<img src="${f.url}"></div></div></section>`).join("");
writeFileSync(output.replace(/\.json$/, ".html"), `<!doctype html><html lang="vi"><meta charset="utf-8"><title>Kiểm tra renderer theo engine OCR</title><style>body{font:14px system-ui;margin:24px}table{border-collapse:collapse}td,th{padding:6px;border:1px solid #ccc}.panes{display:grid;grid-template-columns:repeat(3,1fr);gap:16px}img{display:block;width:100%}section{margin:32px 0}</style><h1>Renderer với tọa độ thật từ từng engine</h1><p>Chỉ các ảnh screenshot nền trắng có nhãn tham chiếu độc lập. Xóa chữ không vẽ để đếm pixel mực còn sót; kiểm tra website/code cần giữ nguyên. Text Việt được tạo để thử fit, không phải kết quả dịch thật. Có tọa độ từng lần vẽ, cỡ font và glyph vượt clip trong JSON. Số 0 không chứng minh bản dịch đúng.</p><table><tr><th>Ảnh</th><th>Engine</th><th>DPI scale</th><th>Pixel mực còn sót</th><th>Pixel metadata thay đổi</th><th>Lượt vẽ vượt clip</th></tr>${rows}</table>${panels}</html>\n`);
console.log(JSON.stringify({ probes: reports.length, cases: cases.size, output: basename(output), residualFailures: reports.filter(r => r.residualInk > 0).length,
  metadataFailures: reports.filter(r => r.metadataChanges > 0).length }));
