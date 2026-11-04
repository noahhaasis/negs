//! The reference for Prep › Match camera brightness (`CameraBrightnessReference.swift`):
//! a RAW's embedded camera preview, never a new development, or a JPEG, PNG
//! or TIFF's own pixels, as a small sRGB PNG the helper's `place_exposure`
//! fits the developed photograph's brightness to. The platform-neutral part
//! is `spektrafilm_io::brightness`, which the browser shares.

use std::path::Path;

use anyhow::Result;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::core::{Interface, PCWSTR};

use crate::platform::wide;
pub use spektrafilm_io::brightness::{failure, same_shape};
#[cfg(test)]
pub use spektrafilm_io::brightness::Unusable;
use spektrafilm_io::brightness::{LONG_SIDE, unusable};

/// A RAW's embedded preview, oriented, as the reference for a photograph of
/// `expected` size (the helper's decoded source). The camera must say it is
/// sRGB: the preview is only a brightness reference, and its colorimetry
/// has to be known.
pub fn camera_reference(path: &Path, expected: (u32, u32)) -> Result<Vec<u8>> {
    let file = std::fs::read(path)?;
    encoded(spektrafilm_io::brightness::camera_preview(&file, expected)?, "The camera preview couldn’t be read.")
}

/// A JPEG, PNG or TIFF as its own brightness reference: the file is what the
/// camera (or the editor before Negs) made, so the developed photo should be
/// about as bright. Read through WIC with its colour profile, untagged as
/// sRGB, as the engine's Automatic reading does. Transparent and
/// floating-point files are refused: the engine drops alpha and reads
/// untagged floats as linear, which WIC doesn't.
pub fn file_reference(path: &Path, expected: (u32, u32)) -> Result<Vec<u8>> {
    const UNREADABLE: &str = "The photograph couldn’t be read for matching.";
    match pixel_kind(path) {
        Some(Kind { alpha: true, .. }) => return unusable("Transparent images can’t be matched."),
        Some(Kind { float: true, .. }) => return unusable("Floating-point images can’t be matched."),
        Some(_) => {}
        None => return unusable(UNREADABLE),
    }
    let Ok(decoded) = crate::thumbnail::wic(path, LONG_SIDE, true) else { return unusable(UNREADABLE) };
    if decoded.width < 2 || decoded.height < 2 || expected.0 == 0 || expected.1 == 0 {
        return unusable(UNREADABLE);
    }
    if !same_shape(decoded.width as f64 / decoded.height as f64, expected.0 as f64 / expected.1 as f64) {
        return unusable("The photograph’s orientation couldn’t be matched.");
    }
    let Some(rgba) = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba) else { return unusable(UNREADABLE) };
    encoded(spektrafilm_io::brightness::reduced(rgba), UNREADABLE)
}

/// An opaque 8-bit sRGB PNG, as the helper's reference.
fn encoded(rgb: image::RgbImage, unreadable: &str) -> Result<Vec<u8>> {
    let mut png = Vec::new();
    if image::DynamicImage::ImageRgb8(rgb).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
        return unusable(unreadable);
    }
    Ok(png)
}

struct Kind {
    alpha: bool,
    float: bool,
}

/// Whether the file's first frame has transparency or floating-point values.
fn pixel_kind(path: &Path) -> Option<Kind> {
    crate::thumbnail::com_thread();
    unsafe {
        let factory = crate::thumbnail::factory().ok()?;
        let name = wide(path);
        let decoder = factory.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        let format = decoder.GetFrame(0).ok()?.GetPixelFormat().ok()?;
        let info: IWICPixelFormatInfo2 = factory.CreateComponentInfo(&format).ok()?.cast().ok()?;
        let alpha = info.SupportsTransparency().ok()?.as_bool();
        let representation = info.GetNumericRepresentation().ok()?;
        let float = representation == WICPixelFormatNumericRepresentationFloat || representation == WICPixelFormatNumericRepresentationFixed;
        Some(Kind { alpha, float })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PNG has no camera preview; a plain one is its own reference, and a
    /// transparent one is refused.
    #[test]
    fn files_are_their_own_reference() {
        let dir = std::env::temp_dir().join(format!("negs-brightness-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let opaque = dir.join("opaque.png");
        image::RgbImage::from_fn(900, 600, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])).save(&opaque).unwrap();
        assert!(camera_reference(&opaque, (900, 600)).is_err());
        let png = file_reference(&opaque, (900, 600)).unwrap();
        let reference = image::load_from_memory(&png).unwrap();
        assert_eq!((reference.width(), reference.height()), (512, 341));
        let turned = file_reference(&opaque, (600, 900)).unwrap_err();
        assert_eq!(failure(&turned).0, "The photograph’s orientation couldn’t be matched.");
        let clear = dir.join("clear.png");
        image::RgbaImage::from_pixel(64, 64, image::Rgba([10, 20, 30, 0])).save(&clear).unwrap();
        assert_eq!(failure(&file_reference(&clear, (64, 64)).unwrap_err()), ("Transparent images can’t be matched.".into(), true));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

