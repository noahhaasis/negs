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
    let embedded_source = std::rc::Rc::new(std::cell::Cell::new((0, 0)));
    let pixbuf = if is_raw(path) {
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
            let source = embedded_source.clone();
            loader.connect_size_prepared(move |loader, w, h| {
                source.set((w, h));
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
        found.ok_or("No supported embedded JPEG preview.")?
    } else {
        gdk_pixbuf::Pixbuf::from_file_at_scale(path, max, max, true).map_err(|e| e.to_string())?
    };
    let swaps = pixbuf
        .option("orientation")
        .and_then(|v| v.parse::<u8>().ok())
        .is_some_and(|v| (5..=8).contains(&v));
    let pixbuf = pixbuf.apply_embedded_orientation().unwrap_or(pixbuf);
    let source = gdk_pixbuf::Pixbuf::file_info(path)
        .map(|(_, w, h)| (w, h))
        .unwrap_or_else(|| {
            if embedded_source.get().0 > 0 {
                embedded_source.get()
            } else {
                (pixbuf.width(), pixbuf.height())
            }
        });
    let source = if swaps { (source.1, source.0) } else { source };
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
                && supported(p)
        })
        .take(10_000)
        .collect();
    paths.sort_by_cached_key(|p| natural_key(&p.file_name().unwrap_or_default().to_string_lossy()));
    paths
}

pub const RAW_EXTENSIONS: &[&str] = &[
    "dng", "cr2", "cr3", "nef", "nrw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "ptx",
    "srw", "x3f", "iiq", "3fr", "ari", "bay", "crw", "dcr", "drf", "erf", "fff", "k25", "kdc",
    "mef", "mos", "mrw", "rwl",
];
pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| RAW_EXTENSIONS.contains(&s.to_ascii_lowercase().as_str()))
}
pub fn supported(path: &Path) -> bool {
    is_raw(path)
        || path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
            ["jpg", "jpeg", "png", "tif", "tiff"].contains(&s.to_ascii_lowercase().as_str())
        })
}
fn natural_key(name: &str) -> String {
    let mut result = String::new();
    let mut digits = String::new();
    for c in name.to_lowercase().chars().chain(std::iter::once('\0')) {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                let value = digits.trim_start_matches('0');
                result.push_str(&format!("{:08}{value}", value.len()));
                digits.clear();
            }
            result.push(c);
        }
    }
    result
}
#[cfg(test)]
mod tests {
    #[test]
    fn natural_photo_order_and_raw_parity() {
        assert!(super::natural_key("photo9.JPG") < super::natural_key("photo10.jpg"));
        assert!(super::is_raw(std::path::Path::new("CAMERA.CR3")));
    }
}
