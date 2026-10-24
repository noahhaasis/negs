//! Images the canvas shows: the developed preview, the placeholder and the
//! original. Each is uploaded to the GPU on the thread that made it, with
//! mipmaps, so the UI thread never copies pixels. Until the GPU is ready the
//! pixels wait in memory. A small copy stays for the histogram.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Result, ensure};

use crate::crop::CropSettings;
use crate::gpu::{self, GpuImage};
use crate::histogram::Histogram;

pub const MAX_PREVIEW: u32 = 8192;
/// The histogram samples at most a million pixels; this copy has them.
const SMALL: u32 = 1024;

struct Inner {
    cpu: Mutex<Option<Vec<u8>>>,
    texture: OnceLock<GpuImage>,
    small: (Vec<u8>, u32, u32),
}

/// A shared image; clones are cheap.
#[derive(Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub id: u64,
    inner: Arc<Inner>,
}

impl Picture {
    /// Reads the helper's RGBA buffer (alpha ignored).
    pub fn read(path: &Path, width: u32, height: u32) -> Result<Picture> {
        ensure!(width > 0 && height > 0 && width <= MAX_PREVIEW && height <= MAX_PREVIEW, "Invalid preview dimensions");
        let bytes = std::fs::read(path)?;
        ensure!(bytes.len() == width as usize * height as usize * 4, "The image engine returned an invalid preview buffer.");
        Ok(Picture::new(bytes, width, height))
    }

    pub fn new(mut rgba: Vec<u8>, width: u32, height: u32) -> Picture {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let small = downsample(&rgba, width, height);
        let picture = Picture {
            width,
            height,
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            inner: Arc::new(Inner { cpu: Mutex::new(Some(rgba)), texture: OnceLock::new(), small }),
        };
        picture.upload();
        picture
    }

    /// Uploads the pixels if the GPU is ready; a no-op otherwise or when done.
    pub fn upload(&self) {
        let Some(gpu) = gpu::shared() else { return };
        let mut cpu = self.inner.cpu.lock().unwrap();
        if let Some(rgba) = cpu.as_ref() {
            let image = gpu.upload(rgba, self.width, self.height);
            let _ = self.inner.texture.set(image);
            *cpu = None;
        }
    }

    pub fn texture(&self) -> Option<&GpuImage> {
        if self.inner.texture.get().is_none() {
            self.upload();
        }
        self.inner.texture.get()
    }

    /// Borrows retained pixels for software presentation. GPU uploads still
    /// release their full-size CPU buffer, so the normal path costs no more.
    pub fn with_pixels<R>(&self, f: impl FnOnce(&[u8]) -> R) -> Option<R> {
        let cpu = self.inner.cpu.lock().unwrap();
        cpu.as_deref().map(f)
    }

    pub fn histogram(&self, crop: &CropSettings) -> Histogram {
        let (rgba, w, h) = &self.inner.small;
        Histogram::of(rgba, *w as usize, *h as usize, crop, 1 << 20)
    }

    pub fn same(&self, other: &Picture) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

/// Nearest-neighbour copy at most `SMALL` pixels on its long side.
fn downsample(rgba: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let scale = (width.max(height) as f64 / SMALL as f64).max(1.0);
    let (w, h) = (((width as f64 / scale).round() as u32).max(1), ((height as f64 / scale).round() as u32).max(1));
    let mut out = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        let sy = ((y as f64 + 0.5) * height as f64 / h as f64) as usize;
        for x in 0..w {
            let sx = ((x as f64 + 0.5) * width as f64 / w as f64) as usize;
            let p = (sy.min(height as usize - 1) * width as usize + sx.min(width as usize - 1)) * 4;
            out.extend_from_slice(&rgba[p..p + 4]);
        }
    }
    (out, w, h)
}

/// The film frame and 35mm strips, decoded once off the UI thread.
pub struct BorderTextures {
    pub film: Option<Picture>,
    pub strips: Mutex<Vec<(String, crate::border::FilmFormat, Picture)>>,
    pub data: std::path::PathBuf,
}

static BORDERS: OnceLock<BorderTextures> = OnceLock::new();

pub fn load_border_textures(data: &Path) {
    let data = data.to_path_buf();
    std::thread::spawn(move || {
        let film = image::open(data.join("borders").join("film.png")).ok().map(|image| {
            let image = image.to_rgba8();
            let (w, h) = image.dimensions();
            Picture::new_rgba_alpha(image.into_raw(), w, h)
        });
        let _ = BORDERS.set(BorderTextures { film, strips: Mutex::new(vec![]), data });
        crate::ui::post(|app: &mut crate::ui::App| app.model.dirty |= crate::model::dirty::CANVAS);
    });
}

pub fn borders() -> Option<&'static BorderTextures> {
    BORDERS.get()
}

/// The 35mm strip for `film` and `format`, loading it in the background the
/// first time it's asked for.
pub fn strip(film: &str, format: crate::border::FilmFormat) -> Option<Picture> {
    let borders = BORDERS.get()?;
    let mut strips = borders.strips.lock().unwrap();
    if let Some((_, _, picture)) = strips.iter().find(|(f, fmt, _)| f == film && *fmt == format) {
        return (picture.width > 0).then(|| picture.clone());
    }
    // A placeholder entry marks the load as started.
    strips.push((film.to_string(), format, Picture::empty()));
    let (film, data) = (film.to_string(), borders.data.clone());
    std::thread::spawn(move || {
        let loaded = crate::border::FilmStripTexture::load(&data, &film, format);
        let borders = BORDERS.get().unwrap();
        let mut strips = borders.strips.lock().unwrap();
        strips.retain(|(f, fmt, _)| !(f == &film && *fmt == format));
        match loaded {
            Some(strip) => {
                let (w, h) = strip.image.dimensions();
                strips.push((film, format, Picture::new_rgba_alpha(strip.image.into_raw(), w, h)));
                drop(strips);
                crate::ui::post(|app: &mut crate::ui::App| app.model.dirty |= crate::model::dirty::CANVAS);
            }
            None => {
                drop(strips);
                crate::ui::post(move |app: &mut crate::ui::App| {
                    app.model.error = Some(format!("The 35mm border for {} couldn’t be loaded.", app.model.film_name()));
                    app.model.dirty |= crate::model::dirty::ERROR;
                });
            }
        }
    });
    None
}

impl Picture {
    /// A texture whose alpha matters (borders).
    pub fn new_rgba_alpha(rgba: Vec<u8>, width: u32, height: u32) -> Picture {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let picture = Picture {
            width,
            height,
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            inner: Arc::new(Inner { cpu: Mutex::new(Some(rgba)), texture: OnceLock::new(), small: (vec![], 0, 0) }),
        };
        picture.upload();
        picture
    }

    fn empty() -> Picture {
        Picture { width: 0, height: 0, id: 0, inner: Arc::new(Inner { cpu: Mutex::new(None), texture: OnceLock::new(), small: (vec![], 0, 0) }) }
    }
}
