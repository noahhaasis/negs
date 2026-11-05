//! The reference for Prep › Match camera brightness, wherever ImageIO
//! doesn't make it (macOS: `CameraBrightnessReference.swift`): a RAW's
//! embedded camera preview, never a new development, as a small sRGB image
//! that `exposure_placement::place_exposure` fits the developed photograph's
//! brightness to. Windows and the browser share it.

use anyhow::Result;
use spektrafilm_math::image::ImageBuf;
use std::path::Path;

/// Why the photograph can't serve as its own reference. Retrying the same
/// photograph gives the same answer, so the app stops offering it.
#[derive(Debug, Clone, PartialEq)]
pub struct Unusable(pub String);

impl std::fmt::Display for Unusable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Unusable {}

pub fn unusable<T>(message: &str) -> Result<T> {
    Err(Unusable(message.into()).into())
}

/// The longest side of the reference, in pixels.
pub const LONG_SIDE: u32 = 512;

pub fn same_shape(aspect: f64, expected: f64) -> bool {
    aspect.max(expected) / aspect.min(expected) <= 1.02
}

/// The original photograph as a brightness reference, independent of its
/// current editing/decode state. RAW uses only the camera's embedded preview;
/// rendered files use their original profile and orientation, never an input
/// interpretation override or a new RAW development.
pub fn reference(path: &Path, expected: (u32, u32)) -> Result<image::RgbImage> {
    if crate::is_raw(path) {
        camera_preview(&std::fs::read(path)?, expected)
    } else {
        file_reference(path, expected)
    }
}

/// A JPEG, PNG or TIFF's own pixels as an opaque, small encoded-sRGB
/// reference. Decode/profile/orientation handling is shared with the input
/// loader. Alpha and floating-point files have no unambiguous display-referred
/// reference and are refused, as in the macOS and Windows clients.
pub fn file_reference(path: &Path, expected: (u32, u32)) -> Result<image::RgbImage> {
    let (mut decoded, info, format) = crate::decode_no_limits(path)?;
    if !matches!(format, image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::Tiff) {
        return unusable("The photograph couldn’t be read for matching.");
    }
    if decoded.color().has_alpha() {
        return unusable("Transparent images can’t be matched.");
    }
    if info.float {
        return unusable("Floating-point images can’t be matched.");
    }
    let (width, height) = (decoded.width(), decoded.height());
    if width < 2 || height < 2 || expected.0 == 0 || expected.1 == 0 {
        return unusable("The photograph couldn’t be read for matching.");
    }
    if !same_shape(width as f64 / height as f64, expected.0 as f64 / expected.1 as f64) {
        return unusable("The photograph’s orientation couldn’t be matched.");
    }
    // Keep profile conversion bounded to the small reference, retaining the
    // original sample depth. Like camera-preview reduction this resamples
    // stored display values, before converting their colour encoding.
    if width.max(height) > LONG_SIDE {
        decoded = decoded.resize(LONG_SIDE, LONG_SIDE, image::imageops::FilterType::Triangle);
    }
    let (linear, _) = crate::colour::convert(decoded, &info, &Default::default(), format == image::ImageFormat::Png)
        .map_err(|_| Unusable("The photograph couldn’t be read for matching.".into()))?;
    use spektrafilm_math::precision::{srgb_encode, to_f64};
    let pixels = linear.data.iter().map(|&value| {
        (to_f64(srgb_encode(value)).clamp(0.0, 1.0) * 255.0).round() as u8
    }).collect();
    image::RgbImage::from_raw(linear.width, linear.height, pixels)
        .ok_or_else(|| anyhow::anyhow!("The brightness reference has invalid dimensions"))
}

/// Write encoded-sRGB reference samples with an explicit sRGB profile. The
/// helper's colour-managed reference reader therefore sees the same light.
pub fn write_reference(rgb: &image::RgbImage, path: &Path) -> Result<()> {
    use image::ImageEncoder;
    use std::io::Write;
    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut encoder = image::codecs::png::PngEncoder::new(&mut file);
    encoder.set_icc_profile(crate::icc::profile(&spektrafilm_math::colorspace::SRGB, false))?;
    encoder.write_image(rgb.as_raw(), rgb.width(), rgb.height(), image::ExtendedColorType::Rgb8)?;
    file.flush()?;
    Ok(())
}

