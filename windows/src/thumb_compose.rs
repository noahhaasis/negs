//! An edited thumbnail as the edit view shows the photograph
//! (`ThumbnailComposer`): cropped and straightened, turned, and inside its
//! border, through `spektrafilm-io`'s own crop and border, the exporter's.
//! Either from the helper's thumbnail (cropped already), or from the open
//! photograph's settled preview as it's left, so filing its thumbnail
//! costs no develop.
//!
//! The two rules of its own, the reduced size and the Rec. 2020 to sRGB
//! conversion, are the shared core's (negs_core_rules::thumb_compose);
//! the rest is the engine's crop and border and `image`'s resizing.

use std::path::PathBuf;

use image::{RgbaImage, imageops};
use negs_core_rules::thumb_compose as rules;
use spektrafilm_io::border::{BorderOptions, Rgb, add_border, extend_for_instagram};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};

use crate::border::BorderSettings;
use crate::crop::CropSettings;

/// How a cropped photograph becomes its thumbnail: its quarter turns and
/// its border (with the film whose 35mm strip it may print, and the data
/// folder the border textures are in).
#[derive(Clone, Debug)]
pub struct Finish {
    pub rotation: i32,
    /// Mirrored left to right before the turns (`io.flip`).
    pub flip: bool,
    pub border: Option<BorderSettings>,
    pub film: String,
    pub data: Option<PathBuf>,
}

impl Finish {
    pub fn of(state: &crate::state::EditState, data: Option<PathBuf>) -> Finish {
        Finish { rotation: state.rotation(), flip: state.flip(), border: state.border.enabled.then_some(state.border), film: state.film.clone(), data }
    }
}

/// `image`, cropped and unturned in sRGB, turned and put inside its border
/// (when the border fits it, as the canvas shows it).
pub fn finish(image: RgbaImage, finish: &Finish) -> RgbaImage {
    let image = if finish.flip { imageops::flip_horizontal(&image) } else { image };
    let image = match finish.rotation.rem_euclid(4) {
        1 => imageops::rotate90(&image),
        2 => imageops::rotate180(&image),
        3 => imageops::rotate270(&image),
        _ => image,
    };
    let (Some(border), Some(data)) = (finish.border, finish.data.as_ref()) else { return image };
    let (w, h) = image.dimensions();
    let Some(border) = border.shown(w, h) else { return image };
    let options = BorderOptions { width: border.style_width(), line: border.line, color: Rgb(border.color.0), line_color: Rgb(border.line_color.0) };
    let buffer = buffer(&image);
    let Ok(mut bordered) = add_border(&buffer, border.style, &options, &finish.film, true, data) else { return image };
    if border.instagram {
        bordered = extend_for_instagram(&bordered, border.style, &options, true);
    }
    rgba(&bordered)
}

/// The thumbnail from a settled preview: `pixels` (RGBA) uncropped, `wide` when its
/// primaries are Rec. 2020 (sRGB's curve either way). Reduced first to
/// about twice what the crop needs, then cropped as the exporter crops,
/// turned and bordered. None for an empty preview.
pub fn from_preview(pixels: &[u8], width: u32, height: u32, wide: bool, crop: &CropSettings, finish_with: &Finish, long_side: u32) -> Option<RgbaImage> {
    let image = RgbaImage::from_raw(width, height, pixels.get(..width as usize * height as usize * 4)?.to_vec())?;
    // The size is the core's rule (negs_core_rules::thumb_compose), the
    // resizing `image`'s.
    let mut image = match rules::reduced_size(width, height, (crop.rect.width, crop.rect.height), long_side) {
        Some((w, h)) => imageops::resize(&image, w, h, imageops::FilterType::Triangle),
        None => image,
    };
    if wide {
        // Rec. 2020 primaries to sRGB's, both on sRGB's curve, clipped.
        rules::to_srgb(&mut image, &crate::gpu::REC2020_TO_SRGB);
    }
    let cropped = if crop.cropped() { rgba(&crop.engine().apply(&buffer(&image))) } else { image };
    Some(finish(cropped, finish_with))
}

fn buffer(image: &RgbaImage) -> ImageBuf {
    let (w, h) = image.dimensions();
    let data = image.pixels().flat_map(|p| [p[0], p[1], p[2]]).map(|v| from_f32(v as f32 / 255.0)).collect();
    ImageBuf::from_data(w, h, data)
}

fn rgba(buffer: &ImageBuf) -> RgbaImage {
    let pixels = buffer.data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2]].map(|v| (to_f32(v).clamp(0.0, 1.0) * 255.0).round() as u8).into_iter().chain([255])).collect();
    RgbaImage::from_raw(buffer.width, buffer.height, pixels).expect("buffer size")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::border::BorderStyle;

    fn grey(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([128, 128, 128, 255]))
    }

    #[test]
    fn a_thumbnail_turns_as_the_photograph_does() {
        let finished = finish(grey(30, 20), &Finish { rotation: 1, flip: false, border: None, film: String::new(), data: None });
        assert_eq!(finished.dimensions(), (20, 30));
    }

    #[test]
    fn a_solid_border_goes_round_the_thumbnail_in_its_colour() {
        let border = BorderSettings { enabled: true, style: BorderStyle::Solid, color: crate::border::BorderColor([255, 0, 0]), ..Default::default() };
        let finished = finish(grey(300, 200), &Finish { rotation: 0, flip: false, border: Some(border), film: String::new(), data: Some(PathBuf::from(".")) });
        // 3 % of the long edge on every side.
        assert_eq!(finished.dimensions(), (318, 218));
        assert_eq!(finished.get_pixel(0, 0).0, [255, 0, 0, 255]);
        assert_eq!(finished.get_pixel(159, 109).0, [128, 128, 128, 255]);
    }

    #[test]
    fn a_preview_is_cropped_as_the_exporter_crops() {
        let mut image = grey(400, 200);
        for x in 0..200 {
            for y in 0..200 {
                image.put_pixel(x, y, image::Rgba([10, 20, 30, 255]));
            }
        }
        let crop = CropSettings { rect: crate::crop::Rect::new(0.0, 0.0, 0.5, 1.0), ..Default::default() };
        let plain = Finish { rotation: 0, flip: false, border: None, film: String::new(), data: None };
        let thumbnail = from_preview(image.as_raw(), 400, 200, false, &crop, &plain, 512).unwrap();
        assert_eq!(thumbnail.dimensions(), (200, 200));
        assert_eq!(thumbnail.get_pixel(100, 100).0, [10, 20, 30, 255]);
    }

    #[test]
    fn a_wide_preview_is_brought_to_srgb() {
        let white = RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        let plain = Finish { rotation: 0, flip: false, border: None, film: String::new(), data: None };
        let thumbnail = from_preview(white.as_raw(), 4, 4, true, &CropSettings::default(), &plain, 512).unwrap();
        // White stays white; a saturated Rec. 2020 green clips in sRGB.
        assert_eq!(thumbnail.get_pixel(0, 0).0, [255, 255, 255, 255]);
    }
}
