//! Private, serial JSON-lines protocol for the native macOS client.
//! stdout is protocol-only; diagnostics go to stderr. EOF releases all caches.
use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use serde_json::{Value, json};
use spektrafilm_core::{
    flat_field,
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

mod pipelines;

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
    /// The colour space a RAW's camera preview says it's in, for the Mac's
    /// Match camera brightness reference (`spektrafilm_io::brightness::
    /// preview_colour_space`): "srgb", "adobe_rgb", "other" or null.
    PreviewColourSpace {
        input: PathBuf,
    },
    /// A colour-managed camera/original reference for clients without a
    /// native colour-managed extractor. Independent of retained render state.
    BrightnessReference {
        input: PathBuf,
        reference_path: PathBuf,
        expected_width: u32,
        expected_height: u32,
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
        /// Read a RAW only as large as this thumbnail needs, where the decoder
        /// can, instead of the whole sensor: far quicker, with the same colours.
        /// The source it leaves is decoded again for anything larger.
        #[serde(default)]
        reduced_decode: bool,
        /// Only decode, as this thumbnail would, and leave what its develop
        /// leaves for a `place_exposure` after: no develop, no preview.
        #[serde(default)]
        decode_only: bool,
        /// Also read `preview_colour_space`'s answer for `input`, while it
        /// decodes, as "preview_colour_space".
        #[serde(default)]
        preview_colour_space: bool,
        /// A negative's tile for another film it could be read as: its
        /// base is found and its print balanced for that film first, as
        /// choosing it does (`negative_base`).
        #[serde(default)]
        read_negative: Option<ReadNegative>,
    },
    /// A scanned colour negative's film base (convert-film route): found
    /// in the clear film, or read at `point` (fractions of the whole,
    /// uncropped photograph), then the print balanced for it
    /// (`negative::neutral_balance`: the film's mid-grey printed neutral,
    /// whatever the enlarger settings were).
    NegativeBase {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        #[serde(default)]
        point: Option<[f64; 2]>,
    },
    /// A negative's filtration on another paper (`negative::paper_change`):
    /// its departure from the neutral on `from_paper` kept on `paper`'s.
    /// Image-independent; the print exposure is metered after
    /// (`negative_meter`).
    NegativePaper {
        film: String,
        from_paper: String,
        paper: String,
        params: Box<RuntimeParams>,
    },
    /// Whether the photograph looks like a scan of a colour negative read as
    /// `film` (`negative::looks_negative`, after finding its base), whatever
    /// its route: where the app places "Your negative". Read from the
    /// decoded source the session holds for the photograph's own white
    /// balance, lens correction and interpretation; no print balance.
    NegativeCheck {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        params: Box<RuntimeParams>,
    },
    /// A minilab's auto print of the negative: the enlarger's magenta,
    /// yellow and print exposure for this frame (`negative::auto_print`),
    /// metered inside `bounds` (the picture: left, top, right, bottom as
    /// fractions of the whole, uncropped photograph; without them the
    /// frame less a border).
    NegativeAuto {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        #[serde(default)]
        bounds: Option<[f64; 4]>,
    },
    /// The print exposure alone for this frame, metered as `negative_auto`
    /// meters it, keeping the enlarger's filtration
    /// (`negative::meter_print_exposure`): a darkroom roll shares its
    /// filtration and each frame gets its own exposure.
    NegativeMeter {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        #[serde(default)]
        bounds: Option<[f64; 4]>,
    },
    /// The magenta and yellow that print `point` (fractions of the whole
    /// photograph) neutral (`negative::neutral_print`).
    NegativeNeutral {
        input: PathBuf,
        #[serde(default)]
        source_generation: u64,
        film: String,
        paper: String,
        params: Box<RuntimeParams>,
        point: [f64; 2],
    },
    /// Frees the thumbnails' caches (the previews were turned off).
    ReleaseThumbnails,
    /// Frees the GPU's kept film sides (ticket 41): the editor closed or
    /// memory is short. Safe at any time; the next render just misses.
    ReleaseFilmCache,
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
        /// Mirrored left to right after the crop, before `rotate_cw`
        /// (`IoParams::flip`). The preview is never mirrored either.
        #[serde(default)]
        flip: bool,
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
    /// Prep's output for the photograph-size and the interactive frame (see
    /// `Pipeline::with_prepared_frame`): each slot only ever sees its frame's
    /// one resample of `source` per size, and is replaced with the source.
    prepared_photo: spektrafilm_core::prep::PreparedFrame,
    prepared_interactive: spektrafilm_core::prep::PreparedFrame,
    /// The source's auto-exposure metering frame. Every render of the source,
    /// at any preview or working size, meters it, as the exporter does, so
    /// the preview's exposure is the export's.
    metering: Option<ImageBuf>,
    /// Calibrated pipelines for every command that renders (ticket 29).
    pipelines: pipelines::Pipelines,
    output: Option<Rendered>,
    /// The source reduced for stock and preset thumbnails.
    thumbnail_source: Option<((u32, u32), ImageBuf)>,
    /// Reduced sources for preset thumbnails whose RAW white balance isn't
    /// the main source's, most recent last.
    thumbnail_wb_sources: Vec<WbSource>,
    /// The metering frame's film exposure in stops for the tone graph's
    /// histogram, keyed by the source and the render pipeline.
    film_stops: Option<(String, Vec<[f32; 3]>)>,
    /// The source reduced for a negative's base and auto print
    /// (`negative::analysis_frame`), until the source changes.
    negative_frame: Option<ImageBuf>,
    /// Counts the sources decoded: with a slot and a size it names a working
    /// frame's pixels for the backend's film-side cache (ticket 41).
    source_loads: u64,
    /// The last light frame read for a negative's flat field, under its
    /// [`light_frame_key`], or why it couldn't be: read once, not per render.
    light_frame: Option<(String, std::result::Result<std::sync::Arc<flat_field::FlatField>, String>)>,
}

/// A read-as tile's base detection: at `point` when the base was picked
/// there (fractions of the whole photograph), else found.
#[derive(Debug, Deserialize)]
struct ReadNegative {
    #[serde(default)]
    point: Option<[f64; 2]>,
}

/// The source decoded with another RAW white balance, or divided by another
/// flat field (a film's tile, printed as a scene, on a negative divided by
/// one), reduced to the largest frame a thumbnail develops
/// (`THUMBNAIL_FRAME_MAX`).
struct WbSource {
    /// Effective (`RawWhiteBalance::effective`).
    wb: spektrafilm_io::RawWhiteBalance,
    /// The light frame it was divided by ([`LightFrame::requested`]).
    flat: Option<LightFrame>,
    colour: spektrafilm_io::InputColour,
    image: ImageBuf,
    /// The auto-exposure metering frame of this decode.
    metering: ImageBuf,
    /// Read reduced, at no more than `THUMBNAIL_FRAME_MAX`.
    reduced: bool,
}

/// The longest frame developed for a cropped thumbnail.
const THUMBNAIL_FRAME_MAX: u32 = 1280;
/// White balances kept for preset thumbnails (few presets differ).
const THUMBNAIL_WB_SOURCES: usize = 4;

/// The long side a reduced read needs for a thumbnail's frame
/// ([`thumbnail_dimensions`]) without knowing the photograph's size: the crop's
/// shorter fraction bounds how much larger than `long_side` the frame can be.
fn reduced_read_side(long_side: u32, crop: Option<&Crop>) -> u32 {
    let fraction = crop.map_or(1.0, |c| c.width.min(c.height)).max(1e-6);
    ((long_side as f64 / fraction).ceil().min(THUMBNAIL_FRAME_MAX as f64) as u32).max(long_side)
}

/// `dimensions` within `image`: a reduced read can round a pixel under the
/// frame its full size calls for.
fn within((width, height): (u32, u32), image: &ImageBuf) -> (u32, u32) {
    (width.min(image.width), height.min(image.height))
}

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
    /// The photograph's whole size; `image`'s unless the read was reduced.
    full: (u32, u32),
    /// The long side a reduced read was asked for; None when it is whole.
    reduced_to: Option<u32>,
    wb: spektrafilm_io::RawWhiteBalance,
    lens: spektrafilm_io::LensCorrection,
    lens_correction_supported: bool,
    interpretation: spektrafilm_io::InputInterpretation,
    colour: spektrafilm_io::InputColour,
    image: ImageBuf,
    /// The light frame a negative asked to be divided by
    /// ([`flat_field::requested`]), and whether `image` was.
    flat: Option<(LightFrame, flat_field::Applied)>,
}

/// A light frame as a flat field names it: its path, with its size and last
/// change, so a light frame replaced at the same path is read again.
#[derive(Debug, Clone, PartialEq)]
struct LightFrame {
    path: PathBuf,
    stamp: Option<(std::time::SystemTime, u64)>,
}

impl LightFrame {
    fn new(path: &Path) -> Self {
        let stamp = std::fs::metadata(path).ok().and_then(|m| Some((m.modified().ok()?, m.len())));
        Self { path: path.to_path_buf(), stamp }
    }

    /// The light frame `params` ask a scan to be divided by, if any.
    fn requested(params: &RuntimeParams) -> Option<Self> {
        flat_field::requested(params).map(Self::new)
    }
}

