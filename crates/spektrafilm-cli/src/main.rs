use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use spektrafilm_core::params::RuntimeParams;
use spektrafilm_core::pipeline::Pipeline;
use spektrafilm_core::profile;
use spektrafilm_io::resize::Fit;
use spektrafilm_math::colorspace::RgbColourspace;

#[derive(Parser)]
#[command(
    name = "spektrafilm",
    about = "Physically-based spectral film emulation"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Process an image through the film simulation pipeline.
    Process {
        /// Input image (TIFF, PNG, or camera RAW).
        input: PathBuf,
        /// Output image path.
        #[arg(short, long)]
        output: PathBuf,
        /// JPEG encoder quality; PNG/TIFF remain lossless.
        #[arg(long, default_value_t = 95, value_parser = clap::value_parser!(u8).range(1..=100))]
        jpeg_quality: u8,
        /// Bits per channel: 8 or 16. Defaults to 16 for TIFF and 8 for PNG;
        /// JPEG is always 8-bit.
        #[arg(long, value_parser = clap::builder::TypedValueParser::map(
            clap::builder::PossibleValuesParser::new(["8", "16"]),
            |s: String| s.parse::<u8>().unwrap(),
        ))]
        bit_depth: Option<u8>,
        /// Output colour space, as colour names it (e.g. sRGB, "Display P3",
        /// "Adobe RGB (1998)", "ProPhoto RGB"). Overrides
        /// io.output_color_space in --params; the file carries its ICC profile.
        #[arg(long)]
        color_space: Option<String>,
        /// How to read a non-RAW input's values (overrides
        /// io.input_interpretation in --params): "auto" (its ICC profile and
        /// metadata, else sRGB), "manual" (0.8: io.input_color_space and
        /// io.input_cctf_decoding), or a colour space to assume: sRGB,
        /// "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020".
        #[arg(long)]
        input_colour: Option<String>,
        /// With --input-colour SPACE: the values are linear light.
        #[arg(long, requires = "input_colour")]
        input_linear: bool,
        /// Omit the source photograph's camera metadata.
        #[arg(long)]
        no_metadata: bool,
        /// Film stock name (e.g. kodak_portra_400).
        #[arg(long)]
        film: String,
        /// Paper stock name (e.g. fujifilm_crystal_archive_typeii).
        /// If omitted and --scan-film is not set, uses the film's target_print.
        #[arg(long)]
        paper: Option<String>,
        /// Scan film directly (skip printing stage).
        #[arg(long)]
        scan_film: bool,
        /// Print per-stage timing information.
        #[arg(long)]
        timings: bool,
        /// Path to JSON params file for overrides. `io.photo_long_side_px`
        /// enlarges or reduces the source before the film so the cropped
        /// photograph (see --crop) has this many pixels on its longest side.
        #[arg(long)]
        params: Option<PathBuf>,
        /// RAW input WB (overrides io.raw_white_balance.mode in --params).
        #[arg(long, value_parser = ["as_shot", "daylight", "tungsten", "custom"])]
        raw_wb: Option<String>,
        /// Custom RAW WB temperature in kelvin (1667..25000; default 5500).
        #[arg(long, requires = "raw_wb")]
        raw_temperature: Option<f64>,
        /// Custom RAW WB green multiplier (>0..4; default 1).
        #[arg(long, requires = "raw_wb")]
        raw_tint: Option<f64>,
        /// Dump the raw f64 output buffer (HxWx3, row-major, channel-interleaved)
        /// before sRGB encoding/clipping. Used for bit-exact parity comparison.
        #[arg(long)]
        raw_out: Option<PathBuf>,
        /// Crop the result to x,y,width,height[,angle]: fractions of the
        /// unrotated image with the origin top left (before rotation), then
        /// degrees to straighten it clockwise.
        #[arg(long)]
        crop: Option<spektrafilm_io::crop::Crop>,
        /// Add a border around the result (after rotation and crop).
        #[arg(long, value_parser = ["solid", "white", "film", "keyline", "35mm"])]
        border: Option<String>,
        /// The solid or keyline mat, or the black around the film frame, as a
        /// fraction of the long edge. Default: 0.03 for a mat, 0 for film.
        #[arg(long, requires = "border")]
        border_width: Option<f64>,
        /// The mat's colour, #rrggbb. Default: white.
        #[arg(long, requires = "border")]
        border_color: Option<spektrafilm_io::border::Rgb>,
        /// The keyline's line, as a fraction of the long edge. Default: 0.004.
        #[arg(long, requires = "border")]
        keyline_width: Option<f64>,
        /// The keyline's colour, #rrggbb. Default: black.
        #[arg(long, requires = "border")]
        keyline_color: Option<spektrafilm_io::border::Rgb>,
        /// Extend the border to Instagram's 3:4 portrait, with the mat's
        /// colour around a solid or keyline border and white around film.
        #[arg(long, requires = "border")]
        instagram: bool,
        /// Resample the result (after the border) to this many pixels on
        /// its long edge. Never enlarges.
        #[arg(long, conflicts_with = "short_edge")]
        long_edge: Option<u32>,
        /// Resample the result (after the border) to this many pixels on
        /// its short edge. Never enlarges.
        #[arg(long)]
        short_edge: Option<u32>,
        /// Run the pipeline N times in the same process. Each iteration is
        /// timed individually so you can see cold-start vs warm-cache speed.
        /// Default: 1 (no repetition).
        #[arg(long, default_value = "1")]
        iters: usize,
        /// Path to the data directory.
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },
    /// List available film and paper profiles.
    ListProfiles {
        /// Path to the data directory.
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },
    /// Export a 3D CUBE LUT for use in other software.
    ExportLut {
        /// Film stock name.
        #[arg(long)]
        film: String,
        /// Paper stock name.
        #[arg(long)]
        paper: Option<String>,
        /// LUT cube size (e.g. 33, 65).
        #[arg(long, default_value = "33")]
        size: u32,
        /// Path to JSON params file for the film/paper look (Post edits excluded).
        #[arg(long)]
        params: Option<PathBuf>,
        /// Output .cube file path.
        #[arg(short, long)]
        output: PathBuf,
        /// Path to the data directory.
        #[arg(long, default_value = "data")]
        data_dir: PathBuf,
    },
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Process {
            input,
            output,
            jpeg_quality,
            bit_depth,
            color_space,
            input_colour,
            input_linear,
            no_metadata,
            film,
            paper,
            scan_film,
            timings,
            params: params_file,
            raw_wb,
            raw_temperature,
            raw_tint,
            raw_out,
            crop,
            border,
            border_width,
            border_color,
            keyline_width,
            keyline_color,
            instagram,
            long_edge,
            short_edge,
            iters,
            data_dir,
        } => {
            let border = border.as_deref().map(str::parse).transpose()?;
            let input_interpretation = input_colour
                .as_deref()
                .map(|spec| parse_input_colour(spec, input_linear))
                .transpose()?;
            let fit = long_edge
                .map(Fit::LongEdge)
                .or(short_edge.map(Fit::ShortEdge));
            let defaults = spektrafilm_io::border::BorderOptions::default();
            let border_options = spektrafilm_io::border::BorderOptions {
                width: border_width,
                line: keyline_width.unwrap_or(defaults.line),
                color: border_color.unwrap_or(defaults.color),
                line_color: keyline_color.unwrap_or(defaults.line_color),
            };
            cmd_process(
                &input,
                &output,
                spektrafilm_io::save::SaveOptions {
                    jpeg_quality,
                    copy_metadata: !no_metadata,
                    bit_depth,
                    ..Default::default()
                },
                color_space.as_deref(),
                input_interpretation,
                &film,
                paper.as_deref(),
                scan_film,
                timings,
                params_file.as_deref(),
                raw_wb.as_deref(),
                raw_temperature,
                raw_tint,
                raw_out.as_deref(),
                crop,
                border,
                &border_options,
                instagram,
                fit,
                iters,
                &data_dir,
            )?;
        }
        Commands::ListProfiles { data_dir } => {
            cmd_list_profiles(&data_dir);
        }
        Commands::ExportLut {
            film,
            paper,
            size,
            params,
            output,
            data_dir,
        } => {
            cmd_export_lut(&film, paper.as_deref(), size, params.as_deref(), &output, &data_dir)?;
        }
    }

    Ok(())
}

