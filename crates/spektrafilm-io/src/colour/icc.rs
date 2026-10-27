//! Reading the ICC profiles embedded in input images (ICC.1 v2 and v4).
//!
//! Every profile is evaluated here exactly, device values → PCS, relative
//! colorimetric:
//!
//! - matrix/TRC RGB and grey TRC profiles: the tone curves as the profile
//!   defines them, then the colorants;
//! - lookup-table profiles (`lut8`, `lut16` or `lutAtoB` in `A2B1`, else
//!   `A2B0`, else `A2B2`, as ICC selects tags for that intent): every stage
//!   of the table as stored, with tetrahedral interpolation of the CLUT, as
//!   LittleCMS does in floating point. ICC gives the table precedence over
//!   any colorant tags, which are never used as an approximation of it.
//!
//! The table is not resampled onto a coarser grid first, which is why this
//! does not use `moxcms` (it resamples to 33³ and was measured up to 11%
//! off LittleCMS in deep shadows).

use spektrafilm_math::colorspace::{COLOURSPACES, PCS_D50, RgbColourspace, bradford_matrix, mat3_mul3_f64};

/// An ICC tone curve, encoded value → linear value.
#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Identity,
    Gamma(f64),
    /// `curv` samples on [0, 1], linearly interpolated.
    Table(Vec<f64>),
    /// `para` function type 0–4 and its parameters g, a, b, c, d, e, f.
    Parametric(u16, [f64; 7]),
}

impl Curve {
    /// Encoded → linear. Analytic curves extend beyond [0, 1] (mirrored below
    /// zero, as extended-range colour spaces do); a table is defined only on
    /// [0, 1], so its input is clamped there.
    pub fn decode(&self, x: f64) -> f64 {
        if x < 0.0 && !matches!(self, Curve::Table(_)) {
            return -self.decode(-x);
        }
        match self {
            Curve::Identity => x,
            Curve::Gamma(g) => x.powf(*g),
            Curve::Table(t) => {
                let p = x.clamp(0.0, 1.0) * (t.len() - 1) as f64;
                let i = (p.floor() as usize).min(t.len() - 2);
                let f = p - i as f64;
                t[i] + (t[i + 1] - t[i]) * f
            }
            Curve::Parametric(kind, [g, a, b, c, d, e, f]) => {
                let power = |x: f64| {
                    let base = a * x + b;
                    if base > 0.0 { base.powf(*g) } else { 0.0 }
                };
                match kind {
                    0 => x.powf(*g),
                    1 => if x >= -b / a { power(x) } else { 0.0 },
                    2 => if x >= -b / a { power(x) + c } else { *c },
                    3 => if x >= *d { power(x) } else { c * x },
                    _ => if x >= *d { power(x) + e } else { c * x + f },
                }
            }
        }
    }

    pub fn is_identity(&self) -> bool {
        match self {
            Curve::Identity => true,
            Curve::Gamma(g) => *g == 1.0,
            Curve::Parametric(0, p) => p[0] == 1.0,
            _ => false,
        }
    }
}

/// What a profile maps device values to.
#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    /// Columns: the red, green and blue colorants in PCS XYZ (D50).
    RgbMatrix { colorants: [[f64; 3]; 3], curves: [Curve; 3] },
    GrayTrc(Curve),
    /// A lookup-table pipeline, for RGB (3 inputs) or grey (1 input) data.
    Lut(Lut),
}

/// How a table's output encodes the PCS.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Pcs {
    /// u1Fixed15 XYZ: 1.0 is 32768/65535 of the range.
    Xyz,
    /// `lut16` Lab: the legacy 16-bit encoding (L 100 at 0xFF00).
    LabLegacy,
    /// `lut8` and `lutAtoB` Lab: L 0–100, a and b −128–127 over the range.
    Lab,
}

#[derive(Clone, Debug, PartialEq)]
enum Stage {
    Curves(Vec<Curve>),
    /// Grid points per input (the first input varies slowest), samples on [0, 1].
    Clut { grid: Vec<usize>, outputs: usize, table: Vec<f64> },
    Matrix { m: [[f64; 3]; 3], offset: [f64; 3] },
}

