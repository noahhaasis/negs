//! Images the canvas shows: the developed preview, the placeholder and the
//! original. Each is uploaded to the GPU on the thread that made it, with
//! mipmaps, so the UI thread never copies pixels. Until the GPU is ready the
//! pixels wait in memory. A small copy stays for the histogram.

use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use anyhow::{Result, ensure};

use crate::crop::CropSettings;
use crate::gpu::{self, GpuImage};
use crate::histogram::Scopes;

pub const MAX_PREVIEW: u32 = 8192;
/// The histogram samples at most a million pixels; this copy has them.
const SMALL: u32 = 1024;

struct Inner {
    cpu: Mutex<Option<Vec<u8>>>,
    /// The pixels stay in memory after the upload (`Picture::kept`).
    keep: bool,
    texture: OnceLock<GpuImage>,
    small: (Vec<u8>, u32, u32),
}

/// A shared image; clones are cheap.
#[derive(Clone)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub id: u64,
    /// The pixels have Rec. 2020 primaries (the helper's proxy of a render
    /// outside sRGB); otherwise sRGB. Both use the sRGB curve.
    pub wide: bool,
    inner: Arc<Inner>,
}

impl Picture {
    /// Reads the helper's RGBA buffer (alpha ignored), kept in memory
    /// after the upload (`kept`): the original.
    pub fn read_kept(path: &Path, width: u32, height: u32) -> Result<Picture> {
        ensure!(width > 0 && height > 0 && width <= MAX_PREVIEW && height <= MAX_PREVIEW, "Invalid preview dimensions");
        let bytes = std::fs::read(path)?;
        ensure!(bytes.len() == width as usize * height as usize * 4, "The image engine returned an invalid preview buffer.");
        Ok(Picture::kept(bytes, width, height))
    }

    pub fn new(rgba: Vec<u8>, width: u32, height: u32) -> Picture {
        Picture::made(rgba, width, height, false)
    }

    /// A picture whose pixels stay in memory after the upload too: the
    /// original, which Compare also draws from its pixels.
    pub fn kept(rgba: Vec<u8>, width: u32, height: u32) -> Picture {
        Picture::made(rgba, width, height, true)
    }

    fn made(mut rgba: Vec<u8>, width: u32, height: u32, keep: bool) -> Picture {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        for alpha in rgba.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let small = downsample(&rgba, width, height);
        let picture = Picture {
            width,
            height,
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            wide: false,
            inner: Arc::new(Inner { cpu: Mutex::new(Some(rgba)), keep, texture: OnceLock::new(), small }),
        };
        picture.upload();
        picture
    }

    /// Marks the pixels as the helper's `display_color_space` names them.
    pub fn in_display_space(mut self, name: &str) -> Picture {
        self.wide = is_wide(name);
        self
    }

    /// Uploads the pixels if the GPU is ready; a no-op otherwise or when done.
    pub fn upload(&self) {
        let Some(gpu) = gpu::shared() else { return };
        let mut cpu = self.inner.cpu.lock().unwrap();
        if let Some(rgba) = cpu.as_ref() {
            if self.inner.texture.get().is_none() {
                let image = gpu.upload(rgba, self.width, self.height);
                let _ = self.inner.texture.set(image);
            }
            if !self.inner.keep {
                *cpu = None;
            }
        }
    }

    /// The pixels are in memory (`with_pixels` has them).
    pub fn has_pixels(&self) -> bool {
        self.inner.cpu.lock().unwrap().is_some()
    }

    /// A kept picture lets its pixels go once it is on the GPU; until then
    /// (no GPU yet, the software canvas) they stay.
    pub fn release_kept(&self) {
        if self.inner.texture.get().is_some() {
            *self.inner.cpu.lock().unwrap() = None;
        }
    }

