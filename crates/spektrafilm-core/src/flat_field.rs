//! The flat field: a frame of the bare light (no film), shot with the roll's
//! camera, lens and aperture, that each scan of the roll is divided by, so
//! the light's and the lens's falloff don't read as density. Negs' own,
//! after the idea in flx-film-convert's notes ("Flat field"), not its code.
//!
//! - The light frame is read once ([`FlatField::load`]), reduced to
//!   [`FIELD_SIDE`] on its long side by averaging (the sensor's noise goes),
//!   cleaned of dust with a median ([`MEDIAN_RADIUS`]) and smoothed
//!   ([`SIGMA`]), then each channel is scaled to 1 at its brightest. What
//!   is left is the shape of the light, not its level or its colour.
//! - A scan is divided by it as it is read, before anything measures it:
//!   the base, the rebate finder, the auto print and the render all see the
//!   same flattened scan ([`FlatField::divide`]).
//! - Only a scan shot with the light frame's camera, lens, aperture and focal
//!   length (by EXIF), and of its shape, is divided; another gets a reason
//!   ([`FlatField::mismatch`]).
//! - The field is kept in the sensor's own orientation, so a scan the camera
//!   tagged as turned (pointing down a copy stand, its level sensor guesses)
//!   still meets the light frame the way the sensor saw both.
//! - Where the light frame is darker than [`FLOOR`] of its brightest (the
//!   film holder's mask, say), the scan is lifted no more than that.

use std::path::Path;

use rayon::prelude::*;
use spektrafilm_math::colorspace::{Cctf, RgbColourspace};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f64, to_f64};

use crate::params::RuntimeParams;

/// The field's long side in pixels: falloff is smooth, so this holds it.
pub const FIELD_SIDE: u32 = 256;
/// The light frame is read no larger than this on its long side.
pub const READ_SIDE: u32 = 2048;
/// Dust specks up to this many field pixels across (about 1% of the long
/// side) are taken out by a median.
pub const MEDIAN_RADIUS: usize = 3;
/// Then the field is smoothed by a Gaussian this wide, in field pixels.
pub const SIGMA: f64 = 3.0;
/// The darkest the field counts, as a fraction of its brightest: a scan is
/// lifted by three stops at most.
pub const FLOOR: f64 = 0.125;
/// Apertures this close (as a ratio) are the same.
const APERTURE_TOLERANCE: f64 = 0.01;
/// Frames whose shapes (width over height) differ by more than this aren't
/// the same frame.
const SHAPE_TOLERANCE: f64 = 0.01;

/// What a frame was shot with, from its EXIF: what a light frame and a
/// scan must share.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Shot {
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub f_number: Option<f64>,
    pub focal_length: Option<f64>,
    /// EXIF orientation, 1 to 8 (1 when it isn't said).
    pub orientation: u16,
}

impl Shot {
    /// Read from the file's EXIF; an empty [`Shot`] when it has none.
    pub fn read(path: &Path) -> Self {
        let Some(meta) = spektrafilm_io::read_metadata(path) else {
            return Self { orientation: 1, ..Default::default() };
        };
        let text = |s: &Option<String>| s.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        let camera = match (text(&meta.make), text(&meta.model)) {
            (None, None) => None,
            (make, model) => Some(format!("{} {}", make.unwrap_or_default(), model.unwrap_or_default()).trim().to_string()),
        };
        let ratio = |n: u32, d: u32| Some(n as f64 / d as f64).filter(|v| d != 0 && *v > 0.0);
        Self {
            camera,
            lens: text(&meta.exif.lens_model),
            f_number: meta.exif.fnumber.and_then(|r| ratio(r.n, r.d)),
            focal_length: meta.exif.focal_length.and_then(|r| ratio(r.n, r.d)),
            orientation: meta.exif.orientation.filter(|o| (1..=8).contains(o)).unwrap_or(1),
        }
    }
}

