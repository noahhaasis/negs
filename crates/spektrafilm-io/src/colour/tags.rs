//! Colour metadata outside the ICC profile: PNG's `cICP`, `sRGB`, `gAMA`
//! and `cHRM` chunks, the EXIF `ColorSpace` and interoperability index, and
//! the number of components of a JPEG frame (to recognise CMYK).

use std::io::Read;

/// The colour chunks of a PNG, read before its image data.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PngChunks {
    /// Colour primaries, transfer characteristics, matrix coefficients and
    /// full-range flag (ITU-T H.273).
    pub cicp: Option<[u8; 4]>,
    pub srgb: bool,
    /// The file's encoding exponent, e.g. 1/2.2.
    pub gamma: Option<f64>,
    /// White, red, green and blue xy.
    pub chromaticities: Option<[[f64; 2]; 4]>,
}

/// Reads the chunks before `IDAT`. `None` if `bytes` isn't a PNG.
pub fn png_chunks(mut reader: impl Read) -> Option<PngChunks> {
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature).ok()?;
    if signature != *b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let mut chunks = PngChunks::default();
    loop {
        let mut head = [0u8; 8];
        if reader.read_exact(&mut head).is_err() {
            return Some(chunks);
        }
        let length = u32::from_be_bytes(head[..4].try_into().unwrap()) as usize;
        let kind = &head[4..8];
        if kind == b"IDAT" || kind == b"IEND" {
            return Some(chunks);
        }
        // Colour chunks are tiny; skip anything else (text, iCCP) unread.
        let small = matches!(kind, b"cICP" | b"sRGB" | b"gAMA" | b"cHRM");
        if small && length <= 64 {
            let mut body = vec![0u8; length + 4];
            reader.read_exact(&mut body).ok()?;
            let u32_at = |i: usize| body.get(i..i + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()));
            match kind {
                b"cICP" if length == 4 => chunks.cicp = Some([body[0], body[1], body[2], body[3]]),
                b"sRGB" if length == 1 => chunks.srgb = true,
                b"gAMA" if length == 4 => chunks.gamma = u32_at(0).filter(|&g| g > 0).map(|g| g as f64 / 100_000.0),
                b"cHRM" if length == 32 => {
                    let v: Vec<f64> = (0..8).filter_map(|i| u32_at(4 * i)).map(|v| v as f64 / 100_000.0).collect();
                    chunks.chromaticities = Some([[v[0], v[1]], [v[2], v[3]], [v[4], v[5]], [v[6], v[7]]]);
                }
                _ => {}
            }
        } else {
            std::io::copy(&mut (&mut reader).take(length as u64 + 4), &mut std::io::sink()).ok()?;
        }
    }
}

/// What the EXIF block says about the colour space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExifColour {
    /// `ColorSpace` = 1.
    Srgb,
    /// `ColorSpace` = 0xFFFF (uncalibrated) with interoperability index
    /// "R03": the DCF option file, Adobe RGB (1998).
    AdobeRgb,
}

