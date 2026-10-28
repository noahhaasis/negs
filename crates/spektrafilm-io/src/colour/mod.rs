//! What an input's pixel values mean, and their conversion to the engine's
//! input representation: **linear light, unclamped floating point, in sRGB
//! (ITU-R BT.709) primaries with a D65 white** — extended-range linear sRGB.
//! Values above 1 and negative coordinates (colours outside the sRGB
//! triangle) are kept; the spectral model takes XYZ from them directly.
//!
//! Camera RAW files are developed by the RAW decoders into this
//! representation (scene-referred). For JPEG, PNG and TIFF the loader decides
//! what the stored values mean ([`InputMode::Auto`]), in this order:
//!
//! 1. PNG `cICP` (which PNG says takes precedence over everything else);
//! 2. an embedded ICC profile that matches the pixels (RGB or grey);
//! 3. PNG `sRGB`, then `gAMA`/`cHRM`;
//! 4. EXIF `ColorSpace` = sRGB, or uncalibrated with the DCF "R03" index
//!    (Adobe RGB);
//! 5. untagged: sRGB for 8/16-bit integer files, linear sRGB for float TIFFs.
//!
//! An unreadable or mismatched profile is skipped with a warning; CMYK and
//! Lab pixel data, and HDR (PQ/HLG) PNGs, are refused rather than guessed.
//! Profiles are evaluated exactly, relative colorimetric: matrix/TRC ones by
//! their curves and colorants, lookup-table ones stage by stage (see
//! [`icc`]); a table is never reduced to the profile's colorant tags. Converting between white points uses Bradford, as
//! ICC does. Decoding a JPEG gives display-referred light: it does not undo
//! the camera's or editor's tone mapping.
//!
//! See `docs/colour-management.md`.

pub mod icc;
pub mod tags;

use anyhow::{Result, bail};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use spektrafilm_math::colorspace::{self, Cctf, PCS_D50, RgbColourspace, SRGB};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f64};

/// How a non-RAW input's pixel values are read. RAW files always use their
/// decoder's development.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputMode {
    /// The file's profile and metadata, with the documented fallbacks.
    #[default]
    Auto,
    /// For untagged or wrongly tagged files: read the values as encoded in
    /// `color_space` (with its transfer function, or as linear light if
    /// `linear`), ignoring any embedded profile and metadata.
    Assume,
    /// The 0.8 loader, unchanged: PNG decoded as sRGB, other formats passed
    /// through for the pipeline's `io.input_color_space` and
    /// `io.input_cctf_decoding`. For scripts and reference tests that set
    /// those fields explicitly.
    Manual,
}

/// `io.input_interpretation`. Every field is always present (also in the
/// apps' saved sessions), so an override survives a merge over defaults.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputInterpretation {
    pub mode: InputMode,
    /// [`InputMode::Assume`]'s colour space, one of [`ASSUMABLE`].
    pub color_space: String,
    /// [`InputMode::Assume`]: the values are linear light.
    pub linear: bool,
}

impl Default for InputInterpretation {
    fn default() -> Self {
        Self { mode: InputMode::Auto, color_space: SRGB.name.into(), linear: false }
    }
}

/// The colour spaces offered as [`InputMode::Assume`] overrides.
pub const ASSUMABLE: [&str; 5] = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB", "ITU-R BT.2020"];

impl InputInterpretation {
    pub fn assume(color_space: &str, linear: bool) -> Self {
        Self { mode: InputMode::Assume, color_space: color_space.into(), linear }
    }

    pub fn manual() -> Self {
        Self { mode: InputMode::Manual, ..Self::default() }
    }

    pub fn validate(&self) -> Result<()> {
        if self.mode == InputMode::Assume && RgbColourspace::find(&self.color_space).is_none() {
            bail!("Unknown input colour space: {}", self.color_space);
        }
        Ok(())
    }
}

/// Where an input's interpretation came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputSource {
    Raw,
    EmbeddedProfile,
    PngCicp,
    PngSrgb,
    PngGamma,
    Exif,
    Untagged,
    Override,
    Manual,
}

