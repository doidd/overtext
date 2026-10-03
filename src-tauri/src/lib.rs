mod cache;
mod capture;
mod layout;
#[cfg(target_os = "macos")]
mod macos;
mod ocr;
mod settings;
mod translate;
#[cfg(target_os = "windows")]
mod windows;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Instant,
};

use capture::{Crop, Frame, Selection};
use parking_lot::Mutex;
use serde::Serialize;
use serde_json::json;
use settings::Settings;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager, RunEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
#[cfg(target_os = "macos")]
use tauri::menu::{PredefinedMenuItem, Submenu};

const CAPTURE_SHORTCUT: &str = "CommandOrControl+Shift+1";
const SELECTOR_PREFIX: &str = "selector-";
const RESULT_PREFIX: &str = "result-";

#[derive(Default)]
struct AppState {
    /// Frozen monitor frames; `Some` while selector windows are open.
    session: Mutex<Option<Vec<Frame>>>,
    /// Set from hotkey press until the selector windows are torn down.
    capturing: AtomicBool,
    next_result: AtomicU32,
    http: reqwest::Client,
    settings: Mutex<Settings>,
    cache: Mutex<Option<cache::Cache>>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Recognition {
    /// Image size in pixels; block geometry is in the same space.
    width: u32,
    height: u32,
    blocks: Vec<layout::Block>,
}

/// OCR + layout for a result image previously written by `finish_capture`.
#[tauri::command]
async fn recognize_capture(app: AppHandle, image_path: PathBuf) -> Result<Recognition, String> {
    let dir = captures_dir(&app)?;
    let path = image_path.canonicalize().map_err(|e| e.to_string())?;
    if !path.starts_with(dir.canonicalize().map_err(|e| e.to_string())?) {
        return Err("image is outside the captures directory".into());
    }
    let ocr_language = app.state::<AppState>().settings.lock().ocr_lang.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let started = Instant::now();
        let image = image::open(&path).map_err(|e| e.to_string())?.to_rgba8();
        let decode = started.elapsed();
        let recognize_started = Instant::now();
        let lines = ocr::recognize(&path, &ocr_language)?;
        let recognize = recognize_started.elapsed();
        let layout_started = Instant::now();
        let blocks = layout::build_blocks(&lines, &image);
        eprintln!("ocr stages: image={}x{}, decode={decode:?}, recognize={recognize:?}, layout={:?}", image.width(), image.height(), layout_started.elapsed());
        eprintln!("ocr: {} lines → {} blocks in {:?}", lines.len(), blocks.len(), started.elapsed());
        Ok(Recognition { width: image.width(), height: image.height(), blocks })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
async fn translate_texts(app: AppHandle, texts: Vec<String>) -> Result<Vec<String>, String> {
    let started = Instant::now();
    let settings = app.state::<AppState>().settings.lock().clone();
    let provider_key = settings.provider_key();
    let target_lang = settings.target_lang.clone();

    // Check cache first for all texts
    let mut cached_results: Vec<Option<String>> = Vec::with_capacity(texts.len());
    let mut missing_indices: Vec<usize> = Vec::new();
    let mut missing_texts: Vec<String> = Vec::new();

    {
        let state = app.state::<AppState>();
        let cache_guard = state.cache.lock();
        for (i, text) in texts.iter().enumerate() {
            if let Some(cache) = &*cache_guard {
                if let Some(trans) = cache.get(text, &target_lang, &provider_key) {
                    cached_results.push(Some(trans));
                    continue;
                }
            }
            cached_results.push(None);
            missing_indices.push(i);
            missing_texts.push(text.clone());
        }
    }

    if missing_texts.is_empty() {
        let hit_count = texts.len();
        eprintln!("translate: all {hit_count} texts from cache in {:?}", started.elapsed());
        return Ok(cached_results.into_iter().map(|opt| opt.unwrap()).collect());
    }

    let key = if settings.provider == settings::Provider::Openai {
        let url = settings.base_url.clone();
        tauri::async_runtime::spawn_blocking(move || settings::api_key(&url)).await.map_err(|e| e.to_string())??
    } else {
        None
    };

    let fetched = translate::translate(&app.state::<AppState>().http, &settings, key.as_deref(), &missing_texts)
        .await
        .inspect_err(|e| eprintln!("translation failed: {e}"))?;

    // Store newly fetched results in cache and merge into final output
    {
        let state = app.state::<AppState>();
        let cache_guard = state.cache.lock();
        for (&orig_idx, trans_text) in missing_indices.iter().zip(fetched.iter()) {
            if let Some(cache) = &*cache_guard {
                cache.set(&texts[orig_idx], &target_lang, &provider_key, trans_text);
            }
            cached_results[orig_idx] = Some(trans_text.clone());
        }
    }

    eprintln!(
        "translate: {} texts ({} fetched, {} from cache) in {:?}",
        texts.len(),
        missing_texts.len(),
        texts.len() - missing_texts.len(),
        started.elapsed()
    );
    Ok(cached_results.into_iter().map(|opt| opt.unwrap()).collect())
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    settings: Settings,
    languages: Vec<(&'static str, &'static str)>,
    ocr_languages: Option<Vec<String>>,
    paddleocr_installed: bool,
}

#[tauri::command]
async fn get_settings(app: AppHandle) -> Result<SettingsView, String> {
    #[cfg(target_os = "windows")]
    let ocr_languages = Some(tauri::async_runtime::spawn_blocking(ocr::available_languages)
        .await.map_err(|e| e.to_string())??);
    #[cfg(not(target_os = "windows"))]
    let ocr_languages = None;
    #[cfg(target_os = "windows")]
    let paddleocr_installed = ocr::paddleocr_installed();
    #[cfg(not(target_os = "windows"))]
    let paddleocr_installed = false;
    Ok(SettingsView { settings: app.state::<AppState>().settings.lock().clone(), languages: settings::LANGUAGES.to_vec(), ocr_languages, paddleocr_installed })
}

#[tauri::command]
async fn install_paddleocr() -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return tauri::async_runtime::spawn_blocking(ocr::install_paddleocr).await.map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "windows"))]
    Err("PaddleOCR fallback hiện chỉ dùng trên Windows".into())
}

