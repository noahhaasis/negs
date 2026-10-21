// Write the fixed inputs of the cross-platform golden test
// (`scripts/golden/run.py`) as linear 32-bit float TIFFs.
//
// Usage:
//   golden_inputs [--long-edge N] <out_dir> [input.raw ...]
//
// RAWs are decoded with rawler on every platform (never Apple's decoder) and
// shrunk by an integer box filter to at most N pixels on the long edge
// (default 1024). The synthetic patterns use integer arithmetic and a fixed
// order of IEEE `+ * /` (correctly rounded on every platform), so every
// platform writes the same bytes. Float TIFFs
// load without any gamma maths, so a golden run tests only the pipeline.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use spektrafilm_io::raw::{RawDecodeOptions, RawDecoder, RawlerDecoder};
use spektrafilm_math::precision::to_f32;
use tiff::encoder::{TiffEncoder, colortype::RGB32Float};

const USAGE: &str = "usage: golden_inputs [--long-edge N] <out_dir> [input.raw ...]";
const SYNTH_W: u32 = 768;
const SYNTH_H: u32 = 512;

fn main() -> Result<()> {
    let mut long_edge = 1024u32;
    let mut positional = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--long-edge" => {
                long_edge = args.next().ok_or_else(|| anyhow!(USAGE))?.parse()?;
            }
            _ => positional.push(arg),
        }
    }
    let Some((out_dir, raws)) = positional.split_first() else {
        bail!(USAGE);
    };
    let out_dir = Path::new(out_dir);
    std::fs::create_dir_all(out_dir)?;

    let mut stems = std::collections::HashSet::new();
    for raw in raws {
        let path = Path::new(raw);
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("raw");
        if !stems.insert(stem.to_owned()) {
            bail!("two inputs are named {stem}; rename one");
        }
        let img = RawlerDecoder
            .decode(path, RawDecodeOptions::default())
            .with_context(|| format!("decoding {}", path.display()))?;
        let data: Vec<f64> = img.data.iter().map(|&v| to_f32(v) as f64).collect();
        let (w, h, data) = box_downscale(img.width, img.height, &data, long_edge);
        if w == 0 || h == 0 {
            bail!("{} shrinks to {w}×{h}; raise --long-edge", path.display());
        }
        write(&out_dir.join(format!("photo_{stem}.tif")), w, h, &data)?;
    }

    let synthetic: [(&str, fn(u32, u32) -> [f64; 3]); 4] =
        [("ramp", ramp), ("gamut", gamut), ("noise", noise), ("edges", edges)];
    for (name, pattern) in synthetic {
        let mut data = Vec::with_capacity((SYNTH_W * SYNTH_H * 3) as usize);
        for y in 0..SYNTH_H {
            for x in 0..SYNTH_W {
                data.extend_from_slice(&pattern(x, y));
            }
        }
        write(&out_dir.join(format!("synth_{name}.tif")), SYNTH_W, SYNTH_H, &data)?;
    }
    Ok(())
}

/// Average k×k blocks, k the smallest integer that fits `long_edge`. The
/// summing order is fixed, so the result is the same everywhere.
fn box_downscale(w: u32, h: u32, data: &[f64], long_edge: u32) -> (u32, u32, Vec<f64>) {
    let k = w.max(h).div_ceil(long_edge.max(1)).max(1);
    let (ow, oh) = (w / k, h / k);
    let mut out = Vec::with_capacity((ow * oh * 3) as usize);
    let n = (k * k) as f64;
    for oy in 0..oh {
        for ox in 0..ow {
            for c in 0..3 {
                let mut sum = 0.0f64;
                for y in oy * k..(oy + 1) * k {
                    for x in ox * k..(ox + 1) * k {
                        sum += data[((y * w + x) * 3 + c) as usize];
                    }
                }
                out.push(sum / n);
            }
        }
    }
    (ow, oh, out)
}

fn write(path: &Path, w: u32, h: u32, data: &[f64]) -> Result<()> {
    let floats: Vec<f32> = data.iter().map(|&v| v as f32).collect();
    let f = File::create(path).with_context(|| format!("creating {}", path.display()))?;
    TiffEncoder::new(BufWriter::new(f))?
        .write_image::<RGB32Float>(w, h, &floats)
        .with_context(|| format!("writing {}", path.display()))?;
    eprintln!("[golden_inputs] wrote {} ({w}×{h})", path.display());
    Ok(())
}

/// Linear ramps from 0 to 16 in six tints, one band each.
fn ramp(x: u32, y: u32) -> [f64; 3] {
    const TINTS: [[f64; 3]; 6] = [
        [1.0, 1.0, 1.0],
        [1.0, 0.2, 0.05],
        [0.05, 1.0, 0.2],
        [0.2, 0.05, 1.0],
        [1.0, 1.0, 0.05],
        [0.6, 0.3, 1.0],
    ];
    let v = x as f64 / (SYNTH_W - 1) as f64 * 16.0;
    let t = TINTS[(y * 6 / SYNTH_H) as usize];
    [v * t[0], v * t[1], v * t[2]]
}

/// Saturated hues (columns) at exposures from 1/32 to 4 (rows).
fn gamut(x: u32, y: u32) -> [f64; 3] {
    const HUES: [[f64; 3]; 12] = [
        [1.0, 0.0, 0.0],
        [1.0, 0.5, 0.0],
        [1.0, 1.0, 0.0],
        [0.5, 1.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 1.0, 0.5],
        [0.0, 1.0, 1.0],
        [0.0, 0.5, 1.0],
        [0.0, 0.0, 1.0],
        [0.5, 0.0, 1.0],
        [1.0, 0.0, 1.0],
        [1.0, 0.0, 0.5],
    ];
    const LEVELS: [f64; 8] = [0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0, 2.0, 4.0];
    let hue = HUES[(x * 12 / SYNTH_W) as usize];
    let level = LEVELS[(y * 8 / SYNTH_H) as usize];
    [hue[0] * level, hue[1] * level, hue[2] * level]
}

/// Uniform noise in [0, 4) from xorshift64*, seeded per pixel, with black
/// and very bright spikes.
fn noise(x: u32, y: u32) -> [f64; 3] {
    let i = (y * SYNTH_W + x) as u64;
    if i % 97 == 0 {
        return [0.0; 3];
    }
    if i % 89 == 0 {
        return [100.0; 3];
    }
    let mut s = i.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = || {
        s ^= s >> 12;
        s ^= s << 25;
        s ^= s >> 27;
        (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64 * 4.0
    };
    [next(), next(), next()]
}

/// A 32 px checkerboard of 0.002 and 40 with a bright disk in the middle.
fn edges(x: u32, y: u32) -> [f64; 3] {
    let (dx, dy) = (x as i64 - (SYNTH_W / 2) as i64, y as i64 - (SYNTH_H / 2) as i64);
    if dx * dx + dy * dy < 80 * 80 {
        return [200.0, 150.0, 80.0];
    }
    let v = if (x / 32 + y / 32) % 2 == 0 { 0.002 } else { 40.0 };
    [v, v, v]
}
