use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use spektrafilm_core::params::RuntimeParams;
use spektrafilm_core::pipeline::Pipeline;
use spektrafilm_core::profile;
use spektrafilm_math::image::ImageBuf;

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
        /// Path to JSON params file for overrides.
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
        /// Add a border around the result (after rotation).
        #[arg(long, value_parser = ["white", "film", "keyline", "35mm"])]
        border: Option<String>,
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
            film,
            paper,
            scan_film,
            timings,
            params: params_file,
            raw_wb,
            raw_temperature,
            raw_tint,
            raw_out,
            border,
            iters,
            data_dir,
        } => {
            let border = border.as_deref().map(str::parse).transpose()?;
            cmd_process(
                &input,
                &output,
                &film,
                paper.as_deref(),
                scan_film,
                timings,
                params_file.as_deref(),
                raw_wb.as_deref(),
                raw_temperature,
                raw_tint,
                raw_out.as_deref(),
                border,
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
            output,
            data_dir,
        } => {
            cmd_export_lut(&film, paper.as_deref(), size, &output, &data_dir)?;
        }
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn cmd_process(
    input: &Path,
    output: &Path,
    film_name: &str,
    paper_name: Option<&str>,
    scan_film: bool,
    show_timings: bool,
    params_file: Option<&Path>,
    raw_wb: Option<&str>,
    raw_temperature: Option<f64>,
    raw_tint: Option<f64>,
    raw_out: Option<&Path>,
    border: Option<spektrafilm_io::border::BorderStyle>,
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
        let f = std::fs::File::open(pf)
            .with_context(|| format!("opening params file: {}", pf.display()))?;
        serde_json::from_reader(std::io::BufReader::new(f))
            .with_context(|| "parsing params file")?
    } else {
        RuntimeParams::default()
    };
    params.io.scan_film = scan_film;
    apply_raw_wb_overrides(
        &mut params.io.raw_white_balance,
        raw_wb,
        raw_temperature,
        raw_tint,
    )?;
    let rotate_cw = params.io.rotate_cw;
    let encoded = params.io.output_cctf_encoding;

    // Auto-detect input color space from file extension
    if spektrafilm_io::input_is_linear_srgb(input) {
        // PNG and RAW come out of the loader as linear sRGB.
        // Tell the pipeline so it doesn't decode gamma a second time.
        params.io.input_color_space = "sRGB".to_string();
        params.io.input_cctf_decoding = false;
    }

    // Load image
    let t = Instant::now();
    let image = spektrafilm_io::load_image_with_wb(
        input,
        spektrafilm_io::Formats::Any,
        params.io.raw_white_balance,
    )?;
    eprintln!(
        "Image loaded: {}x{} ({} MP), {} ms",
        image.width,
        image.height,
        (image.pixel_count() as f64 / 1e6 * 10.0).round() / 10.0,
        t.elapsed().as_millis()
    );

    // Select backend
    let backend = spektrafilm_gpu::select_backend();
    eprintln!("Backend: {}", backend.name());

    // Run pipeline (with full Hanatos2025 spectral upsampling)
    let t = Instant::now();
    // A missing LUT or invalid configuration must fail the export. Falling
    // back used to silently discard the user's parameters and spectral model.
    let pipeline = Pipeline::new_with_spectral(film, print, params, data_dir)
        .map_err(anyhow::Error::msg)
        .context("building spectral pipeline")?;
    let result = if iters > 1 {
        // Warm-cache bench: process the image `iters` times in the same backend
        // instance. The first iteration pays shader-compile cost; subsequent ones
        // hit the cache and reflect "GUI live preview" performance.
        let mut last = pipeline.process(image.clone(), backend.as_ref());
        let iter1_ms = t.elapsed().as_millis();
        eprintln!("Pipeline (iter 1): {} ms (cold)", iter1_ms);
        for i in 2..=iters {
            let ti = Instant::now();
            last = pipeline.process(image.clone(), backend.as_ref());
            eprintln!(
                "Pipeline (iter {}): {} ms (warm)",
                i,
                ti.elapsed().as_millis()
            );
        }
        last
    } else {
        let r = pipeline.process(image, backend.as_ref());
        eprintln!("Pipeline: {} ms", t.elapsed().as_millis());
        r
    };

    // User rotation from the GUI (post-pipeline, matches the preview).
    let result = result.rotated_cw(rotate_cw);
    let result = match border {
        Some(style) => {
            spektrafilm_io::border::add_border(&result, style, film_name, encoded, data_dir)?
        }
        None => result,
    };

    // Save output, carrying over the input's camera metadata (EXIF).
    let t = Instant::now();
    let metadata = spektrafilm_io::read_metadata(input);
    spektrafilm_io::save_image(&result, output, metadata.as_ref())?;
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
    output: &Path,
    data_dir: &Path,
) -> Result<()> {
    let film = profile::load_profile_by_name(data_dir, film_name)
        .with_context(|| format!("loading film profile: {film_name}"))?;

    let print_stock = if let Some(p) = paper_name {
        p.to_string()
    } else if let Some(ref target) = film.info.target_print {
        target.clone()
    } else {
        bail!("no paper specified and film has no target_print");
    };
    let print = profile::load_profile_by_name(data_dir, &print_stock)
        .with_context(|| format!("loading print profile: {print_stock}"))?;

    let mut params = RuntimeParams::default();
    params.film_render.grain.active = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.camera.auto_exposure = false;

    let backend = spektrafilm_gpu::select_backend();
    let pipeline = Pipeline::new(film, print, params);

    // Generate LUT: sample the pipeline on a uniform grid
    let s = size as usize;
    let mut cube_data = String::new();
    cube_data.push_str(&format!("# Created by spektrafilm-rs\n"));
    cube_data.push_str(&format!(
        "TITLE \"spektrafilm {film_name} → {print_stock}\"\n"
    ));
    cube_data.push_str(&format!("LUT_3D_SIZE {size}\n"));
    cube_data.push_str("\n");

    eprintln!("Generating {s}x{s}x{s} CUBE LUT...");

    for bi in 0..s {
        for gi in 0..s {
            for ri in 0..s {
                let r = ri as f32 / (s - 1) as f32;
                let g = gi as f32 / (s - 1) as f32;
                let b = bi as f32 / (s - 1) as f32;

                let img = ImageBuf::from_data(
                    1,
                    1,
                    vec![
                        spektrafilm_math::precision::from_f32(r),
                        spektrafilm_math::precision::from_f32(g),
                        spektrafilm_math::precision::from_f32(b),
                    ],
                );
                let out = pipeline.process(img, backend.as_ref());
                let px = out.get(0, 0);
                cube_data.push_str(&format!("{:.6} {:.6} {:.6}\n", px[0], px[1], px[2]));
            }
        }
    }

    std::fs::write(output, &cube_data)
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