/// Whether a key is stored for `base_url`; the key never leaves the credential store.
#[tauri::command]
async fn has_api_key(base_url: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || settings::api_key(base_url.trim().trim_end_matches('/')).map(|k| k.is_some()))
        .await
        .map_err(|e| e.to_string())?
}

/// `api_key`: `None` leaves the stored key alone, `Some("")` deletes it.
/// For LLM providers, settings are verified with a small probe request first;
/// if that fails, nothing is stored and the exact HTTP/model error is returned.
#[tauri::command]
async fn save_settings(app: AppHandle, settings: Settings, api_key: Option<String>) -> Result<(), String> {
    let settings = settings.validated()?;
    let url = settings.base_url.clone();
    if settings.provider == settings::Provider::Openai {
        let effective_key = match &api_key {
            Some(k) => Some(k.trim().to_owned()).filter(|k| !k.is_empty()),
            None => {
                let u = url.clone();
                tauri::async_runtime::spawn_blocking(move || settings::api_key(&u))
                    .await
                    .map_err(|e| e.to_string())??
            }
        };
        translate::verify(&app.state::<AppState>().http, &settings, effective_key.as_deref())
            .await
            .map_err(|e| e.to_string())?;
    }
    if let Some(key) = api_key {
        tauri::async_runtime::spawn_blocking(move || settings::set_api_key(&url, key.trim()))
            .await
            .map_err(|e| e.to_string())??;
    }
    settings::save(&app, &settings)?;
    *app.state::<AppState>().settings.lock() = settings;
    Ok(())
}

#[tauri::command]
async fn copy_image_to_clipboard(_app: AppHandle, base64_png: String) -> Result<(), String> {
    let bytes = decode_base64_png(&base64_png)?;
    #[cfg(target_os = "macos")]
    {
        macos::copy_image_to_clipboard(&bytes)
    }
    #[cfg(target_os = "windows")]
    {
        tauri::async_runtime::spawn_blocking(move || windows::copy_image_to_clipboard(&bytes))
            .await
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = bytes;
        Err("Copy image is only implemented on macOS and Windows".into())
    }
}

