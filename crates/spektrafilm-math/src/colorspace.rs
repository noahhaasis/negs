/// RGB colour spaces, chromatic adaptation and transfer functions.
///
/// Everything here follows colour-science 0.4.7, which upstream uses for
/// every RGB conversion: the matrices, whitepoints and constants are
/// generated from it by `scripts/gen_colorspaces.py` (`colorspace_data.rs`),
/// and the functions repeat its arithmetic.
///
/// All matrices are row-major: output[i] = dot(matrix[i], input).
pub use crate::colorspace_data::{CAT02, CAT02_INVERSE, CAT16, CAT16_INVERSE, COLOURSPACES};
use crate::colorspace_data as data;

/// A colourspace of `colour.RGB_COLOURSPACES`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RgbColourspace {
    /// colour's name, as upstream's params spell it.
    pub name: &'static str,
    /// Other names colour (or older Negs params) use for it.
    pub aliases: &'static [&'static str],
    pub whitepoint_xy: [f64; 2],
    /// The whitepoint as colour's `xy_to_XYZ` gives it (the CAT input).
    pub whitepoint_xyz: [f64; 3],
    /// The whitepoint as upstream's gamut compressor computes it
    /// (`_xy_to_xyz_unit_y`), which divides in a different order.
    pub whitepoint_xyz_unit_y: [f64; 3],
    /// `matrix_RGB_to_XYZ`, the published matrix where colour stores one.
    pub rgb_to_xyz: [[f64; 3]; 3],
    /// `matrix_XYZ_to_RGB`, which for sRGB, Adobe RGB, ProPhoto, ACES2065-1
    /// and V-Gamut is the published inverse, not the exact one.
    pub xyz_to_rgb: [[f64; 3]; 3],
    /// `matrix_RGB_to_RGB(cs, cs)`: what `RGB_to_RGB` applies before the
    /// output encoding. Not the identity where the published matrices
    /// don't invert exactly (about 1e-4 for ProPhoto).
    pub rgb_to_rgb_self: [[f64; 3]; 3],
    pub cctf: Cctf,
    /// CAM16 `D_RGB` and `A_w` with this whitepoint as the adapting white,
    /// under the output gamut compressor's viewing conditions.
    pub cam16_d_rgb: [f64; 3],
    pub cam16_a_w: f64,
}

impl RgbColourspace {
    /// The colourspace called `name` (colour's name or an alias, any case).
    pub fn find(name: &str) -> Option<&'static RgbColourspace> {
        COLOURSPACES.iter().find(|cs| {
            cs.name.eq_ignore_ascii_case(name)
                || cs.aliases.iter().any(|a| a.eq_ignore_ascii_case(name))
        })
    }

    /// The input colourspace `name`; unknown names fall back to ProPhoto RGB,
    /// upstream's default input.
    pub fn input(name: &str) -> &'static RgbColourspace {
        Self::find(name).unwrap_or(&PROPHOTO)
    }

    /// The output colourspace `name`; unknown names fall back to sRGB,
    /// upstream's default output.
    pub fn output(name: &str) -> &'static RgbColourspace {
        Self::find(name).unwrap_or(&SRGB)
    }
}

pub static SRGB: RgbColourspace = COLOURSPACES[0];
pub static PROPHOTO: RgbColourspace = COLOURSPACES[5];

/// A colourspace's transfer function pair (`cctf_encoding`/`cctf_decoding`).
///
/// Negative values behave as in colour: the curves with a linear toe (sRGB,
/// BT.2020, ROMM, V-Log, DaVinci Intermediate) stay linear below zero, and
/// the pure gamma curves (DCI-P3, Adobe RGB) give NaN, since colour's
/// `gamma_function` defaults to "Indeterminate" (`a ** exponent`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cctf {
    Linear,
    /// `eotf_inverse_sRGB` / `eotf_sRGB`.
    Srgb,
    /// `gamma_function` with these exponents.
    Gamma { encode: f64, decode: f64 },
    /// `oetf_BT2020` / `oetf_inverse_BT2020` (10-bit constants).
    Bt2020,
    /// `cctf_encoding_ROMMRGB` / `cctf_decoding_ROMMRGB` (8-bit, float out).
    Romm,
    /// `log_encoding_VLog` / `log_decoding_VLog`.
    VLog,
    /// `oetf_DaVinciIntermediate` / `oetf_inverse_DaVinciIntermediate`.
    DaVinciIntermediate,
}

