use super::*;
use rawler::{cfa::{CFA, PlaneColor}, decoders::Camera, pixarray::PixU16, rawimage::{BlackLevel, CFAConfig, WhiteLevel}};
use crate::RawWhiteBalanceMode;

fn fixture(width: usize, height: usize, pattern: &str) -> RawImage {
    let mut camera = Camera::new();
    camera.color_matrix.insert(rawler::imgop::xyz::Illuminant::D65,
        vec![0.7, 0.1, 0.0, 0.0, 0.9, 0.1, 0.05, 0.0, 0.8]);
    let config = CFAConfig::new(&CFA::new(pattern), &PlaneColor::new("RGB"));
    // Deterministic high-frequency signal, including black and clipped white.
    let data = (0..width * height).map(|i| {
        let n = (i as u32).wrapping_mul(1664525).wrapping_add(1013904223);
        if i % 13 == 0 { u16::MAX } else if i % 17 == 0 { 0 } else { (n >> 16) as u16 }
    }).collect();
    RawImage::new(camera, PixU16::new_with(data, width, height), 1,
        [2.2, 1.0, 1.4, 1.0], RawPhotometricInterpretation::Cfa(config),
        Some(BlackLevel::new(&[100_u16, 200, 300, 400], 2, 2, 1)),
        Some(WhiteLevel::new(vec![60000])), false)
}

