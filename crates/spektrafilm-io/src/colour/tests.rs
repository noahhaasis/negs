//! Input interpretation: decisions, precedence and conversions, against
//! LittleCMS 2.19 and colour-science 0.4.7 references (see
//! `scripts/gen_colour_input_fixtures.py`).

use super::*;
use image::{DynamicImage, ImageBuffer, Rgb};
use spektrafilm_math::precision::to_f64;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/colour").join(name)
}

fn references() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixture("references.json")).unwrap()).unwrap()
}

fn rgb16(pixels: &[[u16; 3]]) -> DynamicImage {
    let raw: Vec<u16> = pixels.iter().flatten().copied().collect();
    DynamicImage::ImageRgb16(ImageBuffer::<Rgb<u16>, _>::from_raw(pixels.len() as u32, 1, raw).unwrap())
}

fn rgbf(pixels: &[[f32; 3]]) -> DynamicImage {
    let raw: Vec<f32> = pixels.iter().flatten().copied().collect();
    DynamicImage::ImageRgb32F(ImageBuffer::<Rgb<f32>, _>::from_raw(pixels.len() as u32, 1, raw).unwrap())
}

fn pixels(img: &ImageBuf) -> Vec<[f64; 3]> {
    img.data.chunks_exact(3).map(|p| [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]).collect()
}

fn auto(img: DynamicImage, info: &FileColour) -> (Vec<[f64; 3]>, InputColour) {
    let (out, colour) = convert(img, info, &InputInterpretation::default(), false).unwrap();
    (pixels(&out), colour)
}

fn assume(name: &str, linear: bool) -> InputInterpretation {
    InputInterpretation::assume(name, linear)
}

fn assert_close(got: &[[f64; 3]], want: &[[f64; 3]], tol: f64, what: &str) {
    assert_eq!(got.len(), want.len());
    for (i, (g, w)) in got.iter().zip(want).enumerate() {
        for c in 0..3 {
            assert!((g[c] - w[c]).abs() <= tol, "{what}: pixel {i} channel {c}: {g:?} vs {w:?} (tol {tol})");
        }
    }
}

/// Like [`assert_close`], in XYZ (D65): the published sRGB matrix (IEC
/// 61966-2-1, as colour-science has it) maps (1, 1, 1) to (0.9505, 1, 1.0890),
/// not exactly to D65, so another space's white is (0.99984, 1.0001, 1.00007)
/// in sRGB coordinates, but the same XYZ.
fn assert_xyz_close(got: &[[f64; 3]], want: &[[f64; 3]], tol: f64, what: &str) {
    let xyz = |v: &[[f64; 3]]| v.iter().map(|p| colorspace::mat3_vec_f64(&SRGB.rgb_to_xyz, *p)).collect::<Vec<_>>();
    assert_close(&xyz(got), &xyz(want), tol, what);
}

/// Linear sRGB → PCS XYZ (D50), relative colorimetric: what LittleCMS reports.
fn to_pcs(rgb: [f64; 3]) -> [f64; 3] {
    colorspace::mat3_vec_f64(&colorspace::mat3_inverse(&pcs_to_srgb()), rgb)
}

fn profile_info(name: &str) -> FileColour {
    FileColour { icc: Some(std::fs::read(fixture(name)).unwrap()), ..Default::default() }
}

fn reference_rgb(refs: &serde_json::Value, key: &str) -> (Vec<[u16; 3]>, Vec<[f64; 3]>) {
    let rgb = serde_json::from_value(refs[key]["rgb16"].clone()).unwrap();
    let xyz = serde_json::from_value(refs[key]["xyz_d50"].clone()).unwrap();
    (rgb, xyz)
}

// Both evaluate the profile's own tables (tetrahedral CLUT interpolation in
// floating point); they differ in how the 256-entry 16-bit input curves
// are interpolated, which shows in deep shadows (up to 2e-5 absolute,
// where this evaluator is the closer of the two to the generating
// function). transicc prints XYZ to 1e-6. 3e-5 in XYZ is < 0.02 ΔE00.
const LUT_TOLERANCE: f64 = 3e-5;

