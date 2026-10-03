import { invoke } from "@tauri-apps/api/core";

export type Block = {
  x: number;
  y: number;
  width: number;
  height: number;
  lineHeight: number;
  lineCount: number;
  kind: "heading" | "list" | "paragraph" | "code";
  align: "left" | "center" | "right";
  color: string;
  background: string;
  text: string;
  translated: string;
};

/** Block geometry is in image pixels of `width` × `height`. */
export type Translation = { width: number; height: number; blocks: Block[] };

type Recognition = { width: number; height: number; blocks: Omit<Block, "translated">[] };

export type Phase = "recognizing" | "translating";

/** OCR, then translation; `onPhase` reports which step is running. Code/terminal
 *  blocks are never sent to a translator — translating identifiers and syntax
 *  produces nonsense, so they're shown as recognized. */
export async function translateCapture(imagePath: string, onPhase: (phase: Phase) => void): Promise<Translation> {
  onPhase("recognizing");
  const rec = await invoke<Recognition>("recognize_capture", { imagePath });
  if (rec.blocks.length === 0) return { ...rec, blocks: [] };

  const toTranslate = rec.blocks.filter((b) => b.kind !== "code");
  onPhase("translating");
  const translated = toTranslate.length
    ? await invoke<string[]>("translate_texts", { texts: toTranslate.map((b) => b.text) })
    : [];

  let i = 0;
  return { ...rec, blocks: rec.blocks.map((b) => (b.kind === "code" ? { ...b, translated: b.text } : { ...b, translated: translated[i++] })) };
}

/** Structured-text output: headings, list items, code blocks and paragraphs as Markdown. */
export function toMarkdown(blocks: Block[]): string {
  return blocks
    .map((b) => {
      const text = b.translated.trim();
      if (b.kind === "heading") return `## ${text}`;
      if (b.kind === "list") return `- ${text.replace(/^([•·▪◦‣●○■–\-*]|\d+[.)])\s*/, "")}`;
      if (b.kind === "code") return `\`\`\`\n${text}\n\`\`\``;
      return text;
    })
    .join("\n\n")
    .replace(/(^- .*)\n\n(?=- )/gm, "$1\n");
}