impl Source {
    fn current(
        &self,
        path: &Path,
        wb: spektrafilm_io::RawWhiteBalance,
        lens: spektrafilm_io::LensCorrection,
        interpretation: &spektrafilm_io::InputInterpretation,
        flat: Option<&LightFrame>,
    ) -> bool {
        self.path == path
            && self.flat.as_ref().map(|(light, _)| light) == flat
            && if spektrafilm_io::is_raw(path) {
                self.wb == wb && self.lens == lens
            } else {
                &self.interpretation == interpretation
            }
    }

    /// What a response says about the flat field: null when none was asked
    /// for, else the light frame, whether this photograph was divided by it,
    /// and why not.
    fn flat_answer(&self) -> Value {
        self.flat.as_ref().map_or(Value::Null, |(light, applied)| {
            json!({"path": light.path, "used": applied.used, "reason": applied.reason})
        })
    }

    /// Large enough for a read limited to `max_long_side` (0 is whole).
    fn covers(&self, max_long_side: u32) -> bool {
        self.reduced_to.is_none_or(|read| max_long_side != 0 && max_long_side <= read)
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
    catalog["capabilities"] = json!({"save_options": true, "crop_after_post": true, "color_space": true, "presets": true, "input_interpretation": true, "preview_color_space": true, "tone_curve": true, "place_exposure": true, "brightness_reference": true});
    // What a preset leaves as it is: a preset's thumbnail changes only with these.
    catalog["preset_kept"] = json!(presets::KEPT);
    Ok(catalog)
}

/// `preview_colour_space`'s answer for `input`: "srgb", "adobe_rgb", "other" or None.
fn declared_preview_colour_space(input: &Path) -> Result<Option<&'static str>> {
    use spektrafilm_io::brightness::{Declared, preview_colour_space};
    let file = std::fs::read(input).with_context(|| format!("Couldn't read {}", input.display()))?;
    Ok(preview_colour_space(&file).map(|declared| match declared {
        Declared::Srgb => "srgb",
        Declared::AdobeRgb => "adobe_rgb",
        Declared::Other => "other",
    }))
}

impl Session {
    /// Decodes `input` unless the session already holds it with these
    /// settings, and at least `max_long_side` large (0 is whole: see
    /// [`spektrafilm_io::load_image_colour_limited`]).
    ///
    /// A negative with a flat field (`params`, [`flat_field::requested`]) is
    /// divided by it here, as it is read, so every render, thumbnail and
    /// one-shot solve of the source sees the same flattened scan and none
    /// of them pays for it again.
    #[allow(clippy::too_many_arguments)]
    fn load_source(
        &mut self,
        input: &Path,
        source_generation: u64,
        wb: spektrafilm_io::RawWhiteBalance,
        lens: spektrafilm_io::LensCorrection,
        interpretation: spektrafilm_io::InputInterpretation,
        max_long_side: u32,
        params: &RuntimeParams,
    ) -> Result<()> {
        let flat = LightFrame::requested(params);
        let reload = self.source_generation != source_generation
            || self.source.as_ref().is_none_or(|source| {
                !source.current(input, wb, lens, &interpretation, flat.as_ref()) || !source.covers(max_long_side)
            });
        if reload {
            // Release the last source's caches before decoding the next.
            self.source = None;
            self.scaled = None;
            self.interactive = None;
            self.prepared_photo = Default::default();
            self.prepared_interactive = Default::default();
            self.metering = None;
            self.thumbnail_source = None;
            self.thumbnail_wb_sources = Vec::new();
            self.film_stops = None;
            self.negative_frame = None;
            self.source_loads += 1;
            if let Some(backend) = &self.backend {
                backend.release_film_cache();
            }
            let loaded = spektrafilm_io::load_image_colour_limited(
                input,
                spektrafilm_io::Formats::Any,
                wb,
                lens,
                &interpretation,
                max_long_side,
            )?;
            let reduced = loaded.full_size != (loaded.image.width, loaded.image.height);
            let mut image = loaded.image;
            let flat = flat.map(|light| {
                let mut params = params.clone();
                params.io.apply_input_colour(&loaded.colour);
                let applied = self.flatten(&mut image, input, &light.path, &params);
                (light, applied)
            });
            self.source = Some(Source {
                path: input.to_path_buf(),
                full: loaded.full_size,
                reduced_to: reduced.then_some(max_long_side),
                wb,
                lens,
                lens_correction_supported: loaded.lens_correction_supported,
                interpretation,
                colour: loaded.colour,
                image,
                flat,
            });
            self.source_generation = source_generation;
        }
        Ok(())
    }

    /// The held source's [`Source::flat_answer`].
    fn flat_answer(&self) -> Value {
        self.source.as_ref().map_or(Value::Null, Source::flat_answer)
    }

    /// Divides `image` (the scan `input`, as decoded; `params` with its input
    /// colour) by the light frame `light`, reading the light frame unless the
    /// session holds it read the same way.
    fn flatten(&mut self, image: &mut ImageBuf, input: &Path, light: &Path, params: &RuntimeParams) -> flat_field::Applied {
        let key = light_frame_key(light, params);
        if self.light_frame.as_ref().is_none_or(|(old, _)| old != &key) {
            self.light_frame = None; // released before the next is read
            let loaded = flat_field::FlatField::load(light, params).map(std::sync::Arc::new);
            self.light_frame = Some((key, loaded));
        }
        match &self.light_frame.as_ref().unwrap().1 {
            Ok(field) => field.flatten(image, input, params),
            Err(reason) => flat_field::Applied::not_used(reason.clone()),
        }
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
        reduced_decode: bool,
        decode_only: bool,
        colour_space: bool,
        data: &Path,
    ) -> Result<Value> {
        let started = Instant::now();
        ensure!((16..=1024).contains(&long_side), "Invalid thumbnail size");
        let preset_applied = preset.is_some();
        if let Some(text) = preset {
            let preset = presets::import(&text, data).map_err(anyhow::Error::msg)?;
            let negative = params.workflow.converts_film();
            params = Box::new(presets::apply(&preset.params, &params));
            if negative {
                // A negative is still read as its film, with its base and
                // balance (`presets::KEPT_NEGATIVE`). A new paper moves the
                // filtration by the papers' difference, as the app does when
                // a preset changes it (`negative_paper`; the app also meters
                // the exposure again; the tile doesn't).
                if preset.paper != paper {
                    let (m, y) = spektrafilm_core::negative::paper_change(
                        &profile::load_profile_by_name(data, &film)?,
                        profile::load_profile_by_name(data, &paper)?,
                        profile::load_profile_by_name(data, &preset.paper)?,
                        &params,
                        data,
                    )
                    .map_err(anyhow::Error::msg)?;
                    (params.enlarger.m_filter_shift, params.enlarger.y_filter_shift) = (m as f32, y as f32);
                }
                paper = preset.paper;
            } else {
                (film, paper) = (preset.film, preset.paper);
            }
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
        // decode of its own, as does a tile with another flat field (a film
        // printed as a scene, on a negative divided by one). It gets a
        // reduced copy here, so the main render keeps its decoded source.
        // (A preset keeps the lens correction and the interpretation, and on
        // a negative the white balance; a film's tile keeps all three.)
        let flat = LightFrame::requested(&params);
        let own_decode = !decode_only
            && self.source_generation == source_generation
            && self.source.as_ref().is_some_and(|s| {
                s.path == input
                    && (preset_applied && spektrafilm_io::is_raw(&input) && s.wb.effective() != wb.effective()
                        || s.flat.as_ref().map(|(light, _)| light) != flat.as_ref())
            });
        ensure!(!decode_only || !preset_applied, "A preset's thumbnail can't be decoded alone");
        // The camera preview's colour space beside the decode, which takes
        // far longer: the match needs both.
        let declared = std::thread::scope(|scope| -> Result<Option<Option<&'static str>>> {
            let declared = colour_space.then(|| scope.spawn(|| declared_preview_colour_space(&input)));
            if !own_decode {
                let limit = if reduced_decode { reduced_read_side(long_side, crop.as_ref()) } else { 0 };
                self.load_source(&input, source_generation, wb, lens, interpretation.clone(), limit, &params)?;
            }
            declared.map(|thread| thread.join().map_err(|_| anyhow::anyhow!("Reading the preview's colour space failed"))?).transpose()
        })?;
        let decoded = started.elapsed();
        let Source { full, reduced_to, .. } = self.source.as_ref().unwrap();
        let (full, mut reduced) = (*full, reduced_to.is_some());
        // The thumbnail is the whole frame; Image Size describes the export.
        image_size::clear(&mut params);
        let dimensions = thumbnail_dimensions(full, long_side, crop.as_ref());
        let (working, metering) = if own_decode {
            let found = self
                .thumbnail_wb_sources
                .iter()
                .position(|s| s.wb == wb.effective() && s.flat == flat && s.reduced == reduced_decode);
            let index = match found {
                Some(index) => index,
                None => {
                    let limit = if reduced_decode { THUMBNAIL_FRAME_MAX } else { 0 };
                    let mut loaded = spektrafilm_io::load_image_colour_limited(
                        &input, spektrafilm_io::Formats::Any, wb, lens, &interpretation, limit,
                    )?;
                    if let Some(light) = &flat {
                        let mut flat_params = (*params).clone();
                        flat_params.io.apply_input_colour(&loaded.colour);
                        self.flatten(&mut loaded.image, &input, &light.path, &flat_params);
                    }
                    let metering = spektrafilm_core::stages::filming::metering_preview(&loaded.image);
                    // The largest frame any thumbnail develops, so a new crop
                    // reduces this copy instead of decoding again.
                    let largest = within(thumbnail_dimensions(full, THUMBNAIL_FRAME_MAX, None), &loaded.image);
                    let image = if largest == (loaded.image.width, loaded.image.height) {
                        loaded.image
                    } else {
                        downsample_area(&loaded.image, largest)
                    };
                    if self.thumbnail_wb_sources.len() >= THUMBNAIL_WB_SOURCES {
                        self.thumbnail_wb_sources.remove(0);
                    }
                    self.thumbnail_wb_sources.push(WbSource {
                        wb: wb.effective(), flat: flat.clone(), colour: loaded.colour, image, metering, reduced: reduced_decode,
                    });
                    self.thumbnail_wb_sources.len() - 1
                }
            };
            let entry = self.thumbnail_wb_sources.remove(index);
            params.io.apply_input_colour(&entry.colour);
            reduced = entry.reduced;
            let dimensions = within(dimensions, &entry.image);
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
            // A decode alone develops nothing, so it has no frame to shrink.
            let working = if decode_only {
                ImageBuf::from_data(1, 1, vec![Default::default(); 3])
            } else {
                let dimensions = within(dimensions, source);
                working_source(source, &mut self.thumbnail_source, dimensions, downsample_area).clone()
            };
            (working, self.metering.clone())
        };
        let resampled = started.elapsed();
        let key = preview_pipeline_cache_key(&film, &paper, &params);
        let (calibrated, cached) =
            self.pipelines.get_or_build(&key, false, || build_pipeline(data, &film, &paper, &params))?;
        let mut pipeline = calibrated.clone().with_params((*params).clone());
        if let Some(metering) = metering {
            pipeline = pipeline.with_metering_preview(metering);
        }
        let built = started.elapsed();
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        if decode_only {
            return Ok(json!({
                "source_width": full.0, "source_height": full.1, "reduced_decode": reduced,
                "raw": spektrafilm_io::is_raw(&input),
                "preview_colour_space": declared.flatten(), "elapsed_ms": ms(started.elapsed()), "cached_pipeline": cached,
            }));
        }
        let backend = self.backend.get_or_insert_with(spektrafilm_gpu::select_backend);
        let (output, used_backend) = pipeline.process_with_backend(working, backend.as_ref());
        let cropped = crop.map(|crop| crop.apply(&output));
        let output = cropped.as_ref().unwrap_or(&output);
        let processed = started.elapsed();
        let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
        let (width, height) = write_display(output, encoding, &post, &preview_path)?;
        let mut result = json!({
            "width": width, "height": height, "backend": used_backend,
            "display_color_space": DisplaySpace::of(encoding).name(),
            // The photograph's whole size, for a camera brightness match and the
            // layout, even from a reduced read.
            "source_width": full.0, "source_height": full.1, "reduced_decode": reduced,
            "elapsed_ms": ms(started.elapsed()), "cached_pipeline": cached,
            "timing_ms": {"decode": ms(decoded), "resample": ms(resampled - decoded),
                "pipeline": ms(built - resampled), "process": ms(processed - built),
                "write": ms(started.elapsed() - processed)},
        });
        if let Some(declared) = declared {
            result["preview_colour_space"] = json!(declared);
        }
        Ok(result)
    }