/// A light frame, ready to divide scans by.
#[derive(Debug, Clone)]
pub struct FlatField {
    /// The light, smoothed, each channel 1 at its brightest, in the
    /// sensor's orientation.
    field: ImageBuf,
    /// What the light frame was shot with.
    pub shot: Shot,
}

/// Whether a flat field was used on a scan, and why not.
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    pub used: bool,
    /// For Prep, when it wasn't used.
    pub reason: Option<String>,
}

impl Applied {
    pub fn not_used(reason: impl Into<String>) -> Self {
        Self { used: false, reason: Some(reason.into()) }
    }
}

/// The EXIF orientation that undoes `tag`.
fn inverse_orientation(tag: u16) -> u16 {
    match tag {
        6 => 8,
        8 => 6,
        t => t,
    }
}

fn format_f_number(f: f64) -> String {
    let rounded = (f * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 { format!("f/{rounded:.0}") } else { format!("f/{rounded:.1}") }
}

impl FlatField {
    /// Reads the light frame at `path` as a scan would be read (the same
    /// RAW white balance, lens correction and interpretation, from
    /// `params.io`) and makes its field.
    pub fn load(path: &Path, params: &RuntimeParams) -> Result<Self, String> {
        if !path.is_file() {
            return Err(format!("The light frame isn't there any more ({}).", path.display()));
        }
        let io = &params.io;
        let loaded = spektrafilm_io::load_image_colour_limited(
            path,
            spektrafilm_io::Formats::Any,
            io.raw_white_balance,
            io.lens_correction,
            &io.input_interpretation,
            READ_SIDE,
        )
        .map_err(|e| format!("The light frame couldn't be read: {e}"))?;
        let mut light_params = params.clone();
        light_params.io.apply_input_colour(&loaded.colour);
        let mut light = loaded.image;
        crate::stages::filming::decode_input_cctf(&mut light, &light_params);
        Self::from_light(&light, Shot::read(path))
    }

    /// The field of `light`, a light frame decoded upright and linear, shot
    /// as `shot` says.
    pub fn from_light(light: &ImageBuf, shot: Shot) -> Result<Self, String> {
        if light.width < 2 || light.height < 2 {
            return Err("The light frame is too small.".into());
        }
        // Back to the sensor's orientation.
        let light = if shot.orientation == 1 { light.clone() } else { light.clone().oriented(inverse_orientation(shot.orientation)) };
        let long = light.width.max(light.height);
        let small = if long > FIELD_SIDE {
            let scale = FIELD_SIDE as f64 / long as f64;
            let size = |v: u32| ((v as f64 * scale).round() as u32).max(1);
            crate::preview::downsample_area(&light, (size(light.width), size(light.height)))
        } else {
            light
        };
        let (w, h) = (small.width as usize, small.height as usize);
        let mut field = ImageBuf::new(small.width, small.height);
        for c in 0..3 {
            let channel: Vec<f64> = (0..w * h).map(|i| to_f64(small.data[i * 3 + c])).collect();
            if !channel.iter().all(|v| v.is_finite()) {
                return Err("The light frame has pixels that aren't numbers.".into());
            }
            let smooth = gaussian(&median(&channel, w, h, MEDIAN_RADIUS), w, h, SIGMA);
            let brightest = smooth.iter().copied().fold(0.0, f64::max);
            if brightest <= 0.0 {
                return Err("The light frame is black.".into());
            }
            for (i, v) in smooth.iter().enumerate() {
                field.data[i * 3 + c] = from_f64((v / brightest).max(FLOOR));
            }
        }
        Ok(Self { field, shot })
    }

    /// Why a scan shot as `scan` can't use this light frame, or `None`
    /// when it can. `size` is the scan's size as decoded (upright).
    pub fn mismatch(&self, scan: &Shot, size: (u32, u32)) -> Option<String> {
        let light = &self.shot;
        if light.camera != scan.camera {
            return Some("The light frame was shot with a different camera.".into());
        }
        if light.lens != scan.lens {
            return Some("The light frame was shot with a different lens.".into());
        }
        match (light.f_number, scan.f_number) {
            (Some(a), Some(b)) if (a / b - 1.0).abs() > APERTURE_TOLERANCE => {
                return Some(format!(
                    "The light frame was shot at {}; this frame at {}.",
                    format_f_number(a),
                    format_f_number(b)
                ));
            }
            (Some(_), None) | (None, Some(_)) => return Some("The light frame's aperture can't be compared with this frame's.".into()),
            _ => {}
        }
        match (light.focal_length, scan.focal_length) {
            (Some(a), Some(b)) if (a / b - 1.0).abs() > APERTURE_TOLERANCE => {
                return Some("The light frame was shot at a different focal length.".into());
            }
            _ => {}
        }
        // Both in the sensor's orientation.
        let sensor = |(w, h): (u32, u32), orientation: u16| if orientation >= 5 { (h, w) } else { (w, h) };
        let (sw, sh) = sensor(size, scan.orientation);
        let (fw, fh) = (self.field.width, self.field.height);
        let shape = |w: u32, h: u32| w as f64 / h.max(1) as f64;
        // The field's own rounding to FIELD_SIDE moves its shape a little.
        let rounding = 1.0 / fw.min(fh).max(1) as f64;
        if (shape(sw, sh) / shape(fw, fh) - 1.0).abs() > SHAPE_TOLERANCE + rounding {
            return Some("The light frame isn't the shape of this frame.".into());
        }
        None
    }

    /// Divides `image`, a scan shot as `scan`, by the field, as it is read:
    /// still in its input CCTF (`params.io`), so linear light is divided.
    /// Call [`Self::mismatch`] first.
    pub fn divide(&self, image: &mut ImageBuf, scan: &Shot, params: &RuntimeParams) {
        let field = if scan.orientation == 1 { self.field.clone() } else { self.field.clone().oriented(scan.orientation) };
        let cctf = if params.io.input_cctf_decoding {
            RgbColourspace::input(&params.io.input_color_space).cctf
        } else {
            Cctf::Linear
        };
        let (w, h) = (image.width as usize, image.height as usize);
        let (fw, fh) = (field.width as usize, field.height as usize);
        // Where each image column falls on the field: pixel centres to pixel
        // centres, clamped at the edges.
        let taps = |n: usize, fnum: usize| -> Vec<(usize, usize, f64)> {
            (0..n)
                .map(|i| {
                    let u = ((i as f64 + 0.5) * fnum as f64 / n as f64 - 0.5).clamp(0.0, (fnum - 1) as f64);
                    let i0 = u.floor() as usize;
                    let i1 = (i0 + 1).min(fnum - 1);
                    (i0, i1, u - i0 as f64)
                })
                .collect()
        };
        let (xs, ys) = (taps(w, fw), taps(h, fh));
        let f = |x: usize, y: usize, c: usize| to_f64(field.data[(y * fw + x) * 3 + c]);
        image.data.par_chunks_exact_mut(w * 3).enumerate().for_each(|(y, row)| {
            let (y0, y1, ty) = ys[y];
            for (x, &(x0, x1, tx)) in xs.iter().enumerate() {
                for c in 0..3 {
                    let top = f(x0, y0, c) * (1.0 - tx) + f(x1, y0, c) * tx;
                    let bottom = f(x0, y1, c) * (1.0 - tx) + f(x1, y1, c) * tx;
                    let gain = top * (1.0 - ty) + bottom * ty;
                    let v = &mut row[x * 3 + c];
                    *v = from_f64(match cctf {
                        Cctf::Linear => to_f64(*v) / gain,
                        cctf => cctf.encode(cctf.decode(to_f64(*v)) / gain),
                    });
                }
            }
        });
    }

    /// Divides `image` (the scan at `path`, as decoded) by this field when
    /// the scan was shot as the light frame was; else says why not.
    pub fn flatten(&self, image: &mut ImageBuf, path: &Path, params: &RuntimeParams) -> Applied {
        let scan = Shot::read(path);
        if let Some(reason) = self.mismatch(&scan, (image.width, image.height)) {
            return Applied::not_used(reason);
        }
        self.divide(image, &scan, params);
        Applied { used: true, reason: None }
    }

    /// The field: the light, smoothed, each channel 1 at its brightest
    /// (never under [`FLOOR`]), in the sensor's orientation.
    pub fn field(&self) -> &ImageBuf {
        &self.field
    }
}

/// The light frame a render of a scan should be divided by: on the convert
/// route, with `film_render.convert.flat_field` set. `None` is off.
pub fn requested(params: &RuntimeParams) -> Option<&Path> {
    let path = params.film_render.convert.flat_field.trim();
    (params.workflow.converts_film() && !path.is_empty()).then(|| Path::new(path))
}

/// `data[y * w + x]`, carried on past the edges by point reflection
/// (`v(-d) = 2 v(0) - v(d)`), which keeps a steady slope going: falloff
/// goes on falling past the frame's edge, so neither the median nor the
/// Gaussian lifts the edge towards the middle. `x` and `y` may be at most
/// a width or height outside.
fn reflected(data: &[f64], w: usize, h: usize, x: isize, y: isize) -> f64 {
    let (wi, hi) = (w as isize, h as isize);
    if x < 0 {
        return 2.0 * reflected(data, w, h, 0, y) - reflected(data, w, h, (-x).min(wi - 1), y);
    }
    if x >= wi {
        return 2.0 * reflected(data, w, h, wi - 1, y) - reflected(data, w, h, (2 * (wi - 1) - x).max(0), y);
    }
    if y < 0 {
        return 2.0 * data[x as usize] - reflected(data, w, h, x, (-y).min(hi - 1));
    }
    if y >= hi {
        return 2.0 * data[(hi as usize - 1) * w + x as usize] - reflected(data, w, h, x, (2 * (hi - 1) - y).max(0));
    }
    data[y as usize * w + x as usize]
}

/// A median over a square `2 * radius + 1` wide.
fn median(data: &[f64], w: usize, h: usize, radius: usize) -> Vec<f64> {
    let r = radius as isize;
    (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = ((i % w) as isize, (i / w) as isize);
            let mut window = Vec::with_capacity((2 * radius + 1).pow(2));
            for dy in -r..=r {
                for dx in -r..=r {
                    window.push(reflected(data, w, h, x + dx, y + dy));
                }
            }
            let mid = window.len() / 2;
            *window.select_nth_unstable_by(mid, f64::total_cmp).1
        })
        .collect()
}

