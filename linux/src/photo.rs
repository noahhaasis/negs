use gtk::{gdk_pixbuf, prelude::*};
use std::path::{Path, PathBuf};

pub struct Photo {
    pub bytes: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub stride: usize,
    pub alpha: bool,
    pub source: (i32, i32),
    pub histogram: [Vec<u32>; 3],
}

pub fn decode(path: &Path, max: i32) -> Result<Photo, String> {
    let pixbuf = if matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("nef" | "dng" | "cr2" | "arw")
    ) {
        // Preview only: extract an embedded camera JPEG, never develop RAW on the UI thread.
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(128 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        let mut candidates = Vec::new();
        let mut i = 0;
        while i + 3 < bytes.len() {
            if bytes[i..i + 3] == [255, 216, 255] {
                if let Some(end) = bytes[i + 3..].windows(2).position(|w| w == [255, 217]) {
                    candidates.push((i, i + end + 5));
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        candidates.sort_by_key(|(a, b)| std::cmp::Reverse(b - a));
        let mut found = None;
        for (a, b) in candidates.into_iter().take(5) {
            let loader = gdk_pixbuf::PixbufLoader::new();
            loader.connect_size_prepared(move |loader, w, h| {
                let factor = (max as f64 / w.max(h) as f64).min(1.);
                loader.set_size(
                    (w as f64 * factor).round() as i32,
                    (h as f64 * factor).round() as i32,
                );
            });
            if loader.write(&bytes[a..b]).is_ok() && loader.close().is_ok() {
                found = loader.pixbuf();
                if found.is_some() {
                    break;
                }
            }
        }
        found.ok_or("No supported embedded JPEG preview. RAW development will arrive with the engine connection.")?
    } else {
        gdk_pixbuf::Pixbuf::from_file_at_scale(path, max, max, true).map_err(|e| e.to_string())?
    };
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    let source = gdk_pixbuf::Pixbuf::file_info(path)
        .map(|(_, w, h)| (w, h))
        .unwrap_or((pixbuf.width(), pixbuf.height()));
    let raw = pixbuf.read_pixel_bytes();
    let bytes = raw.as_ref().to_vec();
    let mut histogram = [vec![0; 256], vec![0; 256], vec![0; 256]];
    let stride = pixbuf.rowstride() as usize;
    let channels = pixbuf.n_channels() as usize;
    for y in (0..pixbuf.height() as usize).step_by(3) {
        for x in (0..pixbuf.width() as usize).step_by(3) {
            let p = y * stride + x * channels;
            for c in 0..3 {
                histogram[c][bytes[p + c] as usize] += 1;
            }
        }
    }
    Ok(Photo {
        bytes,
        width: pixbuf.width(),
        height: pixbuf.height(),
        stride,
        alpha: pixbuf.has_alpha(),
        source,
        histogram,
    })
}

pub fn folder(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(parent) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            !p.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .starts_with('.')
                && matches!(
                    p.extension()
                        .and_then(|s| s.to_str())
                        .map(str::to_lowercase)
                        .as_deref(),
                    Some("jpg" | "jpeg" | "png" | "tif" | "tiff" | "nef" | "dng" | "arw" | "cr2")
                )
        })
        .take(200)
        .collect();
    paths.sort();
    paths
}
