//! Software counterpart of the canvas shaders. Uses the same ordered draw
//! list and uniforms; only presentation changes, never helper/export pixels.
use super::{Draw, background};
use std::{collections::HashMap, sync::Arc};

type Mip = (Arc<Vec<u8>>, u32, u32);
#[derive(Default)]
pub(super) struct Cache {
    mips: HashMap<(u64, u32), Mip>,
    /// The fitted view's exact-size pictures (ticket 38), the last two.
    exact: Vec<((u64, u32, u32), Arc<Vec<u8>>)>,
}
impl Cache {
    pub(super) fn has_exact(&self, key: (u64, u32, u32)) -> bool {
        self.exact.iter().any(|(k, _)| *k == key)
    }
    pub(super) fn add_exact(&mut self, key: (u64, u32, u32), pixels: Vec<u8>, _picture: u64) {
        if self.exact.len() >= 2 {
            self.exact.remove(0);
        }
        self.exact.push((key, Arc::new(pixels)));
    }
    // Cache only reduced levels, never a second copy of the full image.
    // Like the Vulkan mip chain, each level averages in linear light.
    fn mip(&mut self, picture: &crate::picture::Picture, footprint: f32) -> Option<Mip> {
        let level = footprint.max(1.0).log2().floor().max(0.0) as u32;
        let level = level.min((picture.width.max(picture.height) as f32).log2().floor() as u32);
        if level == 0 {
            return None;
        }
        if let Some(mip) = self.mips.get(&(picture.id, level)) {
            return Some(mip.clone());
        }
        let previous = self.mip(picture, 2.0_f32.powi(level as i32 - 1));
        let reduced = if let Some((bytes, w, h)) = previous {
            reduce(&bytes, w, h)
        } else {
            picture.with_pixels(|bytes| reduce(bytes, picture.width, picture.height))?
        };
        self.mips.insert((picture.id, level), reduced.clone());
        Some(reduced)
    }
}
fn reduce(bytes: &[u8], w: u32, h: u32) -> Mip {
    let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
    let lut: [f32; 256] = std::array::from_fn(|i| decode(i as f32 / 255.0));
    let mut result = Vec::with_capacity(nw as usize * nh as usize * 4);
    for y in 0..nh {
        for x in 0..nw {
            let mut sum = [0.0; 4];
            // Area averaging also includes the last column/row of odd images.
            let (x0, x1) = (x * w / nw, (x + 1) * w / nw);
            let (y0, y1) = (y * h / nh, (y + 1) * h / nh);
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let p = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        sum[c] += if c == 3 {
                            bytes[p + c] as f32 / 255.0
                        } else {
                            lut[bytes[p + c] as usize]
                        };
                    }
                }
            }
            let n = ((x1 - x0) * (y1 - y0)) as f32;
            for (c, v) in sum.into_iter().enumerate() {
                result.push((if c == 3 { v / n } else { encode(v / n) } * 255.0).round() as u8);
            }
        }
    }
    (Arc::new(result), nw, nh)
}
use crate::{
    gpu::{PhotoUniform, QuadInstance},
    icc::Transform,
};

fn decode(x: f32) -> f32 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}
fn present(rgb: [f32; 3], cm: Option<&Transform>) -> [f32; 3] {
    let Some(cm) = cm else { return rgb.map(encode) };
    std::array::from_fn(|c| {
        let m = (0..3).map(|j| cm.matrix[c][j] * rgb[j]).sum();
        let p = encode(m) * (cm.lut.len() - 1) as f32;
        let i = (p.floor() as usize).min(cm.lut.len() - 1);
        let j = (i + 1).min(cm.lut.len() - 1);
        cm.lut[i][c] + (cm.lut[j][c] - cm.lut[i][c]) * (p - i as f32)
    })
}

