// Decode a camera RAW file with the SAME loader the GUI uses
// (`spektrafilm_io::load_raw`) and write the linear-sRGB result as a
// 32-bit float TIFF. The parity harness can then run both pipelines
// (Python and Rust) on the pixels the GUI actually sees, so RAW-decoder
// differences don't affect the comparison.
//
// Usage:
//   decode_raw_gui [--no-orient] <input.raw> <output.tif>
//
// The EXIF orientation is applied by default, as in the GUI.
// `--no-orient` keeps the sensor's native layout (the output of older
// versions of this tool).
//
// Linear sRGB primaries, AlphaMode::None, no gamma. The pipeline will
// decode the input as `input_color_space = "sRGB"` with
// `input_cctf_decoding = false` (matches the GUI defaults for RAW).

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use spektrafilm_math::precision::to_f32;
use tiff::encoder::{TiffEncoder, colortype::RGB32Float};

const USAGE: &str = "usage: decode_raw_gui [--no-orient] <input.raw> <output.tif>";

fn main() -> Result<()> {
    let mut orient = true;
    let mut positional = Vec::new();
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--no-orient" => orient = false,
            _ => positional.push(arg),
        }
    }
    let [input, output] = <[String; 2]>::try_from(positional).map_err(|_| anyhow!(USAGE))?;
    let input = Path::new(&input);
    let output = Path::new(&output);

    eprintln!("[decode_raw_gui] decoding {} via rawler …", input.display());
    let img = if orient {
        spektrafilm_io::load_raw(input)?
    } else {
        spektrafilm_io::load_raw_unoriented(input)?
    };
    let (w, h) = (img.width, img.height);
    eprintln!("[decode_raw_gui] decoded {}×{} → linear sRGB", w, h);
    // Exact: every value started out as an f32.
    let floats: Vec<f32> = img.data.into_iter().map(to_f32).collect();

    let f = File::create(output).with_context(|| format!("creating {}", output.display()))?;
    let mut encoder = TiffEncoder::new(BufWriter::new(f)).context("init TiffEncoder")?;
    encoder
        .write_image::<RGB32Float>(w, h, &floats)
        .context("write TIFF image")?;
    eprintln!("[decode_raw_gui] wrote {}", output.display());
    Ok(())
}
