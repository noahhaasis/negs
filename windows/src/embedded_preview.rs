//! A RAW file's embedded JPEG previews found from its header, without
//! reading the file whole or decoding anything: what `CGImageSource` does
//! for the Mac's thumbnails. The strip and grid decode the smallest one big
//! enough, through WIC, which scales a JPEG as it decodes it (an eighth of
//! the work for a 24 MP preview shown 400 pixels wide), and know every
//! photograph's shape from the largest one's size before any is decoded.
//!
//! TIFF-based RAWs (CR2, NEF, ARW, DNG, ORF, RW2, PEF and the like): the
//! JPEGs named by `JPEGInterchangeFormat`, by a single strip with JPEG
//! compression, or by RW2's `JpgFromRaw`, in IFD0, the IFDs chained after
//! it and their SubIFDs. Fuji RAF: the JPEG its header points at. Anything
//! else (CR3, a lossless-JPEG raw strip, a tiled preview) finds nothing,
//! and the caller decodes the preview through rawler as before.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// One embedded JPEG: where it is and its size as stored.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jpeg {
    pub offset: u64,
    pub length: u64,
    pub width: u32,
    pub height: u32,
}

impl Jpeg {
    fn long_side(&self) -> u32 {
        self.width.max(self.height)
    }
    fn ratio(&self) -> f64 {
        self.width as f64 / self.height.max(1) as f64
    }
}

/// What the header says: the previews, and the EXIF orientation (1 to 8)
/// if the file's own IFDs give one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Previews {
    pub jpegs: Vec<Jpeg>,
    pub orientation: Option<u16>,
}

/// Previews further than this from the largest one's shape are left out:
/// a camera's small thumbnail is often 4:3 with black bands round a 3:2
/// picture.
const SHAPE_TOLERANCE: f64 = 0.015;

impl Previews {
    /// The largest preview, the photograph's shape as its camera shows it.
    pub fn largest(&self) -> Option<Jpeg> {
        self.jpegs.iter().copied().max_by_key(|j| (u64::from(j.width) * u64::from(j.height), j.length))
    }

    /// The smallest preview at least `pixels` on its long side and of the
    /// largest one's shape, else the largest.
    pub fn for_size(&self, pixels: u32) -> Option<Jpeg> {
        let largest = self.largest()?;
        let shaped = |j: &&Jpeg| ((j.ratio() - largest.ratio()) / largest.ratio()).abs() <= SHAPE_TOLERANCE;
        self.jpegs.iter().filter(shaped).filter(|j| j.long_side() >= pixels).min_by_key(|j| (j.long_side(), j.length)).copied().or(Some(largest))
    }

    /// Width ÷ height as shown upright: the largest preview's, turned by
    /// the orientation.
    pub fn ratio(&self) -> Option<f64> {
        let j = self.largest()?;
        let turned = self.orientation.is_some_and(|o| o >= 5);
        Some(if turned { j.height as f64 / j.width.max(1) as f64 } else { j.ratio() })
    }
}

/// Reads `path`'s header for its embedded JPEGs; `None` if it has none this
/// can find (or can't be read).
pub fn find(path: &Path) -> Option<Previews> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    let mut reader = Reader { file: &mut file, size };
    let previews = find_in(&mut reader)?;
    (!previews.jpegs.is_empty()).then_some(previews)
}

/// The bytes of `jpeg` in `path`.
pub fn bytes(path: &Path, jpeg: &Jpeg) -> Option<Vec<u8>> {
    let mut file = File::open(path).ok()?;
    let size = file.metadata().ok()?.len();
    Reader { file: &mut file, size }.read(jpeg.offset, jpeg.length as usize)
}

pub(crate) trait At {
    fn read(&mut self, offset: u64, length: usize) -> Option<Vec<u8>>;
    fn size(&self) -> u64;
}

struct Reader<'a> {
    file: &'a mut File,
    size: u64,
}

impl At for Reader<'_> {
    fn read(&mut self, offset: u64, length: usize) -> Option<Vec<u8>> {
        if offset.checked_add(length as u64)? > self.size {
            return None;
        }
        self.file.seek(SeekFrom::Start(offset)).ok()?;
        let mut buffer = vec![0; length];
        self.file.read_exact(&mut buffer).ok()?;
        Some(buffer)
    }
    fn size(&self) -> u64 {
        self.size
    }
}