/// A params file. One from before `io.input_interpretation` keeps the
/// meaning of its input fields; see `infer_input_interpretation`.
fn read_params(path: &Path) -> Result<RuntimeParams> {
    let text = std::fs::read_to_string(path).with_context(|| format!("opening params file: {}", path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).context("parsing params file")?;
    spektrafilm_core::params::infer_input_interpretation(&mut value);
    serde_json::from_value(value).context("parsing params file")
}

/// `--input-colour`: auto, manual, or a colour space to assume.
fn parse_input_colour(spec: &str, linear: bool) -> Result<spektrafilm_io::InputInterpretation> {
    use spektrafilm_io::InputInterpretation;
    let interpretation = match spec.to_ascii_lowercase().as_str() {
        "auto" | "manual" if linear => bail!("--input-linear needs a colour space, not {spec}"),
        "auto" => InputInterpretation::default(),
        "manual" => InputInterpretation::manual(),
        _ => {
            let space = RgbColourspace::find(spec)
                .filter(|cs| spektrafilm_io::colour::ASSUMABLE.contains(&cs.name))
                .with_context(|| {
                    format!("--input-colour: use auto, manual or one of {}", spektrafilm_io::colour::ASSUMABLE.join(", "))
                })?;
            InputInterpretation::assume(space.name, linear)
        }
    };
    Ok(interpretation)
}

#[allow(clippy::too_many_arguments)]
fn cmd_process(
    input: &Path,
    output: &Path,
    mut save_options: spektrafilm_io::save::SaveOptions,
    color_space: Option<&str>,
    input_interpretation: Option<spektrafilm_io::InputInterpretation>,
    film_name: &str,
    paper_name: Option<&str>,
    scan_film: bool,
    show_timings: bool,
    params_file: Option<&Path>,
    raw_wb: Option<&str>,
    raw_temperature: Option<f64>,
    raw_tint: Option<f64>,
    raw_out: Option<&Path>,
    crop: Option<spektrafilm_io::crop::Crop>,
    border: Option<spektrafilm_io::border::BorderStyle>,
    border_options: &spektrafilm_io::border::BorderOptions,
    instagram: bool,
    fit: Option<Fit>,
    iters: usize,
    data_dir: &Path,
) -> Result<()> {
    let total_start = Instant::now();

    // Load profiles
    let t = Instant::now();
    let film = profile::load_profile_by_name(data_dir, film_name)
        .with_context(|| format!("loading film profile: {film_name}"))?;

    let print_stock = if scan_film {
        film_name.to_string()
    } else if let Some(p) = paper_name {
        p.to_string()
    } else if let Some(ref target) = film.info.target_print {
        target.clone()
    } else {
        bail!("no paper specified and film has no target_print — use --paper or --scan-film");
    };

    let print = profile::load_profile_by_name(data_dir, &print_stock)
        .with_context(|| format!("loading print profile: {print_stock}"))?;
    eprintln!("Profiles loaded: {} ms", t.elapsed().as_millis());

    // Load params (defaults + optional overrides)
    let mut params = if let Some(pf) = params_file {
        read_params(pf)?
    } else {
        RuntimeParams::default()
    };
    params.io.scan_film = scan_film;
    if let Some(interpretation) = input_interpretation {
        params.io.input_interpretation = interpretation;
    }
    params.io.input_interpretation.validate()?;
    apply_raw_wb_overrides(
        &mut params.io.raw_white_balance,
        raw_wb,
        raw_temperature,
        raw_tint,
    )?;
    if let Some(name) = color_space {
        let space = RgbColourspace::find(name)
            .with_context(|| format!("unknown colour space: {name}"))?;
        params.io.output_color_space = space.name.into();
    }
    let rotate_cw = params.io.rotate_cw;
    let encoded = params.io.output_cctf_encoding;
    // Borders, the 35mm strip and resizing work in the output space.
    let finish = spektrafilm_io::encoding::Encoding::new(&params.io.output_color_space, encoded);
    // The pipeline falls back to sRGB for a name it doesn't know; so does the profile.
    save_options.color_space = RgbColourspace::output(&params.io.output_color_space).name.into();
    save_options.linear = !encoded;
    save_options.check(output)?;

    // Load image
    let t = Instant::now();
    let loaded = spektrafilm_io::load_image_colour(
        input,
        spektrafilm_io::Formats::Any,
        params.io.raw_white_balance,
        &params.io.input_interpretation,
    )?;
    eprintln!("Input colour: {}", loaded.colour.summary);
    if let Some(warning) = &loaded.colour.warning {
        eprintln!("Warning: {warning}");
    }
    params.io.apply_input_colour(&loaded.colour);
    let image = loaded.image;
    eprintln!(
        "Image loaded: {}x{} ({} MP), {} ms",
        image.width,
        image.height,
        (image.pixel_count() as f64 / 1e6 * 10.0).round() / 10.0,
        t.elapsed().as_millis()
    );

    // Auto exposure meters the full source, before any resize, as the native
    // preview does, so an export has the preview's exposure.
    let metering = params
        .camera
        .auto_exposure
        .then(|| spektrafilm_core::stages::filming::metering_preview(&image));

    // Image Size: resize the source before the film, so grain and halation
    // form at the working size. A Longest side target in the params describes
    // the cropped photograph, so it is resolved with --crop; the crop itself
    // still applies after the pipeline. The params then ask for no resize.
    let t = Instant::now();
    let (image, size) = spektrafilm_core::image_size::prepare(image, &mut params, crop.as_ref())
        .context("image size")?;
    if size.resizes() {
        eprintln!(
            "Working size: {}x{} (photograph {}x{}), {} ms",
            size.working.0,
            size.working.1,
            size.photo.0,
            size.photo.1,
            t.elapsed().as_millis()
        );
    }

    // Select backend
    let backend = spektrafilm_gpu::select_backend();

    // Run pipeline (with full Hanatos2025 spectral upsampling)
    let t = Instant::now();
    // A missing LUT or invalid configuration must fail the export. Falling
    // back used to silently discard the user's parameters and spectral model.
    let mut pipeline = Pipeline::new_with_spectral(film, print, params, data_dir)
        .map_err(anyhow::Error::msg)
        .context("building spectral pipeline")?;
    if let Some(metering) = metering {
        pipeline = pipeline.with_metering_preview(metering);
    }
    let (result, used_backend) = if iters > 1 {
        // Warm-cache bench: process the image `iters` times in the same backend
        // instance. The first iteration pays shader-compile cost; subsequent ones
        // hit the cache and reflect "GUI live preview" performance.
        let mut last = pipeline.process_with_backend(image.clone(), backend.as_ref());
        let iter1_ms = t.elapsed().as_millis();
        eprintln!("Pipeline (iter 1): {} ms (cold)", iter1_ms);
        for i in 2..=iters {
            let ti = Instant::now();
            last = pipeline.process_with_backend(image.clone(), backend.as_ref());
            eprintln!(
                "Pipeline (iter {}): {} ms (warm)",
                i,
                ti.elapsed().as_millis()
            );
        }
        last
    } else {
        let r = pipeline.process_with_backend(image, backend.as_ref());
        eprintln!("Pipeline: {} ms", t.elapsed().as_millis());
        r
    };
    eprintln!("Backend: {used_backend}");

    // The scan is unclipped, like Python's. Crop, border and resize work on the
    // photograph as the file will hold it, so clip first when one applies;
    // otherwise `--raw-out` keeps the unclipped values and the writer clips.
    let mut result = result;
    if crop.is_some() || border.is_some() || fit.is_some() {
        spektrafilm_core::pipeline::clip_to_display(&mut result, encoded);
    }

    // User crop and rotation from the GUI (post-pipeline, matches the preview).
    let result = match crop {
        Some(crop) => crop.apply(&result),
        None => result,
    };
    let result = result.rotated_cw(rotate_cw);
    let result = match border {
        Some(style) => {
            let bordered = spektrafilm_io::border::add_border(
                &result,
                style,
                border_options,
                film_name,
                finish,
                data_dir,
            )?;
            if instagram {
                spektrafilm_io::border::extend_for_instagram(
                    &bordered,
                    style,
                    border_options,
                    finish,
                )
            } else {
                bordered
            }
        }
        None => result,
    };
    let result = match fit.and_then(|fit| spektrafilm_io::resize::fit(&result, fit, finish)) {
        Some(resized) => resized,
        None => result,
    };

    // Save output, carrying over the input's camera metadata (EXIF).
    let t = Instant::now();
    let metadata = spektrafilm_io::read_metadata(input);
    spektrafilm_io::save::save_image_with_options(&result, output, metadata.as_ref(), &save_options)?;
    eprintln!(
        "Saved: {} ({}x{}), {} ms",
        output.display(),
        result.width,
        result.height,
        t.elapsed().as_millis()
    );

    // Optional raw f64 dump for bit-exact parity comparison.
    if let Some(p) = raw_out {
        use std::io::Write;
        let buf: Vec<f64> = result.data.iter().map(|&v| v as f64).collect();
        let bytes = bytemuck_cast_f64_to_bytes(&buf);
        let mut f = std::fs::File::create(p)
            .with_context(|| format!("creating raw_out file: {}", p.display()))?;
        f.write_all(bytes)?;
        eprintln!("raw f64 buffer: {} ({} values)", p.display(), buf.len());
    }

    if show_timings {
        eprintln!("Total: {} ms", total_start.elapsed().as_millis());
    }

    Ok(())
}

fn bytemuck_cast_f64_to_bytes(v: &[f64]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
}

fn apply_raw_wb_overrides(
    wb: &mut spektrafilm_io::RawWhiteBalance,
    mode: Option<&str>,
    temperature: Option<f64>,
    tint: Option<f64>,
) -> Result<()> {
    use spektrafilm_io::RawWhiteBalanceMode;
    if let Some(mode) = mode {
        wb.mode = match mode {
            "as_shot" => RawWhiteBalanceMode::AsShot,
            "daylight" => RawWhiteBalanceMode::Daylight,
            "tungsten" => RawWhiteBalanceMode::Tungsten,
            "custom" => RawWhiteBalanceMode::Custom,
            _ => bail!("unknown RAW WB mode: {mode}"),
        };
    }
    if (temperature.is_some() || tint.is_some()) && wb.mode != RawWhiteBalanceMode::Custom {
        bail!("--raw-temperature and --raw-tint require --raw-wb custom");
    }
    if let Some(t) = temperature {
        wb.temperature = t;
    }
    if let Some(t) = tint {
        wb.tint = t;
    }
    wb.validate()
}

fn cmd_list_profiles(data_dir: &Path) {
    let profiles_dir = data_dir.join("profiles");
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&profiles_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy().to_string();
            if name.ends_with(".json") {
                names.push(name.trim_end_matches(".json").to_string());
            }
        }
    } else {
        eprintln!("Profile directory not found: {}", profiles_dir.display());
        return;
    }
    names.sort();
    for name in &names {
        // Try to load and show the human-readable name
        if let Ok(p) = profile::load_profile_by_name(data_dir, name) {
            let display_name = p.info.name.as_deref().unwrap_or(name);
            let film_type = &p.info.film_type;
            let support = &p.info.support;
            println!("{name:<40} {display_name:<30} ({film_type}, {support})");
        } else {
            println!("{name}");
        }
    }
}