/// Reads `ColorSpace` (0xA001) and the interoperability index (0x0001 in the
/// interoperability IFD) from a TIFF-structured EXIF block. Anything else,
/// including uncalibrated without "R03", says nothing usable.
pub fn exif_colour(exif: &[u8]) -> Option<ExifColour> {
    let exif = exif.strip_prefix(b"Exif\0\0").unwrap_or(exif);
    let little = match exif.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = exif.get(at..at + 2)?.try_into().ok()?;
        Some(if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = exif.get(at..at + 4)?.try_into().ok()?;
        Some(if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    // (tag, type, count, value offset field position)
    let entries = |ifd: usize| -> Option<Vec<(u16, u16, u32, usize)>> {
        let count = u16_at(ifd)? as usize;
        (0..count.min(512))
            .map(|i| {
                let at = ifd + 2 + 12 * i;
                Some((u16_at(at)?, u16_at(at + 2)?, u32_at(at + 4)?, at + 8))
            })
            .collect()
    };
    let ifd0 = entries(u32_at(4)? as usize)?;
    let exif_ifd = ifd0.iter().find(|e| e.0 == 0x8769).and_then(|e| u32_at(e.3))? as usize;
    let exif_entries = entries(exif_ifd)?;
    let colour_space = exif_entries.iter().find(|e| e.0 == 0xA001).and_then(|e| u16_at(e.3))?;
    match colour_space {
        1 => Some(ExifColour::Srgb),
        0xFFFF => {
            let interop = exif_entries.iter().find(|e| e.0 == 0xA005).and_then(|e| u32_at(e.3))? as usize;
            let index = entries(interop)?.into_iter().find(|e| e.0 == 0x0001 && e.1 == 2)?;
            // Four ASCII bytes or fewer sit in the value field itself.
            let at = if index.2 <= 4 { index.3 } else { u32_at(index.3)? as usize };
            (exif.get(at..at + 3)? == b"R03").then_some(ExifColour::AdobeRgb)
        }
        _ => None,
    }
}

/// The ICC profile of a TIFF's first image (tag 34675), whatever type the
/// writer gave it. `image` reads only BYTE-typed profiles, but TIFF/EP and
/// most writers use UNDEFINED.
pub fn tiff_icc(mut file: impl Read + std::io::Seek) -> Option<Vec<u8>> {
    use std::io::SeekFrom;
    let mut head = [0u8; 8];
    file.read_exact(&mut head).ok()?;
    let little = match &head[0..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16v = |b: [u8; 2]| if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) };
    let u32v = |b: [u8; 4]| if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) };
    if u16v([head[2], head[3]]) != 42 {
        return None; // BigTIFF and others: let `image` speak for them
    }
    file.seek(SeekFrom::Start(u32v(head[4..8].try_into().ok()?) as u64)).ok()?;
    let mut count = [0u8; 2];
    file.read_exact(&mut count).ok()?;
    let mut entries = vec![0u8; 12 * u16v(count) as usize];
    file.read_exact(&mut entries).ok()?;
    let entry = entries.chunks_exact(12).find(|e| u16v([e[0], e[1]]) == 34675)?;
    let width = match u16v([entry[2], entry[3]]) {
        1 | 2 | 6 | 7 => 1,
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        _ => return None,
    };
    let length = (u32v(entry[4..8].try_into().ok()?) as usize).checked_mul(width)?;
    if length <= 4 || length > 64 << 20 {
        return None;
    }
    file.seek(SeekFrom::Start(u32v(entry[8..12].try_into().ok()?) as u64)).ok()?;
    let mut icc = vec![0u8; length];
    file.read_exact(&mut icc).ok()?;
    Some(icc)
}

