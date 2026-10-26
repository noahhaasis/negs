//! ICC profiles for saved images, so other applications know which colour
//! space the pixels are in.
//!
//! The profiles are ICC v2.1 matrix/TRC display profiles, the form every
//! colour-managed reader understands: the colorants are the colour space's
//! primaries adapted to the D50 profile connection space with Bradford, as
//! ICC requires, and the media white point is the colour space's own. The
//! tone curve is a single gamma where the space's decoding is a pure power
//! with an exact `u8Fixed8` exponent (Adobe RGB), otherwise a 1024-entry
//! table of its decoding (sRGB, Display P3, ProPhoto's ROMM curve).

use spektrafilm_math::colorspace::{Cctf, RgbColourspace, mat3_mul3_f64};

/// The ICC profile connection space white, D50 as ICC rounds it.
const PCS_D50: [f64; 3] = [0.9642, 1.0, 0.8249];

/// Bradford cone response matrix (ICC.1 Annex E).
const BRADFORD: [[f64; 3]; 3] = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
];

const TABLE_SIZE: usize = 1024;

/// The profile of `space`. `linear` gives it a linear tone curve, for
/// images saved without the output encoding (`io.output_cctf_encoding`).
pub fn profile(space: &RgbColourspace, linear: bool) -> Vec<u8> {
    let colorants = mat3_mul3_f64(&bradford(space.whitepoint_xyz, PCS_D50), &space.rgb_to_xyz);
    let column = |j: usize| xyz_tag([colorants[0][j], colorants[1][j], colorants[2][j]]);
    let curve = curve_tag(if linear { Cctf::Linear } else { space.cctf });
    let tags: [(&[u8; 4], Vec<u8>); 9] = [
        (b"desc", desc_tag(space.name)),
        (b"cprt", text_tag("No copyright, use freely")),
        (b"wtpt", xyz_tag(space.whitepoint_xyz)),
        (b"rXYZ", column(0)),
        (b"gXYZ", column(1)),
        (b"bXYZ", column(2)),
        (b"rTRC", curve.clone()),
        (b"gTRC", curve.clone()),
        (b"bTRC", curve),
    ];

    let table_end = 128 + 4 + 12 * tags.len();
    let mut data = Vec::new();
    let mut entries = Vec::new();
    // The three curves share one copy, as most profiles do.
    let mut curve_entry = None;
    for (signature, body) in &tags {
        let shared = signature.ends_with(b"TRC");
        let entry = match (shared, curve_entry) {
            (true, Some(entry)) => entry,
            _ => {
                let entry = ((table_end + data.len()) as u32, body.len() as u32);
                data.extend_from_slice(body);
                while data.len() % 4 != 0 {
                    data.push(0);
                }
                if shared {
                    curve_entry = Some(entry);
                }
                entry
            }
        };
        entries.push((**signature, entry));
    }

    let size = table_end + data.len();
    let mut out = Vec::with_capacity(size);
    out.extend((size as u32).to_be_bytes());
    out.extend([0; 4]); // preferred CMM
    out.extend(0x0210_0000u32.to_be_bytes()); // version 2.1
    out.extend(b"mntr");
    out.extend(b"RGB ");
    out.extend(b"XYZ ");
    // A fixed creation date keeps saved files byte-for-byte reproducible.
    for v in [2026u16, 1, 1, 0, 0, 0] {
        out.extend(v.to_be_bytes());
    }
    out.extend(b"acsp");
    out.extend([0; 24]); // platform, flags, manufacturer, model, attributes
    out.extend(0u32.to_be_bytes()); // perceptual intent
    for v in PCS_D50 {
        out.extend(s15_fixed16(v));
    }
    out.resize(128, 0); // creator, reserved
    out.extend((tags.len() as u32).to_be_bytes());
    for (signature, (offset, length)) in entries {
        out.extend(signature);
        out.extend(offset.to_be_bytes());
        out.extend(length.to_be_bytes());
    }
    out.extend(data);
    debug_assert_eq!(out.len(), size);
    out
}

/// Von Kries adaptation from `src` to `dst` white in Bradford cone space.
fn bradford(src: [f64; 3], dst: [f64; 3]) -> [[f64; 3]; 3] {
    let cone = |w: [f64; 3]| BRADFORD.map(|row| row[0] * w[0] + row[1] * w[1] + row[2] * w[2]);
    let (s, d) = (cone(src), cone(dst));
    let scale = [[d[0] / s[0], 0.0, 0.0], [0.0, d[1] / s[1], 0.0], [0.0, 0.0, d[2] / s[2]]];
    mat3_mul3_f64(&invert(&BRADFORD), &mat3_mul3_f64(&scale, &BRADFORD))
}

fn invert(m: &[[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |r: usize, k: usize| {
        let (r1, r2, k1, k2) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
        m[r1][k1] * m[r2][k2] - m[r1][k2] * m[r2][k1]
    };
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    // Transposed cofactors over the determinant.
    std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det))
}

fn s15_fixed16(v: f64) -> [u8; 4] {
    ((v * 65536.0).round() as i32).to_be_bytes()
}