/// A device → PCS lookup-table pipeline.
#[derive(Clone, Debug, PartialEq)]
pub struct Lut {
    pub inputs: usize,
    stages: Vec<Stage>,
    pcs: Pcs,
}

impl Lut {
    /// Device values (in [0, 1]; others are clamped, as the table is only
    /// defined there) → PCS XYZ (D50, white Y = 1), relative colorimetric.
    pub fn eval(&self, input: &[f64]) -> [f64; 3] {
        let mut v: Vec<f64> = input.iter().map(|x| clamp01(*x)).collect();
        for stage in &self.stages {
            v = match stage {
                Stage::Curves(curves) => curves.iter().zip(&v).map(|(c, x)| c.decode(clamp01(*x))).collect(),
                Stage::Clut { grid, outputs, table } => interpolate(grid, *outputs, table, &v),
                Stage::Matrix { m, offset } => {
                    let x: [f64; 3] = std::array::from_fn(|i| clamp01(v[i]));
                    (0..3).map(|i| m[i][0] * x[0] + m[i][1] * x[1] + m[i][2] * x[2] + offset[i]).collect()
                }
            };
        }
        let v: [f64; 3] = std::array::from_fn(|i| clamp01(v[i]));
        match self.pcs {
            Pcs::Xyz => v.map(|x| x * 65535.0 / 32768.0),
            Pcs::LabLegacy => lab_to_xyz(v[0] * 65535.0 / 65280.0 * 100.0, v[1] * 65535.0 / 256.0 - 128.0, v[2] * 65535.0 / 256.0 - 128.0),
            Pcs::Lab => lab_to_xyz(v[0] * 100.0, v[1] * 255.0 - 128.0, v[2] * 255.0 - 128.0),
        }
    }
}

/// NaN becomes 0, so a broken sample can't poison the interpolation.
fn clamp01(x: f64) -> f64 {
    if x.is_nan() { 0.0 } else { x.clamp(0.0, 1.0) }
}

fn lab_to_xyz(l: f64, a: f64, b: f64) -> [f64; 3] {
    let fy = (l + 16.0) / 116.0;
    let f = |t: f64| if t > 6.0 / 29.0 { t * t * t } else { 3.0 * (6.0f64 / 29.0).powi(2) * (t - 4.0 / 29.0) };
    [PCS_D50[0] * f(fy + a / 500.0), PCS_D50[1] * f(fy), PCS_D50[2] * f(fy - b / 200.0)]
}

/// Simplex (for three inputs, tetrahedral) interpolation: from the cell's
/// lowest corner, step along the inputs in order of decreasing fraction.
fn interpolate(grid: &[usize], outputs: usize, table: &[f64], v: &[f64]) -> Vec<f64> {
    let n = grid.len();
    let mut base = vec![0usize; n];
    let mut frac = vec![0f64; n];
    for i in 0..n {
        let p = clamp01(v[i]) * (grid[i] - 1) as f64;
        base[i] = (p.floor() as usize).min(grid[i] - 2);
        frac[i] = p - base[i] as f64;
    }
    // Table offset of a grid point, the last input varying fastest.
    let offset = |point: &[usize]| point.iter().zip(grid).fold(0, |acc, (p, g)| acc * g + p) * outputs;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| frac[b].total_cmp(&frac[a]));
    let mut point = base.clone();
    let mut previous = offset(&point);
    let mut out: Vec<f64> = table[previous..previous + outputs].to_vec();
    for &dim in &order {
        point[dim] += 1;
        let next = offset(&point);
        for (o, (hi, lo)) in out.iter_mut().zip(table[next..next + outputs].iter().zip(&table[previous..previous + outputs])) {
            *o += frac[dim] * (hi - lo);
        }
        previous = next;
    }
    out
}

/// A parsed profile header and tag table. Every read is bounds-checked;
/// a malformed profile is an error, never a panic.
pub struct Profile<'a> {
    data: &'a [u8],
    pub version_major: u8,
    pub class: [u8; 4],
    pub colour_space: [u8; 4],
    pub pcs: [u8; 4],
    tags: Vec<([u8; 4], usize, usize)>,
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}
fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}
fn s15(d: &[u8], at: usize) -> Option<f64> {
    Some(be32(d, at)? as i32 as f64 / 65536.0)
}
fn sig(d: &[u8], at: usize) -> Option<[u8; 4]> {
    d.get(at..at + 4)?.try_into().ok()
}