impl At for &[u8] {
    fn read(&mut self, offset: u64, length: usize) -> Option<Vec<u8>> {
        let start = usize::try_from(offset).ok()?;
        self.get(start..start.checked_add(length)?).map(<[u8]>::to_vec)
    }
    fn size(&self) -> u64 {
        self.len() as u64
    }
}

pub(crate) fn find_in(at: &mut impl At) -> Option<Previews> {
    let head = at.read(0, 16.min(at.size() as usize))?;
    if head.len() >= 16 && &head[..15] == b"FUJIFILMCCD-RAW" {
        return raf(at);
    }
    if head.len() < 8 {
        return None;
    }
    let little = match &head[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let e = Endian(little);
    // TIFF's 42, Olympus' "RO"/"RS" and Panasonic's 0x55.
    if !matches!(e.u16(&head[2..4]), 42 | 0x4f52 | 0x5352 | 0x55) {
        return None;
    }
    let mut walk = Walk { e, jpegs: vec![], orientation: None, visited: vec![] };
    let first = u64::from(e.u32(&head[4..8]));
    walk.chain(at, first, 0);
    let mut jpegs = vec![];
    for (offset, length) in walk.jpegs {
        if let Some(jpeg) = jpeg_at(at, offset, length) {
            if !jpegs.iter().any(|j: &Jpeg| j.offset == jpeg.offset) {
                jpegs.push(jpeg);
            }
        }
    }
    Some(Previews { jpegs, orientation: walk.orientation })
}

/// RAF: the header's JPEG offset and length at 84 and 88, big-endian.
fn raf(at: &mut impl At) -> Option<Previews> {
    let field = at.read(84, 8)?;
    let e = Endian(false);
    let (offset, length) = (u64::from(e.u32(&field[..4])), u64::from(e.u32(&field[4..])));
    let jpegs = jpeg_at(at, offset, length).into_iter().collect();
    Some(Previews { jpegs, orientation: None })
}

#[derive(Clone, Copy)]
struct Endian(bool);

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        let b = [b[0], b[1]];
        if self.0 { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }
    }
    fn u32(self, b: &[u8]) -> u32 {
        let b = [b[0], b[1], b[2], b[3]];
        if self.0 { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
    }
}

struct Walk {
    e: Endian,
    jpegs: Vec<(u64, u64)>,
    orientation: Option<u16>,
    visited: Vec<u64>,
}

const MAX_IFDS: usize = 32;
const MAX_ENTRIES: usize = 1000;

impl Walk {
    /// An IFD and those chained after it.
    fn chain(&mut self, at: &mut impl At, mut offset: u64, depth: u32) {
        let mut ifd0 = depth == 0;
        while offset != 0 && self.visited.len() < MAX_IFDS && !self.visited.contains(&offset) {
            self.visited.push(offset);
            let Some(next) = self.ifd(at, offset, depth, ifd0) else { return };
            ifd0 = false;
            offset = next;
        }
    }

