//! Renders the 35mm border textures in `data/borders/35mm`.
//!
//! One PNG per film profile and format, drawn to ISO 1007 135 dimensions:
//! a black rebate, KS perforations, the stock's edge print, a fixed frame
//! number and a DX barcode. Each texture is laid out with the strip
//! horizontal and is transparent where the photograph goes; `frames.json`
//! records that window. The textures can be replaced by scans as long as
//! the window stays centred and `frames.json` describes it.
//!
//! cargo run --release -p spektrafilm-io --example film35_borders [-- DATA_DIR [PX_PER_MM]]

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::Path;

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use anyhow::{Context, Result};
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{ImageEncoder, Rgba, RgbaImage};

/// Edge print for each profile. The Kodak letter codes (GB, GC) follow
/// Kodak's edge-print tables; check new entries against real negatives.
const EDGE_PRINT: &[(&str, &str)] = &[
    ("fujifilm_c200", "FUJICOLOR C200"),
    ("fujifilm_pro_400h", "FUJI PRO 400H"),
    ("fujifilm_provia_100f", "FUJI RDP III"),
    ("fujifilm_velvia_100", "FUJI RVP 100"),
    ("fujifilm_xtra_400", "FUJICOLOR X-TRA 400"),
    ("kodak_2302", "KODAK 2302"),
    ("kodak_2383", "KODAK 2383"),
    ("kodak_2393", "KODAK 2393"),
    ("kodak_doublex", "EASTMAN 5222"),
    ("kodak_ektachrome_100", "KODAK E100"),
    ("kodak_ektar_100", "KODAK EKTAR 100"),
    ("kodak_gold_200", "KODAK GB 200"),
    ("kodak_kodachrome_64", "KODACHROME 64"),
    ("kodak_portra_160", "KODAK PORTRA 160"),
    ("kodak_portra_400", "KODAK PORTRA 400"),
    ("kodak_portra_800", "KODAK PORTRA 800"),
    ("kodak_portra_800_push1", "KODAK PORTRA 800"),
    ("kodak_portra_800_push2", "KODAK PORTRA 800"),
    ("kodak_trix", "KODAK 7266"),
    ("kodak_ultramax_400", "KODAK GC 400"),
    ("kodak_verita_200d", "KODAK VERITA 200D"),
    ("kodak_vision3_200t", "KODAK 5213"),
    ("kodak_vision3_250d", "KODAK 5207"),
    ("kodak_vision3_500t", "KODAK 5219"),
    ("kodak_vision3_50d", "KODAK 5203"),
];

const FRAME_NUMBER: u32 = 12;

// 135 film, in millimetres.
const FILM_WIDTH: f32 = 35.0;
const PERF_PITCH: f32 = 4.75;
const PERF_SIZE: (f32, f32) = (2.794, 1.981);
const PERF_EDGE: f32 = 2.01;
const PERF_RADIUS: f32 = 0.5;
const GATE_HEIGHT: f32 = 24.0;
const GATE_RADIUS: f32 = 0.35;
const TEXT_SIZE: f32 = 1.25;
const TOP_BASELINE: f32 = 1.48;
const BOTTOM_BASELINE: f32 = FILM_WIDTH - 0.55;

const PERF: [u8; 3] = [247, 244, 236];

#[derive(Clone, Copy)]
enum Format {
    /// 36 × 24 mm, eight perforations.
    Full,
    /// 18 × 24 mm, four perforations.
    Half,
}

impl Format {
    fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Half => "half",
        }
    }
    /// Strip length and gate width.
    fn size(self) -> (f32, f32) {
        match self {
            Self::Full => (8.0 * PERF_PITCH, 36.0),
            Self::Half => (4.0 * PERF_PITCH, 18.0),
        }
    }
    /// The gate as fractions of the texture: [x, y, width, height].
    fn window(self) -> [f32; 4] {
        let (length, gate) = self.size();
        let y = (FILM_WIDTH - GATE_HEIGHT) / 2.0;
        [
            (length - gate) / 2.0 / length,
            y / FILM_WIDTH,
            gate / length,
            GATE_HEIGHT / FILM_WIDTH,
        ]
    }
}

/// The rebate and the edge print's colour as they look in a scan.
fn colors(profile: &serde_json::Value) -> ([u8; 3], [u8; 3]) {
    let info = &profile["info"];
    if info["channel_model"] == "bw" {
        ([26, 26, 25], [216, 216, 212])
    } else if info["type"] == "positive" {
        ([13, 11, 10], [233, 217, 138])
    } else {
        ([13, 11, 10], [242, 164, 60])
    }
}

struct Canvas {
    image: RgbaImage,
    /// Pixels per millimetre.
    scale: f32,
}

