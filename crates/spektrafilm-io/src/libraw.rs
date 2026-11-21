//! LibRaw as rawler's fallback, for a RAW rawler can't decode. LibRaw only
//! unpacks the sensor; the result is a `rawler::RawImage` for the same develop
//! as every other RAW (demosaic, white balance, calibrate). Files rawler
//! decodes never get here, so their renders are unchanged.
//!
//! Where rawler knows the camera but not the compression (Nikon HE/HE*), its
//! metadata is kept (white and black levels, crop, matrices, white balance)
//! and only the pixels are LibRaw's, so the file develops as the camera's
//! other RAWs do. For a camera rawler doesn't know, LibRaw's metadata is used.

use std::collections::HashMap;

use rawler::RawImage;
use rawler::cfa::{CFA, PlaneColor};
use rawler::decoders::{Camera, Orientation, RawDecodeParams};
use rawler::imgop::xyz::Illuminant;
use rawler::imgop::{Dim2, Point, Rect};
use rawler::rawimage::{BlackLevel, CFAConfig, RawImageData, RawPhotometricInterpretation, WhiteLevel};
use rawler::rawsource::RawSource;

use negs_libraw::{Area, Mosaic};

/// The name the app shows for files LibRaw unpacked.
pub(crate) const NAME: &str = "LibRaw";
/// How [`Unpacked::version`] begins.
pub(crate) const VERSION_PREFIX: &str = "LibRaw ";

/// Whose metadata develops LibRaw's pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Metadata {
    /// rawler's where it reads the file's metadata, else LibRaw's.
    Best,
    /// LibRaw's, for comparing the two.
    LibRaw,
}

/// A RAW unpacked by LibRaw, as rawler would have decoded it.
pub(crate) struct Unpacked {
    pub raw: RawImage,
    /// The EXIF orientation LibRaw read, when rawler's metadata wasn't used
    /// (rawler may not read the file at all).
    pub orientation: Option<u16>,
    pub version: String,
}

pub(crate) fn unpack(src: &RawSource, metadata: Metadata) -> Result<Unpacked, negs_libraw::Error> {
    let version = format!("{VERSION_PREFIX}{}", negs_libraw::version());
    if metadata == Metadata::Best
        && let Some(mut raw) = rawler_metadata(src)
    {
        let m = negs_libraw::unpack(src.buf(), Area::Whole)?;
        if fits(&raw, &m) {
            raw.data = RawImageData::Integer(m.data);
            black_from_masked_areas(&mut raw);
            return Ok(Unpacked { raw, orientation: None, version });
        }
    }
    let m = negs_libraw::unpack(src.buf(), Area::Visible)?;
    let orientation = Some(m.orientation);
    Ok(Unpacked { raw: from_libraw(m), orientation, version })
}

/// rawler's metadata-only decode (no sensor data), for a Bayer or X-Trans
/// sensor.
fn rawler_metadata(src: &RawSource) -> Option<RawImage> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rawler::get_decoder(src).and_then(|d| d.raw_image(src, &RawDecodeParams::default(), true)).ok()
    }))
    .ok()
    .flatten()
    .filter(|raw| raw.cpp == 1 && matches!(raw.photometric, RawPhotometricInterpretation::Cfa(_)))
}

/// Whether LibRaw's whole sensor is the one rawler describes: the same size,
/// and the same CFA where LibRaw's visible area starts.
fn fits(raw: &RawImage, m: &Mosaic) -> bool {
    let rawler_cfa = match &raw.photometric {
        RawPhotometricInterpretation::Cfa(config) => &config.cfa,
        _ => return false,
    };
    let letters: Vec<char> = rawler_cfa.name.chars().collect();
    let (w, h) = (rawler_cfa.width, rawler_cfa.height);
    (raw.width, raw.height) == (m.raw_width, m.raw_height)
        && (w, h) == (m.cfa_width, m.cfa_height)
        && m.cfa.chars().enumerate().all(|(i, libraw)| {
            let (r, c) = (i / m.cfa_width + m.top_margin, i % m.cfa_width + m.left_margin);
            letters[(r % h) * w + c % w] == libraw
        })
}

/// A dummy decode can't measure black from the masked areas, so rawler
/// leaves it 0; measure it now there are pixels.
fn black_from_masked_areas(raw: &mut RawImage) {
    if raw.blackareas.is_empty() || raw.blacklevel.as_vec().iter().any(|&b| b != 0.0) {
        return;
    }
    if let RawImageData::Integer(data) = &raw.data
        && let Some(black) = RawImage::calc_black_levels::<u16>(&raw.camera.cfa, &raw.blackareas, raw.width, raw.height, data)
    {
        raw.blacklevel = black;
    }
}

/// A `RawImage` of LibRaw's visible area with LibRaw's metadata.
fn from_libraw(m: Mosaic) -> RawImage {
    let colours = if m.cfa.contains('E') { "RGBE" } else { "RGB" };
    let cfa = CFA::new(&m.cfa);
    let plane_color = PlaneColor::new(colours);

    let rows = colours.len();
    let mut xyz_to_cam = [[0.0f32; 3]; 4];
    xyz_to_cam[..rows].copy_from_slice(&m.cam_xyz[..rows]);
    let mut color_matrix = HashMap::new();
    if xyz_to_cam[..3].iter().flatten().any(|&v| v != 0.0) {
        color_matrix.insert(Illuminant::D65, xyz_to_cam[..rows].as_flattened().to_vec());
    }

    // rawler's order is R G B E, normalised to green; E is NaN for 3 colours.
    let wb_coeffs = if m.cam_mul[..3].iter().all(|&v| v.is_finite() && v > 0.0) {
        let g = m.cam_mul[1];
        let e = if rows == 4 && m.cam_mul[3] > 0.0 { m.cam_mul[3] / g } else { f32::NAN };
        [m.cam_mul[0] / g, 1.0, m.cam_mul[2] / g, e]
    } else {
        [f32::NAN; 4]
    };

    let mut camera = Camera::new();
    camera.make = m.make.clone();
    camera.model = m.model.clone();
    camera.clean_make = m.make.clone();
    camera.clean_model = m.model.clone();
    camera.raw_width = m.width;
    camera.raw_height = m.height;
    camera.cfa = cfa.clone();
    camera.plane_color = plane_color.clone();
    camera.xyz_to_cam = xyz_to_cam;
    camera.color_matrix = color_matrix.clone();
    let bps = (32 - m.white.leading_zeros()) as usize;
    camera.real_bps = bps;

    let crop_area = m.crop.map(|[x, y, w, h]| Rect::new(Point::new(x, y), Dim2::new(w, h)));
    RawImage {
        camera,
        make: m.make.clone(),
        model: m.model.clone(),
        clean_make: m.make,
        clean_model: m.model,
        width: m.width,
        height: m.height,
        cpp: 1,
        bps,
        wb_coeffs,
        whitelevel: WhiteLevel::new(vec![m.white]),
        blacklevel: BlackLevel::new(&m.black, m.cfa_width, m.cfa_height, 1),
        xyz_to_cam,
        photometric: RawPhotometricInterpretation::Cfa(CFAConfig::new(&cfa, &plane_color)),
        active_area: None,
        crop_area,
        blackareas: Vec::new(),
        orientation: Orientation::Normal,
        data: RawImageData::Integer(m.data),
        color_matrix,
        dng_tags: HashMap::new(),
        fuji_rotation_width: None,
    }
}
