//! Private, serial JSON-lines protocol for the native macOS client.
//! stdout is protocol-only; diagnostics go to stderr. EOF releases all caches.
use anyhow::{Context, Result, bail, ensure};
use rayon::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, preview::*, profile};
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_io::{
    border::{BorderStyle, add_border},
    crop::Crop,
};
use spektrafilm_math::{image::ImageBuf, precision::to_f32};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::Instant,
};

#[derive(Deserialize)]
struct Request {
    id: u64,
    #[serde(flatten)]
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    Catalog,
    Render {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        limit_preview: bool,
        preview_path: PathBuf,
    },
    Save {
        render_id: u64,
        output: PathBuf,
        #[serde(default)]
        border: Option<BorderStyle>,
        #[serde(default)]
        crop: Option<Crop>,
    },
}

#[derive(Default)]
struct Session {
    source_generation: u64,
    backend: Option<Box<dyn ComputeBackend>>,
    source: Option<(PathBuf, spektrafilm_io::RawWhiteBalance, ImageBuf)>,
    /// The source resized to the working/preview dimensions, so edits that
    /// don't change the size skip a full-resolution resample.
    scaled: Option<((u32, u32), ImageBuf)>,
    pipeline: Option<(String, Pipeline)>,
    /// The last render: request ID, source, film, rotation, sRGB-encoded, image.
    output: Option<(u64, PathBuf, String, i32, bool, ImageBuf)>,
}

