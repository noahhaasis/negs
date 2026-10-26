//! 3D LUT with tetrahedral interpolation.
//!
//! Port of Python `spektrafilm.utils.fast_interp_lut.apply_lut_tetrahedral_3d`
//! (`_tetra_interp_lut_at_3d`), which upstream experimental 28bf883 made the
//! default of `apply_lut_3d`, so it backs `use_enlarger_lut` /
//! `use_scanner_lut` (upstream main used PCHIP, [`crate::pchip3d`]). Each
//! cube cell splits into six tetrahedra along its main diagonal; a query
//! blends the four corners of the one it falls in. No preparation, and
//! monotonic by construction.
//!
//! Layout as in [`crate::pchip3d`]: `lut[i, j, k, c]` is stored row-major as
//! `flat[((i*size + j)*size + k)*3 + c]`, f64 like Python's LUTs. The
//! comparisons (strict `>`, so ties take the later branch) and the order of
//! the additions follow the numba kernel exactly.

#[inline]
fn idx(size: usize, i: usize, j: usize, k: usize, c: usize) -> usize {
    ((i * size + j) * size + k) * 3 + c
}

/// Interpolate one point of a `size³ × 3` LUT. `r`, `g`, `b` are LUT-axis
/// coordinates in `[0, size-1]` (clamped, as Python clamps them).
#[inline]
pub fn tetrahedral_interp(lut: &[f64], size: usize, r: f64, g: f64, b: f64) -> [f64; 3] {
    let (i, fr) = crate::pchip3d::cubic_base_fraction(r, size);
    let (j, fg) = crate::pchip3d::cubic_base_fraction(g, size);
    let (k, fb) = crate::pchip3d::cubic_base_fraction(b, size);
    let at = |di: usize, dj: usize, dk: usize, c: usize| lut[idx(size, i + di, j + dj, k + dk, c)];
    let mut out = [0.0f64; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let c000 = at(0, 0, 0, c);
        let c111 = at(1, 1, 1, c);
        *o = if fr > fg {
            if fg > fb {
                // fr >= fg >= fb
                let (c100, c110) = (at(1, 0, 0, c), at(1, 1, 0, c));
                c000 + fr * (c100 - c000) + fg * (c110 - c100) + fb * (c111 - c110)
            } else if fr > fb {
                // fr >= fb >= fg
                let (c100, c101) = (at(1, 0, 0, c), at(1, 0, 1, c));
                c000 + fr * (c100 - c000) + fb * (c101 - c100) + fg * (c111 - c101)
            } else {
                // fb >= fr >= fg
                let (c001, c101) = (at(0, 0, 1, c), at(1, 0, 1, c));
                c000 + fb * (c001 - c000) + fr * (c101 - c001) + fg * (c111 - c101)
            }
        } else if fb > fg {
            // fb >= fg >= fr
            let (c001, c011) = (at(0, 0, 1, c), at(0, 1, 1, c));
            c000 + fb * (c001 - c000) + fg * (c011 - c001) + fr * (c111 - c011)
        } else if fb > fr {
            // fg >= fb >= fr
            let (c010, c011) = (at(0, 1, 0, c), at(0, 1, 1, c));
            c000 + fg * (c010 - c000) + fb * (c011 - c010) + fr * (c111 - c011)
        } else {
            // fg >= fr >= fb
            let (c010, c110) = (at(0, 1, 0, c), at(1, 1, 0, c));
            c000 + fg * (c010 - c000) + fr * (c110 - c010) + fb * (c111 - c110)
        };
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lut_from(size: usize, f: impl Fn(f64, f64, f64) -> [f64; 3]) -> Vec<f64> {
        let mut data = vec![0.0; size * size * size * 3];
        for i in 0..size {
            for j in 0..size {
                for k in 0..size {
                    let v = f(i as f64, j as f64, k as f64);
                    data[idx(size, i, j, k, 0)..idx(size, i, j, k, 0) + 3].copy_from_slice(&v);
                }
            }
        }
        data
    }

    #[test]
    fn reproduces_affine_functions_and_clamps() {
        // Tetrahedral interpolation is exact for affine functions in every
        // tetrahedron, whichever branch the ordering picks.
        let f = |r: f64, g: f64, b: f64| [0.5 * r - g + 0.25 * b + 1.0, 2.0 * g, -b + 0.1 * r];
        let size = 5;
        let lut = lut_from(size, f);
        for &(r, g, b) in &[
            (0.3, 0.2, 0.1),
            (0.3, 0.1, 0.2),
            (0.1, 0.3, 0.2),
            (0.2, 0.3, 0.1),
            (0.2, 0.1, 0.3),
            (0.1, 0.2, 0.3),
            (2.5, 2.5, 2.5),
            (3.9, 0.0, 1.7),
        ] {
            let got = tetrahedral_interp(&lut, size, r, g, b);
            let want = f(r, g, b);
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 1e-12, "({r}, {g}, {b}): {got:?} vs {want:?}");
            }
        }
        // Outside [0, size-1] the coordinates clamp to the faces.
        assert_eq!(tetrahedral_interp(&lut, size, -1.0, 9.0, 4.0), f(0.0, 4.0, 4.0));
    }
}
