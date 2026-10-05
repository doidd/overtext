import type { Block, Translation } from "./translation";

const FONT_PER_LINE = 0.82;
const MIN_SHRINK = 0.5;

function wrapText(ctx: CanvasRenderingContext2D, text: string, width: number): string[] {
  const lines: string[] = [];
  for (const paragraph of text.split(/\r?\n/)) {
    let line = "";
    for (const word of paragraph.split(/\s+/).filter(Boolean)) {
      const candidate = line ? `${line} ${word}` : word;
      if (ctx.measureText(candidate).width <= width) {
        line = candidate;
        continue;
      }
      if (line) lines.push(line);
      line = "";
      // Handle unspaced CJK text and long identifiers without clipping.
      for (const char of word) {
        if (line && ctx.measureText(line + char).width > width) {
          lines.push(line);
          line = "";
        }
        line += char;
      }
    }
    lines.push(line);
  }
  return lines;
}

function textRegion(block: Block, blocks: Block[]): { width: number; height: number } {
  if (block.kind !== "list") return { width: block.width, height: block.height };
  // Let translated items use the shared list column, not only the width of a
  // short source sentence. Keep separate columns and distant lists independent.
  const neighbors = blocks.filter((other) => other.kind === "list"
    && Math.abs(other.x - block.x) < Math.max(other.lineHeight, block.lineHeight) * 1.5
    && Math.abs(other.y - block.y) < block.lineHeight * 8);
  const right = Math.max(block.x + block.width, ...neighbors.map((other) => other.x + other.width));
  const next = blocks.filter((other) => other.y > block.y
    && other.x < right && other.x + other.width > block.x)
    .sort((a, b) => a.y - b.y)[0];
  // A one-line source item keeps its original height: the translation shrinks to fit instead of
  // wrapping into the gap before the next item (which also changes pixels outside the item).
  const gap = next && block.lineCount > 1 ? next.y - block.y : 0;
  const margin = next ? Math.max(next.lineHeight, block.lineHeight) * 0.24 : 0;
  const height = gap > 0 && gap < block.lineHeight * 3
    ? Math.max(block.height, gap - margin) : block.height;
  return { width: right - block.x, height };
}

const SYSTEM_FONT = '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif';

function drawBlock(ctx: CanvasRenderingContext2D, block: Block, region: { width: number; height: number }, fontFamily: string) {
  const base = block.lineHeight * FONT_PER_LINE;
  const minimum = base * MIN_SHRINK;
  let fontSize = base;
  let lines: string[] = [];
  let ascent = 0;
  let descent = 0;
  let inkHeight = 0;
  let advance = 0;
  ctx.textBaseline = "alphabetic";
  while (true) {
    ctx.font = `${block.kind === "heading" ? 600 : 400} ${fontSize}px ${fontFamily}`;
    lines = wrapText(ctx, block.translated, region.width);
    const metrics = lines.map((line) => ctx.measureText(line || "M"));
    ascent = Math.max(...metrics.map((m) => m.actualBoundingBoxAscent));
    descent = Math.max(...metrics.map((m) => m.actualBoundingBoxDescent));
    advance = Math.max(fontSize * 1.18, ascent + descent);
    inkHeight = ascent + descent + (lines.length - 1) * advance;
    const offset = Math.max(0, (block.height - inkHeight) / 2);
    const fits = offset + inkHeight <= region.height && metrics.every((m) => m.width <= region.width);
    if (fits || fontSize <= minimum) break;
    fontSize = Math.max(minimum, fontSize * 0.94);
  }

  // Center the whole paragraph (all lines) in the OCR region, not just its first line.
  const baseline = block.y + Math.max(0, (block.height - inkHeight) / 2) + ascent;
  ctx.save();
  ctx.beginPath();
  ctx.rect(block.x, block.y, region.width, region.height);
  ctx.clip();
  ctx.fillStyle = block.color;
  lines.forEach((line, index) => {
    const width = ctx.measureText(line).width;
    const x = block.align === "center" ? block.x + (region.width - width) / 2
      : block.align === "right" ? block.x + region.width - width : block.x;
    ctx.fillText(line, x, baseline + index * advance);
  });
  ctx.restore();
}

export type RenderOptions = {
  /** CSS font-family for translated text. Tests pin a bundled font so measurement is OS independent. */
  fontFamily?: string;
};

/** One renderer for the live result, clipboard, saved image, and history. */
export async function renderTranslatedImage(
  imageSrc: string,
  translation: Translation,
  options: RenderOptions = {},
): Promise<string> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = imageSrc;
  await img.decode();
  await document.fonts.ready;

  const canvas = document.createElement("canvas");
  canvas.width = translation.width;
  canvas.height = translation.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Could not get 2d canvas context");
  ctx.drawImage(img, 0, 0, translation.width, translation.height);

  // Keep unchanged labels/numbers in their original font (e.g. slide badges).
  const blocks = translation.blocks.filter((block) => block.kind !== "code" && block.kind !== "metadata"
    && block.translated.trim() !== block.text.trim());
  const regions = blocks.map((block) => textRegion(block, translation.blocks));
  // Erase all source regions first so neighboring fills cannot overwrite translations.
  blocks.forEach((block, index) => {
    const pad = block.lineHeight * 0.12;
    ctx.fillStyle = block.background;
    const region = regions[index];
    ctx.fillRect(block.x - pad, block.y - pad, region.width + 2 * pad, region.height + 2 * pad);
  });
  const fontFamily = options.fontFamily ?? SYSTEM_FONT;
  blocks.forEach((block, index) => drawBlock(ctx, block, regions[index], fontFamily));
  return canvas.toDataURL("image/png");
}
