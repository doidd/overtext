import { test, expect } from "@playwright/test";
import { readFileSync } from "node:fs";
import ts from "typescript";

// Exercise the production renderer without a Tauri window or network provider.
const renderer = ts.transpileModule(readFileSync("src/renderImage.ts", "utf8"), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText.replace("export async function", "async function").replace(/export \{\};?/, "");

// Text measurement depends on the font, so the same translation can wrap differently per OS.
//  - pinned-font:  bundled Noto Sans; identical on every OS (the reference result).
//  - system-font:  the production font stack on the host OS (smoke test of real conditions).
//  - stress-long:  pinned font with ~50% longer translations, to force shrink-to-fit and
//                  wrapping everywhere; only the pixel-containment invariants must hold.
const pinnedFont = readFileSync("tests/render/fonts/NotoSans-Latin-VN.woff2").toString("base64");
const PINNED = '"OverTextTest", sans-serif';
const lengthen = (t: string) => { const w = t.split(" "); return `${t} ${w.slice(0, Math.ceil(w.length / 2)).join(" ")}`; };

for (const mode of ["pinned-font", "system-font", "stress-long"] as const)
for (const name of ["search-results", "japanese-list", "textract"]) {
  const base = JSON.parse(readFileSync(`tests/render/fixtures/${name}.json`, "utf8"));
  const fixture = mode !== "stress-long" ? base : { ...base, blocks: base.blocks.map((b: any) =>
    b.translated.trim() === b.text.trim() ? b : { ...b, translated: lengthen(b.translated) }) };
  const source = "data:image/png;base64," + readFileSync(`src-tauri/assets/ocr-${name}.png`).toString("base64");

  test(`${mode} ${name}: erase source, preserve surroundings, fit and center translated text`, async ({ page }, testInfo) => {
    await page.setContent('<meta charset="utf-8"><img id="result">');
    await page.addScriptTag({ content: renderer });
    if (mode !== "system-font") {
      await page.addStyleTag({ content: `@font-face{font-family:"OverTextTest";src:url(data:font/woff2;base64,${pinnedFont}) format("woff2");font-weight:100 900;font-stretch:62.5% 100%;}` });
      await page.evaluate(() => Promise.all([document.fonts.load('400 16px "OverTextTest"'), document.fonts.load('600 16px "OverTextTest"')]));
    }
    const result = await page.evaluate(async ({ source, fixture, fontFamily }) => {
      const render = (window as any).renderTranslatedImage;
      const read = async (url: string) => {
        const image = new Image(); image.src = url; await image.decode();
        const canvas = document.createElement("canvas");
        canvas.width = fixture.width; canvas.height = fixture.height;
        const ctx = canvas.getContext("2d")!; ctx.drawImage(image, 0, 0);
        return ctx.getImageData(0, 0, canvas.width, canvas.height).data;
      };
      const active = fixture.blocks.filter((b: any) => !["metadata", "code"].includes(b.kind)
        && b.translated.trim() !== b.text.trim());
      const draws: any[] = [];
      const original = CanvasRenderingContext2D.prototype.fillText;
      CanvasRenderingContext2D.prototype.fillText = function(text, x, y, ...rest) {
        const m = this.measureText(text);
        draws.push({ text, x, y, width: m.width, ascent: m.actualBoundingBoxAscent,
          descent: m.actualBoundingBoxDescent, color: this.fillStyle });
        return original.call(this, text, x, y, ...rest);
      };
      let url: string;
      try { url = await render(source, fixture, { fontFamily }); }
      finally { CanvasRenderingContext2D.prototype.fillText = original; }
      (document.querySelector("#result") as HTMLImageElement).src = url;
      const before = await read(source), after = await read(url);
      // A blank translation isolates source erasure from newly drawn glyphs.
      const blank = await read(await render(source, { ...fixture, blocks: fixture.blocks.map((b: any) =>
        active.includes(b) ? { ...b, translated: " " } : b) }, { fontFamily }));
      let metadataChanges = 0, unerased = 0, missingInk = 0;
      const equal = (a: Uint8ClampedArray, b: Uint8ClampedArray, i: number) =>
        [0, 1, 2, 3].every(c => a[i + c] === b[i + c]);
      const pixels = (b: any, fn: (i: number) => void) => {
        for (let y = Math.ceil(b.y); y < Math.floor(b.y + b.height); y++)
          for (let x = Math.ceil(b.x); x < Math.floor(b.x + b.width); x++) fn((y * fixture.width + x) * 4);
      };
      for (const b of fixture.blocks.filter((b: any) => ["metadata", "code"].includes(b.kind)))
        pixels(b, i => { if (!equal(before, after, i)) metadataChanges++; });
      for (const b of active) {
        const bg = b.background.slice(1).match(/../g).map((v: string) => parseInt(v, 16));
        let ink = 0;
        pixels(b, i => {
          if (bg.some((c: number, j: number) => blank[i + j] !== c)) unerased++;
          if (!equal(blank, after, i)) ink++;
        });
        if (!ink) missingInk++;
      }
      // No modified pixels outside the independently specified allowed regions.
      // Japanese list items share a column ending at the longest item's right edge.
      const listRight = Math.max(...fixture.blocks.filter((b: any) => b.kind === "list").map((b: any) => b.x + b.width));
      const areas = active.map((b: any) => ({ x: b.x, y: b.y,
        width: b.kind === "list" ? listRight - b.x : b.width,
        height: fixture.allowedHeights?.[fixture.blocks.indexOf(b)] ?? b.height, pad: b.lineHeight * .12 }));
      let outsideChanges = 0;
      for (let y = 0; y < fixture.height; y++) for (let x = 0; x < fixture.width; x++) {
        if (areas.some((a: any) => x + 1 > a.x - a.pad && x < a.x + a.width + a.pad
          && y + 1 > a.y - a.pad && y < a.y + a.height + a.pad)) continue;
        if (!equal(before, after, (y * fixture.width + x) * 4)) outsideChanges++;
      }
      return { metadataChanges, unerased, missingInk, outsideChanges, draws, active, listRight, url };
    }, { source, fixture, fontFamily: mode === "system-font" ? undefined : PINNED });
    await testInfo.attach(`${mode}-${name}.png`, { body: Buffer.from(result.url.split(",")[1], "base64"), contentType: "image/png" });
    expect(result.metadataChanges, "preserved metadata/code pixels").toBe(0);
    expect(result.unerased, "all original block pixels must be erased").toBe(0);
    expect(result.missingInk, "each translated block must be visible").toBe(0);
    expect(result.outsideChanges, "surrounding pixels must stay unchanged").toBe(0);
    if (mode === "stress-long") return;
    // Draw operations must contain the whole translation, fit their region, and
    // center the complete paragraph vertically (not only its first line).
    let cursor = 0;
    for (const block of result.active) {
      const lines = [];
      const expected = block.translated.replace(/\s/g, "");
      let joined = "";
      while (cursor < result.draws.length && joined.length < expected.length) {
        const line = result.draws[cursor++]; lines.push(line); joined += line.text.replace(/\s/g, "");
      }
      expect(joined).toBe(expected);
      const right = block.kind === "list" ? result.listRight : block.x + block.width;
      const allowedHeight = fixture.allowedHeights?.[fixture.blocks.findIndex((b: any) => b.x === block.x && b.y === block.y)] ?? block.height;
      for (const line of lines) {
        expect(line.x).toBeGreaterThanOrEqual(block.x - .01);
        expect(line.x + line.width).toBeLessThanOrEqual(right + .01);
        expect(line.y - line.ascent).toBeGreaterThanOrEqual(block.y - .01);
        expect(line.y + line.descent).toBeLessThanOrEqual(block.y + allowedHeight + .01);
        expect(line.color.toLowerCase()).toBe(block.color.toLowerCase());
      }
      const top = Math.min(...lines.map(l => l.y - l.ascent));
      const bottom = Math.max(...lines.map(l => l.y + l.descent));
      // Accented glyphs differ between rows: allow two pixels of optical offset,
      // but reject centering only the first line of a multiline paragraph.
      expect(Math.abs((top + bottom) / 2 - (block.y + Math.max(block.height, bottom - top) / 2))).toBeLessThanOrEqual(2);
    }
    expect(cursor).toBe(result.draws.length);
  });
}
