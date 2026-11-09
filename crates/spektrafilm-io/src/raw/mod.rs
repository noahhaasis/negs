//! Platform-independent RAW development contract. Backends return upright (or
//! explicitly unoriented), full-resolution, scene-linear sRGB, without an upper
//! clamp or photographic tone curve. Preview and export share this entry point.

use std::{fmt, path::Path, str::FromStr};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use spektrafilm_math::image::ImageBuf;

use crate::RawWhiteBalance;

mod dng;
pub use dng::is_dng;

#[cfg(all(target_os = "macos", feature = "apple-raw"))]
mod apple;
#[cfg(all(target_os = "macos", feature = "apple-raw"))]
pub use apple::AppleRawDecoder;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RawDecoderPreference {
    #[default]
    Auto,
    Apple,
    /// Force one Apple generation for compatibility diagnostics.
    Apple9,
    Apple8,
    Rawler,
}

impl FromStr for RawDecoderPreference {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "apple" => Ok(Self::Apple),
            "apple9" => Ok(Self::Apple9),
            "apple8" => Ok(Self::Apple8),
            "rawler" => Ok(Self::Rawler),
            _ => bail!("Unknown RAW decoder {value:?}; choose auto, apple, apple9, apple8, or rawler"),
        }
    }
}

/// The lens corrections the camera maker recorded in the RAW (distortion and
/// vignetting), applied by the decoder where it supports the file. Off keeps
/// the lens's own projection; see docs/lens-corrections-plan.md.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LensCorrection {
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct RawDecodeOptions {
    pub white_balance: RawWhiteBalance,
    pub apply_orientation: bool,
    pub lens_correction: LensCorrection,
    /// Develop no larger than this on the long side where the decoder can do
    /// that quicker than the whole sensor, for small copies; 0 is the whole
    /// sensor. A decoder that can't returns it all, so a caller still reduces
    /// what it gets.
    pub max_long_side: u32,
}

impl Default for RawDecodeOptions {
    fn default() -> Self {
        Self {
            white_balance: RawWhiteBalance::default(),
            apply_orientation: true,
            lens_correction: LensCorrection::default(),
            max_long_side: 0,
        }
    }
}

/// A decoder may decline an unsupported format or a file it cannot render.
/// I/O, allocation and unsafe image dimensions remain fatal errors.
#[derive(Debug)]
pub struct UnsupportedRaw(pub String);

impl fmt::Display for UnsupportedRaw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
impl std::error::Error for UnsupportedRaw {}

/// Implement this trait for future platform/vendor decoders. Keep native
/// dependencies and sensor development inside the backend, not the film engine.
pub trait RawDecoder: Send + Sync {
    fn name(&self) -> &'static str;
    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<RawImage>;
}

pub struct RawImage {
    pub image: ImageBuf,
    /// The size of the whole read, upright as `image`, when this is a reduced
    /// one ([`RawDecodeOptions::max_long_side`]); None when it is the whole.
    pub reduced_from: Option<(u32, u32)>,
    /// The decoder can apply [`LensCorrection`] to this file, whether or not
    /// it was requested.
    pub lens_correction_supported: bool,
    /// Actual Apple decoder identifier, including the DNG suffix where used.
    pub decoder_version: Option<String>,
}

pub struct RawlerDecoder;

impl RawDecoder for RawlerDecoder {
    fn name(&self) -> &'static str {
        "rawler"
    }
    /// Applies the corrections in DNG opcodes and Sony and Fujifilm
    /// metadata ([`crate::lens`]).
    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<RawImage> {
        let raw = crate::load_raw_rawler(path, options.apply_orientation, options.white_balance, options.lens_correction)?;
        Ok(RawImage {
            image: raw.image,
            reduced_from: None,
            lens_correction_supported: raw.lens.is_some(),
            decoder_version: None,
        })
    }
}

pub struct DecodedRaw {
    pub image: ImageBuf,
    /// As [`RawImage::reduced_from`].
    pub reduced_from: Option<(u32, u32)>,
    pub decoder: &'static str,
    pub lens_correction_supported: bool,
    pub decoder_version: Option<String>,
}

impl DecodedRaw {
    fn new(raw: RawImage, decoder: &dyn RawDecoder) -> Self {
        Self {
            image: raw.image,
            reduced_from: raw.reduced_from,
            decoder: decoder.name(),
            lens_correction_supported: raw.lens_correction_supported,
            decoder_version: raw.decoder_version,
        }
    }
}

