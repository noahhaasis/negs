//! End-to-end RAW loading tests use a generated linear DNG: no proprietary
//! camera fixture, download, or GPU is required.
use super::*;
use rawler::{
    RawImage,
    decoders::Camera,
    dng::{CropMode, DngCompression, DngPhotometricConversion, writer::DngWriter},
    imgop::{
        develop::{ProcessingStep, RawDevelop},
        xyz::Illuminant,
    },
    pixarray::PixU16,
    rawimage::{BlackLevel, RawPhotometricInterpretation, WhiteLevel},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

struct TempFile(PathBuf);
impl TempFile {
    fn new(ext: &str) -> Self {
        static SEQUENCE: AtomicU64 = AtomicU64::new(0);
        Self(std::env::temp_dir().join(format!(
            "spektrafilm-wb-{}-{}.{}",
            std::process::id(),
            SEQUENCE.fetch_add(1, Ordering::Relaxed),
            ext
        )))
    }
}
impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn raw_fixture() -> RawImage {
    let mut camera = Camera::new();
    camera.clean_make = "Spektrafilm".into();
    camera.clean_model = "WB test".into();
    camera.color_matrix.insert(
        Illuminant::D65,
        vec![0.5, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.75],
    );
    RawImage::new(
        camera,
        PixU16::new_with([6000, 9000, 7000].repeat(12), 12, 3),
        3,
        [2.2, 1.0, 1.4, 1.0],
        RawPhotometricInterpretation::LinearRaw,
        Some(BlackLevel::zero(1, 1, 3)),
        Some(WhiteLevel::new_bits(16, 3)),
        false,
    )
}

fn write_fixture() -> TempFile {
    let file = TempFile::new("DNG");
    let raw = raw_fixture();
    let mut writer = DngWriter::new(std::fs::File::create(&file.0).unwrap(), [1, 4, 0, 0]).unwrap();
    writer.load_base_tags(&raw).unwrap();
    writer
        .root_ifd_mut()
        .add_tag(rawler::tags::TiffCommonTag::Orientation, 6_u16);
    let mut frame = writer.subframe_on_root(0);
    frame
        .raw_image(
            &raw,
            CropMode::None,
            DngCompression::Uncompressed,
            DngPhotometricConversion::Original,
            1,
        )
        .unwrap();
    frame.finalize().unwrap();
    writer.close().unwrap();
    file
}

#[test]
fn selection_reloads_raw_and_export_json_reproduces_preview_pixels() {
    let file = write_fixture();
    let default = load_image(&file.0, Formats::Any).unwrap();
    assert_eq!((default.width, default.height), (3, 4)); // EXIF remains applied

    // Independent old loading path: decode, default develop minus gamma.
    let src = rawler::rawsource::RawSource::new(&file.0).unwrap();
    let raw = rawler::decode(&src, &Default::default()).unwrap();
    let mut dev = RawDevelop::default();
    dev.steps.retain(|s| *s != ProcessingStep::SRgb);
    let old = intermediate_to_image_buf(dev.develop_intermediate(&raw).unwrap()).oriented(6);
    assert_eq!(default.data, old.data);

    for mode in RawWhiteBalanceMode::ALL {
        let wb = RawWhiteBalance {
            mode,
            temperature: 3200.0,
            tint: 0.85,
        };
        let preview = load_image_with_wb(&file.0, Formats::Any, wb).unwrap();
        let export_wb = serde_json::from_str(&serde_json::to_string(&wb).unwrap()).unwrap();
        let export = load_image_with_wb(&file.0, Formats::TiffPngRaw, export_wb).unwrap();
        assert_eq!(preview.data, export.data, "{mode:?}");
        assert_eq!(
            (preview.width, preview.height),
            (export.width, export.height)
        );
        if mode == RawWhiteBalanceMode::AsShot {
            assert_eq!(preview.data, default.data);
        } else {
            assert_ne!(
                preview.data, default.data,
                "{mode:?} must affect input pixels"
            );
        }
        assert!(preview.data.iter().all(|v| v.is_finite() && *v >= 0.0));
    }
    // Switching back doesn't compound the previous correction.
    assert_eq!(load_raw(&file.0).unwrap().data, default.data);
    let daylight = load_raw_with_wb(
        &file.0,
        RawWhiteBalance {
            mode: RawWhiteBalanceMode::Daylight,
            ..Default::default()
        },
    )
    .unwrap();
    let custom_daylight = load_raw_with_wb(
        &file.0,
        RawWhiteBalance {
            mode: RawWhiteBalanceMode::Custom,
            temperature: 6504.0,
            tint: 1.0,
        },
    )
    .unwrap();
    assert_eq!(daylight.data, custom_daylight.data);
}

/// The browser's routes (`spektrafilm-wasm` decode/load and the opt-in
/// bounded decode) develop RAW bytes, not paths: they must apply the same
/// shared WB as the path loaders, mode by mode, and validate it.
#[test]
fn byte_loaders_apply_the_same_wb_as_path_loaders() {
    let file = write_fixture();
    let bytes = std::fs::read(&file.0).unwrap();
    let wbs = RawWhiteBalanceMode::ALL.map(|mode| RawWhiteBalance { mode, temperature: 3200.0, tint: 0.85 });
    for wb in wbs {
        let path = load_raw_with_wb(&file.0, wb).unwrap();
        assert_eq!(load_raw_bytes(&bytes, wb).unwrap().data, path.data, "{:?}", wb.mode);
        let (unoriented, orientation) = load_raw_bytes_unoriented(&bytes, wb).unwrap();
        assert_eq!(orientation, 6);
        assert_eq!(unoriented.oriented(orientation).data, path.data, "{:?}", wb.mode);
        for mode in [bounded::Mode::LargeBayer, bounded::Mode::ForceBayer] {
            match bounded::load_raw_bytes_kept(&bytes, wb, 0, 1e9, mode) {
                Ok(kept) => assert_eq!(kept.image.oriented(kept.orientation).data, path.data, "{:?} {mode:?}", wb.mode),
                // The linear fixture isn't a Bayer mosaic: forcing the banded
                // developer refuses it rather than ignoring the WB.
                Err(e) => assert!(matches!(mode, bounded::Mode::ForceBayer), "{e:#}"),
            }
        }
    }
    let [as_shot, daylight, tungsten, custom] = wbs.map(|wb| load_raw_bytes(&bytes, wb).unwrap().data);
    assert_ne!(daylight, as_shot);
    assert_ne!(tungsten, daylight);
    assert_ne!(custom, daylight);
    let custom_daylight = RawWhiteBalance { mode: RawWhiteBalanceMode::Custom, temperature: 6504.0, tint: 1.0 };
    assert_eq!(load_raw_bytes(&bytes, custom_daylight).unwrap().data, daylight);
    assert_eq!(load_raw_bytes(&bytes, RawWhiteBalance::default()).unwrap().data, as_shot);
    let invalid = RawWhiteBalance { mode: RawWhiteBalanceMode::Custom, temperature: 1000.0, tint: 1.0 };
    for error in [
        load_raw_bytes(&bytes, invalid).unwrap_err(),
        load_raw_bytes_unoriented(&bytes, invalid).unwrap_err(),
        bounded::load_raw_bytes_kept(&bytes, invalid, 0, 1e9, bounded::Mode::LargeBayer).err().unwrap(),
    ] {
        assert!(format!("{error:#}").contains("temperature must be between 1667 and 25000 K"), "{error:#}");
    }
    // Inactive custom values are kept but not validated or applied.
    let inactive = RawWhiteBalance { mode: RawWhiteBalanceMode::Daylight, temperature: f64::NAN, tint: -1.0 };
    assert_eq!(load_raw_bytes(&bytes, inactive).unwrap().data, daylight);
}

#[test]
fn as_shot_keeps_coefficients_and_missing_metadata_fallback() {
    let mut raw = raw_fixture();
    for coeffs in [[2.2, 1.0, 1.4, 1.0], [f32::NAN; 4]] {
        raw.wb_coeffs = coeffs;
        white_balance::prepare(&mut raw, RawWhiteBalance::default()).unwrap();
        assert_eq!(raw.wb_coeffs.map(f32::to_bits), coeffs.map(f32::to_bits));
    }
    raw.color_matrix.clear();
    assert!(white_balance::prepare(&mut raw, RawWhiteBalance::default()).is_ok());
    assert!(
        white_balance::prepare(
            &mut raw,
            RawWhiteBalance {
                mode: RawWhiteBalanceMode::Daylight,
                ..Default::default()
            }
        )
        .is_err()
    );
}

#[test]
fn non_raw_ignores_wb_including_inapplicable_custom_values() {
    for ext in ["png", "tiff"] {
        let file = TempFile::new(ext);
        image::RgbImage::from_pixel(2, 3, image::Rgb([120, 90, 30]))
            .save(&file.0)
            .unwrap();
        let baseline = load_image(&file.0, Formats::Any).unwrap();
        let wb = RawWhiteBalance {
            mode: RawWhiteBalanceMode::Custom,
            temperature: f64::NAN,
            tint: -1.0,
        };
        assert_eq!(
            baseline.data,
            load_image_with_wb(&file.0, Formats::TiffPngRaw, wb)
                .unwrap()
                .data
        );
    }
}

#[test]
fn monochrome_is_not_tinted() {
    let mut raw = raw_fixture();
    raw.photometric = RawPhotometricInterpretation::BlackIsZero;
    raw.color_matrix.clear();
    let wb = RawWhiteBalance {
        mode: RawWhiteBalanceMode::Custom,
        temperature: 2850.0,
        tint: 0.5,
    };
    white_balance::prepare(&mut raw, wb).unwrap();
    assert!(white_balance::is_monochrome(&raw));
    raw.photometric = RawPhotometricInterpretation::LinearRaw;
    raw.cpp = 1; // Leica monochrome DNG layout
    assert!(white_balance::is_monochrome(&raw));
    white_balance::prepare(&mut raw, wb).unwrap();
    let mut image = rawler::imgop::develop::Intermediate::Monochrome(
        rawler::pixarray::PixF32::new_with(vec![0.2], 1, 1),
    );
    white_balance::finish(&mut image, wb, true).unwrap();
    assert_eq!(
        intermediate_to_image_buf(image).data,
        vec![from_f32(0.2); 3]
    );
}

/// Supply an OS path-list to exercise real decoder/camera matrices without
/// bundling large proprietary fixtures.
#[test]
#[ignore = "requires local camera RAW files in SPEKTRAFILM_TEST_RAWS"]
fn camera_raw_smoke() {
    let paths = std::env::var_os("SPEKTRAFILM_TEST_RAWS").expect("set SPEKTRAFILM_TEST_RAWS");
    for path in std::env::split_paths(&paths) {
        let baseline = load_raw(&path).unwrap();
        for mode in RawWhiteBalanceMode::ALL {
            let wb = RawWhiteBalance {
                mode,
                temperature: 3200.0,
                tint: 0.85,
            };
            let image = load_raw_with_wb(&path, wb).unwrap();
            assert_eq!(
                (image.width, image.height),
                (baseline.width, baseline.height)
            );
            assert!(image.data.iter().all(|v| v.is_finite() && *v >= 0.0));
            if mode == RawWhiteBalanceMode::AsShot {
                assert_eq!(image.data, baseline.data);
            }
            eprintln!(
                "{}: {mode:?}, {}x{} OK",
                path.display(),
                image.width,
                image.height
            );
        }
    }
}
