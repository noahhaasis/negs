//! Image and camera-RAW loading shared by the GUI, the CLI and
//! `decode_raw_gui`.
//!
//! Every loader returns an [`ImageBuf`] in the engine's input representation:
//! linear light, unclamped, in sRGB (BT.709) primaries with a D65 white.
//! - RAW files use the platform decoder (scene-referred).
//! - JPEG, PNG and TIFF are read through their embedded profile or colour
//!   metadata, with documented fallbacks for untagged files ([`colour`]).
//!
//! [`load_image_colour`] also reports what the file was read as
//! ([`InputColour`]), and takes the input interpretation override.
//!
//! [`save_image`] writes the rendered result as TIFF/PNG/JPEG, carrying
//! over the source file's camera metadata read by [`read_metadata`].

pub mod border;
pub mod bounded;
pub mod colour;
pub mod crop;
pub mod encoding;
pub mod film35;
pub mod icc;
pub mod image_size;
pub mod metadata;
pub mod raw;
pub mod resize;
pub mod save;
mod white_balance;

pub use colour::{InputColour, InputInterpretation, InputMode};
pub use metadata::{ImageMetadata, read_metadata};
pub use save::save_image;
pub use raw::LensCorrection;
pub use white_balance::{RawWhiteBalance, RawWhiteBalanceMode};

use std::path::Path;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32};

/// Which non-RAW formats [`load_image`] accepts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Formats {
    /// Any format the `image` crate can decode (GUI).
    Any,
    /// Only TIFF, PNG and camera RAW; anything else is an error (CLI).
    TiffPngRaw,
}

/// Lower-cased file extension of `path`, or `""` when it has none.
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// True if `ext` (lower case, without the dot) is a camera RAW
/// extension. The list only decides when to use the RAW decoder.
pub fn is_raw_extension(ext: &str) -> bool {
    matches!(
        ext,
        "dng"
            | "cr2"
            | "cr3"
            | "nef"
            | "nrw"
            | "arw"
            | "srf"
            | "sr2"
            | "raf"
            | "orf"
            | "rw2"
            | "pef"
            | "ptx"
            | "srw"
            | "x3f"
            | "iiq"
            | "3fr"
            | "ari"
            | "bay"
            | "crw"
            | "dcr"
            | "drf"
            | "erf"
            | "fff"
            | "k25"
            | "kdc"
            | "mef"
            | "mos"
            | "mrw"
            | "rwl"
    )
}

/// Whether the file extension selects the RAW decoder (case-insensitive).
pub fn is_raw(path: &Path) -> bool {
    is_raw_extension(&extension(path))
}

/// Load an image in the engine's representation, applying its EXIF
/// orientation. RAW extensions go to [`load_raw`] in both modes.
pub fn load_image(path: &Path, formats: Formats) -> Result<ImageBuf> {
    load_image_with_wb(path, formats, RawWhiteBalance::default())
}

/// [`load_image_colour`] with automatic input interpretation, without the report.
pub fn load_image_with_wb(path: &Path, formats: Formats, wb: RawWhiteBalance) -> Result<ImageBuf> {
    Ok(load_image_colour(path, formats, wb, LensCorrection::default(), &InputInterpretation::default())?.image)
}

/// An input in the engine's representation, and what it was read as.
pub struct LoadedImage {
    pub image: ImageBuf,
    pub colour: InputColour,
    /// A RAW whose decoder can apply [`LensCorrection`]; false for other files.
    pub lens_correction_supported: bool,
}