impl<'a> Profile<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self, String> {
        let malformed = || "malformed ICC profile".to_string();
        if data.len() < 132 || data.get(36..40) != Some(b"acsp") {
            return Err(malformed());
        }
        let count = be32(data, 128).ok_or_else(malformed)? as usize;
        if count > 1024 {
            return Err(malformed());
        }
        let mut tags = Vec::with_capacity(count);
        for i in 0..count {
            let at = 132 + 12 * i;
            let (signature, offset, size) = (|| Some((sig(data, at)?, be32(data, at + 4)? as usize, be32(data, at + 8)? as usize)))()
                .ok_or_else(malformed)?;
            if offset.checked_add(size).is_none_or(|end| end > data.len()) {
                return Err(malformed());
            }
            tags.push((signature, offset, size));
        }
        Ok(Self {
            data,
            version_major: data[8],
            class: sig(data, 12).ok_or_else(malformed)?,
            colour_space: sig(data, 16).ok_or_else(malformed)?,
            pcs: sig(data, 20).ok_or_else(malformed)?,
            tags,
        })
    }

    fn tag(&self, name: &[u8; 4]) -> Option<&'a [u8]> {
        let &(_, offset, size) = self.tags.iter().find(|(s, ..)| s == name)?;
        self.data.get(offset..offset + size)
    }

    /// The profile's description (`desc`), if it has a readable one.
    pub fn description(&self) -> Option<String> {
        let t = self.tag(b"desc")?;
        let text = match t.get(0..4)? {
            b"desc" => {
                let n = be32(t, 8)? as usize;
                let bytes = t.get(12..12 + n)?;
                String::from_utf8_lossy(bytes.split(|&b| b == 0).next()?).into_owned()
            }
            b"mluc" => {
                // The first record; v4 profiles normally carry en-US first.
                let (len, offset) = (be32(t, 20)? as usize, be32(t, 24)? as usize);
                let units: Vec<u16> = t.get(offset..offset + len)?.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units)
            }
            _ => return None,
        };
        let text = text.trim().to_string();
        (!text.is_empty()).then_some(text)
    }

    fn xyz(&self, name: &[u8; 4]) -> Option<[f64; 3]> {
        let t = self.tag(name)?;
        (t.get(0..4)? == b"XYZ ").then_some(())?;
        Some([s15(t, 8)?, s15(t, 12)?, s15(t, 16)?])
    }

    fn curve(&self, name: &[u8; 4]) -> Option<Curve> {
        parse_curve(self.tag(name)?).map(|(curve, _)| curve)
    }

    /// The `A2B` table ICC selects for relative colorimetric rendering.
    fn lut(&self) -> Option<Result<Lut, String>> {
        let (name, t) = [b"A2B1", b"A2B0", b"A2B2"].into_iter().find_map(|n| Some((n, self.tag(n)?)))?;
        let inputs = match &self.colour_space {
            b"RGB " => 3,
            b"GRAY" => 1,
            other => return Some(Err(format!("it describes {} data", String::from_utf8_lossy(other).trim()))),
        };
        let lab = match &self.pcs {
            b"XYZ " => false,
            b"Lab " => true,
            _ => return Some(Err("its connection space is neither XYZ nor Lab".into())),
        };
        Some(parse_lut(t, inputs, lab).ok_or_else(|| format!("its {} lookup table can't be read", String::from_utf8_lossy(name))))
    }

    /// How this profile maps device values to the connection space.
    pub fn kind(&self) -> Result<Kind, String> {
        if let Some(lut) = self.lut() {
            return lut.map(Kind::Lut);
        }
        if &self.pcs == b"Lab " {
            return Err("it has a Lab connection space but no lookup table".into());
        }
        match &self.colour_space {
            b"RGB " => {
                let unreadable = || "its colorants or tone curves can't be read".to_string();
                let (r, g, b) = (self.xyz(b"rXYZ"), self.xyz(b"gXYZ"), self.xyz(b"bXYZ"));
                let (r, g, b) = (r.ok_or_else(unreadable)?, g.ok_or_else(unreadable)?, b.ok_or_else(unreadable)?);
                let curves = [self.curve(b"rTRC"), self.curve(b"gTRC"), self.curve(b"bTRC")];
                let [Some(cr), Some(cg), Some(cb)] = curves else { return Err(unreadable()) };
                let colorants = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
                if spektrafilm_math::colorspace::mat3_inverse(&colorants).iter().flatten().any(|v| !v.is_finite()) {
                    return Err("its colorants are degenerate".into());
                }
                Ok(Kind::RgbMatrix { colorants, curves: [cr, cg, cb] })
            }
            b"GRAY" => self.curve(b"kTRC").map(Kind::GrayTrc).ok_or_else(|| "its grey tone curve can't be read".into()),
            other => Err(format!("it describes {} data", String::from_utf8_lossy(other).trim())),
        }
    }
}

