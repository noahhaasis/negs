//! Integration tests gating byte-identical shared tiled export against
//! whole-frame export across GPU, CPU fallback, finishing (crop/straighten/
//! flip/rotate/border), EXIF+ICC metadata, and LinearRaw/Bayer sensors.

use std::path::{Path, PathBuf};

use spektrafilm_core::{
    params::RuntimeParams,
    pipeline::{Pipeline, clip_to_display},
    post::{PostParams, PostProcess},
    profile,
    stages,
    tiled_export::{
        DEFAULT_MAX_TILE_HALO, ExportFinish, ExportSource, export_tiled_rgb8,
        export_tiled_to_file, quantize_scalar_u8,
    },
};
use spektrafilm_gpu::{ComputeBackend, cpu_backend::CpuBackend};
use spektrafilm_io::{
    InputColour, RawWhiteBalance,
    border::{BorderOptions, BorderStyle, add_border},
    bounded::CompactRawSensor,
    crop::Crop,
    encoding::Encoding,
    metadata::{ImageMetadata, read_metadata},
    raw::{LensCorrection, RawDecodeOptions, RawDecoderPreference, decode_raw},
    save::{SaveOptions, save_image_with_options, save_rgb8_with_options, save_rgb16_with_options},
};
use spektrafilm_math::{
    image::ImageBuf,
    precision::{from_f32, to_f32},
};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
}

fn synthetic_scene(width: u32, height: u32) -> ImageBuf {
    let mut data = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for y in 0..height {
        let fy = y as f32 / (height - 1).max(1) as f32;
        for x in 0..width {
            let fx = x as f32 / (width - 1).max(1) as f32;
            let checker = (((x / 37) + (y / 29)) % 2) as f32;
            let wave = (fx * 19.0 + fy * 13.0).sin() * 0.5 + 0.5;
            let r = 0.03 + 0.45 * fx + 0.25 * wave + 0.15 * checker;
            let g = 0.04 + 0.35 * fy + 0.20 * (1.0 - wave) + 0.10 * checker;
            let b = 0.02 + 0.40 * (1.0 - fx) * fy + 0.18 * wave;
            data.push(from_f32(r));
            data.push(from_f32(g));
            data.push(from_f32(b));
        }
    }
    ImageBuf::from_data(width, height, data)
}

fn whole_frame_rgb8(
    image: &ImageBuf,
    pipeline: &Pipeline,
    post: &PostParams,
    backend: &dyn ComputeBackend,
) -> Vec<u8> {
    let metering = stages::filming::metering_preview(image);
    let pipe = pipeline.clone().with_metering_preview(metering);
    let mut scan = pipe
        .process_resident_borrowed(image, backend)
        .unwrap_or_else(|| pipe.process_with_backend(image.clone(), backend).0);
    let encoding = Encoding::new(&pipe.params.io.output_color_space, pipe.params.io.output_cctf_encoding);
    clip_to_display(&mut scan, encoding.encoded);
    if !post.is_identity() {
        PostProcess::new(post, encoding.encoded, encoding.space.name).apply(&mut scan);
    }
    scan.data.iter().map(|&v| quantize_scalar_u8(v)).collect()
}

fn max_diff(a: &[u8], b: &[u8]) -> (u8, usize) {
    assert_eq!(a.len(), b.len());
    let mut max_d = 0u8;
    let mut diff_px = 0usize;
    for (pa, pb) in a.chunks_exact(3).zip(b.chunks_exact(3)) {
        let d = pa[0].abs_diff(pb[0]).max(pa[1].abs_diff(pb[1])).max(pa[2].abs_diff(pb[2]));
        if d > 0 {
            diff_px += 1;
            max_d = max_d.max(d);
        }
    }
    (max_d, diff_px)
}

