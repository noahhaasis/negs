//! Adds a border to an 8-bit sRGB image's bytes as the web app's export does
//! (`addBorder` in `crates/spektrafilm-wasm`: each byte / 255 as an encoded
//! sRGB value, `add_border`, `extend_for_instagram` with `--instagram`,
//! rounded back to bytes), and writes them as a binary PPM. It's the
//! native reference `web/checks/border.mjs` compares the browser's with.
//!
//! cargo run --release -p spektrafilm-io --example border_bytes -- IN.ppm OUT.ppm STYLE OPTIONS_JSON FILM [--instagram] [DATA_DIR]

use std::path::Path;

use anyhow::{Context, Result};
use spektrafilm_io::border::{BorderOptions, BorderStyle, add_border, extend_for_instagram};
use spektrafilm_io::encoding::Encoding;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output, style, options, film, rest @ ..] = &args[..] else {
        anyhow::bail!("usage: border_bytes IN.ppm OUT.ppm STYLE OPTIONS_JSON FILM [--instagram] [DATA_DIR]");
    };
    let instagram = rest.iter().any(|a| a == "--instagram");
    let data = rest.iter().find(|a| *a != "--instagram").map_or("data", String::as_str);
    let style: BorderStyle = style.parse()?;
    let options: BorderOptions = serde_json::from_str(options)?;
    let ppm = std::fs::read(input).with_context(|| format!("reading {input}"))?;
    let (width, height, rgb) = read_ppm(&ppm)?;
    let data_buf = rgb.iter().map(|&v| from_f32(v as f32 / 255.0)).collect();
    let image = ImageBuf::from_data(width, height, data_buf);
    let encoding = Encoding::new("sRGB", true);
    let mut out = add_border(&image, style, &options, film, encoding, Path::new(data))?;
    if instagram {
        out = extend_for_instagram(&out, style, &options, encoding);
    }
    let bytes: Vec<u8> = out.data.iter().map(|&v| (to_f32(v).clamp(0.0, 1.0) * 255.0).round() as u8).collect();
    let mut ppm = format!("P6\n{} {}\n255\n", out.width, out.height).into_bytes();
    ppm.extend_from_slice(&bytes);
    std::fs::write(output, ppm).with_context(|| format!("writing {output}"))?;
    eprintln!("{} × {}", out.width, out.height);
    Ok(())
}

/// A binary PPM's size and its RGB bytes, as `web/serve.mjs` writes them.
fn read_ppm(bytes: &[u8]) -> Result<(u32, u32, &[u8])> {
    let mut fields = Vec::new();
    let mut at = 0;
    while fields.len() < 4 {
        let start = at;
        while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
            at += 1;
        }
        if at > start {
            fields.push(std::str::from_utf8(&bytes[start..at])?);
        }
        at += 1;
        anyhow::ensure!(at <= bytes.len(), "not a PPM");
    }
    anyhow::ensure!(fields[0] == "P6" && fields[3] == "255", "not an 8-bit binary PPM");
    let (width, height): (u32, u32) = (fields[1].parse()?, fields[2].parse()?);
    let rgb = bytes.get(at..at + (width * height * 3) as usize).context("the PPM is short")?;
    Ok((width, height, rgb))
}