/// Shared preview/export loader. RAW WB applies to RAW files only, and the
/// input interpretation to non-RAW files only: a RAW is always developed by
/// its decoder. A non-RAW file's white balance is already in its pixels, as
/// are its lens corrections.
pub fn load_image_colour(
    path: &Path,
    formats: Formats,
    wb: RawWhiteBalance,
    lens: LensCorrection,
    interpretation: &InputInterpretation,
) -> Result<LoadedImage> {
    let ext = extension(path);
    if is_raw_extension(&ext) {
        let decoded = raw::decode_raw(
            path,
            raw::RawDecodeOptions { white_balance: wb, apply_orientation: true, lens_correction: lens },
            raw::RawDecoderPreference::Auto,
        )?;
        return Ok(LoadedImage {
            image: decoded.image,
            colour: InputColour::raw(decoded.decoder),
            lens_correction_supported: decoded.lens_correction_supported,
        });
    }
    if formats == Formats::TiffPngRaw && !matches!(ext.as_str(), "tif" | "tiff" | "png") {
        bail!("unsupported image format: .{ext} (supported: tiff, png, raw)");
    }
    let (img, info, format) = decode_no_limits(path)?;
    let (image, colour) = colour::convert(img, &info, interpretation, format == image::ImageFormat::Png)?;
    Ok(LoadedImage { image, colour, lens_correction_supported: false })
}

