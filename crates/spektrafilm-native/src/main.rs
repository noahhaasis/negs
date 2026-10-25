//! Private, serial JSON-lines protocol for the native macOS client.
//! stdout is protocol-only; diagnostics go to stderr. EOF releases all caches.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use spektrafilm_core::{
    image_size,
    params::RuntimeParams,
    pipeline::Pipeline,
    post::{PostParams, PostProcess},
    preview::*,
    profile,
};
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_io::{
    border::{BorderOptions, BorderStyle, add_border, extend_for_instagram},
    crop::Crop,
    resize::{self, Fit},
};
use spektrafilm_math::image::ImageBuf;
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::Instant,
};

/// EditorModel.readPreview rejects larger preview buffers.
const DISPLAY_MAX_DIM: usize = 8192;
/// The only output colour space the helper renders.
const OUTPUT_SPACE: &str = "sRGB";

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
    ExportLut {
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        size: u32,
        output: PathBuf,
    },
    /// Unprocessed, identically framed source for the Windows original view.
    /// This does not replace the retained film render used by Save.
    Original {
        input: PathBuf,
        params: Box<RuntimeParams>,
        limit_preview: bool,
        preview_path: PathBuf,
    },
    Render {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        limit_preview: bool,
        preview_path: PathBuf,
        /// While a slider moves: render at this fraction of the photograph's
        /// size (the canvas resolution), a preview that is never saved.
        #[serde(default)]
        interactive_scale: Option<f32>,
        /// The crop, sent with an Image Size target only: the target describes
        /// the cropped photograph, so its extent sets the working size. The
        /// render itself is never cropped.
        #[serde(default)]
        crop: Option<Crop>,
    },
    Save {
        render_id: u64,
        output: PathBuf,
        #[serde(default)]
        border: Option<BorderStyle>,
        #[serde(default)]
        border_options: BorderOptions,
        /// Extend the border to Instagram's 3:4.
        #[serde(default)]
        instagram: bool,
        #[serde(default)]
        crop: Option<Crop>,
        /// Clockwise quarter turns. The preview is never rotated, so turning
        /// the photograph doesn't need a new render.
        #[serde(default)]
        rotate_cw: i32,
        /// Resample the saved file to this edge, after the border.
        #[serde(default)]
        fit: Option<Fit>,
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
    /// The same for interactive renders, kept beside `scaled` so a drag
    /// after a full-size render doesn't resample the source again.
    interactive: Option<((u32, u32), ImageBuf)>,
    /// The source's auto-exposure metering frame. Every render of the source,
    /// at any preview or working size, meters it, as the exporter does, so
    /// the preview's exposure is the export's.
    metering: Option<ImageBuf>,
    pipeline: Option<(String, Pipeline)>,
    output: Option<Rendered>,
}