/// A RAW's embedded preview in `file`, oriented, trimmed of letterbox bars
/// and reduced to [`LONG_SIDE`], as the reference for a photograph of
/// `expected` size (its decoded source). An Adobe RGB preview is converted;
/// one that says nothing is read as sRGB, as on every platform (cameras and
/// Lightroom make sRGB previews unless set otherwise); one in another declared
/// colour space (a DNG's ProPhoto or gray gamma 2.2) is refused.
pub fn camera_preview(file: &[u8], expected: (u32, u32)) -> Result<image::RgbImage> {
    let source = rawler::rawsource::RawSource::new_from_slice(file);
    let Ok(decoder) = rawler::get_decoder(&source) else {
        return unusable("This RAW has no camera preview to match.");
    };
    let params = rawler::decoders::RawDecodeParams::default();
    let preview = decoder.preview_image(&source, &params).ok().flatten();
    let from_thumbnail = preview.is_none();
    let Some(image) = preview.or_else(|| decoder.thumbnail_image(&source, &params).ok().flatten()) else {
        return unusable("This RAW has no camera preview to match.");
    };
    let metadata = decoder.raw_metadata(&source, &params).ok();
    let orientation = metadata.as_ref().and_then(|m| m.exif.orientation).unwrap_or(1);
    let image = match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    };
    let (width, height) = (image.width(), image.height());
    if width < 64 || height < 64 || expected.0 == 0 || expected.1 == 0 {
        return unusable("The camera preview is too small to match.");
    }
    let declared = declared_for(file, from_thumbnail, metadata.as_ref().and_then(|m| m.exif.color_space));
    let mut rgba = image.to_rgba8();
    drop(image);
    match declared {
        Some(Declared::Srgb) | None => {}
        Some(Declared::AdobeRgb) => adobe_to_srgb(&mut rgba),
        Some(Declared::Other) => return unusable("The camera preview’s colour space isn’t supported."),
    }
    let wanted = expected.0 as f64 / expected.1 as f64;
    if !same_shape(width as f64 / height as f64, wanted) {
        // Only previews of another shape pay for the bar search.
        let gray: Vec<u8> = rgba.pixels().map(|p| luma(p.0)).collect();
        let Some((x, y, w, h)) = trimming_bars(&gray, width as usize, height as usize, wanted) else {
            return unusable("The camera preview is framed differently from the photo.");
        };
        rgba = image::imageops::crop_imm(&rgba, x as u32, y as u32, w as u32, h as u32).to_image();
    }
    Ok(reduced(rgba))
}

/// At most [`LONG_SIDE`], opaque 8-bit sRGB, as the helper's reference PNG
/// holds it.
pub fn reduced(rgba: image::RgbaImage) -> image::RgbImage {
    let (w, h) = rgba.dimensions();
    let scale = (LONG_SIDE as f64 / w.max(h) as f64).min(1.0);
    let (sw, sh) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    if (sw, sh) == (w, h) { rgb } else { image::imageops::resize(&rgb, sw, sh, image::imageops::FilterType::Triangle) }
}

/// The reference in linear sRGB, as `place_exposure` takes it: what the
/// helper's loader makes of the reference PNG (untagged, so sRGB).
pub fn linear(rgb: image::RgbImage) -> Result<ImageBuf> {
    let info = crate::colour::FileColour::default();
    let (image, _) = crate::colour::convert(image::DynamicImage::ImageRgb8(rgb), &info, &Default::default(), true)?;
    Ok(image)
}

/// The colour space the preview [`camera_preview`] takes from `file` says
/// it's in, for the Mac, whose ImageIO reads the same preview's pixels: the
/// same rule on every platform. None when nothing says (read as sRGB) or
/// when `file` has no camera preview.
pub fn preview_colour_space(file: &[u8]) -> Option<Declared> {
    let source = rawler::rawsource::RawSource::new_from_slice(file);
    let decoder = rawler::get_decoder(&source).ok()?;
    let params = rawler::decoders::RawDecodeParams::default();
    let from_thumbnail = decoder.preview_image(&source, &params).ok().flatten().is_none();
    let exif = decoder.raw_metadata(&source, &params).ok().and_then(|m| m.exif.color_space);
    declared_for(file, from_thumbnail, exif)
}

/// The camera's own declaration for its preview, if any (`exif`: the
/// decoder's EXIF ColorSpace). A DNG's preview says first: Lightroom and DNG
/// Converter render it themselves, whatever the camera was set to.
fn declared_for(file: &[u8], from_thumbnail: bool, exif: Option<u16>) -> Option<Declared> {
    dng_preview_colour_space(file, from_thumbnail).or_else(|| match exif {
        Some(1) => Some(Declared::Srgb),
        _ => declared_colour_space(file),
    })
}

/// The colour space a camera says its JPEGs and previews are in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Declared {
    Srgb,
    AdobeRgb,
    /// Another colour space, such as a DNG preview's ProPhoto: not to be
    /// read as sRGB.
    Other,
}

