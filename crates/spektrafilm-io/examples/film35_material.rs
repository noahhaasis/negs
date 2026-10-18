//! Extracts `data/borders/35mm/material.png`, the scanned part of the 35mm
//! border, from a raw (uninverted) scan of a blank 135 colour negative:
//!
//! "135 film. Konica VX 10. DX encoding" by Dmitry Makeev, CC BY-SA 4.0,
//! https://commons.wikimedia.org/wiki/File:135_film._Konica_VX_10._DX_encoding.jpg
//!
//! The tool finds the film's edges and perforations, maps the scan onto
//! ISO 1007 strip coordinates (millimetres along and across the film), and
//! keeps what a drawing can't invent:
//!
//! - grey: dust, hairs and specks, as density above the film base (0–255);
//! - alpha: the base's slow density variation, 128 ± 500 × density.
//!
//! The edge print, DX code, frame numbers and colour patches are thick,
//! large marks; they are removed, and the renderer draws its own.
//!
//! cargo run --release -p spektrafilm-io --example film35_material -- SCAN.jpg [DATA_DIR] [FIRST_PERF]

use std::collections::VecDeque;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use image::{GrayAlphaImage, LumaA, RgbImage};
use spektrafilm_io::film35::{FILM_WIDTH, MATERIAL_LENGTH, MATERIAL_PX_PER_MM, PERF_PITCH};

/// Density above the local base that counts as a mark.
const MARK: f32 = 0.09;
/// Smaller marks (in pixels) are grain, not dust.
const SPECK: usize = 6;
/// Largest slow variation kept, in density.
const VARIATION: f32 = 0.02;
/// Marks this thick (mm) are printing, not dust.
const PRINT_STROKE: f32 = 0.09;
/// Density that maps to full-strength dust.
const DUST_FULL: f32 = 0.6;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let scan = args.next().context("usage: film35_material SCAN.jpg [DATA_DIR] [FIRST_PERF]")?;
    let data = args.next().unwrap_or_else(|| "data".into());
    let first_perf: Option<usize> = args.next().map(|s| s.parse()).transpose()?;
    let rgb = image::open(&scan).with_context(|| format!("opening {scan}"))?.to_rgb8();
    let geometry = Registration::find(&rgb)?;
    eprintln!("{geometry:#?}");

    let start = match first_perf {
        Some(i) => i,
        None => {
            for (i, u) in geometry.perfs.iter().enumerate() {
                eprintln!("perf {i}: {u:.2} mm");
            }
            bail!("pass FIRST_PERF: the strip starts half a pitch before that perforation");
        }
    };
    let u0 = geometry.perfs.get(start).context("no such perforation")? - PERF_PITCH / 2.0;
    ensure!(
        u0 + MATERIAL_LENGTH <= geometry.length,
        "the strip runs past the scan"
    );
    let material = extract(&rgb, &geometry, u0);
    let out = Path::new(&data).join("borders/35mm/material.png");
    material.save(&out)?;
    eprintln!("wrote {}", out.display());
    Ok(())
}

/// Where the film lies in the scan.
#[derive(Debug)]
struct Registration {
    /// Top film edge at x = 0, in source pixels, and its slope.
    top: f32,
    slope: f32,
    /// Source pixels per millimetre along and across the film.
    along: f32,
    across: f32,
    /// Perforation centres along the film, in mm from the scan's left edge.
    perfs: Vec<f32>,
    /// Measured perforation size (along, across) and distance from the
    /// film edge, in mm.
    perf_size: (f32, f32),
    perf_edge: f32,
    /// Usable length of film in the scan, in mm.
    length: f32,
}

fn white(p: &image::Rgb<u8>) -> bool {
    p.0.iter().all(|&c| c > 235)
}