    /// Reads one IFD's previews; the next IFD's offset.
    fn ifd(&mut self, at: &mut impl At, offset: u64, depth: u32, ifd0: bool) -> Option<u64> {
        let e = self.e;
        let count = usize::from(e.u16(&at.read(offset, 2)?));
        if count == 0 || count > MAX_ENTRIES {
            return None;
        }
        let table = at.read(offset + 2, count * 12 + 4)?;
        let (mut interchange, mut interchange_length) = (None, None);
        let (mut strip, mut strip_length, mut compression) = (None, None, None);
        let mut subs = vec![];
        let mut exif = None;
        for entry in table[..count * 12].chunks_exact(12) {
            let (tag, kind, n) = (e.u16(&entry[0..2]), e.u16(&entry[2..4]), e.u32(&entry[4..8]));
            let value = &entry[8..12];
            // The first value of a SHORT or LONG.
            let first = || match kind {
                3 => Some(u64::from(e.u16(value))),
                4 | 13 => Some(u64::from(e.u32(value))),
                _ => None,
            };
            match tag {
                0x0103 => compression = first(),
                0x0111 if n == 1 => strip = first(),
                0x0117 if n == 1 => strip_length = first(),
                0x0201 => interchange = first(),
                0x0202 => interchange_length = first(),
                // RW2's JpgFromRaw: the JPEG's bytes as the value.
                0x002e if kind == 7 && n > 4 => self.jpegs.push((u64::from(e.u32(value)), u64::from(n))),
                0x0112 if ifd0 || self.orientation.is_none() => {
                    if let Some(o) = first().filter(|o| (1..=8).contains(o)) {
                        if ifd0 || self.orientation.is_none() {
                            self.orientation = Some(o as u16);
                        }
                    }
                }
                0x014a if depth < 3 => {
                    if n == 1 {
                        subs.extend(first());
                    } else if kind == 4 || kind == 13 {
                        let n = (n as usize).min(MAX_IFDS);
                        if let Some(list) = at.read(u64::from(e.u32(value)), n * 4) {
                            subs.extend(list.chunks_exact(4).map(|c| u64::from(e.u32(c))));
                        }
                    }
                }
                0x8769 if depth < 3 => exif = first(),
                _ => {}
            }
        }
        if let (Some(o), Some(l)) = (interchange, interchange_length) {
            self.jpegs.push((o, l));
        }
        // A single strip of old- or new-style JPEG (a lossless raw is
        // weeded out by its SOF3).
        if let (Some(o), Some(l), Some(6 | 7)) = (strip, strip_length, compression) {
            self.jpegs.push((o, l));
        }
        for sub in subs {
            self.chain(at, sub, depth + 1);
        }
        if let Some(exif) = exif {
            // Only its orientation, when IFD0 has none.
            if !self.visited.contains(&exif) && self.visited.len() < MAX_IFDS {
                self.visited.push(exif);
                let _ = self.ifd(at, exif, depth + 1, false);
            }
        }
        Some(u64::from(e.u32(&table[count * 12..])))
    }
}