/// Decode with the `image` crate's memory limits disabled and apply the
/// EXIF/TIFF orientation tag, keeping what the file says about its colour.
/// The default 512 MiB cap rejects large float TIFFs (a 60 MP 32-bit RGB
/// frame alone is ~720 MiB). Local files are trusted.
fn decode_no_limits(path: &Path) -> Result<(image::DynamicImage, colour::FileColour, image::ImageFormat)> {
    let mut reader = image::ImageReader::open(path)
        .with_context(|| format!("opening image: {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("detecting image format: {}", path.display()))?;
    reader.no_limits();
    let format = reader.format().with_context(|| format!("unknown image format: {}", path.display()))?;
    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("decoding image: {}", path.display()))?;
    use image::ImageDecoder;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let open = || std::fs::File::open(path).map(std::io::BufReader::new);
    let mut info = colour::FileColour {
        icc: decoder.icc_profile().ok().flatten(),
        exif: decoder.exif_metadata().ok().flatten(),
        cmyk: matches!(decoder.original_color_type(), image::ExtendedColorType::Cmyk8 | image::ExtendedColorType::Cmyk16),
        ..Default::default()
    };
    match format {
        image::ImageFormat::Png => info.png = open().ok().and_then(colour::tags::png_chunks),
        image::ImageFormat::Jpeg => info.cmyk |= open().ok().and_then(colour::tags::jpeg_components) == Some(4),
        image::ImageFormat::Tiff if info.icc.is_none() => info.icc = open().ok().and_then(colour::tags::tiff_icc),
        _ => {}
    }
    let mut img = image::DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decoding image: {}", path.display()))?;
    img.apply_orientation(orientation);
    info.float = matches!(img, image::DynamicImage::ImageRgb32F(_) | image::DynamicImage::ImageRgba32F(_));
    info.gray = !img.color().has_color();
    Ok((img, info, format))
}

/// Decode a camera RAW file to **linear** sRGB primaries, with its EXIF
/// orientation applied.
///
/// Auto selects Apple's full-resolution RAW developer on macOS and rawler
/// elsewhere. Unsupported Apple formats fall back to rawler. Both produce
/// unclamped linear sRGB: values above 1 and negative (outside-sRGB)
/// coordinates are kept.
pub fn load_raw(path: &Path) -> Result<ImageBuf> {
    load_raw_with_wb(path, RawWhiteBalance::default())
}

/// Like [`load_raw`], using the selected input white balance. As shot uses
/// the camera white balance through the selected platform backend.
pub fn load_raw_with_wb(path: &Path, wb: RawWhiteBalance) -> Result<ImageBuf> {
    load_raw_impl(path, true, wb)
}

/// Same as [`load_raw`], but without applying the EXIF orientation.
/// The output keeps the sensor's native layout.
pub fn load_raw_unoriented(path: &Path) -> Result<ImageBuf> {
    load_raw_impl(path, false, RawWhiteBalance::default())
}

fn load_raw_impl(path: &Path, orient: bool, wb: RawWhiteBalance) -> Result<ImageBuf> {
    Ok(raw::decode_raw(
        path,
        raw::RawDecodeOptions {
            white_balance: wb,
            apply_orientation: orient,
            ..Default::default()
        },
        raw::RawDecoderPreference::Auto,
    )?
    .image)
}

fn load_raw_rawler(path: &Path, orient: bool, wb: RawWhiteBalance) -> Result<ImageBuf> {
    use rawler::rawsource::RawSource;
    wb.validate()?;
    let src =
        RawSource::new(path).with_context(|| format!("opening RAW file: {}", path.display()))?;
    develop_rawler(&src, orient, wb)
}

/// [`load_raw_with_wb`] from the file's bytes, with rawler: for browsers,
/// which have no files.
pub fn load_raw_bytes(bytes: &[u8], wb: RawWhiteBalance) -> Result<ImageBuf> {
    wb.validate()?;
    develop_rawler(&rawler::rawsource::RawSource::new_from_slice(bytes), true, wb)
}

/// [`load_raw_bytes`] without turning the image upright, and the EXIF
/// orientation (1 to 8) that would: for a caller that shrinks it first,
/// which then turns a smaller copy.
pub fn load_raw_bytes_unoriented(bytes: &[u8], wb: RawWhiteBalance) -> Result<(ImageBuf, u16)> {
    wb.validate()?;
    let src = rawler::rawsource::RawSource::new_from_slice(bytes);
    let image = develop_rawler(&src, false, wb)?;
    Ok((image, raw_exif_orientation(&src, &rawler::decoders::RawDecodeParams::default())))
}

fn develop_rawler(src: &rawler::rawsource::RawSource, orient: bool, wb: RawWhiteBalance) -> Result<ImageBuf> {
    use rawler::decoders::RawDecodeParams;
    let decode_params = RawDecodeParams::default();
    let raw = rawler::decode(src, &decode_params)
        .map_err(|e| anyhow::anyhow!("RAW decode failed: {e:?}"))?;
    let img = develop_decoded(raw, wb)?;
    if !orient {
        return Ok(img);
    }
    // rawler still sets RawImage::orientation to Normal: read EXIF ourselves.
    Ok(img.oriented(raw_exif_orientation(src, &decode_params)))
}

/// Develop an already decoded sensor; shared with the opt-in bounded loader.
fn develop_decoded(mut raw: rawler::RawImage, wb: RawWhiteBalance) -> Result<ImageBuf> {
    use rawler::imgop::develop::{ProcessingStep, RawDevelop};
    let mut dev = RawDevelop::default();
    // Our `calibrate` replaces rawler's, which clips (see there).
    dev.steps.retain(|s| !matches!(s, ProcessingStep::SRgb | ProcessingStep::Calibrate));
    // rawler guards `decode` against panics but not `develop_intermediate`,
    // which still has `panic!`/`unimplemented!`/`assert!` paths for unusual
    // files (e.g. Fuji SuperCCD with an active area, non-3x3 colour matrices
    // without a D65 entry). The GUI loads on its UI thread, so turn a panic
    // into an error instead of taking the app down.
    let intermediate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<_> {
        white_balance::prepare(&mut raw, wb)?;
        // The develop consumes the sensor data, so it never holds a second
        // copy; the rest (the matrices, the white balance) calibrates.
        let meta = without_data(&mut raw);
        let intermediate = dev
            .develop_intermediate_owned(raw)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        let mut intermediate = calibrate(&meta, intermediate);
        white_balance::finish(&mut intermediate, wb, white_balance::is_monochrome(&meta))?;
        Ok(intermediate)
    }))
    .map_err(|p| anyhow::anyhow!("RAW develop failed: panicked: {}", panic_message(&p)))?
    .context("RAW develop failed")?;
    Ok(intermediate_to_image_buf(intermediate))
}

/// A copy of `raw` without its sensor data.
fn without_data(raw: &mut rawler::RawImage) -> rawler::RawImage {
    let data = std::mem::replace(&mut raw.data, rawler::rawimage::RawImageData::Integer(Vec::new()));
    let meta = raw.clone();
    raw.data = data;
    meta
}