impl Registration {
    fn find(rgb: &RgbImage) -> Result<Self> {
        let (w, h) = rgb.dimensions();
        // The film edges: the first non-white pixel from the top and the
        // bottom of sampled columns, fitted with straight lines.
        let mut top = Vec::new();
        let mut bottom = Vec::new();
        for x in (w / 50..w - w / 50).step_by(16) {
            if let Some(y) = (0..h).find(|&y| !white(rgb.get_pixel(x, y))) {
                top.push((x as f32, y as f32));
            }
            if let Some(y) = (0..h).rev().find(|&y| !white(rgb.get_pixel(x, y))) {
                bottom.push((x as f32, y as f32));
            }
        }
        let (top, top_slope) = fit_line(&top)?;
        let (bottom, bottom_slope) = fit_line(&bottom)?;
        let slope = (top_slope + bottom_slope) / 2.0;
        let cos = 1.0 / (1.0 + slope * slope).sqrt();
        let across = (bottom - top) * cos / FILM_WIDTH;

        // Perforations: white islands inside the film.
        let inside = |x: u32, y: u32| {
            let edge = top + slope * x as f32;
            (y as f32) > edge + 0.5 * across && (y as f32) < edge + (FILM_WIDTH - 0.5) * across
        };
        let mut mask = vec![false; (w * h) as usize];
        for (x, y, p) in rgb.enumerate_pixels() {
            mask[(y * w + x) as usize] = white(p) && inside(x, y);
        }
        let expected = 1.98 * 2.79 * across * across;
        let mut found = Vec::new();
        for component in components(&mask, w as usize, h as usize) {
            let area = component.len() as f32;
            if !(0.5 * expected..1.5 * expected).contains(&area) {
                continue;
            }
            // Extents in film coordinates (still in source pixels).
            let (mut u, mut v) = (Vec::new(), Vec::new());
            for &i in &component {
                let (x, y) = ((i % w as usize) as f32, (i / w as usize) as f32);
                u.push((x + slope * (y - top)) * cos);
                v.push((y - top - slope * x) * cos);
            }
            let span = |s: &[f32]| {
                let (lo, hi) = s.iter().fold((f32::MAX, f32::MIN), |(a, b), &c| (a.min(c), b.max(c)));
                (lo, hi)
            };
            found.push((span(&u), span(&v)));
        }
        ensure!(found.len() >= 8, "found only {} perforations", found.len());
        // Pitch from the top row: a straight line through the centres.
        let mut rows: Vec<Vec<_>> = vec![Vec::new(), Vec::new()];
        for &((u0, u1), (v0, v1)) in &found {
            let row = usize::from((v0 + v1) / 2.0 > FILM_WIDTH / 2.0 * across);
            rows[row].push(((u0 + u1) / 2.0, (u1 - u0, v1 - v0), (v0, v1)));
        }
        for row in &mut rows {
            row.sort_by(|a, b| a.0.total_cmp(&b.0));
        }
        let guess = PERF_PITCH * across;
        let points: Vec<(f32, f32)> = rows[0]
            .iter()
            .map(|&(u, ..)| (((u - rows[0][0].0) / guess).round(), u))
            .collect();
        let (offset, pitch) = fit_line(&points)?;
        let along = pitch / PERF_PITCH;
        let n = points.last().unwrap().0 as usize + 1;
        let perfs = (0..n).map(|i| (offset + pitch * i as f32) / along).collect();
        let mean = |f: &dyn Fn(&(f32, (f32, f32), (f32, f32))) -> f32| {
            let all: Vec<f32> = rows.iter().flatten().map(f).collect();
            all.iter().sum::<f32>() / all.len() as f32
        };
        let perf_size = (
            mean(&|p| p.1.0 + 1.0) / along,
            mean(&|p| p.1.1 + 1.0) / across,
        );
        let perf_edge = rows[0].iter().map(|p| p.2.0).sum::<f32>() / rows[0].len() as f32 / across;
        Ok(Self {
            top,
            slope,
            along,
            across,
            perfs,
            perf_size,
            perf_edge,
            length: w as f32 * cos / along,
        })
    }

    /// Source pixel for strip coordinates in mm.
    fn source(&self, u: f32, v: f32) -> (f32, f32) {
        let cos = 1.0 / (1.0 + self.slope * self.slope).sqrt();
        let sin = self.slope * cos;
        let (a, c) = (u * self.along, v * self.across);
        // u runs along the film, v from the top edge across it.
        let x0 = a * cos - c * sin;
        let y0 = a * sin + c * cos;
        (x0, y0 + self.top)
    }
}

/// Least-squares line through `points`: (value at x = 0, slope), refitted
/// once without outliers.
fn fit_line(points: &[(f32, f32)]) -> Result<(f32, f32)> {
    let fit = |p: &[(f32, f32)]| {
        let n = p.len() as f64;
        let (sx, sy) = p.iter().fold((0.0, 0.0), |(a, b), &(x, y)| (a + x as f64, b + y as f64));
        let (mx, my) = (sx / n, sy / n);
        let (sxx, sxy) = p.iter().fold((0.0, 0.0), |(a, b), &(x, y)| {
            let dx = x as f64 - mx;
            (a + dx * dx, b + dx * (y as f64 - my))
        });
        let slope = sxy / sxx;
        ((my - slope * mx) as f32, slope as f32)
    };
    ensure!(points.len() >= 2, "too few points to fit");
    let (b, m) = fit(points);
    let kept: Vec<_> = points
        .iter()
        .copied()
        .filter(|&(x, y)| (y - (b + m * x)).abs() < 3.0)
        .collect();
    ensure!(kept.len() >= 2, "no line fits");
    Ok(fit(&kept))
}