impl Canvas {
    fn new(length: f32, scale: f32, base: [u8; 3]) -> Self {
        let (w, h) = (
            (length * scale).round() as u32,
            (FILM_WIDTH * scale).round() as u32,
        );
        let [r, g, b] = base;
        Self {
            image: RgbaImage::from_pixel(w, h, Rgba([r, g, b, 255])),
            scale,
        }
    }

    /// Blends `color` into the pixel by `coverage`, keeping its alpha.
    fn blend(&mut self, x: i32, y: i32, color: [u8; 3], coverage: f32) {
        if x < 0 || y < 0 || coverage <= 0.0 {
            return;
        }
        let Some(pixel) = self.image.get_pixel_mut_checked(x as u32, y as u32) else {
            return;
        };
        let k = coverage.min(1.0);
        for (c, &v) in pixel.0.iter_mut().zip(&color) {
            *c = (*c as f32 + (v as f32 - *c as f32) * k).round() as u8;
        }
    }

    /// Calls `f` with each pixel near the rectangle (in mm) and the
    /// rectangle's antialiased coverage of it.
    fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        radius: f32,
        mut f: impl FnMut(&mut Rgba<u8>, f32),
    ) {
        let s = self.scale;
        let (cx, cy, hx, hy, r) = (
            (x + w / 2.0) * s,
            (y + h / 2.0) * s,
            w / 2.0 * s,
            h / 2.0 * s,
            radius * s,
        );
        let (x0, y0) = (
            ((x * s).floor() as i32 - 1).max(0),
            ((y * s).floor() as i32 - 1).max(0),
        );
        let x1 = (((x + w) * s).ceil() as i32 + 1).min(self.image.width() as i32);
        let y1 = (((y + h) * s).ceil() as i32 + 1).min(self.image.height() as i32);
        for py in y0..y1 {
            for px in x0..x1 {
                let qx = (px as f32 + 0.5 - cx).abs() - (hx - r);
                let qy = (py as f32 + 0.5 - cy).abs() - (hy - r);
                let outside = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - r;
                let coverage = (0.5 - outside).clamp(0.0, 1.0);
                if coverage > 0.0 {
                    f(self.image.get_pixel_mut(px as u32, py as u32), coverage);
                }
            }
        }
    }

    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: [u8; 3]) {
        self.rounded_rect(x, y, w, h, radius, |pixel, k| {
            for (c, &v) in pixel.0.iter_mut().zip(&color) {
                *c = (*c as f32 + (v as f32 - *c as f32) * k).round() as u8;
            }
        });
    }

    /// Makes the rectangle transparent, leaving the rebate's colour in it.
    fn cut(&mut self, x: f32, y: f32, w: f32, h: f32, radius: f32) {
        self.rounded_rect(x, y, w, h, radius, |pixel, k| {
            pixel.0[3] = (pixel.0[3] as f32 * (1.0 - k)).round() as u8;
        });
    }

    /// A small triangle pointing along the strip, `left` or right, 4×4 supersampled.
    fn arrow(&mut self, x: f32, baseline: f32, left: bool, color: [u8; 3]) {
        let s = self.scale;
        let (w, h) = (0.62 * s, 0.78 * s);
        let (x0, y0) = (x * s, baseline * s - 0.08 * s - h);
        for py in (y0.floor() as i32)..=((y0 + h).ceil() as i32) {
            for px in (x0.floor() as i32)..=((x0 + w).ceil() as i32) {
                let mut inside = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let u = (px as f32 + (sx as f32 + 0.5) / 4.0 - x0) / w;
                        let v = (py as f32 + (sy as f32 + 0.5) / 4.0 - y0) / h;
                        let u = if left { 1.0 - u } else { u };
                        if (0.0..=1.0).contains(&u) && (v - 0.5).abs() <= 0.5 * (1.0 - u) {
                            inside += 1;
                        }
                    }
                }
                self.blend(px, py, color, inside as f32 / 16.0);
            }
        }
    }

    fn text(&mut self, font: &FontRef, text: &str, x: f32, baseline: f32, color: [u8; 3]) -> f32 {
        let scale = PxScale::from(TEXT_SIZE * self.scale);
        let scaled = font.as_scaled(scale);
        let tracking = TEXT_SIZE * 0.06 * self.scale;
        let mut pen = x * self.scale;
        for c in text.chars() {
            let id = font.glyph_id(c);
            let glyph = id.with_scale_and_position(scale, point(pen, baseline * self.scale));
            if let Some(outline) = font.outline_glyph(glyph) {
                let bounds = outline.px_bounds();
                outline.draw(|gx, gy, k| {
                    self.blend(
                        bounds.min.x as i32 + gx as i32,
                        bounds.min.y as i32 + gy as i32,
                        color,
                        k,
                    )
                });
            }
            pen += scaled.h_advance(id) + tracking;
        }
        pen / self.scale
    }

    /// A two-track DX code: a regular clock track over a data track.
    fn dx(&mut self, x: f32, width: f32, seed: u32, color: [u8; 3]) {
        let n = 24;
        let pitch = width / n as f32;
        let y = FILM_WIDTH - 1.62;
        for i in 0..n {
            let bx = x + i as f32 * pitch;
            self.fill(bx, y, pitch * 0.55, 0.42, 0.0, color);
            if (seed >> (i % 16)) & 1 == 1 || i % 6 == 0 {
                self.fill(bx, y + 0.52, pitch * 0.55, 0.42, 0.0, color);
            }
        }
    }
}