/// The last render. `image` is the scan before the post adjustments, so a
/// post-only edit reuses it; `key` is everything else the render depended on.
struct Rendered {
    id: u64,
    key: Value,
    /// The response to the render, reused when only the post adjustments change.
    response: Value,
    source: PathBuf,
    film: String,
    encoded: bool,
    post: PostParams,
    image: ImageBuf,
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
            Command::ExportLut { film, paper, params, size, output } => {
                let started = Instant::now();
                let cube = spektrafilm_core::lut_export::cube(data, &film, &paper, *params, size)
                    .map_err(anyhow::Error::msg)?;
                std::fs::write(&output, cube)?;
                Ok(json!({"size": size, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}))
            }
            Command::Original { input, params, limit_preview, preview_path } => {
                ensure!(input != preview_path, "Choose a different preview path to preserve the source");
                ensure!(params.io.upscale_factor.is_finite()
                    && (0.1..=1.0).contains(&params.io.upscale_factor), "Invalid working resolution");
                params.io.raw_white_balance.validate()?;
                let source = spektrafilm_io::load_image_with_wb(
                    &input, spektrafilm_io::Formats::Any, params.io.raw_white_balance,
                )?;
                // Never enlarged: an Image Size target is ignored, and the
                // canvas fits the original to the developed photograph.
                let factor = if params.io.photo_long_side_px.is_some() { 1.0 } else { params.io.upscale_factor };
                let dimensions = preview_dimensions(source.width, source.height, factor, limit_preview);
                let resized = (dimensions != (source.width, source.height))
                    .then(|| rescale_preview_input_fast(&source, dimensions));
                let working = resized.as_ref().unwrap_or(&source);
                let encoded = !spektrafilm_io::input_is_linear_srgb(&input);
                let (width, height) = write_display(working, encoded, &PostParams::default(), &preview_path)?;
                Ok(json!({"width": width, "height": height}))
            }
            Command::Save {
                render_id,
                output,
                border,
                border_options,
                instagram,
                crop,
                rotate_cw,
                fit,
            } => {
                let Rendered {
                    id,
                    source,
                    film,
                    encoded,
                    post,
                    image,
                    ..
                } = self.output.as_ref().context("No preview to save")?;
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
                let adjusted = (!post.is_identity())
                    .then(|| PostProcess::new(post, *encoded, OUTPUT_SPACE).applied(image));
                let image = adjusted.as_ref().unwrap_or(image);
                let rotated =
                    (rotate_cw.rem_euclid(4) != 0).then(|| image.clone().rotated_cw(rotate_cw));
                let image = rotated.as_ref().unwrap_or(image);
                let bordered = match border {
                    Some(style) => {
                        let bordered =
                            add_border(image, style, &border_options, film, *encoded, data)?;
                        Some(if instagram {
                            extend_for_instagram(&bordered, style, &border_options, *encoded)
                        } else {
                            bordered
                        })
                    }
                    None => None,
                };
                let image = bordered.as_ref().unwrap_or(image);
                let resized = fit.and_then(|fit| resize::fit(image, fit, *encoded));
                let image = resized.as_ref().unwrap_or(image);
                spektrafilm_io::save_image(image, &output, metadata.as_ref())?;
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
                interactive_scale,
                crop,
            } => {
                let started = Instant::now();
                ensure!(
                    params.io.upscale_factor.is_finite()
                        && (0.1..=1.0).contains(&params.io.upscale_factor),
                    "Invalid working resolution"
                );
                ensure!(
                    interactive_scale.is_none_or(|s| s.is_finite() && s > 0.0),
                    "Invalid interactive scale"
                );
                ensure!(
                    params.io.output_color_space == OUTPUT_SPACE,
                    "Native preview requires sRGB output"
                );
                params.io.raw_white_balance.validate()?;
                // Linear PNG/RAW loader behavior must match the CLI exporter.
                if spektrafilm_io::input_is_linear_srgb(&input) {
                    params.io.input_color_space = "sRGB".into();
                    params.io.input_cctf_decoding = false;
                }
                params.settings.serial_grain = false;
                // Applied to the display and on save, so the pipeline runs without them.
                let post = std::mem::take(&mut params.post);
                let wb = params.io.raw_white_balance;
                let reload = self.source_generation != source_generation
                    || self.source.as_ref().is_none_or(|(path, old_wb, _)| {
                        path != &input || (spektrafilm_io::is_raw(&input) && *old_wb != wb)
                    });
                if reload {
                    // Release the last source's caches before decoding the next.
                    self.source = None;
                    self.scaled = None;
                    self.interactive = None;
                    self.metering = None;
                    let image = spektrafilm_io::load_image_with_wb(
                        &input,
                        spektrafilm_io::Formats::Any,
                        wb,
                    )?;
                    self.source = Some((input.clone(), wb, image));
                    self.source_generation = source_generation;
                }
                if params.camera.auto_exposure && self.metering.is_none() {
                    let source = &self.source.as_ref().unwrap().2;
                    self.metering = Some(spektrafilm_core::stages::filming::metering_preview(source));
                }
                let source = &self.source.as_ref().unwrap().2;
                let (source_width, source_height) = (source.width, source.height);
                // The uncapped frame an export develops, and the photograph the
                // crop cuts from it; an invalid or too-large request fails here,
                // before anything is allocated at that size.
                let size = image_size::resolve(&params, (source_width, source_height), crop.as_ref())?;
                image_size::clear(&mut params);
                // The photograph's preview, at most 12 MP if limited; an
                // interactive render is a smaller frame of it that never
                // enlarges the source.
                let photo_dimensions = preview_cap(size.working, limit_preview);
                let dimensions = match interactive_scale.filter(|&s| s < 1.0) {
                    Some(s) => interactive_dimensions(photo_dimensions, s, (source_width, source_height)),
                    None => photo_dimensions,
                };
                // The resolved size, not the crop: panning a crop, or any crop
                // with the same working size, reuses the render.
                let render_key = json!({
                    "input": input, "source_generation": source_generation, "film": film,
                    "paper": paper, "params": params, "limit_preview": limit_preview,
                    "interactive_scale": interactive_scale, "working": size.working,
                });
                let reused = self.output.take().filter(|last| last.key == render_key);
                if let Some(last) = reused {
                    write_display(&last.image, last.encoded, &post, &preview_path)?;
                    let mut response = last.response.clone();
                    // Edge rounding moves the crop's pixels with its position.
                    response["photo_crop_width"] = json!(size.photo.0);
                    response["photo_crop_height"] = json!(size.photo.1);
                    response["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                    self.output = Some(Rendered {
                        id: request.id,
                        post,
                        ..last
                    });
                    return Ok(response);
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
                let mut pipeline = self
                    .pipeline
                    .as_ref()
                    .unwrap()
                    .1
                    .clone()
                    .with_params((*params).clone());
                if let Some(metering) = &self.metering {
                    pipeline = pipeline.with_metering_preview(metering.clone());
                }
                let working = if dimensions == photo_dimensions {
                    working_source(
                        source,
                        &mut self.scaled,
                        dimensions,
                        rescale_preview_input_fast,
                    )
                } else {
                    working_source(source, &mut self.interactive, dimensions, downsample_area)
                };
                let backend = self
                    .backend
                    .get_or_insert_with(spektrafilm_gpu::select_backend);
                let (output, used_backend) = if pipeline.params.io.scan_film && pipeline.film.is_positive() {
                    pipeline.process_with_backend(working.clone(), backend.as_ref())
                } else {
                    pipeline
                        .process_resident_borrowed(working, backend.as_ref())
                        .map(|output| (output, backend.name()))
                        .unwrap_or_else(|| pipeline.process_with_backend(working.clone(), backend.as_ref()))
                };
                let encoded = params.io.output_cctf_encoding;
                let (width, height) = write_display(&output, encoded, &post, &preview_path)?;
                // At the uncapped working size the output is byte-identical to
                // a Fast (GPU) export, so the app can save it instead of
                // rendering again. The display proxy's limit doesn't matter:
                // Save uses this float output.
                let export_resolution = (output.width, output.height) == size.working;
                let response = json!({
                    "width": width, "height": height, "output_width": output.width, "output_height": output.height,
                    "photo_width": photo_dimensions.0, "photo_height": photo_dimensions.1,
                    "working_width": size.working.0, "working_height": size.working.1,
                    "photo_crop_width": size.photo.0, "photo_crop_height": size.photo.1,
                    "source_width": source_width, "source_height": source_height,
                    "export_resolution": export_resolution,
                    "raw": spektrafilm_io::is_raw(&input), "backend": used_backend,
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                });
                self.output = Some(Rendered {
                    id: request.id,
                    key: render_key,
                    response: response.clone(),
                    source: input,
                    film,
                    encoded,
                    post,
                    image: output,
                });
                Ok(response)
            }
        }
    }
}