/// A baseline or progressive JPEG at `offset`: its stored size from its
/// frame header. A lossless or arithmetic one (a raw strip), or anything
/// else, gives `None`.
fn jpeg_at(at: &mut impl At, offset: u64, length: u64) -> Option<Jpeg> {
    if length < 64 || offset.checked_add(length)? > at.size() {
        return None;
    }
    if at.read(offset, 2)? != [0xff, 0xd8] {
        return None;
    }
    let mut p = offset + 2;
    let end = offset + length;
    for _ in 0..64 {
        if p + 4 > end {
            return None;
        }
        let marker = at.read(p, 4)?;
        if marker[0] != 0xff {
            return None;
        }
        let kind = marker[1];
        if kind == 0xff {
            p += 1;
            continue;
        }
        let len = u64::from(u16::from_be_bytes([marker[2], marker[3]]));
        match kind {
            0xc0..=0xc2 => {
                let sof = at.read(p + 4, 5)?;
                let height = u32::from(u16::from_be_bytes([sof[1], sof[2]]));
                let width = u32::from(u16::from_be_bytes([sof[3], sof[4]]));
                return (width > 0 && height > 0).then_some(Jpeg { offset, length, width, height });
            }
            // Lossless, hierarchical or arithmetic: not a picture WIC shows.
            0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf => return None,
            0xd8 | 0xd9 | 0xda => return None,
            _ => p += 2 + len,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A little-endian TIFF: IFD0 with an orientation and a JPEG by
    /// JPEGInterchangeFormat, one SubIFD with a bigger JPEG strip, and a
    /// lossless-JPEG raw strip in another.
    fn tiff() -> Vec<u8> {
        fn jpeg(sof: u8, w: u16, h: u16) -> Vec<u8> {
            let mut j = vec![0xff, 0xd8, 0xff, 0xe1, 0x00, 0x04, 0, 0, 0xff, sof, 0x00, 0x11, 8];
            j.extend(h.to_be_bytes());
            j.extend(w.to_be_bytes());
            j.resize(200, 0);
            j
        }
        fn entry(tag: u16, kind: u16, n: u32, value: u32) -> Vec<u8> {
            let mut e = tag.to_le_bytes().to_vec();
            e.extend(kind.to_le_bytes());
            e.extend(n.to_le_bytes());
            e.extend(value.to_le_bytes());
            e
        }
        let small = jpeg(0xc0, 640, 480);
        let big = jpeg(0xc0, 6000, 4000);
        let mid = jpeg(0xc2, 1620, 1080);
        let raw = jpeg(0xc3, 6000, 4000);
        let mut f = b"II*\0".to_vec();
        f.extend(8u32.to_le_bytes());
        // IFD0 at 8: 4 entries.
        let ifd0 = 8;
        let sub_list = ifd0 + 2 + 4 * 12 + 4;
        let sub_a = sub_list + 8;
        let sub_b = sub_a + 2 + 3 * 12 + 4;
        let data = sub_b + 2 + 3 * 12 + 4;
        let (small_at, big_at, mid_at, raw_at) = (data, data + 200, data + 400, data + 600);
        f.extend(4u16.to_le_bytes());
        f.extend(entry(0x0112, 3, 1, 6));
        f.extend(entry(0x014a, 4, 2, sub_list));
        f.extend(entry(0x0201, 4, 1, small_at));
        f.extend(entry(0x0202, 4, 1, 200));
        f.extend(0u32.to_le_bytes());
        f.extend(sub_a.to_le_bytes());
        f.extend(sub_b.to_le_bytes());
        f.extend(3u16.to_le_bytes());
        f.extend(entry(0x0103, 3, 1, 6));
        f.extend(entry(0x0111, 4, 1, big_at));
        f.extend(entry(0x0117, 4, 1, 200));
        f.extend(0u32.to_le_bytes());
        f.extend(3u16.to_le_bytes());
        f.extend(entry(0x0103, 3, 1, 7));
        f.extend(entry(0x0111, 4, 1, raw_at));
        f.extend(entry(0x0117, 4, 1, 200));
        f.extend(0u32.to_le_bytes());
        assert_eq!(f.len() as u32, data, "layout");
        let _ = mid_at;
        f.extend(small);
        f.extend(big);
        f.extend(mid);
        f.extend(raw);
        f
    }

    #[test]
    fn finds_the_baseline_and_progressive_previews_and_not_the_lossless_raw() {
        let file = tiff();
        let previews = find_in(&mut file.as_slice()).unwrap();
        let sizes: Vec<(u32, u32)> = previews.jpegs.iter().map(|j| (j.width, j.height)).collect();
        assert_eq!(sizes, vec![(640, 480), (6000, 4000)], "the 1620 one isn't named by any IFD");
        assert_eq!(previews.orientation, Some(6));
        assert_eq!(previews.largest().unwrap().width, 6000);
        // Turned a quarter: 4000 wide, 6000 high.
        assert!((previews.ratio().unwrap() - 4000.0 / 6000.0).abs() < 1e-9);
    }

    #[test]
    fn the_smallest_big_enough_of_the_largest_ones_shape() {
        let j = |w, h| Jpeg { offset: u64::from(w), length: 1000, width: w, height: h };
        let previews = Previews { jpegs: vec![j(160, 120), j(1620, 1080), j(6000, 4000), j(570, 380)], orientation: None };
        assert_eq!(previews.for_size(432).unwrap().width, 570);
        assert_eq!(previews.for_size(768).unwrap().width, 1620);
        assert_eq!(previews.for_size(8000).unwrap().width, 6000);
        // The 4:3 one is never chosen, however small the size.
        assert_eq!(previews.for_size(100).unwrap().width, 570);
    }

    #[test]
    fn nothing_from_what_isnt_tiff_or_raf() {
        assert!(find_in(&mut b"\0\0\0\x18ftypcrx \0\0\0\x01".as_slice()).is_none());
        assert!(find_in(&mut b"short".as_slice()).is_none());
    }

    /// `NEGS_PREVIEW_FOLDER=<folder>`: what each file's header gives.
    #[test]
    #[ignore]
    fn lists_a_folders_previews() {
        let Some(folder) = std::env::var_os("NEGS_PREVIEW_FOLDER") else { return };
        let mut paths: Vec<_> = std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()).collect();
        paths.sort();
        for path in paths {
            let started = std::time::Instant::now();
            let found = find(&path);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            let sizes = found.as_ref().map(|p| p.jpegs.iter().map(|j| format!("{}x{}", j.width, j.height)).collect::<Vec<_>>().join(" "));
            println!("{:>6.2} ms {:?} {:?} {}", ms, found.as_ref().and_then(|p| p.orientation), sizes, path.file_name().unwrap().to_string_lossy());
        }
    }
}
