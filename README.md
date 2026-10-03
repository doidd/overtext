# OverText

Tray app (Tauri v2) that captures a screen region and translates its text into a target
language while keeping the layout.

## Run

```sh
npm install
npm run tauri dev
```

- `⌘⇧1` (or tray menu › *Chụp vùng màn hình*): freeze all monitors, drag to select, `Esc` /
  right-click cancels.
- Result window opens exactly over the selected region and shows the Vietnamese translation
  drawn over the original text. Hover for the toolbar: *Ảnh dịch* / *Văn bản* (Markdown) /
  *Gốc*, *Chép* copies the Markdown. Drag to move, `Esc` or `×` closes.
- First run asks for **Screen Recording** permission (System Settings › Privacy & Security);
  macOS applies it after relaunching the app.
- Settings (tray menu › *Cài đặt…*): target language and translation service. Default is key-less
  (Google Chrome endpoint → Google `gtx` → MyMemory). Or pick an OpenAI-compatible LLM (OpenAI,
  Gemini, Groq, OpenRouter, Ollama or a custom base URL + model). API keys go to the macOS
  Keychain, one per base URL; `settings.json` (app config dir) holds no secrets. If the chosen
  LLM fails the app shows the error and does not fall back to another service. Code blocks are
  never translated. The log shows `ocr: … in …` and `translate: … in …`.
- OCR: on first use macOS compiles Vision's models for the Neural Engine (~60 s, cached per app
  name and macOS build). The app warms them up in the background at launch (`ocr warm-up` log);
  some content can still trigger an extra one-time compile (~30 s).

## Layout

| Path | Role |
|---|---|
| `src-tauri/src/capture.rs` | xcap capture of every monitor, logical → physical crop |
| `src-tauri/src/macos.rs` | Screen Recording permission, cursor position, overlay window level/Spaces |
| `src-tauri/src/lib.rs` | tray, global shortcut, capture session, window lifecycle, commands |
| `src/Selector.tsx` | frozen-screen region selector (one window per monitor) |
| `src-tauri/src/ocr.rs` | Apple Vision text recognition → lines with normalized boxes |
| `src-tauri/src/layout.rs` | lines → blocks (paragraph/heading/list, alignment), text/background colors |
| `src-tauri/src/translate.rs` | providers: key-less (Google Chrome → gtx → MyMemory) or OpenAI-compatible LLM |
| `src-tauri/src/cache.rs` | SQLite persistent translation cache (WAL mode) |
| `src-tauri/src/settings.rs` | settings.json, language list, Keychain API keys |
| `src/Settings.tsx` | settings window: language, provider presets, API key |
| `src/History.tsx` | history window: search, card list, full preview, quick copy |
| `src/Result.tsx` | pinned result: translated overlay with shrink-to-fit, Markdown view, copy |

Captured PNGs live in `$APPCACHE/captures` and are served through the asset protocol; the
scope in `tauri.conf.json` (`app.security.assetProtocol.scope`) must cover that directory.

## Roadmap

| | Milestone | Status |
|---|---|---|
| M0 | Tray, global shortcut, window routing | done |
| M1 | Multi-monitor HiDPI capture, region selector, pinned result | done |
| M2 | OCR via Apple Vision: lines + bounding boxes | done |
| M3 | Block grouping, style extraction, structure (headings, lists) | done (tables pending) |
| M4a | Free translation, target `vi` | done |
| M4b | OpenAI-compatible providers, Keychain keys, settings window, target language | done |
| M4c | Translation cache (SQLite, instant hit, zero network/quota) | done |
| M4d | DeepL, Google Cloud Translation | |
| M5a | Translated image overlay (solid-fill erase) + Markdown text view, copy text | done |
| M5b | Copy/save translated image (native clipboard pasteboard + Save dialog) | done |
| M5c | Translation history window (SQLite persistent storage, thumbnails, search & copy) | done |
| M5d | Inpainting for textured backgrounds, font family matching | |

| M6 | Windows (Windows.Media.Ocr), history, signing/notarization, auto-update | |