fn cmd_export_lut(
    film_name: &str,
    paper_name: Option<&str>,
    size: u32,
    params_file: Option<&Path>,
    output: &Path,
    data_dir: &Path,
) -> Result<()> {
    let params: RuntimeParams = match params_file {
        Some(path) => serde_json::from_str(&std::fs::read_to_string(path)
            .with_context(|| format!("reading params: {}", path.display()))?)?,
        None => RuntimeParams::default(),
    };
    let film = profile::load_profile_by_name(data_dir, film_name)?;
    let paper = if params.io.scan_film {
        film_name
    } else {
        paper_name.or(film.info.target_print.as_deref())
            .context("no paper specified and film has no target_print")?
    };
    let cube = spektrafilm_core::lut_export::cube(data_dir, film_name, paper, params, size)
        .map_err(anyhow::Error::msg)?;
    std::fs::write(output, cube)
        .with_context(|| format!("writing CUBE file: {}", output.display()))?;
    eprintln!("LUT saved: {}", output.display());
    Ok(())
}

#[cfg(test)]
mod raw_wb_tests {
    use super::*;
    use spektrafilm_io::{RawWhiteBalance, RawWhiteBalanceMode};

    #[test]
    fn old_params_default_to_as_shot_and_export_json_preserves_selection() {
        for json in ["{}", r#"{"io":{}}"#] {
            let p: RuntimeParams = serde_json::from_str(json).unwrap();
            assert_eq!(p.io.raw_white_balance, RawWhiteBalance::default());
        }
        let mut p: RuntimeParams = serde_json::from_str(r#"{"io":{"raw_white_balance":{"mode":"custom","temperature":4200,"tint":0.9}},"scanner":{"white_correction":true}}"#).unwrap();
        let selected = p.io.raw_white_balance;
        apply_raw_wb_overrides(&mut p.io.raw_white_balance, None, None, None).unwrap();
        assert_eq!(p.io.raw_white_balance, selected);
        assert!(p.scanner.white_correction);
        apply_raw_wb_overrides(&mut p.io.raw_white_balance, Some("daylight"), None, None).unwrap();
        assert_eq!(p.io.raw_white_balance.mode, RawWhiteBalanceMode::Daylight);
        assert_eq!(p.io.raw_white_balance.temperature, 4200.0);
        assert!(p.scanner.white_correction);
    }

    #[test]
    fn cli_parses_custom_wb_and_rejects_invalid_combinations() {
        let base = [
            "spektrafilm",
            "process",
            "input.NEF",
            "-o",
            "out.png",
            "--film",
            "kodak_gold_200",
        ];
        let cli = Cli::try_parse_from(base.into_iter().chain([
            "--raw-wb",
            "custom",
            "--raw-temperature",
            "3200",
            "--raw-tint",
            "0.85",
        ]))
        .unwrap();
        let Commands::Process {
            raw_wb,
            raw_temperature,
            raw_tint,
            ..
        } = cli.command
        else {
            panic!()
        };
        let mut wb = RawWhiteBalance::default();
        apply_raw_wb_overrides(&mut wb, raw_wb.as_deref(), raw_temperature, raw_tint).unwrap();
        assert_eq!(
            wb,
            RawWhiteBalance {
                mode: RawWhiteBalanceMode::Custom,
                temperature: 3200.0,
                tint: 0.85
            }
        );
        assert!(Cli::try_parse_from(base.into_iter().chain(["--raw-wb", "auto"])).is_err());
        assert!(Cli::try_parse_from(base.into_iter().chain(["--raw-tint", "0.8"])).is_err());
        assert!(apply_raw_wb_overrides(&mut wb, Some("daylight"), Some(3200.0), None).is_err());
        assert!(apply_raw_wb_overrides(&mut wb, Some("custom"), Some(0.0), None).is_err());
    }
}
