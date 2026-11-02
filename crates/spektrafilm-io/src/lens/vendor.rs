//! Sony, Fujifilm, OM/Olympus and Panasonic in-camera lens correction data.
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

/// Panasonic RW2/RWL: the 32-byte `DistortionInfo` (0x0119) in IFD0, sixteen
/// little-endian i16. Distortion only; the formula is from ExifTool and
/// trou/panasonic-rw2 via darktable:
/// `Ru = Rd + scale (a Rd³ + b Rd⁵ + c Rd⁷)`, radii normalised to the half
/// diagonal.
pub(super) fn panasonic(src: &RawSource) -> Option<Profile> {
    let mut reader = src.reader();
    let tiff = GenericTiffReader::new(&mut reader, 0, 0, None, &[]).ok()?;
    let bytes = match &tiff.root_ifd().get_entry(0x0119u16)?.value {
        Value::Undefined(b) | Value::Byte(b) => b.clone(),
        _ => return None,
    };
    panasonic_radial(&bytes).map(|radial| Profile { source: Source::Panasonic, model: Model::Radial(radial) })
}

pub(super) fn panasonic_radial(bytes: &[u8]) -> Option<Radial> {
    if bytes.len() != 32 {
        return None;
    }
    let v: Vec<f64> = bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f64).collect();
    // Low nibble of entry 7: 1 when the camera corrects distortion.
    if (v[7] as i16) & 0x0f != 1 {
        return None;
    }
    let scale = 1.0 / (1.0 + v[5] / 32768.0);
    let (a, b, c) = (v[8] / 32768.0, v[4] / 32768.0, v[11] / 32768.0);
    let knots: Vec<f64> = (0..MAXKNOTS).map(|i| i as f64 / (MAXKNOTS - 1) as f64).collect();
    let cor: Vec<f64> = knots
        .iter()
        .map(|&r| {
            // Invert Ru -> Rd by fixed-point iteration. darktable stops after
            // two, which leaves ~0.3% radius error in the corners of a lens
            // with this much distortion; iterate to convergence instead.
            let mut rd = r;
            for _ in 0..30 {
                let rd2 = rd * rd;
                let f = 1.0 + scale * (a * rd2 + b * rd2 * rd2 + c * rd2 * rd2 * rd2);
                rd = if f > 1e-6 { r / f } else { r };
            }
            if r > 0.0 { rd / r } else { 1.0 }
        })
        .collect();
    Some(Radial { knots_dist: knots.clone(), cor: [cor.clone(), cor.clone(), cor], knots_vig: knots, vig: vec![1.0; MAXKNOTS] })
}

/// A minimal TIFF directory reader for maker notes, whose offsets are
/// relative to the maker note rather than the file.
struct Dir<'a> {
    buf: &'a [u8],
    little: bool,
}