fn catalog(data: &Path) -> Result<Value> {
    let mut profiles = Vec::new();
    for entry in std::fs::read_dir(data.join("profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let stock = path.file_stem().unwrap().to_string_lossy().into_owned();
        let p = profile::load_profile_by_name(data, &stock)?;
        profiles.push(json!({
            "id": stock, "name": p.info.name.as_ref().unwrap_or(&stock),
            "paper": p.info.support == "paper" || p.info.stage == "printing",
            "positive": p.is_positive(), "bw": p.is_bw(),
            "target_print": p.info.target_print, "development_times": p.data.development_time,
        }));
    }
    profiles.sort_by_key(|p| p["name"].as_str().unwrap_or_default().to_lowercase());
    ensure!(
        !profiles.is_empty(),
        "No profiles found in {}",
        data.display()
    );
    Ok(json!({"protocol": 1, "profiles": profiles, "defaults": RuntimeParams::default()}))
}

impl Session {
    fn handle(&mut self, request: Request, data: &Path) -> Result<Value> {
        match request.command {
            Command::Catalog => catalog(data),
            Command::Save {
                render_id,
                output,
                border,
                crop,
            } => {
                let (id, source, film, rotation, encoded, image) =
                    self.output.as_ref().context("No preview to save")?;
                ensure!(
                    *id == render_id,
                    "The preview changed; wait for the current render and try again"
                );
                ensure!(
                    source != &output,
                    "Choose a different output path to preserve the source"
                );
                if let Some(crop) = &crop {
                    crop.validate()?;
                }
                let metadata = spektrafilm_io::read_metadata(source);
                // Crop in the unrotated orientation, then rotate, then add the border.
                let cropped = crop.map(|crop| crop.apply(image));
                let image = cropped.as_ref().unwrap_or(image);
                let rotated =
                    (rotation.rem_euclid(4) != 0).then(|| image.clone().rotated_cw(*rotation));
                let image = rotated.as_ref().unwrap_or(image);
                match border {
                    Some(style) => spektrafilm_io::save_image(
                        &add_border(image, style, film, *encoded, data)?,
                        &output,
                        metadata.as_ref(),
                    )?,
                    None => spektrafilm_io::save_image(image, &output, metadata.as_ref())?,
                }
                Ok(json!({"saved": output}))
            }
            Command::Render {
                input,
                source_generation,
                film,
                paper,
                mut params,
                limit_preview,
                preview_path,
            } => {
                let started = Instant::now();
                ensure!(
                    params.io.upscale_factor.is_finite()
                        && (0.1..=1.0).contains(&params.io.upscale_factor),
                    "Invalid working resolution"
                );
                ensure!(
                    params.io.output_color_space == "sRGB",
                    "Native preview requires sRGB output"
                );
                params.io.raw_white_balance.validate()?;
                // Linear PNG/RAW loader behavior must match the CLI exporter.
                if spektrafilm_io::input_is_linear_srgb(&input) {
                    params.io.input_color_space = "sRGB".into();
                    params.io.input_cctf_decoding = false;
                }
                params.settings.serial_grain = false;
                let wb = params.io.raw_white_balance;
                let reload = self.source_generation != source_generation
                    || self.source.as_ref().is_none_or(|(path, old_wb, _)| {
                        path != &input || (spektrafilm_io::is_raw(&input) && *old_wb != wb)
                    });
                if reload {
                    let image = spektrafilm_io::load_image_with_wb(
                        &input,
                        spektrafilm_io::Formats::Any,
                        wb,
                    )?;
                    self.source = Some((input.clone(), wb, image));
                    self.scaled = None;
                    self.source_generation = source_generation;
                }
                let key = preview_pipeline_cache_key(&film, &paper, &params);
                if self.pipeline.as_ref().is_none_or(|(old, _)| old != &key) {
                    let film_profile = profile::load_profile_by_name(data, &film)?;
                    let print_profile = profile::load_profile_by_name(
                        data,
                        if params.io.scan_film { &film } else { &paper },
                    )?;
                    let pipeline = Pipeline::new_with_spectral(
                        film_profile,
                        print_profile,
                        (*params).clone(),
                        data,
                    )
                    .map_err(anyhow::Error::msg)?;
                    self.pipeline = Some((key, pipeline));
                }
                let source = &self.source.as_ref().unwrap().2;
                let (source_width, source_height) = (source.width, source.height);
                let dimensions = preview_dimensions(
                    source_width,
                    source_height,
                    params.io.upscale_factor,
                    limit_preview,
                );
                params.io.upscale_factor = 1.0;
                let pipeline = self
                    .pipeline
                    .as_ref()
                    .unwrap()
                    .1
                    .clone()
                    .with_params((*params).clone());
                let working = working_source(source, &mut self.scaled, dimensions);
                let backend = self
                    .backend
                    .get_or_insert_with(spektrafilm_gpu::select_backend);
                let output = if pipeline.params.io.scan_film && pipeline.film.is_positive() {
                    pipeline.process(working.clone(), backend.as_ref())
                } else {
                    pipeline
                        .process_resident_borrowed(working, backend.as_ref())
                        .unwrap_or_else(|| pipeline.process(working.clone(), backend.as_ref()))
                };
                // Transport raw RGBA, avoiding PNG compression on every slider edit.
                // Save retains the original float output, independent of this display proxy.
                let (width, height, bytes) = display_rgba(&output, params.io.output_cctf_encoding);
                std::fs::write(&preview_path, bytes).context("Writing preview buffer")?;
                let response = json!({
                    "width": width, "height": height, "output_width": output.width, "output_height": output.height,
                    "source_width": source_width, "source_height": source_height,
                    "raw": spektrafilm_io::is_raw(&input), "backend": backend.name(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                });
                self.output = Some((
                    request.id,
                    input,
                    film,
                    params.io.rotate_cw,
                    params.io.output_cctf_encoding,
                    output,
                ));
                Ok(response)
            }
        }
    }
}

/// The source at `dimensions`. A resampled copy is kept until the source or
/// the size changes, so ordinary edits don't resample the full image again.
fn working_source<'a>(
    source: &'a ImageBuf,
    scaled: &'a mut Option<((u32, u32), ImageBuf)>,
    dimensions: (u32, u32),
) -> &'a ImageBuf {
    if dimensions == (source.width, source.height) {
        *scaled = None;
        return source;
    }
    if scaled.as_ref().is_none_or(|(size, _)| *size != dimensions) {
        *scaled = None; // release the previous size before allocating
        *scaled = Some((dimensions, rescale_preview_input_fast(source, dimensions)));
    }
    &scaled.as_ref().unwrap().1
}