/// Text of a caught panic payload (`panic!` with a literal or a format).
fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// EXIF orientation tag (1..=8) of a RAW file, or 1 if it is missing or
/// unreadable. rawler only guards `decode` against panics, not the
/// metadata parsers. Orientation is best-effort (and the GUI calls this
/// on its UI thread), so a panic falls back to 1.
fn raw_exif_orientation(
    src: &rawler::rawsource::RawSource,
    params: &rawler::decoders::RawDecodeParams,
) -> u16 {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        rawler::get_decoder(src)
            .and_then(|d| d.raw_metadata(src, params))
            .ok()
            .and_then(|m| m.exif.orientation)
    }))
    .ok()
    .flatten()
    .unwrap_or(1)
}

/// Raw sensor values (white level 1) from which a pixel is desaturated toward
/// neutral, reaching neutral at the white level. See [`calibrate`].
const RAW_DESATURATE_FROM: f32 = 0.85;

/// rawler 0.8's Calibrate step (white balance, then camera RGB → linear
/// sRGB through the camera's D65 matrix, Bradford-adapted when it has
/// none), without its final `clip_euclidean_norm_avg`. That clip floored
/// negative channels (colours outside sRGB) and rescaled every pixel with a
/// channel above 1, whether or not the sensor had clipped there, so bright
/// but unclipped highlights lost their linearity and saturation. Here the
/// result stays linear and unclamped. Only near the sensor's white level,
/// where a clipped channel would show as false (typically magenta) colour,
/// is a pixel blended toward the neutral of the same luminance: not at all
/// below [`RAW_DESATURATE_FROM`] of the white level, fully at it.
fn calibrate(raw: &rawler::RawImage, intermediate: rawler::imgop::develop::Intermediate) -> rawler::imgop::develop::Intermediate {
    use rawler::imgop::develop::Intermediate;
    use rawler::imgop::matrix::{multiply, normalize, pseudo_inverse};
    use rawler::imgop::xyz::{Illuminant, SRGB_TO_XYZ_D65};
    use rawler::imgop::{chromatic_adaption::adapt_bradford, xyz::Illuminant as I};
    let (illuminant, matrix) = raw
        .color_matrix_find_first([I::D65, I::A, I::B, I::C, I::D50, I::D55, I::D75, I::Daylight, I::Flash])
        .unwrap_or((Illuminant::D65, vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]));
    let matrix = if illuminant == Illuminant::D65 || matrix.len() != 9 {
        matrix
    } else {
        let m: [[f32; 3]; 3] = std::array::from_fn(|i| std::array::from_fn(|j| matrix[i * 3 + j]));
        adapt_bradford(&illuminant, &Illuminant::D65, &m).as_flattened().to_vec()
    };
    let mut xyz2cam = [[0.0f32; 3]; 4];
    for (i, row) in matrix.chunks_exact(3).take(4).enumerate() {
        xyz2cam[i].copy_from_slice(row);
    }
    let wb = if raw.wb_coeffs[0].is_nan() { [1.0; 4] } else { raw.wb_coeffs };
    let cam2rgb = pseudo_inverse(normalize(multiply(&xyz2cam, &SRGB_TO_XYZ_D65)));
    let luma = [0.2126f32, 0.7152, 0.0722];
    let develop = |cam: &[f32]| -> [f32; 3] {
        let mut rgb = [0.0f32; 3];
        for (c, &v) in cam.iter().enumerate() {
            for (i, out) in rgb.iter_mut().enumerate() {
                *out += cam2rgb[i][c] * v * wb[c];
            }
        }
        let peak = cam.iter().copied().fold(0.0f32, f32::max);
        let t = ((peak - RAW_DESATURATE_FROM) / (1.0 - RAW_DESATURATE_FROM)).clamp(0.0, 1.0);
        if t > 0.0 {
            let t = t * t * (3.0 - 2.0 * t);
            let y = luma[0] * rgb[0] + luma[1] * rgb[1] + luma[2] * rgb[2];
            rgb = rgb.map(|v| v + (y - v) * t);
        }
        rgb
    };
    match intermediate {
        Intermediate::ThreeColor(mut pixels) => {
            pixels.data.par_iter_mut().for_each(|p| *p = develop(p));
            Intermediate::ThreeColor(pixels)
        }
        Intermediate::FourColor(pixels) => {
            let data = pixels.data.par_iter().map(|p| develop(p)).collect();
            Intermediate::ThreeColor(rawler::pixarray::Color2D::new_with(data, pixels.width, pixels.height))
        }
        mono @ Intermediate::Monochrome(_) => mono,
    }
}

