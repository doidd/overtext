use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// ISO 639-1 code → English name (the name is what goes into LLM prompts).
pub const LANGUAGES: [(&str, &str); 16] = [
    ("vi", "Vietnamese"),
    ("en", "English"),
    ("ja", "Japanese"),
    ("ko", "Korean"),
    ("zh-CN", "Simplified Chinese"),
    ("zh-TW", "Traditional Chinese"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("pt", "Portuguese"),
    ("it", "Italian"),
    ("ru", "Russian"),
    ("th", "Thai"),
    ("id", "Indonesian"),
    ("ar", "Arabic"),
    ("hi", "Hindi"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    /// Key-less Google/MyMemory endpoints.
    #[default]
    Free,
    /// Any OpenAI-compatible `/chat/completions` server (OpenAI, Gemini, Groq, OpenRouter, Ollama…).
    Openai,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Settings interface language; independent of OCR and translation.
    pub ui_lang: String,
    pub target_lang: String,
    /// Windows OCR source language (BCP-47); empty prefers installed PaddleOCR
    /// for Japanese/Chinese/English, otherwise uses Windows preferences.
    pub ocr_lang: String,
    pub provider: Provider,
    pub base_url: String,
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_lang: "system".into(),
            target_lang: "vi".into(),
            ocr_lang: String::new(),
            provider: Provider::Free,
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
        }
    }
}

impl Settings {
    pub fn language_name(&self) -> &'static str {
        LANGUAGES.iter().find(|(c, _)| *c == self.target_lang).map_or("Vietnamese", |(_, n)| n)
    }

    pub fn provider_key(&self) -> String {
        match self.provider {
            Provider::Free => "free".into(),
            Provider::Openai => format!("openai:{}:{}", self.base_url, self.model),
        }
    }
    /// Normalizes user input; rejects values that would produce malformed requests.
    pub fn validated(mut self) -> Result<Self, String> {
        let locale = crate::i18n::resolve(&self.ui_lang, &[crate::i18n::system_locale().to_owned()]);
        if !["system", "vi", "en", "ja"].contains(&self.ui_lang.as_str()) {
            return Err(crate::i18n::text(locale, "invalidUi").into());
        }
        self.ocr_lang = self.ocr_lang.trim().to_owned();
        if self.ocr_lang.len() > 64 || !self.ocr_lang.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            return Err(crate::i18n::text(locale, "invalidOcr").into());
        }
        if !LANGUAGES.iter().any(|(c, _)| *c == self.target_lang) {
            return Err(format!("{}: {}", crate::i18n::text(locale, "unsupportedTarget"), self.target_lang));
        }
        self.base_url = self.base_url.trim().trim_end_matches('/').to_owned();
        self.model = self.model.trim().to_owned();
        if self.provider == Provider::Openai {
            if !(self.base_url.starts_with("https://") || self.base_url.starts_with("http://")) {
                return Err(crate::i18n::text(locale, "invalidUrl").into());
            }
            if self.model.is_empty() {
                return Err(crate::i18n::text(locale, "missingModel").into());
            }
        }
        Ok(self)
    }
}

fn path(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_config_dir().map(|d| d.join("settings.json")).map_err(|e| e.to_string())
}

/// Missing or unreadable file falls back to defaults so a corrupt file never blocks startup.
pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .ok()
        .and_then(|p| fs::read(p).ok())
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = path(app)?;
    fs::create_dir_all(path.parent().expect("file path has a parent")).map_err(|e| e.to_string())?;
    fs::write(&path, serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// API keys live in Keychain / Windows Credential Manager, never in `settings.json`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod keychain {
    const SERVICE: &str = "com.overtext.app";

    fn entry(base_url: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, base_url).map_err(|e| e.to_string())
    }

    pub fn get(base_url: &str) -> Result<Option<String>, String> {
        match entry(base_url)?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    pub fn set(base_url: &str, key: &str) -> Result<(), String> {
        let entry = entry(base_url)?;
        if key.is_empty() {
            return match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(e) => Err(e.to_string()),
            };
        }
        entry.set_password(key).map_err(|e| e.to_string())
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod keychain {
    pub fn get(_: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    pub fn set(_: &str, _: &str) -> Result<(), String> {
        Err("API key storage is only implemented on macOS and Windows".into())
    }
}

pub use keychain::{get as api_key, set as set_api_key};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_or_old_settings_files_fill_in_defaults() {
        let s: Settings = serde_json::from_str(r#"{"targetLang":"ja"}"#).unwrap();
        assert_eq!((s.target_lang.as_str(), s.provider), ("ja", Provider::Free));
        assert_eq!(s.model, "gpt-4o-mini");
        assert!(s.ocr_lang.is_empty());
        assert_eq!(s.ui_lang, "system");
    }

    #[test]
    fn interface_language_round_trip_is_independent_of_translation_and_ocr() {
        let settings = Settings { ui_lang: "ja".into(), target_lang: "vi".into(), ocr_lang: "en-US".into(), ..Settings::default() }.validated().unwrap();
        let restored: Settings = serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(restored.ui_lang, "ja");
        assert_eq!(restored.target_lang, "vi");
        assert_eq!(restored.ocr_lang, "en-US");
        assert!(Settings { ui_lang: "xx".into(), ..Settings::default() }.validated().is_err());
    }

    #[test]
    fn validation_normalizes_and_rejects_bad_openai_config() {
        let ok = Settings { provider: Provider::Openai, base_url: " http://localhost:11434/v1/ ".into(), model: " m ".into(), ..Settings::default() };
        let ok = ok.validated().unwrap();
        assert_eq!((ok.base_url.as_str(), ok.model.as_str()), ("http://localhost:11434/v1", "m"));

        let bad_url = Settings { provider: Provider::Openai, base_url: "ftp://x".into(), ..Settings::default() };
        assert!(bad_url.validated().is_err());
        let bad_lang = Settings { target_lang: "xx".into(), ..Settings::default() };
        assert!(bad_lang.validated().is_err());
        // Free provider ignores the OpenAI fields entirely.
        let free = Settings { base_url: "garbage".into(), model: String::new(), ..Settings::default() };
        assert!(free.validated().is_ok());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_credentials_round_trip_and_delete_without_touching_provider_keys() {
        let id = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let url = format!("https://overtext-test.invalid/{}/{id}", std::process::id());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = set_api_key(&self.0, "");
            }
        }
        let _cleanup = Cleanup(url.clone());
        assert!(api_key(&url).unwrap().is_none());
        set_api_key(&url, "overtext-test-key").unwrap();
        assert_eq!(api_key(&url).unwrap().as_deref(), Some("overtext-test-key"));
        set_api_key(&url, "").unwrap();
        assert!(api_key(&url).unwrap().is_none());
        set_api_key(&url, "").unwrap();
    }
}
