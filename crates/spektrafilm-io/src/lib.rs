//! Image and camera-RAW loading shared by the GUI, the CLI and
//! `decode_raw_gui`.
//!
//! Every loader returns an [`ImageBuf`] of **linear** RGB values:
//! - RAW files go through `rawler` and come out in linear sRGB primaries.
//! - PNG is sRGB gamma-encoded and is decoded to linear here.
//! - TIFF (and any other `image`-crate format) passes through unchanged.
//!
//! Use [`input_is_linear_srgb`] to decide the pipeline's input colour
//! space for a given file.
//!
//! [`save_image`] writes the rendered result as TIFF/PNG/JPEG, carrying
//! over the source file's camera metadata read by [`read_metadata`].

pub mod metadata;
pub mod save;
mod white_balance;

pub use metadata::{ImageMetadata, read_metadata};
pub use save::save_image;
pub use white_balance::{RawWhiteBalance, RawWhiteBalanceMode};

use std::path::Path;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{Scalar, from_f32, srgb_decode};

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

/// True if the loaders return linear sRGB for this file (PNG after its
/// sRGB decode, RAW after rawler's develop step). The pipeline should
/// then use `input_color_space = "sRGB"` and
/// `input_cctf_decoding = false`, so the gamma isn't decoded twice.
pub fn input_is_linear_srgb(path: &Path) -> bool {
    let ext = extension(path);
    ext == "png" || is_raw_extension(&ext)
}

/// Whether the file extension selects the RAW decoder (case-insensitive).
pub fn is_raw(path: &Path) -> bool {
    is_raw_extension(&extension(path))
}

/// Load an image as linear RGB, applying its EXIF orientation. RAW
/// extensions go to [`load_raw`] in both modes.
pub fn load_image(path: &Path, formats: Formats) -> Result<ImageBuf> {
    load_image_with_wb(path, formats, RawWhiteBalance::default())
}

/// Shared preview/export loader. RAW WB is ignored for non-RAW inputs,
/// whose white balance is already baked into their RGB pixels.
pub fn load_image_with_wb(path: &Path, formats: Formats, wb: RawWhiteBalance) -> Result<ImageBuf> {
    let ext = extension(path);
    if is_raw_extension(&ext) {
        return load_raw_with_wb(path, wb);
    }
    if formats == Formats::TiffPngRaw && !matches!(ext.as_str(), "tif" | "tiff" | "png") {
        bail!("unsupported image format: .{ext} (supported: tiff, png, raw)");
    }
    let img = decode_no_limits(path)?;
    let rgb = img.to_rgb32f();
    let (w, h) = (rgb.width(), rgb.height());
    let data: Vec<f32> = rgb.into_raw();
    let scalars: Vec<Scalar> = match ext.as_str() {
        // PNG is sRGB gamma-encoded: widen to Scalar first, then decode
        // to linear.
        "png" => data
            .into_par_iter()
            .map(|v| srgb_decode(from_f32(v)))
            .collect(),
        _ => data.into_par_iter().map(from_f32).collect(),
    };
    Ok(ImageBuf::from_data(w, h, scalars))
}

