use std::ffi::c_void;

use objc2_app_kit::{
    NSScreenSaverWindowLevel, NSWindow, NSWindowAnimationBehavior, NSWindowCollectionBehavior,
};
use tauri::WebviewWindow;

#[repr(C)]
struct CGPoint {
    x: f64,
    y: f64,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
    fn CGEventCreate(source: *const c_void) -> *mut c_void;
    fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// True when Screen Recording is granted. Otherwise triggers the system prompt
/// (first call only) and returns false; macOS applies a new grant after relaunch.
pub fn ensure_screen_capture_access() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() || CGRequestScreenCaptureAccess() }
}

/// Cursor position in global logical points, top-left origin (same space as
/// `CGDisplayBounds`, which xcap uses for monitor geometry).
pub fn cursor_position() -> Option<(f64, f64)> {
    unsafe {
        let event = CGEventCreate(std::ptr::null());
        if event.is_null() {
            return None;
        }
        let p = CGEventGetLocation(event);
        CFRelease(event);
        Some((p.x, p.y))
    }
}

/// Lifts a selector window above the menu bar and Dock, on every Space including
/// full-screen apps.
pub fn make_overlay(window: &WebviewWindow) {
    with_ns_window(window, |ns_window| {
        ns_window.setAnimationBehavior(NSWindowAnimationBehavior::None);
        ns_window.setLevel(NSScreenSaverWindowLevel);
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
    });
}

/// Lets a result window open on the Space it was captured from, even when that
/// Space belongs to a full-screen app.
pub fn allow_over_fullscreen(window: &WebviewWindow) {
    with_ns_window(window, |ns_window| {
        ns_window.setCollectionBehavior(
            NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::MoveToActiveSpace,
        );
    });
}

fn with_ns_window(window: &WebviewWindow, f: impl FnOnce(&NSWindow) + Send + 'static) {
    let target = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(ptr) = target.ns_window() else { return };
        // SAFETY: Tauri hands out a live NSWindow pointer; we are on the main thread.
        f(unsafe { &*ptr.cast::<NSWindow>() });
    });
}

/// Copies PNG bytes directly into the system pasteboard.
pub fn copy_image_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypePNG};
    use objc2_foundation::NSData;

    let pb = NSPasteboard::generalPasteboard();
    pb.clearContents();
    let data = NSData::with_bytes(png_bytes);
    let ok = pb.setData_forType(Some(&data), unsafe { NSPasteboardTypePNG });
    if ok {
        Ok(())
    } else {
        Err("Failed to set pasteboard data".into())
    }
}

/// Opens macOS native Save Panel to prompt the user for a destination file path.
pub fn show_save_file_dialog(suggested_name: &str) -> Option<std::path::PathBuf> {
    use objc2_app_kit::{NSModalResponseOK, NSSavePanel};
    use objc2_foundation::{MainThreadMarker, NSString};

    let mtm = MainThreadMarker::new()?;
    let panel = NSSavePanel::savePanel(mtm);
    let title = NSString::from_str(suggested_name);
    panel.setNameFieldStringValue(&title);
    let response = panel.runModal();
    if response == NSModalResponseOK {
        let url = panel.URL()?;
        let path_str = url.path()?;
        Some(std::path::PathBuf::from(path_str.to_string()))
    } else {
        None
    }
}