/// A `curv` or `para` element and its length in bytes (unpadded).
fn parse_curve(t: &[u8]) -> Option<(Curve, usize)> {
    match t.get(0..4)? {
        b"curv" => {
            let n = be32(t, 8)? as usize;
            let curve = match n {
                0 => Curve::Identity,
                1 => Curve::Gamma(be16(t, 12)? as f64 / 256.0),
                n => Curve::Table((0..n).map(|i| be16(t, 12 + 2 * i).map(|v| v as f64 / 65535.0)).collect::<Option<_>>()?),
            };
            Some((curve, 12 + 2 * n))
        }
        b"para" => {
            let kind = be16(t, 8)?;
            let n = *[1usize, 3, 4, 5, 7].get(kind as usize)?;
            let mut p = [0.0; 7];
            for (i, v) in p.iter_mut().take(n).enumerate() {
                *v = s15(t, 12 + 4 * i)?;
            }
            // Types 1 and 2 divide by a; a degenerate curve is unusable.
            ((kind != 1 && kind != 2) || p[1] != 0.0).then_some((Curve::Parametric(kind, p), 12 + 4 * n))
        }
        _ => None,
    }
}

/// `count` curves stored one after another, each padded to 4 bytes.
fn parse_curves(t: &[u8], mut at: usize, count: usize) -> Option<Vec<Curve>> {
    (0..count)
        .map(|_| {
            let (curve, len) = parse_curve(t.get(at..)?)?;
            at += len.next_multiple_of(4);
            Some(curve)
        })
        .collect()
}

/// A CLUT's samples, normalised to [0, 1], refusing absurd sizes.
fn clut(t: &[u8], at: usize, grid: Vec<usize>, outputs: usize, bytes: usize) -> Option<Stage> {
    if grid.iter().any(|&g| g < 2) {
        return None;
    }
    let count = grid.iter().try_fold(outputs, |acc, &g| acc.checked_mul(g))?;
    (count <= 1 << 24).then_some(())?;
    let table = (0..count)
        .map(|i| match bytes {
            1 => t.get(at + i).map(|&v| v as f64 / 255.0),
            _ => be16(t, at + 2 * i).map(|v| v as f64 / 65535.0),
        })
        .collect::<Option<_>>()?;
    Some(Stage::Clut { grid, outputs, table })
}