#[test]
fn lut_profile_matches_littlecms_and_ignores_its_colorants() {
    let refs = references();
    let (rgb, want) = reference_rgb(&refs, "lut_rgb");
    let (got, colour) = auto(rgb16(&rgb), &profile_info("lut_rgb.icc"));
    assert_eq!(colour.source, InputSource::EmbeddedProfile, "{:?}", colour.warning);
    assert_eq!(colour.encoding, "Negs test LUT profile");
    assert!(colour.warning.is_none());
    let got: Vec<[f64; 3]> = got.into_iter().map(to_pcs).collect();
    assert_close(&got, &want, LUT_TOLERANCE, "LUT profile vs LittleCMS");
    // The decoy colorants (linear BT.2020) would put mid-grey's code
    // 32768 at Y = 0.5, not sRGB-decoded 0.2140.
    assert!((got[2][1] - 0.214).abs() < 1e-3, "{:?}", got[2]);
}

#[test]
fn every_lut_encoding_matches_littlecms() {
    // lut16 with legacy 16-bit Lab, lutAtoB with A curves/CLUT/B curves and
    // v4 Lab, and lutAtoB with M curves/matrix+offset/B curves and XYZ.
    let refs = references();
    for (key, name) in [
        ("lut_lab16", "Negs test lut16 Lab profile"),
        ("mab_lab", "Negs test lutAtoB Lab profile"),
        ("mab_xyz", "Negs test lutAtoB matrix profile"),
    ] {
        let (rgb, want) = reference_rgb(&refs, key);
        let (got, colour) = auto(rgb16(&rgb), &profile_info(&format!("{key}.icc")));
        assert_eq!((colour.source, colour.encoding.as_str()), (InputSource::EmbeddedProfile, name), "{:?}", colour.warning);
        let got: Vec<[f64; 3]> = got.into_iter().map(to_pcs).collect();
        assert_close(&got, &want, LUT_TOLERANCE, key);
    }
}

#[test]
fn independent_matrix_profile_matches_littlecms() {
    // LittleCMS's own v4 sRGB: parametric curves, chad-adapted colorants.
    // Exact evaluation on both sides: only f32 output rounding remains.
    let refs = references();
    let (rgb, want) = reference_rgb(&refs, "lcms_srgb");
    let (got, colour) = auto(rgb16(&rgb), &profile_info("lcms_srgb.icc"));
    assert_eq!((colour.source, colour.encoding.as_str()), (InputSource::EmbeddedProfile, "sRGB"));
    let got: Vec<[f64; 3]> = got.into_iter().map(to_pcs).collect();
    assert_close(&got, &want, 2e-5, "LittleCMS sRGB profile");
}

#[test]
fn grey_profile_matches_littlecms() {
    let refs = references();
    let grey: Vec<[u16; 1]> = serde_json::from_value(refs["gray_gamma18"]["gray16"].clone()).unwrap();
    let want: Vec<[f64; 3]> = serde_json::from_value(refs["gray_gamma18"]["xyz_d50"].clone()).unwrap();
    let raw: Vec<u16> = grey.iter().map(|g| g[0]).collect();
    let img = DynamicImage::ImageLuma16(ImageBuffer::from_raw(raw.len() as u32, 1, raw).unwrap());
    let info = FileColour { gray: true, ..profile_info("gray_gamma18.icc") };
    let (got, colour) = auto(img, &info);
    assert_eq!(colour.source, InputSource::EmbeddedProfile);
    for (g, w) in got.iter().zip(&want) {
        // A neutral: equal channels, and the reference's luminance.
        assert!(g[0] == g[1] && g[1] == g[2], "{g:?}");
        assert!((to_pcs(*g)[1] - w[1]).abs() < 2e-5, "{g:?} vs {w:?}");
    }
}

/// In-gamut linear sRGB test colours: neutrals, shadows, a highlight,
/// saturated (but sRGB-representable, so every space can hold them) and
/// mixed colours.
const COLOURS: [[f64; 3]; 9] = [
    [0.0, 0.0, 0.0],
    [0.002, 0.002, 0.002],
    [0.18, 0.18, 0.18],
    [1.0, 1.0, 1.0],
    [0.9, 0.05, 0.02],
    [0.03, 0.6, 0.1],
    [0.01, 0.02, 0.8],
    [0.004, 0.0015, 0.0008],
    [0.95, 0.85, 0.4],
];

