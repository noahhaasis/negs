//! The fitted view's exact-size picture (ticket 38): the developed image
//! resampled once to the very pixels the fit shows, with Lanczos-3 in
//! linear light as Pillow's `Image.LANCZOS` does it, then drawn 1:1 on whole
//! pixels. Trilinear mipmaps between two powers of two lose about half the
//! fine detail; this keeps it. Display only: exports never come here.
//!
//! The GPU runs the same filter in `gpu.rs` (`Gpu::lanczos`); this is the
//! software canvas's, and the reference the tests hold both to.
//!
//! The shared core's (core/src/rules/fit, through negs_core_rules): the
//! same pixels as this module's Rust had (core/diff/tests/exact_fit.rs).

pub use negs_core_rules::exact_fit::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_round_the_fitted_size() {
        assert_eq!(target(6000.0, 4000.0, 2200.0 / 6000.0), (2200, 1467));
        assert_eq!(target(4896.0, 3264.0, 1035.0 / 3264.0), (1553, 1035));
        assert_eq!(target(333.0, 777.0, 0.5), (167, 389));
    }

    #[test]
    fn only_shrinking_larger_textures() {
        assert!(worth_it(0.317, (4896, 3264), (1553, 1035)));
        assert!(!worth_it(1.0, (4896, 3264), (4896, 3264)));
        // Limit preview: the texture is already smaller than the fit.
        assert!(!worth_it(0.3, (1200, 800), (1553, 1035)));
    }

    #[test]
    fn coefficients_follow_pillow() {
        // 2:1 widens the kernel to 12 taps, symmetric about the centre.
        let c = coefficients(100, 50);
        let (start, weights) = &c[25];
        assert_eq!(*start, 45);
        assert_eq!(weights.len(), 12);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        for k in 0..6 {
            assert!((weights[k] - weights[11 - k]).abs() < 1e-6);
        }
        // At the edge the kernel is cut and renormalised.
        let (start, weights) = &c[0];
        assert_eq!(*start, 0);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn flat_stays_flat_and_detail_survives() {
        let (w, h) = (97, 61);
        let grey = vec![128u8; w * h * 4];
        let out = resample(&grey, w as u32, h as u32, 40, 25);
        assert!(out.chunks(4).all(|p| p[0] == 128 && p[1] == 128 && p[2] == 128 && p[3] == 255));
        // Stripes three pixels wide stay stripes at 0.6.
        let stripes: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| if (x / 3) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] })).collect();
        let out = resample(&stripes, w as u32, h as u32, 58, 37);
        let row: Vec<u8> = out[12 * 58 * 4..13 * 58 * 4].chunks(4).map(|p| p[0]).collect();
        let (lo, hi) = (*row[4..54].iter().min().unwrap(), *row[4..54].iter().max().unwrap());
        assert!(hi > 200 && lo < 60, "{lo}..{hi}");
    }
}
