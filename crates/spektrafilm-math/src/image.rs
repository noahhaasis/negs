use crate::precision::{Scalar, ZERO};
use rayon::prelude::*;

/// Contiguous HxWx3 image buffer, row-major, channel-interleaved.
///
/// Pixel type is `Scalar` — f32 by default, f64 with `--features precision-f64`.
#[derive(Clone)]
pub struct ImageBuf {
    pub width: u32,
    pub height: u32,
    pub data: Vec<Scalar>,
}

impl ImageBuf {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![ZERO; (width as usize) * (height as usize) * 3],
        }
    }

    pub fn from_data(width: u32, height: u32, data: Vec<Scalar>) -> Self {
        assert_eq!(data.len(), (width as usize) * (height as usize) * 3);
        Self {
            width,
            height,
            data,
        }
    }

    pub fn pixel_count(&self) -> usize {
        (self.width as usize) * (self.height as usize)
    }

    #[inline]
    pub fn idx(&self, x: u32, y: u32, c: usize) -> usize {
        ((y as usize) * (self.width as usize) + (x as usize)) * 3 + c
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> [Scalar; 3] {
        let i = self.idx(x, y, 0);
        [self.data[i], self.data[i + 1], self.data[i + 2]]
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, rgb: [Scalar; 3]) {
        let i = self.idx(x, y, 0);
        self.data[i] = rgb[0];
        self.data[i + 1] = rgb[1];
        self.data[i + 2] = rgb[2];
    }

    pub fn pixels(&self) -> impl Iterator<Item = &[Scalar]> {
        self.data.chunks_exact(3)
    }

    pub fn pixels_mut(&mut self) -> impl Iterator<Item = &mut [Scalar]> {
        self.data.chunks_exact_mut(3)
    }

    pub fn par_pixels(&self) -> rayon::slice::ChunksExact<'_, Scalar> {
        self.data.par_chunks_exact(3)
    }

    pub fn par_pixels_mut(&mut self) -> rayon::slice::ChunksExactMut<'_, Scalar> {
        self.data.par_chunks_exact_mut(3)
    }

    pub fn rows(&self) -> impl Iterator<Item = &[Scalar]> {
        self.data.chunks_exact((self.width as usize) * 3)
    }

    pub fn par_rows(&self) -> rayon::slice::ChunksExact<'_, Scalar> {
        self.data.par_chunks_exact((self.width as usize) * 3)
    }

    pub fn par_rows_mut(&mut self) -> rayon::slice::ChunksExactMut<'_, Scalar> {
        let w = (self.width as usize) * 3;
        self.data.par_chunks_exact_mut(w)
    }

    pub fn extract_channel(&self, c: usize) -> Vec<Scalar> {
        assert!(c < 3);
        self.data.iter().skip(c).step_by(3).copied().collect()
    }

    pub fn write_channel(&mut self, c: usize, chan: &[Scalar]) {
        assert!(c < 3);
        assert_eq!(chan.len(), self.pixel_count());
        for (i, &v) in chan.iter().enumerate() {
            self.data[i * 3 + c] = v;
        }
    }

    /// Apply an EXIF/TIFF orientation tag (1..=8), returning an image
    /// that displays upright. Unknown values (and 1) return `self`
    /// unchanged (no allocation).
    ///
    /// | tag | transform               |
    /// |-----|-------------------------|
    /// | 1   | none                    |
    /// | 2   | flip horizontal         |
    /// | 3   | rotate 180°             |
    /// | 4   | flip vertical           |
    /// | 5   | transpose               |
    /// | 6   | rotate 90° clockwise    |
    /// | 7   | transverse              |
    /// | 8   | rotate 90° counter-cw   |
    pub fn oriented(self, exif_orientation: u16) -> Self {
        if !(2..=8).contains(&exif_orientation) {
            return self;
        }
        let (w, h) = (self.width as usize, self.height as usize);
        let swap = exif_orientation >= 5;
        let (dw, dh) = if swap { (h, w) } else { (w, h) };
        if dw == 0 || dh == 0 {
            // Nothing to move; `par_chunks_exact_mut(0)` would panic.
            return Self {
                width: dw as u32,
                height: dh as u32,
                data: self.data,
            };
        }
        let src = &self.data;
        let mut data = vec![ZERO; src.len()];
        data.par_chunks_exact_mut(dw * 3)
            .enumerate()
            .for_each(|(dy, row)| {
                for dx in 0..dw {
                    let (sx, sy) = match exif_orientation {
                        2 => (w - 1 - dx, dy),
                        3 => (w - 1 - dx, h - 1 - dy),
                        4 => (dx, h - 1 - dy),
                        5 => (dy, dx),
                        6 => (dy, h - 1 - dx),
                        7 => (w - 1 - dy, h - 1 - dx),
                        _ => (w - 1 - dy, dx), // 8
                    };
                    let si = (sy * w + sx) * 3;
                    row[dx * 3..dx * 3 + 3].copy_from_slice(&src[si..si + 3]);
                }
            });
        Self {
            width: dw as u32,
            height: dh as u32,
            data,
        }
    }

    /// Rotate by `quarter_turns` × 90° clockwise (any integer; taken mod 4).
    pub fn rotated_cw(self, quarter_turns: i32) -> Self {
        let tag = match quarter_turns.rem_euclid(4) {
            1 => 6,
            2 => 3,
            3 => 8,
            _ => 1,
        };
        self.oriented(tag)
    }
}

