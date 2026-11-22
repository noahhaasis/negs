//! windows/src/model/compare.rs's pure helpers on the shared Odin core
//! (core/src/rules/compare/pixels.odin): halving a large develop, and
//! laying the camera's JPEG over the RAW's frame. The rest of that module
//! is the model's frame pipeline (Phase 5) and stays in windows/.
//!
//! What runs where: `halved`'s pixels and `over_frame`'s geometry in Odin;
//! `over_frame`'s filtered resize and overlay are the image crate's and
//! stay here.

use negs_core_sys::sheet as sys;

/// `image` at half its size, each pixel the mean of four.
pub fn halved<C: std::ops::Deref<Target = [u8]>>(image: &image::ImageBuffer<image::Rgba<u8>, C>) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    // As the Rust it replaces: an empty image has no pixel to repeat.
    assert!(width > 0 && height > 0, "halving an empty image");
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let source: &[u8] = image.as_raw();
    assert!(source.len() >= width as usize * height as usize * 4, "an image holds its pixels");
    let mut out = vec![0u8; w as usize * h as usize * 4];
    unsafe { sys::negs_compare_halved(source.as_ptr(), width, height, out.as_mut_ptr()) };
    image::RgbaImage::from_raw(w, h, out).expect("sized")
}

/// The camera's JPEG laid over the RAW's frame (`width` × `height`), so it
/// lines up with the developed image: one of the same shape (within 1%) is
/// stretched to it, another sits centred on black.
pub fn over_frame(rgba: &[u8], width: u32, height: u32, frame: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    let picture = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let mut plan = sys::negs_compare_over_frame_t::default();
    unsafe { sys::negs_compare_over_frame(width, height, frame.0, frame.1, &mut plan) };
    let out = (plan.out[0], plan.out[1]);
    if plan.same != 0 {
        let resized = image::imageops::resize(&picture, out.0.max(1), out.1.max(1), image::imageops::FilterType::Triangle);
        return Some((resized.into_raw(), out.0, out.1));
    }
    let (w, h) = (plan.size[0], plan.size[1]);
    let resized = image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::from_pixel(out.0.max(1), out.1.max(1), image::Rgba([0, 0, 0, 255]));
    image::imageops::overlay(&mut canvas, &resized, plan.at[0], plan.at[1]);
    Some((canvas.into_raw(), out.0, out.1))
}