#[tauri::command]
async fn save_image_to_file(_app: AppHandle, _window: WebviewWindow, base64_png: String, default_name: Option<String>) -> Result<bool, String> {
    let bytes = decode_base64_png(&base64_png)?;
    let name = default_name.unwrap_or_else(|| "translated-screenshot.png".into());
    #[cfg(target_os = "windows")]
    let owner = _window.hwnd().map_err(|e| e.to_string())?.0 as isize;
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let path = tauri::async_runtime::spawn_blocking(move || {
            #[cfg(target_os = "macos")]
            { Ok::<_, String>(macos::show_save_file_dialog(&name)) }
            #[cfg(target_os = "windows")]
            { windows::show_save_file_dialog(&name, owner) }
        })
            .await
            .map_err(|e| e.to_string())??;
        if let Some(mut dest) = path {
            if dest.extension().is_none() {
                dest.set_extension("png");
            }
            fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = bytes;
        let _ = name;
        Err("Save image is only implemented on macOS and Windows".into())
    }
}

#[tauri::command]
async fn record_history(
    app: AppHandle,
    source_markdown: String,
    translated_markdown: String,
    thumbnail_base64: String,
) -> Result<i64, String> {
    let settings = app.state::<AppState>().settings.lock().clone();
    let provider_key = settings.provider_key();
    let target_lang = settings.target_lang.clone();

    let state = app.state::<AppState>();
    let cache_guard = state.cache.lock();
    if let Some(cache) = &*cache_guard {
        cache.add_history(
            &target_lang,
            &provider_key,
            &source_markdown,
            &translated_markdown,
            &thumbnail_base64,
        )
    } else {
        Err("Cache not initialized".into())
    }
}

#[tauri::command]
async fn get_history(app: AppHandle, limit: Option<usize>) -> Result<Vec<cache::HistoryItem>, String> {
    let state = app.state::<AppState>();
    let cache_guard = state.cache.lock();
    if let Some(cache) = &*cache_guard {
        cache.get_history(limit.unwrap_or(50))
    } else {
        Ok(Vec::new())
    }
}

#[tauri::command]
async fn delete_history(app: AppHandle, id: i64) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cache_guard = state.cache.lock();
    if let Some(cache) = &*cache_guard {
        cache.delete_history(id)
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn clear_history(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let cache_guard = state.cache.lock();
    if let Some(cache) = &*cache_guard {
        cache.clear_history()
    } else {
        Ok(())
    }
}

fn open_history(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("history") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("index.html".into()))
        .initialization_script("window.__OVERTEXT__ = {\"view\":\"history\"};")
        .title("OverText — Lịch sử dịch")
        .inner_size(720.0, 560.0)
        .resizable(true)
        .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(err) => eprintln!("cannot open history: {err}"),
    }
}

fn decode_base64_png(input: &str) -> Result<Vec<u8>, String> {
    let data = input.strip_prefix("data:image/png;base64,").unwrap_or(input);
    // Base64 decoding using standard library / existing crates or minimal decoder
    // Let's decode cleanly with a small helper
    base64_decode(data.trim())
}

fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let mut result = Vec::with_capacity(input.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for byte in input.bytes() {
        let val = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' => continue,
            _ => return Err(format!("Invalid base64 character: {byte}")),
        };
        buf = (buf << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            result.push((buf >> bits) as u8);
        }
    }
    Ok(result)
}

fn open_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        let _ = window.show();
        let _ = window.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("index.html".into()))
        .initialization_script("window.__OVERTEXT__ = {\"view\":\"settings\"};")
        .title("OverText")
        .inner_size(460.0, 520.0)
        .resizable(false)
        .build();
    match built {
        Ok(window) => {
            let _ = window.set_focus();
        }
        Err(err) => eprintln!("cannot open settings: {err}"),
    }
}

/// Cache directory served to the webview through the asset protocol. Must match
/// `app.security.assetProtocol.scope` in tauri.conf.json.
fn captures_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_cache_dir()
        .map(|dir| dir.join("captures"))
        .map_err(|e| e.to_string())
}

fn session_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(captures_dir(app)?.join("session"))
}

fn page(app: &AppHandle, label: String, boot: serde_json::Value) -> WebviewWindowBuilder<'_, tauri::Wry, AppHandle> {
    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .initialization_script(format!("window.__OVERTEXT__ = {boot};"))
        .title("OverText")
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .accept_first_mouse(true)
        .visible(false)
}