    /// `params` with the base found (or read at `point`) and the print
    /// balanced for `film`, as `negative_base` answers and the app writes
    /// them on choosing it: what a read-as tile shows. About 0.2 s on the
    /// analysis frame. The print exposure stays the current one: metering it
    /// again (as the app does after) takes about 0.9 s a film, too long for
    /// every tile of the tray, so a tile can print a little lighter or
    /// darker than the film will once chosen.
    #[allow(clippy::too_many_arguments)]
    fn read_negative_for_tile(
        &mut self,
        input: &Path,
        source_generation: u64,
        film: &str,
        paper: &str,
        params: &mut RuntimeParams,
        point: Option<[f64; 2]>,
        data: &Path,
    ) -> Result<()> {
        let mut read = params.clone();
        let (film, paper, frame) = self.negative_frame(input, source_generation, film, paper, &mut read, data)?;
        let found = spektrafilm_core::negative::detect_base(&film, &read, &frame, point.map(|[x, y]| (x, y))).map_err(anyhow::Error::msg)?;
        read.film_render.base = found.base.clone();
        read.film_render.convert.exposure_compensation_ev = found.exposure_ev;
        let neutral = spektrafilm_core::negative::neutral_balance(film, paper, &read, data).map_err(anyhow::Error::msg)?;
        params.film_render.base = found.base;
        params.film_render.convert.exposure_compensation_ev = found.exposure_ev;
        (params.enlarger.m_filter_shift, params.enlarger.y_filter_shift) = (neutral.m_filter_shift as f32, neutral.y_filter_shift as f32);
        Ok(())
    }

    /// The film and paper profiles and the reduced frame a negative's base
    /// and balances read, decoding `input` if the session doesn't hold it.
    /// `params` get the source's input colour.
    fn negative_frame(
        &mut self,
        input: &Path,
        source_generation: u64,
        film: &str,
        paper: &str,
        params: &mut RuntimeParams,
        data: &Path,
    ) -> Result<(profile::Profile, profile::Profile, ImageBuf)> {
        let frame = self.negative_scan(input, source_generation, params)?;
        Ok((profile::load_profile_by_name(data, film)?, profile::load_profile_by_name(data, paper)?, frame))
    }

