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
//!
//! The scan runs in the shared core (core/src/rules/embedded, through
//! `negs_core_rules`); this module keeps its API and tests.

pub use negs_core_rules::embedded_preview::*;

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