/// Ordered compatibility attempts. Every failure is retained if all versions
/// decline; fatal errors stop immediately rather than hiding damaged input or
/// an exhausted allocation behind a different decoder.
#[cfg(any(test, all(target_os = "macos", feature = "apple-raw")))]
fn decode_compatible(
    decoders: &[&dyn RawDecoder],
    path: &Path,
    options: RawDecodeOptions,
) -> Result<RawImage> {
    let mut failures = Vec::new();
    for decoder in decoders {
        match decoder.decode(path, options) {
            Ok(raw) => return Ok(raw),
            Err(error) if error.is::<UnsupportedRaw>() => {
                failures.push(format!("{}: {error:#}", decoder.name()));
            }
            Err(error) => return Err(error.context(format!("{} failed", decoder.name()))),
        }
    }
    Err(UnsupportedRaw(failures.join("; ")).into())
}

#[cfg(any(test, all(target_os = "macos", feature = "apple-raw")))]
fn decode_with_fallback(
    primary: &dyn RawDecoder,
    fallback: &dyn RawDecoder,
    path: &Path,
    options: RawDecodeOptions,
) -> Result<DecodedRaw> {
    match primary.decode(path, options) {
        Ok(raw) => Ok(DecodedRaw::new(raw, primary)),
        Err(error) if error.is::<UnsupportedRaw>() => fallback
            .decode(path, options)
            .map(|raw| DecodedRaw::new(raw, fallback))
            .map_err(|other| {
                anyhow::anyhow!(
                    "{error}; {} fallback also failed: {other:#}",
                    fallback.name()
                )
            }),
        Err(error) => Err(error),
    }
}

