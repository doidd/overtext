# OverText

Tray app (Tauri v2) for macOS and Windows that captures a screen region and translates
its text into a target language while keeping the layout.

## Run

```sh
npm install
npm run tauri dev
```

- `⌘⇧1` on macOS / `Ctrl+Shift+1` on Windows (or tray menu › *Chụp vùng màn hình*): freeze all monitors, drag to select, `Esc` /
  right-click cancels.
- Result window opens exactly over the selected region and shows the Vietnamese translation
  drawn over the original text. Hover for the toolbar: *Ảnh dịch* / *Văn bản* (Markdown) /
  *Gốc*, *Chép* copies the Markdown. Drag to move, `Esc` or `×` closes.
- On macOS, first run asks for **Screen Recording** permission (System Settings › Privacy & Security);
  macOS applies it after relaunching the app.
- Settings (tray menu › *Cài đặt…*): interface language (system / Vietnamese / English / Japanese), target language and translation service. Interface language is saved separately from OCR and translation; unsupported system languages fall back to English. Default translation is key-less
  (Google Chrome endpoint → Google `gtx` → MyMemory). Or pick an OpenAI-compatible LLM (OpenAI,
  Gemini, Groq, OpenRouter, Ollama or a custom base URL + model). API keys go to macOS
  Keychain or Windows Credential Manager, one per base URL; `settings.json` (app config dir) holds no secrets. If the chosen
  LLM fails the app shows the error and does not fall back to another service. Code blocks are
  never translated. The log shows `ocr: … in …` and `translate: … in …`.
- OCR: on first use macOS compiles Vision's models for the Neural Engine (~60 s, cached per app
  name and macOS build). The app warms them up in the background at launch (`ocr warm-up` log);
  some content can still trigger an extra one-time compile (~30 s).

## Windows

- Requires Windows 10/11 and Microsoft Edge WebView2 Runtime.
- For development, install Rust (stable MSVC), Visual Studio Build Tools with
  **Desktop development with C++**, and a Windows SDK. Then run the commands above.
- OCR uses `Windows.Media.Ocr` locally. Choose the source language in
  **Cài đặt › Ngôn ngữ trong ảnh (OCR)**. The app lists installed OCR languages and
  reports missing language packs rather than recognizing Japanese with an English model.
  With PaddleOCR installed, the default reads Japanese, Chinese, and English with
  PaddleOCR's shared recognizer. Otherwise it follows Windows language preferences,
  then the first installed OCR language; an English pack cannot read Japanese.
  Install the **Optical character recognition** language feature for the source language
  in **Settings › Time & language › Language & region**. The translation target language
  is independent of the OCR language. Windows OCR does not automatically detect arbitrary
  source languages; select Japanese (`ja-JP`) for Japanese screenshots. When the selected
  language pack is missing, OverText falls back to PaddleOCR (see below). Small images
  are enlarged up to 3× before OCR to improve recognition of tiny text.
- Screenshots and window placement use physical desktop coordinates; selections and
  translated text use CSS pixels. This supports scaled monitors and negative desktop origins.
- Image copy uses the native Windows clipboard; image save opens a native PNG save dialog.
- Build installers with `npm run tauri build` (NSIS/MSI). Signing and auto-update remain pending.

### PaddleOCR fallback on Windows

In Settings, click **Cài PaddleOCR** once, select the source language (for example
Japanese `ja-JP`), and save. Installation uses an isolated Python 3.12 CPU runtime in
`%LOCALAPPDATA%/OverText/paddleocr`; it needs internet but no administrator rights,
and does not modify system Python or PATH. Developers can also run `npm run ocr:setup`.

With an explicit source language, Windows OCR remains the first choice if it supports that language.
If that language is unavailable (or no native OCR packs are installed), PaddleOCR
receives the same image and returns normalized line boxes for the existing overlay.
Other native errors are reported directly. With the default source language and
PaddleOCR installed, the app uses its Japanese/Chinese/English recognizer even when
Windows has an English OCR pack. Select other source languages explicitly.