    /// The reduced frame of `input` a negative's base and balances read
    /// (`negative_frame`), without the profiles.
    fn negative_scan(&mut self, input: &Path, source_generation: u64, params: &mut RuntimeParams) -> Result<ImageBuf> {
        ensure!(params.workflow.converts_film(), "The photograph isn't read as a negative");
        params.io.raw_white_balance.validate()?;
        params.io.input_interpretation.validate()?;
        params.io.scan_film = false;
        spektrafilm_core::illuminants::standard_illuminant_f64(&params.film_render.convert.scan_illuminant)
            .map_err(|_| anyhow::anyhow!("Unknown scan light: {}", params.film_render.convert.scan_illuminant))?;
        let (wb, lens) = (params.io.raw_white_balance, params.io.lens_correction);
        let interpretation = params.io.input_interpretation.clone();
        let side = spektrafilm_core::negative::ANALYSIS_SIDE;
        self.load_source(input, source_generation, wb, lens, interpretation, side, params)?;
        let source = self.source.as_ref().unwrap();
        params.io.apply_input_colour(&source.colour);
        ensure!(RgbColourspace::find(&params.io.input_color_space).is_some(),
            "Unknown input colour space: {}", params.io.input_color_space);
        Ok(self.negative_frame.get_or_insert_with(|| spektrafilm_core::negative::analysis_frame(&source.image)).clone())
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
            Command::Thumbnail {
                input, source_generation, film, paper, mut params, long_side, preview_path, crop, preset, reduced_decode, decode_only,
                preview_colour_space, read_negative,
            } => {
                if let Some(read) = read_negative {
                    self.read_negative_for_tile(&input, source_generation, &film, &paper, &mut params, read.point, data)?;
                }
                self.thumbnail(
                    input, source_generation, film, paper, params, long_side, preview_path, crop, preset, reduced_decode, decode_only,
                    preview_colour_space, data,
                )
            }
            Command::ToneCurve { film, paper, params, count } => {
                let started = Instant::now();
                let params = spektrafilm_core::tone_curve::tone_params(&params);
                let key = preview_pipeline_cache_key(&film, &paper, &params);
                let (calibrated, cached) =
                    self.pipelines.get_or_build(&key, false, || build_pipeline(data, &film, &paper, &params))?;
                let pipeline = calibrated.clone().with_params(params);
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
            Command::NegativeBase { input, source_generation, film, paper, mut params, point } => {
                let started = Instant::now();
                let (film, paper, frame) = self.negative_frame(&input, source_generation, &film, &paper, &mut params, data)?;
                let point = point.map(|[x, y]| (x, y));
                if let Some((x, y)) = point {
                    ensure!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y), "The point is outside the photograph");
                }
                let found = spektrafilm_core::negative::detect_base(&film, &params, &frame, point).map_err(anyhow::Error::msg)?;
                params.film_render.base = found.base.clone();
                params.film_render.convert.exposure_compensation_ev = found.exposure_ev;
                let neutral = spektrafilm_core::negative::neutral_balance(film, paper, &params, data).map_err(anyhow::Error::msg)?;
                let (m, y) = (neutral.m_filter_shift, neutral.y_filter_shift);
                // The picture inside the film's clear edges, in the decoded
                // photograph's own frame (as `point`): the crop rectangle
                // x0..x1, y0..y1 after straightening by angle_deg (clockwise).
                let picture = found.verdict.picture.as_ref().map(|p| json!({
                    "x0": p.x0, "y0": p.y0, "x1": p.x1, "y1": p.y1, "angle_deg": p.angle_deg,
                    "edges": p.edges.iter().map(|e| e.name()).collect::<Vec<_>>(),
                }));
                Ok(json!({
                    "base": found.base, "exposure_ev": found.exposure_ev,
                    "found": found.verdict.found, "picked": found.picked,
                    "edge": found.verdict.edge.map(|e| e.name()),
                    "edges": found.verdict.edges.iter().map(|e| e.name()).collect::<Vec<_>>(),
                    "picture": picture,
                    "looks_negative": found.looks_negative,
                    "flat_field": self.flat_answer(),
                    "m_filter_shift": m, "y_filter_shift": y,
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                }))
            }
            Command::NegativePaper { film, from_paper, paper, params } => {
                let started = Instant::now();
                let (m, y) = spektrafilm_core::negative::paper_change(
                    &profile::load_profile_by_name(data, &film)?,
                    profile::load_profile_by_name(data, &from_paper)?,
                    profile::load_profile_by_name(data, &paper)?,
                    &params,
                    data,
                )
                .map_err(anyhow::Error::msg)?;
                Ok(json!({"m_filter_shift": m, "y_filter_shift": y,
                          "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}))
            }
            Command::NegativeCheck { input, source_generation, film, mut params } => {
                let started = Instant::now();
                // A scene's source was decoded without a light frame: keep it so.
                if !params.workflow.converts_film() {
                    params.film_render.convert.flat_field.clear();
                    params.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
                }
                let frame = self.negative_scan(&input, source_generation, &mut params)?;
                let film = profile::load_profile_by_name(data, &film)?;
                let found = spektrafilm_core::negative::detect_base(&film, &params, &frame, None).map_err(anyhow::Error::msg)?;
                Ok(json!({"looks_negative": found.looks_negative, "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0}))
            }
            Command::NegativeAuto { input, source_generation, film, paper, mut params, bounds } => {
                let started = Instant::now();
                let bounds = picture_bounds(bounds)?;
                let (film, paper, frame) = self.negative_frame(&input, source_generation, &film, &paper, &mut params, data)?;
                let auto = spektrafilm_core::negative::auto_print(film, paper, &params, data, &frame, &bounds).map_err(anyhow::Error::msg)?;
                Ok(json!({
                    "m_filter_shift": auto.m_filter_shift, "y_filter_shift": auto.y_filter_shift,
                    "print_exposure": auto.print_exposure,
                    "flat_field": self.flat_answer(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                }))
            }
            Command::NegativeMeter { input, source_generation, film, paper, mut params, bounds } => {
                let started = Instant::now();
                let bounds = picture_bounds(bounds)?;
                let (film, paper, frame) = self.negative_frame(&input, source_generation, &film, &paper, &mut params, data)?;
                let exposure = spektrafilm_core::negative::meter_print_exposure(film, paper, &params, data, &frame, &bounds)
                    .map_err(anyhow::Error::msg)?;
                Ok(json!({
                    "print_exposure": exposure,
                    "flat_field": self.flat_answer(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                }))
            }
            Command::NegativeNeutral { input, source_generation, film, paper, mut params, point: [x, y] } => {
                let started = Instant::now();
                ensure!((0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y), "The point is outside the photograph");
                let (film, paper, frame) = self.negative_frame(&input, source_generation, &film, &paper, &mut params, data)?;
                let neutral = spektrafilm_core::negative::neutral_print(film, paper, &params, data, &frame, (x, y))
                    .map_err(anyhow::Error::msg)?;
                Ok(json!({
                    "m_filter_shift": neutral.m_filter_shift, "y_filter_shift": neutral.y_filter_shift,
                    "flat_field": self.flat_answer(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                }))
            }
            Command::ReleaseThumbnails => {
                self.thumbnail_source = None;
                self.pipelines.release_unpinned();
                self.thumbnail_wb_sources = Vec::new();
                Ok(json!({}))
            }
            Command::ReleaseFilmCache => {
                let bytes = self.backend.as_ref().map_or(0, |backend| backend.release_film_cache());
                Ok(json!({"released_bytes": bytes}))
            }
            Command::PreviewColourSpace { input } => Ok(json!({ "colour_space": declared_preview_colour_space(&input)? })),
            Command::BrightnessReference { input, reference_path, expected_width, expected_height } => {
                ensure!(expected_width > 0 && expected_height > 0,
                    "The brightness reference needs the photograph's dimensions");
                ensure!(input != reference_path
                    && !std::fs::canonicalize(&reference_path).ok().is_some_and(|output|
                        std::fs::canonicalize(&input).ok().as_ref() == Some(&output)),
                    "The brightness reference must not replace the original photograph");
                match spektrafilm_io::brightness::reference(&input, (expected_width, expected_height)) {
                    Ok(reference) => {
                        spektrafilm_io::brightness::write_reference(&reference, &reference_path)?;
                        Ok(json!({"usable": true, "reference_path": reference_path,
                            "width": reference.width(), "height": reference.height()}))
                    }
                    Err(error) => {
                        if let Some(spektrafilm_io::brightness::Unusable(reason)) = error.downcast_ref::<spektrafilm_io::brightness::Unusable>() {
                            Ok(json!({"usable": false, "reason": reason, "for_photo": true}))
                        } else {
                            Err(error)
                        }
                    }
                }
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
                                      &params.io.input_interpretation, LightFrame::requested(&params).as_ref()),
                    "The photograph or RAW settings changed; wait for its current preview and try again");
                let reference = placement_reference(&reference_path, &source.image)?;
                params.io.apply_input_colour(&source.colour);
                ensure!(RgbColourspace::find(&params.io.input_color_space).is_some(),
                    "Unknown input colour space: {}", params.io.input_color_space);
                image_size::clear(&mut params);
                let key = preview_pipeline_cache_key(&film, &paper, &params);
                // Borrow the calibrated look when possible, but keep all
                // placement work local. Save must still see the last preview.
                let mut pipeline = match self.pipelines.peek(&key) {
                    Some(pipeline) => pipeline.clone().with_params((*params).clone()),
                    None => build_pipeline(data, &film, &paper, &params)?,
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
                flip,
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
                // Crop in the unrotated orientation, then flip and rotate, then add the border.
                let cropped = crop.map(|crop| crop.apply(image));
                let image = cropped.as_ref().unwrap_or(image);
                let adjusted = (!crop_after_post && !post.is_identity())
                    .then(|| PostProcess::new(post, encoded, encoding.space.name).applied(image));
                let image = adjusted.as_ref().unwrap_or(image);
                let rotated = (flip || rotate_cw.rem_euclid(4) != 0)
                    .then(|| image.clone().flipped_rotated_cw(flip, rotate_cw));
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
                self.load_source(&input, source_generation, wb, lens, interpretation, 0, &params)?;
                let flat_answer = self.source.as_ref().unwrap().flat_answer();
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
                let before_build = started.elapsed();
                let (calibrated, cached_pipeline) =
                    self.pipelines.get_or_build(&key, true, || build_pipeline(data, &film, &paper, &params))?;
                let mut pipeline = calibrated.clone().with_params((*params).clone());
                let pipeline_ms = (started.elapsed() - before_build).as_secs_f64() * 1000.0;
                if let Some(metering) = &self.metering {
                    pipeline = pipeline.with_metering_preview(metering.clone());
                }
                // The tone graph's histogram: the photograph's film exposure,
                // from the metering frame (the whole frame, before spatial effects).
                // Prep's exposure is a uniform gain, so it shifts the stops like
                // the camera's; its other controls reshape them, so then the
                // frame goes through Prep and is counted as exposed.
                let histogram = if params.workflow.converts_film() {
                    // A negative: the film density the converter reads,
                    // as the exposure that made it on the film's neutral
                    // axis, on the same stops. Nothing shifts it.
                    let metering = self
                        .metering
                        .get_or_insert_with(|| spektrafilm_core::stages::filming::metering_preview(source));
                    let stops_key = format!(
                        "negative|{}|{source_generation}|{}|{}",
                        input.display(),
                        key,
                        serde_json::to_string(&params.film_render)?,
                    );
                    if self.film_stops.as_ref().is_none_or(|(old, _)| old != &stops_key) {
                        self.film_stops = Some((stops_key, spektrafilm_core::negative::film_stops(&pipeline, metering)));
                    }
                    let counts = spektrafilm_core::tone_curve::film_histogram(&self.film_stops.as_ref().unwrap().1, 0.0, 128);
                    json!({"stops": spektrafilm_core::tone_curve::STOPS, "ev": 0.0, "counts": counts})
                } else {
                    let metering = self
                        .metering
                        .get_or_insert_with(|| spektrafilm_core::stages::filming::metering_preview(source));
                    let ae = if params.camera.auto_exposure { pipeline.metering_film_raw(metering).1 } else { 0.0 };
                    let reshaped = !spektrafilm_core::prep::PrepParams { exposure_ev: 0.0, ..params.prep }.is_identity();
                    let mut stops_key = format!("{}|{source_generation}|{}", input.display(), key);
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
                    pipeline = pipeline.with_prepared_frame(self.prepared_photo.clone()).with_film_cache(0, self.source_loads);
                    working_source(
                        source,
                        &mut self.scaled,
                        dimensions,
                        rescale_preview_input_fast,
                    )
                } else {
                    pipeline = pipeline.with_prepared_frame(self.prepared_interactive.clone()).with_film_cache(1, self.source_loads);
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
                    "flat_field": flat_answer,
                    "output_color_space": encoding.space.name,
                    "display_color_space": DisplaySpace::of(encoding).name(),
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                    "film_histogram": histogram,
                    "cached_pipeline": cached_pipeline, "pipeline_ms": pipeline_ms,
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

/// What a light frame read for a flat field depends on: the file (its size
/// and last change) and how it is decoded, which is how its scans are. (A
/// RAW's white balance mixes into its colours through the camera matrix, so
/// it changes the map; a preset keeps a negative's, so preset tiles share
/// the light frame read.)
fn light_frame_key(light: &Path, params: &RuntimeParams) -> String {
    let changed = LightFrame::new(light).stamp;
    let io = &params.io;
    format!(
        "{}|{changed:?}|{:?}|{:?}|{}|{}|{}",
        light.display(),
        io.raw_white_balance,
        io.lens_correction,
        serde_json::to_string(&io.input_interpretation).unwrap_or_default(),
        io.input_color_space,
        io.input_cctf_decoding,
    )
}

/// A negative's picture bounds from a request, checked; the default
/// without them.
fn picture_bounds(bounds: Option<[f64; 4]>) -> Result<spektrafilm_core::negative::PictureBounds> {
    bounds.map_or(Ok(Default::default()), |b| spektrafilm_core::negative::PictureBounds::new(b).map_err(anyhow::Error::msg))
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

/// A calibrated pipeline for `film` printed on `paper` (or scanned).
fn build_pipeline(data: &Path, film: &str, paper: &str, params: &RuntimeParams) -> Result<Pipeline> {
    let film_profile = profile::load_profile_by_name(data, film)?;
    // A negative is converted and printed, never scanned as film.
    let scanned = params.io.scan_film && !params.workflow.converts_film();
    let print_profile = profile::load_profile_by_name(data, if scanned { film } else { paper })?;
    let mut params = params.clone();
    params.io.scan_film = scanned;
    Pipeline::new_with_spectral(film_profile, print_profile, params, data).map_err(anyhow::Error::msg)
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
    fn brightness_reference_is_independent_and_refuses_to_replace_the_photo() {
        let dir = std::env::temp_dir().join(format!("negs-reference-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let source = ImageBuf::from_data(4, 2, [0.2, 0.5, 0.8].repeat(8));
        spektrafilm_io::save_image(&source, &input, None).unwrap();
        let before = std::fs::read(&input).unwrap();
        let mut session = Session {
            source_generation: 31,
            output: Some(Rendered { id: 7, key: Value::Null, response: Value::Null,
                source: input.clone(), film: "kodak_gold_200".into(), encoding: Encoding::from(true),
                post: PostParams::default(), image: source.clone() }),
            ..Default::default()
        };
        let output = dir.join("reference.png");
        let command = |reference_path| Command::BrightnessReference {
            input: input.clone(), reference_path, expected_width: 4, expected_height: 2,
        };
        let result = session.handle(Request { id: 8, command: command(output.clone()) }, Path::new(".")).unwrap();
        assert_eq!((result["usable"].as_bool(), result["width"].as_u64(), result["height"].as_u64()),
            (Some(true), Some(4), Some(2)));
        assert!(session.source.is_none() && !session.pipelines.has_main());
        assert_eq!(session.source_generation, 31);
        let retained = session.output.as_ref().unwrap();
        assert_eq!(retained.id, 7);
        assert_eq!(retained.image.data, source.data);
        let reference = placement_reference(&output, &source).unwrap();
        assert_eq!(reference.width, 4);
        let error = session.handle(Request { id: 9, command: command(input.clone()) }, Path::new(".")).unwrap_err();
        assert!(error.to_string().contains("must not replace"));
        assert_eq!(std::fs::read(&input).unwrap(), before);
        let alpha = dir.join("alpha.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([128, 128, 128, 255])).save(&alpha).unwrap();
        let unsuitable = session.handle(Request { id: 10, command: Command::BrightnessReference {
            input: alpha, reference_path: output, expected_width: 4, expected_height: 2,
        } }, Path::new(".")).unwrap();
        assert_eq!(unsuitable["usable"], false);
        assert_eq!(unsuitable["for_photo"], true);
        assert_eq!(unsuitable["reason"], "Transparent images can’t be matched.");
        std::fs::remove_dir_all(dir).unwrap();
    }
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
    fn catalog_marks_fotufilm_ports_and_their_sampled_films() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let result = catalog(&data).unwrap();
        let profiles = result["profiles"].as_array().unwrap();
        let provenance = |p: &Value| (p["fotufilm"].as_bool().unwrap(), p["manufacturer_data"].as_bool().unwrap());
        let fotufilm: Vec<_> = profiles.iter().filter(|p| provenance(p).0).collect();
        let sampled: Vec<_> = fotufilm.iter().filter(|p| !provenance(p).1).map(|p| p["id"].as_str().unwrap()).collect();
        assert_eq!(fotufilm.len(), 29);
        assert_eq!(sampled.len(), 8, "{sampled:?}");
        assert!(sampled.contains(&"cinestill_400d") && !sampled.contains(&"fujifilm_superia_200"));
        // Spektrafilm's own profiles are from manufacturer data.
        assert!(profiles.iter().filter(|p| !provenance(p).0).all(|p| provenance(p).1));
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
                preset: preset.map(str::to_string), reduced_decode: false, decode_only: false, preview_colour_space: false, read_negative: None,
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
        let (_, plain) = thumbnail(&mut session, "kodak_gold_200", "kodak_supra_endura", current.clone(), None, "plain.rgba");
        assert_ne!(with_preset, plain);

        // On a negative the preset keeps the film it's read as, its base and
        // its balance; its own paper moves the filtration by the papers'
        // difference (`negative_paper`).
        let mut negative = current;
        negative.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
        negative.film_render.base.cyan = 1.05;
        negative.enlarger.print_exposure = 0.9;
        let (_, with_preset) =
            thumbnail(&mut session, "kodak_gold_200", "kodak_supra_endura", negative.clone(), Some(text), "negative-preset.rgba");
        let mut expected = presets::apply(&presets::import(text, &data).unwrap().params, &negative);
        assert_eq!((expected.film_render.base.cyan, expected.enlarger.print_exposure), (1.05, 0.9));
        let (m, y) = spektrafilm_core::negative::paper_change(
            &profile::load_profile_by_name(&data, "kodak_gold_200").unwrap(),
            profile::load_profile_by_name(&data, "kodak_supra_endura").unwrap(),
            profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap(),
            &expected,
            &data,
        )
        .unwrap();
        (expected.enlarger.m_filter_shift, expected.enlarger.y_filter_shift) = (m as f32, y as f32);
        let (_, applied) = thumbnail(&mut session, "kodak_gold_200", "kodak_portra_endura", expected, None, "negative-applied.rgba");
        assert_eq!(with_preset, applied);
        assert!(session.output.is_none() && !session.pipelines.has_main());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A reduced read is never smaller than the frame a thumbnail develops,
    /// whatever the photograph's shape, and an uncropped one is exactly it.
    #[test]
    fn reduced_reads_cover_the_thumbnail_frame() {
        for full in [(6048, 4024), (4024, 6048), (8000, 2000), (1000, 1000), (300, 200)] {
            for crop in [None, Some((0.5, 0.5)), Some((0.9, 0.2)), Some((0.15, 0.8)), Some((0.01, 0.01))] {
                let crop = crop.map(|(width, height)| Crop { x: 0.0, y: 0.0, width, height, angle: 0.0 });
                for long_side in [16, 512, 1024] {
                    let read = reduced_read_side(long_side, crop.as_ref());
                    let (w, h) = thumbnail_dimensions(full, long_side, crop.as_ref());
                    assert!(read >= w.max(h) || read >= full.0.max(full.1), "{full:?} {crop:?} {long_side}: {read}");
                    assert!(read <= THUMBNAIL_FRAME_MAX);
                    if crop.is_none() && full.0.max(full.1) >= long_side {
                        assert_eq!(read, w.max(h));
                    }
                }
            }
        }
        let small = ImageBuf::new(512, 340);
        assert_eq!(within((512, 341), &small), (512, 340));
    }

    /// A non-RAW file is always read whole: the same thumbnail either way.
    #[test]
    fn a_reduced_read_of_a_non_raw_file_is_the_whole_file() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-reduced-thumbnail-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("input.png");
        let pixels = (0..48 * 32 * 3).map(|i| ((i * 37) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(48, 32, pixels), &input, None).unwrap();
        let mut session = Session::default();
        let mut thumbnail = |reduced_decode: bool| {
            let preview = dir.join(format!("{reduced_decode}.rgba"));
            let response = session.handle(Request { id: 1, command: Command::Thumbnail {
                input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                paper: "kodak_supra_endura".into(), params: Box::default(), long_side: 24,
                preview_path: preview.clone(), crop: None, preset: None, reduced_decode, decode_only: false,
                preview_colour_space: false, read_negative: None,
            } }, &data).unwrap();
            (response, std::fs::read(preview).unwrap())
        };
        let (whole, whole_pixels) = thumbnail(false);
        let (reduced, reduced_pixels) = thumbnail(true);
        assert_eq!((reduced["source_width"].as_u64(), reduced["source_height"].as_u64()), (Some(48), Some(32)));
        assert_eq!(reduced["reduced_decode"], false);
        assert_eq!(whole["width"], reduced["width"]);
        assert!(whole_pixels == reduced_pixels);

        // Decoded alone: the size, no preview, and nothing declared.
        let preview = dir.join("decoded.rgba");
        let decoded = session.handle(Request { id: 1, command: Command::Thumbnail {
            input: input.clone(), source_generation: 2, film: "kodak_gold_200".into(),
            paper: "kodak_supra_endura".into(), params: Box::default(), long_side: 24,
            preview_path: preview.clone(), crop: None, preset: None, reduced_decode: true, decode_only: true,
            preview_colour_space: true, read_negative: None,
        } }, &data).unwrap();
        assert_eq!((decoded["source_width"].as_u64(), decoded["source_height"].as_u64()), (Some(48), Some(32)));
        assert!(decoded["preview_colour_space"].is_null() && decoded.get("width").is_none());
        assert!(!preview.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The match after a decode alone is the one after the whole thumbnail,
    /// and the colour space read beside the decode is `preview_colour_space`'s.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn a_match_after_a_decode_alone_is_the_same() {
        let input = PathBuf::from(std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE").expect("SPEKTRAFILM_RAW_TEST_FILE"));
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-decode-only-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut params = RuntimeParams::default();
        params.io.output_color_space = "Display P3".into();
        let reference = dir.join("reference.png");
        let placed = |decode_only: bool| {
            let mut session = Session::default();
            let decoded = session.handle(Request { id: 1, command: Command::Thumbnail {
                input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                paper: "kodak_supra_endura".into(), params: Box::new(params.clone()), long_side: 512,
                preview_path: dir.join("thumbnail.rgba"), crop: None, preset: None, reduced_decode: true, decode_only,
                preview_colour_space: decode_only, read_negative: None,
            } }, &data).unwrap();
            let (width, height) = (decoded["source_width"].as_u64().unwrap() as u32, decoded["source_height"].as_u64().unwrap() as u32);
            if !reference.exists() {
                session.handle(Request { id: 2, command: Command::BrightnessReference {
                    input: input.clone(), reference_path: reference.clone(), expected_width: width, expected_height: height,
                } }, &data).unwrap();
            }
            let placed = session.handle(Request { id: 3, command: Command::PlaceExposure {
                input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(), paper: "kodak_supra_endura".into(),
                params: Box::new(params.clone()), reference_path: reference.clone(),
            } }, &data).unwrap();
            (decoded, placed)
        };
        let (_, whole) = placed(false);
        let (decoded, alone) = placed(true);
        assert_eq!(whole["exposure_ev"], alone["exposure_ev"]);
        assert_eq!(whole["highlight_limited"], alone["highlight_limited"]);
        let declared = Session::default().handle(Request { id: 4, command: Command::PreviewColourSpace { input: input.clone() } }, &data).unwrap();
        assert_eq!(decoded["preview_colour_space"], declared["colour_space"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Ticket 39: the editor solves a new photograph's match on a whole
    /// decode alone, before any render. The match is the one solved after
    /// the settled render, as before, with and without auto exposure.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn the_editors_match_before_the_first_render_is_the_same() {
        let input = PathBuf::from(std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE").expect("SPEKTRAFILM_RAW_TEST_FILE"));
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-match-first-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let reference = dir.join("reference.png");
        for auto_exposure in [true, false] {
            let mut params = RuntimeParams::default();
            params.camera.auto_exposure = auto_exposure;
            let placed = |decode_first: bool| {
                let mut session = Session::default();
                let (width, height) = if decode_first {
                    let decoded = session.handle(Request { id: 1, command: Command::Thumbnail {
                        input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                        paper: "kodak_supra_endura".into(), params: Box::new(params.clone()), long_side: 16,
                        preview_path: dir.join("decode.rgba"), crop: None, preset: None, reduced_decode: false, decode_only: true,
                        preview_colour_space: true, read_negative: None,
                    } }, &data).unwrap();
                    assert_eq!(decoded["raw"].as_bool(), Some(true));
                    (decoded["source_width"].as_u64().unwrap() as u32, decoded["source_height"].as_u64().unwrap() as u32)
                } else {
                    let rendered = session.handle(Request { id: 1, command: Command::Render {
                        input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                        paper: "kodak_supra_endura".into(), params: Box::new(params.clone()), limit_preview: false,
                        preview_path: dir.join("render.rgba"), interactive_scale: None, crop: None,
                    } }, &data).unwrap();
                    (rendered["source_width"].as_u64().unwrap() as u32, rendered["source_height"].as_u64().unwrap() as u32)
                };
                if !reference.exists() {
                    session.handle(Request { id: 2, command: Command::BrightnessReference {
                        input: input.clone(), reference_path: reference.clone(), expected_width: width, expected_height: height,
                    } }, &data).unwrap();
                }
                let placed = session.handle(Request { id: 3, command: Command::PlaceExposure {
                    input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(), paper: "kodak_supra_endura".into(),
                    params: Box::new(params.clone()), reference_path: reference.clone(),
                } }, &data).unwrap();
                ((width, height), placed)
            };
            let (size, rendered) = placed(false);
            let (decoded_size, decoded) = placed(true);
            assert_eq!(size, decoded_size);
            assert_eq!(rendered["exposure_ev"], decoded["exposure_ev"], "auto exposure {auto_exposure}");
            assert_eq!(rendered["highlight_limited"], decoded["highlight_limited"]);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A reduced read reports the whole photograph's size, and anything that
    /// needs more of it decodes again; a whole source serves a reduced request.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn a_reduced_read_is_decoded_again_for_more() {
        let input = PathBuf::from(std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE").expect("SPEKTRAFILM_RAW_TEST_FILE"));
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-reduced-raw-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut session = Session::default();
        let mut thumbnail = |long_side: u32, reduced_decode: bool| {
            session.handle(Request { id: 1, command: Command::Thumbnail {
                input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                paper: "kodak_supra_endura".into(), params: Box::default(), long_side,
                preview_path: dir.join("thumbnail.rgba"), crop: None, preset: None, reduced_decode, decode_only: false,
                preview_colour_space: false, read_negative: None,
            } }, &data).unwrap()
        };
        let reduced = thumbnail(256, true);
        assert_eq!(reduced["reduced_decode"], true);
        // Smaller than the read: no new decode.
        assert_eq!(thumbnail(128, true)["timing_ms"]["decode"].as_f64().map(|ms| ms < 5.0), Some(true));
        assert_eq!(thumbnail(512, true)["reduced_decode"], true);
        let whole = thumbnail(256, false);
        assert_eq!(whole["reduced_decode"], false);
        assert_eq!(
            (reduced["source_width"].clone(), reduced["source_height"].clone()),
            (whole["source_width"].clone(), whole["source_height"].clone())
        );
        assert_eq!((reduced["width"].clone(), reduced["height"].clone()), (whole["width"].clone(), whole["height"].clone()));
        assert_eq!(thumbnail(256, true)["reduced_decode"], false);
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
    /// Save cuts the crop from the unmirrored render, then flips it left to
    /// right, then turns it.
    #[test]
    fn save_crops_then_flips_then_turns() {
        let dir = std::env::temp_dir().join(format!("negs-flip-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 4 × 2, each pixel's red its column and green its row (in eighths).
        let mut source = ImageBuf::new(4, 2);
        for y in 0..2 {
            for x in 0..4 {
                source.set(x, y, [x as f32 / 8.0, y as f32 / 8.0, 0.0]);
            }
        }
        let mut session = Session {
            output: Some(Rendered { id: 1, key: Value::Null, response: Value::Null,
                source: dir.join("input.png"), film: "kodak_gold_200".into(), encoding: Encoding::from(true),
                post: PostParams::default(), image: source }),
            ..Default::default()
        };
        // The right three columns, flipped, then a quarter turn clockwise.
        let saved = dir.join("saved.png");
        let crop = Crop { x: 0.25, y: 0.0, width: 0.75, height: 1.0, angle: 0.0 };
        session.handle(Request { id: 2, command: Command::Save {
            render_id: 1, output: saved.clone(), save_options: Default::default(), crop_after_post: false, border: None,
            border_options: BorderOptions::default(), instagram: false,
            crop: Some(crop), rotate_cw: 1, flip: true, fit: None,
        } }, Path::new(".")).unwrap();
        let out = image::open(&saved).unwrap().to_rgb8();
        assert_eq!(out.dimensions(), (2, 3));
        let at = |x: u32, y: u32| {
            let p = out.get_pixel(x, y);
            ((p[0] as f32 / 255.0 * 8.0).round() as u32, (p[1] as f32 / 255.0 * 8.0).round() as u32)
        };
        // Flipped, the cut's columns run 3, 2, 1; the quarter turn puts its
        // bottom row on the left and its first column at the top.
        assert_eq!([at(0, 0), at(1, 0)], [(3, 1), (3, 0)]);
        assert_eq!([at(0, 2), at(1, 2)], [(1, 1), (1, 0)]);
        std::fs::remove_dir_all(dir).unwrap();
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
            crop: None, rotate_cw: 0, flip: false, fit: None,
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
            crop: None, rotate_cw: 0, flip: false, fit: None,
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
                border_options: BorderOptions::default(), instagram: false, crop: None, rotate_cw: 0, flip: false, fit: None,
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

    /// The app renders a film change at canvas size first (ticket 28): the
    /// export-size render that follows matches one made without that step,
    /// in the kept output and in the display buffer.
    #[test]
    fn a_quick_frame_before_a_film_change_leaves_its_settled_render_unchanged() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-quick-frame-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, preview) = (dir.join("input.png"), dir.join("preview.rgba"));
        let pixels = (0..96 * 64 * 3).map(|i| ((i * 37) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(96, 64, pixels), &input, None).unwrap();
        let with_film = |film: &str, interactive_scale: Option<f32>| {
            let Command::Render { mut params, .. } = render(&input, &preview, None, None, None) else { unreachable!() };
            params.settings.serial_grain = false;
            Command::Render {
                input: input.clone(), source_generation: 1, film: film.into(), paper: "kodak_portra_endura".into(),
                params, limit_preview: false, preview_path: preview.clone(), interactive_scale, crop: None,
            }
        };
        let settled = |quick_first: bool| {
            let mut session = Session::default();
            session.handle(Request { id: 1, command: with_film("kodak_portra_400", None) }, &data).unwrap();
            if quick_first {
                let quick = session.handle(Request { id: 2, command: with_film("kodak_gold_200", Some(0.5)) }, &data).unwrap();
                assert_eq!(quick["output_width"].as_u64(), Some(48));
                assert_eq!(quick["export_resolution"].as_bool(), Some(false));
            }
            let response = session.handle(Request { id: 3, command: with_film("kodak_gold_200", None) }, &data).unwrap();
            assert_eq!(response["export_resolution"].as_bool(), Some(true));
            (session.output.unwrap().image.data, std::fs::read(&preview).unwrap())
        };
        let (direct, direct_display) = settled(false);
        let (after_quick, after_quick_display) = settled(true);
        assert!(direct == after_quick, "the settled render differs after a quick frame");
        assert!(direct_display == after_quick_display, "the display buffer differs after a quick frame");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Ticket 39: a photograph's first frame comes after a decode alone (the
    /// whole photograph) and a quick frame. Its settled render matches the
    /// one a fresh helper renders straight away, in the kept output and the
    /// display buffer, and the decode reports the size and RAW-ness the
    /// render does.
    #[test]
    fn a_first_frame_after_a_decode_alone_and_a_quick_frame_is_unchanged() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-decode-first-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, preview) = (dir.join("input.png"), dir.join("preview.rgba"));
        let pixels = (0..96 * 64 * 3).map(|i| ((i * 53) % 255) as spektrafilm_math::Scalar / 255.0).collect();
        spektrafilm_io::save_image(&ImageBuf::from_data(96, 64, pixels), &input, None).unwrap();
        let command = |interactive_scale: Option<f32>| {
            let Command::Render { mut params, .. } = render(&input, &preview, None, None, None) else { unreachable!() };
            params.settings.serial_grain = false;
            Command::Render {
                input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(), paper: "kodak_portra_endura".into(),
                params, limit_preview: false, preview_path: preview.clone(), interactive_scale, crop: None,
            }
        };
        let settled = |decode_first: bool| {
            let mut session = Session::default();
            if decode_first {
                let Command::Render { params, .. } = command(None) else { unreachable!() };
                let decoded = session.handle(Request { id: 1, command: Command::Thumbnail {
                    input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
                    paper: "kodak_portra_endura".into(), params, long_side: 16,
                    preview_path: dir.join("decode.rgba"), crop: None, preset: None, reduced_decode: false, decode_only: true,
                    preview_colour_space: false, read_negative: None,
                } }, &data).unwrap();
                assert_eq!((decoded["source_width"].as_u64(), decoded["source_height"].as_u64()), (Some(96), Some(64)));
                assert_eq!(decoded["raw"].as_bool(), Some(false));
                assert!(session.thumbnail_source.is_none(), "a decode alone shrinks nothing");
                session.handle(Request { id: 2, command: command(Some(0.5)) }, &data).unwrap();
            }
            let response = session.handle(Request { id: 3, command: command(None) }, &data).unwrap();
            assert_eq!(response["export_resolution"].as_bool(), Some(true));
            assert_eq!(response["raw"].as_bool(), Some(false));
            (session.output.unwrap().image.data, std::fs::read(&preview).unwrap(), response["film_histogram"].clone())
        };
        let (direct, direct_display, direct_histogram) = settled(false);
        let (after, after_display, after_histogram) = settled(true);
        assert!(direct == after, "the settled render differs after a decode alone");
        assert!(direct_display == after_display, "the display buffer differs after a decode alone");
        assert_eq!(direct_histogram, after_histogram);
        std::fs::remove_dir_all(dir).unwrap();
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
                flip: false,
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

    #[test]
    fn a_negative_meters_its_own_print_exposure_and_shows_its_film_density() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-negative-meter-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, preview) = (dir.join("negative.tif"), dir.join("preview.rgba"));
        let mut params = RuntimeParams::default();
        params.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
        // The file is linear sRGB, as it's read.
        params.io.input_color_space = "sRGB".into();
        params.io.input_cctf_decoding = false;
        let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
        let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
        let mut filming = params.clone();
        filming.film_render.grain.active = false;
        let pipeline = Pipeline::new_with_spectral(film, paper, filming, &data).unwrap();
        // A grey a stop over mid-grey on Portra 400, scanned, after a strip
        // of clear film: a negative whose exposure is known.
        let grey = (0.184 * 2.0) as spektrafilm_math::Scalar;
        let filmed = pipeline.film_scene(ImageBuf::from_data(2, 2, vec![grey; 12]));
        let cmy: [f64; 3] = std::array::from_fn(|c| filmed.data[c] as f64);
        let converter = pipeline.film_converter();
        let (w, h) = (64u32, 40u32);
        let (clear, picture) = (converter.forward([0.0; 3]), converter.forward(cmy));
        let pixels = (0..w * h).flat_map(|i| if i % w < 8 { clear } else { picture }).map(|v| v as spektrafilm_math::Scalar).collect();
        let options = spektrafilm_io::save::SaveOptions { linear: true, ..Default::default() };
        spektrafilm_io::save::save_image_with_options(&ImageBuf::from_data(w, h, pixels), &input, None, &options).unwrap();

        let mut session = Session::default();
        let mut request = |id, command| session.handle(Request { id, command }, &data);
        let meter = |bounds| Command::NegativeMeter {
            input: input.clone(), source_generation: 1, film: "kodak_portra_400".into(),
            paper: "kodak_portra_endura".into(), params: Box::new(params.clone()), bounds,
        };
        let over = request(1, meter(None)).unwrap();
        let exposure = over["print_exposure"].as_f64().unwrap();
        assert!(exposure > 1.3, "a negative a stop over prints longer: {over}");
        assert_eq!(request(2, meter(Some([0.3, 0.1, 0.9, 0.9]))).unwrap()["print_exposure"].as_f64(), Some(exposure));
        assert!(request(3, meter(Some([0.0, 0.0, 0.1, 1.0]))).unwrap_err().to_string().contains("no picture"));
        assert!(request(4, meter(Some([0.5, 0.0, 0.4, 1.0]))).is_err());

        let response = request(5, Command::Render {
            input: input.clone(), source_generation: 1, film: "kodak_portra_400".into(),
            paper: "kodak_portra_endura".into(), params: Box::new(params.clone()), limit_preview: false,
            preview_path: preview.clone(), interactive_scale: None, crop: None,
        })
        .unwrap();
        let histogram = &response["film_histogram"];
        assert_eq!(histogram["ev"].as_f64(), Some(0.0));
        let (lo, hi) = spektrafilm_core::tone_curve::STOPS;
        let over_bin = ((1.0 - lo) * 128.0 / (hi - lo)) as usize;
        for channel in histogram["counts"].as_array().unwrap() {
            let counts: Vec<u64> = channel.as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            assert_eq!(counts.iter().sum::<u64>(), ((w - 8) * h) as u64, "the clear film isn't counted");
            let peak = (0..counts.len()).max_by_key(|&i| counts[i]).unwrap();
            assert!(peak.abs_diff(over_bin) <= 1, "the film reads a stop over: bin {peak}, want {over_bin}");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// `negative_check` says whether a photograph printed as a scene looks
    /// like a colour negative, as `negative_base` would once it's read as
    /// one, from the source its render decoded: no second decode.
    #[test]
    fn negative_check_reads_the_rendered_source() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-negative-check-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (w, h) = (64u32, 40u32);
        let write = |name: &str, pixel: &dyn Fn(u32, u32) -> [f64; 3]| {
            let input = dir.join(name);
            let pixels = (0..w * h).flat_map(|i| pixel(i % w, i / w));
            let image = ImageBuf::from_data(w, h, pixels.map(|v| v as spektrafilm_math::Scalar).collect());
            let options = spektrafilm_io::save::SaveOptions { linear: true, ..Default::default() };
            spektrafilm_io::save::save_image_with_options(&image, &input, None, &options).unwrap();
            input
        };
        let negative = write("negative.tif", &|x, y| {
            if x < 8 { [0.706, 0.237, 0.144] } else { [0.25 + 0.002 * x as f64, 0.06 + 0.001 * y as f64, 0.05] }
        });
        let scene = write("scene.tif", &|x, y| if y < 15 { [0.15, 0.35, 0.8] } else if y < 28 { [0.9, 0.9, 0.85] } else { [0.08, 0.3 + 0.002 * x as f64, 0.05] });
        let mut params = RuntimeParams::default();
        params.io.input_color_space = "sRGB".into();
        params.io.input_cctf_decoding = false;
        params.film_render.grain.active = false;
        assert!(!params.workflow.converts_film());

        let mut session = Session::default();
        let check = |session: &mut Session, input: &Path, generation: u64| {
            let preview = dir.join("preview.rgba");
            session.handle(Request { id: 1, command: Command::Render {
                input: input.to_path_buf(), source_generation: generation, film: "kodak_portra_400".into(),
                paper: "kodak_portra_endura".into(), params: Box::new(params.clone()), limit_preview: false,
                preview_path: preview, interactive_scale: None, crop: None,
            } }, &data).unwrap();
            let loads = session.source_loads;
            let answer = session.handle(Request { id: 2, command: Command::NegativeCheck {
                input: input.to_path_buf(), source_generation: generation, film: "kodak_portra_400".into(),
                params: Box::new(params.clone()),
            } }, &data).unwrap();
            assert_eq!(session.source_loads, loads, "the rendered source is read again");
            answer["looks_negative"].as_bool().unwrap()
        };
        assert!(check(&mut session, &negative, 1), "the negative");
        assert!(!check(&mut session, &scene, 2), "the scene");
        // As negative_base says, read as a negative.
        let mut read = params.clone();
        read.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
        let found = session.handle(Request { id: 3, command: Command::NegativeBase {
            input: negative.clone(), source_generation: 3, film: "kodak_portra_400".into(), paper: "kodak_portra_endura".into(),
            params: Box::new(read), point: None,
        } }, &data).unwrap();
        assert_eq!(found["looks_negative"].as_bool(), Some(true));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A read-as tile finds the base and balances the print for its film,
    /// as choosing that film does: the same picture as a tile of the
    /// detection's answer.
    #[test]
    fn a_read_as_tile_reads_the_negative_for_its_film() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-read-as-tile-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("negative.tif");
        let (w, h) = (64u32, 40u32);
        let pixels = (0..w * h).flat_map(|i| {
            let (x, y) = (i % w, i / w);
            if x < 8 { [0.706, 0.237, 0.144] } else { [0.25 + 0.002 * x as f64, 0.06 + 0.001 * y as f64, 0.05] }
        });
        let image = ImageBuf::from_data(w, h, pixels.map(|v| v as spektrafilm_math::Scalar).collect());
        let options = spektrafilm_io::save::SaveOptions { linear: true, ..Default::default() };
        spektrafilm_io::save::save_image_with_options(&image, &input, None, &options).unwrap();
        let mut params = RuntimeParams::default();
        params.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
        params.io.input_color_space = "sRGB".into();
        params.io.input_cctf_decoding = false;
        params.film_render.grain.active = false;

        let mut session = Session::default();
        let film = "kodak_gold_200";
        let found = session.handle(Request { id: 1, command: Command::NegativeBase {
            input: input.clone(), source_generation: 1, film: film.into(), paper: "kodak_portra_endura".into(),
            params: Box::new(params.clone()), point: None,
        } }, &data).unwrap();
        let mut chosen = params.clone();
        chosen.film_render.base = serde_json::from_value(found["base"].clone()).unwrap();
        chosen.film_render.convert.exposure_compensation_ev = found["exposure_ev"].as_f64().unwrap();
        chosen.enlarger.m_filter_shift = found["m_filter_shift"].as_f64().unwrap() as f32;
        chosen.enlarger.y_filter_shift = found["y_filter_shift"].as_f64().unwrap() as f32;
        let mut tile = |id, params: &RuntimeParams, read_negative, name: &str| {
            let preview = dir.join(name);
            session.handle(Request { id, command: Command::Thumbnail {
                input: input.clone(), source_generation: 1, film: film.into(), paper: "kodak_portra_endura".into(),
                params: Box::new(params.clone()), long_side: 32, preview_path: preview.clone(), crop: None, preset: None,
                reduced_decode: false, decode_only: false, preview_colour_space: false, read_negative,
            } }, &data).unwrap();
            std::fs::read(preview).unwrap()
        };
        let read = tile(2, &params, Some(ReadNegative { point: None }), "read.rgba");
        assert!(read == tile(3, &chosen, None, "chosen.rgba"), "the tile shows the film as choosing it reads it");
        assert!(read != tile(4, &params, None, "stale.rgba"), "not on the other film's base");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A film's tile on a negative divided by a flat field (the film printed
    /// as a scene, so not divided) decodes a copy of its own: the main
    /// render keeps its flattened source. A light frame replaced at the
    /// same path is read again.
    #[test]
    fn a_flat_field_source_stays_through_film_tiles_and_follows_its_light_frame() {
        let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let dir = std::env::temp_dir().join(format!("negs-flat-source-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (input, light, preview) = (dir.join("negative.tif"), dir.join("light.tif"), dir.join("preview.rgba"));
        let (w, h) = (48u32, 32u32);
        let save = |path: &Path, f: &dyn Fn(u32, u32) -> f64| {
            let pixels = (0..w * h).flat_map(|i| {
                let v = f(i % w, i / w);
                [v * 0.7, v * 0.25, v * 0.15]
            });
            let image = ImageBuf::from_data(w, h, pixels.map(|v| v as spektrafilm_math::Scalar).collect());
            let options = spektrafilm_io::save::SaveOptions { linear: true, ..Default::default() };
            spektrafilm_io::save::save_image_with_options(&image, path, None, &options).unwrap();
        };
        save(&input, &|x, _| 0.4 + 0.4 * x as f64 / w as f64);
        save(&light, &|x, _| 1.0 - 0.3 * (x as f64 / w as f64 - 0.5).abs());
        let mut params = RuntimeParams::default();
        params.workflow.route = spektrafilm_core::params::ROUTE_CONVERT_PRINT.into();
        params.io.input_color_space = "sRGB".into();
        params.io.input_cctf_decoding = false;
        params.film_render.convert.flat_field = light.to_string_lossy().into_owned();
        let mut scene = params.clone();
        scene.workflow = Default::default();
        scene.film_render.convert = Default::default();

        let mut session = Session::default();
        let request = |session: &mut Session, id, command| session.handle(Request { id, command }, &data).unwrap();
        let render = |params: &RuntimeParams| Command::Render {
            input: input.clone(), source_generation: 1, film: "kodak_portra_400".into(),
            paper: "kodak_portra_endura".into(), params: Box::new(params.clone()), limit_preview: false,
            preview_path: preview.clone(), interactive_scale: None, crop: None,
        };
        let tile = |params: &RuntimeParams| Command::Thumbnail {
            input: input.clone(), source_generation: 1, film: "kodak_gold_200".into(),
            paper: "kodak_portra_endura".into(), params: Box::new(params.clone()), long_side: 32,
            preview_path: dir.join("tile.rgba"), crop: None, preset: None, reduced_decode: false, decode_only: false,
            preview_colour_space: false, read_negative: None,
        };
        assert_eq!(request(&mut session, 1, render(&params))["flat_field"]["used"], true);
        request(&mut session, 2, tile(&scene));
        request(&mut session, 3, tile(&params));
        request(&mut session, 4, render(&params));
        assert_eq!(session.source_loads, 1, "the film's tile didn't replace the flattened source");
        let (scene_tile, negative_tile) = (session.thumbnail_wb_sources.len(), session.thumbnail_source.is_some());
        assert_eq!((scene_tile, negative_tile), (1, true));

        // The same path, another light frame (another size on disk too).
        std::thread::sleep(std::time::Duration::from_millis(20));
        let pixels = (0..w * h * 3).map(|_| 0.9 as spektrafilm_math::Scalar).collect();
        let options = spektrafilm_io::save::SaveOptions { linear: false, ..Default::default() };
        spektrafilm_io::save::save_image_with_options(&ImageBuf::from_data(w, h, pixels), &light, None, &options).unwrap();
        request(&mut session, 5, render(&params));
        assert_eq!(session.source_loads, 2, "a new light frame is read again");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
