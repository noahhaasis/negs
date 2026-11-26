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
                gain_maps: Vec::new(),
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
    let banded = develop_bands_cfa(raw.clone(), wb, (62, 56), 16, true, &[]).unwrap();
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
            gain_maps: Vec::new(),
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

fn linear_rgb_fixture(width: usize, height: usize) -> RawImage {
    let mut camera = Camera::new();
    camera.color_matrix.insert(
        rawler::imgop::xyz::Illuminant::D65,
        vec![0.7, 0.1, 0.0, 0.0, 0.9, 0.1, 0.05, 0.0, 0.8],
    );
    let data = (0..width * height * 3)
        .map(|i| {
            let n = (i as u32).wrapping_mul(1664525).wrapping_add(1013904223);
            if i % 19 == 0 { 50000 } else { ((n >> 17) as u16).saturating_add(500) }
        })
        .collect();
    RawImage::new(
        camera,
        PixU16::new_with(data, width * 3, height),
        3,
        [1.8, 1.0, 1.5, 1.0],
        RawPhotometricInterpretation::LinearRaw,
        Some(BlackLevel::new(&[100_u16, 150, 200], 1, 1, 3)),
        Some(WhiteLevel::new(vec![60000, 60000, 60000])),
        false,
    )
}

#[test]
fn gain_map_cfa_bit_exact_parity_across_full_banded_and_compact_paths() {
    use crate::gain_map::{OpcodeStage, parse_stage, test_helpers::*};
    let active = Rect::new(Point::new(2, 2), Dim2::new(60, 48));
    let crop = Rect::new(Point::new(4, 3), Dim2::new(55, 44));
    // 4 per-CFA-position OpcodeList2 GainMaps (mirroring Android DngCreator)
    // with asymmetric channel gains (>1.0 in corners, different R/Gr/Gb/B)
    // plus an OpcodeList3 3-plane GainMap.
    let list2_bodies = [
        encode_gain_map_body(0, 0, 48, 60, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.2, 1.5, 2.1, 1.4, 1.0, 1.5, 2.3, 1.6, 2.4]),
        encode_gain_map_body(0, 1, 48, 60, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.0, 1.4, 1.9, 1.3, 1.0, 1.4, 2.1, 1.5, 2.0]),
        encode_gain_map_body(1, 0, 48, 60, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.05, 1.42, 1.95, 1.32, 1.02, 1.41, 2.12, 1.51, 2.03]),
        encode_gain_map_body(1, 1, 48, 60, 0, 1, 2, 2, 3, 3, 0.5, 0.5, 0.0, 0.0, 1,
            &[2.8, 1.8, 2.7, 1.7, 1.05, 1.75, 2.9, 1.85, 3.0]),
    ];
    let list2_refs: Vec<(u32, &[u8])> = list2_bodies.iter().map(|b| (9u32, b.as_slice())).collect();
    let mut gain_maps = parse_stage(&encode_opcode_list(&list2_refs), OpcodeStage::List2);
    let list3_body = encode_gain_map_body(
        0, 0, 48, 60, 0, 3, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 3,
        &[1.05, 1.0, 1.1,  1.1, 1.02, 1.15,  1.08, 1.01, 1.12,  1.12, 1.03, 1.2],
    );
    gain_maps.extend(parse_stage(&encode_opcode_list(&[(9, &list3_body)]), OpcodeStage::List3));
    assert_eq!(gain_maps.len(), 5);

    for pattern in ["RGGB", "BGGR", "GBRG", "GRBG"] {
        let mut raw = fixture(64, 52, pattern);
        raw.active_area = Some(active);
        raw.crop_area = Some(crop);
        let wb = RawWhiteBalance::default();

        let no_gain = crate::develop_decoded_with(raw.clone(), wb, crate::XTransDemosaic::default(), &[]).unwrap();
        let full = crate::develop_decoded_with(raw.clone(), wb, crate::XTransDemosaic::default(), &gain_maps).unwrap();
        assert_ne!(no_gain.data, full.data, "gain maps must visibly change output");

        // 1. Banded CFA preview matches full-frame + downsample bit for bit.
        let target = (27, 22);
        let banded = develop_bands_cfa(raw.clone(), wb, target, 7, false, &gain_maps).unwrap();
        assert_eq!(banded.data, downsample_reference(&full, target).data, "{pattern} banded mismatch");

        // 2. Compact BayerSensor tiled demosaic + small_preview match bit for bit across orientations 1..=8.
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
                gain_maps: gain_maps.clone(),
            };
            // Check small_preview parity
            let small = sensor.small_preview(24).unwrap();
            let small_target = kept_size(crop.d.w as u32, crop.d.h as u32, 24, 0.0);
            let expected_small = downsample_reference(&full, small_target).oriented(orientation);
            assert_eq!(small.data, expected_small.data, "{pattern} orient={orientation} small_preview mismatch");

            // Check tiled demosaic parity (including padded edge tiles)
            let (fw, fh) = sensor.full_size();
            let step = 19u32;
            let mut y = 0;
            while y < fh {
                let mut x = 0;
                while x < fw {
                    let tile = sensor.demosaic_upright_rect(x, y, step + 5, step + 3).unwrap();
                    for ly in 0..tile.height {
                        let gy = (y + ly).min(fh - 1);
                        for lx in 0..tile.width {
                            let gx = (x + lx).min(fw - 1);
                            assert_eq!(
                                tile.get(lx, ly),
                                expected.get(gx, gy),
                                "{pattern} orient={orientation} tile=({x},{y}) at ({lx},{ly})"
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
fn gain_map_linear_rgb_bit_exact_parity_across_full_banded_and_compact_paths() {
    use crate::gain_map::{OpcodeStage, parse_stage, test_helpers::*};
    let mut raw = linear_rgb_fixture(40, 32);
    let active = Rect::new(Point::new(0, 0), Dim2::new(40, 32));
    let crop = Rect::new(Point::new(3, 2), Dim2::new(34, 27));
    raw.active_area = Some(active);
    raw.crop_area = Some(crop);

    // Map 1 (List2): MapPlanes = 1 across planes 0..3
    let b1 = encode_gain_map_body(0, 0, 32, 40, 0, 3, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 1, &[1.8, 1.2, 1.3, 1.9]);
    // Map 2 (List3): MapPlanes = 3 across planes 0..3 with per-channel shading
    let b2 = encode_gain_map_body(
        0, 0, 32, 40, 0, 3, 1, 1, 2, 2, 1.0, 1.0, 0.0, 0.0, 3,
        &[1.1, 1.0, 1.4,  1.2, 1.05, 1.3,  1.15, 1.02, 1.35,  1.25, 1.08, 1.5],
    );
    let mut gain_maps = parse_stage(&encode_opcode_list(&[(9, &b1)]), OpcodeStage::List2);
    gain_maps.extend(parse_stage(&encode_opcode_list(&[(9, &b2)]), OpcodeStage::List3));

    let wb = RawWhiteBalance::default();
    let full = crate::develop_decoded_with(raw.clone(), wb, crate::XTransDemosaic::default(), &gain_maps).unwrap();

    // 1. Banded LinearRaw preview matches full-frame + downsample bit for bit.
    let target = (17, 13);
    let banded = develop_bands_linear_rgb(raw.clone(), wb, target, 5, &gain_maps).unwrap();
    assert_eq!(banded.data, downsample_reference(&full, target).data, "LinearRaw banded mismatch");

    // 2. CompactRawSensor::LinearRgb tiled extract matches full-frame bit for bit across orientations 1..=8.
    let mut prepared = raw.clone();
    white_balance::prepare(&mut prepared, wb).unwrap();
    let black = prepared.blacklevel.as_vec();
    let white = prepared.whitelevel.as_vec();
    for orientation in 1..=8 {
        let expected = full.clone().oriented(orientation);
        let sensor = CompactRawSensor {
            inner: CompactRawKind::LinearRgb {
                raw: prepared.clone(),
                active,
                crop,
                wb,
                black: black.clone(),
                white: white.clone(),
                orientation,
                gain_maps: gain_maps.clone(),
            },
            decoder: "rawler",
            lens_correction_supported: false,
        };
        let (fw, fh) = sensor.full_size();
        assert_eq!((fw, fh), (expected.width, expected.height));
        let step = 13u32;
        let mut y = 0;
        while y < fh {
            let mut x = 0;
            while x < fw {
                let tile = sensor.extract_upright_rect(x, y, step + 4, step + 3).unwrap();
                for ly in 0..tile.height {
                    let gy = (y + ly).min(fh - 1);
                    for lx in 0..tile.width {
                        let gx = (x + lx).min(fw - 1);
                        assert_eq!(
                            tile.get(lx, ly),
                            expected.get(gx, gy),
                            "LinearRaw orient={orientation} tile=({x},{y}) at ({lx},{ly})"
                        );
                    }
                }
                x += step;
            }
            y += step;
        }
    }
}