/// Convert rawler's developed output to RGB, unclamped. Monochrome is copied
/// into all three channels; FourColor (only without calibration) drops the
/// fourth channel.
fn intermediate_to_image_buf(intermediate: rawler::imgop::develop::Intermediate) -> ImageBuf {
    use rawler::imgop::develop::Intermediate;

    let (w, h, scalars): (usize, usize, Vec<Scalar>) = match intermediate {
        Intermediate::Monochrome(pixels) => {
            let mut out = vec![from_f32(0.0); pixels.width * pixels.height * 3];
            out.par_chunks_exact_mut(3)
                .zip(pixels.data.par_iter())
                .for_each(|(dst, &v)| dst.fill(from_f32(v)));
            (pixels.width, pixels.height, out)
        }
        // The same f32s, in their buffer.
        #[cfg(not(feature = "precision-f64"))]
        Intermediate::ThreeColor(pixels) => (pixels.width, pixels.height, flatten_rgb(pixels.data)),
        #[cfg(feature = "precision-f64")]
        Intermediate::ThreeColor(pixels) => {
            let mut out = vec![from_f32(0.0); pixels.width * pixels.height * 3];
            out.par_chunks_exact_mut(3)
                .zip(pixels.data.par_iter())
                .for_each(|(dst, src)| {
                    dst.copy_from_slice(&[from_f32(src[0]), from_f32(src[1]), from_f32(src[2])]);
                });
            (pixels.width, pixels.height, out)
        }
        Intermediate::FourColor(pixels) => {
            let mut out = vec![from_f32(0.0); pixels.width * pixels.height * 3];
            out.par_chunks_exact_mut(3)
                .zip(pixels.data.par_iter())
                .for_each(|(dst, src)| {
                    dst.copy_from_slice(&[from_f32(src[0]), from_f32(src[1]), from_f32(src[2])]);
                });
            (pixels.width, pixels.height, out)
        }
    };
    ImageBuf::from_data(w as u32, h as u32, scalars)
}

/// RGB pixels as their channels, without a copy.
#[cfg(not(feature = "precision-f64"))]
fn flatten_rgb(pixels: Vec<[f32; 3]>) -> Vec<f32> {
    let mut pixels = std::mem::ManuallyDrop::new(pixels);
    let (ptr, len, capacity) = (pixels.as_mut_ptr(), pixels.len(), pixels.capacity());
    // SAFETY: [f32; 3] is three f32s with f32's alignment, so the allocation
    // is one of `capacity * 3` f32s, `len * 3` of them initialised.
    unsafe { Vec::from_raw_parts(ptr.cast::<f32>(), len * 3, capacity * 3) }
}

