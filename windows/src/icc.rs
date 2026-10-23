//! The monitor's colour profile for the canvas (§20.2, decided: convert to
//! the monitor's ICC profile; plain sRGB when it can't be used). Windows
//! doesn't colour-manage a swap chain, so an sRGB preview would look
//! oversaturated on a wide-gamut monitor. Matrix/TRC profiles (what monitor
//! calibration writes) become a 3×3 matrix from linear sRGB to the monitor's
//! linear RGB, and a LUT that applies the inverse of its tone curves.
//! LUT-based profiles, and profiles that are sRGB anyway, give `None`.

use std::path::{Path, PathBuf};

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::ColorSystem::GetICMProfileW;
use windows::core::{PCWSTR, PWSTR};

use crate::gpu::LUT_SIZE;

/// Linear sRGB to PCS XYZ (D50), Bradford-adapted, as ICC profiles use it.
const SRGB_TO_D50: [[f64; 3]; 3] = [
    [0.4360747, 0.3850649, 0.1430804],
    [0.2225045, 0.7168786, 0.0606169],
    [0.0139322, 0.0971045, 0.7141733],
];

#[derive(Clone, Debug, PartialEq)]
pub enum Curve {
    Gamma(f64),
    Table(Vec<f64>),
    /// ICC parametric curve: function type and its parameters.
    Parametric(u16, Vec<f64>),
}

impl Curve {
    /// Encoded value to linear light.
    pub fn eval(&self, x: f64) -> f64 {
        let x = x.clamp(0.0, 1.0);
        match self {
            Curve::Gamma(g) => x.powf(*g),
            Curve::Table(t) => {
                if t.len() < 2 {
                    return x;
                }
                let p = x * (t.len() - 1) as f64;
                let i = (p.floor() as usize).min(t.len() - 2);
                let f = p - i as f64;
                t[i] * (1.0 - f) + t[i + 1] * f
            }
            Curve::Parametric(kind, p) => {
                let g = p[0];
                let at = |i: usize| p.get(i).copied().unwrap_or(0.0);
                let (a, b, c, d, e, f) = (at(1), at(2), at(3), at(4), at(5), at(6));
                match kind {
                    0 => x.powf(g),
                    1 => if x >= -b / a { (a * x + b).max(0.0).powf(g) } else { 0.0 },
                    2 => if x >= -b / a { (a * x + b).max(0.0).powf(g) + c } else { c },
                    3 => if x >= d { (a * x + b).max(0.0).powf(g) } else { c * x },
                    _ => if x >= d { (a * x + b).max(0.0).powf(g) + e } else { c * x + f },
                }
            }
        }
        .clamp(0.0, 1.0)
    }