/// A `lut8`, `lut16` or `lutAtoB` table with `inputs` device channels and a
/// 3-channel PCS output.
fn parse_lut(t: &[u8], inputs: usize, lab: bool) -> Option<Lut> {
    let (i, o) = (*t.get(8)? as usize, *t.get(9)? as usize);
    (i == inputs && o == 3).then_some(())?;
    let mut stages = Vec::new();
    let pcs;
    match t.get(0..4)? {
        kind @ (b"mft1" | b"mft2") => {
            // The 3x3 matrix applies only to XYZ input, never to device data.
            let wide = kind == b"mft2";
            let grid = *t.get(10)? as usize;
            let (n, m, mut at, bytes) = if wide { (be16(t, 48)? as usize, be16(t, 50)? as usize, 52, 2) } else { (256, 256, 48, 1) };
            (n >= 2 && m >= 2).then_some(())?;
            let table = |at: usize, len: usize| -> Option<Curve> {
                let v = (0..len)
                    .map(|k| if wide { be16(t, at + 2 * k).map(|v| v as f64 / 65535.0) } else { t.get(at + k).map(|&v| v as f64 / 255.0) })
                    .collect::<Option<_>>()?;
                Some(Curve::Table(v))
            };
            stages.push(Stage::Curves((0..i).map(|k| table(at + k * n * bytes, n)).collect::<Option<_>>()?));
            at += i * n * bytes;
            let clut = clut(t, at, vec![grid; i], o, bytes)?;
            let Stage::Clut { table: samples, .. } = &clut else { unreachable!() };
            at += samples.len() * bytes;
            stages.push(clut);
            stages.push(Stage::Curves((0..o).map(|k| table(at + k * m * bytes, m)).collect::<Option<_>>()?));
            pcs = match (lab, wide) {
                (false, _) => Pcs::Xyz,
                (true, true) => Pcs::LabLegacy,
                (true, false) => Pcs::Lab,
            };
        }
        b"mAB " => {
            let offset = |k: usize| be32(t, 12 + 4 * k).map(|v| v as usize);
            let (b, matrix, m, clut_at, a) = (offset(0)?, offset(1)?, offset(2)?, offset(3)?, offset(4)?);
            // Order: A curves, CLUT, M curves, matrix, B curves (B required).
            (b != 0).then_some(())?;
            if a != 0 {
                stages.push(Stage::Curves(parse_curves(t, a, i)?));
            }
            if clut_at != 0 {
                let grid = (0..i).map(|k| t.get(clut_at + k).map(|&g| g as usize)).collect::<Option<_>>()?;
                let bytes = *t.get(clut_at + 16)? as usize;
                (bytes == 1 || bytes == 2).then_some(())?;
                stages.push(clut(t, clut_at + 20, grid, o, bytes)?);
            } else if i != o {
                return None;
            }
            if m != 0 {
                stages.push(Stage::Curves(parse_curves(t, m, o)?));
            }
            if matrix != 0 {
                let e = (0..12).map(|k| s15(t, matrix + 4 * k)).collect::<Option<Vec<_>>>()?;
                stages.push(Stage::Matrix {
                    m: [[e[0], e[1], e[2]], [e[3], e[4], e[5]], [e[6], e[7], e[8]]],
                    offset: [e[9], e[10], e[11]],
                });
            }
            stages.push(Stage::Curves(parse_curves(t, b, o)?));
            pcs = if lab { Pcs::Lab } else { Pcs::Xyz };
        }
        _ => return None,
    }
    Some(Lut { inputs, stages, pcs })
}