PaddleOCR 3.3.2 / PaddlePaddle 3.2.2 are pinned. Japanese uses PP-OCRv5 mobile detection
and server recognition, with a shared Japanese/Chinese/English recognizer. Models
download on first use into the runtime's `models` directory; recognition then runs
locally. Translation still uses the configured translation provider. The worker keeps
the active model in memory, serializes concurrent requests, and restarts after errors
or a 180-second timeout. It is stopped on normal app exit. Setup and inference logs are
`install.log` and `worker.log` in the runtime directory.
`worker.log` also records `paddleocr timing` JSON for each request: import/model
initialization, image decode/resize, detection, recognition, and total milliseconds,
plus input/processed sizes and region counts. It does not log recognized text.
The app stderr logs the matching request ID, worker reuse, queue wait, roundtrip,
and separate image decode/OCR/layout timings. Detection/recognition timings include
each model's preprocessing, inference, and postprocessing.
CPU inference enables MKL-DNN by default with four threads. Set
`OVERTEXT_OCR_MKLDNN=0` before launching the app to disable it for troubleshooting.
`python scripts/benchmark-paddleocr.py` compares both modes on the two Japanese
fixtures (three requests each), saving timings and OCR output in
`logs/mkldnn-comparison.json`.

The app includes the worker and setup scripts, but the Python packages/models must be
installed once on each machine; they are not bundled into the installer.

## Verification

```sh
npm run build
python scripts/test-paddleocr-worker.py
cargo test --manifest-path src-tauri/Cargo.toml --test desktop --locked
npm run tauri build -- --no-bundle
```

The Windows OCR test recognizes the bundled warm-up image and checks its line boxes;
it requires an installed OCR language that can read that image (for example English).
The explicit desktop test executable embeds the Windows manifest required by native controls.
After installing Japanese OCR, run the Japanese slide regression with
`cargo test --manifest-path src-tauri/Cargo.toml --test desktop recognizes_small_japanese -- --ignored`.
To verify the actual fallback after installing PaddleOCR (without a Windows Japanese pack), run
`cargo test --manifest-path src-tauri/Cargo.toml --test desktop japanese_fallback -- --ignored --nocapture`.
The supplied Japanese card regression verifies default OCR and block grouping with
`cargo test --manifest-path src-tauri/Cargo.toml --test desktop automatic_ocr_preserves_japanese_card_content -- --ignored --nocapture`.
Desktop CI runs on Windows and macOS. Manual checks: capture with `Ctrl+Shift+1`, cancel
with Escape/right-click, translate a region, copy text/image, save/cancel the PNG dialog,
reopen history, and save/relaunch/remove a provider key. Verify placement on monitors at
100%, 125%, and 150%, including a secondary monitor to the left of the primary.

## Layout

| Path | Role |
|---|---|
| `src-tauri/src/capture.rs` | xcap capture of every monitor, logical → physical crop |
| `src-tauri/src/macos.rs` | Screen Recording permission, cursor position, overlay window level/Spaces |
| `src-tauri/src/windows.rs` | cursor position, native image clipboard, PNG save dialog |
| `src-tauri/src/lib.rs` | tray, global shortcut, capture session, window lifecycle, commands |
| `src/Selector.tsx` | frozen-screen region selector (one window per monitor) |
| `src-tauri/src/ocr.rs` | Apple Vision / Windows OCR → lines with normalized boxes |
| `src-tauri/src/layout.rs` | lines → blocks (paragraph/heading/list, alignment), text/background colors |
| `src-tauri/src/translate.rs` | providers: key-less (Google Chrome → gtx → MyMemory) or OpenAI-compatible LLM |
| `src-tauri/src/cache.rs` | SQLite persistent translation cache (WAL mode) |
| `src-tauri/src/settings.rs` | settings.json, language list, Keychain / Credential Manager API keys |
| `src/Settings.tsx` | settings window: language, provider presets, API key |
| `src/History.tsx` | history window: search, card list, full preview, quick copy |
| `src/Result.tsx` | pinned result: shared rendered image, Markdown view, copy |
| `src/renderImage.ts` | source-region erase, glyph alignment and fitting shared by result/copy/save/history |

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

| M6a | Windows OCR, DPI-aware capture placement, image copy/save, Credential Manager | implemented |
| M6b | Signing/notarization, auto-update | |

## License

[MIT](LICENSE)
