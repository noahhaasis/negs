//! windows/src/thumb_compose.rs's own rules on the shared Odin core
//! (core/src/rules/compose). The module itself stays in windows/src: it is
//! the engine's crop and border (`spektrafilm-io`), `image`'s resizing and
//! turns, and the edit state (Phase 4's) around two rules, which are here.

use negs_core_sys::picture as sys;

/// The size a `width` × `height` preview is reduced to before the crop
/// `crop_rect` (`width` × `height` fractions) is cut from it for a
/// `long_side` thumbnail: about twice what the crop needs. None when it is
/// small enough already.
pub fn reduced_size(width: u32, height: u32, crop_rect: (f64, f64), long_side: u32) -> Option<(u32, u32)> {
    let mut out = [0u32; 2];
    (unsafe { sys::negs_compose_reduced_size(width, height, crop_rect.0, crop_rect.1, long_side, &mut out) } != 0).then_some((out[0], out[1]))
}

/// RGBA pixels with Rec. 2020 primaries to sRGB's, both on sRGB's curve,
/// clipped, in place. `m` is the row-major matrix (the GPU's
/// `REC2020_TO_SRGB`).
pub fn to_srgb(rgba: &mut [u8], m: &[[f32; 3]; 3]) {
    unsafe { sys::negs_compose_to_srgb(rgba.as_mut_ptr(), rgba.len(), m) };
}
