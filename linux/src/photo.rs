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
    let mut raw_orientation = None;
    let pixbuf = if is_raw(path) {
        // Preview only: extract an embedded camera JPEG, never develop RAW on the UI thread.
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        use std::io::Read;
        let mut bytes = Vec::new();
        file.take(128 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        raw_orientation = tiff_orientation(&bytes);
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
    // Camera JPEG previews can say "normal" even when the RAW's primary TIFF
    // directory records a portrait shot. Replace that tag before applying it,
    // so the camera orientation is applied exactly once, including reflections.
    if let Some(orientation) = raw_orientation {
        pixbuf.remove_option("orientation");
        pixbuf.set_option("orientation", &orientation.to_string());
    }
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

/// Read only the primary image's orientation in TIFF-based RAW containers
/// (NEF, DNG, ARW, CR2). Other containers retain the embedded JPEG's orientation.
fn tiff_orientation(bytes: &[u8]) -> Option<u16> {
    let little = match bytes.get(..4)? {
        b"II\x2a\x00" => true,
        b"MM\x00\x2a" => false,
        _ => return None,
    };
    let short = |offset: usize| -> Option<u16> {
        let value = bytes.get(offset..offset.checked_add(2)?)?.try_into().ok()?;
        Some(if little {
            u16::from_le_bytes(value)
        } else {
            u16::from_be_bytes(value)
        })
    };
    let long = |offset: usize| -> Option<u32> {
        let value = bytes.get(offset..offset.checked_add(4)?)?.try_into().ok()?;
        Some(if little {
            u32::from_le_bytes(value)
        } else {
            u32::from_be_bytes(value)
        })
    };
    let directory = usize::try_from(long(4)?).ok()?;
    let count = short(directory)? as usize;
    let start = directory.checked_add(2)?;
    let end = start.checked_add(count.checked_mul(12)?)?;
    bytes.get(start..end)?;
    for index in 0..count {
        let offset = start + index * 12;
        if short(offset)? == 0x0112 && short(offset + 2)? == 3 && long(offset + 4)? == 1 {
            return short(offset + 8).filter(|v| (1..=8).contains(v));
        }
    }
    None
}

pub fn folder(path: &Path) -> Vec<PathBuf> {
    let Some(parent) = path.parent() else {
        return Vec::new();
    };
    in_directory(parent)
}
/// Supported photographs in a directory, in natural filename order.
pub fn in_directory(parent: &Path) -> Vec<PathBuf> {
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
    use super::*;

    fn raw_header(orientation: u16, little: bool) -> Vec<u8> {
        let short = |v: u16| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let long = |v: u32| {
            if little {
                v.to_le_bytes()
            } else {
                v.to_be_bytes()
            }
        };
        let mut bytes = Vec::from(if little { *b"II" } else { *b"MM" });
        bytes.extend(short(42));
        bytes.extend(long(8));
        bytes.extend(short(1));
        bytes.extend(short(0x0112));
        bytes.extend(short(3));
        bytes.extend(long(1));
        bytes.extend(short(orientation));
        bytes.extend(short(0));
        bytes.extend(long(0));
        bytes
    }

    #[test]
    fn raw_orientation_handles_byte_order_and_invalid_metadata() {
        for little in [true, false] {
            for orientation in 1..=8 {
                let bytes = raw_header(orientation, little);
                assert_eq!(tiff_orientation(&bytes), Some(orientation));
                for size in 0..22 {
                    assert_eq!(tiff_orientation(&bytes[..size]), None);
                }
            }
            for invalid in [0, 9, u16::MAX] {
                assert_eq!(tiff_orientation(&raw_header(invalid, little)), None);
            }
        }
        for offset in [0_u32, 7, u32::MAX] {
            let mut bytes = raw_header(8, true);
            bytes[4..8].copy_from_slice(&offset.to_le_bytes());
            assert_eq!(tiff_orientation(&bytes), None);
        }
        let mut bytes = raw_header(8, true);
        bytes[12..14].copy_from_slice(&4_u16.to_le_bytes());
        assert_eq!(tiff_orientation(&bytes), None);
        bytes = raw_header(8, true);
        bytes[14..18].copy_from_slice(&2_u32.to_le_bytes());
        assert_eq!(tiff_orientation(&bytes), None);
        assert_eq!(tiff_orientation(b"not a TIFF"), None);
    }

    // Needs the desktop image decoder service, which is unavailable in the
    // filesystem sandbox. Run explicitly with --ignored outside that sandbox.
    #[test]
    #[ignore = "requires the GdkPixbuf image decoder service"]
    fn raw_preview_uses_container_orientation_once() {
        let pixbuf = gdk_pixbuf::Pixbuf::from_mut_slice(
            (0..20)
                .flat_map(|y| {
                    (0..40).flat_map(move |x| {
                        [
                            if x < 20 { 240 } else { 10 },
                            if y < 10 { 240 } else { 10 },
                            10,
                        ]
                    })
                })
                .collect::<Vec<u8>>(),
            gdk_pixbuf::Colorspace::Rgb,
            false,
            8,
            40,
            20,
            120,
        );
        let encoded = pixbuf
            .save_to_bufferv("jpeg", &[("quality", "100")])
            .unwrap();
        // Give the preview a conflicting EXIF tag to catch double rotations
        // and ensure that a valid container tag of 1 also takes precedence.
        let mut exif = Vec::from(*b"Exif\0\0");
        exif.extend(raw_header(6, true));
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend(((exif.len() + 2) as u16).to_be_bytes());
        jpeg.extend(exif);
        jpeg.extend(&encoded[2..]);
        let directory = std::env::temp_dir().join(format!(
            "negs-orientation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("portrait.NEF");
        // Top-left source corner for each EXIF transform, including mirrors.
        let corners = [
            (240, 240),
            (10, 240),
            (10, 10),
            (240, 10),
            (240, 240),
            (240, 10),
            (10, 10),
            (10, 240),
        ];
        for orientation in 1..=8 {
            let mut bytes = raw_header(orientation, orientation % 2 == 0);
            bytes.extend(&jpeg);
            std::fs::write(&path, &bytes).unwrap();
            let photo = decode(&path, 192).unwrap();
            let dimensions = if orientation >= 5 { (20, 40) } else { (40, 20) };
            assert_eq!((photo.width, photo.height), dimensions);
            assert_eq!(photo.source, dimensions);
            let (r, g) = corners[orientation as usize - 1];
            assert!(
                (photo.bytes[0] as i32 - r).abs() < 10,
                "orientation {orientation}"
            );
            assert!(
                (photo.bytes[1] as i32 - g).abs() < 10,
                "orientation {orientation}"
            );
            let small = decode(&path, 10).unwrap();
            assert_eq!(
                (small.width, small.height),
                if orientation >= 5 { (5, 10) } else { (10, 5) }
            );
            assert_eq!(small.source, dimensions);
        }
        let jpeg_path = directory.join("portrait.jpg");
        std::fs::write(&jpeg_path, &jpeg).unwrap();
        let photo = decode(&jpeg_path, 40).unwrap();
        assert_eq!((photo.width, photo.height), (20, 40));
        assert_eq!(photo.source, (20, 40));
        // An absent/invalid container orientation preserves the preview tag.
        let mut bytes = raw_header(0, true);
        bytes.extend(&jpeg);
        std::fs::write(&path, bytes).unwrap();
        let photo = decode(&path, 192).unwrap();
        assert_eq!((photo.width, photo.height), (20, 40));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn natural_photo_order_and_raw_parity() {
        assert!(super::natural_key("photo9.JPG") < super::natural_key("photo10.jpg"));
        assert!(super::is_raw(std::path::Path::new("CAMERA.CR3")));
    }
}