#[cfg(test)]
mod wb_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_extension_list() {
        for ext in ["nef", "dng", "cr3", "arw", "raf", "orf", "rwl"] {
            assert!(is_raw_extension(ext), "{ext}");
        }
        for ext in ["png", "tif", "jpg", "NEF", ""] {
            assert!(!is_raw_extension(ext), "{ext}");
        }
    }

    // No real camera file reliably reaches every `Intermediate` variant:
    // with rawler 0.8's default steps, Calibrate always turns FourColor
    // into ThreeColor (falling back to an identity matrix), and Monochrome
    // needs a true monochrome sensor. So test the conversion on synthetic
    // data. 3 px wide x 2 px high so width/height mixups show.
    mod intermediate {
        use super::super::intermediate_to_image_buf;
        use rawler::imgop::develop::Intermediate;
        use rawler::pixarray::{Color2D, PixF32};
        use spektrafilm_math::precision::{Scalar, from_f32};

        fn s(v: &[f32]) -> Vec<Scalar> {
            v.iter().copied().map(from_f32).collect()
        }

        #[test]
        fn monochrome_is_copied_into_rgb_unclamped() {
            let px = vec![0.25, -0.5, 1.5, 0.0, -0.0, 0.125];
            let img =
                intermediate_to_image_buf(Intermediate::Monochrome(PixF32::new_with(px, 3, 2)));
            assert_eq!((img.width, img.height), (3, 2));
            #[rustfmt::skip]
            let want = s(&[
                0.25, 0.25, 0.25,  -0.5, -0.5, -0.5,  1.5, 1.5, 1.5,
                0.0, 0.0, 0.0,     -0.0, -0.0, -0.0,  0.125, 0.125, 0.125,
            ]);
            assert_eq!(img.data, want);
        }

        #[test]
        fn three_color_keeps_channel_order_unclamped() {
            let px = vec![
                [0.1, 0.2, 0.3],
                [-1.0, 0.5, 2.0],
                [0.0, -0.25, 0.75],
                [1.0, 1.0, -1.0],
                [0.4, 0.5, 0.6],
                [0.7, 0.8, 0.9],
            ];
            let img =
                intermediate_to_image_buf(Intermediate::ThreeColor(Color2D::new_with(px, 3, 2)));
            assert_eq!((img.width, img.height), (3, 2));
            #[rustfmt::skip]
            let want = s(&[
                0.1, 0.2, 0.3,  -1.0, 0.5, 2.0,  0.0, -0.25, 0.75,
                1.0, 1.0, -1.0,  0.4, 0.5, 0.6,  0.7, 0.8, 0.9,
            ]);
            assert_eq!(img.data, want);
        }

        #[test]
        fn four_color_drops_fourth_channel_and_clamps() {
            let px = vec![
                [0.1, 0.2, 0.3, 9.0],
                [-1.0, 0.5, 2.0, 9.0],
                [0.0, -0.25, 0.75, -9.0],
                [1.0, 1.0, -1.0, 9.0],
                [0.4, 0.5, 0.6, 9.0],
                [0.7, 0.8, 0.9, 9.0],
            ];
            let img =
                intermediate_to_image_buf(Intermediate::FourColor(Color2D::new_with(px, 3, 2)));
            assert_eq!((img.width, img.height), (3, 2));
            #[rustfmt::skip]
            let want = s(&[
                0.1, 0.2, 0.3,  -1.0, 0.5, 2.0,  0.0, -0.25, 0.75,
                1.0, 1.0, -1.0,  0.4, 0.5, 0.6,  0.7, 0.8, 0.9,
            ]);
            assert_eq!(img.data, want);
        }
    }

    #[test]
    fn panic_message_reads_str_and_string_payloads() {
        // libtest captures the panic output, so no need to touch the
        // (process-global) panic hook.
        let quiet = |f: fn()| panic_message(&std::panic::catch_unwind(f).unwrap_err());
        assert_eq!(quiet(|| panic!("literal")), "literal");
        assert_eq!(quiet(|| panic!("formatted {}", 42)), "formatted 42");
        assert_eq!(quiet(|| std::panic::panic_any(7u8)), "unknown panic");
    }

    #[test]
    fn cli_mode_rejects_other_formats() {
        let err = load_image(Path::new("does-not-matter.jpg"), Formats::TiffPngRaw).unwrap_err();
        assert_eq!(
            err.to_string(),
            "unsupported image format: .jpg (supported: tiff, png, raw)"
        );
    }
}