struct Raster {
    w: u32,
    h: u32,
    pixels: Vec<u8>,
    cm: Option<Transform>,
    decode: [f32; 256],
    /// Draws reach only inside this (pixels), as the GPU's scissor.
    clip: Option<[f32; 4]>,
}
impl Raster {
    fn blend(&mut self, x: u32, y: u32, linear: [f32; 3], alpha: f32) {
        let a = alpha.clamp(0.0, 1.0);
        if a == 0.0 {
            return;
        }
        let rgb = present(linear, self.cm.as_ref());
        let p = ((y * self.w + x) * 4) as usize;
        for c in 0..3 {
            // The Vulkan UNORM target blends encoded output in the same way.
            self.pixels[p + 2 - c] = (rgb[c] * 255.0 * a
                + self.pixels[p + 2 - c] as f32 * (1.0 - a))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
    }
    fn area(&mut self, r: [f32; 4], mut f: impl FnMut(&mut Self, u32, u32, f32, f32)) {
        let c = self.clip.unwrap_or([0.0, 0.0, self.w as f32, self.h as f32]);
        let x0 = r[0].max(c[0]).floor().max(0.0) as u32;
        let y0 = r[1].max(c[1]).floor().max(0.0) as u32;
        let x1 = (r[0] + r[2]).min(c[0] + c[2]).ceil().clamp(0.0, self.w as f32) as u32;
        let y1 = (r[1] + r[3]).min(c[1] + c[3]).ceil().clamp(0.0, self.h as f32) as u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                if px >= r[0] && py >= r[1] && px < r[0] + r[2] && py < r[1] + r[3] {
                    f(self, x, y, px, py);
                }
            }
        }
    }
    fn photo(&mut self, rgba: &[u8], w: u32, h: u32, p: &PhotoUniform) {
        let (s, k) = p.origin_scale[3].sin_cos();
        self.area(p.quad, |out, x, y, px, py| {
            let t = [
                (px - p.origin_scale[0]) / p.origin_scale[2],
                (py - p.origin_scale[1]) / p.origin_scale[2],
            ];
            let c = [p.shown[0] * 0.5, p.shown[1] * 0.5];
            let d = [t[0] - c[0], t[1] - c[1]];
            let g = [c[0] + d[0] * k + d[1] * s, c[1] - d[0] * s + d[1] * k];
            let mut alpha = 1.0;
            if p.shown[3] > 0.5 {
                let inside = g[0].min(p.shown[0] - g[0]).min(g[1]).min(p.shown[1] - g[1]);
                alpha = (inside * p.origin_scale[2] + 0.5).clamp(0.0, 1.0);
            }
            if alpha == 0.0 {
                return;
            }
            let mut uv = [g[0] / p.shown[0], g[1] / p.shown[1]];
            for _ in 0..p.shown[2] as u32 {
                uv = [uv[1], 1.0 - uv[0]];
            }
            let mut color = sample(rgba, w, h, uv, &out.decode);
            if p.extra[1] > 0.5 {
                let m = crate::gpu::REC2020_TO_SRGB;
                let rgb = [color[0], color[1], color[2]];
                for (c, row) in m.iter().enumerate() {
                    color[c] = row[0] * rgb[0] + row[1] * rgb[1] + row[2] * rgb[2];
                }
            }
            let shaded = p.shown[3] > 0.5
                && (t[0] < p.clip[0]
                    || t[1] < p.clip[1]
                    || t[0] > p.clip[0] + p.clip[2]
                    || t[1] > p.clip[1] + p.clip[3]);
            out.blend(x, y, [color[0], color[1], color[2]], alpha);
            if shaded {
                // rgba(8, 7, 6, .6) over it, as the shader does.
                let shade = [8.0, 7.0, 6.0].map(|c: f32| decode(c / 255.0));
                out.blend(x, y, shade, p.extra[0] * alpha);
            }
        });
    }
    fn quad(&mut self, q: &QuadInstance, texture: Option<(&[u8], u32, u32)>) {
        let base = [decode(q.color[0]), decode(q.color[1]), decode(q.color[2])];
        self.area(q.rect, |out, x, y, px, py| {
            let local = [px - q.rect[0], py - q.rect[1]];
            let mut alpha = q.color[3];
            let mut color = base;
            if let Some((rgba, w, h)) = texture {
                let mut uv = [local[0] / q.rect[2], local[1] / q.rect[3]];
                if q.params[2] as u32 & 2 != 0 {
                    uv = [1.0 - uv[1], uv[0]];
                }
                uv = [
                    q.uv[0] + (q.uv[2] - q.uv[0]) * uv[0],
                    q.uv[1] + (q.uv[3] - q.uv[1]) * uv[1],
                ];
                let texel = sample(rgba, w, h, uv, &out.decode);
                for c in 0..3 {
                    color[c] *= texel[c];
                }
                alpha *= texel[3];
            }
            let radius = q.params[0];
            let stroke = q.params[1];
            let blur = q.params[3];
            if blur > 0.0 {
                let a = (local[0] - q.rect[2] * 0.5).abs() - (q.rect[2] * 0.5 - 3.0 * blur) + radius;
                let b = (local[1] - q.rect[3] * 0.5).abs() - (q.rect[3] * 0.5 - 3.0 * blur) + radius;
                let d = a.max(0.0).hypot(b.max(0.0)) + a.max(b).min(0.0) - radius;
                alpha *= 0.5 * (1.0 - erf(d / (blur * std::f32::consts::SQRT_2)));
            } else if radius > 0.0 || stroke > 0.0 {
                let a = (local[0] - q.rect[2] * 0.5).abs() - q.rect[2] * 0.5 + radius;
                let b = (local[1] - q.rect[3] * 0.5).abs() - q.rect[3] * 0.5 + radius;
                let d = a.max(0.0).hypot(b.max(0.0)) + a.max(b).min(0.0) - radius;
                alpha *= (0.5 - d).clamp(0.0, 1.0);
                if stroke > 0.0 {
                    alpha *= (d + stroke + 0.5).clamp(0.0, 1.0);
                }
            }
            out.blend(x, y, color, alpha);
        });
    }
}