/// 4-connected components of `mask`, as lists of indices.
fn components(mask: &[bool], w: usize, h: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..mask.len() {
        if !mask[start] || seen[start] {
            continue;
        }
        let mut component = Vec::new();
        seen[start] = true;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            component.push(i);
            let (x, y) = (i % w, i / w);
            let mut visit = |j: usize| {
                if mask[j] && !seen[j] {
                    seen[j] = true;
                    queue.push_back(j);
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
        }
        out.push(component);
    }
    out
}

/// Approximately Gaussian blur with standard deviation `sigma` pixels:
/// three box passes per axis.
fn blur(values: &mut [f32], w: usize, h: usize, sigma: f32) {
    let r = ((sigma * sigma + 1.0).sqrt() as usize).max(1);
    let mut line = Vec::new();
    for _ in 0..3 {
        for y in 0..h {
            box_line(&mut values[y * w..(y + 1) * w], 1, r, &mut line);
        }
        for x in 0..w {
            box_strided(values, x, w, h, r, &mut line);
        }
    }
}

fn box_line(row: &mut [f32], _stride: usize, r: usize, line: &mut Vec<f32>) {
    line.clear();
    line.extend_from_slice(row);
    let n = row.len();
    let mut sum: f32 = (0..=r).map(|i| line[i.min(n - 1)]).sum::<f32>() + r as f32 * line[0];
    for (i, out) in row.iter_mut().enumerate() {
        *out = sum / (2 * r + 1) as f32;
        let add = line[(i + r + 1).min(n - 1)];
        let sub = line[i.saturating_sub(r)];
        sum += add - sub;
    }
}

fn box_strided(values: &mut [f32], x: usize, w: usize, h: usize, r: usize, line: &mut Vec<f32>) {
    let mut column: Vec<f32> = (0..h).map(|y| values[y * w + x]).collect();
    box_line(&mut column, 1, r, line);
    for (y, v) in column.into_iter().enumerate() {
        values[y * w + x] = v;
    }
}

/// Blur of `values` that ignores pixels where `weight` is 0.
fn masked_blur(values: &[f32], weight: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let mut num: Vec<f32> = values.iter().zip(weight).map(|(v, k)| v * k).collect();
    let mut den = weight.to_vec();
    blur(&mut num, w, h, sigma);
    blur(&mut den, w, h, sigma);
    num.iter().zip(&den).map(|(n, d)| n / d.max(1e-4)).collect()
}

/// Square erosion of `mask` by `r` pixels.
fn erode(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let mut rows = vec![false; mask.len()];
    for y in 0..h {
        for x in r..w.saturating_sub(r) {
            rows[y * w + x] = (x - r..=x + r).all(|i| mask[y * w + i]);
        }
    }
    let mut out = vec![false; mask.len()];
    for y in r..h.saturating_sub(r) {
        for x in 0..w {
            out[y * w + x] = (y - r..=y + r).all(|j| rows[j * w + x]);
        }
    }
    out
}

fn dilate(mask: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    let inverse: Vec<bool> = mask.iter().map(|m| !m).collect();
    erode(&inverse, w, h, r).iter().map(|m| !m).collect()
}

fn extract(rgb: &RgbImage, geometry: &Registration, u0: f32) -> GrayAlphaImage {
    let m = MATERIAL_PX_PER_MM;
    let (w, h) = (
        (MATERIAL_LENGTH * m).round() as usize,
        (FILM_WIDTH * m).round() as usize,
    );
    // The scan on the strip grid, bilinearly resampled.
    let (sw, sh) = (rgb.width() as i64, rgb.height() as i64);
    let sample = |x: f32, y: f32| -> [f32; 3] {
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: i64, y: i64, c: usize| {
            rgb.get_pixel(x.clamp(0, sw - 1) as u32, y.clamp(0, sh - 1) as u32).0[c] as f32
        };
        std::array::from_fn(|c| {
            let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1, y0, c) * fx;
            let bottom = at(x0, y0 + 1, c) * (1.0 - fx) + at(x0 + 1, y0 + 1, c) * fx;
            top * (1.0 - fy) + bottom * fy
        })
    };
    let mut scan = vec![[0.0f32; 3]; w * h];
    for j in 0..h {
        for i in 0..w {
            let (u, v) = (u0 + (i as f32 + 0.5) / m, (j as f32 + 0.5) / m);
            let (x, y) = geometry.source(u, v);
            scan[j * w + i] = sample(x, y);
        }
    }

    // Perforations, padded, and the film's cut edges carry no material.
    let (pw, ph) = geometry.perf_size;
    let pad = 0.25;
    let perf_mask: Vec<bool> = (0..w * h)
        .map(|k| {
            let (u, v) = ((k % w) as f32 / m, (k / w) as f32 / m);
            let du = u.rem_euclid(PERF_PITCH) - PERF_PITCH / 2.0;
            let in_row = |top: f32| (v - top).abs() <= ph / 2.0 + pad;
            let row = in_row(geometry.perf_edge + ph / 2.0)
                || in_row(FILM_WIDTH - geometry.perf_edge - ph / 2.0);
            (row && du.abs() <= pw / 2.0 + pad) || v < 0.1 || v > FILM_WIDTH - 0.1 || scan[k].iter().all(|&c| c > 235.0)
        })
        .collect();

    // Density over the film base, in the densest channel, so that the
    // coloured patches count as marks too.
    let base: [f32; 3] = std::array::from_fn(|c| {
        let mut values: Vec<f32> = scan
            .iter()
            .zip(&perf_mask)
            .filter(|(_, p)| !**p)
            .map(|(s, _)| s[c])
            .collect();
        values.sort_by(f32::total_cmp);
        values[values.len() / 2]
    });
    let density: Vec<f32> = scan
        .iter()
        .map(|s| (0..3).map(|c| (base[c] / s[c].max(1.0)).ln()).fold(f32::MIN, f32::max))
        .collect();

    // Marks: dense spots over a local base that ignores them.
    let weight: Vec<f32> = perf_mask.iter().map(|&p| if p { 0.0 } else { 1.0 }).collect();
    let local = masked_blur(&density, &weight, w, h, 0.3 * m);
    let marks: Vec<bool> = (0..w * h)
        .map(|k| !perf_mask[k] && density[k] - local[k] > MARK)
        .collect();
    // Printing is thick: anything that survives erosion by half a stroke.
    let thick = erode(&marks, w, h, (PRINT_STROKE / 2.0 * m) as usize);
    let mut print = vec![false; w * h];
    let mut dust = vec![false; w * h];
    for component in components(&marks, w, h) {
        let printed = component.iter().any(|&k| thick[k]);
        if !printed && component.len() < SPECK {
            continue;
        }
        for k in component {
            if printed {
                print[k] = true;
            } else {
                dust[k] = true;
            }
        }
    }
    // Dot-matrix printing leaves loose dots around each mark.
    let print = dilate(&print, w, h, (0.25 * m) as usize);
    let dust = dilate(&dust, w, h, 1);

    // The base without any marks, and the marks over it.
    let clean: Vec<f32> = (0..w * h)
        .map(|k| if perf_mask[k] || print[k] || dust[k] { 0.0 } else { 1.0 })
        .collect();
    let fine = masked_blur(&density, &clean, w, h, 0.05 * m);
    let slow = masked_blur(&density, &clean, w, h, 0.4 * m);
    // How much clean film the slow field saw: where it was mostly printing
    // it is a guess, so it fades to the median.
    let mut support = clean.clone();
    blur(&mut support, w, h, 0.4 * m);
    let mut levels: Vec<f32> = slow
        .iter()
        .zip(&clean)
        .filter(|(_, c)| **c > 0.0)
        .map(|(s, _)| *s)
        .collect();
    levels.sort_by(f32::total_cmp);
    let level = levels[levels.len() / 2];

    let mut out = GrayAlphaImage::new(w as u32, h as u32);
    for (k, pixel) in out.pixels_mut().enumerate() {
        let speck = if dust[k] && !print[k] && !perf_mask[k] {
            ((density[k] - fine[k]) / DUST_FULL).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let trust = ((support[k] - 0.4) / 0.4).clamp(0.0, 1.0);
        let variation = 128.0 + 500.0 * trust * (slow[k] - level).clamp(-VARIATION, VARIATION);
        *pixel = LumaA([
            (speck * 255.0).round() as u8,
            variation.round().clamp(0.0, 255.0) as u8,
        ]);
    }
    out
}