/// Decode with the `image` crate's memory limits disabled and apply the
/// EXIF/TIFF orientation tag. The default 512 MiB cap rejects large float
/// TIFFs (a 60 MP 32-bit RGB frame alone is ~720 MiB). Local files are
/// trusted.
fn decode_no_limits(path: &Path) -> Result<image::DynamicImage> {
    let mut reader = image::ImageReader::open(path)
        .with_context(|| format!("opening image: {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("detecting image format: {}", path.display()))?;
    reader.no_limits();
    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("decoding image: {}", path.display()))?;
    let orientation = image::ImageDecoder::orientation(&mut decoder)
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = image::DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decoding image: {}", path.display()))?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// Decode a camera RAW file to **linear** sRGB primaries, with its EXIF
/// orientation applied.
///
/// `rawler::RawDevelop::default()` already does rescale → demosaic →
/// crop → white balance → camera→sRGB matrix. We drop the final `SRgb`
/// gamma step to get linear sRGB, because the spektrafilm pipeline
/// applies its own sRGB encoding at the end. Negative values are
/// clamped to 0.
///
/// Limitation: rawler can't read Lightroom-exported "lossy DNG"
/// (`ljpeg sof.precision 8`); those files return an error.
pub fn load_raw(path: &Path) -> Result<ImageBuf> {
    load_raw_with_wb(path, RawWhiteBalance::default())
}

/// Like [`load_raw`], using the selected input white balance. As shot
/// preserves the original default development path exactly.
pub fn load_raw_with_wb(path: &Path, wb: RawWhiteBalance) -> Result<ImageBuf> {
    load_raw_impl(path, true, wb)
}

/// Same as [`load_raw`], but without applying the EXIF orientation.
/// The output keeps the sensor's native layout.
pub fn load_raw_unoriented(path: &Path) -> Result<ImageBuf> {
    load_raw_impl(path, false, RawWhiteBalance::default())
}

fn load_raw_impl(path: &Path, orient: bool, wb: RawWhiteBalance) -> Result<ImageBuf> {
    use rawler::{
        decoders::RawDecodeParams,
        imgop::develop::{ProcessingStep, RawDevelop},
        rawsource::RawSource,
    };
    wb.validate()?;
    let src =
        RawSource::new(path).with_context(|| format!("opening RAW file: {}", path.display()))?;
    let decode_params = RawDecodeParams::default();
    let mut raw = rawler::decode(&src, &decode_params)
        .map_err(|e| anyhow::anyhow!("RAW decode failed: {e:?}"))?;
    let mut dev = RawDevelop::default();
    dev.steps.retain(|s| !matches!(s, ProcessingStep::SRgb));
    // rawler guards `decode` against panics but not `develop_intermediate`,
    // which still has `panic!`/`unimplemented!`/`assert!` paths for unusual
    // files (e.g. Fuji SuperCCD with an active area, non-3x3 colour matrices
    // without a D65 entry). The GUI loads on its UI thread, so turn a panic
    // into an error instead of taking the app down.
    let intermediate = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| -> Result<_> {
        white_balance::prepare(&mut raw, wb)?;
        let mut intermediate = dev
            .develop_intermediate(&raw)
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        white_balance::finish(&mut intermediate, wb, white_balance::is_monochrome(&raw))?;
        Ok(intermediate)
    }))
    .map_err(|p| anyhow::anyhow!("RAW develop failed: panicked: {}", panic_message(&p)))?
    .context("RAW develop failed")?;
    // The developed pixels no longer depend on the sensor buffer.
    drop(raw);
    let img = intermediate_to_image_buf(intermediate);
    if !orient {
        return Ok(img);
    }
    // rawler 0.8 still always sets `RawImage::orientation` to Normal (an
    // upstream TODO), so read the EXIF tag ourselves.
    let orientation = raw_exif_orientation(&src, &decode_params);
    Ok(img.oriented(orientation))
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

/// Convert rawler's developed output to RGB, clamping negatives to 0.
/// Monochrome is copied into all three channels; FourColor drops the
/// fourth channel.
fn intermediate_to_image_buf(intermediate: rawler::imgop::develop::Intermediate) -> ImageBuf {
    use rawler::imgop::develop::Intermediate;

    let floor = |v: f32| from_f32(v.max(0.0));
    let (w, h, scalars): (usize, usize, Vec<Scalar>) = match intermediate {
        Intermediate::Monochrome(pixels) => (
            pixels.width,
            pixels.height,
            pixels
                .data
                .par_iter()
                .flat_map_iter(|&v| {
                    let v = floor(v);
                    [v, v, v]
                })
                .collect(),
        ),
        Intermediate::ThreeColor(pixels) => (
            pixels.width,
            pixels.height,
            pixels
                .data
                .par_iter()
                .flat_map_iter(|px| [floor(px[0]), floor(px[1]), floor(px[2])])
                .collect(),
        ),
        Intermediate::FourColor(pixels) => (
            pixels.width,
            pixels.height,
            pixels
                .data
                .par_iter()
                .flat_map_iter(|px| [floor(px[0]), floor(px[1]), floor(px[2])])
                .collect(),
        ),
    };
    ImageBuf::from_data(w as u32, h as u32, scalars)
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

    #[test]
    fn linear_srgb_detection() {
        assert!(input_is_linear_srgb(Path::new("a/b.PNG")));
        assert!(input_is_linear_srgb(Path::new("x.Nef")));
        assert!(!input_is_linear_srgb(Path::new("x.tiff")));
        assert!(!input_is_linear_srgb(Path::new("x.jpg")));
        assert!(!input_is_linear_srgb(Path::new("noext")));
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
        fn monochrome_is_copied_into_rgb_and_clamped() {
            let px = vec![0.25, -0.5, 1.5, 0.0, -0.0, 0.125];
            let img =
                intermediate_to_image_buf(Intermediate::Monochrome(PixF32::new_with(px, 3, 2)));
            assert_eq!((img.width, img.height), (3, 2));
            #[rustfmt::skip]
            let want = s(&[
                0.25, 0.25, 0.25,  0.0, 0.0, 0.0,  1.5, 1.5, 1.5,
                0.0, 0.0, 0.0,     0.0, 0.0, 0.0,  0.125, 0.125, 0.125,
            ]);
            assert_eq!(img.data, want);
        }

        #[test]
        fn three_color_keeps_channel_order_and_clamps() {
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
                0.1, 0.2, 0.3,  0.0, 0.5, 2.0,  0.0, 0.0, 0.75,
                1.0, 1.0, 0.0,  0.4, 0.5, 0.6,  0.7, 0.8, 0.9,
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
                0.1, 0.2, 0.3,  0.0, 0.5, 2.0,  0.0, 0.0, 0.75,
                1.0, 1.0, 0.0,  0.4, 0.5, 0.6,  0.7, 0.8, 0.9,
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
