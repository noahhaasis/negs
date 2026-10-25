//! Tetrahedral 3D-LUT interpolation against frozen Python experimental
//! 28bf883 (`apply_lut_3d`, whose default method is `tetrahedral`; main used
//! PCHIP). It backs `use_enlarger_lut` / `use_scanner_lut`.
//!
//! Regenerate the fixture with `scripts/parity/freeze_tetrahedral_lut_reference.py`
//! (Python only, never from Rust output).

use serde::Deserialize;
use spektrafilm_math::tetra3d::tetrahedral_interp;

/// Values are f64 bit patterns in hex, so the comparison can be exact.
#[derive(Deserialize)]
struct Fixture {
    size: usize,
    lut: Vec<String>,
    inputs: Vec<[String; 3]>,
    expected: Vec<[String; 3]>,
}

fn f64_from_hex(bits: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(bits, 16).unwrap())
}

#[test]
fn tetrahedral_lut_matches_python() {
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/tetrahedral_lut_python_28bf883.json")).unwrap();
    assert_eq!(fixture.lut.len(), fixture.size.pow(3) * 3);
    assert_eq!(fixture.inputs.len(), fixture.expected.len());
    let lut: Vec<f64> = fixture.lut.iter().map(|b| f64_from_hex(b)).collect();
    // Python scales the normalized input by `size - 1` before interpolating.
    let scale = (fixture.size - 1) as f64;
    for (input, want) in fixture.inputs.iter().zip(&fixture.expected) {
        let input = input.clone().map(|b| f64_from_hex(&b));
        let want = want.clone().map(|b| f64_from_hex(&b));
        let got = tetrahedral_interp(
            &lut,
            fixture.size,
            input[0] * scale,
            input[1] * scale,
            input[2] * scale,
        );
        // Same operations in the same order: bit for bit.
        assert_eq!(got, want, "input {input:?}");
    }
}