/// Writes the preview for the app as raw RGBA, avoiding PNG compression on
/// every slider edit. Save keeps the float output, independent of this proxy.
fn write_display(
    image: &ImageBuf,
    encoded: bool,
    post: &PostParams,
    path: &Path,
) -> Result<(usize, usize)> {
    let post = (!post.is_identity()).then(|| PostProcess::new(post, encoded, OUTPUT_SPACE));
    let (width, height, bytes) =
        display_rgba_with_post(image, DISPLAY_MAX_DIM, encoded, post.as_ref());
    std::fs::write(path, bytes).context("Writing preview buffer")?;
    Ok((width, height))
}

/// The source at `dimensions`. A resampled copy is kept until the source or
/// the size changes, so ordinary edits don't resample the full image again.
fn working_source<'a>(
    source: &'a ImageBuf,
    scaled: &'a mut Option<((u32, u32), ImageBuf)>,
    dimensions: (u32, u32),
    resample: fn(&ImageBuf, (u32, u32)) -> ImageBuf,
) -> &'a ImageBuf {
    if dimensions == (source.width, source.height) {
        *scaled = None;
        return source;
    }
    if scaled.as_ref().is_none_or(|(size, _)| *size != dimensions) {
        *scaled = None; // release the previous size before allocating
        *scaled = Some((dimensions, resample(source, dimensions)));
    }
    &scaled.as_ref().unwrap().1
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
    fn bw_expansion_catalog_keeps_negative_reversal_and_paper_distinct() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let result = catalog(&data).unwrap();
        let profiles = result["profiles"].as_array().unwrap();
        let stock = |id: &str| profiles.iter().find(|p| p["id"] == id).unwrap();
        let film = stock("kodak_trix400_d76_8min");
        let paper = stock("ilford_mgrc_deluxe_grade2");
        assert_eq!(film["paper"], false);
        assert_eq!(film["positive"], false);
        assert_eq!(film["target_print"], paper["id"]);
        assert_eq!(paper["paper"], true);
        assert_eq!(paper["bw"], true);
        assert_eq!(stock("kodak_trix")["positive"], true);
    }
    #[test]
    fn linear_display_is_encoded_without_mutating_output() {
        let image = ImageBuf::from_data(1, 1, vec![0.0, 0.18, 1.0]);
        assert_eq!(
            display_rgba(&image, DISPLAY_MAX_DIM, false).2,
            vec![0, 118, 255, 255]
        );
        assert_eq!(
            display_rgba(&image, DISPLAY_MAX_DIM, true).2,
            vec![0, 46, 255, 255]
        );
        assert_eq!(image.data[1], 0.18);
    }
    #[test]
    fn original_keeps_the_render_available_for_save() {
        let dir = std::env::temp_dir().join(format!("negs-original-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let preview = dir.join("original.rgba");
        let source = ImageBuf::from_data(2, 1, vec![0.0, 0.5, 1.0, 1.0, 0.5, 0.0]);
        spektrafilm_io::save_image(&source, &input, None).unwrap();
        let mut session = Session {
            output: Some(Rendered { id: 7, key: Value::Null, response: Value::Null,
                source: input.clone(), film: "kodak_gold_200".into(), encoded: true,
                post: PostParams::default(), image: source.clone() }),
            ..Default::default()
        };
        let params = Box::new(RuntimeParams::default());
        let response = session.handle(Request { id: 8, command: Command::Original {
            input: input.clone(), params, limit_preview: false, preview_path: preview.clone(),
        } }, Path::new(".")).unwrap();
        assert_eq!(response["width"], 2);
        assert_eq!(response["height"], 1);
        assert_eq!(std::fs::read(preview).unwrap().len(), 8);
        let retained = session.output.as_ref().unwrap();
        assert_eq!(retained.id, 7);
        assert_eq!(retained.image.data, source.data);
        let saved = dir.join("saved.png");
        session.handle(Request { id: 9, command: Command::Save {
            render_id: 7, output: saved.clone(), border: None,
            border_options: BorderOptions::default(), instagram: false,
            crop: None, rotate_cw: 0, fit: None,
        } }, Path::new(".")).unwrap();
        assert_eq!(std::fs::read(input).unwrap(), std::fs::read(saved).unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }
    fn render(input: &Path, preview: &Path, target: Option<u32>, crop: Option<Crop>, interactive_scale: Option<f32>) -> Command {
        let mut params = RuntimeParams::default();
        params.io.photo_long_side_px = target;
        Command::Render {
            input: input.into(), source_generation: 1, film: "kodak_portra_400".into(),
            paper: "kodak_portra_endura".into(), params: Box::new(params), limit_preview: false,
            preview_path: preview.into(), interactive_scale, crop,
        }
    }

    #[test]
    fn image_size_renders_report_sizes_reuse_pans_and_keep_drags_within_the_source() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-image-size-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, preview) = (dir.join("input.png"), dir.join("preview.rgba"));
        let pixels = (0..40 * 30 * 3).map(|i| ((i * 37) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(40, 30, pixels), &input, None).unwrap();
        let mut session = Session::default();
        let mut send = |id, command| session.handle(Request { id, command }, &data);

        // A centred half-extent crop at 60 px develops the whole frame at 120 × 90.
        let centre = Crop { x: 0.25, y: 0.25, width: 0.5, height: 0.5, angle: 0.0 };
        let full = send(1, render(&input, &preview, Some(60), Some(centre), None)).unwrap();
        assert_eq!((full["working_width"].as_u64(), full["working_height"].as_u64()), (Some(120), Some(90)));
        assert_eq!((full["photo_crop_width"].as_u64(), full["photo_crop_height"].as_u64()), (Some(60), Some(45)));
        assert_eq!((full["output_width"].as_u64(), full["source_width"].as_u64()), (Some(120), Some(40)));
        assert_eq!(full["export_resolution"], true);
        drop(send);
        let image = session.output.as_ref().unwrap().image.data.as_ptr();

        // Panning the crop reuses the simulation and reports its own pixels.
        let mut send = |id, command| session.handle(Request { id, command }, &data);
        let panned = Crop { x: 0.2, y: 0.3, ..centre };
        let reused = send(2, render(&input, &preview, Some(60), Some(panned), None)).unwrap();
        assert_eq!(reused["photo_crop_width"].as_u64(), Some(60));
        assert_eq!(reused["output_width"].as_u64(), Some(120));
        drop(send);
        let retained = session.output.as_ref().unwrap();
        assert_eq!((retained.id, retained.image.data.as_ptr()), (2, image));

        // Zoomed past fit on a 3x enlargement, a drag frame stays inside the
        // source and is never saved as an export.
        let mut send = |id, command| session.handle(Request { id, command }, &data);
        let drag = send(3, render(&input, &preview, Some(120), None, Some(0.6))).unwrap();
        assert_eq!((drag["output_width"].as_u64(), drag["output_height"].as_u64()), (Some(40), Some(30)));
        assert_eq!(drag["export_resolution"], false);
        assert_eq!(drag["working_width"].as_u64(), Some(120));

        // Too large a request fails before allocation and keeps the last render.
        let speck = Crop { x: 0.5, y: 0.5, width: 0.002, height: 0.002, angle: 0.0 };
        let error = send(4, render(&input, &preview, Some(65_535), Some(speck), None)).unwrap_err().to_string();
        assert!(error.contains("MP limit"), "{error}");
        let mixed = {
            let Command::Render { mut params, .. } = render(&input, &preview, Some(60), None, None) else { unreachable!() };
            params.io.upscale_factor = 0.5;
            Command::Render { input: input.clone(), source_generation: 1, film: "kodak_portra_400".into(),
                paper: "kodak_portra_endura".into(), params, limit_preview: false, preview_path: preview.clone(),
                interactive_scale: None, crop: None }
        };
        assert!(send(5, mixed).unwrap_err().to_string().contains("factor must be 1"));
        // Original never enlarges, whatever the target.
        let original = send(6, Command::Original { input: input.clone(), params: {
            let mut params = RuntimeParams::default(); params.io.photo_long_side_px = Some(400); Box::new(params)
        }, limit_preview: false, preview_path: preview.clone() }).unwrap();
        assert_eq!((original["width"].as_u64(), original["height"].as_u64()), (Some(40), Some(30)));
        drop(send);
        assert_eq!(session.output.as_ref().unwrap().id, 3);
        assert_ne!(session.output.as_ref().unwrap().image.data.as_ptr(), image);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn working_source_is_resampled_once_per_size() {
        let source = ImageBuf::from_data(4, 2, vec![0.5; 24]);
        let mut scaled = None;
        let first = working_source(&source, &mut scaled, (2, 1), rescale_preview_input_fast)
            .data
            .as_ptr();
        assert_eq!(
            working_source(&source, &mut scaled, (2, 1), rescale_preview_input_fast)
                .data
                .as_ptr(),
            first
        );
        assert_eq!(
            working_source(&source, &mut scaled, (1, 1), rescale_preview_input_fast).width,
            1
        );
        let full = working_source(&source, &mut scaled, (4, 2), rescale_preview_input_fast);
        assert_eq!(full.data.as_ptr(), source.data.as_ptr());
        assert!(scaled.is_none());
    }
    #[test]
    fn save_reads_the_apps_border_options() {
        use spektrafilm_io::border::Rgb;
        // As `saveCommand` in the app's EditorModel writes it.
        let request: Request = serde_json::from_str(
            r##"{"id": 1, "command": "save", "render_id": 1, "output": "out.png", "rotate_cw": 0,
                "border": "white", "instagram": true,
                "border_options": {"width": 0.05, "line": 0.004, "color": "#1f2a44", "line_color": "#000000"}}"##,
        )
        .unwrap();
        let Command::Save {
            border,
            border_options,
            instagram,
            ..
        } = request.command
        else {
            panic!("not a save")
        };
        assert_eq!(border, Some(BorderStyle::Solid));
        assert!(instagram);
        assert_eq!(
            border_options,
            BorderOptions {
                width: Some(0.05),
                color: Rgb([0x1f, 0x2a, 0x44]),
                ..Default::default()
            }
        );
    }

    #[test]
    fn stale_preview_cannot_be_saved() {
        let mut session = Session {
            output: Some(Rendered {
                id: 2,
                key: Value::Null,
                response: Value::Null,
                source: "source.png".into(),
                film: "kodak_gold_200".into(),
                encoded: true,
                post: PostParams::default(),
                image: ImageBuf::from_data(1, 1, vec![0.0; 3]),
            }),
            ..Default::default()
        };
        let request = Request {
            id: 3,
            command: Command::Save {
                render_id: 1,
                output: "unused.png".into(),
                border: None,
                border_options: BorderOptions::default(),
                instagram: false,
                crop: None,
                rotate_cw: 0,
                fit: None,
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