    /// Tests, which have no GPU: what an upload leaves in memory.
    #[cfg(test)]
    pub fn as_if_uploaded(&self) {
        if !self.inner.keep {
            *self.inner.cpu.lock().unwrap() = None;
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

    /// The histogram and waveform of the crop, the waveform across the
    /// photograph as shown, mirrored and turned.
    pub fn scopes(&self, crop: &CropSettings, orientation: crate::crop::Orientation) -> Scopes {
        let (rgba, w, h) = &self.inner.small;
        Scopes::shown(rgba, *w as usize, *h as usize, crop, orientation, 1 << 20)
    }

}

/// The helper's name for the Rec. 2020 proxy space.
pub const WIDE_DISPLAY_SPACE: &str = "Rec. 2020 (sRGB transfer)";

pub fn is_wide(display_space: &str) -> bool {
    display_space == WIDE_DISPLAY_SPACE
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

/// Gives every clear texel the colour of the nearest texel that shows.
/// Sampling, mipmaps and blending all work in straight alpha, so a clear
/// texel's colour leaks into its neighbours: `film.png` is white where it's
/// clear, and drawn small its black edge got a light rim. Core Animation
/// premultiplies, so the Mac never showed it; the exporter draws the file
/// as it is.
fn bleed_into_clear(rgba: &mut [u8], width: u32, height: u32) {
    let (w, h) = (width as usize, height as usize);
    let clear = |rgba: &[u8], i: usize| rgba[i * 4 + 3] == 0;
    let mut done: Vec<bool> = (0..w * h).map(|i| !clear(rgba, i)).collect();
    let neighbours = |i: usize| {
        let (x, y) = (i % w, i / w);
        [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)].into_iter().flatten()
    };
    // Breadth first from the visible texels on the clear part's edge, so
    // each clear one copies a neighbour one step nearer to what shows.
    let mut queue: std::collections::VecDeque<usize> = (0..w * h).filter(|&i| done[i] && neighbours(i).any(|n| !done[n])).collect();
    while let Some(i) = queue.pop_front() {
        for n in neighbours(i) {
            if !done[n] {
                done[n] = true;
                rgba.copy_within(i * 4..i * 4 + 3, n * 4);
                queue.push_back(n);
            }
        }
    }
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
    pub fn new_rgba_alpha(mut rgba: Vec<u8>, width: u32, height: u32) -> Picture {
        bleed_into_clear(&mut rgba, width, height);
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let picture = Picture {
            width,
            height,
            id: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            wide: false,
            inner: Arc::new(Inner { cpu: Mutex::new(Some(rgba)), keep: false, texture: OnceLock::new(), small: (vec![], 0, 0) }),
        };
        picture.upload();
        picture
    }

    fn empty() -> Picture {
        Picture { width: 0, height: 0, id: 0, wide: false, inner: Arc::new(Inner { cpu: Mutex::new(None), keep: false, texture: OnceLock::new(), small: (vec![], 0, 0) }) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The film frame's clear middle is white in the file; after the bleed
    /// it's the frame's near-black, and nothing that shows has changed.
    #[test]
    fn clear_texels_take_the_nearest_visible_colour() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data/borders/film.png");
        let original = image::open(path).unwrap().to_rgba8();
        let (w, h) = original.dimensions();
        let mut bled = original.clone().into_raw();
        bleed_into_clear(&mut bled, w, h);
        let raw = original.as_raw();
        let (mut clear, mut sum) = (0u64, 0u64);
        for (before, after) in raw.chunks_exact(4).zip(bled.chunks_exact(4)) {
            assert_eq!(before[3], after[3]);
            if before[3] > 0 {
                assert_eq!(before, after);
            } else {
                // Dust specks in the scan are grey; nothing clear stays white.
                assert!(after[..3] != [255; 3], "a clear texel kept its white");
                clear += 1;
                sum += after[..3].iter().map(|&v| v as u64).sum::<u64>();
            }
        }
        assert!(clear > 0 && sum / (3 * clear) < 32, "clear texels average {}", sum / (3 * clear));
        let mut tiny = vec![255, 255, 255, 0, 10, 20, 30, 255, 255, 255, 255, 0];
        bleed_into_clear(&mut tiny, 3, 1);
        assert_eq!(tiny, [10, 20, 30, 0, 10, 20, 30, 255, 10, 20, 30, 0]);
        let mut empty = vec![255, 255, 255, 0];
        bleed_into_clear(&mut empty, 1, 1);
        assert_eq!(empty, [255, 255, 255, 0]);
    }
}