/// What the loader read an input as, for the apps to show.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct InputColour {
    pub source: InputSource,
    /// The colour encoding: "Display P3", "Linear sRGB", a profile's name…
    pub encoding: String,
    /// One line for people: "Display P3 (embedded profile)".
    pub summary: String,
    /// What automatic interpretation reads the file as, so an override can
    /// show what it replaces. Equal to `summary` without an override.
    pub automatic: String,
    /// An unreadable or ignored profile, or another caveat.
    pub warning: Option<String>,
    /// Camera data (RAW) rather than an already rendered, display-referred file.
    pub scene_referred: bool,
    /// The pixels are the engine's representation (linear, sRGB primaries).
    /// False only for manual pass-through, where the pipeline's own input
    /// fields interpret them.
    pub linear_srgb: bool,
}

impl InputColour {
    pub fn raw(decoder: &str) -> Self {
        let summary = format!("Camera RAW, developed by {decoder}");
        Self {
            source: InputSource::Raw,
            encoding: "Linear sRGB (scene-referred)".into(),
            automatic: summary.clone(),
            summary,
            warning: None,
            scene_referred: true,
            linear_srgb: true,
        }
    }
}

/// A transfer function: a colour space's, or an ICC curve.
#[derive(Clone, Debug, PartialEq)]
enum Trc {
    Cctf(Cctf),
    Icc(icc::Curve),
}

impl Trc {
    fn decode(&self, x: f64) -> f64 {
        match self {
            Trc::Cctf(c) => {
                // colour's pure gammas are NaN below 0; mirror them instead.
                if x < 0.0 { -c.decode(-x) } else { c.decode(x) }
            }
            Trc::Icc(c) => c.decode(x),
        }
    }
    fn is_identity(&self) -> bool {
        match self {
            Trc::Cctf(c) => *c == Cctf::Linear,
            Trc::Icc(c) => c.is_identity(),
        }
    }
}

/// How to turn the decoded values into the engine's representation.
#[derive(Clone, Debug, PartialEq)]
enum Decode {
    /// Manual mode: the values unchanged.
    Passthrough,
    /// Per-channel tone curves, then a matrix to linear sRGB (`None`: none needed).
    Rgb { curves: [Trc; 3], matrix: Option<[[f64; 3]; 3]> },
    /// A grey tone curve into all three channels (a neutral).
    Gray(Trc),
    /// A lookup-table profile to PCS XYZ, then to linear sRGB.
    Lut(icc::Lut),
}

impl Decode {
    fn named(space: &RgbColourspace, linear: bool) -> Self {
        let trc = Trc::Cctf(if linear { Cctf::Linear } else { space.cctf });
        let matrix = (space.name != SRGB.name).then(|| colorspace::icc_rgb_to_rgb(space, &SRGB));
        Decode::Rgb { curves: [trc.clone(), trc.clone(), trc], matrix }
    }
}

/// PCS XYZ (D50) → linear sRGB, as a colour-managed reader leaves the PCS.
fn pcs_to_srgb() -> [[f64; 3]; 3] {
    colorspace::mat3_mul3_f64(
        &colorspace::mat3_inverse(&SRGB.rgb_to_xyz),
        &colorspace::bradford_matrix(PCS_D50, SRGB.whitepoint_xyz),
    )
}

/// Linear RGB with these xy primaries and white → linear sRGB (Bradford to D65).
fn chromaticities_to_srgb(c: [[f64; 2]; 4]) -> Option<[[f64; 3]; 3]> {
    let xyz = |[x, y]: [f64; 2]| (y > 0.0).then(|| [x / y, 1.0, (1.0 - x - y) / y]);
    let (w, r, g, b) = (xyz(c[0])?, xyz(c[1])?, xyz(c[2])?, xyz(c[3])?);
    let p = [[r[0], g[0], b[0]], [r[1], g[1], b[1]], [r[2], g[2], b[2]]];
    let s = colorspace::mat3_vec_f64(&colorspace::mat3_inverse(&p), w);
    let to_xyz: [[f64; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| p[i][j] * s[j]));
    let m = colorspace::mat3_mul3_f64(
        &colorspace::mat3_inverse(&SRGB.rgb_to_xyz),
        &colorspace::mat3_mul3_f64(&colorspace::bradford_matrix(w, SRGB.whitepoint_xyz), &to_xyz),
    );
    m.iter().flatten().all(|v| v.is_finite()).then_some(m)
}

/// What the loader knows about a decoded file's colour.
#[derive(Clone, Debug, Default)]
pub struct FileColour {
    pub icc: Option<Vec<u8>>,
    pub exif: Option<Vec<u8>>,
    pub png: Option<tags::PngChunks>,
    /// CMYK pixel data (which `image` would convert naively).
    pub cmyk: bool,
    /// Floating-point samples.
    pub float: bool,
    /// One colour channel.
    pub gray: bool,
}