#[test]
fn tiled_gpu_matches_whole_frame_byte_for_byte_with_grain_usm_glare_unsharp_prep_post() {
    let backend = spektrafilm_gpu::select_backend();
    let data = data_dir();
    let img = synthetic_scene(640, 440);

    let mut params = RuntimeParams::default();
    params.io.apply_input_colour(&InputColour::raw("rawler"));
    params.settings.serial_grain = false;
    // Keep halation & DIR off so no 150px-sigma IIR seg_len rounding masks grain/USM/glare/unsharp/Prep/Post parity.
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.film_render.grain.active = true;
    params.film_render.grain.mult_usm_amount = 0.6;
    params.film_render.glare.active = true;
    params.print_render.glare.active = true;
    params.scanner.unsharp_mask = [1.2, 0.35];
    params.prep.highlights = -35.0;
    params.prep.shadows = 25.0;
    params.prep.midtone_detail = 30.0;
    params.prep.exposure_ev = -0.25;

    let mut post = PostParams::default();
    post.contrast = 12.0;
    post.saturation = 8.0;

    let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
    let pipeline = Pipeline::new_with_spectral(film, paper, params, &data).unwrap();

    let whole = whole_frame_rgb8(&img, &pipeline, &post, backend.as_ref());
    let source = ExportSource::from_image(img);
    // Force a multi-tile grid (e.g. 2x2 or 3x2) so two-pass grain USM, TiledPrep,
    // edge-aligned tile origins, glare, unsharp, and Post all exercise tiling.
    let (w, h, tiled, report) = export_tiled_rgb8(
        &source,
        &pipeline,
        &post,
        backend.as_ref(),
        90_000,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    assert_eq!((w, h), (640, 440));
    assert!(report.total_tiles > 1, "expected multi-tile grid, got {}", report.total_tiles);
    assert!(report.two_pass_usm, "expected two-pass global grain USM");

    let (max_d, diff_px) = max_diff(&whole, &tiled);
    assert_eq!(
        max_d, 0,
        "expected byte-identical tiled vs whole-frame export (backend={}), got max_u8_diff={max_d} across {diff_px} pixels",
        report.backend
    );
}

#[test]
fn cpu_tiled_fallback_is_byte_identical_across_tile_grids_with_grain_usm_and_glare() {
    let data = data_dir();
    let img = synthetic_scene(600, 400);

    let mut params = RuntimeParams::default();
    params.io.apply_input_colour(&InputColour::raw("rawler"));
    params.settings.serial_grain = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.film_render.grain.active = true;
    params.film_render.grain.mult_usm_amount = 0.5;
    params.film_render.glare.active = true;
    params.print_render.glare.active = true;
    params.scanner.unsharp_mask = [1.0, 0.25];

    let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
    let pipeline = Pipeline::new_with_spectral(film, paper, params, &data).unwrap();
    let source = ExportSource::from_image(img);

    let (_, _, tiled_a, rep_a) = export_tiled_rgb8(
        &source,
        &pipeline,
        &PostParams::default(),
        &CpuBackend,
        80_000,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    let (_, _, tiled_b, rep_b) = export_tiled_rgb8(
        &source,
        &pipeline,
        &PostParams::default(),
        &CpuBackend,
        140_000,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    assert!(rep_a.total_tiles > 1 && rep_b.total_tiles > 1);
    assert_ne!(rep_a.total_tiles, rep_b.total_tiles);
    assert!(rep_a.two_pass_usm && rep_b.two_pass_usm);

    let (max_d, diff_px) = max_diff(&tiled_a, &tiled_b);
    assert_eq!(
        max_d, 0,
        "CPU tiled fallback differed between grids ({} vs {} tiles): max_u8_diff={max_d}, diff_px={diff_px}",
        rep_a.total_tiles, rep_b.total_tiles
    );
}

#[test]
fn export_tiled_to_file_matches_whole_frame_save_with_finishing_and_metadata() {
    let backend = spektrafilm_gpu::select_backend();
    let data = data_dir();
    let tmp = std::env::temp_dir().join(format!("negs_tiled_test_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();

    // Write a 16-bit PNG input with synthetic EXIF metadata.
    let input_img = synthetic_scene(480, 320);
    let input_path = tmp.join("input.png");
    let mut meta = ImageMetadata::default();
    meta.make = Some("Leica Camera AG".to_owned());
    meta.model = Some("Q3".to_owned());
    save_image_with_options(
        &input_img,
        &input_path,
        Some(&meta),
        &SaveOptions { bit_depth: Some(16), copy_metadata: true, ..SaveOptions::default() },
    )
    .unwrap();

    let mut params = RuntimeParams::default();
    params.settings.serial_grain = false;
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.active = false;
    params.film_render.grain.active = true;
    params.post.contrast = 10.0;

    // 1. Axis-aligned crop + flip + rotate_cw + border + JPEG (fast RGB8 path)
    let crop_axis = Crop { x: 0.1, y: 0.15, width: 0.75, height: 0.7, angle: 0.0 };
    let border_opts = BorderOptions::default();
    let save_jpg = SaveOptions { jpeg_quality: 92, copy_metadata: true, ..SaveOptions::default() };

    let whole_jpg = tmp.join("whole_axis.jpg");
    let tiled_jpg = tmp.join("tiled_axis.jpg");
    {
        let loaded = spektrafilm_io::load_image_colour(
            &input_path,
            spektrafilm_io::Formats::Any,
            RawWhiteBalance::default(),
            LensCorrection::default(),
            &spektrafilm_io::InputInterpretation::default(),
        )
        .unwrap();
        let mut p = params.clone();
        p.io.apply_input_colour(&loaded.colour);
        let post = std::mem::take(&mut p.post);
        let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
        let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
        let metering = stages::filming::metering_preview(&loaded.image);
        let pipe = Pipeline::new_with_spectral(film, paper, p.clone(), &data)
            .unwrap()
            .with_metering_preview(metering);
        let mut scan = pipe
            .process_resident_borrowed(&loaded.image, backend.as_ref())
            .unwrap_or_else(|| pipe.process_with_backend(loaded.image.clone(), backend.as_ref()).0);
        let encoding = Encoding::new(&p.io.output_color_space, p.io.output_cctf_encoding);
        clip_to_display(&mut scan, encoding.encoded);
        PostProcess::new(&post, encoding.encoded, encoding.space.name).apply(&mut scan);
        let cropped = crop_axis.apply(&scan);
        let rotated = cropped.flipped_rotated_cw(true, 1);
        let bordered = add_border(&rotated, BorderStyle::Solid, &border_opts, "kodak_portra_400", encoding, &data).unwrap();
        let src_meta = read_metadata(&input_path);
        save_image_with_options(&bordered, &whole_jpg, src_meta.as_ref(), &save_jpg).unwrap();
    }
    let rep = export_tiled_to_file(
        &input_path,
        &tiled_jpg,
        "kodak_portra_400",
        "kodak_portra_endura",
        params.clone(),
        ExportFinish {
            post: params.post.clone(),
            crop: Some(crop_axis),
            crop_after_post: true,
            flip: true,
            rotate_cw: 1,
            border: Some(BorderStyle::Solid),
            border_options: border_opts.clone(),
            instagram: false,
            fit: None,
            save_options: save_jpg.clone(),
        },
        &data,
        backend.as_ref(),
        60_000,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    assert!(rep.total_tiles > 1);
    assert_eq!(
        std::fs::read(&whole_jpg).unwrap(),
        std::fs::read(&tiled_jpg).unwrap(),
        "axis-aligned crop + flip + rotate + border JPEG must be byte-identical (including EXIF + ICC)"
    );
    let out_meta = read_metadata(&tiled_jpg).expect("EXIF preserved on tiled JPEG");
    assert_eq!(out_meta.make.as_deref(), Some("Leica Camera AG"));
    assert_eq!(out_meta.model.as_deref(), Some("Q3"));

    // 2. Straightened crop (angle != 0) + PNG (exact f32 bilinear per-tile path)
    let crop_rot = Crop { x: 0.15, y: 0.15, width: 0.65, height: 0.65, angle: 7.5 };
    let save_png = SaveOptions { bit_depth: Some(8), copy_metadata: true, ..SaveOptions::default() };
    let whole_png = tmp.join("whole_straight.png");
    let tiled_png = tmp.join("tiled_straight.png");
    {
        let loaded = spektrafilm_io::load_image_colour(
            &input_path,
            spektrafilm_io::Formats::Any,
            RawWhiteBalance::default(),
            LensCorrection::default(),
            &spektrafilm_io::InputInterpretation::default(),
        )
        .unwrap();
        let mut p = params.clone();
        p.io.apply_input_colour(&loaded.colour);
        let post = std::mem::take(&mut p.post);
        let film = profile::load_profile_by_name(&data, "kodak_portra_400").unwrap();
        let paper = profile::load_profile_by_name(&data, "kodak_portra_endura").unwrap();
        let metering = stages::filming::metering_preview(&loaded.image);
        let pipe = Pipeline::new_with_spectral(film, paper, p.clone(), &data)
            .unwrap()
            .with_metering_preview(metering);
        let mut scan = pipe
            .process_resident_borrowed(&loaded.image, backend.as_ref())
            .unwrap_or_else(|| pipe.process_with_backend(loaded.image.clone(), backend.as_ref()).0);
        let encoding = Encoding::new(&p.io.output_color_space, p.io.output_cctf_encoding);
        clip_to_display(&mut scan, encoding.encoded);
        PostProcess::new(&post, encoding.encoded, encoding.space.name).apply(&mut scan);
        let cropped = crop_rot.apply(&scan);
        let src_meta = read_metadata(&input_path);
        save_image_with_options(&cropped, &whole_png, src_meta.as_ref(), &save_png).unwrap();
    }
    let rep2 = export_tiled_to_file(
        &input_path,
        &tiled_png,
        "kodak_portra_400",
        "kodak_portra_endura",
        params.clone(),
        ExportFinish {
            post: params.post.clone(),
            crop: Some(crop_rot),
            crop_after_post: true,
            flip: false,
            rotate_cw: 0,
            border: None,
            border_options: border_opts,
            instagram: false,
            fit: None,
            save_options: save_png.clone(),
        },
        &data,
        backend.as_ref(),
        60_000,
        DEFAULT_MAX_TILE_HALO,
    )
    .unwrap();
    assert!(rep2.total_tiles > 1);
    assert_eq!(
        std::fs::read(&whole_png).unwrap(),
        std::fs::read(&tiled_png).unwrap(),
        "straightened crop PNG must be byte-identical (including EXIF + ICC)"
    );

    // 3. Direct save_rgb8_with_options & save_rgb16_with_options byte parity
    let p8_a = tmp.join("direct8_a.png");
    let p8_b = tmp.join("direct8_b.png");
    let p16_a = tmp.join("direct16_a.tif");
    let p16_b = tmp.join("direct16_b.tif");
    let rgb8: Vec<u8> = input_img.data.iter().map(|&v| quantize_scalar_u8(v)).collect();
    let rgb16: Vec<u16> = input_img
        .data
        .iter()
        .map(|&v| (to_f32(v).clamp(0.0, 1.0) * 65535.0).round_ties_even() as u16)
        .collect();
    save_image_with_options(&input_img, &p8_a, Some(&meta), &save_png).unwrap();
    save_rgb8_with_options(&rgb8, input_img.width, input_img.height, &p8_b, Some(&meta), &save_png).unwrap();
    assert_eq!(std::fs::read(&p8_a).unwrap(), std::fs::read(&p8_b).unwrap());

    let save_tif16 = SaveOptions { bit_depth: Some(16), copy_metadata: true, ..SaveOptions::default() };
    save_image_with_options(&input_img, &p16_a, Some(&meta), &save_tif16).unwrap();
    save_rgb16_with_options(&rgb16, input_img.width, input_img.height, &p16_b, Some(&meta), &save_tif16).unwrap();
    assert_eq!(std::fs::read(&p16_a).unwrap(), std::fs::read(&p16_b).unwrap());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn compact_linear_raw_sensor_matches_full_decode_on_bundled_dng() {
    let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/sample/DSC_6377.dng");
    if !sample.exists() {
        return;
    }
    let sensor = CompactRawSensor::try_open_path(
        &sample,
        RawWhiteBalance::default(),
        LensCorrection::default(),
        1_000_000,
    )
    .unwrap()
    .expect("DSC_6377.dng should open as compact linear-rgb sensor");
    assert_eq!(sensor.kind(), "linear-rgb-sensor");

    let full = decode_raw(
        &sample,
        RawDecodeOptions {
            white_balance: RawWhiteBalance::default(),
            apply_orientation: true,
            lens_correction: LensCorrection::default(),
            max_long_side: 0,
        },
        RawDecoderPreference::Rawler,
    )
    .unwrap();
    assert_eq!(sensor.full_size(), (full.image.width, full.image.height));

    // Check several interior & edge tiles against whole-frame decode_raw.
    let (fw, fh) = sensor.full_size();
    for (ox, oy, w, h) in [
        (0, 0, 256, 256),
        (1234, 987, 300, 200),
        (fw - 256, fh - 256, 256, 256),
    ] {
        let tile = sensor.extract_upright_rect(ox, oy, w, h).unwrap();
        for ly in 0..h as usize {
            for lx in 0..w as usize {
                let ti = (ly * w as usize + lx) * 3;
                let fi = ((oy as usize + ly) * fw as usize + (ox as usize + lx)) * 3;
                for c in 0..3 {
                    assert_eq!(
                        to_f32(tile.data[ti + c]).to_bits(),
                        to_f32(full.image.data[fi + c]).to_bits(),
                        "LinearRaw tile mismatch at ({}, {}, ch={c})",
                        ox as usize + lx,
                        oy as usize + ly
                    );
                }
            }
        }
    }
}
