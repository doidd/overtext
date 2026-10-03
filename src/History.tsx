import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

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
      .finally(() => setLoading(false));
  };

  useEffect(() => {
    reload();
  }, []);

  const deleteItem = async (id: number) => {
    await invoke("delete_history", { id });
    setItems((prev) => prev.filter((it) => it.id !== id));
    if (selectedId === id) {
      const remaining = items.filter((it) => it.id !== id);
      setSelectedId(remaining.length > 0 ? remaining[0].id : null);
    }
  };

  const clearAll = async () => {
    if (!window.confirm("Bạn có chắc chắn muốn xóa toàn bộ lịch sử dịch?")) return;
    await invoke("clear_history");
    setItems([]);
    setSelectedId(null);
  };

  const copy = async (text: string, type: string) => {
    await navigator.clipboard.writeText(text);
    setCopied(type);
    setTimeout(() => setCopied(null), 1500);
  };

  const copyImage = async (base64Png: string) => {
    await invoke("copy_image_to_clipboard", { base64Png });
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
    return `${d.toLocaleDateString("vi-VN")} ${d.toLocaleTimeString("vi-VN", { hour: "2-digit", minute: "2-digit" })}`;
  };

  return (
    <div className="history-window">
      <div className="history-sidebar">
        <div className="history-search-bar">
          <input
            type="search"
            placeholder="Tìm kiếm lịch sử..."
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
          {items.length > 0 && (
            <button className="clear-btn" title="Xóa tất cả" onClick={clearAll}>
              Xóa hết
            </button>
          )}
        </div>

        <div className="history-list">
          {loading && <p className="muted" style={{ padding: 12 }}>Đang tải...</p>}
          {!loading && filtered.length === 0 && (
            <p className="muted" style={{ padding: 12 }}>
              {items.length === 0 ? "Chưa có lịch sử dịch nào." : "Không tìm thấy kết quả."}
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
                  <img src={item.thumbnailBase64} alt="Thumb" className="history-thumb" />
                ) : (
                  <div className="history-thumb-placeholder">Ảnh</div>
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
                title="Xóa mục này"
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
                <span className="lang-pill">Ngôn ngữ: {selected.targetLang.toUpperCase()}</span>
                <span className="provider-pill">Dịch vụ: {selected.providerKey}</span>
                <span className="time-pill">{formatDate(selected.createdAt)}</span>
              </div>
              <div className="detail-actions">
                {selected.thumbnailBase64 && (
                  <button onClick={() => copyImage(selected.thumbnailBase64)}>
                    {copied === "image" ? "Đã chép ảnh" : "Chép ảnh"}
                  </button>
                )}
                <button onClick={() => copy(selected.translatedMarkdown, "trans")}>
                  {copied === "trans" ? "Đã chép bản dịch" : "Chép bản dịch"}
                </button>
                <button onClick={() => copy(selected.sourceMarkdown, "source")}>
                  {copied === "source" ? "Đã chép gốc" : "Chép chữ gốc"}
                </button>
              </div>
            </div>

            {selected.thumbnailBase64 && (
              <div className="detail-preview-image">
                <img src={selected.thumbnailBase64} alt="Captured" />
              </div>
            )}

            <div className="detail-markdown-section">
              <h4>Bản dịch (Markdown)</h4>
              <pre>{selected.translatedMarkdown}</pre>
            </div>

            <div className="detail-markdown-section">
              <h4>Nội dung gốc</h4>
              <pre>{selected.sourceMarkdown}</pre>
            </div>
          </div>
        ) : (
          <div className="no-selection">
            <p className="muted">Chọn một mục từ danh sách bên trái để xem chi tiết</p>
          </div>
        )}
      </div>
    </div>
  );
}