impl Dir<'_> {
    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.buf.get(at..at + 2)?.try_into().ok()?;
        Some(if self.little { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.buf.get(at..at + 4)?.try_into().ok()?;
        Some(if self.little { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
    }
    /// The entry `tag` of the directory at `at`: (type, count, value bytes),
    /// with out-of-line values found relative to `base`.
    fn entry(&self, at: usize, base: usize, tag: u16) -> Option<(u16, usize, &[u8])> {
        let n = self.u16(at)? as usize;
        for i in 0..n.min(1000) {
            let e = at + 2 + 12 * i;
            if self.u16(e)? != tag {
                continue;
            }
            let (kind, count) = (self.u16(e + 2)?, self.u32(e + 4)? as usize);
            let size = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => return None,
            } * count;
            let data = if size <= 4 {
                self.buf.get(e + 8..e + 8 + size)?
            } else {
                let off = base + self.u32(e + 8)? as usize;
                self.buf.get(off..off + size)?
            };
            return Some((kind, count, data));
        }
        None
    }
    fn floats(&self, entry: (u16, usize, &[u8])) -> Option<Vec<f64>> {
        let (kind, _, data) = entry;
        (kind == 11).then(|| {
            data.chunks_exact(4)
                .map(|b| {
                    let b: [u8; 4] = b.try_into().unwrap();
                    (if self.little { f32::from_le_bytes(b) } else { f32::from_be_bytes(b) }) as f64
                })
                .collect()
        })
    }
}

/// OM/Olympus ORF: distortion (0x150a, four floats) and lateral CA (0x150c,
/// six floats) in the maker note's ImageProcessing directory (0x2040).
pub(super) fn olympus(src: &RawSource) -> Option<Profile> {
    let buf = src.buf();
    let little = buf.get(0..2)? == b"II";
    let file = Dir { buf, little };
    let ifd0 = file.u32(4)? as usize;
    let exif = file.u32(ifd0 + 2 + 12 * index_of(&file, ifd0, 0x8769)? + 8)? as usize;
    let (_, _, note) = file.entry(exif, 0, 0x927c)?;
    let start = note.as_ptr() as usize - buf.as_ptr() as usize;
    // "OLYMPUS\0II\3\0" then the directory; "OM SYSTEM\0\0\0II\4\0" likewise.
    let header = if note.starts_with(b"OLYMPUS\0") {
        12
    } else if note.starts_with(b"OM SYSTEM") {
        16
    } else {
        return None;
    };
    let notes = Dir { buf, little: note.get(header - 4..header - 2)? == b"II" };
    let (_, _, processing) = notes.entry(start + header, start, 0x2040)?;
    let processing = start + notes.u32(processing.as_ptr() as usize - buf.as_ptr() as usize)? as usize;
    let dist = notes.entry(processing, start, 0x150a).and_then(|e| notes.floats(e));
    let ca = notes.entry(processing, start, 0x150c).and_then(|e| notes.floats(e));
    olympus_radial(dist.as_deref(), ca.as_deref())
        .map(|radial| Profile { source: Source::Olympus, model: Model::Radial(radial) })
}

fn index_of(dir: &Dir, at: usize, tag: u16) -> Option<usize> {
    let n = dir.u16(at)? as usize;
    (0..n.min(1000)).find(|&i| dir.u16(at + 2 + 12 * i) == Some(tag))
}

pub(super) fn olympus_radial(dist: Option<&[f64]>, ca: Option<&[f64]>) -> Option<Radial> {
    // '0 0 0 1' is no correction; the fourth value scales the corner radius.
    let dist = dist.filter(|d| d.len() == 4 && d[..3].iter().any(|&v| v != 0.0));
    let ca = ca.filter(|c| c.len() == 6 && c.iter().any(|&v| v != 0.0));
    if dist.is_none() && ca.is_none() {
        return None;
    }
    let (drs, dk2, dk4, dk6) = dist.map_or((1.0, 0.0, 0.0, 0.0), |d| (d[3], d[0], d[1], d[2]));
    let knots: Vec<f64> = (0..MAXKNOTS).map(|i| i as f64 / (MAXKNOTS - 1) as f64).collect();
    let mut cor = [vec![1.0; MAXKNOTS], vec![1.0; MAXKNOTS], vec![1.0; MAXKNOTS]];
    for (i, &r) in knots.iter().enumerate() {
        // Rin = Rout drs (1 + dk2 (Rout drs)² + dk4 (Rout drs)⁴ + dk6 (Rout drs)⁶)
        let rs2 = (r * drs).powi(2);
        let g = drs * (1.0 + rs2 * (dk2 + rs2 * (dk4 + rs2 * dk6)));
        for plane in &mut cor {
            plane[i] = g;
        }
        if let Some(c) = ca
            && r > 0.0
        {
            // Rin_with_CA = Rin ((1 + ca0) + ca2 Rin² + ca4 Rin⁴), per red and blue.
            let rd = g * r;
            let rd2 = rd * rd;
            cor[0][i] += rd * (c[0] + rd2 * (c[1] + rd2 * c[2])) / r;
            cor[2][i] += rd * (c[3] + rd2 * (c[4] + rd2 * c[5])) / r;
        }
    }
    Some(Radial { knots_dist: knots.clone(), cor, knots_vig: knots, vig: vec![1.0; MAXKNOTS] })
}
