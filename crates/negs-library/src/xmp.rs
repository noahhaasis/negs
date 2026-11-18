//! The rating an export carries, as Lightroom writes it: `xmp:Rating`, the
//! stars or −1 for a reject (`ExportRating` in `BatchExport.swift`). Added
//! to the finished file without touching its image data, so the pixels
//! stay as the exporter wrote them. macOS uses ImageIO for JPEG and TIFF and
//! its own code for PNG; here all three are done by hand.

use crate::item::Summary;
use crate::sidecar::Flag;

/// The rating an export embeds: the stars, or −1 for a reject. `None` (no
/// packet at all) for an unrated, unrejected photograph.
pub fn export_rating(summary: &Summary) -> Option<i32> {
    if summary.flag == Some(Flag::Reject) {
        return Some(-1);
    }
    (summary.rating > 0).then_some(summary.rating as i32)
}

/// The XMP packet with only the rating (`ExportRating.packet`).
pub fn packet(rating: i32) -> String {
    format!(
        "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?><x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:xmp=\"http://ns.adobe.com/xap/1.0/\" xmp:Rating=\"{rating}\"/></rdf:RDF></x:xmpmeta><?xpacket end=\"w\"?>"
    )
}

/// `bytes` (a PNG, JPEG or TIFF) with the rating, replacing any XMP packet
/// it had. `None` for anything else, which is left as it is.
pub fn embed(bytes: &[u8], rating: i32) -> Option<Vec<u8>> {
    if bytes.starts_with(&PNG_SIGNATURE) {
        embed_png(bytes, rating)
    } else if bytes.starts_with(&[0xFF, 0xD8]) {
        embed_jpeg(bytes, rating)
    } else if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
        embed_tiff(bytes, rating)
    } else {
        None
    }
}

