//! A RAW file's embedded JPEG previews found from its header, without
//! reading the file whole or decoding anything (windows/src/embedded_preview.rs's
//! API; the scan runs in core/src/rules/embedded). The strip and grid decode
//! the smallest one big enough, and know every photograph's shape from the
//! largest one's size before any is decoded.
//!
//! TIFF-based RAWs (CR2, NEF, ARW, DNG, ORF, RW2, PEF and the like): the
//! JPEGs named by `JPEGInterchangeFormat`, by a single strip with JPEG
//! compression, or by RW2's `JpgFromRaw`, in IFD0, the IFDs chained after
//! it and their SubIFDs. Fuji RAF: the JPEG its header points at. Anything
//! else (CR3, a lossless-JPEG raw strip, a tiled preview) finds nothing,
//! and the caller decodes the preview through rawler as before.

use std::path::Path;

use negs_core_sys::media as sys;

/// One embedded JPEG: where it is and its size as stored.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Jpeg {
    pub offset: u64,
    pub length: u64,
    pub width: u32,
    pub height: u32,
}

/// What the header says: the previews, and the EXIF orientation (1 to 8)
/// if the file's own IFDs give one.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Previews {
    pub jpegs: Vec<Jpeg>,
    pub orientation: Option<u16>,
}

fn jpeg(j: sys::negs_jpeg_t) -> Jpeg {
    Jpeg { offset: j.offset, length: j.length, width: j.width, height: j.height }
}

impl Previews {
    fn raw(&self) -> (*const sys::negs_jpeg_t, usize) {
        // Jpeg is negs_jpeg_t's layout (repr(C), the same fields).
        (self.jpegs.as_ptr().cast(), self.jpegs.len())
    }

    /// The largest preview, the photograph's shape as its camera shows it.
    pub fn largest(&self) -> Option<Jpeg> {
        let (p, n) = self.raw();
        let mut out = sys::negs_jpeg_t::default();
        (unsafe { sys::negs_previews_largest(p, n, &mut out) } != 0).then(|| jpeg(out))
    }

    /// The smallest preview at least `pixels` on its long side and of the
    /// largest one's shape, else the largest.
    pub fn for_size(&self, pixels: u32) -> Option<Jpeg> {
        let (p, n) = self.raw();
        let mut out = sys::negs_jpeg_t::default();
        (unsafe { sys::negs_previews_for_size(p, n, pixels, &mut out) } != 0).then(|| jpeg(out))
    }

    /// Width ÷ height as shown upright: the largest preview's, turned by
    /// the orientation.
    pub fn ratio(&self) -> Option<f64> {
        let (p, n) = self.raw();
        let mut out = 0.0;
        let (o, has) = self.orientation.map_or((0, 0), |o| (o, 1));
        (unsafe { sys::negs_previews_ratio(p, n, o, has, &mut out) } != 0).then_some(out)
    }
}

fn take(mut raw: sys::negs_previews_t) -> Previews {
    let jpegs = if raw.jpegs.is_null() { vec![] } else { unsafe { std::slice::from_raw_parts(raw.jpegs, raw.count) }.iter().copied().map(jpeg).collect() };
    let orientation = (raw.has_orientation != 0).then_some(raw.orientation);
    unsafe { sys::negs_previews_free(&mut raw) };
    Previews { jpegs, orientation }
}

/// A path's bytes as the core takes them: the OS's own on Unix, WTF-8 on
/// Windows (UTF-8 for every name Windows can store as Unicode).
fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

/// Reads `path`'s header for its embedded JPEGs; `None` if it has none this
/// can find (or can't be read).
pub fn find(path: &Path) -> Option<Previews> {
    let bytes = path_bytes(path);
    let mut raw = sys::negs_previews_t::default();
    let found = unsafe { sys::negs_embedded_find(bytes.as_ptr(), bytes.len(), &mut raw) } != 0;
    let previews = take(raw);
    found.then_some(previews)
}

/// The bytes of `jpeg` in `path`.
pub fn bytes(path: &Path, jpeg: &Jpeg) -> Option<Vec<u8>> {
    let p = path_bytes(path);
    let mut out = negs_core_sys::negs_buf_t { data: std::ptr::null_mut(), len: 0 };
    let ok = unsafe { sys::negs_embedded_bytes(p.as_ptr(), p.len(), jpeg.offset, jpeg.length, &mut out) } != 0;
    let bytes = negs_core_sys::take_buf(out);
    ok.then_some(bytes)
}

/// A header in memory: what `find` reads from a file. `None` when it isn't
/// a TIFF or RAF header; the previews may be empty.
pub fn find_in(at: &mut &[u8]) -> Option<Previews> {
    let mut raw = sys::negs_previews_t::default();
    let found = unsafe { sys::negs_embedded_find_bytes(at.as_ptr(), at.len(), &mut raw) } != 0;
    let previews = take(raw);
    found.then_some(previews)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_is_negs_jpeg_t() {
        use std::mem::{offset_of, size_of};
        assert_eq!(size_of::<Jpeg>(), size_of::<sys::negs_jpeg_t>());
        assert_eq!(offset_of!(Jpeg, length), offset_of!(sys::negs_jpeg_t, length));
        assert_eq!(offset_of!(Jpeg, width), offset_of!(sys::negs_jpeg_t, width));
        assert_eq!(offset_of!(Jpeg, height), offset_of!(sys::negs_jpeg_t, height));
    }

    /// windows/src/embedded_preview.rs's own tests run there against this.
    #[test]
    fn the_smallest_big_enough_of_the_largest_ones_shape() {
        let j = |w, h| Jpeg { offset: u64::from(w), length: 1000, width: w, height: h };
        let previews = Previews { jpegs: vec![j(160, 120), j(1620, 1080), j(6000, 4000), j(570, 380)], orientation: None };
        assert_eq!(previews.for_size(432).unwrap().width, 570);
        assert_eq!(previews.for_size(768).unwrap().width, 1620);
        assert_eq!(previews.for_size(8000).unwrap().width, 6000);
        assert_eq!(previews.for_size(100).unwrap().width, 570);
        assert!(Previews::default().largest().is_none());
    }

    #[test]
    fn nothing_from_what_isnt_tiff_or_raf() {
        assert!(find_in(&mut b"\0\0\0\x18ftypcrx \0\0\0\x01".as_slice()).is_none());
        assert!(find_in(&mut b"short".as_slice()).is_none());
        assert!(find(Path::new("/no/such/file.nef")).is_none());
    }
}