/// colour's `spow`: `sign(a) * |a| ** p`.
#[inline]
fn spow(a: f64, p: f64) -> f64 {
    let s = if a > 0.0 {
        1.0
    } else if a < 0.0 {
        -1.0
    } else if a == 0.0 {
        0.0
    } else {
        f64::NAN
    };
    s * a.abs().powf(p)
}

impl Cctf {
    /// Linear → encoded, as `cs.cctf_encoding`.
    pub fn encode(self, x: f64) -> f64 {
        match self {
            Cctf::Linear => x,
            Cctf::Srgb => {
                if x <= 0.0031308 {
                    x * 12.92
                } else {
                    1.055 * spow(x, 1.0 / 2.4) - 0.055
                }
            }
            Cctf::Gamma { encode, .. } => x.powf(encode),
            Cctf::Bt2020 => {
                let (a, b) = (data::BT2020_ALPHA, data::BT2020_BETA);
                if b > x {
                    x * 4.5
                } else {
                    a * spow(x, 0.45) - (a - 1.0)
                }
            }
            Cctf::Romm => {
                let i_max = data::ROMM_I_MAX;
                let x_p = if data::ROMM_E_T > x {
                    x * 16.0 * i_max
                } else {
                    spow(x, 1.0 / 1.8) * i_max
                };
                x_p / i_max
            }
            Cctf::VLog => {
                if x < data::VLOG_CUT1 {
                    5.6 * x + 0.125
                } else {
                    data::VLOG_C * (x + data::VLOG_B).log10() + data::VLOG_D
                }
            }
            Cctf::DaVinciIntermediate => {
                if x <= data::DI_LIN_CUT {
                    x * data::DI_M
                } else {
                    data::DI_C * ((x + data::DI_A).log2() + data::DI_B)
                }
            }
        }
    }

    /// Encoded → linear, as `cs.cctf_decoding`.
    pub fn decode(self, x: f64) -> f64 {
        match self {
            Cctf::Linear => x,
            Cctf::Srgb => {
                if data::SRGB_DECODE_CUT >= x {
                    x / 12.92
                } else {
                    spow((x + 0.055) / 1.055, 2.4)
                }
            }
            Cctf::Gamma { decode, .. } => x.powf(decode),
            Cctf::Bt2020 => {
                let a = data::BT2020_ALPHA;
                if x < data::BT2020_DECODE_CUT {
                    x / 4.5
                } else {
                    spow((x + (a - 1.0)) / a, 1.0 / 0.45)
                }
            }
            Cctf::Romm => {
                let i_max = data::ROMM_I_MAX;
                let x_p = x * i_max;
                if x_p < 16.0 * data::ROMM_E_T * i_max {
                    x_p / (16.0 * i_max)
                } else {
                    spow(x_p / i_max, 1.8)
                }
            }
            Cctf::VLog => {
                if x < data::VLOG_CUT2 {
                    (x - 0.125) / 5.6
                } else {
                    10f64.powf((x - data::VLOG_D) / data::VLOG_C) - data::VLOG_B
                }
            }
            Cctf::DaVinciIntermediate => {
                if x <= data::DI_LOG_CUT {
                    x / data::DI_M
                } else {
                    2f64.powf(x / data::DI_C - data::DI_B) - data::DI_A
                }
            }
        }
    }

    /// The sRGB curve, which the GPU post-scan passes implement.
    pub fn is_srgb(self) -> bool {
        self == Cctf::Srgb
    }
}

/// Round a matrix to f32.
pub const fn mat_f32(m: [[f64; 3]; 3]) -> [[f32; 3]; 3] {
    let mut out = [[0.0f32; 3]; 3];
    let mut i = 0;
    while i < 3 {
        let mut j = 0;
        while j < 3 {
            out[i][j] = m[i][j] as f32;
            j += 1;
        }
        i += 1;
    }
    out
}

/// sRGB → CIE XYZ (D65), assuming linear input.
pub const SRGB_TO_XYZ: [[f32; 3]; 3] = mat_f32(COLOURSPACES[0].rgb_to_xyz);
/// CIE XYZ (D65) → sRGB linear (colour's published inverse).
pub const XYZ_TO_SRGB: [[f32; 3]; 3] = mat_f32(COLOURSPACES[0].xyz_to_rgb);

/// Convert xy chromaticity to XYZ (Y=1).
pub fn xy_to_xyz(x: f32, y: f32) -> [f32; 3] {
    xy_to_xyz_f64(x as f64, y as f64).map(|v| v as f32)
}