fn start_capture(app: &AppHandle) {
    if app.state::<AppState>().capturing.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(err) = open_selectors(&app) {
            eprintln!("capture failed: {err}");
            end_capture(&app);
        }
    });
}

fn open_selectors(app: &AppHandle) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    if !macos::ensure_screen_capture_access() {
        return Err("Screen Recording permission not granted \
                    (System Settings › Privacy & Security › Screen & System Audio Recording)"
            .into());
    }

    let dir = session_dir(app)?;
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let frames = capture::capture_all(&dir)?;

    // Window creation blocks on the main thread, which also runs sync commands that
    // lock the session: never hold the lock while building windows.
    let monitors: Vec<_> = frames
        .iter()
        .map(|f| (f.x, f.y, f.width, f.height, f.coordinate_scale, f.path.clone()))
        .collect();
    *app.state::<AppState>().session.lock() = Some(frames);

    for (i, (x, y, width, height, scale, path)) in monitors.into_iter().enumerate() {
        let boot = json!({ "view": "selector", "monitor": i, "imagePath": path, "width": f64::from(width) / scale, "height": f64::from(height) / scale });
        let builder = page(app, format!("{SELECTOR_PREFIX}{i}"), boot);
        #[cfg(not(target_os = "windows"))]
        let builder = builder
            .position(f64::from(x), f64::from(y))
            .inner_size(f64::from(width), f64::from(height));
        let window = builder
            .shadow(false)
            .visible_on_all_workspaces(true)
            .build()
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "macos")]
        macos::make_overlay(&window);
        #[cfg(target_os = "windows")]
        {
            window.set_position(tauri::PhysicalPosition::new(x, y)).map_err(|e| e.to_string())?;
            window.set_size(tauri::PhysicalSize::new(width, height)).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn end_capture(app: &AppHandle) {
    let state = app.state::<AppState>();
    state.session.lock().take();
    for (label, window) in app.webview_windows() {
        if label.starts_with(SELECTOR_PREFIX) {
            let _ = window.destroy();
        }
    }
    if let Ok(dir) = session_dir(app) {
        let _ = fs::remove_dir_all(dir);
    }
    state.capturing.store(false, Ordering::SeqCst);
}

fn open_result(app: &AppHandle, n: u32, path: &Path, crop: &Crop) -> Result<(), String> {
    let boot = json!({ "view": "result", "imagePath": path, "width": crop.width, "height": crop.height });
    let builder = page(app, format!("{RESULT_PREFIX}{n}"), boot);
    #[cfg(not(target_os = "windows"))]
    let builder = builder
        .position(crop.x, crop.y)
        .inner_size(crop.width, crop.height);
    let window = builder
        .build()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    macos::allow_over_fullscreen(&window);
    #[cfg(target_os = "windows")]
    {
        window.set_position(tauri::PhysicalPosition::new(crop.x.round() as i32, crop.y.round() as i32))
            .map_err(|e| e.to_string())?;
        window.set_size(tauri::PhysicalSize::new(
            (crop.width * crop.coordinate_scale).round() as u32,
            (crop.height * crop.coordinate_scale).round() as u32,
        )).map_err(|e| e.to_string())?;
    }
    let path = path.to_owned();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = fs::remove_file(&path);
        }
    });
    Ok(())
}

/// Whether the cursor is on the monitor a selector window covers.
fn cursor_on_monitor(app: &AppHandle, monitor: usize) -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        #[cfg(target_os = "macos")]
        let cursor = macos::cursor_position();
        #[cfg(target_os = "windows")]
        let cursor = windows::cursor_position();
        let state = app.state::<AppState>();
        let session = state.session.lock();
        let (Some(frame), Some((cx, cy))) =
            (session.as_ref().and_then(|f| f.get(monitor)), cursor)
        else {
            return false;
        };
        let (x, y) = (f64::from(frame.x), f64::from(frame.y));
        cx >= x && cx < x + f64::from(frame.width) && cy >= y && cy < y + f64::from(frame.height)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        monitor == 0
    }
}