/// A separable Gaussian.
fn gaussian(data: &[f64], w: usize, h: usize, sigma: f64) -> Vec<f64> {
    let r = (3.0 * sigma).ceil() as isize;
    let kernel: Vec<f64> = (-r..=r).map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f64 = kernel.iter().sum();
    let kernel: Vec<f64> = kernel.iter().map(|k| k / total).collect();
    let pass = |src: &[f64], horizontal: bool| -> Vec<f64> {
        (0..w * h)
            .into_par_iter()
            .map(|i| {
                let (x, y) = ((i % w) as isize, (i / w) as isize);
                kernel
                    .iter()
                    .enumerate()
                    .map(|(k, weight)| {
                        let d = k as isize - r;
                        let (sx, sy) = if horizontal { (x + d, y) } else { (x, y + d) };
                        weight * reflected(src, w, h, sx, sy)
                    })
                    .sum()
            })
            .collect()
    };
    pass(&pass(data, true), false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A light falling off as cos⁴ of the angle off the axis, with a lens
    /// whose half diagonal sees `half_angle` radians; 1 in the middle.
    fn falloff(x: f64, y: f64, w: f64, h: f64, half_angle: f64) -> f64 {
        let (dx, dy) = (x - w / 2.0, y - h / 2.0);
        let r = (dx * dx + dy * dy).sqrt() / (w * w + h * h).sqrt() * 2.0;
        (r * half_angle).cos().powi(4)
    }

    fn image(w: u32, h: u32, mut f: impl FnMut(u32, u32) -> [f64; 3]) -> ImageBuf {
        let mut img = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let p = f(x, y);
                img.set(x, y, [from_f64(p[0]), from_f64(p[1]), from_f64(p[2])]);
            }
        }
        img
    }

    fn shot() -> Shot {
        Shot {
            camera: Some("NIKON Z f".into()),
            lens: Some("60mm macro".into()),
            f_number: Some(8.0),
            focal_length: Some(60.0),
            orientation: 1,
        }
    }

    fn linear_params() -> RuntimeParams {
        let mut params = RuntimeParams::default();
        params.io.input_cctf_decoding = false;
        params
    }

    /// Light falling off, warmer to the edges, a dust speck and 1% noise
    /// (a real light frame has far more pixels to each of the field's, so
    /// far less noise there).
    fn light_frame(w: u32, h: u32) -> ImageBuf {
        let mut seed = 1u64;
        let mut noise = move || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            1.0 + ((seed >> 33) as f64 / (1u64 << 31) as f64 - 0.5) * 0.02
        };
        image(w, h, |x, y| {
            let f = falloff(x as f64, y as f64, w as f64, h as f64, 0.6);
            let dust = if (x as i32 - 300).abs() < 4 && (y as i32 - 100).abs() < 4 { 0.3 } else { 1.0 };
            [0.8 * f.powf(0.9) * dust * noise(), 0.7 * f * dust * noise(), 0.5 * f.powf(1.1) * dust * noise()]
        })
    }

    #[test]
    fn dividing_a_scan_by_its_light_frame_takes_out_the_falloff() {
        let (w, h) = (600, 400);
        let flat = FlatField::from_light(&light_frame(w, h), shot()).unwrap();
        // An even grey shot through the same light and lens.
        let mut scan = image(w, h, |x, y| {
            let f = falloff(x as f64, y as f64, w as f64, h as f64, 0.6);
            [0.3 * f.powf(0.9), 0.2 * f, 0.1 * f.powf(1.1)]
        });
        let corner = scan.get(2, 2);
        assert!(to_f64(corner[1]) < 0.2 * 0.75, "the corner falls off");
        flat.divide(&mut scan, &shot(), &linear_params());
        // Even again, at the brightest point's level, dust and all.
        let centre = scan.get(w / 2, h / 2).map(to_f64);
        for (x, y) in [(2, 2), (w - 3, 2), (w / 2, h - 3), (300, 100), (w - 3, h - 3)] {
            let p = scan.get(x, y).map(to_f64);
            for c in 0..3 {
                assert!((p[c] / centre[c] - 1.0).abs() < 0.01, "({x}, {y}) channel {c}: {} against {}", p[c], centre[c]);
            }
        }
        assert!((centre[1] / 0.2 - 1.0).abs() < 0.01, "{}", centre[1]);
    }

    #[test]
    fn a_turned_scan_meets_the_light_frame_as_the_sensor_saw_both() {
        let (w, h) = (600, 400);
        // Light brighter to the sensor's left.
        let lit = |x: u32| 0.5 + 0.5 * (1.0 - x as f64 / w as f64);
        let light = image(w, h, |x, _| [lit(x); 3]);
        let flat = FlatField::from_light(&light, shot()).unwrap();
        // The scan, tagged as turned (6: shown turned a quarter clockwise).
        let turned = Shot { orientation: 6, ..shot() };
        let mut scan = image(w, h, |x, _| [0.2 * lit(x); 3]).oriented(6);
        assert_eq!(flat.mismatch(&turned, (scan.width, scan.height)), None);
        flat.divide(&mut scan, &turned, &linear_params());
        let (a, b) = (to_f64(scan.get(5, 5)[0]), to_f64(scan.get(scan.width - 6, scan.height - 6)[0]));
        assert!((a / b - 1.0).abs() < 0.01, "{a} {b}");
    }

    #[test]
    fn an_encoded_scan_is_divided_in_linear_light() {
        let light = image(64, 48, |x, _| [if x < 32 { 1.0 } else { 0.5 }; 3]);
        let flat = FlatField::from_light(&light, shot()).unwrap();
        let mut params = RuntimeParams::default();
        params.io.input_cctf_decoding = true;
        params.io.input_color_space = "sRGB".into();
        let cctf = RgbColourspace::input("sRGB").cctf;
        let mut scan = image(64, 48, |_, _| [cctf.encode(0.25); 3]);
        flat.divide(&mut scan, &shot(), &params);
        let right = cctf.decode(to_f64(scan.get(63, 24)[0]));
        assert!((right - 0.5).abs() < 1e-3, "{right}");
    }

    #[test]
    fn a_scan_shot_otherwise_says_why_it_isnt_divided() {
        let flat = FlatField::from_light(&light_frame(300, 200), shot()).unwrap();
        let size = (3000, 2000);
        assert_eq!(flat.mismatch(&shot(), size), None);
        let lens = Shot { lens: Some("50mm".into()), ..shot() };
        assert_eq!(flat.mismatch(&lens, size).unwrap(), "The light frame was shot with a different lens.");
        let body = Shot { camera: Some("NIKON Z 7".into()), ..shot() };
        assert_eq!(flat.mismatch(&body, size).unwrap(), "The light frame was shot with a different camera.");
        let stop = Shot { f_number: Some(11.0), ..shot() };
        assert_eq!(flat.mismatch(&stop, size).unwrap(), "The light frame was shot at f/8; this frame at f/11.");
        let half = Shot { f_number: Some(5.6), ..shot() };
        assert_eq!(flat.mismatch(&half, size).unwrap(), "The light frame was shot at f/8; this frame at f/5.6.");
        let zoom = Shot { focal_length: Some(40.0), ..shot() };
        assert!(flat.mismatch(&zoom, size).is_some());
        assert_eq!(flat.mismatch(&shot(), (3000, 3000)).unwrap(), "The light frame isn't the shape of this frame.");
        // Turned by its tag, the same frame.
        assert_eq!(flat.mismatch(&Shot { orientation: 8, ..shot() }, (2000, 3000)), None);
        // Neither says anything: nothing to tell them apart.
        let blank = FlatField::from_light(&light_frame(300, 200), Shot { orientation: 1, ..Default::default() }).unwrap();
        assert_eq!(blank.mismatch(&Shot { orientation: 1, ..Default::default() }, size), None);
    }

    #[test]
    fn the_holders_mask_is_lifted_no_more_than_the_floor() {
        let light = image(200, 100, |x, _| [if x < 20 { 0.0 } else { 1.0 }; 3]);
        let flat = FlatField::from_light(&light, shot()).unwrap();
        let mut scan = image(200, 100, |_, _| [0.01; 3]);
        flat.divide(&mut scan, &shot(), &linear_params());
        assert!((to_f64(scan.get(0, 50)[0]) - 0.01 / FLOOR).abs() < 1e-6);
        assert!((to_f64(scan.get(199, 50)[0]) - 0.01).abs() < 1e-6);
    }

    #[test]
    fn off_unless_a_negative_names_a_light_frame() {
        let mut params = RuntimeParams::default();
        params.film_render.convert.flat_field = "/light.nef".into();
        assert_eq!(requested(&params), None, "a photograph isn't divided");
        params.workflow.route = crate::params::ROUTE_CONVERT_PRINT.into();
        assert_eq!(requested(&params), Some(Path::new("/light.nef")));
        params.film_render.convert.flat_field = String::new();
        assert_eq!(requested(&params), None);
        // Off is written as empty, so the apps' defaults carry the key and
        // keep an edit's light frame when they restore it; params without
        // it read as off.
        let json = serde_json::to_value(RuntimeParams::default()).unwrap();
        assert_eq!(json["film_render"]["convert"]["flat_field"], "");
        let mut saved = serde_json::to_value(&params).unwrap();
        saved["film_render"]["convert"].as_object_mut().unwrap().remove("flat_field");
        assert_eq!(serde_json::from_value::<RuntimeParams>(saved).unwrap().film_render.convert, params.film_render.convert);
    }
}
