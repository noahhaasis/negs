//! Scanner colour noise reduction: what a lab scanner's software does to
//! take the colour out of the grain while keeping its texture.
//!
//! A joint (cross) bilateral filter smooths the chroma with luma as the
//! guide: neighbours count less the more their luminance differs, so colour
//! edges that follow a luminance edge stay put. To be fast it runs on a grid
//! of `factor`-pixel block means, and joint bilateral upsampling (Kopf et
//! al. 2007) brings the smoothed chroma back to full resolution, weighting
//! the four nearest cells by how close their luma is to the pixel's. Block
//! means also average most of the grain out of the guide.
//!
//! The filter works on log2 values: film grain is a density fluctuation, so
//! it has about the same size there in every tone. Luma `L` is the
//! luminance-weighted mean of the three log channels, and chroma is each log
//! channel minus `L` (red and blue are filtered; green follows from the
//! weights). Each pixel's change is made on its own value (so 0 changes
//! nothing), and the result is rescaled to the pixel's original linear
//! luminance: the grain keeps its brightness pattern exactly and only loses
//! its colour.
//!
//! The CPU code here is the reference; the `chroma_denoise_*.wgsl` shaders
//! are the same arithmetic in f32, with the sums in the same order.
use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, to_f32};

/// Offset before the log: black sits 10 stops below white instead of at
/// minus infinity, where the noise of the darkest tones would dominate.
pub const LOG_OFFSET: f32 = 1.0 / 1024.0;
/// Range σ of the grid filter, in stops of block-mean luma. On 400-speed
/// grain at 24 MP (σ 0.01–0.08 in the block means, half frame included)
/// the colour left over is the same for any σ from 0.1 to 0.2: what remains
/// is coarser than the radius. So the smallest, which keeps edges best.
pub const RANGE_SIGMA: f32 = 0.1;
/// Range σ of the upsampling, in stops. The weights are normalised over
/// the nine cells, so a grainy pixel still takes its neighbourhood's colour,
/// and a pixel next to an edge takes its own side's.
pub const UPSAMPLE_SIGMA: f32 = 0.1;
/// Smaller radii are drawn at this one, in pixels: the grid is at least 2 px.
pub const MIN_RADIUS_PX: f32 = 2.0;

/// The prepared filter for one image.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChromaDenoiseParams {
    /// How far each pixel's chroma moves to the filtered chroma, 0..1.
    pub strength: f32,
    /// Grid cell size in pixels.
    pub factor: u32,
    /// The radius on the grid, in cells, and the window it needs.
    pub radius_cells: f32,
    pub radius: u32,
    /// `1 / (2σ²)` of the grid filter's and the upsampling's range kernels.
    pub range_k: f32,
    pub upsample_k: f32,
    /// The output space's luminance weights.
    pub luma: [f32; 3],
}

impl ChromaDenoiseParams {
    /// `amount` 0..100 as the control shows it, `radius_px` the radius at
    /// the render's size. `None` when it does nothing.
    pub fn new(amount: f32, radius_px: f32, luma: [f32; 3]) -> Option<Self> {
        if amount.is_nan() || amount <= 0.0 || radius_px.is_nan() || radius_px <= 0.0 || !radius_px.is_finite() {
            return None;
        }
        let radius_px = radius_px.max(MIN_RADIUS_PX);
        // Cells of at least a third of the radius: up to three either side
        // keep the window round and the grid small. (Rounding down instead
        // made 35 mm at 24 MP a 2 px grid, 23 ms on an M1 Pro instead of 17,
        // for no less colour left.)
        let factor = ((radius_px / 3.0).ceil() as u32).clamp(2, 64);
        let radius_cells = radius_px / factor as f32;
        let k = |sigma: f32| 0.5 / (sigma * sigma);
        Some(Self {
            strength: (amount / 100.0).min(1.0),
            factor,
            radius_cells,
            radius: radius_cells.ceil() as u32,
            range_k: k(RANGE_SIGMA),
            upsample_k: k(UPSAMPLE_SIGMA),
            luma,
        })
    }

    pub fn grid(&self, width: u32, height: u32) -> (u32, u32) {
        (width.div_ceil(self.factor), height.div_ceil(self.factor))
    }

    /// Bytes the two 3-channel grids take.
    pub fn grid_bytes(&self, width: u32, height: u32) -> u64 {
        let (gw, gh) = self.grid(width, height);
        u64::from(gw) * u64::from(gh) * 6 * 4
    }

