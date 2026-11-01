//! Sony and Fujifilm in-camera lens correction data.
//!
//! Ported from darktable's embedded-metadata lens correction:
//! `_check_lens_correction_data` (src/common/exif.cc) and
//! `_init_coeffs_md_v2` (src/iop/lens.cc), based on the algorithm by Freddie
//! Witherden (darktable pull request 7092).
//! Copyright (C) 2019-2026 darktable developers. darktable is free software
//! under the GNU General Public License, version 3 or later; Negs uses this
//! port under version 3.
//!
//! darktable's final autoscale is replaced by [`super::fill_zoom`], which
//! applies to every source.

use std::io::{Seek, SeekFrom};

use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{GenericTiffReader, IFD, Value};
use rawler::rawsource::RawSource;

use super::{Model, Profile, Radial, Source, spline};

/// Knots of the radius darktable resamples Fujifilm's curves onto.
const MAXKNOTS: usize = 16;

fn values(ifd: &IFD, tag: u16) -> Option<Vec<f64>> {
    let v = match &ifd.get_entry(tag)?.value {
        Value::SShort(v) => v.iter().map(|&x| x as f64).collect(),
        Value::Short(v) => v.iter().map(|&x| x as f64).collect(),
        Value::SLong(v) => v.iter().map(|&x| x as f64).collect(),
        Value::Long(v) => v.iter().map(|&x| x as f64).collect(),
        Value::SRational(v) => v.iter().map(|r| if r.d == 0 { 0.0 } else { r.n as f64 / r.d as f64 }).collect(),
        Value::Rational(v) => v.iter().map(|r| if r.d == 0 { 0.0 } else { r.n as f64 / r.d as f64 }).collect(),
        Value::Float(v) => v.iter().map(|&x| x as f64).collect(),
        _ => return None,
    };
    Some(v)
}

/// Sony ARW: `VignettingCorrParams` (0x7032), `ChromaticAberrationCorrParams`
/// (0x7035) and `DistortionCorrParams` (0x7037) in the RAW's SubIFD. Each
/// starts with its knot count.
pub(super) fn sony(src: &RawSource) -> Option<Profile> {
    let mut reader = src.reader();
    let tiff = GenericTiffReader::new(&mut reader, 0, 0, None, &[]).ok()?;
    let ifd = tiff.find_first_ifd_with_tag(0x7037u16)?;
    let dist = values(ifd, 0x7037)?;
    let ca = values(ifd, 0x7035)?;
    let vig = values(ifd, 0x7032)?;
    sony_radial(&dist, &ca, &vig).map(|radial| Profile { source: Source::Sony, model: Model::Radial(radial) })
}

pub(super) fn sony_radial(dist: &[f64], ca: &[f64], vig: &[f64]) -> Option<Radial> {
    let nc = *dist.first()? as usize;
    if !(2..=16).contains(&nc)
        || ca.first() != Some(&(2.0 * nc as f64))
        || vig.first() != Some(&(nc as f64))
        || dist.len() <= nc
        || ca.len() <= 2 * nc
        || vig.len() <= nc
    {
        return None;
    }
    let knots: Vec<f64> = (0..nc).map(|i| (i as f64 + 0.5) / (nc - 1) as f64).collect();
    let mut cor = [vec![0.0; nc], vec![0.0; nc], vec![0.0; nc]];
    let mut v = vec![0.0; nc];
    for i in 0..nc {
        let d = dist[i + 1] * 2f64.powi(-14) + 1.0;
        cor[0][i] = d * (ca[i + 1] * 2f64.powi(-21) + 1.0);
        cor[1][i] = d;
        cor[2][i] = d * (ca[nc + i + 1] * 2f64.powi(-21) + 1.0);
        v[i] = 2f64.powf(0.5 - 2f64.powf(vig[i + 1] * 2f64.powi(-13) - 1.0));
    }
    Some(Radial { knots_dist: knots.clone(), cor, knots_vig: knots, vig: v })
}

