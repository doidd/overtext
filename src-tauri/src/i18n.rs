use std::sync::{atomic::{AtomicU8, Ordering}, OnceLock};

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn resolve(choice: &str, languages: &[String]) -> &'static str {
    match choice {
        "en" => return "en",
        "vi" => return "vi",
        "ja" => return "ja",
        _ => {}
    }
    for tag in languages {
        match tag.to_lowercase().split(['-', '_']).next().unwrap_or("") {
            "en" => return "en",
            "vi" => return "vi",
            "ja" => return "ja",
            _ => {}
        }
    }
    "en"
}

pub fn system_locale() -> &'static str {
    static SYSTEM: OnceLock<&'static str> = OnceLock::new();
    SYSTEM.get_or_init(|| resolve("system", &system_languages()))
}

fn system_languages() -> Vec<String> {
    #[cfg(target_os = "windows")]
    if let Ok(languages) = windows::Globalization::ApplicationLanguages::Languages() {
        return (0..languages.Size().unwrap_or(0))
            .filter_map(|i| languages.GetAt(i).ok().map(|s| s.to_string())).collect();
    }
    #[cfg(target_os = "macos")]
    if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"]).output()
    {
        if output.status.success() {
            return String::from_utf8_lossy(&output.stdout).split([',', '\n'])
                .map(|s| s.trim().trim_matches(['(', ')', '"']).trim().to_owned())
                .filter(|s| !s.is_empty()).collect();
        }
    }
    ["LC_ALL", "LC_MESSAGES", "LANG"].iter()
        .filter_map(|key| std::env::var(key).ok()).collect()
}

pub fn set_locale(locale: &str) {
    CURRENT.store(match locale { "vi" => 1, "ja" => 2, _ => 0 }, Ordering::Relaxed);
}

pub fn text(locale: &str, key: &str) -> &'static str {
    static CATALOG: OnceLock<serde_json::Value> = OnceLock::new();
    let catalog = CATALOG.get_or_init(|| serde_json::from_str(include_str!("../../src/locales/messages.json")).expect("valid UI catalog"));
    catalog.get(locale).and_then(|m| m.get(key)).and_then(|v| v.as_str())
        .or_else(|| catalog["en"].get(key).and_then(|v| v.as_str())).expect("known UI message")
}

pub fn current(key: &str) -> &'static str {
    text(match CURRENT.load(Ordering::Relaxed) { 1 => "vi", 2 => "ja", _ => "en" }, key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_resolves_overrides_regions_and_fallback() {
        assert_eq!(resolve("ja", &["vi-VN".into()]), "ja");
        assert_eq!(resolve("system", &["VI-vn".into()]), "vi");
        assert_eq!(resolve("system", &["ja_JP".into()]), "ja");
        assert_eq!(resolve("system", &["de-DE".into()]), "en");
    }

    #[test]
    fn native_and_web_ui_share_complete_catalogs() {
        let catalog: serde_json::Value = serde_json::from_str(include_str!("../../src/locales/messages.json")).unwrap();
        for locale in ["en", "vi", "ja"] {
            assert_eq!(catalog[locale].as_object().unwrap().len(), catalog["en"].as_object().unwrap().len());
            for key in catalog["en"].as_object().unwrap().keys() {
                assert!(!text(locale, key).is_empty());
            }
        }
        assert_eq!(text("en", "capture"), "Capture screen region");
        assert_eq!(text("ja", "settingsMenu"), "設定…");
    }
}