/// The error function, as the shader's (Abramowitz and Stegun 7.1.28).
fn erf(x: f32) -> f32 {
    let a = x.abs();
    let t = 1.0 + a * (0.070_523_08 + a * (0.042_282_01 + a * (0.009_270_527 + a * (0.000_152_014_3 + a * (0.000_276_567_2 + a * 0.000_043_063_8)))));
    x.signum() * (1.0 - 1.0 / t.powi(16))
}

/// Bilinear sampling in linear sRGB, including clamp-to-edge and straight alpha.
fn sample(bytes: &[u8], w: u32, h: u32, uv: [f32; 2], lut: &[f32; 256]) -> [f32; 4] {
    let p = [
        (uv[0] * w as f32 - 0.5).clamp(0.0, w.saturating_sub(1) as f32),
        (uv[1] * h as f32 - 0.5).clamp(0.0, h.saturating_sub(1) as f32),
    ];
    let (x, y) = (p[0].floor() as u32, p[1].floor() as u32);
    let mut result = [0.0; 4];
    for j in 0..2 {
        for i in 0..2 {
            let weight = if i == 0 {
                1.0 - p[0] + x as f32
            } else {
                p[0] - x as f32
            } * if j == 0 {
                1.0 - p[1] + y as f32
            } else {
                p[1] - y as f32
            };
            let at = (((y + j).min(h - 1) * w + (x + i).min(w - 1)) * 4) as usize;
            for c in 0..4 {
                result[c] += weight
                    * if c == 3 {
                        bytes[at + c] as f32 / 255.0
                    } else {
                        lut[bytes[at + c] as usize]
                    };
            }
        }
    }
    result
}

pub(super) fn render(
    w: u32,
    h: u32,
    dpi: u32,
    draws: &[Draw],
    cm: Option<&Transform>,
    cache: &mut Cache,
) -> Vec<u8> {
    let pixels = background()
        .iter()
        .rev()
        .copied()
        .chain([255])
        .cycle()
        .take(w as usize * h as usize * 4)
        .collect();
    render_onto(pixels, w, h, dpi, draws, cm, cache)
}

