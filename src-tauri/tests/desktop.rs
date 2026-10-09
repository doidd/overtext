// An explicit test executable lets build.rs link the Windows application
// manifest. Cargo's implicit library test executable cannot receive that link arg.
// These modules are shared with the app crate; items only the app uses are not dead here.
#![allow(dead_code, unused_imports)]

#[path = "../src/cache.rs"]
mod cache;
#[path = "../src/capture.rs"]
mod capture;
#[path = "../src/layout.rs"]
mod layout;
#[path = "../src/ocr.rs"]
mod ocr;
#[path = "../src/settings.rs"]
mod settings;
#[path = "../src/i18n.rs"]
mod i18n;
#[path = "../src/translate.rs"]
mod translate;

#[cfg(target_os = "windows")]
#[path = "support/ocr_benchmark.rs"]
mod ocr_benchmark;

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Installs RapidOCR runtime and downloads/verifies Mobile and Server models"]
fn rapidocr_install_and_switch_models() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/ocr-japanese-list.png");
    ocr::install_rapidocr("mobile").unwrap();
    assert!(ocr::rapidocr_installed("mobile"));
    ocr::install_rapidocr("server").unwrap();
    assert!(ocr::rapidocr_installed("server") && ocr::rapidocr_installed("mobile"));
    let image = image::open(&path).unwrap().to_rgba8();
    for engine in [settings::OcrEngine::RapidMobile, settings::OcrEngine::RapidServer, settings::OcrEngine::RapidMobile] {
        let lines = ocr::recognize_configured(&path, "", &engine).unwrap();
        assert!(lines.len() >= 5);
        let blocks = layout::build_blocks(&lines, &image);
        assert_eq!(blocks.iter().filter(|b| matches!(b.kind, layout::Kind::List)).count(), 4);
    }
    assert!(ocr::recognize_configured(&path, "ko-KR", &settings::OcrEngine::RapidMobile).is_err());
    assert!(ocr::install_rapidocr("invalid").is_err());
    ocr::shutdown_rapidocr();
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Benchmark requires installed Japanese and English Windows OCR packs"]
fn benchmark_native_ocr_on_layout_fixtures() {
    let languages = ocr::available_languages().unwrap();
    let mut cases = Vec::new();
    for (image, prefix) in [
        ("ocr-japanese-card.png", "ja"), ("ocr-japanese.png", "ja"),
        ("ocr-japanese-list.png", "ja"), ("ocr-search-results.png", "en"),
    ] {
        let language = languages.iter().find(|tag| tag.split('-').next() == Some(prefix))
            .unwrap_or_else(|| panic!("Missing Windows OCR pack {prefix}; installed: {languages:?}"));
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets").join(image);
        let mut runs = Vec::new();
        for _ in 0..3 {
            let started = std::time::Instant::now();
            let lines = ocr::recognize(&path, language).unwrap();
            let elapsed = started.elapsed().as_secs_f64() * 1000.0;
            assert!(!lines.is_empty());
            eprintln!("native benchmark: {image} {language} {elapsed:.1}ms {} lines", lines.len());
            runs.push(serde_json::json!({"ms": elapsed, "lines": lines}));
        }
        cases.push(serde_json::json!({"image": image, "language": language, "runs": runs}));
    }
    let logs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("ocr-native-comparison.json"), serde_json::to_string_pretty(&cases).unwrap()).unwrap();
}

#[cfg(target_os = "windows")]
#[test]
#[ignore = "Requires installed PaddleOCR runtime and cached Japanese model"]
fn automatic_ocr_preserves_japanese_card_content() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/ocr-japanese-list.png");
    assert!(ocr::paddleocr_installed());
    let lines = ocr::recognize(&path, "").unwrap();
    let image = image::open(&path).unwrap().to_rgba8();
    let blocks = layout::build_blocks(&lines, &image);
    assert!(blocks.len() >= 5);
    assert_eq!(blocks.iter().filter(|b| matches!(b.kind, layout::Kind::List)).count(), 4);
    assert!(blocks.iter().filter(|b| matches!(b.kind, layout::Kind::List)).all(|b| b.line_count == 1));
    eprintln!("layout preview: {}", serde_json::to_string(&blocks).unwrap());
    let text: String = blocks.iter()
        .filter(|block| !matches!(block.kind, layout::Kind::Code))
        .flat_map(|block| block.text.chars().filter(|c| !c.is_whitespace()))
        .collect();
    eprintln!("Automatic OCR: {} lines, {} blocks; {text}", lines.len(), blocks.len());
    assert!(lines.len() >= 5, "{text}");
    for expected in ["データ加工", "重要度", "集計軸", "共通指摘ID", "分析要件"] {
        assert!(text.contains(expected), "Missing {expected}: {text}");
    }
    ocr::shutdown_paddleocr();
}