/// `COLOURS` encoded in `space` (with its transfer function unless
/// `linear`), at 16 bits.
fn encoded(space: &RgbColourspace, linear: bool) -> Vec<[u16; 3]> {
    let m = colorspace::icc_rgb_to_rgb(&SRGB, space);
    COLOURS
        .iter()
        .map(|&c| {
            colorspace::mat3_vec_f64(&m, c).map(|v| {
                let e = if linear { v } else { space.cctf.encode(v.max(0.0)) };
                (e.clamp(0.0, 1.0) * 65535.0).round() as u16
            })
        })
        .collect()
}

// 16-bit quantisation (half a code is 7.6e-6 encoded, up to ~6e-5 linear
// near white for the steepest curves), the profile's 1024-entry u16 curve
// table and s15Fixed16 colorants, and the white definitions themselves:
// D50-based spaces come back to D65 by Bradford at xy-derived D65 (Z
// 1.08883), while the published sRGB matrix's white has Z 1.0890. 2.5e-4
// in XYZ is under 0.05 ΔE00 anywhere in these ramps.
const PROFILE_TOLERANCE: f64 = 2.5e-4;

#[test]
fn the_same_colours_in_any_tagged_space_load_alike() {
    let dir = std::env::temp_dir().join(format!("negs-colour-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    for name in ASSUMABLE {
        let cs = space(name);
        for linear in [false, true] {
            let codes = encoded(cs, linear);
            let buf = ImageBuf::from_data(
                codes.len() as u32,
                1,
                codes.iter().flatten().map(|&v| from_f64(v as f64 / 65535.0)).collect(),
            );
            for ext in ["png", "tiff"] {
                let path = dir.join(format!("{}-{linear}.{ext}", name.replace(' ', "_")));
                let options = crate::save::SaveOptions {
                    bit_depth: Some(16),
                    color_space: name.into(),
                    linear,
                    ..Default::default()
                };
                crate::save::save_image_with_options(&buf, &path, None, &options).unwrap();
                let loaded = crate::load_image_colour(&path, crate::Formats::Any, Default::default(), Default::default(), &InputInterpretation::default()).unwrap();
                assert_eq!(loaded.colour.source, InputSource::EmbeddedProfile, "{name} {ext}");
                assert_eq!(loaded.colour.encoding, display_name(cs, linear), "{name} {ext}");
                let what = format!("{name} linear={linear} {ext}");
                assert_xyz_close(&pixels(&loaded.image), &COLOURS, PROFILE_TOLERANCE, &what);

                // The same values, untagged, with the matching override:
                // exact named-space decoding (no curve table), so it agrees
                // with the profile path more closely than with `COLOURS`.
                let (out, colour) = convert(rgb16(&codes), &FileColour::default(), &assume(name, linear), false).unwrap();
                assert_eq!(colour.source, InputSource::Override);
                assert_xyz_close(&pixels(&out), &COLOURS, PROFILE_TOLERANCE, &format!("{what} override"));
                assert_xyz_close(&pixels(&out), &pixels(&loaded.image), 1e-4, &format!("{what} override vs profile"));
            }
        }
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn named_space_decoding_matches_colour_science() {
    // colour.RGB_to_RGB(v, space, "sRGB", "Bradford",
    //     apply_cctf_decoding=True, apply_cctf_encoding=False), colour 0.4.7.
    // Results outside [0, 1] are colours outside sRGB, kept as such.
    let cases: [(&str, [f64; 3], [f64; 3]); 4] = [
        ("ProPhoto RGB", [0.2, 0.5, 0.8], [-0.30201875, 0.33917549, 0.73284287]),
        ("Display P3", [0.9, 0.1, 0.2], [0.96216842, -0.02257071, 0.02013114]),
        ("Adobe RGB (1998)", [0.3, 0.6, 0.1], [-0.03051079, 0.32515892, -0.00736329]),
        ("ITU-R BT.2020", [0.5, 0.4, 0.3], [0.32168615, 0.16279609, 0.09563454]),
    ];
    for (name, input, want) in cases {
        let img = rgbf(&[input.map(|v| v as f32)]);
        // Float data is evaluated directly. colour derives ProPhoto's white
        // from its xy (0.3457, 0.3585); Negs uses the ICC D50 (0.9642, 1,
        // 0.8249), as ROMM RGB specifies: 2e-5 apart after adaptation.
        let (out, _) = convert(img, &FileColour { float: true, ..Default::default() }, &assume(name, false), false).unwrap();
        assert_close(&pixels(&out), &[want], 5e-5, name);
    }
}

#[test]
fn precedence_follows_the_documented_order() {
    let srgb_icc = crate::icc::profile(&SRGB, false);
    let p3_icc = crate::icc::profile(space("Display P3"), false);
    let png = |f: fn(&mut tags::PngChunks)| {
        let mut c = tags::PngChunks::default();
        f(&mut c);
        Some(c)
    };
    let adobe_exif = tags::tests::exif_block(0xFFFF, Some(b"R03\0"), true);
    let srgb_exif = tags::tests::exif_block(1, None, false);
    let cases: Vec<(&str, FileColour, InputSource, &str)> = vec![
        (
            "cICP beats the profile",
            FileColour { icc: Some(srgb_icc.clone()), png: png(|c| c.cicp = Some([12, 13, 0, 1])), ..Default::default() },
            InputSource::PngCicp,
            "Display P3",
        ),
        (
            "profile beats sRGB chunk and EXIF",
            FileColour { icc: Some(p3_icc.clone()), png: png(|c| c.srgb = true), exif: Some(srgb_exif.clone()), ..Default::default() },
            InputSource::EmbeddedProfile,
            "Display P3",
        ),
        (
            "sRGB chunk beats gAMA",
            FileColour { png: png(|c| { c.srgb = true; c.gamma = Some(1.0) }), ..Default::default() },
            InputSource::PngSrgb,
            "sRGB",
        ),
        (
            "gAMA alone",
            FileColour { png: png(|c| c.gamma = Some(1.0 / 2.2)), ..Default::default() },
            InputSource::PngGamma,
            "Gamma 2.20, sRGB primaries",
        ),
        ("EXIF Adobe RGB (R03)", FileColour { exif: Some(adobe_exif.clone()), ..Default::default() }, InputSource::Exif, "Adobe RGB"),
        ("EXIF sRGB", FileColour { exif: Some(srgb_exif.clone()), ..Default::default() }, InputSource::Exif, "sRGB"),
        ("untagged integer", FileColour::default(), InputSource::Untagged, "sRGB"),
        ("untagged float", FileColour { float: true, ..Default::default() }, InputSource::Untagged, "Linear sRGB"),
        (
            "unsupported cICP falls through",
            FileColour { png: png(|c| { c.cicp = Some([5, 13, 0, 1]); c.srgb = true }), ..Default::default() },
            InputSource::PngSrgb,
            "sRGB",
        ),
    ];
    for (what, info, source, encoding) in cases {
        let img = if info.float { rgbf(&[[0.5, 0.5, 0.5]]) } else { rgb16(&[[32768, 32768, 32768]]) };
        let (_, colour) = auto(img, &info);
        assert_eq!((colour.source, colour.encoding.as_str()), (source, encoding), "{what}");
        assert_eq!(colour.summary, colour.automatic, "{what}");
        assert!(colour.linear_srgb && !colour.scene_referred, "{what}");
    }
}

#[test]
fn each_source_decodes_its_own_way() {
    let codes = encoded(space("Display P3"), false);
    let p3 = |info: FileColour| auto(rgb16(&codes), &info).0;
    let reference = pixels(&convert(rgb16(&codes), &FileColour::default(), &assume("Display P3", false), false).unwrap().0);
    // cICP P3 / sRGB transfer and an embedded P3 profile read the codes alike.
    let cicp = FileColour { png: Some(tags::PngChunks { cicp: Some([12, 13, 0, 1]), ..Default::default() }), ..Default::default() };
    assert_close(&p3(cicp), &reference, 1e-6, "cICP");
    let icc = FileColour { icc: Some(crate::icc::profile(space("Display P3"), false)), ..Default::default() };
    assert_close(&p3(icc), &reference, PROFILE_TOLERANCE, "profile");

    // gAMA 1.0 + cHRM with BT.2020's chromaticities = linear BT.2020.
    let codes = encoded(space("ITU-R BT.2020"), true);
    let chrm = [[0.3127, 0.3290], [0.708, 0.292], [0.170, 0.797], [0.131, 0.046]];
    let info = FileColour {
        png: Some(tags::PngChunks { gamma: Some(1.0), chromaticities: Some(chrm), ..Default::default() }),
        ..Default::default()
    };
    let (got, colour) = auto(rgb16(&codes), &info);
    assert_eq!(colour.encoding, "Gamma 1.00, stated primaries");
    assert_xyz_close(&got, &COLOURS, 1e-4, "gAMA/cHRM");

    // EXIF R03: Adobe RGB values.
    let codes = encoded(space("Adobe RGB (1998)"), false);
    let info = FileColour { exif: Some(tags::tests::exif_block(0xFFFF, Some(b"R03\0"), false)), ..Default::default() };
    assert_xyz_close(&auto(rgb16(&codes), &info).0, &COLOURS, 1e-4, "EXIF Adobe RGB");
}

#[test]
fn unusable_profiles_warn_and_fall_through() {
    let grey = std::fs::read(fixture("gray_gamma18.icc")).unwrap();
    let cases = [
        (grey, "it is a GRAY profile, but the image is RGB"),
        (b"not a profile".to_vec(), "Ignored the embedded colour profile"),
        (
            icc::tests::build(b"RGB ", b"XYZ ", &[]),
            "Ignored the embedded colour profile",
        ),
    ];
    let exif = tags::tests::exif_block(0xFFFF, Some(b"R03\0"), true);
    for (data, message) in cases {
        let info = FileColour { icc: Some(data), exif: Some(exif.clone()), ..Default::default() };
        let (_, colour) = auto(rgb16(&[[1000, 2000, 3000]]), &info);
        assert_eq!(colour.source, InputSource::Exif);
        let warning = colour.warning.expect("a warning");
        assert!(warning.contains(message), "{warning}");
    }
}

#[test]
fn unsupported_colour_models_are_refused() {
    let cmyk = FileColour { cmyk: true, ..Default::default() };
    let err = convert(rgb16(&[[0, 0, 0]]), &cmyk, &InputInterpretation::default(), false).unwrap_err();
    assert!(err.to_string().contains("CMYK"), "{err}");
    // Not even an override reads CMYK as RGB.
    assert!(convert(rgb16(&[[0, 0, 0]]), &cmyk, &assume("sRGB", false), false).is_err());

    for transfer in [16, 18] {
        let hdr = FileColour { png: Some(tags::PngChunks { cicp: Some([9, transfer, 0, 1]), ..Default::default() }), ..Default::default() };
        let err = convert(rgb16(&[[0, 0, 0]]), &hdr, &InputInterpretation::default(), true).unwrap_err();
        assert!(err.to_string().contains("HDR"), "{err}");
    }

    let lab = icc::tests::build(b"Lab ", b"Lab ", &[]);
    let info = FileColour { icc: Some(lab), ..Default::default() };
    let (_, colour) = auto(rgb16(&[[0, 0, 0]]), &info);
    assert_eq!(colour.source, InputSource::Untagged);
    assert!(colour.warning.is_some());

    assert!(assume("Not a space", false).validate().is_err());

    // The serialized form keeps every field, and missing ones default.
    let json = serde_json::to_value(assume("Display P3", true)).unwrap();
    assert_eq!(json, serde_json::json!({"mode": "assume", "color_space": "Display P3", "linear": true}));
    let partial: InputInterpretation = serde_json::from_value(serde_json::json!({"mode": "manual"})).unwrap();
    assert_eq!(partial, InputInterpretation::manual());
}

#[test]
fn overrides_ignore_the_file_and_report_what_they_replace() {
    let info = FileColour { icc: Some(crate::icc::profile(space("ProPhoto RGB"), false)), ..Default::default() };
    let codes = encoded(space("Display P3"), false);
    let (out, colour) = convert(rgb16(&codes), &info, &assume("Display P3", false), false).unwrap();
    assert_eq!(colour.source, InputSource::Override);
    assert_eq!(colour.summary, "Display P3 (override)");
    assert_eq!(colour.automatic, "ProPhoto RGB (embedded profile)");
    assert_xyz_close(&pixels(&out), &COLOURS, 1e-4, "override");

    // A grey image stays neutral under an RGB override.
    let img = DynamicImage::ImageLuma16(ImageBuffer::from_raw(2, 1, vec![1000u16, 40000]).unwrap());
    let info = FileColour { gray: true, ..Default::default() };
    let (out, _) = convert(img, &info, &assume("ProPhoto RGB", false), false).unwrap();
    for p in pixels(&out) {
        assert!(p[0] == p[1] && p[1] == p[2], "{p:?}");
    }
}

#[test]
fn manual_mode_keeps_the_08_loader() {
    let img = || rgb16(&[[0, 32768, 65535]]);
    let info = FileColour { icc: Some(crate::icc::profile(space("ProPhoto RGB"), false)), ..Default::default() };
    // PNG: decoded as sRGB whatever it is tagged.
    let (png, colour) = convert(img(), &info, &InputInterpretation::manual(), true).unwrap();
    assert!(colour.linear_srgb);
    let want = [[0.0, spektrafilm_math::colorspace::Cctf::Srgb.decode(32768.0 / 65535.0), 1.0]];
    assert_close(&pixels(&png), &want, 1e-6, "manual PNG");
    // Others: the stored values, for the pipeline's input fields.
    let (tiff, colour) = convert(img(), &info, &InputInterpretation::manual(), false).unwrap();
    assert!(!colour.linear_srgb);
    assert_eq!(colour.source, InputSource::Manual);
    assert_close(&pixels(&tiff), &[[0.0, 32768.0 / 65535.0, 1.0]], 1e-6, "manual TIFF");
}

#[test]
fn float_values_outside_the_unit_range_are_kept() {
    let input = [[-0.25f32, 0.5, 4.0], [1.5, -0.01, 0.0], [12.0, 12.0, 12.0]];
    let info = FileColour { float: true, ..Default::default() };
    let (out, colour) = convert(rgbf(&input), &info, &InputInterpretation::default(), false).unwrap();
    assert_eq!(colour.encoding, "Linear sRGB");
    let want: Vec<[f64; 3]> = input.iter().map(|p| p.map(f64::from)).collect();
    assert_close(&pixels(&out), &want, 0.0, "untagged float");

    // Linear P3 above 1: the matrix keeps both the excess and the
    // negative sRGB coordinates of P3's saturated green.
    let (out, _) = convert(rgbf(&[[0.0, 2.0, 0.0]]), &info, &assume("Display P3", true), false).unwrap();
    let m = colorspace::icc_rgb_to_rgb(space("Display P3"), &SRGB);
    let want = colorspace::mat3_vec_f64(&m, [0.0, 2.0, 0.0]);
    assert!(want[0] < 0.0 && want[1] > 2.0);
    assert_close(&pixels(&out), &[want], 1e-6, "linear P3");

    // Non-finite samples become 0, with a warning.
    let (out, colour) = convert(rgbf(&[[f32::NAN, 0.5, f32::INFINITY]]), &info, &InputInterpretation::default(), false).unwrap();
    assert_eq!(pixels(&out)[0], [0.0, 0.5, 0.0]);
    assert!(colour.warning.unwrap().contains("2 samples"));

    // Transfer functions mirror below zero instead of producing NaN.
    let (out, _) = convert(rgbf(&[[-0.5, 0.5, 1.25]]), &info, &assume("Adobe RGB (1998)", false), false).unwrap();
    assert!(pixels(&out)[0].iter().all(|v| v.is_finite()));
}

#[test]
fn loader_reads_png_profiles_and_refuses_cmyk_jpeg() {
    use image::ImageEncoder;
    let dir = std::env::temp_dir().join(format!("negs-colour-load-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    // A 16-bit PNG carrying the LUT profile in iCCP.
    let refs = references();
    let (rgb, want) = reference_rgb(&refs, "lut_rgb");
    let path = dir.join("lut.png");
    let mut encoder = image::codecs::png::PngEncoder::new(std::fs::File::create(&path).unwrap());
    encoder.set_icc_profile(std::fs::read(fixture("lut_rgb.icc")).unwrap()).unwrap();
    let bytes: Vec<u8> = rgb.iter().flatten().flat_map(|v| v.to_ne_bytes()).collect();
    encoder.write_image(&bytes, rgb.len() as u32, 1, image::ExtendedColorType::Rgb16).unwrap();
    let loaded = crate::load_image_colour(&path, crate::Formats::Any, Default::default(), Default::default(), &InputInterpretation::default()).unwrap();
    assert_eq!(loaded.colour.summary, "Negs test LUT profile (embedded profile)");
    let got: Vec<[f64; 3]> = pixels(&loaded.image).into_iter().map(to_pcs).collect();
    assert_close(&got, &want, LUT_TOLERANCE, "PNG iCCP LUT profile");

    let err = crate::load_image_colour(&fixture("cmyk.jpg"), crate::Formats::Any, Default::default(), Default::default(), &InputInterpretation::default())
        .err()
        .expect("CMYK refused");
    assert!(err.to_string().contains("CMYK"), "{err}");
    std::fs::remove_dir_all(&dir).ok();
}
