//! Windows-only, isolated PaddleOCR worker. The model stays resident between captures.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{atomic::{AtomicU64, Ordering}, mpsc, Arc, OnceLock},
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Deserialize;

use super::OcrLine;

const WORKER_CODE: &str = include_str!("../assets/paddleocr-worker.py");
const INSTALLER: &str = include_str!("../../scripts/install-paddleocr.ps1");
const CREATE_NO_WINDOW: u32 = 0x08000000;
static WORKER: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();
// Separate from the request mutex so exiting can stop an in-flight inference.
static PROCESS: OnceLock<Mutex<Option<Arc<Mutex<Child>>>>> = OnceLock::new();

fn runtime_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("OverText/paddleocr"))
        .ok_or_else(|| "Không tìm thấy thư mục dữ liệu PaddleOCR".into())
}

pub fn installed() -> bool {
    runtime_dir()
        .is_ok_and(|dir| dir.join("ready-v1").is_file() && dir.join("python.exe").is_file())
}

pub fn install() -> Result<(), String> {
    if installed() {
        return Ok(());
    }
    let dir = runtime_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join("install.ps1");
    fs::write(&script, INSTALLER).map_err(|e| e.to_string())?;
    let log = dir.join("install.log");
    let output = fs::File::create(&log).map_err(|e| e.to_string())?;
    let stderr = output.try_clone().map_err(|e| e.to_string())?;
    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(stderr)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("Không chạy được trình cài PaddleOCR: {e}"))?;
    if !status.success() || !installed() {
        return Err(format!(
            "Cài PaddleOCR thất bại. Kiểm tra kết nối mạng và thử lại. Chi tiết: {}",
            log.display()
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct Reply {
    lines: Option<Vec<OcrLine>>,
    error: Option<String>,
}

fn parse_reply(reply: &str) -> Result<Vec<OcrLine>, String> {
    let reply: Reply = serde_json::from_str(reply)
        .map_err(|e| format!("PaddleOCR trả dữ liệu không hợp lệ: {e}"))?;
    if let Some(error) = reply.error {
        return Err(format!("PaddleOCR: {error}"));
    }
    let lines = reply.lines.ok_or("PaddleOCR không trả kết quả")?;
    for line in &lines {
        if ![line.x, line.y, line.width, line.height]
            .iter()
            .all(|v| v.is_finite())
            || line.x < 0.0
            || line.y < 0.0
            || line.width <= 0.0
            || line.height <= 0.0
            || line.x + line.width > 1.001
            || line.y + line.height > 1.001
        {
            return Err("PaddleOCR trả tọa độ không hợp lệ".into());
        }
    }
    Ok(lines)
}

struct Worker {
    child: Arc<Mutex<Child>>,
    input: ChildStdin,
    replies: mpsc::Receiver<Result<String, String>>,
}

impl Worker {
    fn start() -> Result<Self, String> {
        if !installed() {
            return Err("Chưa cài PaddleOCR. Mở Cài đặt OverText → Cài PaddleOCR để dùng khi Windows thiếu ngôn ngữ OCR.".into());
        }
        let dir = runtime_dir()?;
        let log = fs::File::create(dir.join("worker.log")).map_err(|e| e.to_string())?;
        let mut child = Command::new(dir.join("python.exe"))
            .args(["-u", "-c", WORKER_CODE])
            .env("PYTHONUTF8", "1")
            .env("PADDLE_PDX_CACHE_HOME", dir.join("models"))
            .env("PADDLE_PDX_DISABLE_MODEL_SOURCE_CHECK", "True")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("Không khởi động được PaddleOCR: {e}"))?;
        let input = child.stdin.take().expect("piped stdin");
        let output = child.stdout.take().expect("piped stdout");
        let child = Arc::new(Mutex::new(child));
        *PROCESS.get_or_init(|| Mutex::new(None)).lock() = Some(child.clone());
        let (sender, replies) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                if sender.send(line.map_err(|e| e.to_string())).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            input,
            replies,
        })
    }

    fn recognize(&mut self, path: &Path, language: &str, request_id: u64) -> Result<Vec<OcrLine>, String> {
        let started = Instant::now();
        let mut request =
            serde_json::to_vec(&serde_json::json!({ "path": path, "language": language, "request_id": request_id }))
                .map_err(|e| e.to_string())?;
        request.push(b'\n');
        self.input
            .write_all(&request)
            .and_then(|_| self.input.flush())
            .map_err(|e| format!("PaddleOCR worker đã dừng: {e}"))?;
        // The first request can download the model. Kill a timed-out worker so
        // its late response cannot be mistaken for a subsequent capture.
        let reply = self.replies.recv_timeout(Duration::from_secs(180))
            .map_err(|e| format!("PaddleOCR không phản hồi ({e}). Kiểm tra mạng khi tải model lần đầu; xem worker.log trong dữ liệu OverText/paddleocr."))??;
        let roundtrip = started.elapsed();
        let parse_started = Instant::now();
        let result = parse_reply(&reply);
        eprintln!("paddleocr request {request_id}: roundtrip={roundtrip:?}, parse={:?}, success={}", parse_started.elapsed(), result.is_ok());
        result
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        {
            let mut child = self.child.lock();
            let _ = child.kill();
            let _ = child.wait();
        }
        if let Some(process) = PROCESS.get() {
            let mut active = process.lock();
            if active
                .as_ref()
                .is_some_and(|child| Arc::ptr_eq(child, &self.child))
            {
                *active = None;
            }
        }
    }
}

