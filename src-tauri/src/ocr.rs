use std::path::Path;

use serde::{Deserialize, Serialize};

/// One recognized text line. Geometry is normalized to the image (0..1), top-left origin.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Text image recognized at startup so the first real capture does not pay for
/// Vision's one-time model preparation.
const WARMUP_PNG: &[u8] = include_bytes!("../assets/ocr-warmup.png");

#[cfg(target_os = "macos")]
pub fn recognize(path: &Path, _language: &str) -> Result<Vec<OcrLine>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    use objc2_vision::VNImageRequestHandler;

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // SAFETY: empty options dictionary of the documented key/value types.
    let handler = unsafe {
        VNImageRequestHandler::initWithURL_options(
            VNImageRequestHandler::alloc(),
            &url,
            &NSDictionary::new(),
        )
    };
    vision::recognize(&handler)
}

/// macOS compiles Vision's core text models for the Neural Engine on first use (~60 s),
/// caching them in `~/Library/Caches/<process name>/com.apple.e5rt.e5bundlecache/<OS build>`,
/// and loads them per process (~0.3 s). One small recognition at launch moves both costs
/// off the first capture. Some content later triggers extra models (~30 s once each).
#[cfg(target_os = "macos")]
pub fn warm_up() -> Result<usize, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSData, NSDictionary};
    use objc2_vision::VNImageRequestHandler;

    let data = NSData::with_bytes(WARMUP_PNG);
    let handler = VNImageRequestHandler::initWithData_options(
        VNImageRequestHandler::alloc(),
        &data,
        &NSDictionary::new(),
    );
    vision::recognize(&handler).map(|lines| lines.len())
}

#[cfg(target_os = "macos")]
mod vision {
    use objc2_foundation::NSArray;
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
    };

    use super::OcrLine;

    pub fn recognize(handler: &VNImageRequestHandler) -> Result<Vec<OcrLine>, String> {
        let request = VNRecognizeTextRequest::new();
        request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
        request.setUsesLanguageCorrection(true);
        request.setAutomaticallyDetectsLanguage(true);

        let requests = NSArray::from_slice(&[request.as_ref() as &VNRequest]);
        handler
            .performRequests_error(&requests)
            .map_err(|e| e.localizedDescription().to_string())?;

        let Some(results) = request.results() else {
            return Ok(Vec::new());
        };
        Ok(results
            .iter()
            .filter_map(|obs| {
                let text = obs.topCandidates(1).firstObject()?.string().to_string();
                // SAFETY: plain struct getter on a live observation.
                let b = unsafe { obs.boundingBox() };
                Some(OcrLine {
                    text,
                    x: b.origin.x,
                    y: 1.0 - b.origin.y - b.size.height,
                    width: b.size.width,
                    height: b.size.height,
                })
            })
            .filter(|l| !l.text.trim().is_empty())
            .collect())
    }
}

#[cfg(all(target_os = "windows", test))]
pub fn recognize(path: &Path, language: &str) -> Result<Vec<OcrLine>, String> {
    let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
    // An installed English Windows pack says nothing about the screenshot's
    // language. The default uses Paddle's Japanese/Chinese/English model when
    // available, rather than silently reading Japanese with an English model.
    let automatic_paddle = language.is_empty() && paddle::installed();
    route_windows_ocr(
        !automatic_paddle && windows_ocr::language_supported(language)?,
        || windows_ocr::recognize(image, language),
        || {
            eprintln!("ocr engine: PaddleOCR; source language: {language:?}");
            paddle::recognize(path, language).map_err(|e| {
                if automatic_paddle {
                    return format!("PaddleOCR: {e}");
                }
                format!("{} ({language}) {e}", crate::i18n::current("missingOcr"))
            })
        },
    )
}

#[cfg(target_os = "windows")]
#[path = "paddle.rs"]
mod paddle;

#[cfg(target_os = "windows")]
#[path = "rapid.rs"]
mod rapid;

#[cfg(target_os = "windows")]
pub use rapid::{install as install_rapidocr, installed as rapidocr_installed, shutdown as shutdown_rapidocr};

