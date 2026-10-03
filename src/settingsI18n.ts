export type UiLanguage = "system" | "vi" | "en" | "ja";
export type Locale = Exclude<UiLanguage, "system">;

const en = {
  title: "Settings — OverText", interfaceLanguage: "Interface language", system: "System language",
  ocrLanguage: "Image language (OCR)", autoOcr: "Automatic — Japanese / Chinese / English", windowsOcr: "Windows language preferences",
  installedOcr: "Installed Windows OCR languages:", none: "None",
  autoHint: "Automatically reads Japanese, Chinese and English. For other languages, select the image language above.",
  windowsHint: "Windows reads installed languages only. Select the image language or install PaddleOCR for Japanese / Chinese / English.",
  fallbackHint: "PaddleOCR will be used when Windows does not have the OCR language pack.",
  sourceHint: "Select the image language independently of the translation language.",
  paddleInstalled: "PaddleOCR is installed. Models download on first use; OCR then runs locally.",
  paddleHint: "Install PaddleOCR for automatic Japanese / Chinese / English recognition and languages missing from Windows. Internet is needed to download the recognizer; images are processed locally.",
  install: "Install PaddleOCR", installing: "Installing PaddleOCR…", installProgress: "Downloading and installing PaddleOCR. This may take a few minutes…",
  installDone: "PaddleOCR installed. Save the source language and capture again.",
  target: "Translation language", service: "Translation service", free: "Free (Google, no API key)", openai: "OpenAI-compatible API (LLM)",
  freeHint: "Uses unofficial Google endpoints with MyMemory as a fallback. Limits or service changes may occur at any time.",
  provider: "Provider", custom: "Custom", local: "local", baseUrl: "Base URL", model: "Model", apiKey: "API key", optional: "(optional)",
  savedKey: "Stored securely — leave blank to keep it", pasteKey: "Paste API key", undoKey: "Undo key deletion", deleteKey: "Delete stored key",
  keyHint: "Captured text is sent to the server above. Keys are stored in Keychain (macOS) or Credential Manager (Windows). If this service fails, the app reports the error without sending text to another service.",
  close: "Close", save: "Save", saving: "Saving…", checking: "Checking connection…", saved: "Saved.", verified: "Saved (connection successful).",
  loading: "Loading settings…", loadError: "Could not load settings", saveError: "Could not save settings", installError: "Could not install PaddleOCR",
};
type Messages = { [K in keyof typeof en]: string };
export type MessageKey = keyof Messages;

