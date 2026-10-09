import { useEffect, useRef, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { toMarkdown, translateCapture, type Phase, type Translation } from "./translation";
import { useAppLocale } from "./useAppLocale";
import { renderTranslatedImage } from "./renderImage";

type Props = { imagePath: string; width: number; height: number };
type View = "image" | "text" | "original" | "source";

export function Result({ imagePath }: Props) {
  const { t } = useAppLocale();
  const [failed, setFailed] = useState(false);
  const [translatedImage, setTranslatedImage] = useState<string | null>(null);
  const windowShown = useRef(false);
  const [translation, setTranslation] = useState<Translation | null>(null);
  const [error, setError] = useState<{ key?: "copyImageError" | "saveImageError"; detail: string } | null>(null);
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
        const rendered = res.blocks.length ? await renderTranslatedImage(convertFileSrc(imagePath), res) : null;
        setTranslatedImage(rendered);
        setTranslation(res);
        if (res && res.blocks.length > 0) {
          try {
            const srcMd = res.blocks.map((b) => b.text).join("\n\n");
            const transMd = toMarkdown(res.blocks);
            // Create lightweight thumbnail data URL
            const thumbDataUrl = rendered;
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
      .catch((e) => setError({ detail: String(e) }))
      .finally(() => clearTimeout(timer));
  }, [imagePath]);

  const onLoad = async (img: HTMLImageElement) => {
    if (windowShown.current) return;
    windowShown.current = true;
    await img.decode().catch(() => {});
    invoke("window_ready");
  };

  const errorText = error ? (error.key ? t[error.key] + ": " : "") + error.detail : "";
  const markdown = translation ? toMarkdown(translation.blocks) : "";
  const sourceText = translation ? translation.blocks.map((b) => b.text).join("\n\n") : "";

  const copyText = async () => {
    try { await navigator.clipboard.writeText(view === "source" ? sourceText : markdown); }
    catch (e) { setError({ detail: String(e) }); return; }
    setCopiedText(true);
    setTimeout(() => setCopiedText(false), 1200);
  };

  const copyImage = async () => {
    if (!translation || processingImage) return;
    setProcessingImage(true);
    try {
      const dataUrl = translatedImage ?? await renderTranslatedImage(convertFileSrc(imagePath), translation);
      await invoke("copy_image_to_clipboard", { base64Png: dataUrl });
      setCopiedImage(true);
      setTimeout(() => setCopiedImage(false), 1200);
    } catch (e) {
      setError({ key: "copyImageError", detail: String(e) });
    } finally {
      setProcessingImage(false);
    }
  };

  const saveImage = async () => {
    if (!translation || processingImage) return;
    setProcessingImage(true);
    try {
      const dataUrl = translatedImage ?? await renderTranslatedImage(convertFileSrc(imagePath), translation);
      const now = new Date();
      const dateStr = `${now.getFullYear()}${String(now.getMonth() + 1).padStart(2, "0")}${String(now.getDate()).padStart(2, "0")}_${String(now.getHours()).padStart(2, "0")}${String(now.getMinutes()).padStart(2, "0")}${String(now.getSeconds()).padStart(2, "0")}`;
      const defaultName = `OverText_${dateStr}.png`;
      const saved = await invoke<boolean>("save_image_to_file", { base64Png: dataUrl, defaultName });
      if (saved) {
        setSavedImage(true);
        setTimeout(() => setSavedImage(false), 1500);
      }
    } catch (e) {
      setError({ key: "saveImageError", detail: String(e) });
    } finally {
      setProcessingImage(false);
    }
  };

  return (
    <div className="result" data-tauri-drag-region>
      {failed ? (
        <p className="error" data-tauri-drag-region>
          {t.imageLoadError}: {imagePath}
        </p>
      ) : (
        <img
          src={view === "image" && translatedImage ? translatedImage : convertFileSrc(imagePath)}
          draggable={false}
          data-tauri-drag-region
          onLoad={(e) => onLoad(e.currentTarget)}
          onError={() => {
            setFailed(true);
            invoke("window_ready");
          }}
        />
      )}

      {translation && (view === "text" || view === "source") && (
        <div className="text-view">
          {translation.blocks.length === 0 ? <p className="muted">{t.noText}</p> : <pre>{view === "source" ? sourceText : markdown}</pre>}
        </div>
      )}

      <div className="toolbar">
        {!translation && !error && (
          <span className="status">
            {phase === "translating"
              ? t.translating
              : slowOcr
                ? t.slowOcr
                : t.recognizing}
          </span>
        )}
        {error && <span className="status error-pill" title={errorText}>{t.error}: {errorText}</span>}
        {translation?.blocks.length === 0 && <span className="status error-pill">
          {t.noTextHint}
        </span>}
        {translation && (
          <>
            {(["image", "text", "original", "source"] as const).map((v) => (
              <button key={v} className={view === v ? "active" : ""} onClick={() => { setView(v); setCopiedText(false); }}>
                {v === "image" ? t.translatedImage : v === "text" ? t.textView : v === "original" ? t.original : t.ocrText}
              </button>
            ))}
            <button onClick={copyImage} title={t.copyImageHint}>
              {copiedImage ? t.copiedImage : t.copyImage}
            </button>
            <button onClick={saveImage} title={t.saveImageHint}>
              {savedImage ? t.savedImage : t.saveImage}
            </button>
            <button onClick={copyText} title={view === "source" ? t.copySource : t.copyTextHint}>
              {view === "source" ? (copiedText ? t.copiedSource : t.copySource) : (copiedText ? t.copiedText : t.copyText)}
            </button>
          </>
        )}
        <button title={t.closeHint} onClick={() => getCurrentWindow().close()}>
          ×
        </button>
      </div>
    </div>
  );
}