/// The camera's declared colour space, from the RAW's EXIF (ColorSpace), the
/// maker note (Nikon, Canon and Pentax keep it there), or the EXIF of a JPEG
/// embedded in the file. None when nothing says.
pub fn declared_colour_space(file: &[u8]) -> Option<Declared> {
    let exif = |tiff: &Tiff, ifd0: usize| tiff.entry(ifd0, 0x8769).and_then(|e| tiff.long(e)).map(|o| o as usize);
    let standard = |tiff: &Tiff, ifd: usize| match tiff.entry(ifd, 0xA001).and_then(|e| tiff.short(e)) {
        Some(1) => Some(Declared::Srgb),
        Some(2) => Some(Declared::AdobeRgb),
        _ => None,
    };
    if let Some(tiff) = Tiff::new(file, 0)
        && let Some(ifd0) = tiff.first()
        && let Some(exif) = exif(&tiff, ifd0)
    {
        if let Some(found) = standard(&tiff, exif) {
            return Some(found);
        }
        if let Some(found) = tiff.entry(exif, 0x927C).and_then(|note| maker_note(&tiff, ifd0, note)) {
            return Some(found);
        }
    }
    // An embedded JPEG's own EXIF, as in Fujifilm and Panasonic files.
    let mut at = 0;
    while let Some(found) = find(&file[at..], b"Exif\0\0") {
        let start = at + found + 6;
        if let Some(tiff) = Tiff::new(file, start)
            && let Some(ifd0) = tiff.first()
            && let Some(declared) = exif(&tiff, ifd0).and_then(|e| standard(&tiff, e))
        {
            return Some(declared);
        }
        at = start;
    }
    None
}

/// The colour space a DNG says its camera preview is in: PreviewColorSpace
/// (0xC71A) in the preview's own IFD, as Lightroom and Adobe DNG Converter
/// write it. The IFD is the one rawler's DNG decoder takes: the first SubIFD
/// marked as a reduced-resolution image (`preview_image`), or else IFD0 when
/// it is one (`thumbnail_image`, `from_thumbnail`). Gray gamma 2.2, ProPhoto
/// and later values are [`Declared::Other`]; None for other files and for
/// Unknown (0).
pub fn dng_preview_colour_space(file: &[u8], from_thumbnail: bool) -> Option<Declared> {
    let tiff = Tiff::new(file, 0)?;
    let ifd0 = tiff.first()?;
    tiff.entry(ifd0, 0xC612)?; // DNGVersion: rawler's DNG decoder read it
    let reduced = |ifd: usize| tiff.entry(ifd, 0x00FE).and_then(|e| tiff.number(e)) == Some(1);
    let ifd = if from_thumbnail {
        Some(ifd0).filter(|&ifd| reduced(ifd))
    } else {
        let (offset, length) = tiff.value_offset(tiff.entry(ifd0, 0x014A)?)?;
        let at = offset - tiff.base;
        (0..length / 4).filter_map(|i| tiff.u32(at + 4 * i)).map(|o| o as usize).find(|&ifd| reduced(ifd))
    }?;
    match tiff.entry(ifd, 0xC71A).and_then(|e| tiff.number(e))? {
        0 => None,
        2 => Some(Declared::Srgb),
        3 => Some(Declared::AdobeRgb),
        _ => Some(Declared::Other),
    }
}

