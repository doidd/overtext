import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";

type Provider = "free" | "openai";
type SettingsData = { targetLang: string; provider: Provider; baseUrl: string; model: string };
type View = { settings: SettingsData; languages: [string, string][] };

/** Model names drift over time; they're only starting points and stay editable. */
const PRESETS: { name: string; baseUrl: string; model: string; needsKey: boolean }[] = [
  { name: "OpenAI", baseUrl: "https://api.openai.com/v1", model: "gpt-4o-mini", needsKey: true },
  { name: "Gemini", baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", model: "gemini-2.5-flash", needsKey: true },
  { name: "Groq", baseUrl: "https://api.groq.com/openai/v1", model: "llama-3.3-70b-versatile", needsKey: true },
  { name: "OpenRouter", baseUrl: "https://openrouter.ai/api/v1", model: "meta-llama/llama-3.3-70b-instruct:free", needsKey: true },
  { name: "Ollama (chạy trên máy)", baseUrl: "http://localhost:11434/v1", model: "qwen2.5:7b", needsKey: false },
];

export function Settings() {
  const [view, setView] = useState<View | null>(null);
  const [s, setS] = useState<SettingsData | null>(null);
  const [apiKey, setApiKey] = useState("");
  const [clearKey, setClearKey] = useState(false);
  const [hasKey, setHasKey] = useState(false);
  const [status, setStatus] = useState<{ ok: boolean; text: string } | null>(null);
  const [saving, setSaving] = useState(false);

  const checkKey = (url: string) => {
    setClearKey(false);
    invoke<boolean>("has_api_key", { baseUrl: url }).then(setHasKey, () => setHasKey(false));
  };

  useEffect(() => {
    invoke<View>("get_settings").then((v) => {
      setView(v);
      setS(v.settings);
      if (v.settings.provider === "openai") checkKey(v.settings.baseUrl);
    });
  }, []);


  if (!view || !s) return null;
  const set = (patch: Partial<SettingsData>) => setS({ ...s, ...patch });
  const preset = PRESETS.find((p) => p.baseUrl === s.baseUrl);

  const save = async () => {
    setSaving(true);
    setStatus({ ok: true, text: s.provider === "openai" ? "Đang kiểm tra kết nối…" : "Đang lưu…" });
    try {
      // null keeps the stored key, "" deletes it, anything else replaces it.
      const key = clearKey ? "" : apiKey.trim() ? apiKey.trim() : null;
      await invoke("save_settings", { settings: s, apiKey: s.provider === "openai" ? key : null });
      setStatus({ ok: true, text: "Đã lưu (kết nối thành công)." });
      if (s.provider === "openai") setHasKey(key === null ? hasKey : key !== "");
      setApiKey("");
      setClearKey(false);
    } catch (e) {
      setStatus({ ok: false, text: String(e) });
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="settings">
      <label>
        Ngôn ngữ đích
        <select value={s.targetLang} onChange={(e) => set({ targetLang: e.target.value })}>
          {view.languages.map(([code, name]) => (
            <option key={code} value={code}>
              {name}
            </option>
          ))}
        </select>
      </label>

      <label>
        Dịch vụ dịch
        <select
          value={s.provider}
          onChange={(e) => {
            set({ provider: e.target.value as Provider });
            if (e.target.value === "openai") checkKey(s.baseUrl);
          }}
        >
          <option value="free">Miễn phí (Google, không cần key)</option>
          <option value="openai">API theo chuẩn OpenAI (LLM)</option>
        </select>
      </label>

      {s.provider === "free" && (
        <p className="hint">
          Dùng endpoint không chính thức của Google, dự phòng MyMemory. Có thể bị giới hạn hoặc thay đổi bất cứ lúc nào.
        </p>
      )}

      {s.provider === "openai" && (
        <>
          <label>
            Nhà cung cấp
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
              <option value="">Tùy chỉnh</option>
              {PRESETS.map((p) => (
                <option key={p.name} value={p.name}>
                  {p.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Base URL
            <input
              value={s.baseUrl}
              onChange={(e) => set({ baseUrl: e.target.value })}
              onBlur={() => checkKey(s.baseUrl)}
              spellCheck={false}
            />
          </label>
          <label>
            Model
            <input value={s.model} onChange={(e) => set({ model: e.target.value })} spellCheck={false} />
          </label>
          <label>
            API key {preset && !preset.needsKey && <span className="muted">(không bắt buộc)</span>}
            <input
              type="password"
              value={apiKey}
              autoComplete="off"
              placeholder={hasKey && !clearKey ? "Đã lưu trong Keychain — để trống để giữ nguyên" : "Dán API key"}
              onChange={(e) => {
                setApiKey(e.target.value);
                setClearKey(false);
              }}
            />
          </label>
          {hasKey && (
            <button className="link" onClick={() => setClearKey(!clearKey)}>
              {clearKey ? "Hoàn tác xóa key" : "Xóa key đã lưu"}
            </button>
          )}
          <p className="hint">
            Văn bản trong ảnh chụp sẽ được gửi tới máy chủ trên. Key chỉ lưu trong Keychain của macOS. Khi dịch vụ này lỗi,
            app báo lỗi và không tự gửi sang dịch vụ khác.
          </p>
        </>
      )}

      <div className="actions">
        {status && <span className={status.ok ? "ok" : "err"}>{status.text}</span>}
        <button onClick={() => getCurrentWindow().close()}>Đóng</button>
        <button className="primary" onClick={save} disabled={saving}>
          {saving ? "Đang lưu…" : "Lưu"}
        </button>
      </div>
    </div>
  );
}