/// On macOS Auto tries Apple RAW 9, Apple RAW 8, then rawler, per file.
/// Unsupported versions and incompatible renders permit fallback; I/O and
/// resource failures remain errors. Apple stays within those two Apple versions;
/// Apple9/Apple8 force one generation for diagnostics. Elsewhere Auto uses rawler.
pub fn decode_raw(
    path: &Path,
    options: RawDecodeOptions,
    preference: RawDecoderPreference,
) -> Result<DecodedRaw> {
    options.white_balance.validate()?;
    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    if preference == RawDecoderPreference::Auto {
        return decode_with_fallback(&AppleRawDecoder, &RawlerDecoder, path, options);
    } else if preference == RawDecoderPreference::Apple {
        return AppleRawDecoder
            .decode(path, options)
            .map(|raw| DecodedRaw::new(raw, &AppleRawDecoder));
    } else if matches!(preference, RawDecoderPreference::Apple9 | RawDecoderPreference::Apple8) {
        let decoder = apple::AppleRawVersion(if preference == RawDecoderPreference::Apple9 { 9 } else { 8 });
        return decoder.decode(path, options)
            .map(|raw| DecodedRaw::new(raw, &AppleRawDecoder));
    }
    if matches!(preference, RawDecoderPreference::Apple | RawDecoderPreference::Apple9 | RawDecoderPreference::Apple8) {
        bail!("Apple RAW decoding requires macOS and the apple-raw Cargo feature");
    }
    RawlerDecoder
        .decode(path, options)
        .map(|raw| DecodedRaw::new(raw, &RawlerDecoder))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    use anyhow::Context;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Backend {
        calls: AtomicUsize,
        mode: u8,
    }
    impl RawDecoder for Backend {
        fn name(&self) -> &'static str {
            "test backend"
        }
        fn decode(&self, _: &Path, _: RawDecodeOptions) -> Result<RawImage> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode {
                1 => Err(UnsupportedRaw("unsupported profile".into()).into()),
                2 => bail!("render failed"),
                _ => Ok(RawImage {
                    image: ImageBuf::new(1, 1),
                    reduced_from: None,
                    lens_correction_supported: false,
                    decoder_version: None,
                }),
            }
        }
    }

    #[test]
    fn fallback_only_for_unsupported_formats_never_for_failed_renders() {
        let fallback = Backend {
            calls: AtomicUsize::new(0),
            mode: 0,
        };
        for mode in [0, 1, 2] {
            let primary = Backend {
                calls: AtomicUsize::new(0),
                mode,
            };
            let result = decode_with_fallback(
                &primary,
                &fallback,
                Path::new("test.nef"),
                RawDecodeOptions::default(),
            );
            assert_eq!(result.is_ok(), mode != 2);
            assert_eq!(primary.calls.load(Ordering::SeqCst), 1);
            assert_eq!(
                fallback.calls.load(Ordering::SeqCst),
                usize::from(mode >= 1)
            );
        }
    }

    #[test]
    fn compatibility_order_stops_after_first_success_or_fatal_error() {
        // 0 success, 1 incompatible, 2 fatal: exercise 9 → 8 → rawler
        // independently of the host OS and installed camera support.
        for (modes, expected_calls, success) in [
            ([0, 0, 0], [1, 0, 0], true),
            ([1, 0, 0], [1, 1, 0], true),
            ([1, 1, 0], [1, 1, 1], true),
            ([1, 1, 1], [1, 1, 1], false),
            ([2, 0, 0], [1, 0, 0], false),
            ([1, 2, 0], [1, 1, 0], false),
        ] {
            let backends = modes.map(|mode| Backend { calls: AtomicUsize::new(0), mode });
            let result = decode_compatible(
                &[&backends[0], &backends[1], &backends[2]],
                Path::new("test.orf"), RawDecodeOptions::default(),
            );
            assert_eq!(result.is_ok(), success, "{modes:?}");
            assert_eq!(backends.each_ref().map(|b| b.calls.load(Ordering::SeqCst)), expected_calls);
            if modes == [1, 1, 1] {
                let error = result.err().unwrap();
                assert!(error.is::<UnsupportedRaw>());
                assert_eq!(error.to_string().matches("unsupported profile").count(), 3);
            }
        }
    }

    #[test]
    fn forced_apple_generations_are_explicit_diagnostic_preferences() {
        assert_eq!("apple9".parse::<RawDecoderPreference>().unwrap(), RawDecoderPreference::Apple9);
        assert_eq!("apple8".parse::<RawDecoderPreference>().unwrap(), RawDecoderPreference::Apple8);
        assert!("apple7".parse::<RawDecoderPreference>().is_err());
    }

    #[test]
    fn invalid_white_balance_is_rejected_before_any_file_is_opened() {
        let options = RawDecodeOptions {
            white_balance: crate::RawWhiteBalance {
                mode: crate::RawWhiteBalanceMode::Custom,
                temperature: f64::NAN,
                tint: 1.0,
            },
            ..Default::default()
        };
        let error = decode_raw(Path::new("absent.nef"), options, RawDecoderPreference::Auto)
            .err()
            .unwrap();
        assert!(error.to_string().contains("temperature"));
    }

    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn apple_raw_orientation_white_balance_and_highlights() -> Result<()> {
        use crate::{RawWhiteBalanceMode, raw_exif_orientation};
        use rawler::{decoders::RawDecodeParams, rawsource::RawSource};
        let path = std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE")
            .context("SPEKTRAFILM_RAW_TEST_FILE is required")?;
        let path = Path::new(&path);
        let options = RawDecodeOptions::default();
        let upright = AppleRawDecoder.decode(path, options)?.image;
        let sensor = AppleRawDecoder.decode(
            path,
            RawDecodeOptions {
                apply_orientation: false,
                ..options
            },
        )?.image;
        let orientation = raw_exif_orientation(&RawSource::new(path)?, &RawDecodeParams::default());
        let transformed = sensor.oriented(orientation);
        assert_eq!(
            (upright.width, upright.height),
            (transformed.width, transformed.height)
        );
        let error = upright
            .data
            .iter()
            .zip(&transformed.data)
            .map(|(a, b)| (a - b).abs() as f64)
            .fold(0.0, f64::max);
        assert!(error < 2e-5, "orientation difference {error}");
        let daylight = RawDecodeOptions {
            white_balance: RawWhiteBalance {
                mode: RawWhiteBalanceMode::Daylight,
                ..Default::default()
            },
            ..options
        };
        let day = AppleRawDecoder.decode(path, daylight)?.image;
        let identity = AppleRawDecoder.decode(
            path,
            RawDecodeOptions {
                white_balance: RawWhiteBalance {
                    mode: RawWhiteBalanceMode::Custom,
                    temperature: 6504.0,
                    tint: 1.0,
                },
                ..options
            },
        )?.image;
        assert_eq!(
            day.data, identity.data,
            "custom daylight must be exactly daylight"
        );
        let tinted = AppleRawDecoder.decode(
            path,
            RawDecodeOptions {
                white_balance: RawWhiteBalance {
                    mode: RawWhiteBalanceMode::Custom,
                    temperature: 3200.0,
                    tint: 0.8,
                },
                ..options
            },
        )?.image;
        assert!(
            day.data
                .iter()
                .zip(&tinted.data)
                .any(|(a, b)| (a - b).abs() > 0.001)
        );
        assert!(upright.data.iter().all(|v| v.is_finite()));
        // A fixture with exposure metadata can contain values above 1. Their
        // preservation is also measured by raw_decoder_probe, not asserted for
        // a photograph which may have no saturated scene content.
        Ok(())
    }

    /// Apple corrects within the same extent, so a crop keeps its size. A
    /// file without correction data is unchanged by the request.
    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn apple_raw_lens_correction() -> Result<()> {
        let path = std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE")
            .context("SPEKTRAFILM_RAW_TEST_FILE is required")?;
        let path = Path::new(&path);
        let off = AppleRawDecoder.decode(path, RawDecodeOptions::default())?;
        let on = AppleRawDecoder.decode(
            path,
            RawDecodeOptions {
                lens_correction: LensCorrection { enabled: true },
                ..Default::default()
            },
        )?;
        assert_eq!(off.lens_correction_supported, on.lens_correction_supported);
        assert_eq!(
            (off.image.width, off.image.height),
            (on.image.width, on.image.height)
        );
        assert_eq!(off.image.data != on.image.data, on.lens_correction_supported);
        Ok(())
    }

    /// Where Apple cannot correct a file, Negs corrects it unoriented and
    /// orients it itself. That orientation must equal Apple's to the bit, so
    /// the developed photograph is the same as Apple orienting it.
    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn apple_orientation_matches_negs() -> Result<()> {
        let path = std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE")
            .context("SPEKTRAFILM_RAW_TEST_FILE is required")?;
        let path = Path::new(&path);
        let apple = AppleRawDecoder.decode(path, RawDecodeOptions::default())?.image;
        let upright = AppleRawDecoder.decode(
            path,
            RawDecodeOptions { apply_orientation: false, ..Default::default() },
        )?;
        let src = rawler::rawsource::RawSource::new(path)?;
        let orientation = crate::raw_exif_orientation(&src, &Default::default());
        let ours = upright.image.oriented(orientation);
        assert_eq!((ours.width, ours.height), (apple.width, apple.height));
        assert!(ours.data == apple.data, "orientation {orientation} differs from Apple's");
        Ok(())
    }

    /// A read limited in size is a smaller copy of the same picture: the same
    /// shape and the same colours region by region, and it reports the whole
    /// read's size. No limit, or one the sensor is within, is the whole read.
    #[cfg(all(target_os = "macos", feature = "apple-raw"))]
    #[test]
    #[ignore = "requires a local camera RAW: set SPEKTRAFILM_RAW_TEST_FILE"]
    fn apple_raw_reduced_read_matches_the_whole_one() -> Result<()> {
        let path = std::env::var_os("SPEKTRAFILM_RAW_TEST_FILE")
            .context("SPEKTRAFILM_RAW_TEST_FILE is required")?;
        let path = Path::new(&path);
        for lens_correction in [LensCorrection { enabled: false }, LensCorrection { enabled: true }] {
            let whole = AppleRawDecoder.decode(path, RawDecodeOptions { lens_correction, ..Default::default() })?;
            let full = &whole.image;
            assert!(whole.reduced_from.is_none());
            let reduced = AppleRawDecoder.decode(
                path,
                RawDecodeOptions { lens_correction, max_long_side: 1200, ..Default::default() },
            )?;
            assert_eq!(reduced.reduced_from, Some((full.width, full.height)));
            assert_eq!(reduced.lens_correction_supported, whole.lens_correction_supported);
            let small = &reduced.image;
            assert_eq!(small.width.max(small.height), 1200);
            let aspect = |w: u32, h: u32| w as f64 / h as f64;
            assert!((aspect(full.width, full.height) - aspect(small.width, small.height)).abs() < 0.01);
            let region = |image: &ImageBuf, cx: u32, cy: u32| -> [f64; 3] {
                let (x0, x1) = (image.width * cx / 8, image.width * (cx + 1) / 8);
                let (y0, y1) = (image.height * cy / 8, image.height * (cy + 1) / 8);
                let mut total = [0.0f64; 3];
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y as usize * image.width as usize + x as usize) * 3;
                        for (c, total) in total.iter_mut().enumerate() {
                            *total += image.data[i + c] as f64;
                        }
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)) as f64;
                total.map(|t| t / n)
            };
            for cy in 0..8 {
                for cx in 0..8 {
                    let (a, b) = (region(full, cx, cy), region(small, cx, cy));
                    for c in 0..3 {
                        assert!((a[c] - b[c]).abs() < 0.01, "region {cx},{cy} channel {c}: {} and {}", a[c], b[c]);
                    }
                }
            }
            let wide = AppleRawDecoder.decode(
                path,
                RawDecodeOptions { lens_correction, max_long_side: 100_000, ..Default::default() },
            )?;
            assert!(wide.reduced_from.is_none());
            assert!(wide.image.data == full.data);
        }
        Ok(())
    }
}
