//! Platform-independent RAW development contract. Backends return upright (or
//! explicitly unoriented), full-resolution, scene-linear sRGB, without an upper
//! clamp or photographic tone curve. Preview and export share this entry point.

use std::{fmt, path::Path, str::FromStr};

use anyhow::{Result, bail};
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
    Rawler,
}

impl FromStr for RawDecoderPreference {
    type Err = anyhow::Error;

    fn from_str(value: &str) -> Result<Self> {
        match value {
            "auto" => Ok(Self::Auto),
            "apple" => Ok(Self::Apple),
            "rawler" => Ok(Self::Rawler),
            _ => bail!("Unknown RAW decoder {value:?}; choose auto, apple, or rawler"),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RawDecodeOptions {
    pub white_balance: RawWhiteBalance,
    pub apply_orientation: bool,
}

impl Default for RawDecodeOptions {
    fn default() -> Self {
        Self {
            white_balance: RawWhiteBalance::default(),
            apply_orientation: true,
        }
    }
}

/// A backend may decline an unsupported format. A failed render must not be
/// silently retried with different colour processing.
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
    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<ImageBuf>;
}

pub struct RawlerDecoder;

impl RawDecoder for RawlerDecoder {
    fn name(&self) -> &'static str {
        "rawler"
    }
    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<ImageBuf> {
        crate::load_raw_rawler(path, options.apply_orientation, options.white_balance)
    }
}

pub struct DecodedRaw {
    pub image: ImageBuf,
    pub decoder: &'static str,
}

#[cfg(any(test, all(target_os = "macos", feature = "apple-raw")))]
fn decode_with_fallback(
    primary: &dyn RawDecoder,
    fallback: &dyn RawDecoder,
    path: &Path,
    options: RawDecodeOptions,
) -> Result<DecodedRaw> {
    match primary.decode(path, options) {
        Ok(image) => Ok(DecodedRaw {
            image,
            decoder: primary.name(),
        }),
        Err(error) if error.is::<UnsupportedRaw>() => fallback
            .decode(path, options)
            .map(|image| DecodedRaw {
                image,
                decoder: fallback.name(),
            })
            .map_err(|other| {
                anyhow::anyhow!(
                    "{error}; {} fallback also failed: {other:#}",
                    fallback.name()
                )
            }),
        Err(error) => Err(error),
    }
}

/// Auto prefers Apple on macOS, and uses rawler elsewhere. Only an explicit
/// unsupported-format response permits fallback; I/O and rendering failures
/// remain errors. An explicit preference never falls back.
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
            .map(|image| DecodedRaw {
                image,
                decoder: AppleRawDecoder.name(),
            });
    }
    if preference == RawDecoderPreference::Apple {
        bail!("Apple RAW decoding requires macOS and the apple-raw Cargo feature");
    }
    RawlerDecoder.decode(path, options).map(|image| DecodedRaw {
        image,
        decoder: RawlerDecoder.name(),
    })
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
        fn decode(&self, _: &Path, _: RawDecodeOptions) -> Result<ImageBuf> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.mode {
                1 => Err(UnsupportedRaw("unsupported profile".into()).into()),
                2 => bail!("render failed"),
                _ => Ok(ImageBuf::new(1, 1)),
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
        let upright = AppleRawDecoder.decode(path, options)?;
        let sensor = AppleRawDecoder.decode(
            path,
            RawDecodeOptions {
                apply_orientation: false,
                ..options
            },
        )?;
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
        let day = AppleRawDecoder.decode(path, daylight)?;
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
        )?;
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
        )?;
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
}
