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
#[path = "../src/translate.rs"]
mod translate;

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
