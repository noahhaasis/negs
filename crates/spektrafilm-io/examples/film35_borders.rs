//! Renders the macOS preview's 35mm strips into `data/borders/35mm`: one
//! PNG per film with an edge print, plus `generic.png`, for each format,
//! and `frames.json`, the gate as fractions of the strip. The exporter
//! draws the same strips itself at the export's resolution
//! (`spektrafilm_io::film35`); run this after changing that code or
//! `material.png`.
//!
//! cargo run --release -p spektrafilm-io --example film35_borders [-- DATA_DIR [PX_PER_MM]]

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, RgbaImage};
use spektrafilm_io::border::FilmFormat;
use spektrafilm_io::film35::{STOCKS, Strip};

fn save(image: &RgbaImage, path: &Path) -> Result<()> {
    let file =
        BufWriter::new(File::create(path).with_context(|| format!("creating {}", path.display()))?);
    PngEncoder::new_with_quality(file, CompressionType::Best, FilterType::Adaptive).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )?;
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let data = args.next().unwrap_or_else(|| "data".into());
    // 100 px/mm gives whole-pixel strips no wider than the preview's 4096.
    let scale: f32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(100.0);
    let data = Path::new(&data);
    let root = data.join("borders/35mm");
    let films = STOCKS.iter().map(|(id, ..)| *id).chain(["generic"]);
    let mut frames = serde_json::Map::new();
    for format in [FilmFormat::Full, FilmFormat::Half] {
        let dir = root.join(format.name());
        fs::create_dir_all(&dir)?;
        for film in films.clone() {
            let strip = Strip::load(data, film, format)?;
            save(&strip.preview(scale), &dir.join(format!("{film}.png")))?;
        }
        let window = Strip::window(format).map(|v| (v * 1e6).round() / 1e6);
        frames.insert(format.name().into(), serde_json::json!({ "window": window }));
        eprintln!("{}: {} strips", dir.display(), films.clone().count());
    }
    fs::write(
        root.join("frames.json"),
        serde_json::to_string_pretty(&frames)? + "\n",
    )?;
    Ok(())
}