pub fn warm_up_configured(engine: &crate::settings::OcrEngine) -> Result<usize, String> {
    #[cfg(target_os = "windows")]
    if let Some(model) = engine.rapid_model() {
        return rapid::warm_up(model);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = engine;
    warm_up()
}

pub fn recognize_configured(path: &Path, language: &str, engine: &crate::settings::OcrEngine) -> Result<Vec<OcrLine>, String> {
    #[cfg(target_os = "windows")]
    {
        if let Some(model) = engine.rapid_model() {
            let tag = language.split('-').next().unwrap_or_default().to_lowercase();
            if !["", "ja", "zh", "en"].contains(&tag.as_str()) {
                return Err(crate::i18n::current("rapidLanguage").into());
            }
            eprintln!("ocr engine: RapidOCR {model}/ONNX CPU; source language: {language:?}");
            return rapid::recognize(path, model);
        }
        if matches!(engine, crate::settings::OcrEngine::Paddle) {
            return paddle::recognize(path, language);
        }
        if !windows_ocr::language_supported(language)? {
            return Err(crate::i18n::current("missingOcr").into());
        }
        let image = image::open(path).map_err(|e| e.to_string())?.to_rgba8();
        windows_ocr::recognize(image, language)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = engine;
        recognize(path, language)
    }
}

#[cfg(target_os = "windows")]
#[cfg_attr(test, allow(unused_imports))]
pub use paddle::{
    install as install_paddleocr, installed as paddleocr_installed, shutdown as shutdown_paddleocr,
};

#[cfg(all(target_os = "windows", test))]
fn route_windows_ocr<T>(
    supported: bool,
    native: impl FnOnce() -> Result<T, String>,
    fallback: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if supported {
        native()
    } else {
        fallback()
    }
}

#[cfg(all(test, target_os = "windows"))]
mod routing_tests {
    use super::*;

    #[test]
    fn missing_language_uses_fallback_and_supported_language_uses_windows() {
        assert_eq!(
            route_windows_ocr(
                false,
                || panic!("must not OCR in a different language"),
                || Ok(42)
            ),
            Ok(42)
        );
        assert_eq!(
            route_windows_ocr(true, || Ok(7), || panic!("native language available")),
            Ok(7)
        );
        // Genuine native failures do not silently switch backends.
        assert_eq!(
            route_windows_ocr::<u8>(
                true,
                || Err("bad image".into()),
                || panic!("not a missing language")
            ),
            Err("bad image".into())
        );
        assert_eq!(
            route_windows_ocr::<u8>(false, || panic!(), || Err("missing runtime".into())),
            Err("missing runtime".into())
        );
    }
}

#[cfg(target_os = "windows")]
pub fn warm_up() -> Result<usize, String> {
    let image = image::load_from_memory(WARMUP_PNG)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    windows_ocr::recognize(image, "").map(|lines| lines.len())
}

#[cfg(target_os = "windows")]
pub fn available_languages() -> Result<Vec<String>, String> {
    windows_ocr::available_languages()
}

#[cfg(target_os = "windows")]
mod windows_ocr {
    use std::sync::OnceLock;

    use ::windows::{
        core::HSTRING,
        Globalization::Language,
        Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap},
        Media::Ocr::OcrEngine,
        Storage::Streams::DataWriter,
        Win32::System::{
            Com::CoIncrementMTAUsage,
            WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        },
    };

    use super::OcrLine;

    // The blocking worker must initialize WinRT before creating its OCR objects.
    // Drop after all WinRT objects, balancing even a successful S_FALSE result.
    struct Apartment;
    impl Apartment {
        fn new() -> Result<Self, String> {
            // windows-rs caches agile activation factories for the process.
            // Keep the MTA alive between blocking jobs: the last RoUninitialize
            // otherwise invalidates those factories, crashing a later capture.
            // This single process-lifetime cookie is reclaimed by Windows at exit.
            static MTA: OnceLock<Result<(), String>> = OnceLock::new();
            MTA.get_or_init(|| {
                // SAFETY: once per process; deliberately outlives cached factories.
                unsafe { CoIncrementMTAUsage() }
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
            .clone()?;
            // SAFETY: each call is balanced by Drop on this same worker thread.
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
            Ok(Self)
        }
    }
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: this guard owns a successful RoInitialize call.
            unsafe { RoUninitialize() };
        }
    }

    pub fn available_languages() -> Result<Vec<String>, String> {
        let _apartment = Apartment::new()?;
        let run = || -> ::windows::core::Result<Vec<String>> {
            OcrEngine::AvailableRecognizerLanguages()?
                .into_iter()
                .map(|language| language.LanguageTag().map(|tag| tag.to_string()))
                .collect()
        };
        run().map_err(|e| e.to_string())
    }

    pub fn language_supported(source_language: &str) -> Result<bool, String> {
        let _apartment = Apartment::new()?;
        let run = || -> ::windows::core::Result<bool> {
            if source_language.is_empty() {
                Ok(OcrEngine::AvailableRecognizerLanguages()?.Size()? > 0)
            } else {
                OcrEngine::IsLanguageSupported(&Language::CreateLanguage(&HSTRING::from(
                    source_language,
                ))?)
            }
        };
        run().map_err(|e| format!("Windows OCR: {e}"))
    }

    pub fn recognize(
        mut image: image::RgbaImage,
        source_language: &str,
    ) -> Result<Vec<OcrLine>, String> {
        let _apartment = Apartment::new()?;
        let run = || -> ::windows::core::Result<Vec<OcrLine>> {
            let engine = if !source_language.is_empty() {
                let language = Language::CreateLanguage(&HSTRING::from(source_language))?;
                if !OcrEngine::IsLanguageSupported(&language)? {
                    return Err(::windows::core::Error::new(
                        ::windows::core::HRESULT(0x80004005u32 as i32),
                        format!("{} ({source_language})", crate::i18n::current("missingOcr")),
                    ));
                }
                OcrEngine::TryCreateFromLanguage(&language)?
            } else {
                match OcrEngine::TryCreateFromUserProfileLanguages() {
                    Ok(engine) => engine,
                    Err(_) => {
                        let languages = OcrEngine::AvailableRecognizerLanguages()?;
                        if languages.Size()? == 0 {
                            return Err(::windows::core::Error::new(
                            ::windows::core::HRESULT(0x80004005u32 as i32),
                            crate::i18n::current("missingOcr"),
                        ));
                        }
                        OcrEngine::TryCreateFromLanguage(&languages.GetAt(0)?)?
                    }
                }
            };
            eprintln!(
                "ocr language: {}",
                engine.RecognizerLanguage()?.LanguageTag()?
            );
            // Windows OCR rejects images beyond MaxImageDimension. Resize while
            // preserving aspect ratio; normalized boxes still map to the original.
            let max = OcrEngine::MaxImageDimension()?;
            if image.width() > max || image.height() > max {
                image = image::DynamicImage::ImageRgba8(image)
                    .resize(max, max, image::imageops::FilterType::Lanczos3)
                    .to_rgba8();
            } else if image.width().max(image.height()) < 1200 {
                // Enlarge tiny UI glyphs; normalized boxes map to the original.
                let scale = (f64::from(max.min(1200))
                    / f64::from(image.width().max(image.height())))
                .min(3.0);
                image = image::imageops::resize(
                    &image,
                    (f64::from(image.width()) * scale).round() as u32,
                    (f64::from(image.height()) * scale).round() as u32,
                    image::imageops::FilterType::Lanczos3,
                );
            }
            let (width, height) = image.dimensions();
            // SoftwareBitmap expects BGRA, whereas image supplies RGBA.
            for pixel in image.pixels_mut() {
                pixel.0.swap(0, 2);
            }
            let writer = DataWriter::new()?;
            writer.WriteBytes(image.as_raw())?;
            let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
                &writer.DetachBuffer()?,
                BitmapPixelFormat::Bgra8,
                width as i32,
                height as i32,
                BitmapAlphaMode::Ignore,
            )?;
            let result = engine.RecognizeAsync(&bitmap)?.join()?;
            let mut lines = Vec::new();
            for line in result.Lines()? {
                let text = line.Text()?.to_string();
                if text.trim().is_empty() {
                    continue;
                }
                // Windows returns boxes per word; union them into a line box.
                let mut left = f64::INFINITY;
                let mut top = f64::INFINITY;
                let mut right = f64::NEG_INFINITY;
                let mut bottom = f64::NEG_INFINITY;
                for word in line.Words()? {
                    let rect = word.BoundingRect()?;
                    left = left.min(f64::from(rect.X));
                    top = top.min(f64::from(rect.Y));
                    right = right.max(f64::from(rect.X + rect.Width));
                    bottom = bottom.max(f64::from(rect.Y + rect.Height));
                }
                if right > left && bottom > top {
                    lines.push(OcrLine {
                        text,
                        x: left / f64::from(width),
                        y: top / f64::from(height),
                        width: (right - left) / f64::from(width),
                        height: (bottom - top) / f64::from(height),
                    });
                }
            }
            Ok(lines)
        };
        run().map_err(|e| format!("Windows OCR: {e}"))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn recognizes_fixture_with_normalized_line_boxes() {
            let image = image::load_from_memory(super::super::WARMUP_PNG)
                .unwrap()
                .to_rgba8();
            let lines = recognize(image, "").unwrap();
            assert!(
                !lines.is_empty(),
                "OCR language pack must recognize the warm-up image"
            );
            for line in lines {
                assert!(!line.text.trim().is_empty());
                assert!(line.x >= 0.0 && line.y >= 0.0);
                assert!(line.width > 0.0 && line.height > 0.0);
                assert!(line.x + line.width <= 1.001);
                assert!(line.y + line.height <= 1.001);
            }
        }

        #[test]
        fn recognizes_image_larger_than_windows_limit() {
            let _apartment = Apartment::new().unwrap();
            let max = OcrEngine::MaxImageDimension().unwrap();
            let fixture = image::load_from_memory(super::super::WARMUP_PNG)
                .unwrap()
                .to_rgba8();
            let mut image =
                image::RgbaImage::from_pixel(max + 1, fixture.height(), image::Rgba([255; 4]));
            image::imageops::overlay(&mut image, &fixture, 0, 0);
            let lines = recognize(image, "").unwrap();
            assert!(!lines.is_empty());
            assert!(lines.iter().all(|line| line.x + line.width <= 1.001));
        }

        #[test]
        fn missing_japanese_model_is_reported_instead_of_using_english() {
            // Windows can expose Japanese as "ja" while accepting "ja-JP".
            if language_supported("ja-JP").unwrap() {
                return;
            }
            let image = image::load_from_memory(include_bytes!("../assets/ocr-japanese.png"))
                .unwrap()
                .to_rgba8();
            let error = recognize(image, "ja-JP").unwrap_err();
            assert!(
                error.contains("ja-JP") && error.contains(crate::i18n::current("missingOcr")),
                "{error}"
            );
        }

        #[test]
        fn explicit_english_model_recognizes_small_fixture() {
            let image = image::load_from_memory(super::super::WARMUP_PNG)
                .unwrap()
                .to_rgba8();
            assert!(!recognize(image, "en-US").unwrap().is_empty());
        }

        #[test]
        #[ignore = "Requires the Windows Japanese OCR language feature"]
        fn recognizes_small_japanese_slide_with_selected_model() {
            let image = image::load_from_memory(include_bytes!("../assets/ocr-japanese.png"))
                .unwrap()
                .to_rgba8();
            let text: String = recognize(image, "ja-JP")
                .unwrap()
                .into_iter()
                .flat_map(|line| {
                    line.text
                        .chars()
                        .filter(|c| !c.is_whitespace())
                        .collect::<Vec<_>>()
                })
                .collect();
            assert!(
                text.contains("データ") && text.contains("加工") && text.contains("整理"),
                "{text}"
            );
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn recognize(_path: &Path, _language: &str) -> Result<Vec<OcrLine>, String> {
    Err("OCR is only implemented on macOS and Windows".into())
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn warm_up() -> Result<usize, String> {
    let _ = WARMUP_PNG;
    Ok(0)
}
