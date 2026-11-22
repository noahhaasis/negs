//! windows/src/exact_fit.rs's API on the shared Odin core
//! (core/src/rules/fit). The fitted view's exact-size picture (ticket 38):
//! the developed image resampled once to the very pixels the fit shows,
//! with Lanczos-3 in linear light as Pillow's `Image.LANCZOS` does it, then
//! drawn 1:1 on whole pixels. Display only: exports never come here.
//!
//! Everything runs in Odin, the resampling on up to 16 threads; the pixels
//! cross through the caller's buffers and are identical to the Rust's
//! (core/diff/tests/exact_fit.rs).

use negs_core_sys::picture as sys;

/// The exact picture's size for a `w` × `h` photograph (its layout size,
/// before crop and turns) shown at `z` physical pixels per photo pixel.
pub fn target(w: f64, h: f64, z: f64) -> (u32, u32) {
    let mut out = [0u32; 2];
    unsafe { sys::negs_fit_target(w, h, z, &mut out) };
    (out[0], out[1])
}

/// Whether the fitted view should draw an exact picture: shrinking, and the
/// texture (smaller than the layout with Limit preview) larger than the target.
pub fn worth_it(z: f64, texture: (u32, u32), target: (u32, u32)) -> bool {
    unsafe { sys::negs_fit_worth_it(z, texture.0, texture.1, target.0, target.1) != 0 }
}

/// For each of `output` pixels along an axis of `input`, the first input
/// pixel it reads and the normalised weights from there (Pillow's
/// `precompute_coeffs`: the kernel widened by the reduction, cut at the
/// edges and renormalised).
pub fn coefficients(input: u32, output: u32) -> Vec<(u32, Vec<f32>)> {
    let n = unsafe { sys::negs_fit_coefficients(input, output, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), 0) };
    let mut starts = vec![0u32; output as usize];
    let mut counts = vec![0u32; output as usize];
    let mut weights = vec![0f32; n.max(1)];
    unsafe { sys::negs_fit_coefficients(input, output, starts.as_mut_ptr(), counts.as_mut_ptr(), weights.as_mut_ptr(), weights.len()) };
    let mut at = 0;
    starts
        .into_iter()
        .zip(counts)
        .map(|(start, count)| {
            let w = weights[at..at + count as usize].to_vec();
            at += count as usize;
            (start, w)
        })
        .collect()
}

/// Resamples sRGB RGBA `w` × `h` to `tw` × `th` in linear light: across,
/// then down, as Pillow does, in floats between. Opaque out.
pub fn resample(rgba: &[u8], w: u32, h: u32, tw: u32, th: u32) -> Vec<u8> {
    assert!(tw > 0 && th > 0, "exact_fit::resample: a zero target");
    assert!(rgba.len() >= w as usize * h as usize * 4, "exact_fit::resample: {} bytes for {w} × {h}", rgba.len());
    let mut out = vec![0u8; tw as usize * th as usize * 4];
    let ok = unsafe { sys::negs_fit_resample(rgba.as_ptr(), rgba.len(), w, h, tw, th, out.as_mut_ptr(), out.len()) };
    assert!(ok != 0, "exact_fit::resample failed");
    out
}

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
        assert!(!worth_it(0.3, (1200, 800), (1553, 1035)));
    }

    #[test]
    fn coefficients_follow_pillow() {
        let c = coefficients(100, 50);
        let (start, weights) = &c[25];
        assert_eq!(*start, 45);
        assert_eq!(weights.len(), 12);
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        for k in 0..6 {
            assert!((weights[k] - weights[11 - k]).abs() < 1e-6);
        }
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
        let stripes: Vec<u8> = (0..h).flat_map(|_| (0..w).flat_map(|x| if (x / 3) % 2 == 0 { [255, 255, 255, 255] } else { [0, 0, 0, 255] })).collect();
        let out = resample(&stripes, w as u32, h as u32, 58, 37);
        let row: Vec<u8> = out[12 * 58 * 4..13 * 58 * 4].chunks(4).map(|p| p[0]).collect();
        let (lo, hi) = (*row[4..54].iter().min().unwrap(), *row[4..54].iter().max().unwrap());
        assert!(hi > 200 && lo < 60, "{lo}..{hi}");
    }
}
