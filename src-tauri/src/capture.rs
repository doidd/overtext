use std::{
    fs::File,
    io::BufWriter,
    path::{Path, PathBuf},
    thread,
};

use image::{
    codecs::png::{CompressionType, FilterType, PngEncoder},
    imageops, ExtendedColorType, ImageEncoder, RgbaImage,
};
use serde::Deserialize;

/// Frozen screenshot of one monitor. Geometry is in logical points (global desktop
/// coordinates, top-left origin); `image` holds physical pixels.
pub struct Frame {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub image: RgbaImage,
    pub path: PathBuf,
}

/// Rectangle chosen in a selector window, in CSS pixels (= logical points) relative
/// to that monitor.
#[derive(Debug, Deserialize)]
pub struct Selection {
    pub monitor: usize,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Cropped region with its logical placement on the global desktop.
pub struct Crop {
    pub image: RgbaImage,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Captures every monitor in parallel and writes each frame to `dir/monitor-<i>.png`.
pub fn capture_all(dir: &Path) -> Result<Vec<Frame>, String> {
    let monitors = xcap::Monitor::all().map_err(|e| e.to_string())?;
    let handles: Vec<_> = monitors
        .into_iter()
        .enumerate()
        .map(|(i, monitor)| {
            let path = dir.join(format!("monitor-{i}.png"));
            thread::spawn(move || capture_monitor(&monitor, path))
        })
        .collect();
    handles
        .into_iter()
        .map(|h| h.join().map_err(|_| "capture thread panicked".to_string())?)
        .collect()
}

fn capture_monitor(monitor: &xcap::Monitor, path: PathBuf) -> Result<Frame, String> {
    let e = |e: xcap::XCapError| e.to_string();
    let image = monitor.capture_image().map_err(e)?;
    write_png(&image, &path)?;
    Ok(Frame {
        x: monitor.x().map_err(e)?,
        y: monitor.y().map_err(e)?,
        width: monitor.width().map_err(e)?,
        height: monitor.height().map_err(e)?,
        image,
        path,
    })
}

/// Fast, lossless PNG: these files are short-lived and read back immediately.
pub fn write_png(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let file = File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    PngEncoder::new_with_quality(BufWriter::new(file), CompressionType::Fast, FilterType::Sub)
        .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgba8)
        .map_err(|e| e.to_string())
}

impl Frame {
    /// Maps a logical selection onto physical pixels. Returns `None` when the
    /// selection is degenerate after clamping to the monitor.
    pub fn crop(&self, sel: &Selection) -> Option<Crop> {
        let (w, h) = (f64::from(self.width), f64::from(self.height));
        let left = sel.x.clamp(0.0, w);
        let top = sel.y.clamp(0.0, h);
        let right = (sel.x + sel.width).clamp(0.0, w);
        let bottom = (sel.y + sel.height).clamp(0.0, h);

        let sx = f64::from(self.image.width()) / w;
        let sy = f64::from(self.image.height()) / h;
        let px = (left * sx).floor() as u32;
        let py = (top * sy).floor() as u32;
        let pw = ((right * sx).ceil() as u32).min(self.image.width()).saturating_sub(px);
        let ph = ((bottom * sy).ceil() as u32).min(self.image.height()).saturating_sub(py);
        if pw < 2 || ph < 2 {
            return None;
        }

        Some(Crop {
            image: imageops::crop_imm(&self.image, px, py, pw, ph).to_image(),
            x: f64::from(self.x) + left,
            y: f64::from(self.y) + top,
            width: right - left,
            height: bottom - top,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// 100×50pt secondary monitor at (-100, 20) rendered at 2×; each pixel encodes
    /// its own coordinates so crops can be checked by content.
    fn retina_frame() -> Frame {
        Frame {
            x: -100,
            y: 20,
            width: 100,
            height: 50,
            image: RgbaImage::from_fn(200, 100, |x, y| Rgba([x as u8, y as u8, 0, 255])),
            path: PathBuf::new(),
        }
    }

    fn sel(x: f64, y: f64, width: f64, height: f64) -> Selection {
        Selection { monitor: 0, x, y, width, height }
    }

    #[test]
    fn maps_logical_selection_to_physical_pixels_and_global_position() {
        let crop = retina_frame().crop(&sel(10.0, 5.0, 30.0, 20.0)).unwrap();
        assert_eq!(crop.image.dimensions(), (60, 40));
        assert_eq!(crop.image.get_pixel(0, 0), &Rgba([20, 10, 0, 255]));
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (-90.0, 25.0, 30.0, 20.0));
    }

    #[test]
    fn fractional_selection_covers_partial_pixels() {
        let crop = retina_frame().crop(&sel(10.25, 5.0, 1.5, 1.0)).unwrap();
        // 20.5..23.5 physical → pixels 20..24
        assert_eq!(crop.image.dimensions(), (4, 2));
        assert_eq!(crop.image.get_pixel(0, 0)[0], 20);
    }

    #[test]
    fn clamps_selection_dragged_past_monitor_edges() {
        let crop = retina_frame().crop(&sel(-30.0, 40.0, 50.0, 30.0)).unwrap();
        assert_eq!(crop.image.dimensions(), (40, 20));
        assert_eq!(crop.image.get_pixel(0, 0), &Rgba([0, 80, 0, 255]));
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (-100.0, 60.0, 20.0, 10.0));
    }

    #[test]
    fn rejects_selection_outside_monitor() {
        assert!(retina_frame().crop(&sel(120.0, 10.0, 20.0, 20.0)).is_none());
        assert!(retina_frame().crop(&sel(10.0, 10.0, 0.0, 20.0)).is_none());
    }
}