    /// The spatial weight of a cell `d2` squared cells away: a disc that
    /// falls off in `d²` (Epanechnikov), 0 at the radius.
    #[inline]
    pub fn spatial_weight(&self, d2: u32) -> f32 {
        (1.0 - d2 as f32 / (self.radius_cells * self.radius_cells)).max(0.0)
    }
}

/// Log luma and the red and blue chroma of a linear pixel.
#[inline]
fn log_lcc(px: [f32; 3], w: [f32; 3]) -> [f32; 3] {
    let f = px.map(|v| (v.max(0.0) + LOG_OFFSET).log2());
    let l = w[0] * f[0] + w[1] * f[1] + w[2] * f[2];
    [l, f[0] - l, f[2] - l]
}

/// Block means of (L, cr, cb) on the grid.
fn block_means(image: &ImageBuf, p: &ChromaDenoiseParams) -> Vec<f32> {
    let (w, h) = (image.width as usize, image.height as usize);
    let k = p.factor as usize;
    let (gw, gh) = p.grid(image.width, image.height);
    let (gw, gh) = (gw as usize, gh as usize);
    let mut grid = vec![0.0f32; gw * gh * 3];
    grid.par_chunks_mut(gw * 3).enumerate().for_each(|(gy, row)| {
        let (y0, y1) = (gy * k, ((gy + 1) * k).min(h));
        for gx in 0..gw {
            let (x0, x1) = (gx * k, ((gx + 1) * k).min(w));
            let mut sum = [0.0f32; 3];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = (y * w + x) * 3;
                    let v = log_lcc([to_f32(image.data[i]), to_f32(image.data[i + 1]), to_f32(image.data[i + 2])], p.luma);
                    for c in 0..3 {
                        sum[c] += v[c];
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as f32;
            for c in 0..3 {
                row[gx * 3 + c] = sum[c] / n;
            }
        }
    });
    grid
}

/// The joint bilateral filter on the grid: each cell's chroma becomes the
/// weighted mean of its window's, the cell's own luma kept. Rows, then
/// columns of the window, as the shader sums.
fn bilateral(grid: &[f32], gw: usize, gh: usize, p: &ChromaDenoiseParams) -> Vec<f32> {
    let r = p.radius as i64;
    let mut out = vec![0.0f32; grid.len()];
    out.par_chunks_mut(gw * 3).enumerate().for_each(|(y, row)| {
        for x in 0..gw {
            let i = (y * gw + x) * 3;
            let l = grid[i];
            let (mut sw, mut sr, mut sb) = (0.0f32, 0.0f32, 0.0f32);
            for dy in -r..=r {
                let ny = y as i64 + dy;
                if ny < 0 || ny >= gh as i64 {
                    continue;
                }
                for dx in -r..=r {
                    let nx = x as i64 + dx;
                    if nx < 0 || nx >= gw as i64 {
                        continue;
                    }
                    let ws = p.spatial_weight((dx * dx + dy * dy) as u32);
                    if ws <= 0.0 {
                        continue;
                    }
                    let j = (ny as usize * gw + nx as usize) * 3;
                    let dl = grid[j] - l;
                    let wt = ws * (-dl * dl * p.range_k).exp();
                    sw += wt;
                    sr += wt * grid[j + 1];
                    sb += wt * grid[j + 2];
                }
            }
            row[x * 3] = l;
            row[x * 3 + 1] = sr / sw;
            row[x * 3 + 2] = sb / sw;
        }
    });
    out
}

/// A pixel's position on the grid, in cells (cell centres at integers), and
/// the nearest cell.
#[inline]
pub fn grid_position(x: u32, factor: u32) -> (f32, i64) {
    let u = (x as f32 + 0.5) / factor as f32 - 0.5;
    (u, u.round() as i64)
}

/// Upsampling's spatial weight of a cell `d` cells from the pixel along one
/// axis: a tent reaching 1.5 cells, so all of the 3 × 3 cells around the
/// nearest one count (bilinear weights would leave a pixel on a cell
/// centre only that cell, which on an edge holds both sides' colour).
#[inline]
pub fn tent(d: f32) -> f32 {
    (1.0 - d.abs() / 1.5).max(0.0)
}

/// One pixel with its chroma (cr, cb) moved toward `target`, at its own
/// linear luminance.
#[inline]
fn shift_chroma(px: [f32; 3], c: [f32; 2], target: [f32; 2], p: &ChromaDenoiseParams) -> [f32; 3] {
    let w = p.luma;
    let dr = p.strength * (target[0] - c[0]);
    let db = p.strength * (target[1] - c[1]);
    let dg = -(w[0] * dr + w[2] * db) / w[1];
    let d = [dr, dg, db];
    let out: [f32; 3] = std::array::from_fn(|i| px[i] + (px[i].max(0.0) + LOG_OFFSET) * (d[i].exp2() - 1.0));
    let y = w[0] * px[0] + w[1] * px[1] + w[2] * px[2];
    let y_out = w[0] * out[0] + w[1] * out[1] + w[2] * out[2];
    if y > 0.0 && y_out > 1e-8 {
        let k = y / y_out;
        out.map(|v| v * k)
    } else {
        out
    }
}

/// Apply the filter in place.
pub fn apply(image: &mut ImageBuf, p: &ChromaDenoiseParams) {
    let (w, h) = (image.width, image.height);
    if w == 0 || h == 0 {
        return;
    }
    let (gw, gh) = p.grid(w, h);
    let grid = bilateral(&block_means(image, p), gw as usize, gh as usize, p);
    image.data.par_chunks_mut(w as usize * 3).enumerate().for_each(|(y, row)| {
        let (v, cy) = grid_position(y as u32, p.factor);
        for x in 0..w {
            let (u, cx) = grid_position(x, p.factor);
            let i = x as usize * 3;
            let px = [to_f32(row[i]), to_f32(row[i + 1]), to_f32(row[i + 2])];
            let [l, cr, cb] = log_lcc(px, p.luma);
            // Joint bilateral upsampling over the 3 × 3 cells around the
            // nearest, rows then columns as the shader sums.
            let (mut sw, mut sr, mut sb) = (0.0f32, 0.0f32, 0.0f32);
            let (mut fw, mut fr, mut fb) = (0.0f32, 0.0f32, 0.0f32);
            for gy in cy - 1..=cy + 1 {
                if gy < 0 || gy >= gh as i64 {
                    continue;
                }
                let wy = tent(gy as f32 - v);
                for gx in cx - 1..=cx + 1 {
                    if gx < 0 || gx >= gw as i64 {
                        continue;
                    }
                    let ws = wy * tent(gx as f32 - u);
                    let j = (gy as usize * gw as usize + gx as usize) * 3;
                    let dl = grid[j] - l;
                    let wt = ws * (-dl * dl * p.upsample_k).exp();
                    sw += wt;
                    sr += wt * grid[j + 1];
                    sb += wt * grid[j + 2];
                    fw += ws;
                    fr += ws * grid[j + 1];
                    fb += ws * grid[j + 2];
                }
            }
            // Far from all nine cells' luma (a lone speck): spatial weights only.
            let target = if sw > 1e-20 { [sr / sw, sb / sw] } else { [fr / fw, fb / fw] };
            let out = shift_chroma(px, [cr, cb], target, p);
            for c in 0..3 {
                row[i + c] = from_f32(out[c]);
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRGB_Y: [f32; 3] = [0.2126, 0.7152, 0.0722];

    fn noise(seed: &mut u64) -> f32 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 40) as f32 / (1u64 << 24) as f32) - 0.5
    }

    /// Independent per-channel multiplicative noise, as dye-layer grain
    /// gives, on mid grey or on a red | blue edge down the middle.
    fn grainy(width: u32, height: u32, edge: bool) -> ImageBuf {
        let mut seed = 7;
        let mut image = ImageBuf::new(width, height);
        for y in 0..height {
            for x in 0..width {
                let base = if edge && x >= width / 2 { [0.08, 0.12, 0.4] } else if edge { [0.4, 0.12, 0.08] } else { [0.18; 3] };
                let i = ((y * width + x) * 3) as usize;
                for (c, b) in base.iter().enumerate() {
                    image.data[i + c] = from_f32(b * (0.25 * noise(&mut seed)).exp2());
                }
            }
        }
        image
    }

    fn pixel(image: &ImageBuf, x: u32, y: u32) -> [f32; 3] {
        let i = ((y * image.width + x) * 3) as usize;
        [to_f32(image.data[i]), to_f32(image.data[i + 1]), to_f32(image.data[i + 2])]
    }

    fn luminance(px: [f32; 3]) -> f32 {
        SRGB_Y[0] * px[0] + SRGB_Y[1] * px[1] + SRGB_Y[2] * px[2]
    }

    fn chroma_spread(image: &ImageBuf, x0: u32, x1: u32) -> f32 {
        let values: Vec<f32> =
            (0..image.height).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| log_lcc(pixel(image, x, y), SRGB_Y)[1]).collect();
        let mean = values.iter().sum::<f32>() / values.len() as f32;
        (values.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / values.len() as f32).sqrt()
    }

    /// Mean red chroma of a column.
    fn column(image: &ImageBuf, x: u32) -> f32 {
        (0..image.height).map(|y| log_lcc(pixel(image, x, y), SRGB_Y)[1]).sum::<f32>() / image.height as f32
    }

    #[test]
    fn zero_is_off() {
        assert_eq!(ChromaDenoiseParams::new(0.0, 5.0, SRGB_Y), None);
        assert_eq!(ChromaDenoiseParams::new(50.0, 0.0, SRGB_Y), None);
    }

    #[test]
    fn keeps_every_pixel_s_luminance_and_removes_the_colour_noise() {
        let before = grainy(96, 64, false);
        let mut after = before.clone();
        apply(&mut after, &ChromaDenoiseParams::new(100.0, 6.0, SRGB_Y).unwrap());
        for (y, x) in (0..64).flat_map(|y| (0..96).map(move |x| (y, x))) {
            let (a, b) = (luminance(pixel(&before, x, y)), luminance(pixel(&after, x, y)));
            assert!((a - b).abs() <= 1e-6 * a.max(1.0), "({x}, {y}): {a} → {b}");
        }
        let (s0, s1) = (chroma_spread(&before, 0, 96), chroma_spread(&after, 0, 96));
        assert!(s1 < 0.15 * s0, "chroma spread {s0} → {s1}");
    }

    #[test]
    fn any_size_works() {
        for (w, h) in [(1, 1), (1, 7), (5, 1), (3, 2), (17, 9)] {
            let mut image = grainy(w, h, false);
            let before = image.clone();
            apply(&mut image, &ChromaDenoiseParams::new(100.0, 12.0, SRGB_Y).unwrap());
            assert!(image.data.iter().all(|v| v.is_finite()), "{w}×{h}");
            for (y, x) in (0..h).flat_map(|y| (0..w).map(move |x| (y, x))) {
                let (a, b) = (luminance(pixel(&before, x, y)), luminance(pixel(&image, x, y)));
                assert!((a - b).abs() <= 1e-6, "{w}×{h} ({x}, {y}): {a} → {b}");
            }
        }
    }

    #[test]
    fn black_and_negative_pixels_stay_finite() {
        let mut image = ImageBuf::from_data(4, 2, [0.0, 0.0, 0.0, -0.01, 0.2, 0.1, 1e-9, 0.0, 0.0, 3.0, 2.5, 0.0, 0.2, 0.2, 0.2, -0.5, -0.5, -0.5, 0.0, 1.0, 0.0, 0.1, 0.1, 0.1].map(from_f32).to_vec());
        apply(&mut image, &ChromaDenoiseParams::new(100.0, 4.0, SRGB_Y).unwrap());
        assert!(image.data.iter().all(|v| v.is_finite()), "{:?}", image.data);
    }

    #[test]
    fn half_strength_removes_about_half() {
        let before = grainy(96, 64, false);
        let mut after = before.clone();
        apply(&mut after, &ChromaDenoiseParams::new(50.0, 6.0, SRGB_Y).unwrap());
        let (s0, s1) = (chroma_spread(&before, 0, 96), chroma_spread(&after, 0, 96));
        assert!((0.45..0.6).contains(&(s1 / s0)), "chroma spread {s0} → {s1}");
    }

    #[test]
    fn a_colour_edge_on_a_luma_edge_stays_put() {
        // The two sides differ by 0.43 stops of luminance and 2.2 of chroma.
        let before = grainy(128, 64, true);
        let mut after = before.clone();
        apply(&mut after, &ChromaDenoiseParams::new(100.0, 8.0, SRGB_Y).unwrap());
        // A shift of up to 0.4 of the 2.2 stops right at the edge (the pixels
        // there are as close in luma to the other side as to their own),
        // gone 4 px away.
        for x in 56..72 {
            let (b, a) = (column(&before, x), column(&after, x));
            let limit = if (60..68).contains(&x) { 0.4 } else { 0.15 };
            assert!((a - b).abs() < limit, "column {x}: {b} → {a}");
        }
        // Away from the edge the noise is gone.
        assert!(chroma_spread(&after, 8, 48) < 0.15 * chroma_spread(&before, 8, 48));
    }
}