/// colour's `xyY_to_XYZ(xy_to_xyY(xy))` at Y = 1, with its operation order.
pub fn xy_to_xyz_f64(x: f64, y: f64) -> [f64; 3] {
    if y <= 0.0 {
        return [0.0, 1.0, 0.0];
    }
    let y_y = 1.0 / y;
    [x * y_y, 1.0, (1.0 - (x + y)) * y_y]
}

/// Apply 3x3 matrix to RGB triple.
#[inline]
pub fn mat3_mul(m: &[[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    [
        m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2],
        m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2],
        m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2],
    ]
}

/// Chromatic adaptation from source white to destination white using CAT02.
/// Computes in f64 internally for precision, returns f32.
pub fn chromatic_adaptation_matrix(src_white: [f32; 3], dst_white: [f32; 3]) -> [[f32; 3]; 3] {
    let m = chromatic_adaptation_matrix_f64(src_white.map(f64::from), dst_white.map(f64::from));
    mat_f32(m)
}

/// CAT02 von Kries adaptation, as colour's
/// `matrix_chromatic_adaptation_VonKries(..., transform="CAT02")`, the
/// default of `RGB_to_XYZ`, `XYZ_to_RGB` and `RGB_to_RGB`.
pub fn chromatic_adaptation_matrix_f64(src_white: [f64; 3], dst_white: [f64; 3]) -> [[f64; 3]; 3] {
    von_kries(src_white, dst_white, &CAT02, &CAT02_INVERSE)
}

/// CAT16 (Li et al. 2017) von Kries chromatic adaptation. Same form as
/// `chromatic_adaptation_matrix_f64` (CAT02) but with the CIECAM16 cone
/// matrix `M16`, which avoids CAT02's cone-primary instabilities around
/// blue/violet. Matches colour-science's
/// `matrix_chromatic_adaptation_VonKries(..., transform="CAT16")`.
pub fn chromatic_adaptation_matrix_cat16_f64(
    src_white: [f64; 3],
    dst_white: [f64; 3],
) -> [[f64; 3]; 3] {
    von_kries(src_white, dst_white, &CAT16, &CAT16_INVERSE)
}

/// Shared von Kries adaptation: `(inv · diag(dst_lms/src_lms)) · fwd`, where
/// `fwd`/`inv` are the cone matrix and its inverse for the chosen transform.
fn von_kries(
    src_white: [f64; 3],
    dst_white: [f64; 3],
    fwd: &[[f64; 3]; 3],
    inv: &[[f64; 3]; 3],
) -> [[f64; 3]; 3] {
    let mut src_lms = [0.0f64; 3];
    let mut dst_lms = [0.0f64; 3];
    for i in 0..3 {
        src_lms[i] = fwd[i][0] * src_white[0] + fwd[i][1] * src_white[1] + fwd[i][2] * src_white[2];
        dst_lms[i] = fwd[i][0] * dst_white[0] + fwd[i][1] * dst_white[1] + fwd[i][2] * dst_white[2];
    }
    let gain = [
        dst_lms[0] / src_lms[0],
        dst_lms[1] / src_lms[1],
        dst_lms[2] / src_lms[2],
    ];

    let mut result = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            result[i][j] = inv[i][0] * gain[0] * fwd[0][j]
                + inv[i][1] * gain[1] * fwd[1][j]
                + inv[i][2] * gain[2] * fwd[2][j];
        }
    }
    result
}

/// The ICC profile connection space white, D50 as ICC.1 rounds it.
pub const PCS_D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// Bradford cone response matrix (ICC.1 Annex E).
pub const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

/// Von Kries adaptation from `src` to `dst` white in Bradford cone space, the
/// transform ICC profiles use to reach and leave the D50 connection space.
pub fn bradford_matrix(src: [f64; 3], dst: [f64; 3]) -> [[f64; 3]; 3] {
    von_kries(src, dst, &BRADFORD, &mat3_inverse(&BRADFORD))
}

/// The inverse of a 3x3 matrix (transposed cofactors over the determinant).
pub fn mat3_inverse(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |r: usize, k: usize| {
        let (r1, r2, k1, k2) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det))
}