const vi: Messages = {
  title: "Cài đặt — OverText", interfaceLanguage: "Ngôn ngữ giao diện", system: "Theo ngôn ngữ hệ thống",
  ocrLanguage: "Ngôn ngữ trong ảnh (OCR)", autoOcr: "Tự động — Nhật / Trung / Anh", windowsOcr: "Theo ngôn ngữ Windows",
  installedOcr: "Ngôn ngữ OCR Windows đã cài:", none: "Chưa có",
  autoHint: "Tự động đọc tiếng Nhật, Trung và Anh. Với ngôn ngữ khác, chọn ngôn ngữ của ảnh bên trên.",
  windowsHint: "Windows chỉ đọc các ngôn ngữ đã cài. Chọn ngôn ngữ của ảnh hoặc cài PaddleOCR để đọc tiếng Nhật / Trung / Anh.",
  fallbackHint: "Ngôn ngữ này sẽ dùng PaddleOCR khi Windows thiếu gói OCR.", sourceHint: "Chọn ngôn ngữ của ảnh, độc lập với ngôn ngữ dịch.",
  paddleInstalled: "PaddleOCR đã cài. Model tải ở lần dùng đầu, sau đó OCR chạy trên máy.",
  paddleHint: "Cài PaddleOCR để đọc tiếng Nhật / Trung / Anh ở chế độ tự động và hỗ trợ ngôn ngữ Windows còn thiếu. Cần mạng để tải bộ nhận dạng; ảnh được xử lý trên máy.",
  install: "Cài PaddleOCR", installing: "Đang cài PaddleOCR…", installProgress: "Đang tải và cài PaddleOCR, có thể mất vài phút…",
  installDone: "Đã cài PaddleOCR. Lưu ngôn ngữ nguồn rồi chụp lại.", target: "Ngôn ngữ đích", service: "Dịch vụ dịch",
  free: "Miễn phí (Google, không cần key)", openai: "API theo chuẩn OpenAI (LLM)",
  freeHint: "Dùng endpoint không chính thức của Google, dự phòng MyMemory. Có thể bị giới hạn hoặc thay đổi bất cứ lúc nào.",
  provider: "Nhà cung cấp", custom: "Tùy chỉnh", local: "chạy trên máy", baseUrl: "URL cơ sở", model: "Model", apiKey: "API key", optional: "(không bắt buộc)",
  savedKey: "Đã lưu an toàn — để trống để giữ nguyên", pasteKey: "Dán API key", undoKey: "Hoàn tác xóa key", deleteKey: "Xóa key đã lưu",
  keyHint: "Văn bản trong ảnh chụp sẽ được gửi tới máy chủ trên. Key lưu trong Keychain (macOS) hoặc Credential Manager (Windows). Khi dịch vụ này lỗi, app báo lỗi và không tự gửi sang dịch vụ khác.",
  close: "Đóng", save: "Lưu", saving: "Đang lưu…", checking: "Đang kiểm tra kết nối…", saved: "Đã lưu.", verified: "Đã lưu (kết nối thành công).",
  loading: "Đang tải cài đặt…", loadError: "Không tải được cài đặt", saveError: "Không lưu được cài đặt", installError: "Không cài được PaddleOCR",
};
const ja: Messages = {
  title: "設定 — OverText", interfaceLanguage: "表示言語", system: "システムの言語",
  ocrLanguage: "画像の言語（OCR）", autoOcr: "自動 — 日本語 / 中国語 / 英語", windowsOcr: "Windows の言語設定",
  installedOcr: "インストール済みの Windows OCR 言語:", none: "なし",
  autoHint: "日本語・中国語・英語を自動認識します。他の言語は上で画像の言語を選択してください。",
  windowsHint: "Windows はインストール済みの言語のみ認識します。画像の言語を選択するか、日本語・中国語・英語用に PaddleOCR をインストールしてください。",
  fallbackHint: "Windows に OCR 言語パックがない場合は PaddleOCR を使用します。", sourceHint: "画像の言語は翻訳先の言語とは別に選択できます。",
  paddleInstalled: "PaddleOCR はインストール済みです。初回使用時にモデルをダウンロードし、その後は端末上で OCR を実行します。",
  paddleHint: "日本語・中国語・英語の自動認識や Windows にない言語には PaddleOCR をインストールしてください。認識エンジンのダウンロードには通信が必要です。画像は端末上で処理されます。",
  install: "PaddleOCR をインストール", installing: "PaddleOCR をインストール中…", installProgress: "PaddleOCR をダウンロード・インストール中です。数分かかることがあります…",
  installDone: "PaddleOCR をインストールしました。画像の言語を保存して再度キャプチャしてください。",
  target: "翻訳先の言語", service: "翻訳サービス", free: "無料（Google、API キー不要）", openai: "OpenAI 互換 API（LLM）",
  freeHint: "非公式の Google エンドポイントを使用し、MyMemory にフォールバックします。制限やサービス変更が発生する場合があります。",
  provider: "プロバイダー", custom: "カスタム", local: "ローカル", baseUrl: "ベース URL", model: "モデル", apiKey: "API キー", optional: "（任意）",
  savedKey: "安全に保存済み — 空欄で保持", pasteKey: "API キーを貼り付け", undoKey: "キーの削除を取り消す", deleteKey: "保存済みのキーを削除",
  keyHint: "キャプチャしたテキストは上記のサーバーに送信されます。キーは Keychain（macOS）または資格情報マネージャー（Windows）に保存されます。サービスに障害が発生した場合、エラーを表示し、別のサービスには送信しません。",
  close: "閉じる", save: "保存", saving: "保存中…", checking: "接続を確認中…", saved: "保存しました。", verified: "保存しました（接続成功）。",
  loading: "設定を読み込み中…", loadError: "設定を読み込めませんでした", saveError: "設定を保存できませんでした", installError: "PaddleOCR をインストールできませんでした",
};
export const messages: Record<Locale, Messages> = { en, vi, ja };

export function resolveLocale(choice: string, systemLanguages: readonly string[]): Locale {
  if (choice === "vi" || choice === "en" || choice === "ja") return choice;
  for (const tag of systemLanguages) {
    const language = tag.toLowerCase().split("-")[0];
    if (language === "vi" || language === "en" || language === "ja") return language;
  }
  return "en";
}

export function languageName(code: string, fallback: string, locale: Locale): string {
  try {
    return new Intl.DisplayNames([locale], { type: "language" }).of(code) ?? fallback;
  } catch {
    return fallback;
  }
}

// Windows may report "ja" for a language selected as "ja-JP".
export function nativeOcrAvailable(code: string, installed: readonly string[]): boolean {
  const requested = code.toLowerCase();
  return installed.some((tag) => requested === tag.toLowerCase() || requested.startsWith(`${tag.toLowerCase()}-`));
}
