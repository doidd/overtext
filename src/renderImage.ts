import type { Translation } from "./translation";

const FONT_PER_LINE = 0.82;
const MIN_SHRINK = 0.5;

/**
 * Bakes the translated text blocks onto the original image canvas at full physical resolution.
 * Returns a PNG data URL (data:image/png;base64,...).
 */
export async function renderTranslatedImage(
  imageSrc: string,
  translation: Translation,
): Promise<string> {
  const img = new Image();
  img.crossOrigin = "anonymous";
  img.src = imageSrc;
  await img.decode();

  const canvas = document.createElement("canvas");
  canvas.width = translation.width;
  canvas.height = translation.height;
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("Could not get 2d canvas context");

  // Draw background original image
  ctx.drawImage(img, 0, 0, translation.width, translation.height);

  // Render each non-code translated block over the image
  for (const block of translation.blocks) {
    if (block.kind === "code") continue;

    const pad = block.lineHeight * 0.12;
    const boxX = block.x - pad;
    const boxY = block.y - pad;
    const boxW = block.width + 2 * pad;
    const boxH = block.height + 2 * pad;

    // Erase original text by drawing solid background fill
    ctx.fillStyle = block.background;
    ctx.fillRect(boxX, boxY, boxW, boxH);

    // Prepare text styling and find optimal font size so it fits inside box
    const baseFontSize = block.lineHeight * FONT_PER_LINE;
    const minFontSize = baseFontSize * MIN_SHRINK;
    const isHeading = block.kind === "heading";

    let fontSize = baseFontSize;
    let lines: string[] = [];

    // Helper to calculate wrapped lines given a font size
    const getLines = (size: number): string[] => {
      ctx.font = `${isHeading ? "600 " : "400 "}${size}px -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif`;
      const words = block.translated.split(/\s+/);
      const result: string[] = [];
      let currentLine = "";

      for (const word of words) {
        const testLine = currentLine ? `${currentLine} ${word}` : word;
        const metrics = ctx.measureText(testLine);
        if (metrics.width > boxW - pad && currentLine) {
          result.push(currentLine);
          currentLine = word;
        } else {
          currentLine = testLine;
        }
      }
      if (currentLine) result.push(currentLine);
      return result;
    };

    // Auto-shrink font until text fits height and width
    while (fontSize > minFontSize) {
      lines = getLines(fontSize);
      const lineHeight = fontSize * 1.18;
      const totalH = lines.length * lineHeight;
      if (totalH <= boxH + 2) break;
      fontSize *= 0.94;
    }

    // Draw lines
    ctx.fillStyle = block.color;
    ctx.textBaseline = "top";
    const actualLineHeight = fontSize * 1.18;
    const startY = boxY + pad;

    lines.forEach((line, idx) => {
      const lineY = startY + idx * actualLineHeight;
      let lineX = boxX + pad;

      if (block.align === "center") {
        const w = ctx.measureText(line).width;
        lineX = boxX + (boxW - w) / 2;
      } else if (block.align === "right") {
        const w = ctx.measureText(line).width;
        lineX = boxX + boxW - pad - w;
      }

      ctx.fillText(line, lineX, lineY);
    });
  }

  return canvas.toDataURL("image/png");
}