fn strip(
    font: &FontRef,
    format: Format,
    name: &str,
    base: [u8; 3],
    ink: [u8; 3],
    scale: f32,
) -> RgbaImage {
    let (length, gate) = format.size();
    let mut canvas = Canvas::new(length, scale, base);
    canvas.cut(
        (length - gate) / 2.0,
        (FILM_WIDTH - GATE_HEIGHT) / 2.0,
        gate,
        GATE_HEIGHT,
        GATE_RADIUS,
    );
    let perfs = (length / PERF_PITCH).round() as usize;
    for i in 0..perfs {
        let x = PERF_PITCH * (i as f32 + 0.5) - PERF_SIZE.0 / 2.0;
        for y in [PERF_EDGE, FILM_WIDTH - PERF_EDGE - PERF_SIZE.1] {
            canvas.fill(x, y, PERF_SIZE.0, PERF_SIZE.1, PERF_RADIUS, PERF);
        }
    }
    let seed = name.bytes().fold(0x811c_9dc5u32, |h, b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    });
    match format {
        Format::Full => {
            canvas.text(font, name, 5.2, TOP_BASELINE, ink);
            canvas.arrow(30.0, TOP_BASELINE, true, ink);
            canvas.text(font, &FRAME_NUMBER.to_string(), 4.4, BOTTOM_BASELINE, ink);
            canvas.dx(9.5, 10.5, seed, ink);
            canvas.arrow(22.2, BOTTOM_BASELINE, false, ink);
            canvas.text(
                font,
                &format!("{FRAME_NUMBER}A"),
                23.0,
                BOTTOM_BASELINE,
                ink,
            );
        }
        Format::Half => {
            canvas.text(font, name, 2.0, TOP_BASELINE, ink);
            canvas.arrow(2.0, BOTTOM_BASELINE, false, ink);
            canvas.text(font, &format!("{FRAME_NUMBER}A"), 2.8, BOTTOM_BASELINE, ink);
            canvas.dx(9.0, 8.0, seed, ink);
        }
    }
    canvas.image
}

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
    let scale: f32 = args.next().map(|s| s.parse()).transpose()?.unwrap_or(150.0);
    let data = Path::new(&data);
    let font = FontRef::try_from_slice(include_bytes!("fonts/BarlowCondensed-SemiBold.ttf"))?;

    let mut films = Vec::new();
    for entry in fs::read_dir(data.join("profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let profile: serde_json::Value = serde_json::from_reader(File::open(&path)?)?;
        if profile["info"]["support"] != "film" {
            continue;
        }
        let stock = path.file_stem().unwrap().to_string_lossy().into_owned();
        let name = match EDGE_PRINT.iter().find(|(id, _)| *id == stock) {
            Some((_, name)) => name.to_string(),
            None => {
                let name = profile["info"]["name"]
                    .as_str()
                    .unwrap_or(&stock)
                    .to_uppercase();
                eprintln!("warning: no edge print for {stock}; using {name:?}");
                name
            }
        };
        let (base, ink) = colors(&profile);
        films.push((stock, name, base, ink));
    }
    films.sort();
    films.push((
        "generic".into(),
        String::new(),
        [13, 11, 10],
        [242, 164, 60],
    ));

    let root = data.join("borders/35mm");
    let mut frames = serde_json::Map::new();
    for format in [Format::Full, Format::Half] {
        let dir = root.join(format.name());
        fs::create_dir_all(&dir)?;
        for (stock, name, base, ink) in &films {
            save(
                &strip(&font, format, name, *base, *ink, scale),
                &dir.join(format!("{stock}.png")),
            )?;
        }
        let window = format.window().map(|v| (v as f64 * 1e6).round() / 1e6);
        frames.insert(
            format.name().into(),
            serde_json::json!({ "window": window }),
        );
        eprintln!("{}: {} textures", dir.display(), films.len());
    }
    fs::write(
        root.join("frames.json"),
        serde_json::to_string_pretty(&frames)? + "\n",
    )?;
    Ok(())
}