fn space(name: &str) -> &'static RgbColourspace {
    RgbColourspace::find(name).expect("built-in colour space")
}

fn display_name(space: &RgbColourspace, linear: bool) -> String {
    let name = match space.name {
        "Adobe RGB (1998)" => "Adobe RGB",
        "ITU-R BT.2020" => "Rec. 2020",
        other => other,
    };
    if linear { format!("Linear {name}") } else { name.to_string() }
}

struct Decision {
    decode: Decode,
    source: InputSource,
    encoding: String,
    warning: Option<String>,
}

/// PNG `cICP`: the primaries and transfer characteristics Negs can read.
fn cicp(code: [u8; 4]) -> Result<Option<(Decode, String)>, String> {
    let [primaries, transfer, matrix, full_range] = code;
    if matches!(transfer, 16 | 18) {
        return Err("it is an HDR image (PQ or HLG transfer), which Negs doesn't support".into());
    }
    let primaries = match primaries {
        1 => space("sRGB"),
        12 => space("Display P3"),
        9 => space("ITU-R BT.2020"),
        _ => return Ok(None),
    };
    let (trc, curve_name) = match transfer {
        13 => (Cctf::Srgb, "sRGB curve"),
        8 => (Cctf::Linear, "linear"),
        // BT.709/601/2020 share one curve; colour's BT.2020 10-bit constants are BT.709's.
        1 | 6 | 14 | 15 => (Cctf::Bt2020, "BT.709 curve"),
        _ => return Ok(None),
    };
    if matrix != 0 || full_range != 1 {
        return Ok(None);
    }
    let Decode::Rgb { matrix, .. } = Decode::named(primaries, false) else { unreachable!() };
    let t = Trc::Cctf(trc);
    let name = match (primaries.name, trc) {
        (_, Cctf::Linear) => display_name(primaries, true),
        ("sRGB", Cctf::Srgb) => "sRGB".into(),
        ("Display P3", Cctf::Srgb) => "Display P3".into(),
        _ => format!("{} primaries, {curve_name}", display_name(primaries, false)),
    };
    Ok(Some((Decode::Rgb { curves: [t.clone(), t.clone(), t], matrix }, name)))
}

/// An embedded ICC profile, if it is usable for these pixels.
fn embedded(data: &[u8], gray: bool) -> Result<(Decode, String), String> {
    let profile = icc::Profile::parse(data)?;
    let kind = profile.kind()?;
    let matches = match &kind {
        icc::Kind::RgbMatrix { .. } => !gray,
        icc::Kind::GrayTrc(_) => gray,
        icc::Kind::Lut(lut) => lut.inputs == if gray { 1 } else { 3 },
    };
    if !matches {
        let what = String::from_utf8_lossy(&profile.colour_space).trim().to_string();
        return Err(format!("it is a {what} profile, but the image is {}", if gray { "grey" } else { "RGB" }));
    }
    if !matches!(&profile.class, b"mntr" | b"scnr" | b"spac") {
        return Err(format!("it is a {} profile, not an image profile", String::from_utf8_lossy(&profile.class).trim()));
    }
    let name = icc::recognise(&kind)
        .map(|(cs, linear)| display_name(cs, linear))
        .or_else(|| profile.description())
        .unwrap_or_else(|| "Unnamed profile".into());
    let decode = match kind {
        icc::Kind::RgbMatrix { colorants, curves } => {
            let [r, g, b] = curves.map(Trc::Icc);
            Decode::Rgb { curves: [r, g, b], matrix: Some(colorspace::mat3_mul3_f64(&pcs_to_srgb(), &colorants)) }
        }
        icc::Kind::GrayTrc(curve) => Decode::Gray(Trc::Icc(curve)),
        icc::Kind::Lut(lut) => Decode::Lut(lut),
    };
    Ok((decode, name))
}