/// The standard colour space whose ICC colorants and tone curves `kind`
/// matches, for naming it: `(space, linear)`. Only a label; the profile's own
/// numbers are what convert the pixels.
pub fn recognise(kind: &Kind) -> Option<(&'static RgbColourspace, bool)> {
    let Kind::RgbMatrix { colorants, curves } = kind else { return None };
    let candidates = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"];
    COLOURSPACES.iter().filter(|cs| candidates.contains(&cs.name)).find_map(|cs| {
        let expected = mat3_mul3_f64(&bradford_matrix(cs.whitepoint_xyz, PCS_D50), &cs.rgb_to_xyz);
        let close = (0..3).all(|i| (0..3).all(|j| (expected[i][j] - colorants[i][j]).abs() < 1e-3));
        if !close {
            return None;
        }
        let matches = |decode: &dyn Fn(f64) -> f64| {
            curves.iter().all(|c| (0..=256).all(|i| {
                let x = i as f64 / 256.0;
                (c.decode(x) - decode(x)).abs() < 1e-3
            }))
        };
        if matches(&|x| x) {
            Some((cs, true))
        } else {
            matches(&|x| cs.cctf.decode(x)).then_some((cs, false))
        }
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A minimal v2 profile with these tags, for tests.
    pub(crate) fn build(colour_space: &[u8; 4], pcs: &[u8; 4], tags: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut data = vec![0u8; 128];
        data[8] = 2;
        data[12..16].copy_from_slice(b"mntr");
        data[16..20].copy_from_slice(colour_space);
        data[20..24].copy_from_slice(pcs);
        data[36..40].copy_from_slice(b"acsp");
        data.extend((tags.len() as u32).to_be_bytes());
        let mut offset = 128 + 4 + 12 * tags.len();
        let mut bodies: Vec<u8> = Vec::new();
        for (signature, body) in tags {
            data.extend(*signature);
            data.extend((offset as u32).to_be_bytes());
            data.extend((body.len() as u32).to_be_bytes());
            offset += body.len();
            bodies.extend(body);
        }
        data.extend(bodies);
        let size = data.len() as u32;
        data[0..4].copy_from_slice(&size.to_be_bytes());
        data
    }

    pub(crate) fn para(kind: u16, p: &[f64]) -> Vec<u8> {
        let mut t = b"para\0\0\0\0".to_vec();
        t.extend(kind.to_be_bytes());
        t.extend([0, 0]);
        for v in p {
            t.extend(((v * 65536.0).round() as i32).to_be_bytes());
        }
        t
    }

    #[test]
    fn parametric_and_table_curves_decode_like_their_definitions() {
        let srgb = Curve::Parametric(3, [2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045, 0.0, 0.0]);
        for x in [0.0, 0.02, 0.04045, 0.2, 0.5, 1.0] {
            let want = if x < 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055f64).powf(2.4) };
            assert!((srgb.decode(x) - want).abs() < 1e-12, "{x}");
        }
        // Mirrored below zero and extended above one.
        assert!((srgb.decode(-0.5) + srgb.decode(0.5)).abs() < 1e-15);
        assert!(srgb.decode(1.5) > 1.0);
        let table = Curve::Table(vec![0.0, 0.25, 1.0]);
        assert_eq!(table.decode(0.75), 0.625);
        assert_eq!(table.decode(2.0), 1.0, "tables clamp outside their domain");
        assert_eq!(Curve::Gamma(2.0).decode(0.5), 0.25);
        let type4 = Curve::Parametric(4, [2.0, 1.0, 0.0, 0.5, 0.1, 0.01, 0.02]);
        assert_eq!(type4.decode(0.05), 0.5 * 0.05 + 0.02);
        assert!((type4.decode(0.5) - (0.25 + 0.01)).abs() < 1e-15);
    }

    #[test]
    fn lut_profiles_are_never_reduced_to_their_colorants() {
        let xyz = |v: [f64; 3]| {
            let mut t = b"XYZ \0\0\0\0".to_vec();
            v.iter().for_each(|c| t.extend(((c * 65536.0).round() as i32).to_be_bytes()));
            t
        };
        let curve = para(0, &[1.0]);
        let tags: Vec<(&[u8; 4], Vec<u8>)> = vec![
            (b"rXYZ", xyz([0.4361, 0.2225, 0.0139])),
            (b"gXYZ", xyz([0.3851, 0.7169, 0.0971])),
            (b"bXYZ", xyz([0.1431, 0.0606, 0.7141])),
            (b"rTRC", curve.clone()),
            (b"gTRC", curve.clone()),
            (b"bTRC", curve),
        ];
        let matrix = build(b"RGB ", b"XYZ ", &tags);
        let kind = Profile::parse(&matrix).unwrap().kind().unwrap();
        assert_eq!(recognise(&kind).map(|(cs, linear)| (cs.name, linear)), Some(("sRGB", true)));
        // An unreadable table is an error, never a fallback to the colorants.
        let mut with_lut = tags.clone();
        with_lut.push((b"A2B0", b"mft2\0\0\0\0".to_vec()));
        let err = Profile::parse(&build(b"RGB ", b"XYZ ", &with_lut)).unwrap().kind().unwrap_err();
        assert!(err.contains("A2B0 lookup table"), "{err}");
        let err = Profile::parse(&build(b"CMYK", b"Lab ", &[(b"A2B0", vec![0; 64])])).unwrap().kind().unwrap_err();
        assert!(err.contains("CMYK"), "{err}");
        assert!(Profile::parse(&build(b"CMYK", b"XYZ ", &[])).unwrap().kind().is_err());
        assert!(Profile::parse(&build(b"RGB ", b"Lab ", &[])).unwrap().kind().is_err());
    }

    #[test]
    fn lut_stages_evaluate_in_order() {
        // lutAtoB, Lab PCS: A curves (gamma 2), 2-point identity CLUT,
        // M curves (identity), matrix (scale 0.5, offset), B curves (identity).
        let curve = |g: f64| para(0, &[g]);
        let curves = |g: f64, n: usize| (0..n).flat_map(|_| curve(g)).collect::<Vec<u8>>();
        let mut clut = vec![2u8, 2, 2];
        clut.resize(16, 0);
        clut.extend([2, 0, 0, 0]);
        for r in 0..2u16 {
            for g in 0..2u16 {
                for b in 0..2u16 {
                    for v in [r, g, b] {
                        clut.extend((v * 65535).to_be_bytes());
                    }
                }
            }
        }
        let matrix: Vec<u8> = [0.5, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.5, 0.25, 0.0, 0.0]
            .iter()
            .flat_map(|v: &f64| ((v * 65536.0).round() as i32).to_be_bytes())
            .collect();
        let (b, m) = (curves(1.0, 3), curves(1.0, 3));
        let a = curves(2.0, 3);
        let at_b = 32;
        let at_matrix = at_b + b.len();
        let at_m = at_matrix + matrix.len();
        let at_clut = at_m + m.len();
        let at_a = at_clut + clut.len();
        let mut tag = b"mAB \0\0\0\0".to_vec();
        tag.extend([3, 3, 0, 0]);
        for o in [at_b, at_matrix, at_m, at_clut, at_a] {
            tag.extend((o as u32).to_be_bytes());
        }
        for part in [&b, &matrix, &m, &clut, &a] {
            tag.extend(part.iter());
        }
        let profile = build(b"RGB ", b"Lab ", &[(b"A2B0", tag)]);
        let Kind::Lut(lut) = Profile::parse(&profile).unwrap().kind().unwrap() else { panic!() };
        // (0.6, 0.5, 0.5) → A: (0.36, 0.25, 0.25) → matrix: (0.43, 0.125, 0.125)
        // → Lab (43, 0.125·255 − 128, …).
        let want = lab_to_xyz(43.0, 0.125 * 255.0 - 128.0, 0.125 * 255.0 - 128.0);
        let got = lut.eval(&[0.6, 0.5, 0.5]);
        for c in 0..3 {
            assert!((got[c] - want[c]).abs() < 1e-4, "{got:?} vs {want:?}");
        }
    }

    #[test]
    fn tetrahedral_interpolation_is_exact_for_linear_tables() {
        // A 3-point grid sampling an affine function reproduces it anywhere.
        let f = |p: [f64; 3]| [0.2 * p[0] + 0.3 * p[1] + 0.1 * p[2], p[1] * 0.9, 0.05 + 0.5 * p[2]];
        let mut table = Vec::new();
        for r in 0..3 {
            for g in 0..3 {
                for b in 0..3 {
                    table.extend(f([r as f64 / 2.0, g as f64 / 2.0, b as f64 / 2.0]));
                }
            }
        }
        for p in [[0.1, 0.7, 0.3], [0.9, 0.2, 0.55], [0.5, 0.5, 0.5], [1.0, 0.0, 1.0]] {
            let got = interpolate(&[3, 3, 3], 3, &table, &p);
            let want = f(p);
            for c in 0..3 {
                assert!((got[c] - want[c]).abs() < 1e-12, "{p:?}");
            }
        }
    }

    #[test]
    fn malformed_profiles_are_errors() {
        assert!(Profile::parse(b"not a profile").is_err());
        let mut bad = build(b"RGB ", b"XYZ ", &[(b"rXYZ", vec![0; 20])]);
        bad[132 + 4..132 + 8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(Profile::parse(&bad).is_err());
        let truncated = build(b"RGB ", b"XYZ ", &[(b"rXYZ", vec![0; 4])]);
        assert!(Profile::parse(&truncated).unwrap().kind().is_err());
    }
}