#[test]
fn full_and_banded_pixels_are_exact_across_phases_crops_wb_and_rotations() {
    for pattern in ["RGGB", "BGGR", "GBRG", "GRBG"] {
        for (width, height) in [(47, 83), (48, 84)] {
            for (ax, ay, cx, cy) in [(0, 0, 0, 0), (1, 3, 2, 1), (2, 2, 1, 2)] {
                let mut raw = fixture(width, height, pattern);
                let active = Rect::new(Point::new(ax, ay), Dim2::new(width - ax - 2, height - ay - 3));
                raw.active_area = Some(active);
                raw.crop_area = Some(Rect::new(Point::new(ax + cx, ay + cy), Dim2::new(active.d.w - cx - 1, active.d.h - cy - 2)));
                for mode in RawWhiteBalanceMode::ALL {
                    let wb = RawWhiteBalance { mode, temperature: 3200.0, tint: 0.85 };
                    let full = crate::develop_decoded(raw.clone(), wb).unwrap();
                    for target in [(19, 31), (1, 1), (full.width, full.height)] {
                        let expected = if target == (full.width, full.height) { full.clone() } else { downsample_reference(&full, target) };
                        for band in [1, 7, 16, 32, 128] {
                            let actual = develop_bands(raw.clone(), wb, target, band).unwrap();
                            assert!(actual.data.iter().all(|v| v.is_finite()));
                            for rotation in 1..=8 {
                                assert_eq!(actual.clone().oriented(rotation).data, expected.clone().oriented(rotation).data,
                                    "{pattern}, {width}x{height}, crop {ax},{ay}/{cx},{cy}, {mode:?}, {target:?}, band {band}, rotation {rotation}");
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn true_sensor_borders_odd_trailing_scaling_and_float_mosaics_match() {
    for pattern in ["RGGB", "BGGR", "GBRG", "GRBG"] {
        for (w, h) in [(47, 83), (48, 84), (7, 7), (8, 8)] {
            for float in [false, true] {
                let mut raw = fixture(w, h, pattern);
                // No active/default crop: include the actual last odd row and
                // column, whose unscaled values are legacy rawler behavior.
                raw.active_area = None;
                raw.crop_area = None;
                if float {
                    let RawImageData::Integer(v) = &raw.data else { unreachable!() };
                    raw.data = RawImageData::Float(v.iter().map(|&v| v as f32 + 0.25).collect());
                }
                let full = crate::develop_decoded(raw.clone(), Default::default()).unwrap();
                for target in [(3, 4), (w as u32, h as u32)] {
                    let expected = downsample_reference(&full, target);
                    let actual = develop_bands(raw.clone(), Default::default(), target, 1).unwrap();
                    assert_eq!(actual.data, expected.data, "{pattern} {w}x{h} float={float}, target={target:?}");
                }
            }
        }
    }
}

#[test]
fn unsupported_sensors_and_invalid_crops_are_rejected() {
    let mut raw = fixture(48, 84, "RGGB");
    raw.crop_area = Some(Rect::new(Point::new(47, 0), Dim2::new(2, 84)));
    assert!(geometry(&raw).is_err());
    raw.crop_area = None;
    raw.photometric = RawPhotometricInterpretation::LinearRaw;
    assert!(geometry(&raw).is_err());
    raw.photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new(&CFA::new(&"RGB".repeat(12)), &PlaneColor::new("RGB")));
    assert!(geometry(&raw).is_err());
    raw.photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new(&CFA::new("RGBE"), &PlaneColor::new("RGBE")));
    assert!(geometry(&raw).is_err());
    for cap in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(load_raw_bytes_kept(&[], Default::default(), 0, cap, Mode::LargeBayer).is_err());
    }
}

#[test]
fn extreme_reduction_fails_instead_of_allocating_full_height_rgb() {
    let raw = fixture(48, 513, "RGGB");
    assert!(develop_bands(raw, Default::default(), (1, 1), 32).is_err());
    let raw = fixture(48, 513, "RGGB");
    let full = crate::develop_decoded(raw.clone(), Default::default()).unwrap();
    let expected = downsample_reference(&full, (12, 5));
    // More than 128 source rows per requested 32-output-row group: the
    // developer must split the group, without changing reducer spans/order.
    assert_eq!(develop_bands(raw, Default::default(), (12, 5), 32).unwrap().data, expected.data);
}

#[test]
fn cap_matches_leica_working_copy() {
    assert_eq!(kept_size(9520, 6336, 0, 12e6), (4246, 2826));
    assert_eq!(kept_size(9520, 6336, 0, 0.0), (9520, 6336));
}

#[test]
fn bayer_sensor_tiled_demosaic_matches_full_develop_across_orientations() {
    for pattern in ["RGGB", "BGGR", "GBRG", "GRBG"] {
        let mut raw = fixture(64, 52, pattern);
        let active = Rect::new(Point::new(2, 2), Dim2::new(60, 48));
        let crop = Rect::new(Point::new(4, 3), Dim2::new(55, 44));
        raw.active_area = Some(active);
        raw.crop_area = Some(crop);
        let wb = RawWhiteBalance::default();
        let full = crate::develop_decoded(raw.clone(), wb).unwrap();
        let mut prepared = raw.clone();
        white_balance::prepare(&mut prepared, wb).unwrap();
        let black = prepared.blacklevel.as_bayer_array();
        let white = prepared.whitelevel.as_bayer_array();
        for orientation in 1..=8 {
            let expected = full.clone().oriented(orientation);
            let sensor = BayerSensor {
                raw: prepared.clone(),
                active,
                crop,
                wb,
                black,
                white,
                orientation,
                is_xtrans: false,
            };
            let (fw, fh) = sensor.full_size();
            assert_eq!((fw, fh), (expected.width, expected.height));
            // Tile with arbitrary step sizes including padded edge tiles past fw/fh.
            let step = 19u32;
            let mut y = 0;
            while y < fh {
                let mut x = 0;
                while x < fw {
                    let tile = sensor.demosaic_upright_rect(x, y, step + 5, step + 3).unwrap();
                    assert_eq!((tile.width, tile.height), (step + 5, step + 3));
                    for ly in 0..tile.height {
                        let gy = (y + ly).min(fh - 1);
                        for lx in 0..tile.width {
                            let gx = (x + lx).min(fw - 1);
                            assert_eq!(
                                tile.get(lx, ly),
                                expected.get(gx, gy),
                                "{pattern} orient={orientation} tile=({x},{y}) at ({lx},{ly})->({gx},{gy})"
                            );
                        }
                    }
                    x += step;
                }
                y += step;
            }
        }
    }
}

#[test]
fn xtrans_sensor_banded_and_tiled_demosaic_matches_full_develop() {
    // Authentic Fuji X-Trans 6×6 CFA pattern:
    // G B G G R G / R G R B G B / G B G G R G / G R G G B G / B G B R G R / G R G G B G
    const XTRANS: &str = "GBGGRGRGRBGBGBGGRGGRGGBGBGBRGRGRGGBG";
    let mut raw = fixture(144, 132, XTRANS);
    let active = Rect::new(Point::new(6, 6), Dim2::new(132, 120));
    let crop = Rect::new(Point::new(8, 8), Dim2::new(124, 112));
    raw.active_area = Some(active);
    raw.crop_area = Some(crop);
    let wb = RawWhiteBalance::default();
    let full = crate::develop_decoded(raw.clone(), wb).unwrap();

    // Banded develop for mobile open.
    let banded = develop_bands_cfa(raw.clone(), wb, (62, 56), 16, true).unwrap();
    let expected_small = downsample_reference(&full, (62, 56));
    assert_eq!(banded.data.len(), expected_small.data.len());
    let max_diff = banded.data.iter().zip(&expected_small.data)
        .map(|(a, b)| (to_f32(*a) - to_f32(*b)).abs())
        .fold(0.0f32, f32::max);
    assert!(max_diff < 1e-4, "X-Trans banded develop diverged from full: max_diff={max_diff}");

    // Tiled demosaic across orientations for mobile full-res export.
    let mut prepared = raw.clone();
    white_balance::prepare(&mut prepared, wb).unwrap();
    let black = prepared.blacklevel.as_bayer_array();
    let white = prepared.whitelevel.as_bayer_array();
    for orientation in [1u16, 3, 6, 8] {
        let expected = full.clone().oriented(orientation);
        let sensor = BayerSensor {
            raw: prepared.clone(),
            active,
            crop,
            wb,
            black,
            white,
            orientation,
            is_xtrans: true,
        };
        let (fw, fh) = sensor.full_size();
        assert_eq!((fw, fh), (expected.width, expected.height));
        let step = 48u32;
        let mut worst = 0.0f32;
        let mut y = 0;
        while y < fh {
            let mut x = 0;
            while x < fw {
                let tile = sensor.demosaic_upright_rect(x, y, step + 8, step + 6).unwrap();
                for ly in 0..tile.height {
                    let gy = (y + ly).min(fh - 1);
                    for lx in 0..tile.width {
                        let gx = (x + lx).min(fw - 1);
                        let got = tile.get(lx, ly);
                        let want = expected.get(gx, gy);
                        for c in 0..3 {
                            worst = worst.max((to_f32(got[c]) - to_f32(want[c])).abs());
                        }
                    }
                }
                x += step;
            }
            y += step;
        }
        assert!(worst < 1e-4, "X-Trans tiled demosaic orient={orientation} diverged: worst={worst}");
    }
}