pub fn recognize(path: &Path, language: &str) -> Result<Vec<OcrLine>, String> {
    static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
    let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let mut slot = WORKER.get_or_init(|| Mutex::new(None)).lock();
    let queue_wait = started.elapsed();
    let startup_started = Instant::now();
    let worker_reused = slot.is_some();
    if slot.is_none() {
        *slot = Some(Worker::start()?);
    }
    let startup = startup_started.elapsed();
    let result = slot
        .as_mut()
        .expect("initialized worker")
        .recognize(path, language, request_id);
    eprintln!("paddleocr request {request_id}: queue_wait={queue_wait:?}, worker_start={startup:?}, worker_reused={worker_reused}, total={:?}", started.elapsed());
    if result.is_err() {
        // Restart lazily on the next request after a crash, timeout, or error.
        *slot = None;
    }
    result
}

pub fn shutdown() {
    if let Some(child) = PROCESS.get().and_then(|process| process.lock().take()) {
        let mut child = child.lock();
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Some(worker) = WORKER.get() {
        if let Some(mut slot) = worker.try_lock() {
            *slot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_reply_preserves_japanese_and_rejects_invalid_geometry() {
        let lines = parse_reply(
            r#"{"lines":[{"text":"データ加工","x":0.1,"y":0.2,"width":0.7,"height":0.1}]}"#,
        )
        .unwrap();
        assert_eq!(lines[0].text, "データ加工");
        assert!(parse_reply(
            r#"{"lines":[{"text":"bad","x":0.9,"y":0.2,"width":0.7,"height":0.1}]}"#
        )
        .is_err());
        assert!(parse_reply(r#"{"error":"model download failed"}"#)
            .unwrap_err()
            .contains("model download failed"));
        assert!(parse_reply("{}").is_err());
        assert!(parse_reply("not JSON").is_err());
        assert!(parse_reply(r#"{"lines":[]}"#).unwrap().is_empty());
    }

    #[test]
    #[ignore = "Requires installed PaddleOCR runtime; downloads the Japanese model on first run"]
    fn japanese_fallback_recognizes_slide_without_windows_language_pack() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/ocr-japanese.png");
        let lines = super::super::recognize(&path, "ja-JP").unwrap();
        eprintln!(
            "Japanese OCR: {:?}",
            lines.iter().map(|line| &line.text).collect::<Vec<_>>()
        );
        let text: String = lines
            .iter()
            .flat_map(|line| line.text.chars().filter(|c| !c.is_whitespace()))
            .collect();
        assert!(
            text.contains("データ") && text.contains("加工") && text.contains("整理"),
            "{text}"
        );
        assert!(
            lines.len() >= 5
                && ["分類", "集計", "分析要件"]
                    .iter()
                    .all(|word| text.contains(word)),
            "{text}"
        );
        // A second request exercises reuse of the resident model, not just startup.
        // The default must also recognize Japanese even if Windows has only
        // an English pack. This is the regression that used to yield gibberish.
        let next = super::super::recognize(&path, "").unwrap();
        assert_eq!(lines.len(), next.len());
        assert_eq!(
            lines.iter().map(|line| &line.text).collect::<Vec<_>>(),
            next.iter().map(|line| &line.text).collect::<Vec<_>>()
        );
        shutdown();
    }
}