/// Fujifilm RAF: `GeometricDistortionParams` (0xf00b),
/// `ChromaticAberrationParams` (0xf00f) and `VignettingParams` (0xf010) in the
/// FujiIFD of the RAF's second TIFF block. The 1.25× crop modes are not read
/// (darktable scales the knots by 1.25 for them).
pub(super) fn fujifilm(src: &RawSource) -> Option<Profile> {
    let buf = src.buf();
    let offset = u32::from_be_bytes(buf.get(100..104)?.try_into().ok()?);
    let mut reader = src.reader();
    reader.seek(SeekFrom::Start(offset as u64)).ok()?;
    let tiff = GenericTiffReader::new(&mut reader, offset, 0, None, &[0xf000]).ok()?;
    let ifd = tiff.find_first_ifd_with_tag(0xf00bu16)?;
    let dist = values(ifd, 0xf00b)?;
    let ca = values(ifd, 0xf00f)?;
    let vig = values(ifd, 0xf010)?;
    fuji_radial(&dist, &ca, &vig).map(|radial| Profile { source: Source::Fujifilm, model: Model::Radial(radial) })
}

/// Per-knot data in darktable's order: knots, distortion (percent), red and
/// blue CA, vignetting (percent of the centre's brightness).
fn fuji_knots(dist: &[f64], ca: &[f64], vig: &[f64]) -> Option<Vec<[f64; 5]>> {
    let mut out = Vec::new();
    match (dist.len(), ca.len(), vig.len()) {
        // X-Trans IV/V
        (19, 29, 19) => {
            for i in 0..9 {
                let k = dist[i + 1];
                if k != ca[i + 1] || k != vig[i + 1] {
                    return None;
                }
                out.push([k, dist[i + 10], ca[i + 10], ca[i + 19], vig[i + 10]]);
            }
        }
        // X-Trans I/II/III: the CA data has no first knot (0).
        (23, 31, 23) => {
            for i in 0..11 {
                let k = dist[i + 1];
                let kc = if i == 0 { 0.0 } else { ca[i] };
                if k != kc || k != vig[i + 1] {
                    return None;
                }
                let (r, b) = if i == 0 { (0.0, 0.0) } else { (ca[i + 10], ca[i + 20]) };
                out.push([k, dist[i + 12], r, b, vig[i + 12]]);
            }
        }
        _ => return None,
    }
    Some(out)
}

pub(super) fn fuji_radial(dist: &[f64], ca: &[f64], vig: &[f64]) -> Option<Radial> {
    let data = fuji_knots(dist, ca, vig)?;
    let (mut knots_in, mut cor_in, mut ca_r_in, mut ca_b_in) = (vec![], vec![], vec![], vec![]);
    let (mut knots_vig, mut v) = (vec![], vec![]);
    // A knot without corrections at the centre, if the data has none there.
    if data[0][0] > 0.0 {
        knots_in.push(0.0);
        cor_in.push(1.0);
        ca_r_in.push(0.0);
        ca_b_in.push(0.0);
        knots_vig.push(0.0);
        v.push(1.0);
    }
    for &[k, d, r, b, vg] in &data {
        knots_in.push(k);
        cor_in.push(d / 100.0 + 1.0);
        ca_r_in.push(r);
        ca_b_in.push(b);
        // Vignetting acts before distortion, on the source radius.
        knots_vig.push(k);
        v.push(vg / 100.0);
    }
    // The distortion curve is a function of the source radius; resample it as
    // a function of the corrected radius.
    let mut knots_dist = vec![0.0; MAXKNOTS];
    let mut cor = [vec![0.0; MAXKNOTS], vec![0.0; MAXKNOTS], vec![0.0; MAXKNOTS]];
    for i in 0..MAXKNOTS {
        let rin = i as f64 / (MAXKNOTS - 1) as f64;
        let m = spline(&knots_in, &cor_in, rin);
        knots_dist[i] = rin / m;
        cor[0][i] = m * (spline(&knots_in, &ca_r_in, rin) + 1.0);
        cor[1][i] = m;
        cor[2][i] = m * (spline(&knots_in, &ca_b_in, rin) + 1.0);
    }
    Some(Radial { knots_dist, cor, knots_vig, vig: v })
}