fn automatic(info: &FileColour) -> Result<Decision> {
    let mut warning = None;
    if let Some(code) = info.png.as_ref().and_then(|p| p.cicp) {
        match cicp(code) {
            Ok(Some((decode, encoding))) => return Ok(Decision { decode, source: InputSource::PngCicp, encoding, warning }),
            Ok(None) => warning = Some(format!("Ignored unsupported cICP colour code {code:?}.")),
            Err(e) => bail!("This PNG can't be read correctly: {e}."),
        }
    }
    if let Some(data) = &info.icc {
        match embedded(data, info.gray) {
            Ok((decode, encoding)) => return Ok(Decision { decode, source: InputSource::EmbeddedProfile, encoding, warning }),
            Err(e) => warning = Some(format!("Ignored the embedded colour profile: {e}.")),
        }
    }
    if let Some(png) = &info.png {
        if png.srgb {
            return Ok(Decision { decode: Decode::named(&SRGB, false), source: InputSource::PngSrgb, encoding: "sRGB".into(), warning });
        }
        if png.gamma.is_some() || png.chromaticities.is_some() {
            let matrix = match png.chromaticities {
                Some(c) => chromaticities_to_srgb(c),
                None => None,
            };
            let curve = match png.gamma {
                Some(g) => Trc::Icc(icc::Curve::Gamma(1.0 / g)),
                None => Trc::Cctf(Cctf::Srgb),
            };
            let encoding = match png.gamma {
                Some(g) => format!("Gamma {:.2}{}", 1.0 / g, if matrix.is_some() { ", stated primaries" } else { ", sRGB primaries" }),
                None => "sRGB curve, stated primaries".into(),
            };
            if png.chromaticities.is_some() && matrix.is_none() {
                warning = Some("Ignored invalid cHRM chromaticities.".into());
            }
            let decode = if info.gray { Decode::Gray(curve) } else { Decode::Rgb { curves: [curve.clone(), curve.clone(), curve], matrix } };
            return Ok(Decision { decode, source: InputSource::PngGamma, encoding, warning });
        }
    }
    if let Some(colour) = info.exif.as_deref().and_then(tags::exif_colour) {
        let cs = match colour {
            tags::ExifColour::Srgb => &SRGB,
            tags::ExifColour::AdobeRgb => space("Adobe RGB (1998)"),
        };
        return Ok(Decision { decode: Decode::named(cs, false), source: InputSource::Exif, encoding: display_name(cs, false), warning });
    }
    let linear = info.float;
    Ok(Decision { decode: Decode::named(&SRGB, linear), source: InputSource::Untagged, encoding: display_name(&SRGB, linear), warning })
}

fn summary(source: InputSource, encoding: &str) -> String {
    match source {
        InputSource::Raw => encoding.to_string(),
        InputSource::EmbeddedProfile => format!("{encoding} (embedded profile)"),
        InputSource::PngCicp => format!("{encoding} (PNG cICP)"),
        InputSource::PngSrgb => format!("{encoding} (PNG sRGB chunk)"),
        InputSource::PngGamma => format!("{encoding} (PNG gAMA/cHRM)"),
        InputSource::Exif => format!("{encoding} (EXIF)"),
        InputSource::Untagged => format!("{encoding} (untagged, assumed)"),
        InputSource::Override => format!("{encoding} (override)"),
        InputSource::Manual => format!("{encoding} (manual)"),
    }
}

/// Decides how to read a decoded non-RAW file and converts it to the
/// engine's representation (or passes it through in manual mode).
pub fn convert(image: image::DynamicImage, info: &FileColour, interpretation: &InputInterpretation, png: bool) -> Result<(ImageBuf, InputColour)> {
    interpretation.validate()?;
    if info.cmyk {
        bail!("CMYK images aren't supported. Convert the image to RGB with its profile in an image editor first.");
    }
    let auto = automatic(info)?;
    let automatic_summary = summary(auto.source, &auto.encoding);
    let decision = match interpretation.mode {
        InputMode::Auto => auto,
        InputMode::Assume => {
            let (cs, linear) = (space(&interpretation.color_space), interpretation.linear);
            let decode = Decode::named(cs, linear);
            let decode = match (info.gray, decode) {
                (true, Decode::Rgb { curves: [c, ..], .. }) => Decode::Gray(c),
                (_, d) => d,
            };
            Decision { decode, source: InputSource::Override, encoding: display_name(cs, linear), warning: None }
        }
        InputMode::Manual if png => Decision {
            decode: Decode::named(&SRGB, false),
            source: InputSource::Manual,
            encoding: "sRGB (0.8 PNG loader)".into(),
            warning: None,
        },
        InputMode::Manual => Decision {
            decode: Decode::Passthrough,
            source: InputSource::Manual,
            encoding: "Values as stored, for io.input_color_space".into(),
            warning: None,
        },
    };
    let linear_srgb = decision.decode != Decode::Passthrough;
    let mut image = apply(image, &decision.decode)?;
    // Float files can hold NaN or infinity, which no backend can render
    // consistently (GPU shaders may assume finite maths).
    let non_finite: usize = image
        .data
        .par_iter_mut()
        .map(|v| if v.is_finite() { 0 } else { *v = from_f64(0.0); 1 })
        .sum();
    let mut warning = decision.warning;
    if non_finite > 0 {
        let note = format!("{non_finite} samples weren't finite numbers and were set to 0.");
        warning = Some(warning.map_or(note.clone(), |w| format!("{w} {note}")));
    }
    let summary = summary(decision.source, &decision.encoding);
    Ok((image, InputColour {
        source: decision.source,
        encoding: decision.encoding,
        automatic: automatic_summary,
        summary,
        warning,
        scene_referred: false,
        linear_srgb,
    }))
}