/// Linear `from` RGB → linear `to` RGB as a colour-managed application
/// converts between the two ICC profiles (relative colorimetric): through
/// XYZ, with Bradford between the whites. Each space's own matrix is inverted
/// exactly, so converting there and back is the identity, and the same space
/// gives the identity. Values outside [0, 1] are kept.
pub fn icc_rgb_to_rgb(from: &RgbColourspace, to: &RgbColourspace) -> [[f64; 3]; 3] {
    if from.name == to.name {
        return [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    }
    let adapt = bradford_matrix(from.whitepoint_xyz, to.whitepoint_xyz);
    mat3_mul3_f64(&mat3_inverse(&to.rgb_to_xyz), &mat3_mul3_f64(&adapt, &from.rgb_to_xyz))
}

/// `m · v`.
#[inline]
pub fn mat3_vec_f64(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    m.map(|row| row[0] * v[0] + row[1] * v[1] + row[2] * v[2])
}

/// `a · b` for 3x3 matrices.
pub fn mat3_mul3_f64(a: &[[f64; 3]; 3], b: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let mut out = [[0.0f64; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            out[i][j] = a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j];
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precision::{from_f64, srgb_decode, srgb_encode};

    /// CAT16 adaptation matches colour-science's
    /// `matrix_chromatic_adaptation_VonKries(transform="CAT16")` for the
    /// ProPhoto-white → D55 case used by the input projection.
    #[test]
    fn cat16_matches_colour() {
        let src = PROPHOTO.whitepoint_xyz;
        let dst = [0.9567982961086806, 1.0, 0.9213965001151279]; // D55
        let got = chromatic_adaptation_matrix_cat16_f64(src, dst);
        let want = [
            [
                0.9955740273098072,
                -0.016544957254590615,
                0.01613798204777648,
            ],
            [
                -0.0022206862456965637,
                1.0026392099167525,
                -0.0006033317106774956,
            ],
            [
                -0.00013585839424725028,
                0.0054854466982645,
                1.1102132484680078,
            ],
        ];
        for i in 0..3 {
            for j in 0..3 {
                assert!(
                    (got[i][j] - want[i][j]).abs() < 1e-12,
                    "[{i}][{j}] got {} want {}",
                    got[i][j],
                    want[i][j]
                );
            }
        }
    }

    #[test]
    fn names_and_aliases_resolve() {
        assert_eq!(RgbColourspace::find("ProPhoto RGB").unwrap().name, "ProPhoto RGB");
        assert_eq!(RgbColourspace::find("Rec. 2020").unwrap().name, "ITU-R BT.2020");
        assert_eq!(RgbColourspace::find("srgb").unwrap().name, "sRGB");
        assert_eq!(RgbColourspace::find("aces").unwrap().name, "ACES2065-1");
        assert!(RgbColourspace::find("nonsense").is_none());
        assert_eq!(RgbColourspace::input("nonsense").name, "ProPhoto RGB");
        assert_eq!(RgbColourspace::output("nonsense").name, "sRGB");
    }

    #[test]
    fn cctfs_round_trip() {
        for cs in COLOURSPACES {
            // Away from the V-Log cuts, where the two curves meet only to ~1e-6.
            for &v in &[0.0, 1e-4, 0.05, 0.18, 0.5, 1.0] {
                let back = cs.cctf.decode(cs.cctf.encode(v));
                assert!((back - v).abs() < 1e-12, "{}: {v} -> {back}", cs.name);
            }
        }
    }

    #[test]
    fn test_srgb_roundtrip() {
        for &v in &[0.0_f64, 0.001, 0.01, 0.1, 0.18, 0.5, 0.9, 1.0] {
            let s = from_f64(v);
            let encoded = srgb_encode(s);
            let decoded = srgb_decode(encoded);
            assert!(
                (s - decoded).abs() < from_f64(1e-5),
                "roundtrip failed for {v}: encoded={encoded}, decoded={decoded}"
            );
        }
    }

    #[test]
    fn test_identity_adaptation() {
        let d65 = xy_to_xyz(0.3127, 0.329);
        let m = chromatic_adaptation_matrix(d65, d65);
        for i in 0..3 {
            for j in 0..3 {
                let expected = if i == j { 1.0 } else { 0.0 };
                assert!(
                    (m[i][j] - expected).abs() < 1e-4,
                    "identity adaptation [{i}][{j}]: expected {expected}, got {}",
                    m[i][j]
                );
            }
        }
    }

    #[test]
    fn test_srgb_xyz_roundtrip() {
        let rgb = [0.5, 0.3, 0.8];
        let xyz = mat3_mul(&SRGB_TO_XYZ, rgb);
        let rgb2 = mat3_mul(&XYZ_TO_SRGB, xyz);
        for i in 0..3 {
            assert!(
                (rgb[i] - rgb2[i]).abs() < 1e-4,
                "RGB roundtrip failed channel {i}: {:.6} vs {:.6}",
                rgb[i],
                rgb2[i]
            );
        }
    }
}