fn xyz_tag(xyz: [f64; 3]) -> Vec<u8> {
    let mut tag = b"XYZ \0\0\0\0".to_vec();
    for v in xyz {
        tag.extend(s15_fixed16(v));
    }
    tag
}

fn curve_tag(cctf: Cctf) -> Vec<u8> {
    let mut tag = b"curv\0\0\0\0".to_vec();
    match cctf {
        Cctf::Linear => tag.extend(0u32.to_be_bytes()),
        Cctf::Gamma { decode, .. } if (decode * 256.0).fract() == 0.0 => {
            tag.extend(1u32.to_be_bytes());
            tag.extend(((decode * 256.0) as u16).to_be_bytes());
        }
        _ => {
            tag.extend((TABLE_SIZE as u32).to_be_bytes());
            for i in 0..TABLE_SIZE {
                let y = cctf.decode(i as f64 / (TABLE_SIZE - 1) as f64).clamp(0.0, 1.0);
                tag.extend(((y * 65535.0).round() as u16).to_be_bytes());
            }
        }
    }
    tag
}

/// `textDescriptionType`: ASCII only, empty Unicode and ScriptCode parts.
fn desc_tag(text: &str) -> Vec<u8> {
    let mut tag = b"desc\0\0\0\0".to_vec();
    tag.extend((text.len() as u32 + 1).to_be_bytes());
    tag.extend(text.as_bytes());
    tag.push(0);
    tag.extend([0; 4 + 4 + 2 + 1 + 67]);
    tag
}

fn text_tag(text: &str) -> Vec<u8> {
    let mut tag = b"text\0\0\0\0".to_vec();
    tag.extend(text.as_bytes());
    tag.push(0);
    tag
}

#[cfg(test)]
mod tests {
    use super::*;
    use spektrafilm_math::colorspace::{PROPHOTO, SRGB};

    fn tag<'a>(profile: &'a [u8], signature: &[u8; 4]) -> &'a [u8] {
        let count = u32::from_be_bytes(profile[128..132].try_into().unwrap()) as usize;
        let entry = (0..count)
            .map(|i| &profile[132 + 12 * i..144 + 12 * i])
            .find(|e| &e[..4] == signature)
            .unwrap();
        let offset = u32::from_be_bytes(entry[4..8].try_into().unwrap()) as usize;
        let length = u32::from_be_bytes(entry[8..12].try_into().unwrap()) as usize;
        &profile[offset..offset + length]
    }

    fn xyz(tag: &[u8]) -> [f64; 3] {
        std::array::from_fn(|i| {
            i32::from_be_bytes(tag[8 + 4 * i..12 + 4 * i].try_into().unwrap()) as f64 / 65536.0
        })
    }

    #[test]
    fn header_is_well_formed() {
        let p = profile(&SRGB, false);
        assert_eq!(u32::from_be_bytes(p[0..4].try_into().unwrap()) as usize, p.len());
        assert_eq!(&p[36..40], b"acsp");
        assert_eq!(&p[12..24], b"mntrRGB XYZ ");
        assert_eq!(p.len() % 4, 0);
    }

    #[test]
    fn colorants_add_up_to_d50() {
        for space in spektrafilm_math::colorspace::COLOURSPACES.iter().take(6) {
            let p = profile(space, false);
            let sum: [f64; 3] = std::array::from_fn(|i| {
                xyz(tag(&p, b"rXYZ"))[i] + xyz(tag(&p, b"gXYZ"))[i] + xyz(tag(&p, b"bXYZ"))[i]
            });
            for i in 0..3 {
                assert!((sum[i] - PCS_D50[i]).abs() < 2e-4, "{}: {sum:?}", space.name);
            }
        }
    }

    #[test]
    fn srgb_matches_the_published_profile() {
        // The colorants of the IEC 61966-2.1 profile, to its precision.
        let p = profile(&SRGB, false);
        let red = xyz(tag(&p, b"rXYZ"));
        for (a, b) in red.iter().zip([0.4361, 0.2225, 0.0139]) {
            assert!((a - b).abs() < 2e-4, "{red:?}");
        }
    }

    #[test]
    fn curves() {
        let gamma = |p: &[u8]| tag(p, b"rTRC")[8..].to_vec();
        let adobe = spektrafilm_math::colorspace::RgbColourspace::find("Adobe RGB (1998)").unwrap();
        assert_eq!(gamma(&profile(adobe, false)), [0, 0, 0, 1, 2, 51]); // 563/256
        assert_eq!(gamma(&profile(&SRGB, true)), [0, 0, 0, 0]);
        let romm = profile(&PROPHOTO, false);
        let table = &tag(&romm, b"rTRC")[12..];
        assert_eq!(table.len(), 2 * TABLE_SIZE);
        assert_eq!(&table[table.len() - 2..], [0xff, 0xff]);
        // The three curves share one copy.
        assert_eq!(tag(&romm, b"rTRC").as_ptr(), tag(&romm, b"bTRC").as_ptr());
    }
}
