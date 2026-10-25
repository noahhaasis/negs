//! Image Size resizes the source before the film: grain and halation form at
//! the working size, and physical (µm) effects keep their size on the film.
use spektrafilm_core::{image_size, params::RuntimeParams, pipeline::Pipeline, profile, stages::filming};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_io::{crop::Crop, resize};
use spektrafilm_math::{image::ImageBuf, precision::{from_f64, to_f64}};
use std::path::{Path, PathBuf};

fn data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn pipeline(params: RuntimeParams) -> Pipeline {
    let dir = data();
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
        params,
        &dir,
    )
    .unwrap()
}

fn linear_params() -> RuntimeParams {
    let mut params = RuntimeParams::default();
    params.io.input_color_space = "sRGB".into();
    params.io.input_cctf_decoding = false;
    params
}

/// A textured scene with a highlight well above 1, as RAW files have.
fn scene(width: u32, height: u32) -> ImageBuf {
    let mut image = ImageBuf::new(width, height);
    for y in 0..height {
        for x in 0..width {
            let v = 0.05 + 0.4 * ((x * 7 + y * 13) % 17) as f64 / 17.0;
            image.set(x, y, [from_f64(v), from_f64(v * 0.8), from_f64(v * 0.6)]);
        }
    }
    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        image.set(width / 3 + dx, height / 2 + dy, [from_f64(8.0); 3]);
    }
    image
}

fn mean_abs_difference(a: &ImageBuf, b: &ImageBuf) -> f64 {
    assert_eq!((a.width, a.height), (b.width, b.height));
    a.data.iter().zip(&b.data).map(|(&x, &y)| (to_f64(x) - to_f64(y)).abs()).sum::<f64>() / a.data.len() as f64
}

#[test]
fn early_enlargement_develops_grain_and_halation_at_the_working_size() {
    let source = scene(48, 32);
    let mut params = linear_params();
    params.film_render.grain.active = true;
    params.film_render.halation.active = true;
    params.io.photo_long_side_px = Some(96);
    let crop = Crop { x: 0.0, y: 0.0, width: 1.0, height: 1.0, angle: 0.0 };

    // The pipeline's own resize, for the full frame...
    let early = pipeline(params.clone()).process(source.clone(), &CpuBackend);
    assert_eq!((early.width, early.height), (96, 64));
    // ...is the CLI's and helper's preparation followed by a factor-1 render,
    // metering the source as they do.
    let mut prepared_params = params.clone();
    let metering = filming::metering_preview(&source);
    let (prepared, size) = image_size::prepare(source.clone(), &mut prepared_params, Some(&crop)).unwrap();
    assert_eq!(size.working, (96, 64));
    assert_eq!(prepared_params.io.photo_long_side_px, None);
    let explicit = pipeline(prepared_params).with_metering_preview(metering).process(prepared, &CpuBackend);
    assert_eq!(early.data, explicit.data);

    // Negative control: developing first and enlarging the print afterwards
    // spreads the same grain over twice as many pixels.
    params.io.photo_long_side_px = None;
    let late = resize::catmull_rom(&pipeline(params).process(source, &CpuBackend), (96, 64));
    let difference = mean_abs_difference(&early, &late);
    assert!(difference > 0.01, "late enlargement should differ: {difference}");
}

/// The 10–90% width, in pixels, of the edge across the middle row.
fn edge_width(image: &ImageBuf) -> f64 {
    let y = image.height / 2;
    let row: Vec<f64> = (0..image.width).map(|x| to_f64(image.get(x, y)[1])).collect();
    let (dark, bright) = (row[0], row[row.len() - 1]);
    let at = |fraction: f64| {
        let level = dark + (bright - dark) * fraction;
        let i = row.iter().position(|&v| (v - level) * (bright - dark).signum() >= 0.0).unwrap();
        // Interpolate between the samples either side of the level.
        let (a, b) = (row[i - 1], row[i]);
        (i - 1) as f64 + (level - a) / (b - a)
    };
    at(0.9) - at(0.1)
}

#[test]
fn physical_blur_keeps_its_size_on_the_film() {
    let (width, height) = (240, 24);
    let mut source = ImageBuf::new(width, height);
    for y in 0..height {
        for x in 0..width {
            source.set(x, y, [from_f64(if x < width / 2 { 0.03 } else { 0.5 }); 3]);
        }
    }
    let mut params = linear_params();
    params.camera.auto_exposure = false;
    params.film_render.grain.active = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.film_render.glare.active = false;
    params.print_render.glare.active = false;
    params.scanner.lens_blur = 0.0;
    params.scanner.unsharp_mask = [0.0, 0.0];
    params.enlarger.lens_blur = 0.0;
    // 36 mm over 240 px is 150 µm per pixel: σ ≈ 5 px, then ≈ 10 px at 2×.
    params.camera.film_format_mm = 36.0;
    params.camera.lens_blur_um = 750.0;
    let original = edge_width(&pipeline(params.clone()).process(source.clone(), &CpuBackend));
    params.io.photo_long_side_px = Some(width * 2);
    let doubled = edge_width(&pipeline(params).process(source, &CpuBackend));
    let ratio = doubled / original;
    assert!((1.85..=2.15).contains(&ratio), "{original} px → {doubled} px ({ratio}×)");
}

/// Auto exposure meters the full source frame, as upstream meters the frame
/// before its crop and resize: every working size, preview cap and
/// interactive frame that carries the source's metering preview gets the
/// source's EV, while metering each resized frame itself would drift.
#[test]
fn auto_exposure_is_the_sources_at_every_size() {
    let source = scene(1200, 800);
    let base = pipeline(linear_params());
    let metering = filming::metering_preview(&source);
    let ev = base.autoexposure_ev(&metering);
    assert!(ev.is_finite() && ev != 0.0);
    // `process` on the source itself meters it before its own resize.
    assert_eq!(base.frame_autoexposure_ev(&source), ev);
    let mut drift = 0f64;
    for size in [(2400, 1600), (900, 600), (600, 400), (240, 160)] {
        let working = resize::catmull_rom(&source, size);
        let carried = base.clone().with_metering_preview(metering.clone());
        assert_eq!(carried.frame_autoexposure_ev(&working), ev, "{size:?}");
        drift = drift.max((base.frame_autoexposure_ev(&working) - ev).abs());
    }
    // Negative control: the resized frames meter differently on their own.
    assert!(drift > 1e-4, "drift {drift}");
}