fn display_rgba(image: &ImageBuf, encoded: bool) -> (u32, u32, Vec<u8>) {
    let scale = (8192.0 / image.width.max(image.height) as f64).min(1.0);
    let width = (image.width as f64 * scale).round().max(1.0) as u32;
    let height = (image.height as f64 * scale).round().max(1.0) as u32;
    let small = (width != image.width || height != image.height)
        .then(|| rescale_preview_input_fast(image, (width, height)));
    let display = small.as_ref().unwrap_or(image);
    let mut bytes = vec![255; width as usize * height as usize * 4];
    bytes
        .par_chunks_exact_mut(4)
        .zip(display.data.par_chunks_exact(3))
        .for_each(|(dst, src)| {
            for c in 0..3 {
                let mut v = to_f32(src[c]);
                if !encoded {
                    v = if v <= 0.0031308 {
                        v * 12.92
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    };
                }
                dst[c] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        });
    (width, height, bytes)
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 3 || args[1] != "--data-dir" {
        bail!("Usage: spektrafilm-native --data-dir PATH");
    }
    let data = PathBuf::from(&args[2]);
    let mut session = Session::default();
    let mut stdout = std::io::stdout().lock();
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        let id = serde_json::from_str::<Value>(&line)
            .ok()
            .and_then(|v| v["id"].as_u64())
            .unwrap_or(0);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<Value> {
            session.handle(serde_json::from_str(&line)?, &data)
        }))
        .unwrap_or_else(|_| {
            // A panicked GPU/decoder may have invalidated its cached state.
            session = Session::default();
            Err(anyhow::anyhow!(
                "The image engine failed. Try a different image or settings."
            ))
        });
        let response = match result {
            Ok(result) => json!({"id": id, "result": result}),
            Err(error) => json!({"id": id, "error": format!("{error:#}")}),
        };
        serde_json::to_writer(&mut stdout, &response)?;
        writeln!(stdout)?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linear_display_is_encoded_without_mutating_output() {
        let image = ImageBuf::from_data(1, 1, vec![0.0, 0.18, 1.0]);
        assert_eq!(display_rgba(&image, false).2, vec![0, 118, 255, 255]);
        assert_eq!(display_rgba(&image, true).2, vec![0, 46, 255, 255]);
        assert_eq!(image.data[1], 0.18);
    }
    #[test]
    fn working_source_is_resampled_once_per_size() {
        let source = ImageBuf::from_data(4, 2, vec![0.5; 24]);
        let mut scaled = None;
        let first = working_source(&source, &mut scaled, (2, 1)).data.as_ptr();
        assert_eq!(
            working_source(&source, &mut scaled, (2, 1)).data.as_ptr(),
            first
        );
        assert_eq!(working_source(&source, &mut scaled, (1, 1)).width, 1);
        let full = working_source(&source, &mut scaled, (4, 2));
        assert_eq!(full.data.as_ptr(), source.data.as_ptr());
        assert!(scaled.is_none());
    }
    #[test]
    fn stale_preview_cannot_be_saved() {
        let mut session = Session {
            output: Some((
                2,
                "source.png".into(),
                "kodak_gold_200".into(),
                0,
                true,
                ImageBuf::from_data(1, 1, vec![0.0; 3]),
            )),
            ..Default::default()
        };
        let request = Request {
            id: 3,
            command: Command::Save {
                render_id: 1,
                output: "unused.png".into(),
                border: None,
                crop: None,
            },
        };
        assert!(
            session
                .handle(request, Path::new("."))
                .unwrap_err()
                .to_string()
                .contains("preview changed")
        );
    }
}