/// The largest integer sample value, or `None` for float data.
fn integer_max(image: &image::DynamicImage) -> Option<u32> {
    use image::DynamicImage::*;
    match image {
        ImageLuma8(_) | ImageLumaA8(_) | ImageRgb8(_) | ImageRgba8(_) => Some(255),
        ImageLuma16(_) | ImageLumaA16(_) | ImageRgb16(_) | ImageRgba16(_) => Some(65535),
        _ => None,
    }
}

/// A tone curve as a function of the decoded [0, 1] sample: tabulated per
/// integer code for integer data, evaluated directly for float data.
fn curve_fn(trc: &Trc, max: Option<u32>) -> Box<dyn Fn(f32) -> f64 + Send + Sync> {
    if trc.is_identity() {
        return Box::new(f64::from);
    }
    match max {
        Some(max) => {
            let table: Vec<f64> = (0..=max).map(|i| trc.decode(i as f64 / max as f64)).collect();
            let scale = max as f32;
            Box::new(move |v| table[((v * scale).round() as usize).min(table.len() - 1)])
        }
        None => {
            let trc = trc.clone();
            Box::new(move |v| trc.decode(f64::from(v)))
        }
    }
}

fn apply(image: image::DynamicImage, decode: &Decode) -> Result<ImageBuf> {
    let (w, h) = (image.width(), image.height());
    let max = integer_max(&image);
    let out = match decode {
        Decode::Passthrough => image.to_rgb32f().into_raw().into_par_iter().map(|v| from_f64(f64::from(v))).collect(),
        Decode::Rgb { curves, matrix } => {
            let rgb = image.to_rgb32f().into_raw();
            drop(image);
            let f = [curve_fn(&curves[0], max), curve_fn(&curves[1], max), curve_fn(&curves[2], max)];
            let mut out: Vec<Scalar> = vec![from_f64(0.0); rgb.len()];
            out.par_chunks_exact_mut(3).zip(rgb.par_chunks_exact(3)).for_each(|(o, p)| {
                let lin = [f[0](p[0]), f[1](p[1]), f[2](p[2])];
                let v = matrix.as_ref().map_or(lin, |m| colorspace::mat3_vec_f64(m, lin));
                o.copy_from_slice(&v.map(from_f64));
            });
            out
        }
        Decode::Gray(trc) => {
            let luma = image.to_luma32f().into_raw();
            let f = curve_fn(trc, max);
            let mut out: Vec<Scalar> = vec![from_f64(0.0); luma.len() * 3];
            out.par_chunks_exact_mut(3).zip(luma.par_iter()).for_each(|(o, &v)| o.fill(from_f64(f(v))));
            out
        }
        Decode::Lut(lut) => {
            let gray = lut.inputs == 1;
            let src = if gray { image.to_luma32f().into_raw() } else { image.to_rgb32f().into_raw() };
            drop(image);
            let to_srgb = pcs_to_srgb();
            let mut out: Vec<Scalar> = vec![from_f64(0.0); w as usize * h as usize * 3];
            out.par_chunks_exact_mut(3).zip(src.par_chunks_exact(lut.inputs)).for_each(|(o, p)| {
                let device: Vec<f64> = p.iter().map(|&v| f64::from(v)).collect();
                let v = colorspace::mat3_vec_f64(&to_srgb, lut.eval(&device));
                o.copy_from_slice(&v.map(from_f64));
            });
            out
        }
    };
    Ok(ImageBuf::from_data(w, h, out))
}

#[cfg(test)]
mod tests;
