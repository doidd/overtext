import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

import { useAppLocale } from "./useAppLocale";

export type HistoryItem = {
  id: number;
  createdAt: number;
  targetLang: string;
  providerKey: string;
  sourceMarkdown: string;
  translatedMarkdown: string;
  thumbnailBase64: string;
};

export function History() {
  const { t, locale } = useAppLocale();
  const [error, setError] = useState<string | null>(null);
  const [items, setItems] = useState<HistoryItem[]>([]);
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [search, setSearch] = useState("");
  const [copied, setCopied] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const reload = () => {
    setLoading(true);
    invoke<HistoryItem[]>("get_history", { limit: 100 })
      .then((res) => {
        setItems(res);
        if (res.length > 0 && selectedId === null) {
          setSelectedId(res[0].id);
        }
      })
      .catch(e => setError(String(e)))
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    reload();
  }, []);

  const deleteItem = async (id: number) => {
    try { await invoke("delete_history", { id }); } catch (e) { setError(String(e)); return; }
    setItems((prev) => prev.filter((it) => it.id !== id));
    if (selectedId === id) {
      const remaining = items.filter((it) => it.id !== id);
      setSelectedId(remaining.length > 0 ? remaining[0].id : null);
    }
  };

  const clearAll = async () => {
    if (!window.confirm(t.clearConfirm)) return;
    try { await invoke("clear_history"); } catch (e) { setError(String(e)); return; }
    setItems([]);
    setSelectedId(null);
  };

  const copy = async (text: string, type: string) => {
    try { await navigator.clipboard.writeText(text); } catch (e) { setError(String(e)); return; }
    setCopied(type);
    setTimeout(() => setCopied(null), 1500);
  };

  const copyImage = async (base64Png: string) => {
    try { await invoke("copy_image_to_clipboard", { base64Png }); } catch (e) { setError(String(e)); return; }
    setCopied("image");
    setTimeout(() => setCopied(null), 1500);
  };

  const filtered = items.filter((it) => {
    if (!search.trim()) return true;
    const q = search.toLowerCase();
    return (
      it.translatedMarkdown.toLowerCase().includes(q) ||
      it.sourceMarkdown.toLowerCase().includes(q) ||
      it.targetLang.toLowerCase().includes(q) ||
      it.providerKey.toLowerCase().includes(q)
    );
  });

  const selected = items.find((it) => it.id === selectedId);

  const formatDate = (secs: number) => {
    const d = new Date(secs * 1000);
    return `${d.toLocaleDateString(locale)} ${d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })}`;
  };

  return (
    <div className="history-window">
      <div className="history-sidebar">
        <div className="history-search-bar">
          <input
            type="search"
            placeholder={t.searchHistory}
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          {items.length > 0 && (
            <button className="clear-btn" title={t.clearAll} onClick={clearAll}>
              {t.clearAll}
            </button>
          )}
        </div>

        <div className="history-list">
          {error && <p className="error">{t.historyError}: {error}</p>}
          {loading && <p className="muted" style={{ padding: 12 }}>{t.loadingHistory}</p>}
          {!loading && filtered.length === 0 && (
            <p className="muted" style={{ padding: 12 }}>
              {items.length === 0 ? t.emptyHistory : t.noResults}
            </p>
          )}
          {filtered.map((item) => (
            <div
              key={item.id}
              className={`history-card ${item.id === selectedId ? "active" : ""}`}
              onClick={() => setSelectedId(item.id)}
            >
              <div className="history-thumb-wrap">
                {item.thumbnailBase64 ? (
                  <img src={item.thumbnailBase64} alt={t.image} className="history-thumb" />
                ) : (
                  <div className="history-thumb-placeholder">{t.image}</div>
                )}
              </div>
              <div className="history-card-body">
                <div className="history-card-meta">
                  <span className="lang-tag">{item.targetLang.toUpperCase()}</span>
                  <span className="time-tag">{formatDate(item.createdAt)}</span>
                </div>
                <div className="history-card-snippet">{item.translatedMarkdown || item.sourceMarkdown}</div>
              </div>
              <button
                className="delete-card-btn"
                title={t.deleteItem}
                onClick={(e) => {
                  e.stopPropagation();
                  deleteItem(item.id);
                }}
              >
                ×
              </button>
            </div>
          ))}
        </div>
      </div>

      <div className="history-detail">
        {selected ? (
          <div className="history-detail-inner">
            <div className="history-detail-header">
              <div>
                <span className="lang-pill">{t.language}: {selected.targetLang.toUpperCase()}</span>
                <span className="provider-pill">{t.service}: {selected.providerKey}</span>
                <span className="time-pill">{formatDate(selected.createdAt)}</span>
              </div>
              <div className="detail-actions">
                {selected.thumbnailBase64 && (
                  <button onClick={() => copyImage(selected.thumbnailBase64)}>
                    {copied === "image" ? t.copiedImage : t.copyImage}
                  </button>
                )}
                <button onClick={() => copy(selected.translatedMarkdown, "trans")}>
                  {copied === "trans" ? t.copiedTranslation : t.copyTranslation}
                </button>
                <button onClick={() => copy(selected.sourceMarkdown, "source")}>
                  {copied === "source" ? t.copiedSource : t.copySource}
                </button>
              </div>
            </div>

            {selected.thumbnailBase64 && (
              <div className="detail-preview-image">
                <img src={selected.thumbnailBase64} alt={t.image} />
              </div>
            )}

            <div className="detail-markdown-section">
              <h4>{t.translationMarkdown}</h4>
              <pre>{selected.translatedMarkdown}</pre>
            </div>

            <div className="detail-markdown-section">
              <h4>{t.sourceText}</h4>
              <pre>{selected.sourceMarkdown}</pre>
            </div>
          </div>
        ) : (
          <div className="no-selection">
            <p className="muted">{t.selectHistory}</p>
          </div>
        )}
      </div>
    </div>
  );
}
