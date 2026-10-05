//! Windows-only, isolated RapidOCR worker. The model stays resident between captures.
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, OnceLock,
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;
use serde::Deserialize;

use super::OcrLine;

const WORKER_CODE: &str = include_str!("../assets/rapidocr-worker.py");
const INSTALLER: &str = include_str!("../../scripts/install-rapidocr.ps1");
const CREATE_NO_WINDOW: u32 = 0x08000000;
static WORKER: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();
static INSTALL: OnceLock<Mutex<()>> = OnceLock::new();
// Separate from the request mutex so exiting can stop an in-flight inference.
static PROCESS: OnceLock<Mutex<Option<Arc<Mutex<Child>>>>> = OnceLock::new();

fn runtime_dir() -> Result<PathBuf, String> {
    std::env::var_os("LOCALAPPDATA")
        .map(|dir| PathBuf::from(dir).join("OverText/rapidocr"))
        .ok_or_else(|| crate::i18n::current("rapidData").into())
}

pub fn installed(model: &str) -> bool {
    runtime_dir().is_ok_and(|dir| {
        matches!(model, "mobile" | "server")
            && dir.join(format!("ready-{model}-v1")).is_file()
            && dir.join("runtime-v1").is_file()
            && [
                "python.exe".to_owned(),
                "models/ch_PP-OCRv5_det_mobile.onnx".to_owned(),
                "models/ch_ppocr_mobile_v2.0_cls_mobile.onnx".to_owned(),
                format!("models/ch_PP-OCRv5_rec_{model}.onnx"),
            ]
            .iter()
            .all(|path| {
                dir.join(path)
                    .metadata()
                    .is_ok_and(|meta| meta.is_file() && meta.len() > 0)
            })
    })
}

pub fn install(model: &str) -> Result<(), String> {
    let _install_guard = INSTALL.get_or_init(|| Mutex::new(())).lock();
    if !matches!(model, "mobile" | "server") {
        return Err("Unsupported RapidOCR model".into());
    }
    if installed(model) {
        return Ok(());
    }
    let dir = runtime_dir()?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join("install.ps1");
    fs::write(&script, INSTALLER).map_err(|e| e.to_string())?;
    fs::write(dir.join("worker.py"), WORKER_CODE).map_err(|e| e.to_string())?;
    fs::write(
        dir.join("warmup.png"),
        include_bytes!("../assets/ocr-warmup.png"),
    )
    .map_err(|e| e.to_string())?;
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
        .args(["-Model", model])
        .stdin(Stdio::null())
        .stdout(output)
        .stderr(stderr)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|e| format!("{}: {e}", crate::i18n::current("rapidInstaller")))?;
    if !status.success() || !installed(model) {
        return Err(format!(
            "{} {}",
            crate::i18n::current("rapidInstallFailed"),
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
        .map_err(|e| format!("{}: {e}", crate::i18n::current("rapidInvalid")))?;
    if let Some(error) = reply.error {
        return Err(format!("RapidOCR: {error}"));
    }
    let lines = reply.lines.ok_or(crate::i18n::current("rapidNoResult"))?;
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
            return Err(crate::i18n::current("rapidGeometry").into());
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
    fn start(model: &str) -> Result<Self, String> {
        if !installed(model) {
            return Err(crate::i18n::current("rapidMissing").into());
        }
        let dir = runtime_dir()?;
        let log = fs::File::create(dir.join("worker.log")).map_err(|e| e.to_string())?;
        let mut child = Command::new(dir.join("python.exe"))
            .args(["-u", "-c", WORKER_CODE])
            .env("PYTHONUTF8", "1")
            .env("OVERTEXT_RAPID_DIR", &dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("{}: {e}", crate::i18n::current("rapidStart")))?;
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

    fn recognize(
        &mut self,
        path: &Path,
        language: &str,
        request_id: u64,
    ) -> Result<Vec<OcrLine>, String> {
        let started = Instant::now();
        let mut request = serde_json::to_vec(
            &serde_json::json!({ "path": path, "model": language, "request_id": request_id }),
        )
        .map_err(|e| e.to_string())?;
        request.push(b'\n');
        self.input
            .write_all(&request)
            .and_then(|_| self.input.flush())
            .map_err(|e| format!("{}: {e}", crate::i18n::current("rapidStopped")))?;
        // Models are already installed. Kill a timed-out worker so
        // its late response cannot be mistaken for a subsequent capture.
        let reply = self
            .replies
            .recv_timeout(Duration::from_secs(60))
            .map_err(|e| format!("{} ({e})", crate::i18n::current("rapidTimeout")))??;
        let roundtrip = started.elapsed();
        let parse_started = Instant::now();
        let result = parse_reply(&reply);
        eprintln!(
            "rapidocr request {request_id}: roundtrip={roundtrip:?}, parse={:?}, success={}",
            parse_started.elapsed(),
            result.is_ok()
        );
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
    if !installed(language) {
        return Err(crate::i18n::current("rapidMissing").into());
    }
    static NEXT_REQUEST: AtomicU64 = AtomicU64::new(1);
    let request_id = NEXT_REQUEST.fetch_add(1, Ordering::Relaxed);
    let started = Instant::now();
    let mut slot = WORKER.get_or_init(|| Mutex::new(None)).lock();
    let queue_wait = started.elapsed();
    let startup_started = Instant::now();
    let worker_reused = slot.is_some();
    if slot.is_none() {
        *slot = Some(Worker::start(language)?);
    }
    let startup = startup_started.elapsed();
    let result = slot
        .as_mut()
        .expect("initialized worker")
        .recognize(path, language, request_id);
    eprintln!("rapidocr request {request_id}: queue_wait={queue_wait:?}, worker_start={startup:?}, worker_reused={worker_reused}, total={:?}", started.elapsed());
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

pub fn warm_up(model: &str) -> Result<usize, String> {
    if !installed(model) {
        return Ok(0);
    }
    let path = runtime_dir()?.join("warmup.png");
    recognize(&path, model).map(|lines| lines.len())
}
