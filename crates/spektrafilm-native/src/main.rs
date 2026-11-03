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
    presets,
    preview::*,
    profile,
};
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_io::{
    border::{BorderOptions, BorderStyle, add_border, extend_for_instagram},
    crop::Crop,
    encoding::Encoding,
    resize::{self, Fit},
};
use spektrafilm_core::stages::scanning::convert_output;
use spektrafilm_math::{colorspace::RgbColourspace, image::ImageBuf};
use std::{
    io::{BufRead, Write},
    path::{Path, PathBuf},
    time::Instant,
};

/// EditorModel.readPreview rejects larger preview buffers.
const DISPLAY_MAX_DIM: usize = 8192;

#[derive(Deserialize)]
struct Request {
    id: u64,
    #[serde(flatten)]
    command: Command,
}

#[derive(Deserialize)]
#[serde(tag = "command", rename_all = "snake_case")]
enum Command {
    /// The enlarger M/Y filter shifts that print a mid-grey neutral with
    /// these settings (upstream's print balance); image-independent.
    BalancePrint {
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
    },
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
    /// A preset file's text (Python spektrafilm's GUI state) as the stocks
    /// and params to apply over `params`, which keeps what a preset doesn't
    /// carry (see `spektrafilm_core::presets::KEPT`). Without `params`, over
    /// the defaults: enough to check a file.
    PresetImport {
        text: String,
        #[serde(default)]
        params: Option<Box<RuntimeParams>>,
    },
    /// The settings as a preset file's text.
    PresetExport {
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
    },
    Original {
        input: PathBuf,
        params: Box<RuntimeParams>,
        limit_preview: bool,
        preview_path: PathBuf,
    },
    /// Fit the graded exposure to an embedded camera-preview reference without
    /// replacing the current render or changing the decoded source.
    PlaceExposure {
        input: PathBuf,
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        reference_path: PathBuf,
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
    /// A small render of the current source with another film/paper or a
    /// preset, for the stock and preset previews. Independent of the main render: it never replaces the
    /// retained output (Save) nor the cached main pipeline.
    Thumbnail {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        /// The thumbnail's long edge in pixels.
        long_side: u32,
        preview_path: PathBuf,
        /// The photograph's crop. The whole frame is developed, as for the
        /// main preview, then cut, so grain and halation keep their scale.
        #[serde(default)]
        crop: Option<Crop>,
        /// A preset file's text, applied over `params` as `preset_import`
        /// applies it; its film and paper replace `film` and `paper`.
        #[serde(default)]
        preset: Option<String>,
    },
    /// Frees the thumbnails' caches (the previews were turned off).
    ReleaseThumbnails,
    /// The tone graph's curve: `count` greys through film → print → scan
    /// (`spektrafilm_core::tone_curve`). Image-independent.
    ToneCurve {
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        #[serde(default)]
        count: Option<usize>,
    },
    Save {
        render_id: u64,
        output: PathBuf,
        #[serde(default)]
        save_options: spektrafilm_io::save::SaveOptions,
        /// Match the displayed preview and CLI: adjust before interpolating a
        /// straightened crop. Older clients retain their existing save order.
        #[serde(default)]
        crop_after_post: bool,
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
    source: Option<Source>,
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
    /// The source reduced for stock and preset thumbnails.
    thumbnail_source: Option<((u32, u32), ImageBuf)>,
    /// Calibrated pipelines for thumbnails, most recent last. A stock's
    /// pipeline outlives edits that don't change its key (exposure, grain…).
    thumbnail_pipelines: Vec<(String, Pipeline)>,
    /// Reduced sources for preset thumbnails whose RAW white balance isn't
    /// the main source's, most recent last.
    thumbnail_wb_sources: Vec<WbSource>,
    /// The tone graph's calibrated pipeline (per-pixel params).
    tone_pipeline: Option<(String, Pipeline)>,
    /// The metering frame's film exposure in stops for the tone graph's
    /// histogram, keyed by the source and the render pipeline.
    film_stops: Option<(String, Vec<[f32; 3]>)>,
}

/// The source decoded with another RAW white balance, reduced to the largest
/// frame a thumbnail develops (`THUMBNAIL_FRAME_MAX`).
struct WbSource {
    /// Effective (`RawWhiteBalance::effective`).
    wb: spektrafilm_io::RawWhiteBalance,
    colour: spektrafilm_io::InputColour,
    image: ImageBuf,
    /// The auto-exposure metering frame of this decode.
    metering: ImageBuf,
}

/// Every film with its paper, and every paper with the current film.
const THUMBNAIL_PIPELINES: usize = 40;
/// The longest frame developed for a cropped thumbnail.
const THUMBNAIL_FRAME_MAX: u32 = 1280;
/// White balances kept for preset thumbnails (few presets differ).
const THUMBNAIL_WB_SOURCES: usize = 4;

/// The frame developed for a thumbnail: large enough that the crop fills
/// `long_side`, within a bound (a tight crop then gets fewer pixels).
fn thumbnail_dimensions((width, height): (u32, u32), long_side: u32, crop: Option<&Crop>) -> (u32, u32) {
    let (fraction_w, fraction_h) = crop.map_or((1.0, 1.0), |c| (c.width, c.height));
    let cropped_long = (width as f64 * fraction_w).max(height as f64 * fraction_h).max(1.0);
    let frame_long = (width.max(height) as f64 * long_side as f64 / cropped_long).min(THUMBNAIL_FRAME_MAX as f64);
    let scale = frame_long / width.max(height) as f64;
    if scale >= 1.0 {
        (width, height)
    } else {
        (((width as f64 * scale).round() as u32).max(1), ((height as f64 * scale).round() as u32).max(1))
    }
}

/// The decoded input, and what it was decoded with: a RAW's white balance
/// and a non-RAW file's interpretation each need a new decode.
struct Source {
    path: PathBuf,
    wb: spektrafilm_io::RawWhiteBalance,
    lens: spektrafilm_io::LensCorrection,
    lens_correction_supported: bool,
    interpretation: spektrafilm_io::InputInterpretation,
    colour: spektrafilm_io::InputColour,
    image: ImageBuf,
}

impl Source {
    fn current(
        &self,
        path: &Path,
        wb: spektrafilm_io::RawWhiteBalance,
        lens: spektrafilm_io::LensCorrection,
        interpretation: &spektrafilm_io::InputInterpretation,
    ) -> bool {
        self.path == path
            && if spektrafilm_io::is_raw(path) {
                self.wb == wb && self.lens == lens
            } else {
                &self.interpretation == interpretation
            }
    }
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
    /// The render's output space and encoding: the preview space. Post, the
    /// border and resizing work in it; a save to another space converts.
    encoding: Encoding,
    post: PostParams,
    image: ImageBuf,
}

fn catalog(data: &Path) -> Result<Value> {
    let mut stocks = Vec::new();
    for entry in std::fs::read_dir(data.join("profiles"))? {
        let path = entry?.path();
        if path.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        stocks.push(path.file_stem().unwrap().to_string_lossy().into_owned());
    }
    let mut catalog = spektrafilm_core::catalog::catalog(data, stocks)?;
    ensure!(
        !catalog["profiles"].as_array().is_some_and(Vec::is_empty),
        "No profiles found in {}",
        data.display()
    );
    catalog["protocol"] = json!(1);
    catalog["capabilities"] = json!({"save_options": true, "crop_after_post": true, "color_space": true, "presets": true, "input_interpretation": true, "preview_color_space": true, "tone_curve": true, "place_exposure": true});
    // What a preset leaves as it is: a preset's thumbnail changes only with these.
    catalog["preset_kept"] = json!(presets::KEPT);
    Ok(catalog)
}

impl Session {
    /// Decodes `input` unless the session already holds it with these settings.
    fn load_source(
        &mut self,
        input: &Path,
        source_generation: u64,
        wb: spektrafilm_io::RawWhiteBalance,
        lens: spektrafilm_io::LensCorrection,
        interpretation: spektrafilm_io::InputInterpretation,
    ) -> Result<()> {
        let reload = self.source_generation != source_generation
            || self.source.as_ref().is_none_or(|source| !source.current(input, wb, lens, &interpretation));
        if reload {
            // Release the last source's caches before decoding the next.
            self.source = None;
            self.scaled = None;
            self.interactive = None;
            self.metering = None;
            self.thumbnail_source = None;
            self.thumbnail_wb_sources = Vec::new();
            self.film_stops = None;
            let loaded = spektrafilm_io::load_image_colour(
                input,
                spektrafilm_io::Formats::Any,
                wb,
                lens,
                &interpretation,
            )?;
            self.source = Some(Source {
                path: input.to_path_buf(),
                wb,
                lens,
                lens_correction_supported: loaded.lens_correction_supported,
                interpretation,
                colour: loaded.colour,
                image: loaded.image,
            });
            self.source_generation = source_generation;
        }
        Ok(())
    }

    fn thumbnail(
        &mut self,
        input: PathBuf,
        source_generation: u64,
        mut film: String,
        mut paper: String,
        mut params: Box<RuntimeParams>,
        long_side: u32,
        preview_path: PathBuf,
        crop: Option<Crop>,
        preset: Option<String>,
        data: &Path,
    ) -> Result<Value> {
        let started = Instant::now();
        ensure!((16..=1024).contains(&long_side), "Invalid thumbnail size");
        let preset_applied = preset.is_some();
        if let Some(text) = preset {
            let preset = presets::import(&text, data).map_err(anyhow::Error::msg)?;
            params = Box::new(presets::apply(&preset.params, &params));
            (film, paper) = (preset.film, preset.paper);
        }
        if let Some(crop) = &crop {
            crop.validate()?;
        }
        ensure!(
            RgbColourspace::find(&params.io.output_color_space).is_some(),
            "Unknown output colour space: {}",
            params.io.output_color_space
        );
        params.io.raw_white_balance.validate()?;
        params.settings.serial_grain = false;
        let post = std::mem::take(&mut params.post);
        let interpretation = params.io.input_interpretation.clone();
        interpretation.validate()?;
        let (wb, lens) = (params.io.raw_white_balance, params.io.lens_correction);
        // A preset whose RAW white balance isn't the photograph's needs a
        // decode of its own. It gets a reduced copy here, so the main render
        // keeps its decoded source. (A preset keeps the lens correction and
        // the interpretation, so only the white balance can differ.)
        let own_decode = preset_applied
            && spektrafilm_io::is_raw(&input)
            && self.source_generation == source_generation
            && self.source.as_ref().is_some_and(|s| s.path == input && s.wb.effective() != wb.effective());
        if !own_decode {
            self.load_source(&input, source_generation, wb, lens, interpretation.clone())?;
        }
        let decoded = started.elapsed();
        let source = &self.source.as_ref().unwrap().image;
        // The thumbnail is the whole frame; Image Size describes the export.
        image_size::clear(&mut params);
        let full = (source.width, source.height);
        let dimensions = thumbnail_dimensions(full, long_side, crop.as_ref());
        let (working, metering) = if own_decode {
            let index = match self.thumbnail_wb_sources.iter().position(|s| s.wb == wb.effective()) {
                Some(index) => index,
                None => {
                    let loaded = spektrafilm_io::load_image_colour(
                        &input, spektrafilm_io::Formats::Any, wb, lens, &interpretation,
                    )?;
                    let metering = spektrafilm_core::stages::filming::metering_preview(&loaded.image);
                    // The largest frame any thumbnail develops, so a new crop
                    // reduces this copy instead of decoding again.
                    let largest = thumbnail_dimensions(full, THUMBNAIL_FRAME_MAX, None);
                    let image = if largest == (loaded.image.width, loaded.image.height) {
                        loaded.image
                    } else {
                        downsample_area(&loaded.image, largest)
                    };
                    if self.thumbnail_wb_sources.len() >= THUMBNAIL_WB_SOURCES {
                        self.thumbnail_wb_sources.remove(0);
                    }
                    self.thumbnail_wb_sources.push(WbSource { wb: wb.effective(), colour: loaded.colour, image, metering });
                    self.thumbnail_wb_sources.len() - 1
                }
            };
            let entry = self.thumbnail_wb_sources.remove(index);
            params.io.apply_input_colour(&entry.colour);
            let working = if dimensions == (entry.image.width, entry.image.height) {
                entry.image.clone()
            } else {
                downsample_area(&entry.image, dimensions)
            };
            let metering = Some(entry.metering.clone());
            self.thumbnail_wb_sources.push(entry);
            (working, metering)
        } else {
            let Source { image: source, colour, .. } = self.source.as_ref().unwrap();
            params.io.apply_input_colour(colour);
            if params.camera.auto_exposure && self.metering.is_none() {
                self.metering = Some(spektrafilm_core::stages::filming::metering_preview(source));
            }
            let working = working_source(source, &mut self.thumbnail_source, dimensions, downsample_area).clone();
            (working, self.metering.clone())
        };
        let resampled = started.elapsed();
        let key = preview_pipeline_cache_key(&film, &paper, &params);
        let cached = self.thumbnail_pipelines.iter().position(|(k, _)| *k == key);
        let calibrated = match cached {
            Some(index) => self.thumbnail_pipelines.remove(index).1,
            None => {
                let film_profile = profile::load_profile_by_name(data, &film)?;
                let print_profile =
                    profile::load_profile_by_name(data, if params.io.scan_film { &film } else { &paper })?;
                Pipeline::new_with_spectral(film_profile, print_profile, (*params).clone(), data)
                    .map_err(anyhow::Error::msg)?
            }
        };
        let mut pipeline = calibrated.clone().with_params((*params).clone());
        if self.thumbnail_pipelines.len() >= THUMBNAIL_PIPELINES {
            self.thumbnail_pipelines.remove(0);
        }
        self.thumbnail_pipelines.push((key, calibrated));
        if let Some(metering) = metering {
            pipeline = pipeline.with_metering_preview(metering);
        }
        let built = started.elapsed();
        let backend = self.backend.get_or_insert_with(spektrafilm_gpu::select_backend);
        let (output, used_backend) = pipeline.process_with_backend(working, backend.as_ref());
        let cropped = crop.map(|crop| crop.apply(&output));
        let output = cropped.as_ref().unwrap_or(&output);
        let processed = started.elapsed();
        let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
        let (width, height) = write_display(output, encoding, &post, &preview_path)?;
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        Ok(json!({
            "width": width, "height": height, "backend": used_backend,
            "display_color_space": DisplaySpace::of(encoding).name(),
            "elapsed_ms": ms(started.elapsed()), "cached_pipeline": cached.is_some(),
            "timing_ms": {"decode": ms(decoded), "resample": ms(resampled - decoded),
                "pipeline": ms(built - resampled), "process": ms(processed - built),
                "write": ms(started.elapsed() - processed)},
        }))
    }

    fn handle(&mut self, request: Request, data: &Path) -> Result<Value> {
        match request.command {
            Command::Catalog => catalog(data),
            Command::BalancePrint { film, paper, params } => {
                let started = Instant::now();
                let (m, y) = spektrafilm_core::print_balance::solve_neutral_filter_shifts(
                    profile::load_profile_by_name(data, &film)?,
                    profile::load_profile_by_name(data, &paper)?,
                    &params,
                    data,
                    spektrafilm_core::print_balance::MIDGRAY,
                )
                .map_err(anyhow::Error::msg)?;
                Ok(json!({"m_filter_shift": m, "y_filter_shift": y,
                          "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}))
            }
            Command::ExportLut { film, paper, params, size, output } => {
                let started = Instant::now();
                let cube = spektrafilm_core::lut_export::cube(data, &film, &paper, *params, size)
                    .map_err(anyhow::Error::msg)?;
                std::fs::write(&output, cube)?;
                Ok(json!({"size": size, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}))
            }
            Command::PresetImport { text, params } => {
                let preset = presets::import(&text, data).map_err(anyhow::Error::msg)?;
                let params = presets::apply(&preset.params, &params.unwrap_or_default());
                Ok(json!({"film": preset.film, "paper": preset.paper, "params": params}))
            }
            Command::PresetExport { film, paper, params } => {
                let text = presets::export(&film, &paper, &params, data).map_err(anyhow::Error::msg)?;
                Ok(json!({"text": text}))
            }
            Command::Original { input, params, limit_preview, preview_path } => {
                ensure!(input != preview_path, "Choose a different preview path to preserve the source");
                ensure!(params.io.upscale_factor.is_finite()
                    && (0.1..=1.0).contains(&params.io.upscale_factor), "Invalid working resolution");
                params.io.raw_white_balance.validate()?;
                params.io.input_interpretation.validate()?;
                let loaded = spektrafilm_io::load_image_colour(
                    &input, spektrafilm_io::Formats::Any, params.io.raw_white_balance,
                    params.io.lens_correction, &params.io.input_interpretation,
                )?;
                let source = loaded.image;
                // Never enlarged: an Image Size target is ignored, and the
                // canvas fits the original to the developed photograph.
                let factor = if params.io.photo_long_side_px.is_some() { 1.0 } else { params.io.upscale_factor };
                let dimensions = preview_dimensions(source.width, source.height, factor, limit_preview);
                let resized = (dimensions != (source.width, source.height))
                    .then(|| rescale_preview_input_fast(&source, dimensions));
                let working = resized.as_ref().unwrap_or(&source);
                let (encoding, converted) = original_encoding(&params, &loaded.colour, working);
                let working = converted.as_ref().unwrap_or(working);
                let (width, height) = write_display(working, encoding, &PostParams::default(), &preview_path)?;
                Ok(json!({"width": width, "height": height, "input_colour": loaded.colour,
                          "display_color_space": DisplaySpace::of(encoding).name()}))
            }
            Command::Thumbnail { input, source_generation, film, paper, params, long_side, preview_path, crop, preset } => {
                self.thumbnail(input, source_generation, film, paper, params, long_side, preview_path, crop, preset, data)
            }
            Command::ToneCurve { film, paper, params, count } => {
                let started = Instant::now();
                let params = spektrafilm_core::tone_curve::tone_params(&params);
                let key = preview_pipeline_cache_key(&film, &paper, &params);
                let cached = self.tone_pipeline.as_ref().is_some_and(|(old, _)| old == &key);
                if !cached {
                    let film_profile = profile::load_profile_by_name(data, &film)?;
                    let print_profile =
                        profile::load_profile_by_name(data, if params.io.scan_film { &film } else { &paper })?;
                    let pipeline = Pipeline::new_with_spectral(film_profile, print_profile, params.clone(), data)
                        .map_err(anyhow::Error::msg)?;
                    self.tone_pipeline = Some((key, pipeline));
                }
                let pipeline = self.tone_pipeline.as_ref().unwrap().1.clone().with_params(params);
                let built = started.elapsed();
                let curve = spektrafilm_core::tone_curve::tone_curve(&pipeline, count.unwrap_or(256));
                let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
                Ok(json!({
                    "stops": spektrafilm_core::tone_curve::STOPS,
                    "film": curve.film, "paper": curve.paper, "rgb": curve.rgb,
                    "cached_pipeline": cached,
                    "timing_ms": {"pipeline": ms(built), "curve": ms(started.elapsed() - built)},
                    "elapsed_ms": ms(started.elapsed()),
                }))
            }
            Command::ReleaseThumbnails => {
                self.thumbnail_source = None;
                self.thumbnail_pipelines = Vec::new();
                self.thumbnail_wb_sources = Vec::new();
                Ok(json!({}))
            }
            Command::PlaceExposure { input, source_generation, film, paper, mut params, reference_path } => {
                let started = Instant::now();
                params.prep.validate().map_err(anyhow::Error::msg)?;
                params.io.raw_white_balance.validate()?;
                params.io.input_interpretation.validate()?;
                ensure!(params.io.upscale_factor.is_finite()
                    && (0.1..=1.0).contains(&params.io.upscale_factor), "Invalid working resolution");
                ensure!(RgbColourspace::find(&params.io.output_color_space).is_some(),
                    "Unknown output colour space: {}", params.io.output_color_space);
                let source = self.source.as_ref().context("Open a photograph and wait for its preview before placing exposure")?;
                ensure!(self.source_generation == source_generation
                    && source.current(&input, params.io.raw_white_balance, params.io.lens_correction,
                                      &params.io.input_interpretation),
                    "The photograph or RAW settings changed; wait for its current preview and try again");
                let reference = placement_reference(&reference_path, &source.image)?;
                params.io.apply_input_colour(&source.colour);
                ensure!(RgbColourspace::find(&params.io.input_color_space).is_some(),
                    "Unknown input colour space: {}", params.io.input_color_space);
                image_size::clear(&mut params);
                let key = preview_pipeline_cache_key(&film, &paper, &params);
                // Borrow the calibrated look when possible, but keep all
                // placement work local. Save must still see the last preview.
                let pipeline = if let Some((cached_key, pipeline)) = &self.pipeline {
                    if cached_key == &key { Some(pipeline.clone()) } else { None }
                } else { None };
                let mut pipeline = match pipeline {
                    Some(pipeline) => pipeline.with_params((*params).clone()),
                    None => Pipeline::new_with_spectral(
                        profile::load_profile_by_name(data, &film)?,
                        profile::load_profile_by_name(data, if params.io.scan_film { &film } else { &paper })?,
                        (*params).clone(), data,
                    ).map_err(anyhow::Error::msg)?,
                };
                if let Some(metering) = &self.metering {
                    pipeline = pipeline.with_metering_preview(metering.clone());
                }
                let backend = self.backend.get_or_insert_with(spektrafilm_gpu::select_backend);
                let placement = spektrafilm_core::exposure_placement::place_exposure(
                    &pipeline, &source.image, &reference, backend.as_ref(),
                ).map_err(anyhow::Error::msg)?;
                let mut result = serde_json::to_value(placement)?;
                result["elapsed_ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                Ok(result)
            }
            Command::Save {
                render_id,
                output,
                save_options,
                crop_after_post,
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
                    encoding,
                    post,
                    image,
                    ..
                } = self.output.as_mut().context("No preview to save")?;
                let (encoding, encoded) = (*encoding, encoding.encoded);
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
                save_options.check(&output)?;
                let metadata = spektrafilm_io::read_metadata(source);
                // The render is the unclipped scan; Post, crop, border and resize
                // work on it as the file will hold it. Clipping in place keeps
                // memory flat, and the display clips the same way.
                spektrafilm_core::pipeline::clip_to_display(image, encoded);
                let image = &*image;
                let adjusted_before = (crop_after_post && !post.is_identity())
                    .then(|| PostProcess::new(post, encoded, encoding.space.name).applied(image));
                let image = adjusted_before.as_ref().unwrap_or(image);
                // Crop in the unrotated orientation, then rotate, then add the border.
                let cropped = crop.map(|crop| crop.apply(image));
                let image = cropped.as_ref().unwrap_or(image);
                let adjusted = (!crop_after_post && !post.is_identity())
                    .then(|| PostProcess::new(post, encoded, encoding.space.name).applied(image));
                let image = adjusted.as_ref().unwrap_or(image);
                let rotated =
                    (rotate_cw.rem_euclid(4) != 0).then(|| image.clone().rotated_cw(rotate_cw));
                let image = rotated.as_ref().unwrap_or(image);
                let bordered = match border {
                    Some(style) => {
                        let bordered =
                            add_border(image, style, &border_options, film, encoding, data)?;
                        Some(if instagram {
                            extend_for_instagram(&bordered, style, &border_options, encoding)
                        } else {
                            bordered
                        })
                    }
                    None => None,
                };
                let image = bordered.as_ref().unwrap_or(image);
                let resized = fit.and_then(|fit| resize::fit(image, fit, encoding));
                let image = resized.as_ref().unwrap_or(image);
                // Saved in the preview's space, the file holds the render's
                // values. Another space gets the preview's colours, clipped to
                // the smaller of the two gamuts (relative colorimetric).
                let space = RgbColourspace::find(&save_options.color_space)
                    .with_context(|| format!("Unknown colour space: {}", save_options.color_space))?;
                let converted = (space.name != encoding.space.name).then(|| {
                    let mut converted = image.clone();
                    convert_output(&mut converted, encoding.space.name, space.name, encoded);
                    converted
                });
                let image = converted.as_ref().unwrap_or(image);
                let save_options = spektrafilm_io::save::SaveOptions { linear: !encoded, ..save_options };
                spektrafilm_io::save::save_image_with_options(image, &output, metadata.as_ref(), &save_options)?;
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
                // The preview space: the app sends the export's colour space,
                // so the preview judges the export's gamut.
                ensure!(
                    RgbColourspace::find(&params.io.output_color_space).is_some(),
                    "Unknown output colour space: {}",
                    params.io.output_color_space
                );
                params.io.raw_white_balance.validate()?;
                params.settings.serial_grain = false;
                params.prep.validate().map_err(anyhow::Error::msg)?;
                // Applied to the display and on save, so the pipeline runs without them.
                let post = std::mem::take(&mut params.post);
                let wb = params.io.raw_white_balance;
                let lens = params.io.lens_correction;
                let interpretation = params.io.input_interpretation.clone();
                interpretation.validate()?;
                self.load_source(&input, source_generation, wb, lens, interpretation)?;
                let Source { image: source, colour: input_colour, lens_correction_supported, .. } =
                    self.source.as_ref().unwrap();
                // RAW and decoded files are the engine's linear sRGB; manual
                // pass-through keeps the params' input fields, as 0.8 did.
                params.io.apply_input_colour(input_colour);
                if params.camera.auto_exposure && self.metering.is_none() {
                    self.metering = Some(spektrafilm_core::stages::filming::metering_preview(source));
                }
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
                    write_display(&last.image, last.encoding, &post, &preview_path)?;
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
                // The tone graph's histogram: the photograph's film exposure,
                // from the metering frame (the whole frame, before spatial effects).
                // Prep's exposure is a uniform gain, so it shifts the stops like
                // the camera's; its other controls reshape them, so then the
                // frame goes through Prep and is counted as exposed.
                let histogram = {
                    let metering = self
                        .metering
                        .get_or_insert_with(|| spektrafilm_core::stages::filming::metering_preview(source));
                    let ae = if params.camera.auto_exposure { pipeline.metering_film_raw(metering).1 } else { 0.0 };
                    let reshaped = !spektrafilm_core::prep::PrepParams { exposure_ev: 0.0, ..params.prep }.is_identity();
                    let mut stops_key = format!("{}|{source_generation}|{}", input.display(), self.pipeline.as_ref().unwrap().0);
                    if reshaped {
                        stops_key += &format!("|{:?}|{ae}|{}", params.prep, params.camera.exposure_compensation_ev);
                    }
                    if self.film_stops.as_ref().is_none_or(|(old, _)| old != &stops_key) {
                        let raw = if reshaped {
                            pipeline.metering_film_raw_prepared(metering, ae).map_err(anyhow::Error::msg)?
                        } else {
                            pipeline.metering_film_raw(metering).0
                        };
                        self.film_stops = Some((stops_key, spektrafilm_core::tone_curve::raw_stops(&raw)));
                    }
                    let ev = if reshaped { 0.0 } else { ae + params.camera.exposure_compensation_ev as f64 + params.prep.exposure_ev as f64 };
                    let counts = spektrafilm_core::tone_curve::film_histogram(&self.film_stops.as_ref().unwrap().1, ev, 128);
                    json!({"stops": spektrafilm_core::tone_curve::STOPS, "ev": ev, "counts": counts})
                };
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
                let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
                let (width, height) = write_display(&output, encoding, &post, &preview_path)?;
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
                    "input_colour": input_colour,
                    "lens_correction_supported": lens_correction_supported,
                    "output_color_space": encoding.space.name,
                    "display_color_space": DisplaySpace::of(encoding).name(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                    "film_histogram": histogram,
                });
                self.output = Some(Rendered {
                    id: request.id,
                    key: render_key,
                    response: response.clone(),
                    source: input,
                    film,
                    encoding,
                    post,
                    image: output,
                });
                Ok(response)
            }
        }
    }
}

/// References are small, upright, complete camera frames. Decode their ICC/
/// transfer metadata through the same colour-managed loader as other images;
/// the solver receives linear sRGB, never encoded PNG samples mistaken for light.
fn placement_reference(path: &Path, source: &ImageBuf) -> Result<ImageBuf> {
    let metadata = std::fs::metadata(path).context("Reading camera-preview reference")?;
    ensure!(metadata.is_file() && metadata.len() <= 32 * 1024 * 1024,
        "The camera-preview reference must be a PNG smaller than 32 MB");
    let reader = image::ImageReader::open(path)?.with_guessed_format()?;
    ensure!(reader.format() == Some(image::ImageFormat::Png), "The camera-preview reference must be a PNG");
    let (width, height) = reader.into_dimensions()?;
    ensure!((2..=2048).contains(&width) && (2..=2048).contains(&height),
        "The camera-preview reference must be between 2 and 2048 pixels on each side");
    let reference = spektrafilm_io::load_image(path, spektrafilm_io::Formats::Any)?;
    let aspect = reference.width as f64 / reference.height as f64;
    let source_aspect = source.width as f64 / source.height as f64;
    ensure!((aspect / source_aspect - 1.0).abs() <= 0.02,
        "The camera-preview reference must show the whole photograph with matching orientation");
    ensure!(reference.data.iter().all(|v| v.is_finite()), "The camera-preview reference contains invalid pixels");
    Ok(reference)
}

/// A request line. Params from before `io.input_interpretation` keep the
/// meaning of their input fields; see `infer_input_interpretation`.
fn parse_request(line: &str) -> Result<Request> {
    let mut value: Value = serde_json::from_str(line)?;
    if let Some(params) = value.get_mut("params") {
        spektrafilm_core::params::infer_input_interpretation(params);
    }
    Ok(serde_json::from_value(value)?)
}

/// Writes the preview for the app as raw RGBA, avoiding PNG compression on
/// every slider edit. Save keeps the float output, independent of this proxy.
/// The bytes are in `DisplaySpace::of(encoding)`.
fn write_display(
    image: &ImageBuf,
    encoding: Encoding,
    post: &PostParams,
    path: &Path,
) -> Result<(usize, usize)> {
    let post = (!post.is_identity()).then(|| PostProcess::new(post, encoding.encoded, encoding.space.name));
    let (width, height, bytes) =
        display_rgba_with_post(image, DISPLAY_MAX_DIM, encoding, post.as_ref());
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
            session.handle(parse_request(&line)?, &data)
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
    fn presets_import_over_the_current_params_and_export() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let mut session = Session::default();
        let mut current = RuntimeParams::default();
        current.io.photo_long_side_px = Some(2400);
        current.camera.exposure_compensation_ev = 1.0;
        let text = r#"{"simulation": {"film_stock": "kodak_portra_400", "route": "input > film > scan"},
                       "couplers": {"langmuir_donor_k_rgb": [Infinity, 1.0, 1.0]}}"#;
        let imported = session.handle(Request { id: 1, command: Command::PresetImport {
            text: text.into(), params: Some(Box::new(current)),
        } }, &data).unwrap();
        assert_eq!(imported["film"], "kodak_portra_400");
        assert_eq!(imported["params"]["io"]["photo_long_side_px"], 2400);
        assert_eq!(imported["params"]["io"]["scan_film"], true);
        assert_eq!(imported["params"]["camera"]["exposure_compensation_ev"], 0.0);
        assert_eq!(imported["params"]["film_render"]["dir_couplers"]["langmuir_donor_k_rgb"][0], Value::Null);
        let params: RuntimeParams = serde_json::from_value(imported["params"].clone()).unwrap();
        let exported = session.handle(Request { id: 2, command: Command::PresetExport {
            film: "kodak_portra_400".into(), paper: "kodak_supra_endura".into(), params: Box::new(params),
        } }, &data).unwrap();
        let text = exported["text"].as_str().unwrap();
        assert!(text.contains("Infinity"), "{text}");
        let refused = session.handle(Request { id: 3, command: Command::PresetImport {
            text: r#"{"simulation": {"film_stock": "kodak_unknown"}}"#.into(), params: None,
        } }, &data).unwrap_err();
        assert!(refused.to_string().contains("kodak_unknown"), "{refused}");
    }

    /// A preset's thumbnail is the thumbnail of what `preset_import` makes of
    /// it over the current params, and it leaves the main render alone.
    #[test]
    fn preset_thumbnails_apply_the_preset_like_preset_import() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-preset-thumbnail-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let pixels = (0..48 * 32 * 3).map(|i| ((i * 37) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(48, 32, pixels), &input, None).unwrap();
        let text = r#"{"simulation": {"film_stock": "kodak_portra_400", "print_paper": "kodak_portra_endura"},
                       "camera": {"exposure_compensation_ev": 1.5}}"#;
        let mut current = RuntimeParams::default();
        current.io.output_color_space = "Display P3".into();
        current.camera.exposure_compensation_ev = -1.0;
        let mut session = Session::default();
        let thumbnail = |session: &mut Session, film: &str, paper: &str, params: RuntimeParams, preset: Option<&str>, name: &str| {
            let preview = dir.join(name);
            let response = session.handle(Request { id: 1, command: Command::Thumbnail {
                input: input.clone(), source_generation: 1, film: film.into(), paper: paper.into(),
                params: Box::new(params), long_side: 24, preview_path: preview.clone(), crop: None,
                preset: preset.map(str::to_string),
            } }, &data).unwrap();
            (response, std::fs::read(preview).unwrap())
        };
        let (response, with_preset) =
            thumbnail(&mut session, "kodak_gold_200", "kodak_supra_endura", current.clone(), Some(text), "preset.rgba");
        assert_eq!((response["width"].as_u64(), response["height"].as_u64()), (Some(24), Some(16)));
        // The photograph's colour space stays; the preset's look comes in.
        assert_eq!(response["display_color_space"], DisplaySpace::Rec2020SrgbTransfer.name());
        let imported = session.handle(Request { id: 2, command: Command::PresetImport {
            text: text.into(), params: Some(Box::new(current.clone())),
        } }, &data).unwrap();
        assert_eq!(imported["params"]["camera"]["exposure_compensation_ev"], 1.5);
        let params: RuntimeParams = serde_json::from_value(imported["params"].clone()).unwrap();
        let film = imported["film"].as_str().unwrap();
        let paper = imported["paper"].as_str().unwrap();
        let (_, applied) = thumbnail(&mut session, film, paper, params, None, "applied.rgba");
        assert_eq!(with_preset, applied);
        let (_, plain) = thumbnail(&mut session, "kodak_gold_200", "kodak_supra_endura", current, None, "plain.rgba");
        assert_ne!(with_preset, plain);
        assert!(session.output.is_none() && session.pipeline.is_none());
        std::fs::remove_dir_all(dir).unwrap();
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
                source: input.clone(), film: "kodak_gold_200".into(), encoding: Encoding::from(true),
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
            render_id: 7, output: saved.clone(), save_options: Default::default(), crop_after_post: false, border: None,
            border_options: BorderOptions::default(), instagram: false,
            crop: None, rotate_cw: 0, fit: None,
        } }, Path::new(".")).unwrap();
        assert_eq!(std::fs::read(input).unwrap(), std::fs::read(saved).unwrap());
        // Another colour space converts the sRGB render and carries its profile.
        let wide = dir.join("wide.png");
        let save_options = spektrafilm_io::save::SaveOptions {
            bit_depth: Some(16), color_space: "Display P3".into(), ..Default::default()
        };
        session.handle(Request { id: 10, command: Command::Save {
            render_id: 7, output: wide.clone(), save_options, crop_after_post: false, border: None,
            border_options: BorderOptions::default(), instagram: false,
            crop: None, rotate_cw: 0, fit: None,
        } }, Path::new(".")).unwrap();
        let mut decoder = image::ImageReader::open(&wide).unwrap().into_decoder().unwrap();
        let icc = image::ImageDecoder::icc_profile(&mut decoder).unwrap().unwrap();
        let p3 = RgbColourspace::find("Display P3").unwrap();
        assert_eq!(icc, spektrafilm_io::icc::profile(p3, false));
        let pixels = image::DynamicImage::from_decoder(decoder).unwrap().to_rgb16().into_raw();
        // sRGB's (0, 0.5, 1) and (1, 0.5, 0) lie inside Display P3's primaries.
        assert!(pixels[0] > 0 && pixels[2] < 65535, "{pixels:?}");
        assert!(pixels[3] < 65535 && pixels[5] > 0, "{pixels:?}");
        std::fs::remove_dir_all(dir).unwrap();
    }
    /// A Display P3 preview renders in P3 and shows through the Rec. 2020
    /// proxy; saving it as P3 writes the render's own values, and saving it
    /// as sRGB converts them. The original view shares the preview space.
    #[test]
    fn wide_preview_space_renders_and_saves_without_reconverting() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-wide-preview-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, preview) = (dir.join("input.png"), dir.join("preview.rgba"));
        let pixels = (0..24 * 16 * 3).map(|i| ((i * 37) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(24, 16, pixels), &input, None).unwrap();
        let mut session = Session::default();
        let Command::Render { mut params, .. } = render(&input, &preview, None, None, None) else { unreachable!() };
        params.io.output_color_space = "Display P3".into();
        params.settings.serial_grain = false;
        let command = |params: Box<RuntimeParams>| Command::Render {
            input: input.clone(), source_generation: 1, film: "kodak_portra_400".into(),
            paper: "kodak_portra_endura".into(), params, limit_preview: false,
            preview_path: preview.clone(), interactive_scale: None, crop: None,
        };
        let response = session.handle(Request { id: 1, command: command(params.clone()) }, &data).unwrap();
        assert_eq!(response["output_color_space"], "Display P3");
        assert_eq!(response["display_color_space"], DisplaySpace::Rec2020SrgbTransfer.name());
        let rendered = session.output.as_ref().unwrap();
        assert_eq!(rendered.encoding, Encoding::new("Display P3", true));
        let mut clipped = rendered.image.clone();
        spektrafilm_core::pipeline::clip_to_display(&mut clipped, true);

        let save = |session: &mut Session, name: &str, space: &str| {
            let output = dir.join(name);
            let save_options = spektrafilm_io::save::SaveOptions {
                bit_depth: Some(16), color_space: space.into(), ..Default::default()
            };
            session.handle(Request { id: 2, command: Command::Save {
                render_id: 1, output: output.clone(), save_options, crop_after_post: false, border: None,
                border_options: BorderOptions::default(), instagram: false, crop: None, rotate_cw: 0, fit: None,
            } }, &data).unwrap();
            image::open(&output).unwrap().to_rgb16().into_raw()
        };
        let p3 = save(&mut session, "p3.png", "Display P3");
        for (saved, want) in p3.iter().zip(&clipped.data) {
            assert!((f64::from(*saved) / 65535.0 - f64::from(*want)).abs() <= 0.5 / 65535.0 + 1e-6);
        }
        let srgb = save(&mut session, "srgb.png", "sRGB");
        let mut converted = clipped.clone();
        convert_output(&mut converted, "Display P3", "sRGB", true);
        for (saved, want) in srgb.iter().zip(&converted.data) {
            assert!((f64::from(*saved) / 65535.0 - f64::from(*want)).abs() <= 0.5 / 65535.0 + 1e-6);
        }
        assert_ne!(p3, srgb);

        let original = session.handle(Request { id: 3, command: Command::Original {
            input: input.clone(), params, limit_preview: false, preview_path: dir.join("original.rgba"),
        } }, &data).unwrap();
        assert_eq!(original["display_color_space"], DisplaySpace::Rec2020SrgbTransfer.name());
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
            save_options,
            crop_after_post,
            ..
        } = request.command
        else {
            panic!("not a save")
        };
        assert_eq!(border, Some(BorderStyle::Solid));
        assert!(instagram);
        assert_eq!(save_options.jpeg_quality, 95);
        assert!(save_options.copy_metadata);
        assert_eq!(save_options.bit_depth, None);
        assert_eq!(save_options.color_space, "sRGB");
        assert!(!crop_after_post, "existing clients retain their original Post/crop order");
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
                encoding: Encoding::from(true),
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
                save_options: Default::default(),
                crop_after_post: false,
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
