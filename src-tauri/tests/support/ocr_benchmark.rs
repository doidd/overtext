//! Opt-in corpus benchmark: calls the same engines and layout as the app.
use crate::{layout, ocr, settings::OcrEngine};
use serde_json::{json, Value};
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "Requires prepared corpus, Windows language packs and installed Rapid models"]
fn benchmark_ocr_corpus() {
    let manifest_path = PathBuf::from(
        std::env::var("OVERTEXT_BENCHMARK_MANIFEST").expect("set OVERTEXT_BENCHMARK_MANIFEST"),
    );
    let output = PathBuf::from(
        std::env::var("OVERTEXT_BENCHMARK_OUTPUT").expect("set OVERTEXT_BENCHMARK_OUTPUT"),
    );
    let input: Value = serde_json::from_slice(&std::fs::read(manifest_path).unwrap()).unwrap();
    assert_eq!(input["version"], 1);
    let repeats = std::env::var("OVERTEXT_BENCHMARK_REPEATS")
        .unwrap_or_else(|_| "6".into())
        .parse::<usize>()
        .unwrap();
    assert!(
        repeats >= 3,
        "need an initial request and at least two warm repeats"
    );
    let requested = std::env::var("OVERTEXT_BENCHMARK_ENGINES")
        .unwrap_or_else(|_| "windows,rapid-mobile,rapid-server".into());
    let engines: Vec<_> = requested
        .split(',')
        .map(|name| match name.trim() {
            "windows" => ("windows", OcrEngine::Windows),
            "rapid-mobile" => ("rapid-mobile", OcrEngine::RapidMobile),
            "rapid-server" => ("rapid-server", OcrEngine::RapidServer),
            other => panic!("unsupported benchmark engine: {other}"),
        })
        .collect();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let languages = ocr::available_languages().unwrap();
    let mut report = json!({"version":1,"repeats":repeats,"windows_languages":languages,"corpus":input,"results":[]});
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    for (name, engine) in engines {
        // A new resident Rapid worker per model; never silently switch engines.
        ocr::shutdown_rapidocr();
        for case in report["corpus"]["cases"].as_array().unwrap().clone() {
            let id = case["id"].as_str().unwrap();
            let path = root.join(case["image"].as_str().unwrap());
            let image = image::open(&path).unwrap().to_rgba8();
            let language = case["language"].as_str().unwrap();
            let mut runs = Vec::new();
            for repeat in 0..repeats {
                let start = Instant::now();
                let result = ocr::recognize_configured(&path, language, &engine);
                let ocr_ms = start.elapsed().as_secs_f64() * 1000.0;
                let run = match result {
                    Ok(lines) => {
                        let layout_start = Instant::now();
                        let blocks = layout::build_blocks(&lines, &image);
                        json!({"repeat":repeat,"ocr_ms":ocr_ms,"layout_ms":layout_start.elapsed().as_secs_f64()*1000.0,
                               "total_ms":start.elapsed().as_secs_f64()*1000.0,"lines":lines,"blocks":blocks})
                    }
                    Err(error) => json!({"repeat":repeat,"ocr_ms":ocr_ms,"error":error}),
                };
                eprintln!(
                    "corpus benchmark: {name} {id} {}/{repeats} {ocr_ms:.0}ms{}",
                    repeat + 1,
                    if run.get("error").is_some() {
                        " ERROR"
                    } else {
                        ""
                    }
                );
                let failed = run.get("error").is_some();
                runs.push(run);
                if failed {
                    break;
                }
            }
            report["results"]
                .as_array_mut()
                .unwrap()
                .push(json!({"case":id,"engine":name,"runs":runs}));
            std::fs::write(&output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
    }
    ocr::shutdown_rapidocr();
    assert!(!report["results"].as_array().unwrap().is_empty());
}
