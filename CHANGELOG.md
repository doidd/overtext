# Changelog

## 0.1.0 (pre-release)

First public build. macOS (Apple Silicon, macOS 13+) and Windows 10/11.

### Features
- Capture a screen region with `⌘⇧1` / `Ctrl+Shift+1` across multiple HiDPI monitors.
- Local OCR. macOS: Apple Vision. Windows: **RapidOCR Mobile by default** (ONNX, CPU; optional
  Server model), with Windows OCR and PaddleOCR as explicit alternatives.
- Layout-preserving translation drawn over the original text (solid-fill erase, auto-fit), plus a
  Markdown text view. Code blocks are detected and left untranslated.
- Translation services: key-less (Google endpoints, MyMemory fallback) or any OpenAI-compatible API
  (OpenAI, Gemini, Groq, OpenRouter, Ollama, custom). The connection is verified when saving. API
  keys live in the macOS Keychain / Windows Credential Manager.
- Persistent translation cache and a searchable translation history.
- Copy or save the translated image; copy the translated text.
- Interface in Vietnamese, English and Japanese, independent of the OCR and translation languages.

### Known limitations
- **Not code-signed or notarized.** macOS Gatekeeper and Windows SmartScreen will warn on first launch.
- The macOS Screen Recording permission may be requested again after updating, because the ad-hoc
  signature changes with each build.
- On first use macOS compiles Vision's models (30–100 s); later runs are fast.
- The key-less translation endpoints are unofficial and may be rate limited or change.
- Windows: the OCR engine (RapidOCR/PaddleOCR) is not bundled; install it from Settings (needs internet,
  no administrator rights). Until then capture reports a missing installation. The Windows build has
  had less real-device testing than macOS.
- Apple Silicon only (no Intel Mac build).
- No tables or inpainting for textured backgrounds yet; no automatic updates.