/// Draws `draws` over `pixels` (BGRA, `w` × `h`).
pub(super) fn render_onto(
    pixels: Vec<u8>,
    w: u32,
    h: u32,
    dpi: u32,
    draws: &[Draw],
    cm: Option<&Transform>,
    cache: &mut Cache,
) -> Vec<u8> {
    let mut out = Raster {
        w,
        h,
        pixels,
        cm: cm.cloned(),
        decode: std::array::from_fn(|i| decode(i as f32 / 255.0)),
        clip: None,
    };
    for draw in draws {
        match draw {
            Draw::Clip(r) => out.clip = r.map(|r| [r.x as f32, r.y as f32, r.width as f32, r.height as f32]),
            Draw::Glyph(glyph, color, (x, y)) => {
                let plain = crate::ui::theme::tokens().plain;
                let (gw, gh) = glyph.size();
                if let Some(mask) = crate::ui::d2d::shape_mask(gw as f32, gh as f32, dpi as f32 / 96.0, plain, |p| glyph.draw(p)) {
                    let rgb = [decode(color[0]), decode(color[1]), decode(color[2])];
                    let (x, y) = (x.round() as f32, y.round() as f32);
                    out.area([x, y, mask.width as f32, mask.height as f32], |out, px, py, sx, sy| {
                        let mx = (sx - x) as u32;
                        let my = (sy - y) as u32;
                        out.blend(px, py, rgb, color[3] * mask.coverage[(my * mask.width + mx) as usize] as f32 / 255.0);
                    });
                }
            }
            Draw::Photo(picture, p) => {
                let (sw, sh) = if p.shown[2] as u32 % 2 == 0 {
                    (p.shown[0], p.shown[1])
                } else {
                    (p.shown[1], p.shown[0])
                };
                let footprint =
                    (picture.width as f32 / sw).max(picture.height as f32 / sh) / p.origin_scale[2];
                if let Some((bytes, w, h)) = cache.mip(picture, footprint) {
                    out.photo(&bytes, w, h, p);
                } else {
                    picture.with_pixels(|bytes| out.photo(bytes, picture.width, picture.height, p));
                }
            }
            Draw::Exact(key, p) => {
                if let Some((_, bytes)) = cache.exact.iter().find(|(k, _)| k == key) {
                    let bytes = bytes.clone();
                    out.photo(&bytes, key.1, key.2, p);
                }
            }
            Draw::Fill(q) => out.quad(q, None),
            Draw::Texture(picture, q) => {
                let (sw, sh) = if q.params[2] as u32 & 2 != 0 {
                    (q.rect[3], q.rect[2])
                } else {
                    (q.rect[2], q.rect[3])
                };
                let footprint = (picture.width as f32 * (q.uv[2] - q.uv[0]).abs() / sw)
                    .max(picture.height as f32 * (q.uv[3] - q.uv[1]).abs() / sh);
                if let Some((bytes, w, h)) = cache.mip(picture, footprint) {
                    out.quad(q, Some((&bytes, w, h)));
                } else {
                    picture.with_pixels(|bytes| {
                        out.quad(q, Some((bytes, picture.width, picture.height)))
                    });
                }
            }
            Draw::Words(text, style, width, color, (x, y)) => {
                let plain = crate::ui::theme::tokens().plain;
                if let Some(mask) = crate::ui::d2d::mask(text, style, *width, crate::ui::d2d::Align::Center, dpi as f32 / 96.0, plain) {
                    let rgb = [decode(color[0]), decode(color[1]), decode(color[2])];
                    let (x, y) = (x.round() as f32, y.round() as f32);
                    out.area([x, y, mask.width as f32, mask.height as f32], |out, px, py, sx, sy| {
                        let mx = (sx - x) as u32;
                        let my = (sy - y) as u32;
                        out.blend(px, py, rgb, color[3] * mask.coverage[(my * mask.width + mx) as usize] as f32 / 255.0);
                    });
                }
            }
        }
    }
    if draws.iter().any(|d| matches!(d, Draw::Photo(..) | Draw::Texture(..))) {
        cache.mips.retain(|(id, _), _| {
            draws.iter().any(|d| match d {
                Draw::Photo(p, _) | Draw::Texture(p, _) => p.id == *id,
                _ => false,
            })
        });
    }
    out.pixels
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn interpolation_is_linear_light_and_edges_clamp() {
        let lut = std::array::from_fn(|i| decode(i as f32 / 255.0));
        let bytes = [0, 0, 0, 255, 255, 255, 255, 255];
        assert!((encode(sample(&bytes, 2, 1, [0.5, 0.5], &lut)[0]) - 0.735357).abs() < 1e-5);
        assert_eq!(
            sample(&bytes, 2, 1, [-1.0, 0.5], &lut),
            [0.0, 0.0, 0.0, 1.0]
        );
    }
    #[test]
    fn minification_averages_light_and_keeps_odd_edges() {
        let (bytes, w, h) = reduce(
            &[0, 0, 0, 255, 255, 255, 255, 255, 255, 255, 255, 255],
            3,
            1,
        );
        assert_eq!((w, h), (1, 1));
        assert!((bytes[0] as i32 - 213).abs() <= 1);
        assert_eq!(bytes[3], 255);
    }
    #[test]
    fn crop_shade_is_applied_in_linear_light() {
        let mut out = Raster {
            w: 2,
            h: 1,
            pixels: vec![0; 8],
            cm: None,
            decode: std::array::from_fn(|i| decode(i as f32 / 255.0)),
            clip: None,
        };
        let p = PhotoUniform {
            origin_scale: [0.0, 0.0, 1.0, 0.0],
            shown: [2.0, 1.0, 0.0, 1.0],
            clip: [0.0, 0.0, 1.0, 1.0],
            quad: [0.0, 0.0, 2.0, 1.0],
            extra: [0.6, 0.0, 0.0, 0.0],
        };
        out.photo(&[255; 8], 2, 1, &p);
        assert_eq!(out.pixels[0], 255);
        // rgba(8, 7, 6, .6) over white, mixed in linear light.
        assert!((out.pixels[4] as i32 - 170).abs() <= 1, "{}", out.pixels[4]);
    }
    #[test]
    fn quarter_turn_follows_photo_uniforms() {
        let bytes = [255, 0, 0, 255, 0, 255, 0, 255];
        let mut out = Raster {
            w: 1,
            h: 2,
            pixels: vec![0; 8],
            cm: None,
            decode: std::array::from_fn(|i| decode(i as f32 / 255.0)),
            clip: None,
        };
        let p = PhotoUniform {
            origin_scale: [0.0, 0.0, 1.0, 0.0],
            shown: [1.0, 2.0, 1.0, 0.0],
            quad: [0.0, 0.0, 1.0, 2.0],
            ..Default::default()
        };
        out.photo(&bytes, 2, 1, &p);
        assert_eq!(&out.pixels[..3], &[0, 0, 255]);
        assert_eq!(&out.pixels[4..7], &[0, 255, 0]);
    }
}
