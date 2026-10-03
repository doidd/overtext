import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { languageName, messages, nativeOcrAvailable, resolveLocale, type MessageKey, type UiLanguage } from "./settingsI18n";

type Provider = "free" | "openai";
type SettingsData = { uiLang: UiLanguage; targetLang: string; ocrLang: string; provider: Provider; baseUrl: string; model: string };
type View = { settings: SettingsData; languages: [string, string][]; ocrLanguages: string[] | null; paddleocrInstalled: boolean };

const OCR_LANGUAGES: [string, string][] = [
  ["ja-JP", "Japanese"], ["en-US", "English"],
  ["ko-KR", "Korean"], ["zh-Hans-CN", "Simplified Chinese"],
  ["zh-Hant-TW", "Traditional Chinese"], ["fr-FR", "Français"], ["de-DE", "Deutsch"],
];

/** Model names drift over time; they're only starting points and stay editable. */
const PRESETS: { name: string; baseUrl: string; model: string; needsKey: boolean }[] = [
  { name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini", needsKey: true },
  { name: "Gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.5-flash", needsKey: true },
  { name: "Groq", baseUrl: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile", needsKey: true },
  { name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "meta-llama/llama-3.3-70b-instruct:free", needsKey: true },
  { name: "Ollama", baseUrl: "http://localhost:11434/v1", model: "qwen2.5:7b", needsKey: false },
];

export function Settings() {
  const [view, setView] = useState<View | null>(null);
  const [s, setS] = useState<SettingsData | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [hasKey, setHasKey] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; key: MessageKey; detail?: string } | null>(null);
  const [saving, setSaving] = useState(false);
  const [installingOcr, setInstallingOcr] = useState(false);

  const locale = resolveLocale(s?.uiLang ?? "system", navigator.languages);
  const t = messages[locale];
  const statusText = status ? t[status.key] + (status.detail ? ": " + status.detail : "") : "";
  useEffect(() => {
    document.documentElement.lang = locale;
    void getCurrentWindow().setTitle(t.title).catch(() => {});
  }, [locale, t.title]);

  const checkKey = (url: string) => {
    setClearKey(false);
    invoke<boolean>("has_api_key", { baseUrl: url }).then(setHasKey, () => setHasKey(false));
  };

  useEffect(() => {
    invoke<View>("get_settings").then((v) => {
      setView(v);
      setS({ ...v.settings, uiLang: v.settings.uiLang ?? "system" });
      if (v.settings.provider === "openai") checkKey(v.settings.baseUrl);
    }).catch((e) => setStatus({ ok: false, key: "loadError", detail: String(e) }));
  }, []);


  if (!view || !s) return <p className={status ? "error" : "hint"}>{status ? statusText : t.loading}</p>;
  const set = (patch: Partial<SettingsData>) => setS({ ...s, ...patch });
  const preset = PRESETS.find((p) => p.baseUrl === s.baseUrl);
  const ocrOptions = [...OCR_LANGUAGES, ...(view.ocrLanguages ?? [])
    .filter((tag) => tag === s.ocrLang || !OCR_LANGUAGES.some(([code]) => nativeOcrAvailable(code, [tag]))).map((tag): [string, string] => [tag, tag])];

  const installOcr = async () => {
    setInstallingOcr(true);
    setStatus({ ok: true, key: "installProgress" });
    try {
      await invoke("install_paddleocr");
      setView({ ...view, paddleocrInstalled: true });
      setStatus({ ok: true, key: "installDone" });
    } catch (e) {
      setStatus({ ok: false, key: "installError", detail: String(e) });
    } finally {
      setInstallingOcr(false);
    }
  };

  const save = async () => {
    setSaving(true);
    setStatus({ ok: true, key: s.provider === "openai" ? "checking" : "saving" });
    try {
      // null keeps the stored key, "" deletes it, anything else replaces it.
      const key = clearKey ? "" : apiKey.trim() ? apiKey.trim() : null;
      await invoke("save_settings", { settings: s, apiKey: s.provider === "openai" ? key : null });
      setStatus({ ok: true, key: s.provider === "openai" ? "verified" : "saved" });
      if (s.provider === "openai") setHasKey(key === null ? hasKey : key !== "");
      setApiKey("");
      setClearKey(false);
    } catch (e) {
      setStatus({ ok: false, key: "saveError", detail: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="settings">
      <label>
        {t.interfaceLanguage}
        <select value={s.uiLang} onChange={(e) => set({ uiLang: e.target.value as UiLanguage })}>
          <option value="system">{t.system}</option>
          <option value="vi">Tiếng Việt</option>
          <option value="en">English</option>
          <option value="ja">日本語</option>
        </select>
      </label>
      {view.ocrLanguages !== null && (
        <>
          <label>
            {t.ocrLanguage}
            <select value={s.ocrLang} onChange={(e) => set({ ocrLang: e.target.value })}>
              <option value="">{view.paddleocrInstalled ? t.autoOcr : t.windowsOcr}</option>
              {ocrOptions.map(([code, name]) => <option key={code} value={code}>
                {languageName(code, name, locale)}{nativeOcrAvailable(code, view.ocrLanguages ?? []) ? "" : " (PaddleOCR)"}
              </option>)}
            </select>
          </label>
          <p className="hint">
            {t.installedOcr} {view.ocrLanguages.length ? view.ocrLanguages.map(code => languageName(code, code, locale)).join(", ") : t.none}.
            {!s.ocrLang
              ? view.paddleocrInstalled
                ? " " + t.autoHint
                : " " + t.windowsHint
              : !nativeOcrAvailable(s.ocrLang, view.ocrLanguages)
              ? " " + t.fallbackHint
              : " " + t.sourceHint}
          </p>
          <p className="hint">
            {view.paddleocrInstalled
              ? t.paddleInstalled
              : t.paddleHint}
          </p>
          {!view.paddleocrInstalled && <button onClick={installOcr} disabled={installingOcr}>
            {installingOcr ? t.installing : t.install}
          </button>}
        </>
      )}
      <label>
        {t.target}
        <select value={s.targetLang} onChange={(e) => set({ targetLang: e.target.value })}>
          {view.languages.map(([code, name]) => (
            <option key={code} value={code}>
              {languageName(code, name, locale)}
            </option>
          ))}
        </select>
      </label>

      <label>
        {t.service}
        <select
          value={s.provider}
          onChange={(e) => {
            set({ provider: e.target.value as Provider });
            if (e.target.value === "openai") checkKey(s.baseUrl);
          }}
        >
          <option value="free">{t.free}</option>
          <option value="openai">{t.openai}</option>
        </select>
      </label>

      {s.provider === "free" && (
        <p className="hint">
          {t.freeHint}
        </p>
      )}

      {s.provider === "openai" && (
        <>
          <label>
            {t.provider}
            <select
              value={preset?.name ?? ""}
              onChange={(e) => {
                const p = PRESETS.find((x) => x.name === e.target.value);
                if (p) {
                  set({ baseUrl: p.baseUrl, model: p.model });
                  checkKey(p.baseUrl);
                }
              }}
            >
              <option value="">{t.custom}</option>
              {PRESETS.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}{!p.needsKey ? ` (${t.local})` : ""}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t.baseUrl}
            <input
              value={s.baseUrl}
              onChange={(e) => set({ baseUrl: e.target.value })}
              onBlur={() => checkKey(s.baseUrl)}
              spellCheck={false}
            />
          </label>
          <label>
            {t.model}
            <input value={s.model} onChange={(e) => set({ model: e.target.value })} spellCheck={false} />
          </label>
          <label>
            {t.apiKey} {preset && !preset.needsKey && <span className="muted">{t.optional}</span>}
            <input
              type="password"
              value={apiKey}
              autoComplete="off"
              placeholder={hasKey && !clearKey ? t.savedKey : t.pasteKey}
              onChange={(e) => {
                setApiKey(e.target.value);
                setClearKey(false);
              }}
            />
          </label>
          {hasKey && (
            <button className="link" onClick={() => setClearKey(!clearKey)}>
              {clearKey ? t.undoKey : t.deleteKey}
            </button>
          )}
          <p className="hint">
            {t.keyHint}
          </p>
        </>
      )}

      <div className="actions">
        {status && <span className={status.ok ? "ok" : "err"}>{statusText}</span>}
        <button onClick={() => getCurrentWindow().close()}>{t.close}</button>
        <button className="primary" onClick={save} disabled={saving}>
          {saving ? t.saving : t.save}
        </button>
      </div>
    </div>
  );
}
