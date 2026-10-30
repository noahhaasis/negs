//! Independent native fixture for the phone's smaller working source.
//! cargo run --release -p spektrafilm-cli --example working_input -- INPUT OUT.tif MAX_PIXELS
//! Always rawler for RAWs, shrink before EXIF orientation, as the WASM decoder.
use anyhow::{Result, bail};
use spektrafilm_core::preview;
use spektrafilm_io::{Formats, is_raw, load_image, load_raw_bytes_unoriented};
use spektrafilm_math::precision::to_f32;
use std::{fs::File, io::{BufWriter, Write}, path::Path};
use tiff::encoder::{TiffEncoder, colortype::RGB32Float};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 { bail!("INPUT OUT.tif MAX_PIXELS"); }
    let max_pixels: f64 = args[2].parse()?;
    let path = Path::new(&args[0]);
    let (image, orientation) = if is_raw(path) {
        load_raw_bytes_unoriented(&std::fs::read(path)?, Default::default())?
    } else {
        (load_image(path, Formats::Any)?, 1)
    };
    let (w, h) = (image.width, image.height);
    let (long, short) = (w.max(h) as f64, w.min(h) as f64);
    let side = if w as f64 * h as f64 > max_pixels { (max_pixels * long / short).sqrt().floor() } else { long };
    let scale = (side / long).min(1.0);
    let size = (((w as f64 * scale).round() as u32).clamp(1, w), ((h as f64 * scale).round() as u32).clamp(1, h));
    let small = preview::downsample_area(&image, size).oriented(orientation);
    let data: Vec<f32> = small.data.iter().map(|&x| to_f32(x)).collect();
    TiffEncoder::new(BufWriter::new(File::create(&args[1])?))?
        .write_image::<RGB32Float>(small.width, small.height, &data)?;
    let mut raw = BufWriter::new(File::create(format!("{}.f32", args[1]))?);
    for v in data { raw.write_all(&v.to_le_bytes())?; }
    println!("{}", serde_json::json!({"width": small.width, "height": small.height,
        "fullWidth": if orientation >= 5 { h } else { w }, "fullHeight": if orientation >= 5 { w } else { h }}));
    Ok(())
}