    /// Linear light back to the encoded value, by bisection (the curves
    /// are monotonic).
    pub fn invert(&self, y: f64) -> f64 {
        let (mut low, mut high) = (0.0, 1.0);
        for _ in 0..40 {
            let mid = (low + high) / 2.0;
            if self.eval(mid) < y { low = mid } else { high = mid }
        }
        (low + high) / 2.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MatrixProfile {
    /// Columns: the red, green and blue colorants in PCS XYZ (D50).
    pub colorants: [[f64; 3]; 3],
    pub curves: [Curve; 3],
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

/// Parses a matrix/TRC RGB display profile.
pub fn parse(data: &[u8]) -> Option<MatrixProfile> {
    if data.get(16..20)? != b"RGB " || data.get(20..24)? != b"XYZ " {
        return None;
    }
    let count = be32(data, 128)? as usize;
    let mut tags = std::collections::HashMap::new();
    for i in 0..count.min(256) {
        let at = 132 + i * 12;
        let signature = data.get(at..at + 4)?;
        tags.insert(signature.to_vec(), (be32(data, at + 4)? as usize, be32(data, at + 8)? as usize));
    }
    let tag = |name: &[u8]| -> Option<&[u8]> {
        let (offset, size) = *tags.get(name)?;
        data.get(offset..offset + size)
    };
    let xyz = |name: &[u8]| -> Option<[f64; 3]> {
        let t = tag(name)?;
        if t.get(0..4)? != b"XYZ " {
            return None;
        }
        Some([s15(t, 8)?, s15(t, 12)?, s15(t, 16)?])
    };
    let curve = |name: &[u8]| -> Option<Curve> {
        let t = tag(name)?;
        match t.get(0..4)? {
            b"curv" => {
                let n = be32(t, 8)? as usize;
                match n {
                    0 => Some(Curve::Gamma(1.0)),
                    1 => Some(Curve::Gamma(be16(t, 12)? as f64 / 256.0)),
                    _ => Some(Curve::Table((0..n).map(|i| be16(t, 12 + 2 * i).map(|v| v as f64 / 65535.0)).collect::<Option<_>>()?)),
                }
            }
            b"para" => {
                let kind = be16(t, 8)?;
                let params = [1usize, 3, 4, 5, 7].get(kind as usize)?;
                Some(Curve::Parametric(kind, (0..*params).map(|i| s15(t, 12 + 4 * i)).collect::<Option<_>>()?))
            }
            _ => None,
        }
    };
    let (r, g, b) = (xyz(b"rXYZ")?, xyz(b"gXYZ")?, xyz(b"bXYZ")?);
    Some(MatrixProfile {
        colorants: [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]],
        curves: [curve(b"rTRC")?, curve(b"gTRC")?, curve(b"bTRC")?],
    })
}

fn invert3(m: &[[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    if det.abs() < 1e-12 {
        return None;
    }
    let c = |a: usize, b: usize, c: usize, d: usize| m[a][b] * m[c][d] - m[a][d] * m[c][b];
    Some([
        [c(1, 1, 2, 2) / det, -c(0, 1, 2, 2) / det, c(0, 1, 1, 2) / det],
        [-c(1, 0, 2, 2) / det, c(0, 0, 2, 2) / det, -c(0, 0, 1, 2) / det],
        [c(1, 0, 2, 1) / det, -c(0, 0, 2, 1) / det, c(0, 0, 1, 1) / det],
    ])
}

fn multiply(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

fn srgb_decode(v: f64) -> f64 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

/// What the canvas needs: linear sRGB to monitor linear, and the LUT from
/// sRGB-encoded index to the monitor's encoding.
#[derive(Clone, Debug)]
pub struct Transform {
    pub matrix: [[f32; 3]; 3],
    pub lut: Vec<[f32; 3]>,
    pub profile: PathBuf,
}

impl MatrixProfile {
    pub fn transform(&self, profile: &Path) -> Option<Transform> {
        let to_monitor = multiply(&invert3(&self.colorants)?, &SRGB_TO_D50);
        // A profile that is sRGB (the Windows default) changes nothing: skip
        // the round trip and its rounding.
        let identity = (0..3).all(|i| (0..3).all(|j| (to_monitor[i][j] - if i == j { 1.0 } else { 0.0 }).abs() < 2e-3));
        let srgb_curves = self.curves.iter().all(|c| (0..=16).all(|i| {
            let x = i as f64 / 16.0;
            (c.eval(x) - srgb_decode(x)).abs() < 2e-3
        }));
        if identity && srgb_curves {
            return None;
        }
        let lut = (0..LUT_SIZE)
            .map(|i| {
                let linear = srgb_decode(i as f64 / (LUT_SIZE - 1) as f64);
                [0, 1, 2].map(|c| self.curves[c].invert(linear) as f32)
            })
            .collect();
        Some(Transform { matrix: to_monitor.map(|row| row.map(|v| v as f32)), lut, profile: profile.to_path_buf() })
    }
}

/// The profile of the monitor `hwnd` is on, if Windows has one assigned.
pub fn monitor_profile(hwnd: HWND) -> Option<PathBuf> {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if !GetMonitorInfoW(monitor, &mut info as *mut _ as *mut MONITORINFO).as_bool() {
            return None;
        }
        let dc = CreateDCW(PCWSTR(info.szDevice.as_ptr()), PCWSTR(info.szDevice.as_ptr()), PCWSTR::null(), None);
        if dc.is_invalid() {
            return None;
        }
        let mut buffer = [0u16; 260];
        let mut size = buffer.len() as u32;
        let ok = GetICMProfileW(dc, &mut size, PWSTR(buffer.as_mut_ptr())).as_bool();
        let _ = DeleteDC(dc);
        ok.then(|| PathBuf::from(crate::platform::from_wide(&buffer)))
    }
}

/// The transform for `hwnd`'s monitor, or None for plain sRGB.
pub fn for_window(hwnd: HWND) -> Option<Transform> {
    let path = monitor_profile(hwnd)?;
    let data = std::fs::read(&path).ok()?;
    parse(&data)?.transform(&path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(signature: &[u8], body: Vec<u8>) -> (Vec<u8>, Vec<u8>) {
        (signature.to_vec(), body)
    }

    fn s15b(v: f64) -> [u8; 4] {
        ((v * 65536.0).round() as i32).to_be_bytes()
    }

    fn profile(colorants: [[f64; 3]; 3], gamma: f64) -> Vec<u8> {
        let xyz = |c: [f64; 3]| {
            let mut b = b"XYZ \0\0\0\0".to_vec();
            for v in c {
                b.extend(s15b(v));
            }
            b
        };
        let curv = || {
            let mut b = b"curv\0\0\0\0".to_vec();
            b.extend(1u32.to_be_bytes());
            b.extend(((gamma * 256.0).round() as u16).to_be_bytes());
            b
        };
        let tags = vec![
            tag(b"rXYZ", xyz([colorants[0][0], colorants[1][0], colorants[2][0]])),
            tag(b"gXYZ", xyz([colorants[0][1], colorants[1][1], colorants[2][1]])),
            tag(b"bXYZ", xyz([colorants[0][2], colorants[1][2], colorants[2][2]])),
            tag(b"rTRC", curv()),
            tag(b"gTRC", curv()),
            tag(b"bTRC", curv()),
        ];
        let mut data = vec![0u8; 128];
        data[16..20].copy_from_slice(b"RGB ");
        data[20..24].copy_from_slice(b"XYZ ");
        data.extend((tags.len() as u32).to_be_bytes());
        let mut offset = 128 + 4 + tags.len() * 12;
        let mut bodies: Vec<u8> = vec![];
        for (signature, body) in &tags {
            data.extend(signature);
            data.extend((offset as u32).to_be_bytes());
            data.extend((body.len() as u32).to_be_bytes());
            offset += body.len();
            bodies.extend(body);
        }
        data.extend(bodies);
        data
    }

    /// Display P3 colorants (D50-adapted), as in Apple's and Windows' P3 profiles.
    const P3: [[f64; 3]; 3] = [[0.5151, 0.2920, 0.1571], [0.2412, 0.6922, 0.0666], [-0.0011, 0.0419, 0.7841]];

    #[test]
    fn srgb_profiles_change_nothing() {
        let data = profile(SRGB_TO_D50, 2.2);
        let parsed = parse(&data).unwrap();
        assert_eq!(parsed.curves[0], Curve::Gamma(2.19921875));
        // Gamma 2.2 is not the sRGB curve, so it still converts…
        assert!(parsed.transform(Path::new("x.icc")).is_some());
        // …but colorants and a curve of sRGB's own shape don't.
        let srgb = MatrixProfile {
            colorants: SRGB_TO_D50,
            curves: [0, 1, 2].map(|_| Curve::Parametric(3, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])),
        };
        assert!(srgb.transform(Path::new("srgb.icc")).is_none());
    }

    #[test]
    fn wide_gamut_monitors_get_a_matrix_and_curves() {
        let parsed = parse(&profile(P3, 2.2)).unwrap();
        let transform = parsed.transform(Path::new("p3.icc")).unwrap();
        // sRGB red is less saturated on a P3 monitor: some green and blue appear.
        let red = [transform.matrix[0][0], transform.matrix[1][0], transform.matrix[2][0]];
        assert!(red[0] > 0.8 && red[0] < 0.85 && red[1] > 0.03 && red[2] > 0.0, "{red:?}");
        // White stays white.
        for row in transform.matrix {
            assert!((row.iter().sum::<f32>() - 1.0).abs() < 2e-3);
        }
        // The LUT inverts gamma 2.2: sRGB 50 % grey (linear 0.214) encodes at 0.4964.
        let mid = transform.lut[(LUT_SIZE as usize - 1) / 2];
        assert!((mid[0] as f64 - 0.214f64.powf(1.0 / 2.19921875)).abs() < 2e-3, "{mid:?}");
        assert_eq!(transform.lut.len(), LUT_SIZE as usize);
    }

    #[test]
    fn curves_invert() {
        for curve in [Curve::Gamma(2.2), Curve::Table(vec![0.0, 0.1, 0.5, 1.0]), Curve::Parametric(3, vec![2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045])] {
            for i in 0..=10 {
                let x = i as f64 / 10.0;
                assert!((curve.invert(curve.eval(x)) - x).abs() < 1e-6, "{curve:?} {x}");
            }
        }
        assert!(parse(b"not a profile").is_none());
    }
}
