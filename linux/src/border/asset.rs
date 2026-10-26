//! Trusted, bundled border PNGs: decode once at native resolution, without the
//! photograph loader's scaling, orientation, metadata pass or histogram.
use std::{collections::VecDeque, fs::File, io::BufReader, path::Path};

pub struct Decoded {
    pub bytes: Vec<u8>,
    pub width: i32,
    pub height: i32,
    pub alpha: bool,
    pub stride: usize,
    pub window: [f64; 4],
}

pub fn load(data: &Path, key: &str) -> Result<Decoded, String> {
    let (path, window) = if key == "film" {
        (data.join("borders/film.png"), [0., 0., 1., 1.])
    } else {
        let (format, film) = key.split_once('/').ok_or("Invalid border key")?;
        let root = data.join("borders/35mm");
        let frames: serde_json::Value = serde_json::from_slice(
            &std::fs::read(root.join("frames.json")).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let mut window = [0.; 4];
        for (i, value) in window.iter_mut().enumerate() {
            *value = frames[format]["window"][i]
                .as_f64()
                .ok_or("Invalid film gate")?;
        }
        let path = root.join(format).join(format!("{film}.png"));
        (
            if path.is_file() {
                path
            } else {
                root.join(format).join("generic.png")
            },
            window,
        )
    };
    let mut decoder =
        png::Decoder::new(BufReader::new(File::open(path).map_err(|e| e.to_string())?));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let size = reader
        .output_buffer_size()
        .filter(|n| *n <= 64 * 1024 * 1024)
        .ok_or("Border PNG exceeds the decode limit")?;
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes).map_err(|e| e.to_string())?;
    if !matches!(info.color_type, png::ColorType::Rgb | png::ColorType::Rgba) {
        return Err("Border PNG must be RGB or RGBA".into());
    }
    bytes.truncate(info.buffer_size());
    Ok(Decoded {
        bytes,
        width: info.width as i32,
        height: info.height as i32,
        alpha: info.color_type == png::ColorType::Rgba,
        stride: info.line_size,
        window,
    })
}

// Keep GPU textures as well as their backing bytes alive across style/stock
// switches, with a strict budget. Never retain the whole stock catalog.
pub struct Cache<T> {
    entries: VecDeque<(String, usize, T)>,
    bytes: usize,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
        }
    }
}
const CACHE_BYTES: usize = 96 * 1024 * 1024;
const CACHE_ENTRIES: usize = 4;
impl<T: Clone> Cache<T> {
    pub fn get(&mut self, key: &str) -> Option<T> {
        let index = self.entries.iter().position(|(k, _, _)| k == key)?;
        let entry = self.entries.remove(index)?;
        let value = entry.2.clone();
        self.entries.push_back(entry);
        Some(value)
    }
    pub fn insert(&mut self, key: String, bytes: usize, value: T) {
        if let Some(index) = self.entries.iter().position(|(k, _, _)| k == &key) {
            self.bytes -= self.entries.remove(index).unwrap().1;
        }
        if bytes > CACHE_BYTES {
            return;
        }
        while self.bytes + bytes > CACHE_BYTES || self.entries.len() >= CACHE_ENTRIES {
            self.bytes -= self.entries.pop_front().unwrap().1;
        }
        self.bytes += bytes;
        self.entries.push_back((key, bytes, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cache_reuses_recent_textures_and_bounds_memory() {
        let mut cache = Cache::default();
        let mb = 1024 * 1024;
        cache.insert("film".into(), 15 * mb, 1);
        cache.insert("full/a".into(), 51 * mb, 2);
        assert_eq!(cache.get("film"), Some(1));
        cache.insert("full/b".into(), 51 * mb, 3);
        assert_eq!(cache.get("full/a"), None);
        assert_eq!(cache.get("film"), Some(1));
        assert_eq!(cache.get("full/b"), Some(3));
        for n in 0..10 {
            cache.insert(n.to_string(), 1, n);
        }
        assert_eq!(cache.entries.len(), CACHE_ENTRIES);
        assert!(cache.bytes <= CACHE_BYTES);
        cache.insert("too large".into(), CACHE_BYTES + 1, 20);
        assert_eq!(cache.get("too large"), None);
        cache.insert("9".into(), 2, 99);
        assert_eq!(cache.get("9"), Some(99));
        assert_eq!(cache.bytes, 5);
    }

    #[test]
    fn bundled_pngs_keep_native_pixels_and_gate_geometry() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
        let film = load(&data, "film").unwrap();
        assert_eq!((film.width, film.height), (2518, 1550));
        assert!(film.alpha);
        assert_eq!(film.bytes.len(), film.stride * film.height as usize);
        for format in ["full", "half"] {
            let dir = data.join("borders/35mm").join(format);
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.extension().is_none_or(|ext| ext != "png") {
                    continue;
                }
                let key = format!("{format}/{}", path.file_stem().unwrap().to_str().unwrap());
                let frame = load(&data, &key).unwrap();
                assert_eq!(
                    (frame.width, frame.height),
                    (if format == "full" { 3800 } else { 1900 }, 3500)
                );
                assert!(frame.alpha);
                assert_eq!(frame.bytes.len(), frame.stride * frame.height as usize);
                // The photo gate remains transparent; alpha detail outside it
                // is preserved verbatim, including partially opaque film edges.
                let center =
                    (frame.height as usize / 2) * frame.stride + (frame.width as usize / 2) * 4;
                assert_eq!(frame.bytes[center + 3], 0);
                assert_eq!(frame.window, [0.026316, 0.157143, 0.947368, 0.685714]);
            }
            let fallback = load(&data, &format!("{format}/missing-stock")).unwrap();
            let generic = load(&data, &format!("{format}/generic")).unwrap();
            assert_eq!(fallback.bytes, generic.bytes);
        }
    }
}
