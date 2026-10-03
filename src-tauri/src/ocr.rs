use std::path::Path;

use serde::Serialize;

/// One recognized text line. Geometry is normalized to the image (0..1), top-left origin.
#[derive(Debug, Clone, Serialize)]
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
pub fn recognize(path: &Path) -> Result<Vec<OcrLine>, String> {
    use objc2::AnyThread;
    use objc2_foundation::{NSDictionary, NSString, NSURL};
    use objc2_vision::VNImageRequestHandler;

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    // SAFETY: empty options dictionary of the documented key/value types.
    let handler = unsafe {
        VNImageRequestHandler::initWithURL_options(VNImageRequestHandler::alloc(), &url, &NSDictionary::new())
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
    let handler =
        VNImageRequestHandler::initWithData_options(VNImageRequestHandler::alloc(), &data, &NSDictionary::new());
    vision::recognize(&handler).map(|lines| lines.len())
}

#[cfg(target_os = "macos")]
mod vision {
    use objc2_foundation::NSArray;
    use objc2_vision::{VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel};

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

        let Some(results) = request.results() else { return Ok(Vec::new()) };
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

#[cfg(not(target_os = "macos"))]
pub fn recognize(_path: &Path) -> Result<Vec<OcrLine>, String> {
    Err("OCR is only implemented on macOS".into())
}

#[cfg(not(target_os = "macos"))]
pub fn warm_up() -> Result<usize, String> {
    let _ = WARMUP_PNG;
    Ok(0)
}
