//! Private, serial JSON-lines protocol for the native macOS client.
//! stdout is protocol-only; diagnostics go to stderr. EOF releases all caches.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use spektrafilm_core::{
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
            Command::Original { input, params, limit_preview, preview_path } => {
                ensure!(input != preview_path, "Choose a different preview path to preserve the source");
                ensure!(params.io.upscale_factor.is_finite()
                    && (0.1..=1.0).contains(&params.io.upscale_factor), "Invalid working resolution");
                params.io.raw_white_balance.validate()?;
                let source = spektrafilm_io::load_image_with_wb(
                    &input, spektrafilm_io::Formats::Any, params.io.raw_white_balance,
                )?;
                let dimensions = preview_dimensions(source.width, source.height,
                    params.io.upscale_factor, limit_preview);
                let working = rescale_preview_input_fast(&source, dimensions);
                let encoded = !spektrafilm_io::input_is_linear_srgb(&input);
                let (width, height) = write_display(&working, encoded, &PostParams::default(), &preview_path)?;
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
                let render_key = json!({
                    "input": input, "source_generation": source_generation, "film": film,
                    "paper": paper, "params": params, "limit_preview": limit_preview,
                    "interactive_scale": interactive_scale,
                });
                let reused = self.output.take().filter(|last| last.key == render_key);
                if let Some(last) = reused {
                    write_display(&last.image, last.encoded, &post, &preview_path)?;
                    let mut response = last.response.clone();
                    response["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                    self.output = Some(Rendered {
                        id: request.id,
                        post,
                        ..last
                    });
                    return Ok(response);
                }
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
                    self.interactive = None;
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
                let upscale_factor = params.io.upscale_factor;
                // The photograph's size; an interactive render is a smaller
                // frame of the same photograph.
                let photo_dimensions =
                    preview_dimensions(source_width, source_height, upscale_factor, limit_preview);
                let dimensions = match interactive_scale.filter(|&s| s < 1.0) {
                    Some(s) => (
                        ((photo_dimensions.0 as f32 * s).round() as u32).max(1),
                        ((photo_dimensions.1 as f32 * s).round() as u32).max(1),
                    ),
                    None => photo_dimensions,
                };
                params.io.upscale_factor = 1.0;
                let pipeline = self
                    .pipeline
                    .as_ref()
                    .unwrap()
                    .1
                    .clone()
                    .with_params((*params).clone());
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
                // At this size the output is byte-identical to a Fast (GPU)
                // export, so the app can save it instead of rendering again.
                let export_resolution = dimensions
                    == preview_dimensions(source_width, source_height, upscale_factor, false);
                let response = json!({
                    "width": width, "height": height, "output_width": output.width, "output_height": output.height,
                    "photo_width": photo_dimensions.0, "photo_height": photo_dimensions.1,
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
