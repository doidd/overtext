use std::{borrow::Cow, ffi::OsString, os::windows::ffi::OsStringExt, path::PathBuf};

use ::windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

/// Physical virtual-desktop pixels, matching xcap's Windows monitor coordinates.
pub fn cursor_position() -> Option<(f64, f64)> {
    let mut point = POINT::default();
    // SAFETY: point is a writable POINT for the duration of the call.
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((f64::from(point.x), f64::from(point.y)))
}

pub fn copy_image_to_clipboard(png_bytes: &[u8]) -> Result<(), String> {
    // Serialize operations because Windows only permits one open clipboard.
    static CLIPBOARD: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _guard = CLIPBOARD.lock();
    let image = image::load_from_memory_with_format(png_bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .to_rgba8();
    let data = arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: Cow::Borrowed(image.as_raw()),
    };
    arboard::Clipboard::new()
        .map_err(|e| e.to_string())?
        .set_image(data)
        .map_err(|e| e.to_string())
}

pub fn show_save_file_dialog(suggested_name: &str, owner: isize, title: &str) -> Result<Option<PathBuf>, String> {
    use ::windows::{
        core::{w, HRESULT, HSTRING},
        Win32::Foundation::HWND,
        Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_INPROC_SERVER,
            COINIT_APARTMENTTHREADED,
        },
        Win32::UI::Shell::{
            Common::COMDLG_FILTERSPEC, FileSaveDialog, IFileSaveDialog, FOS_FORCEFILESYSTEM,
            FOS_NOCHANGEDIR, FOS_OVERWRITEPROMPT, FOS_PATHMUSTEXIST, SIGDN_FILESYSPATH,
        },
    };

    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: balances the successful initialization on this worker.
            unsafe { CoUninitialize() };
        }
    }

    let run = || -> ::windows::core::Result<Option<PathBuf>> {
        // SAFETY: the blocking worker owns this STA until the dialog is dropped.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        let _apartment = Apartment;
        // SAFETY: all COM objects are used on this initialized thread; strings
        // and filter buffers remain alive through their synchronous setter calls.
        unsafe {
            let dialog: IFileSaveDialog =
                CoCreateInstance(&FileSaveDialog, None, CLSCTX_INPROC_SERVER)?;
            dialog.SetOptions(
                dialog.GetOptions()?
                    | FOS_FORCEFILESYSTEM
                    | FOS_NOCHANGEDIR
                    | FOS_PATHMUSTEXIST
                    | FOS_OVERWRITEPROMPT,
            )?;
            dialog.SetTitle(&HSTRING::from(title))?;
            dialog.SetFileName(&HSTRING::from(suggested_name))?;
            dialog.SetDefaultExtension(w!("png"))?;
            let filter_name: Vec<u16> = crate::i18n::current("pngImage").encode_utf16().chain(Some(0)).collect();
            dialog.SetFileTypes(&[COMDLG_FILTERSPEC {
                pszName: ::windows::core::PCWSTR(filter_name.as_ptr()),
                pszSpec: w!("*.png"),
            }])?;
            if let Err(error) = dialog.Show(Some(HWND(owner as *mut _))) {
                if error.code() == HRESULT(0x800704c7u32 as i32) {
                    return Ok(None); // HRESULT_FROM_WIN32(ERROR_CANCELLED)
                }
                return Err(error);
            }
            let name = dialog.GetResult()?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = PathBuf::from(OsString::from_wide(name.as_wide()));
            CoTaskMemFree(Some(name.0.cast()));
            Ok(Some(path))
        }
    };
    run().map_err(|error| format!("Windows save dialog: {error}"))
}
