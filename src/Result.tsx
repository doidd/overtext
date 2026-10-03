import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toMarkdown, translateCapture, type Block, type Phase, type Translation } from "./translation";
import { renderTranslatedImage } from "./renderImage";

type Props = { imagePath: string; width: number; height: number };
type View = "image" | "text" | "original";

/** Vision line boxes span ascender→descender; font size is a bit smaller. */
const FONT_PER_LINE = 0.82;
const MIN_SHRINK = 0.5;

function OverlayBlock({ block, scale }: { block: Block; scale: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const base = block.lineHeight * scale * FONT_PER_LINE;

  // Shrink until the (usually longer) translation fits the original box.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    let size = base;
    el.style.fontSize = `${size}px`;
    while ((el.scrollHeight > el.clientHeight + 1 || el.scrollWidth > el.clientWidth + 1) && size > base * MIN_SHRINK) {
      size *= 0.94;
      el.style.fontSize = `${size}px`;
    }
  }, [base, block.translated]);

  const pad = block.lineHeight * scale * 0.12;
  return (
    <div
      ref={ref}
      className="block"
      data-tauri-drag-region
      style={{
        left: block.x * scale - pad,
        top: block.y * scale - pad,
        width: block.width * scale + 2 * pad,
        height: block.height * scale + 2 * pad,
        padding: pad,
        color: block.color,
        background: block.background,
        textAlign: block.align,
        fontWeight: block.kind === "heading" ? 600 : 400,
        lineHeight: block.lineCount > 1 ? block.height / block.lineCount / (block.lineHeight * FONT_PER_LINE) : 1.15,
      }}
    >
      {block.translated}
    </div>
  );
}

export function Result({ imagePath, width }: Props) {
  const [failed, setFailed] = useState(false);
  const [translation, setTranslation] = useState<Translation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>("image");
  const [copiedText, setCopiedText] = useState(false);
  const [copiedImage, setCopiedImage] = useState(false);
  const [savedImage, setSavedImage] = useState(false);
  const [processingImage, setProcessingImage] = useState(false);
  const [phase, setPhase] = useState<Phase>("recognizing");

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") getCurrentWindow().close();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // OCR reads the PNG from disk, so it can start before the <img> has decoded.
  const requested = useRef(false);
  const [slowOcr, setSlowOcr] = useState(false);
  useEffect(() => {
    if (requested.current) return;
    requested.current = true;
    // macOS compiles Vision's models on first use (core ~60 s, occasional extras ~30 s);
    // results are cached per app name and macOS build, so this is rare after the first run.
    const timer = setTimeout(() => setSlowOcr(true), 3000);
    translateCapture(imagePath, (p) => {
      if (p !== "recognizing") clearTimeout(timer);
      setPhase(p);
    })
      .then(async (res) => {
        setTranslation(res);
        if (res && res.blocks.length > 0) {
          try {
            const srcMd = res.blocks.map((b) => b.text).join("\n\n");
            const transMd = toMarkdown(res.blocks);
            // Create lightweight thumbnail data URL
            const thumbDataUrl = await renderTranslatedImage(convertFileSrc(imagePath), res);
            await invoke("record_history", {
              sourceMarkdown: srcMd,
              translatedMarkdown: transMd,
              thumbnailBase64: thumbDataUrl,
            });
          } catch (err) {
            console.error("Failed to auto-record history:", err);
          }
        }
      })
      .catch((e) => setError(String(e)))
      .finally(() => clearTimeout(timer));
  }, [imagePath]);

  const onLoad = async (img: HTMLImageElement) => {
    await img.decode().catch(() => {});
    invoke("window_ready");
  };

  const scale = translation ? width / translation.width : 1;
  const markdown = translation ? toMarkdown(translation.blocks) : "";

  const copyText = async () => {
    await navigator.clipboard.writeText(markdown);
    setCopiedText(true);
    setTimeout(() => setCopiedText(false), 1200);
  };

  const copyImage = async () => {
    if (!translation || processingImage) return;
    setProcessingImage(true);
    try {
      const dataUrl = await renderTranslatedImage(convertFileSrc(imagePath), translation);
      await invoke("copy_image_to_clipboard", { base64Png: dataUrl });
      setCopiedImage(true);
      setTimeout(() => setCopiedImage(false), 1200);
    } catch (e) {
      setError(`Lỗi chép ảnh: ${e}`);
    } finally {
      setProcessingImage(false);
    }
  };

  const saveImage = async () => {
    if (!translation || processingImage) return;
    setProcessingImage(true);
    try {
      const dataUrl = await renderTranslatedImage(convertFileSrc(imagePath), translation);
      const now = new Date();
      const dateStr = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
      const defaultName = `OverText_${dateStr}.png`;
      const saved = await invoke<boolean>("save_image_to_file", { base64Png: dataUrl, defaultName });
      if (saved) {
        setSavedImage(true);
        setTimeout(() => setSavedImage(false), 1500);
      }
    } catch (e) {
      setError(`Lỗi lưu ảnh: ${e}`);
    } finally {
      setProcessingImage(false);
    }
  };

  return (
    <div className="result" data-tauri-drag-region>
      {failed ? (
        <p className="error" data-tauri-drag-region>
          Không tải được ảnh: {imagePath}
        </p>
      ) : (
        <img
          src={convertFileSrc(imagePath)}
          draggable={false}
          data-tauri-drag-region
          onLoad={(e) => onLoad(e.currentTarget)}
          onError={() => {
            setFailed(true);
            invoke("window_ready");
          }}
        />
      )}

      {translation && view === "image" &&
        translation.blocks
          .filter((b) => b.kind !== "code")
          .map((b, i) => <OverlayBlock key={i} block={b} scale={scale} />)}

      {translation && view === "text" && (
        <div className="text-view">
          {translation.blocks.length === 0 ? <p className="muted">Không tìm thấy chữ.</p> : <pre>{markdown}</pre>}
        </div>
      )}

      <div className="toolbar">
        {!translation && !error && (
          <span className="status">
            {phase === "translating"
              ? "Đang dịch…"
              : slowOcr
                ? "macOS đang chuẩn bị mô hình OCR (chỉ lần đầu, ~30–60 giây)…"
                : "Đang nhận dạng chữ…"}
          </span>
        )}
        {error && <span className="status error-pill" title={error}>Lỗi: {error}</span>}
        {translation && (
          <>
            {(["image", "text", "original"] as const).map((v) => (
              <button key={v} className={view === v ? "active" : ""} onClick={() => setView(v)}>
                {v === "image" ? "Ảnh dịch" : v === "text" ? "Văn bản" : "Gốc"}
              </button>
            ))}
            <button onClick={copyImage} title="Sao chép ảnh đã dịch vào clipboard">
              {copiedImage ? "Đã chép ảnh" : "Chép ảnh"}
            </button>
            <button onClick={saveImage} title="Lưu ảnh đã dịch ra file PNG">
              {savedImage ? "Đã lưu ảnh" : "Lưu ảnh"}
            </button>
            <button onClick={copyText} title="Sao chép văn bản Markdown">
              {copiedText ? "Đã chép chữ" : "Chép chữ"}
            </button>
          </>
        )}
        <button title="Đóng (Esc)" onClick={() => getCurrentWindow().close()}>
          ×
        </button>
      </div>
    </div>
  );
}