impl std::fmt::Debug for ImageBuf {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ImageBuf({}x{}, {} pixels)",
            self.width,
            self.height,
            self.pixel_count()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precision::from_f64;

    #[test]
    fn test_new_and_dimensions() {
        let img = ImageBuf::new(4, 3);
        assert_eq!(img.width, 4);
        assert_eq!(img.height, 3);
        assert_eq!(img.data.len(), 4 * 3 * 3);
        assert_eq!(img.pixel_count(), 12);
    }

    #[test]
    fn test_get_set_pixel() {
        let mut img = ImageBuf::new(2, 2);
        img.set(1, 0, [from_f64(0.5), from_f64(0.6), from_f64(0.7)]);
        let px = img.get(1, 0);
        assert_eq!(px, [from_f64(0.5), from_f64(0.6), from_f64(0.7)]);
    }

    #[test]
    fn test_pixels_iter() {
        let img = ImageBuf::from_data(
            2,
            1,
            vec![
                from_f64(1.0),
                from_f64(2.0),
                from_f64(3.0),
                from_f64(4.0),
                from_f64(5.0),
                from_f64(6.0),
            ],
        );
        let pixels: Vec<&[Scalar]> = img.pixels().collect();
        assert_eq!(pixels.len(), 2);
        assert_eq!(pixels[0], &[from_f64(1.0), from_f64(2.0), from_f64(3.0)]);
        assert_eq!(pixels[1], &[from_f64(4.0), from_f64(5.0), from_f64(6.0)]);
    }

    /// 3×2 image where pixel (x, y) holds [x, y, 0].
    fn coord_img() -> ImageBuf {
        let mut img = ImageBuf::new(3, 2);
        for y in 0..2 {
            for x in 0..3 {
                img.set(
                    x,
                    y,
                    [from_f64(x as f64), from_f64(y as f64), from_f64(0.0)],
                );
            }
        }
        img
    }

    fn src_at(img: &ImageBuf, x: u32, y: u32) -> (u32, u32) {
        let p = img.get(x, y);
        (p[0] as u32, p[1] as u32)
    }

    #[test]
    fn test_rotate_cw() {
        let r = coord_img().rotated_cw(1);
        assert_eq!((r.width, r.height), (2, 3));
        // Top-left after a CW turn is the old bottom-left.
        assert_eq!(src_at(&r, 0, 0), (0, 1));
        // Top-right is the old top-left.
        assert_eq!(src_at(&r, 1, 0), (0, 0));
        // Bottom-right is the old top-right.
        assert_eq!(src_at(&r, 1, 2), (2, 0));
    }

    #[test]
    fn test_rotate_ccw_and_roundtrip() {
        let r = coord_img().rotated_cw(-1);
        assert_eq!((r.width, r.height), (2, 3));
        // Top-left after a CCW turn is the old top-right.
        assert_eq!(src_at(&r, 0, 0), (2, 0));
        let back = r.rotated_cw(1);
        assert_eq!(back.data, coord_img().data);
        let r180 = coord_img().rotated_cw(2);
        assert_eq!(src_at(&r180, 0, 0), (2, 1));
    }

    #[test]
    fn test_exif_flips() {
        assert_eq!(src_at(&coord_img().oriented(2), 0, 0), (2, 0));
        assert_eq!(src_at(&coord_img().oriented(4), 0, 0), (0, 1));
        let t = coord_img().oriented(5);
        assert_eq!((t.width, t.height), (2, 3));
        assert_eq!(src_at(&t, 1, 2), (2, 1));
        let tv = coord_img().oriented(7);
        assert_eq!(src_at(&tv, 0, 0), (2, 1));
    }

    #[test]
    fn test_orient_empty() {
        let r = ImageBuf::new(0, 5).rotated_cw(1);
        assert_eq!((r.width, r.height), (5, 0));
        assert!(r.data.is_empty());
    }

    #[test]
    fn test_extract_write_channel() {
        let mut img = ImageBuf::from_data(
            2,
            1,
            vec![
                from_f64(1.0),
                from_f64(2.0),
                from_f64(3.0),
                from_f64(4.0),
                from_f64(5.0),
                from_f64(6.0),
            ],
        );
        let g = img.extract_channel(1);
        assert_eq!(g, vec![from_f64(2.0), from_f64(5.0)]);
        img.write_channel(1, &[from_f64(10.0), from_f64(20.0)]);
        assert_eq!(
            img.get(0, 0),
            [from_f64(1.0), from_f64(10.0), from_f64(3.0)]
        );
        assert_eq!(
            img.get(1, 0),
            [from_f64(4.0), from_f64(20.0), from_f64(6.0)]
        );
    }
}