/// Called by a page once its image has loaded, so windows never flash empty.
#[tauri::command]
fn window_ready(app: AppHandle, window: WebviewWindow) -> Result<(), String> {
    window.show().map_err(|e| e.to_string())?;
    let focus = match window.label().strip_prefix(SELECTOR_PREFIX) {
        Some(index) => index.parse().is_ok_and(|i| cursor_on_monitor(&app, i)),
        None => true,
    };
    if focus {
        window.set_focus().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn cancel_capture(app: AppHandle, error: Option<String>) {
    if let Some(error) = error {
        eprintln!("capture aborted: {error}");
    }
    end_capture(&app);
}

#[tauri::command]
async fn finish_capture(app: AppHandle, selection: Selection) -> Result<(), String> {
    let crop = {
        let state = app.state::<AppState>();
        let session = state.session.lock();
        let frame = session
            .as_ref()
            .and_then(|frames| frames.get(selection.monitor))
            .ok_or("no capture in progress")?;
        frame.crop(&selection).ok_or("selection is empty")?
    };
    end_capture(&app);

    let n = app.state::<AppState>().next_result.fetch_add(1, Ordering::Relaxed);
    let path = captures_dir(&app)?.join(format!("{RESULT_PREFIX}{n}.png"));
    capture::write_png(&crop.image, &path)?;
    open_result(&app, n, &path, &crop)
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let capture = MenuItem::with_id(app, "capture", "Chụp vùng màn hình", true, Some(CAPTURE_SHORTCUT))?;
    let history = MenuItem::with_id(app, "history", "Lịch sử dịch…", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Cài đặt…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Thoát OverText", true, None::<&str>)?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("OverText")
        .menu(&Menu::with_items(app, &[&capture, &history, &settings, &quit])?)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "capture" => start_capture(app),
            "history" => open_history(app),
            "settings" => open_settings(app),
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState { http: translate::client(), ..AppState::default() })
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        start_capture(app);
                    }
                })
                .build(),
        )
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle();
            // Accessory apps have no default menu, so ⌘C/⌘V/⌘A would not reach text fields.
            #[cfg(target_os = "macos")]
            let edit = Submenu::with_items(
                handle,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(handle, None)?,
                    &PredefinedMenuItem::redo(handle, None)?,
                    &PredefinedMenuItem::separator(handle)?,
                    &PredefinedMenuItem::cut(handle, None)?,
                    &PredefinedMenuItem::copy(handle, None)?,
                    &PredefinedMenuItem::paste(handle, None)?,
                    &PredefinedMenuItem::select_all(handle, None)?,
                ],
            )?;
            #[cfg(target_os = "macos")]
            app.set_menu(Menu::with_items(handle, &[&edit])?)?;
            // Leftovers from a previous run; nothing references them any more.
            if let Ok(dir) = captures_dir(handle) {
                let _ = fs::remove_dir_all(dir);
            }
            build_tray(handle)?;
            if let Err(err) = handle.global_shortcut().register(CAPTURE_SHORTCUT) {
                eprintln!("cannot register {CAPTURE_SHORTCUT}: {err}");
            }
            *handle.state::<AppState>().settings.lock() = settings::load(handle);
            if let Ok(cache_dir) = handle.path().app_cache_dir() {
                let db_path = cache_dir.join("translations.db");
                match cache::Cache::open(&db_path) {
                    Ok(c) => *handle.state::<AppState>().cache.lock() = Some(c),
                    Err(e) => eprintln!("failed to initialize translation cache: {e}"),
                }
            }
            std::thread::spawn(|| {
                let started = Instant::now();
                match ocr::warm_up() {
                    Ok(lines) => eprintln!("ocr warm-up: {lines} lines in {:?}", started.elapsed()),
                    Err(err) => eprintln!("ocr warm-up failed: {err}"),
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            window_ready,
            cancel_capture,
            finish_capture,
            recognize_capture,
            translate_texts,
            get_settings,
            install_paddleocr,
            has_api_key,
            save_settings,
            copy_image_to_clipboard,
            save_image_to_file,
            record_history,
            get_history,
            delete_history,
            clear_history
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            #[cfg(target_os = "windows")]
            if let RunEvent::Exit = event {
                ocr::shutdown_paddleocr();
            }
            // Tray app: closing the last window must not quit.
            if let RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
}
