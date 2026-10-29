//! Reproducible decode-only timing/quality probe. No film rendering or file
//! writing is included in the timings. Optional artifacts stay local.
use std::{path::PathBuf, time::Instant};

use anyhow::{Context, Result, bail};
use serde_json::json;
use spektrafilm_io::{
    RawWhiteBalanceMode,
    raw::{RawDecodeOptions, RawDecoderPreference, decode_raw},
};
use spektrafilm_math::precision::to_f32;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let mut decoder = RawDecoderPreference::Auto;
    let mut iterations = 1usize;
    let mut options = RawDecodeOptions::default();
    let mut output = None;
    let mut input = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--decoder" => decoder = args.next().context("missing decoder")?.parse()?,
            "--iterations" => iterations = args.next().context("missing iterations")?.parse()?,
            "--no-orient" => options.apply_orientation = false,
            "--lens-correction" => options.lens_correction.enabled = true,
            "--wb" => {
                options.white_balance =
                    serde_json::from_value(json!({"mode":args.next().context("missing WB")?}))?
            }
            "--temperature" => {
                options.white_balance.mode = RawWhiteBalanceMode::Custom;
                options.white_balance.temperature =
                    args.next().context("missing temperature")?.parse()?;
            }
            "--tint" => {
                options.white_balance.mode = RawWhiteBalanceMode::Custom;
                options.white_balance.tint = args.next().context("missing tint")?.parse()?;
            }
            "--output" => {
                output = Some(PathBuf::from(
                    args.next().context("missing output directory")?,
                ))
            }
            "--help" => {
                println!(
                    "raw_decoder_probe [--decoder auto|apple|rawler] [--iterations N] [--no-orient] [--lens-correction] [--wb as_shot|daylight|tungsten|custom] [--temperature K] [--tint T] [--output DIR] INPUT"
                );
                return Ok(());
            }
            _ if input.is_none() => input = Some(PathBuf::from(arg)),
            _ => bail!("unexpected argument {arg}"),
        }
    }
    anyhow::ensure!(
        (1..=100).contains(&iterations),
        "iterations must be 1..=100"
    );
    let input = input.context("missing INPUT")?;
    let mut times = Vec::new();
    let mut result = None;
    for _ in 0..iterations {
        drop(result.take());
        let start = Instant::now();
        match decode_raw(&input, options, decoder) {
            Ok(decoded) => {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
                result = Some(decoded);
            }
            Err(error) => {
                println!(
                    "{}",
                    json!({"success":false,"error":format!("{error:#}"),"requested":format!("{decoder:?}")})
                );
                std::process::exit(1);
            }
        }
    }
    let decoded = result.unwrap();
    let image = decoded.image;
    let data: Vec<f32> = image.data.iter().copied().map(to_f32).collect();
    let mut minimum = [f32::INFINITY; 3];
    let mut maximum = [f32::NEG_INFINITY; 3];
    let mut sum = [0f64; 3];
    let (mut zero, mut above_one, mut nonfinite) = (0usize, 0usize, 0usize);
    for pixel in data.chunks_exact(3) {
        for c in 0..3 {
            let v = pixel[c];
            minimum[c] = minimum[c].min(v);
            maximum[c] = maximum[c].max(v);
            sum[c] += f64::from(v);
            zero += usize::from(v == 0.0);
            above_one += usize::from(v > 1.0);
            nonfinite += usize::from(!v.is_finite());
        }
    }
    if let Some(out) = &output {
        std::fs::create_dir_all(out)?;
        let mut bytes = Vec::with_capacity(data.len() * 4);
        for value in &data {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        std::fs::write(out.join("linear-rgb.f32"), bytes)?;
        let display = image::RgbImage::from_fn(image.width, image.height, |x, y| {
            let i = ((y * image.width + x) * 3) as usize;
            image::Rgb(std::array::from_fn(|c| {
                let v = data[i + c].max(0.0);
                let encoded = if v <= 0.0031308 {
                    12.92 * v
                } else {
                    1.055 * v.powf(1.0 / 2.4) - 0.055
                };
                (encoded.clamp(0.0, 1.0) * 255.0).round() as u8
            }))
        });
        image::DynamicImage::ImageRgb8(display)
            .thumbnail(1600, 1200)
            .save(out.join("preview.png"))?;
    }
    let report = json!({"success":true,"input":input.file_name().unwrap().to_string_lossy(),"decoder":decoded.decoder,
        "requested":format!("{decoder:?}"),"white_balance":options.white_balance,"oriented":options.apply_orientation,
        "lens_correction":options.lens_correction.enabled,"lens_correction_supported":decoded.lens_correction_supported,
        "width":image.width,"height":image.height,"decode_ms":times,"min":minimum,"max":maximum,
        "mean":sum.map(|s|s/image.pixel_count() as f64),"zeros":zero,"above_one":above_one,"nonfinite":nonfinite});
    if let Some(out) = output {
        std::fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    }
    println!("{report}");
    Ok(())
}
