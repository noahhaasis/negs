//! Auto exposure against frozen upstream experimental 28bf883 references
//! (scripts/parity/freeze_autoexposure_reference.py): the full-frame metering
//! preview and `measure_raw_autoexposure_ev` for every metering method.

use serde::Deserialize;
use spektrafilm_core::stages::filming;
use spektrafilm_math::{image::ImageBuf, precision::{from_f64, to_f64}};
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Fixture {
    previews: Vec<PreviewCase>,
    raw_fields: Vec<RawCase>,
}

#[derive(Deserialize)]
struct PreviewCase {
    height: u32,
    width: u32,
    offset: f64,
    scale: f64,
    shape: [u32; 3],
    sum: f64,
    sum_sq: f64,
    indices: Vec<usize>,
    values: Vec<f64>,
    ev: BTreeMap<String, f64>,
}

#[derive(Deserialize)]
struct RawCase {
    height: u32,
    width: u32,
    offset: f64,
    scale: f64,
    ev: BTreeMap<String, f64>,
}

fn fixture() -> Fixture {
    serde_json::from_str(include_str!("fixtures/autoexposure_python_28bf883.json")).unwrap()
}

/// The fixture's integer-generated frame (`frame` in the freeze script).
fn frame(height: u32, width: u32, offset: f64, scale: f64) -> ImageBuf {
    let (h, w) = (height as i64, width as i64);
    let mut data = Vec::with_capacity((h * w * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            let bright = (6 * y - 2 * h).pow(2) + (6 * x - (18 * w).div_euclid(5)).pow(2) <= h * h;
            for c in 0..3 {
                let v = offset + scale * ((131 * y + 71 * x + 37 * c + (x * y) % 17) % 97) as f64 / 97.0;
                data.push(from_f64(if bright { 4.0 * v } else { v }));
            }
        }
    }
    ImageBuf::from_data(width, height, data)
}

#[cfg(feature = "precision-f64")]
const VALUE_TOLERANCE: f64 = 1e-12;
#[cfg(not(feature = "precision-f64"))]
const VALUE_TOLERANCE: f64 = 1e-6;
#[cfg(feature = "precision-f64")]
const EV_TOLERANCE: f64 = 1e-12;
#[cfg(not(feature = "precision-f64"))]
const EV_TOLERANCE: f64 = 1e-6;

#[test]
fn metering_preview_matches_python() {
    for case in fixture().previews {
        let image = frame(case.height, case.width, case.offset, case.scale);
        let preview = filming::metering_preview(&image);
        let label = format!("{}x{}", case.height, case.width);
        assert_eq!([preview.height, preview.width, 3], case.shape, "{label}");
        let values: Vec<f64> = preview.data.iter().map(|&v| to_f64(v)).collect();
        for (&i, &expected) in case.indices.iter().zip(&case.values) {
            assert!((values[i] - expected).abs() < VALUE_TOLERANCE, "{label} [{i}]: {} vs {expected}", values[i]);
        }
        let sum: f64 = values.iter().sum();
        let sum_sq: f64 = values.iter().map(|v| v * v).sum();
        assert!((sum - case.sum).abs() < VALUE_TOLERANCE * values.len() as f64, "{label} sum {sum} vs {}", case.sum);
        assert!((sum_sq - case.sum_sq).abs() < VALUE_TOLERANCE * values.len() as f64 * 16.0, "{label} sum of squares");
        for (method, expected) in &case.ev {
            let ev = filming::measure_raw_autoexposure_ev(&preview, method, 1.0);
            assert!((ev - expected).abs() < EV_TOLERANCE, "{label} {method}: {ev} vs {expected}");
        }
    }
}

#[test]
fn raw_metering_matches_python_for_every_method() {
    let fixture = fixture();
    for case in fixture.raw_fields {
        assert_eq!(case.ev.len(), 8);
        let raw = frame(case.height, case.width, case.offset, case.scale);
        for (method, expected) in &case.ev {
            let ev = filming::measure_raw_autoexposure_ev(&raw, method, 1.0);
            assert!(
                (ev - expected).abs() < EV_TOLERANCE,
                "{}x{} {method}: {ev} vs {expected}",
                case.height,
                case.width
            );
        }
    }
}

/// The target scales the metered value: a field at 2 meters 1 EV down to
/// the default target of 1, and 0 EV to a target of 2.
#[test]
fn target_sets_the_metered_level() {
    let raw = ImageBuf::from_data(2, 2, vec![from_f64(2.0); 12]);
    assert!((filming::measure_raw_autoexposure_ev(&raw, "average", 1.0) + 1.0).abs() < 1e-12);
    assert!(filming::measure_raw_autoexposure_ev(&raw, "average", 2.0).abs() < 1e-12);
}