fn maker_note(tiff: &Tiff, ifd0: usize, note: usize) -> Option<Declared> {
    let (offset, count) = tiff.value_offset(note)?;
    let data = tiff.data.get(offset..offset + count)?;
    let make = tiff.entry(ifd0, 0x010F).and_then(|e| tiff.value_offset(e)).and_then(|(o, n)| tiff.data.get(o..o + n)).unwrap_or_default();
    if data.starts_with(b"Nikon\0") {
        // Nikon type 3: its own TIFF header 10 bytes in. ColorSpace is 0x001E.
        let inner = Tiff::new(tiff.data, offset + 10)?;
        return match inner.entry(inner.first()?, 0x001E).and_then(|e| inner.short(e))? {
            1 => Some(Declared::Srgb),
            2 => Some(Declared::AdobeRgb),
            _ => None,
        };
    }
    // An IFD at the note itself, with offsets in the file's TIFF.
    let ifd = offset.checked_sub(tiff.base)?;
    let (tag, srgb, adobe) = if make.starts_with(b"Canon") {
        (0x00B4, 1, 2)
    } else if make.starts_with(b"PENTAX") || make.starts_with(b"RICOH") || make.starts_with(b"Asahi") {
        (0x0037, 0, 1)
    } else {
        return None;
    };
    match tiff.entry(ifd, tag).and_then(|e| tiff.short(e))? {
        v if v == srgb => Some(Declared::Srgb),
        v if v == adobe => Some(Declared::AdobeRgb),
        _ => None,
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A TIFF structure in `data` starting at `base`, bounds-checked throughout.
/// Accepts the variants RAW files use (ORF's IIRO and MMOR, RW2's IIU).
struct Tiff<'a> {
    data: &'a [u8],
    base: usize,
    little: bool,
}

impl<'a> Tiff<'a> {
    fn new(data: &'a [u8], base: usize) -> Option<Self> {
        let little = match data.get(base..base.checked_add(2)?)? {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };
        let tiff = Tiff { data, base, little };
        matches!(tiff.u16(2)?, 42 | 0x4F52 | 0x5352 | 0x55).then_some(tiff)
    }
    fn bytes<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        let start = self.base.checked_add(at)?;
        self.data.get(start..start.checked_add(N)?)?.try_into().ok()
    }
    fn u16(&self, at: usize) -> Option<u16> {
        let b = self.bytes::<2>(at)?;
        Some(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let b = self.bytes::<4>(at)?;
        Some(if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }
    fn first(&self) -> Option<usize> {
        self.u32(4).map(|o| o as usize)
    }
    /// The entry for `tag` in the IFD at `ifd` (relative to the base).
    fn entry(&self, ifd: usize, tag: u16) -> Option<usize> {
        let count = self.u16(ifd)? as usize;
        (0..count.min(1024)).map(|i| ifd + 2 + 12 * i).find(|&e| self.u16(e) == Some(tag))
    }
    fn short(&self, entry: usize) -> Option<u16> {
        if self.u16(entry + 2)? == 3 { self.u16(entry + 8) } else { None }
    }
    fn long(&self, entry: usize) -> Option<u32> {
        match self.u16(entry + 2)? {
            4 | 13 => self.u32(entry + 8),
            _ => None,
        }
    }
    /// A short or long entry's value, as the TIFF spec lets either hold it.
    fn number(&self, entry: usize) -> Option<u32> {
        self.long(entry).or_else(|| self.short(entry).map(u32::from))
    }
    /// An entry's bytes: their absolute offset in `data` and their length.
    fn value_offset(&self, entry: usize) -> Option<(usize, usize)> {
        let size = match self.u16(entry + 2)? {
            1 | 2 | 6 | 7 => 1,
            3 | 8 => 2,
            4 | 9 | 11 | 13 => 4,
            5 | 10 | 12 => 8,
            _ => return None,
        };
        let length = size * self.u32(entry + 4)? as usize;
        let offset = if length <= 4 { entry + 8 } else { self.u32(entry + 8)? as usize };
        let absolute = self.base.checked_add(offset)?;
        (absolute.checked_add(length)? <= self.data.len()).then_some((absolute, length))
    }
}

/// Converts Adobe RGB (1998) pixels to sRGB, clipping what sRGB can't hold:
/// the reference only has to be as bright as the camera's preview.
pub fn adobe_to_srgb(rgba: &mut image::RgbaImage) {
    const M: [[f64; 3]; 3] = [[1.3982832, -0.3982831, 0.0], [0.0, 1.0, 0.0], [0.0, -0.0429383, 1.0429383]];
    let decode: Vec<f64> = (0..256).map(|v| (v as f64 / 255.0).powf(563.0 / 256.0)).collect();
    let encode = |v: f64| {
        let v = v.clamp(0.0, 1.0);
        let e = if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as u8
    };
    for p in rgba.pixels_mut() {
        let linear = [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]];
        for c in 0..3 {
            p[c] = encode(M[c][0] * linear[0] + M[c][1] * linear[1] + M[c][2] * linear[2]);
        }
    }
}

pub fn luma(p: [u8; 4]) -> u8 {
    (0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64).round() as u8
}

/// The picture inside black bars, when that has the photo's shape, as
/// x, y, width, height in `gray` (row 0 at the top). Some cameras letterbox a
/// 4:3 preview of their 3:2 frame: the Leica M8 with 13 rows top and bottom
/// (320×240), the M (Typ 240) and Monochrom (Typ 246) with 14 rows at the top
/// only (160×120); also the Sony A100, Minolta Dynax 7D and Nikon D1. Each
/// edge is searched on its own, so one-sided bars work; a dark scene edge
/// that isn't a bar leaves the wrong shape and is refused like any other crop.
pub fn trimming_bars(gray: &[u8], width: usize, height: usize, expected: f64) -> Option<(usize, usize, usize, usize)> {
    if width == 0 || height == 0 || gray.len() < width * height {
        return None;
    }
    // Bars measure 0–4 of 255; 24 leaves room for JPEG noise without taking
    // in shadows with detail.
    const BLACK: u8 = 24;
    let bar_row = |y: usize| gray[y * width..(y + 1) * width].iter().all(|&v| v <= BLACK);
    let (mut top, mut bottom) = (0, height);
    while top < bottom && bar_row(top) {
        top += 1;
    }
    while bottom > top && bar_row(bottom - 1) {
        bottom -= 1;
    }
    let bar_column = |x: usize| (top..bottom).all(|y| gray[y * width + x] <= BLACK);
    let (mut left, mut right) = (0, width);
    while left < right && bar_column(left) {
        left += 1;
    }
    while right > left && bar_column(right - 1) {
        right -= 1;
    }
    let (w, h) = (right - left, bottom - top);
    let shrunk = w < width || h < height;
    (shrunk && w >= 64 && h >= 64 && same_shape(w as f64 / h as f64, expected)).then_some((left, top, w, h))
}

/// The note under the switch for a failed match, and whether it holds for
/// the photograph (the reference is the problem) or until the next edit. The
/// engine's reasons are worded for logs, so they're matched on phrases from
/// `exposure_placement.rs` and never shown as they are.
pub fn failure(error: &anyhow::Error) -> (String, bool) {
    if let Some(Unusable(message)) = error.downcast_ref::<Unusable>() {
        return (message.clone(), true);
    }
    failure_reason(&format!("{error:#}"))
}

/// [`failure`] of an engine reason as text, as a browser receives it.
pub fn failure_reason(reason: &str) -> (String, bool) {
    if reason.contains("midtone quantiles") {
        return ("The camera preview is too dark or too bright to match.".into(), true);
    }
    if reason.contains("±3 EV") {
        return ("Matching would take more than ±3 EV.".into(), false);
    }
    ("No reliable brightness match with these settings.".into(), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct ReferenceFiles(std::path::PathBuf);
    impl ReferenceFiles {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("negs-brightness-{name}-{}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
        fn path(&self, name: &str) -> std::path::PathBuf { self.0.join(name) }
    }
    impl Drop for ReferenceFiles {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); }
    }

    #[test]
    fn original_file_reference_obeys_its_profile_and_round_trips_as_srgb() {
        let files = ReferenceFiles::new("profile");
        let source = files.path("adobe.png");
        let pixels = ImageBuf::from_data(4, 2, [0.2, 0.45, 0.7].repeat(8));
        let options = crate::save::SaveOptions { color_space: "Adobe RGB (1998)".into(), ..Default::default() };
        crate::save::save_image_with_options(&pixels, &source, None, &options).unwrap();
        let rgb = file_reference(&source, (4, 2)).unwrap();
        assert_eq!(rgb.dimensions(), (4, 2));
        assert_ne!(rgb.get_pixel(0, 0).0, [51, 115, 179], "profile conversion must not pass stored Adobe RGB through as sRGB");
        let output = files.path("reference.png");
        write_reference(&rgb, &output).unwrap();
        let expected = crate::load_image(&source, crate::Formats::Any).unwrap();
        let roundtrip = crate::load_image(&output, crate::Formats::Any).unwrap();
        for (&a, &b) in expected.data.iter().zip(&roundtrip.data) {
            // The display-referred reference clips to sRGB's gamut.
            assert!((a.clamp(0.0, 1.0) - b).abs() < 0.006, "{a} vs {b}");
        }
    }

    #[test]
    fn original_file_reference_is_bounded_and_rejects_ambiguous_pixels() {
        let files = ReferenceFiles::new("kinds");
        let input = files.path("large.png");
        image::RgbImage::from_pixel(1024, 512, image::Rgb([128, 128, 128])).save(&input).unwrap();
        let rgb = reference(&input, (1024, 512)).unwrap();
        assert_eq!(rgb.dimensions(), (512, 256));
        assert_eq!(rgb.get_pixel(0, 0).0, [128; 3], "untagged integer images are sRGB");
        assert_eq!(failure(&reference(&input, (512, 1024)).unwrap_err()),
            ("The photograph’s orientation couldn’t be matched.".into(), true));
        let alpha = files.path("alpha.png");
        image::RgbaImage::from_pixel(4, 2, image::Rgba([128, 128, 128, 255])).save(&alpha).unwrap();
        assert_eq!(failure(&reference(&alpha, (4, 2)).unwrap_err()),
            ("Transparent images can’t be matched.".into(), true));
        let float = files.path("float.tiff");
        image::DynamicImage::ImageRgb32F(image::Rgb32FImage::from_pixel(4, 2, image::Rgb([0.18; 3])))
            .save(&float).unwrap();
        assert_eq!(failure(&reference(&float, (4, 2)).unwrap_err()),
            ("Floating-point images can’t be matched.".into(), true));
    }

    #[test]
    fn original_file_reference_applies_orientation_once() {
        use image::ImageEncoder;
        let files = ReferenceFiles::new("orientation");
        let input = files.path("portrait.png");
        // TIFF orientation 6 (90 degrees clockwise), embedded in PNG's eXIf.
        let mut exif = b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec();
        let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&input).unwrap());
        encoder.set_exif_metadata(std::mem::take(&mut exif)).unwrap();
        let pixels = image::RgbImage::from_fn(4, 2, |x, y| image::Rgb([(x * 60) as u8, (y * 150) as u8, 100]));
        encoder.write_image(pixels.as_raw(), 4, 2, image::ExtendedColorType::Rgb8).unwrap();
        let rgb = file_reference(&input, (2, 4)).unwrap();
        assert_eq!(rgb.dimensions(), (2, 4));
        assert_eq!(rgb, image::imageops::rotate90(&pixels));
    }

    /// A grey test preview with a textured picture and black bars (value 2,
    /// like the Leica previews) on the given edges.
    fn preview(width: usize, height: usize, top: usize, bottom: usize, left: usize, right: usize) -> Vec<u8> {
        let mut pixels = vec![2u8; width * height];
        for y in top..height - bottom {
            for x in left..width - right {
                pixels[y * width + x] = (60 + (x * 7 + y * 13) % 150) as u8;
            }
        }
        pixels
    }

    /// The macOS app's cases (`ExposurePlacementTests`).
    #[test]
    fn letterboxed_previews_are_trimmed_to_the_picture() {
        let (m240, m8) = (5976.0 / 3992.0, 3916.0 / 2634.0);
        assert_eq!(trimming_bars(&preview(160, 120, 14, 0, 0, 0), 160, 120, m240).map(|r| (r.2, r.3)), Some((160, 106)));
        assert_eq!(trimming_bars(&preview(320, 240, 13, 13, 0, 0), 320, 240, m8).map(|r| (r.2, r.3)), Some((320, 214)));
        assert_eq!(trimming_bars(&preview(512, 341, 0, 0, 29, 28), 512, 341, 4.0 / 3.0), Some((29, 0, 455, 341)));
    }

    #[test]
    fn cropped_or_dark_edged_previews_are_not_trimmed() {
        assert_eq!(trimming_bars(&preview(160, 120, 0, 0, 0, 0), 160, 120, 1.5), None, "A real crop has no bars");
        assert_eq!(trimming_bars(&preview(160, 120, 30, 0, 0, 0), 160, 120, 1.5), None, "A dark sky leaves the wrong shape");
        assert_eq!(trimming_bars(&preview(90, 68, 4, 4, 0, 0), 90, 68, 1.5), None, "Too small to meter");
    }

    #[test]
    fn engine_reasons_become_short_notes() {
        let cases = [
            ("Camera preview has too few usable midtone quantiles; exposure unchanged", "The camera preview is too dark or too bright to match.", true),
            ("Camera brightness requires exposure outside the supported ±3 EV range", "Matching would take more than ±3 EV.", false),
            ("Selected film response is not reliably monotone for Auto exposure", "No reliable brightness match with these settings.", false),
        ];
        for (reason, message, photo) in cases {
            assert_eq!(failure(&anyhow::anyhow!(reason)), (message.to_string(), photo), "{reason}");
            assert_eq!(failure_reason(reason), (message.to_string(), photo), "{reason}");
        }
        let error: anyhow::Error = Unusable("This RAW has no camera preview to match.".into()).into();
        assert_eq!(failure(&error), ("This RAW has no camera preview to match.".into(), true));
    }

    /// A little-endian TIFF: IFD0 with Make and an Exif IFD with `exif`
    /// entries and an optional maker note.
    fn tiff(make: &[u8], exif: &[(u16, u16, u32, u32)], note: Option<Vec<u8>>) -> Vec<u8> {
        let mut d = b"II*\0\x08\0\0\0".to_vec();
        let entry = |d: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: u32| {
            d.extend(tag.to_le_bytes());
            d.extend(typ.to_le_bytes());
            d.extend(count.to_le_bytes());
            d.extend(value.to_le_bytes());
        };
        let (exif_at, make_at, note_at) = (8 + 2 + 2 * 12 + 4, 200u32, 300u32);
        d.extend(2u16.to_le_bytes());
        entry(&mut d, 0x010F, 2, make.len() as u32, make_at);
        entry(&mut d, 0x8769, 4, 1, exif_at as u32);
        d.extend(0u32.to_le_bytes());
        let mut entries = exif.to_vec();
        if let Some(note) = &note {
            entries.push((0x927C, 7, note.len() as u32, note_at));
        }
        d.extend((entries.len() as u16).to_le_bytes());
        for (tag, typ, count, value) in entries {
            entry(&mut d, tag, typ, count, value);
        }
        d.extend(0u32.to_le_bytes());
        d.resize(make_at as usize, 0);
        d.extend(make);
        d.resize(note_at as usize, 0);
        d.extend(note.unwrap_or_default());
        d
    }

    /// One IFD entry holding a short, after a two-byte count of one.
    fn short_ifd(tag: u16, value: u16) -> Vec<u8> {
        let mut d = 1u16.to_le_bytes().to_vec();
        d.extend(tag.to_le_bytes());
        d.extend(3u16.to_le_bytes());
        d.extend(1u32.to_le_bytes());
        d.extend((value as u32).to_le_bytes());
        d.extend(0u32.to_le_bytes());
        d
    }

    /// A camera's declared colour space, in the EXIF or the maker note.
    #[test]
    fn declared_colour_spaces_are_found() {
        assert_eq!(declared_colour_space(&tiff(b"SONY\0", &[(0xA001, 3, 1, 1)], None)), Some(Declared::Srgb));
        assert_eq!(declared_colour_space(&tiff(b"SONY\0", &[(0xA001, 3, 1, 2)], None)), Some(Declared::AdobeRgb));
        assert_eq!(declared_colour_space(&tiff(b"SONY\0", &[(0xA001, 3, 1, 0xFFFF)], None)), None, "Uncalibrated says nothing");
        assert_eq!(declared_colour_space(&tiff(b"SONY\0", &[], None)), None);
        // Nikon: "Nikon\0", a version, then its own TIFF with ColorSpace 0x001E.
        let mut nikon = b"Nikon\0\x02\x11\0\0II*\0\x08\0\0\0".to_vec();
        nikon.extend(short_ifd(0x001E, 2));
        assert_eq!(declared_colour_space(&tiff(b"NIKON CORPORATION\0", &[], Some(nikon))), Some(Declared::AdobeRgb));
        // Canon and Pentax: an IFD at the note.
        assert_eq!(declared_colour_space(&tiff(b"Canon\0", &[], Some(short_ifd(0x00B4, 1)))), Some(Declared::Srgb));
        assert_eq!(declared_colour_space(&tiff(b"PENTAX Corporation\0", &[], Some(short_ifd(0x0037, 0)))), Some(Declared::Srgb));
        assert_eq!(declared_colour_space(&tiff(b"Leica\0", &[], Some(short_ifd(0x00B4, 1)))), None, "Unknown maker notes say nothing");
        // An embedded JPEG's EXIF.
        let mut raf = b"FUJIFILMCCD-RAW junk \xFF\xD8\xFF\xE1\0\0Exif\0\0".to_vec();
        raf.extend(tiff(b"FUJIFILM\0", &[(0xA001, 3, 1, 1)], None));
        assert_eq!(declared_colour_space(&raf), Some(Declared::Srgb));
        // Truncated or foreign data never panics.
        let whole = tiff(b"Canon\0", &[(0xA001, 3, 1, 1)], Some(short_ifd(0x00B4, 1)));
        for n in 0..whole.len() {
            let _ = declared_colour_space(&whole[..n]);
        }
        assert_eq!(declared_colour_space(b"not a tiff at all"), None);
    }

    /// A little-endian DNG (or, without `dng_version`, a plain TIFF): IFD0
    /// with NewSubFileType `main`, PreviewColorSpace `main_space` and SubIFDs,
    /// each a NewSubFileType and an optional PreviewColorSpace (type, value).
    fn dng(dng_version: bool, main: u32, main_space: Option<u32>, subs: &[(u32, Option<(u16, u32)>)]) -> Vec<u8> {
        let entry = |d: &mut Vec<u8>, tag: u16, typ: u16, count: u32, value: u32| {
            d.extend(tag.to_le_bytes());
            d.extend(typ.to_le_bytes());
            d.extend(count.to_le_bytes());
            d.extend(value.to_le_bytes());
        };
        let (list_at, sub_at) = (100u32, |i: usize| 200 + 40 * i as u32);
        let mut d = b"II*\0\x08\0\0\0".to_vec();
        let count = 2 + dng_version as u16 + main_space.is_some() as u16;
        d.extend(count.to_le_bytes());
        entry(&mut d, 0x00FE, 4, 1, main);
        let subs_value = if subs.len() == 1 { sub_at(0) } else { list_at };
        entry(&mut d, 0x014A, 4, subs.len() as u32, subs_value);
        if dng_version {
            entry(&mut d, 0xC612, 1, 4, u32::from_le_bytes([1, 4, 0, 0]));
        }
        if let Some(space) = main_space {
            entry(&mut d, 0xC71A, 4, 1, space);
        }
        d.extend(0u32.to_le_bytes());
        d.resize(list_at as usize, 0);
        for i in 0..subs.len() {
            d.extend(sub_at(i).to_le_bytes());
        }
        for (i, &(kind, space)) in subs.iter().enumerate() {
            d.resize(sub_at(i) as usize, 0);
            d.extend((1 + space.is_some() as u16).to_le_bytes());
            entry(&mut d, 0x00FE, 4, 1, kind);
            if let Some((typ, value)) = space {
                entry(&mut d, 0xC71A, typ, 1, value);
            }
            d.extend(0u32.to_le_bytes());
        }
        d
    }

    /// A DNG's PreviewColorSpace, from the IFD rawler takes the preview from.
    #[test]
    fn dng_preview_colour_spaces_are_found() {
        // Lightroom: IFD0 a thumbnail, SubIFD 1 the raw, SubIFD 2 the preview.
        let lightroom = dng(true, 1, Some(2), &[(0, None), (1, Some((4, 2))), (1, Some((4, 2)))]);
        assert_eq!(dng_preview_colour_space(&lightroom, false), Some(Declared::Srgb));
        assert_eq!(dng_preview_colour_space(&lightroom, true), Some(Declared::Srgb));
        // Each image says for itself.
        let mixed = dng(true, 1, Some(2), &[(0, None), (1, Some((4, 3)))]);
        assert_eq!(dng_preview_colour_space(&mixed, false), Some(Declared::AdobeRgb));
        assert_eq!(dng_preview_colour_space(&mixed, true), Some(Declared::Srgb));
        assert_eq!(dng_preview_colour_space(&dng(true, 1, None, &[(1, Some((3, 3)))]), false), Some(Declared::AdobeRgb), "a short, one SubIFD");
        // Only the first reduced SubIFD counts, as rawler's preview_image.
        assert_eq!(dng_preview_colour_space(&dng(true, 0, None, &[(0, None), (1, None), (1, Some((4, 2)))]), false), None);
        // An IFD0 that is the raw is no thumbnail.
        assert_eq!(dng_preview_colour_space(&dng(true, 0, Some(2), &[(1, Some((4, 2)))]), true), None);
        // Gray gamma 2.2, ProPhoto and later values are another colour space;
        // Unknown says nothing, nor does a TIFF that isn't a DNG.
        for value in [1, 4, 5] {
            assert_eq!(dng_preview_colour_space(&dng(true, 1, Some(value), &[(1, Some((4, value)))]), false), Some(Declared::Other), "{value}");
        }
        assert_eq!(dng_preview_colour_space(&dng(true, 1, Some(0), &[(1, Some((4, 0)))]), false), None);
        assert_eq!(dng_preview_colour_space(&dng(false, 1, Some(2), &[(1, Some((4, 2)))]), false), None);
        // Truncated or foreign data never panics.
        for n in 0..lightroom.len() {
            let _ = dng_preview_colour_space(&lightroom[..n], false);
            let _ = dng_preview_colour_space(&lightroom[..n], true);
        }
        assert_eq!(dng_preview_colour_space(b"not a tiff at all", false), None);
    }

    /// Adobe RGB greys stay grey; its saturated green clips to sRGB's.
    #[test]
    fn adobe_rgb_previews_become_srgb() {
        let mut image = image::RgbaImage::from_pixel(2, 1, image::Rgba([128, 128, 128, 255]));
        image.put_pixel(1, 0, image::Rgba([0, 255, 0, 255]));
        adobe_to_srgb(&mut image);
        let grey = image.get_pixel(0, 0).0;
        assert!(grey[0] == grey[1] && grey[1] == grey[2] && (126..=130).contains(&grey[0]), "{grey:?}");
        assert_eq!(image.get_pixel(1, 0).0, [0, 255, 0, 255]);
    }

    /// Not a RAW: no preview, and it says so.
    #[test]
    fn non_raw_bytes_have_no_camera_preview() {
        let error = camera_preview(b"not a raw at all", (900, 600)).unwrap_err();
        assert_eq!(failure(&error), ("This RAW has no camera preview to match.".into(), true));
        assert_eq!(preview_colour_space(b"not a raw at all"), None);
    }

    /// The reference PNG's pixels, as the helper reads them: linear sRGB.
    #[test]
    fn the_reference_is_linear_srgb() {
        let rgb = image::RgbImage::from_fn(4, 2, |x, _| image::Rgb([(x * 80) as u8, 128, 255]));
        let linear = linear(rgb).unwrap();
        let v = |i: usize| spektrafilm_math::precision::to_f64(linear.data[i]);
        assert_eq!((linear.width, linear.height), (4, 2));
        assert!((v(1) - 0.2158605).abs() < 1e-5, "{}", v(1));
        assert!((v(2) - 1.0).abs() < 1e-6 && v(0) == 0.0);
    }

    /// Optional: a real RAW (`NEGS_TEST_RAW`) gives an sRGB preview of its
    /// shape, 3:2 unless `NEGS_TEST_RAW_SHAPE` says otherwise (\"2x3\").
    #[test]
    fn a_real_raw_gives_its_camera_preview() {
        let Some(path) = std::env::var_os("NEGS_TEST_RAW") else { return };
        let file = std::fs::read(path).unwrap();
        eprintln!("declared {:?}, DNG preview {:?}, preview {:?}", declared_colour_space(&file), dng_preview_colour_space(&file, false), preview_colour_space(&file));
        let shape = std::env::var("NEGS_TEST_RAW_SHAPE").ok().and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        });
        let preview = camera_preview(&file, shape.unwrap_or((3, 2))).unwrap();
        eprintln!("preview {}x{}", preview.width(), preview.height());
        assert_eq!(preview.width().max(preview.height()), LONG_SIDE);
    }
}