/// The number of components in a JPEG's frame header: 1 grey, 3 YCbCr/RGB,
/// 4 CMYK/YCCK. `None` if it isn't a JPEG or has no frame.
pub fn jpeg_components(mut reader: impl Read) -> Option<u8> {
    let mut soi = [0u8; 2];
    reader.read_exact(&mut soi).ok()?;
    if soi != [0xFF, 0xD8] {
        return None;
    }
    loop {
        let mut marker = [0u8; 2];
        reader.read_exact(&mut marker).ok()?;
        if marker[0] != 0xFF {
            return None;
        }
        let kind = marker[1];
        if kind == 0xFF || (0xD0..=0xD7).contains(&kind) || kind == 0x01 {
            continue;
        }
        let mut length = [0u8; 2];
        reader.read_exact(&mut length).ok()?;
        let length = u16::from_be_bytes(length) as usize;
        let is_sof = matches!(kind, 0xC0..=0xCF) && !matches!(kind, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let mut header = [0u8; 6];
            reader.read_exact(&mut header).ok()?;
            return Some(header[5]);
        }
        if kind == 0xDA || length < 2 {
            return None;
        }
        std::io::copy(&mut (&mut reader).take(length as u64 - 2), &mut std::io::sink()).ok()?;
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A TIFF-structured EXIF block with ColorSpace and an interoperability index.
    pub(crate) fn exif_block(colour_space: u16, index: Option<&[u8; 4]>, little: bool) -> Vec<u8> {
        let u16b = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
        let u32b = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
        let mut d = Vec::new();
        d.extend(if little { b"II" } else { b"MM" });
        d.extend(u16b(42));
        d.extend(u32b(8));
        // IFD0 at 8: one entry, the Exif IFD pointer.
        d.extend(u16b(1));
        d.extend(u16b(0x8769));
        d.extend(u16b(4));
        d.extend(u32b(1));
        d.extend(u32b(26));
        d.extend(u32b(0));
        // Exif IFD at 26: ColorSpace and the interop pointer.
        d.extend(u16b(2));
        d.extend(u16b(0xA001));
        d.extend(u16b(3));
        d.extend(u32b(1));
        d.extend(u16b(colour_space));
        d.extend([0, 0]);
        d.extend(u16b(0xA005));
        d.extend(u16b(4));
        d.extend(u32b(1));
        d.extend(u32b(56));
        d.extend(u32b(0));
        // Interop IFD at 56.
        d.extend(u16b(1));
        d.extend(u16b(0x0001));
        d.extend(u16b(2));
        d.extend(u32b(4));
        d.extend(index.copied().unwrap_or(*b"R98\0"));
        d.extend(u32b(0));
        d
    }

    #[test]
    fn exif_colour_space_and_dcf_adobe_rgb() {
        for little in [true, false] {
            assert_eq!(exif_colour(&exif_block(1, None, little)), Some(ExifColour::Srgb));
            assert_eq!(exif_colour(&exif_block(0xFFFF, Some(b"R03\0"), little)), Some(ExifColour::AdobeRgb));
            assert_eq!(exif_colour(&exif_block(0xFFFF, Some(b"R98\0"), little)), None);
            let mut prefixed = b"Exif\0\0".to_vec();
            prefixed.extend(exif_block(1, None, little));
            assert_eq!(exif_colour(&prefixed), Some(ExifColour::Srgb));
        }
        assert_eq!(exif_colour(b"garbage"), None);
        assert_eq!(exif_colour(&exif_block(1, None, true)[..20]), None);
    }

    #[test]
    fn png_colour_chunks_are_read_until_the_image_data() {
        let chunk = |kind: &[u8; 4], body: &[u8]| {
            let mut c = (body.len() as u32).to_be_bytes().to_vec();
            c.extend(kind);
            c.extend(body);
            c.extend([0; 4]); // CRC, unchecked
            c
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend(chunk(b"IHDR", &[0; 13]));
        png.extend(chunk(b"gAMA", &45455u32.to_be_bytes()));
        png.extend(chunk(b"tEXt", b"Comment\0hello"));
        png.extend(chunk(b"cICP", &[12, 13, 0, 1]));
        png.extend(chunk(b"IDAT", &[0; 4]));
        png.extend(chunk(b"sRGB", &[0]));
        let chunks = png_chunks(&png[..]).unwrap();
        assert_eq!(chunks.cicp, Some([12, 13, 0, 1]));
        assert!((chunks.gamma.unwrap() - 0.45455).abs() < 1e-9);
        assert!(!chunks.srgb, "chunks after IDAT are ignored");
        assert_eq!(png_chunks(&b"GIF89a"[..]), None);
    }

    #[test]
    fn jpeg_component_count() {
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0];
        jpeg.extend([0xFF, 0xC0, 0, 17, 8, 0, 1, 0, 1, 4]);
        assert_eq!(jpeg_components(&jpeg[..]), Some(4));
        assert_eq!(jpeg_components(&b"\x89PNG"[..]), None);
    }
}