/// Adds the rating to the file at `path` through a hidden file beside it
/// and a rename, as macOS does.
pub fn embed_in_file(path: &std::path::Path, rating: i32) -> std::io::Result<bool> {
    let bytes = std::fs::read(path)?;
    let Some(out) = embed(&bytes, rating) else { return Ok(false) };
    let staging = path.with_file_name(format!(".negs-rating-{}-{}", std::process::id(), crate::item::file_name(path)));
    let result = std::fs::write(&staging, out).and_then(|_| std::fs::rename(&staging, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    result.map(|_| true)
}

/// The rating a file carries, for tests and checks: the last packet's (a
/// TIFF keeps its old one, unreferenced, before the new).
pub fn read_rating(bytes: &[u8]) -> Option<i32> {
    let text = String::from_utf8_lossy(bytes);
    let at = text.rfind("xmp:Rating")?;
    let rest = &text[at + "xmp:Rating".len()..];
    let value: String = rest.trim_start_matches(['=', '"', '>', ' ']).chars().take_while(|c| *c == '-' || c.is_ascii_digit()).collect();
    value.parse().ok()
}

const PNG_SIGNATURE: [u8; 8] = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
const XMP_KEYWORD: &[u8] = b"XML:com.adobe.xmp";

fn crc32(data: &[u8]) -> u32 {
    let mut c: u32 = 0xFFFF_FFFF;
    for &byte in data {
        c ^= byte as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
    }
    c ^ 0xFFFF_FFFF
}

/// A PNG gets the packet as an `iTXt` chunk before its image data,
/// replacing one already there; every other byte stays
/// (`ExportRating.embedInPNG`).
fn embed_png(data: &[u8], rating: i32) -> Option<Vec<u8>> {
    let mut chunks: Vec<(&[u8], std::ops::Range<usize>)> = Vec::new();
    let mut at = 8;
    while at + 12 <= data.len() {
        let length = u32::from_be_bytes(data[at..at + 4].try_into().ok()?) as usize;
        if at + 12 + length > data.len() {
            return None;
        }
        chunks.push((&data[at + 4..at + 8], at..at + 12 + length));
        at += 12 + length;
    }
    let image = chunks.iter().position(|(kind, _)| *kind == b"IDAT")?;
    let mut body = b"iTXt".to_vec();
    body.extend_from_slice(XMP_KEYWORD);
    body.extend_from_slice(&[0, 0, 0, 0, 0]);
    body.extend_from_slice(packet(rating).as_bytes());
    let mut chunk = ((body.len() - 4) as u32).to_be_bytes().to_vec();
    chunk.extend_from_slice(&body);
    chunk.extend_from_slice(&crc32(&body).to_be_bytes());
    let mut out = PNG_SIGNATURE.to_vec();
    for (index, (kind, range)) in chunks.iter().enumerate() {
        if index == image {
            out.extend_from_slice(&chunk);
        }
        let start = range.start + 8;
        let is_xmp = *kind == b"iTXt" && data[start..range.end.min(start + XMP_KEYWORD.len())] == *XMP_KEYWORD;
        if !is_xmp {
            out.extend_from_slice(&data[range.clone()]);
        }
    }
    Some(out)
}

const JPEG_XMP: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";

/// A JPEG gets the packet as an APP1 segment after its JFIF and Exif
/// segments, replacing one already there.
fn embed_jpeg(data: &[u8], rating: i32) -> Option<Vec<u8>> {
    let mut at = 2;
    let mut insert = 2;
    let mut segments: Vec<std::ops::Range<usize>> = Vec::new();
    // The markers before the image data, each with its length.
    while at + 4 <= data.len() && data[at] == 0xFF {
        let marker = data[at + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let length = u16::from_be_bytes([data[at + 2], data[at + 3]]) as usize;
        if length < 2 || at + 2 + length > data.len() {
            return None;
        }
        let range = at..at + 2 + length;
        if marker == 0xE0 || (marker == 0xE1 && !data[at + 4..range.end].starts_with(JPEG_XMP)) {
            if insert == range.start {
                insert = range.end;
            }
        }
        segments.push(range);
        at += 2 + length;
    }
    let mut payload = JPEG_XMP.to_vec();
    payload.extend_from_slice(packet(rating).as_bytes());
    if payload.len() + 2 > 0xFFFF {
        return None;
    }
    let mut segment = vec![0xFF, 0xE1];
    segment.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    segment.extend_from_slice(&payload);
    let old_xmp: Vec<std::ops::Range<usize>> =
        segments.into_iter().filter(|r| data[r.start + 1] == 0xE1 && data[r.start + 4..r.end].starts_with(JPEG_XMP)).collect();
    let mut out = Vec::with_capacity(data.len() + segment.len());
    let mut position = 0;
    for range in &old_xmp {
        if range.start >= insert {
            break;
        }
        out.extend_from_slice(&data[position..range.start]);
        position = range.end;
    }
    out.extend_from_slice(&data[position..insert]);
    out.extend_from_slice(&segment);
    position = insert;
    for range in old_xmp.iter().filter(|r| r.start >= insert) {
        out.extend_from_slice(&data[position..range.start]);
        position = range.end;
    }
    out.extend_from_slice(&data[position..]);
    Some(out)
}

/// A TIFF gets the packet as tag 700 of its first image: the image's
/// directory is written again at the end of the file with the tag, and the
/// header points to it. The image data stays where it is.
fn embed_tiff(data: &[u8], rating: i32) -> Option<Vec<u8>> {
    let little = data.starts_with(b"II");
    let u16_at = |at: usize| -> Option<u16> {
        let b: [u8; 2] = data.get(at..at + 2)?.try_into().ok()?;
        Some(if little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let b: [u8; 4] = data.get(at..at + 4)?.try_into().ok()?;
        Some(if little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    };
    let put16 = |v: u16| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let put32 = |v: u32| if little { v.to_le_bytes() } else { v.to_be_bytes() };
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    let next = u32_at(ifd + 2 + count * 12)?;
    let mut entries: Vec<[u8; 12]> = Vec::with_capacity(count + 1);
    for i in 0..count {
        let entry: [u8; 12] = data.get(ifd + 2 + i * 12..ifd + 14 + i * 12)?.try_into().ok()?;
        if u16_at(ifd + 2 + i * 12)? != 700 {
            entries.push(entry);
        }
    }
    let xmp = packet(rating).into_bytes();
    let mut out = data.to_vec();
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let xmp_at = out.len() as u32;
    out.extend_from_slice(&xmp);
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let mut tag = [0u8; 12];
    tag[0..2].copy_from_slice(&put16(700));
    tag[2..4].copy_from_slice(&put16(1)); // BYTE
    tag[4..8].copy_from_slice(&put32(xmp.len() as u32));
    tag[8..12].copy_from_slice(&put32(xmp_at));
    entries.push(tag);
    let tag_of = |e: &[u8; 12]| if little { u16::from_le_bytes([e[0], e[1]]) } else { u16::from_be_bytes([e[0], e[1]]) };
    entries.sort_by_key(tag_of);
    let new_ifd = u32::try_from(out.len()).ok()?;
    out.extend_from_slice(&put16(entries.len() as u16));
    for entry in &entries {
        out.extend_from_slice(entry);
    }
    out.extend_from_slice(&put32(next));
    out[4..8].copy_from_slice(&put32(new_ifd));
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rating_an_export_carries() {
        assert_eq!(export_rating(&Summary::default()), None);
        assert_eq!(export_rating(&Summary { rating: 4, ..Summary::default() }), Some(4));
        assert_eq!(export_rating(&Summary { rating: 4, flag: Some(Flag::Reject), ..Summary::default() }), Some(-1));
        assert_eq!(export_rating(&Summary { flag: Some(Flag::Pick), ..Summary::default() }), None, "picks have no standard field");
    }

    fn png() -> Vec<u8> {
        let mut out = PNG_SIGNATURE.to_vec();
        for (kind, body) in [(&b"IHDR"[..], &[0u8; 13][..]), (b"IDAT", b"pixels"), (b"IEND", b"")] {
            out.extend_from_slice(&(body.len() as u32).to_be_bytes());
            let mut chunk = kind.to_vec();
            chunk.extend_from_slice(body);
            out.extend_from_slice(&chunk);
            out.extend_from_slice(&crc32(&chunk).to_be_bytes());
        }
        out
    }

    #[test]
    fn png_jpeg_and_tiff_keep_their_image_data() {
        let original = png();
        let rated = embed(&original, 3).unwrap();
        assert_eq!(read_rating(&rated), Some(3));
        let again = embed(&rated, -1).unwrap();
        assert_eq!(read_rating(&again), Some(-1));
        assert_eq!(again.len(), rated.len() + 1, "the packet is replaced, not added");
        assert!(again.windows(6).any(|w| w == b"pixels"));

        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x4A, 0x46];
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02, 1, 2, 3, 0xFF, 0xD9]);
        let rated = embed(&jpeg, 5).unwrap();
        assert_eq!(read_rating(&rated), Some(5));
        assert_eq!(&rated[..8], &jpeg[..8], "after the JFIF segment");
        assert!(rated.ends_with(&jpeg[8..]));
        let again = embed(&rated, 2).unwrap();
        assert_eq!(read_rating(&again), Some(2));
        assert_eq!(again.len(), rated.len());

        // A one-entry TIFF directory, little-endian.
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&[0x00, 0x01, 3, 0, 1, 0, 0, 0, 16, 0, 0, 0]);
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let rated = embed(&tiff, 1).unwrap();
        assert_eq!(read_rating(&rated), Some(1));
        assert!(rated.starts_with(&tiff[..4]) && rated[8..tiff.len()] == tiff[8..], "the old directory and image data stay");
        let directory = u32::from_le_bytes(rated[4..8].try_into().unwrap()) as usize;
        assert_eq!(u16::from_le_bytes([rated[directory], rated[directory + 1]]), 2);
        let again = embed(&rated, 4).unwrap();
        let directory = u32::from_le_bytes(again[4..8].try_into().unwrap()) as usize;
        assert_eq!(u16::from_le_bytes([again[directory], again[directory + 1]]), 2, "one XMP tag");
        assert_eq!(embed(b"GIF89a", 3), None);
    }
}
