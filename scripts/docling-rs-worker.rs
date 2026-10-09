//! Isolated benchmark worker. This is deliberately outside the Tauri binary.
use docling_pdf::{OcrLang, Pipeline};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::time::Instant;

#[allow(dead_code)]
#[path = "../src-tauri/src/layout.rs"]
mod layout;
mod ocr {
    #[derive(serde::Deserialize)]
    pub struct OcrLine {
        pub text: String,
        pub x: f64,
        pub y: f64,
        pub width: f64,
        pub height: f64,
    }
}

fn emit(value: Value) {
    println!("{value}");
    io::stdout().flush().expect("worker stdout closed");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let models = std::path::PathBuf::from(std::env::var("DOCLING_RS_MODELS_DIR")?);
    for name in [
        "layout_heron_int8.onnx",
        "ocr_det.onnx",
        "ocr_rec_v6.onnx",
        "ocr_rec_v6_dict.txt",
    ] {
        if !models.join(name).is_file() {
            return Err(format!("Required prototype model is missing: {name}").into());
        }
    }
    if !std::path::Path::new(&std::env::var("ORT_DYLIB_PATH")?).is_file() {
        return Err("ORT_DYLIB_PATH must point to an existing ONNX Runtime DLL".into());
    }
    // Models are lazy: ready means the protocol is ready, not warmed inference.
    let mut pipeline = Pipeline::new()?.no_table_former(true);
    emit(
        json!({"event":"ready", "protocol":1, "engine":"docling.rs", "version":"1.104.2", "warmed":false}),
    );
    for line in io::stdin().lock().lines() {
        let request: Value = match serde_json::from_str(&line?) {
            Ok(value) => value,
            Err(error) => {
                emit(json!({"id":null,"error":error.to_string()}));
                continue;
            }
        };
        if request["command"] == "shutdown" {
            break;
        }
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let started = Instant::now();
        let result = (|| -> Result<Value, Box<dyn std::error::Error>> {
            let path = request["image"].as_str().ok_or("image path is required")?;
            if request["command"] == "rapid-layout" {
                let image = image::open(path)?.to_rgba8();
                let lines: Vec<ocr::OcrLine> = serde_json::from_value(request["lines"].clone())?;
                return Ok(
                    json!({"width":image.width(),"height":image.height(),"blocks":layout::build_blocks(&lines, &image)}),
                );
            }
            let language = request["language"].as_str().unwrap_or("en");
            let language = OcrLang::parse(language)
                .ok_or("Docling.rs 1.104.2 exposes only en/ch recognition; unsupported language")?;
            pipeline.set_ocr_lang(Some(language));
            let bytes = std::fs::read(path)?;
            let image = image::load_from_memory(&bytes)?;
            let document = pipeline.convert_image(&bytes, path)?;
            Ok(
                json!({"width":image.width(), "height":image.height(), "document":document.export_to_json_value()}),
            )
        })();
        match result {
            Ok(mut value) => {
                value["id"] = id;
                value["total_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                emit(value);
            }
            Err(error) => emit(
                json!({"id":id,"error":error.to_string(),"total_ms":started.elapsed().as_secs_f64()*1000.0}),
            ),
        }
    }
    Ok(())
}
