//! The reference for Prep › Match camera brightness (`CameraBrightnessReference.swift`):
//! a RAW's embedded camera preview, never a new development, or a JPEG, PNG
//! or TIFF's own pixels, as a small sRGB PNG the helper's `place_exposure`
//! fits the developed photograph's brightness to.

use std::path::Path;

use anyhow::Result;
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::core::{Interface, PCWSTR};

use crate::platform::wide;

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

fn unusable<T>(message: &str) -> Result<T> {
    Err(Unusable(message.into()).into())
}

/// The longest side of the reference, in pixels.
const LONG_SIDE: u32 = 512;

pub fn same_shape(aspect: f64, expected: f64) -> bool {
    aspect.max(expected) / aspect.min(expected) <= 1.02
}

/// A RAW's embedded preview, oriented, as the reference for a photograph of
/// `expected` size (the helper's decoded source). The camera must say it is
/// sRGB: the preview is only a brightness reference, and its colorimetry
/// has to be known.
pub fn camera_reference(path: &Path, expected: (u32, u32)) -> Result<Vec<u8>> {
    let source = rawler::rawsource::RawSource::new(path)?;
    let decoder = rawler::get_decoder(&source)?;
    let params = rawler::decoders::RawDecodeParams::default();
    let Some(image) = decoder
        .preview_image(&source, &params)
        .ok()
        .flatten()
        .or_else(|| decoder.thumbnail_image(&source, &params).ok().flatten())
    else {
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
    // The colorimetry has to be known: ImageIO reads it on the Mac; here the
    // camera's own declaration does, and Adobe RGB is converted.
    let declared = match metadata.as_ref().and_then(|m| m.exif.color_space) {
        Some(1) => Some(Declared::Srgb),
        _ => std::fs::read(path).ok().and_then(|file| declared_colour_space(&file)),
    };
    let mut rgba = image.to_rgba8();
    match declared {
        Some(Declared::Srgb) => {}
        Some(Declared::AdobeRgb) => adobe_to_srgb(&mut rgba),
        None => return unusable("The camera preview has no color profile."),
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
    encoded(rgba, "The camera preview couldn’t be read.")
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
    encoded(rgba, UNREADABLE)
}

/// The colour space a camera says its JPEGs and previews are in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Declared {
    Srgb,
    AdobeRgb,
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
fn adobe_to_srgb(rgba: &mut image::RgbaImage) {
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

fn luma(p: [u8; 4]) -> u8 {
    (0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64).round() as u8
}

/// At most 512 px, opaque 8-bit sRGB PNG, as the helper's reference.
fn encoded(rgba: image::RgbaImage, unreadable: &str) -> Result<Vec<u8>> {
    let (w, h) = rgba.dimensions();
    let scale = (LONG_SIDE as f64 / w.max(h) as f64).min(1.0);
    let (sw, sh) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let rgb = image::DynamicImage::ImageRgba8(rgba).to_rgb8();
    let rgb = if (sw, sh) == (w, h) { rgb } else { image::imageops::resize(&rgb, sw, sh, image::imageops::FilterType::Triangle) };
    let mut png = Vec::new();
    if image::DynamicImage::ImageRgb8(rgb).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).is_err() {
        return unusable(unreadable);
    }
    Ok(png)
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
    let reason = format!("{error:#}");
    if reason.contains("midtone quantiles") {
        return ("The camera preview is too dark or too bright to match.".into(), true);
    }
    if reason.contains("±3 EV") {
        return ("Matching would take more than ±3 EV.".into(), false);
    }
    ("No reliable brightness match with these settings.".into(), false)
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

    /// Optional: a real RAW (`NEGS_TEST_RAW`) declares its colour space.
    #[test]
    fn a_real_raw_declares_its_colour_space() {
        let Some(path) = std::env::var_os("NEGS_TEST_RAW") else { return };
        let declared = declared_colour_space(&std::fs::read(path).unwrap());
        eprintln!("declared {declared:?}");
        assert!(declared.is_some());
    }

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

