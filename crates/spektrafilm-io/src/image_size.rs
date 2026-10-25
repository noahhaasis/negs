//! Image Size: the resolution the film is developed at.
//!
//! The source is resized before the film simulation, so grain and halation
//! form at the new size. A Longest side target describes the *cropped*
//! photograph, before borders and export reductions, but the crop itself stays
//! after the simulation: the whole frame is developed, scaled so that the
//! crop's longest side lands on the target. Film format therefore still spans
//! the full working frame (`filming::pixel_size_um`), as in the Python app.
//!
//! The working size depends only on the crop's extent, not its position or
//! straightening angle, so panning a crop never develops the film again. The
//! crop's actual pixels, rounded as [`Crop::pixels`] rounds them, are within
//! one pixel of the target.
//!
//! The macOS app projects the same size for its crop readout
//! (`macos/Sources/NegsApp/ImageSize.swift`); keep the formula in sync.

use std::fmt;
use std::ops::RangeInclusive;

use spektrafilm_math::precision::Scalar;

use crate::crop::Crop;

/// The Longest side targets the apps accept, like their export sizes.
pub const PHOTO_LONG_SIDE_RANGE: RangeInclusive<u32> = 16..=65_535;

/// The largest working frame an enlargement may request: about 3× a 12 MP
/// photograph (108 MP). A provisional product bound, not a memory guarantee;
/// developing it needs several gigabytes. Sources larger than this still
/// develop at their own size or smaller.
pub const MAX_WORKING_PIXELS: u64 = 128_000_000;

/// Why a size request can't be developed. Checked before anything is
/// allocated at the requested size.
#[derive(Clone, Debug, PartialEq)]
pub enum SizeError {
    EmptySource,
    InvalidFactor(f32),
    TargetOutOfRange(u32),
    /// A Longest side target together with a working-resolution factor other
    /// than 1. Neither silently wins.
    TargetWithFactor(f32),
    InvalidCrop(String),
    TooLarge { width: f64, height: f64 },
}

impl fmt::Display for SizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptySource => write!(f, "The photograph has no pixels"),
            Self::InvalidFactor(factor) => write!(f, "Invalid working resolution: {factor}"),
            Self::TargetOutOfRange(px) => write!(
                f,
                "The longest side must be between {} and {} px, not {px}",
                PHOTO_LONG_SIDE_RANGE.start(),
                PHOTO_LONG_SIDE_RANGE.end()
            ),
            Self::TargetWithFactor(factor) => write!(
                f,
                "Image Size sets a longest side, so the working resolution factor must be 1, not {factor}"
            ),
            Self::InvalidCrop(reason) => write!(f, "{reason}"),
            Self::TooLarge { width, height } => write!(
                f,
                "This size needs a {} × {} px frame ({} MP) to develop, more than the {} MP limit. \
                 Choose a smaller longest side or a larger crop.",
                width.round(),
                height.round(),
                (width * height / 1e6).round(),
                MAX_WORKING_PIXELS / 1_000_000
            ),
        }
    }
}

impl std::error::Error for SizeError {}

/// A resolved size: the frame the film develops at and the crop's pixels in it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResolvedSize {
    /// The oriented, decoded source.
    pub source: (u32, u32),
    /// The whole frame, uncapped, that the film develops at.
    pub working: (u32, u32),
    /// The photograph: the crop's pixels in the working frame, before
    /// rotation, borders and export reductions. The working frame if uncropped.
    pub photo: (u32, u32),
}

impl ResolvedSize {
    /// The working frame differs from the source, so the source is resampled.
    pub fn resizes(&self) -> bool {
        self.working != self.source
    }
}

/// The working frame for `source` (width, height) and the request, and the
/// photograph `crop` cuts from it.
///
/// - `photo_long_side: None` keeps the legacy `factor` (`io.upscale_factor`)
///   with its existing f32 rounding; 1 is Original.
/// - `photo_long_side: Some(target)` scales the frame so the crop's longest
///   side is `target`, from the crop's extent only. `factor` must be 1.
///
/// Enlargements are bounded by [`MAX_WORKING_PIXELS`]; native size and
/// reductions of larger sources are not.
pub fn resolve(
    source: (u32, u32),
    factor: f32,
    photo_long_side: Option<u32>,
    crop: Option<&Crop>,
) -> Result<ResolvedSize, SizeError> {
    let (w0, h0) = source;
    if w0 == 0 || h0 == 0 {
        return Err(SizeError::EmptySource);
    }
    if !factor.is_finite() || factor <= 0.0 {
        return Err(SizeError::InvalidFactor(factor));
    }
    if let Some(crop) = crop {
        crop.validate().map_err(|e| SizeError::InvalidCrop(e.to_string()))?;
    }
    let (width, height) = match photo_long_side {
        Some(target) => {
            if !PHOTO_LONG_SIDE_RANGE.contains(&target) {
                return Err(SizeError::TargetOutOfRange(target));
            }
            if (factor - 1.0).abs() >= 1e-6 {
                return Err(SizeError::TargetWithFactor(factor));
            }
            let (cw, ch) = crop.map_or((1.0, 1.0), |c| (c.width, c.height));
            let scale = f64::from(target) / (cw * f64::from(w0)).max(ch * f64::from(h0));
            ((f64::from(w0) * scale).round().max(1.0), (f64::from(h0) * scale).round().max(1.0))
        }
        // Today's working-resolution rounding, in f32.
        None if (factor - 1.0).abs() < 1e-6 => (f64::from(w0), f64::from(h0)),
        None => (
            f64::from(((w0 as f32) * factor).round().max(1.0)),
            f64::from(((h0 as f32) * factor).round().max(1.0)),
        ),
    };
    let too_large = SizeError::TooLarge { width, height };
    let enlarges = width * height > f64::from(w0) * f64::from(h0);
    if !(width.is_finite() && height.is_finite())
        || width > f64::from(u32::MAX)
        || height > f64::from(u32::MAX)
        || (enlarges && width * height > MAX_WORKING_PIXELS as f64)
    {
        return Err(too_large);
    }
    let working = (width as u32, height as u32);
    // Every buffer of the working frame must be addressable.
    (working.0 as usize)
        .checked_mul(working.1 as usize)
        .and_then(|px| px.checked_mul(3 * size_of::<Scalar>()))
        .filter(|&bytes| bytes <= isize::MAX as usize)
        .ok_or(too_large)?;
    let photo = match crop {
        Some(crop) if crop.angle != 0.0 => crop.straightened_size(working.0, working.1),
        Some(crop) => {
            let (_, _, w, h) = crop.pixels(working.0, working.1);
            (w, h)
        }
        None => working,
    };
    Ok(ResolvedSize { source, working, photo })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crop(x: f64, y: f64, width: f64, height: f64, angle: f64) -> Crop {
        Crop { x, y, width, height, angle }
    }

    #[test]
    fn half_extent_crop_develops_the_whole_frame_at_twice_the_size() {
        // The plan's example: a central quarter of a 24 MP photograph at 6000 px.
        let centre = crop(0.25, 0.25, 0.5, 0.5, 0.0);
        let size = resolve((6000, 4000), 1.0, Some(6000), Some(&centre)).unwrap();
        assert_eq!(size.working, (12000, 8000));
        assert_eq!(size.photo, (6000, 4000));
        assert!(size.resizes());
        // Portrait, and the same crop straightened.
        let portrait = resolve((4000, 6000), 1.0, Some(6000), Some(&centre)).unwrap();
        assert_eq!((portrait.working, portrait.photo), ((8000, 12000), (4000, 6000)));
        let turned = crop(0.25, 0.25, 0.5, 0.5, 3.5);
        assert_eq!(resolve((6000, 4000), 1.0, Some(6000), Some(&turned)).unwrap().photo, (6000, 4000));
    }

    #[test]
    fn original_and_legacy_factors_keep_their_rounding() {
        let original = resolve((6000, 4000), 1.0, None, None).unwrap();
        assert_eq!((original.working, original.resizes()), ((6000, 4000), false));
        let half = resolve((6001, 4001), 0.5, None, None).unwrap();
        assert_eq!(half.working, (((6001f32 * 0.5).round()) as u32, ((4001f32 * 0.5).round()) as u32));
        assert_eq!(resolve((30, 20), 1.5, None, None).unwrap().working, (45, 30));
        assert_eq!(resolve((3, 1), 0.1, None, None).unwrap().working, (1, 1));
        // The crop never changes a legacy size, only the photograph.
        let cropped = resolve((6000, 4000), 0.5, None, Some(&crop(0.0, 0.0, 0.5, 1.0, 0.0))).unwrap();
        assert_eq!((cropped.working, cropped.photo), ((3000, 2000), (1500, 2000)));
    }

    #[test]
    fn the_crop_position_and_angle_never_change_the_working_size() {
        let at = |x: f64, y: f64, angle: f64| {
            resolve((5472, 3648), 1.0, Some(4321), Some(&crop(x, y, 0.37, 0.61, angle))).unwrap()
        };
        let first = at(0.0, 0.0, 0.0);
        for (x, y, angle) in [(0.3, 0.2, 0.0), (0.63, 0.39, 0.0), (0.1, 0.1, -7.0), (0.2, 0.15, 12.5)] {
            assert_eq!(at(x, y, angle).working, first.working);
        }
        // Rounding at the edges can move the photograph by a pixel, never more.
        for x in [0.0, 0.1, 0.2, 0.3, 0.33, 0.6] {
            let photo = at(x, 0.1, 0.0).photo;
            assert!(photo.0.max(photo.1).abs_diff(4321) <= 1, "{photo:?} at {x}");
        }
    }

    #[test]
    fn quarter_turns_keep_the_longest_side() {
        // A quarter turn swaps the photograph's sides after the pipeline.
        let wide = resolve((6000, 4000), 1.0, Some(3000), Some(&crop(0.1, 0.0, 0.4, 0.9, 0.0))).unwrap();
        let (w, h) = wide.photo;
        assert_eq!(w.max(h).abs_diff(3000), 0);
    }

    /// Deterministic pseudo-random crops, including tiny ones and edges on
    /// half pixels, never miss the target by more than a pixel.
    #[test]
    fn random_crops_land_within_a_pixel_of_the_target() {
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        for i in 0..20_000 {
            let source = (16 + (next() * 9000.0) as u32, 16 + (next() * 9000.0) as u32);
            let (cw, ch) = (0.002 + next() * 0.998, 0.002 + next() * 0.998);
            let (x, y) = (next() * (1.0 - cw), next() * (1.0 - ch));
            let angle = if i % 3 == 0 { (next() - 0.5) * 20.0 } else { 0.0 };
            let target = 16 + (next() * 12_000.0) as u32;
            let c = crop(x, y, cw, ch, angle);
            match resolve(source, 1.0, Some(target), Some(&c)) {
                Ok(size) => {
                    let long = size.photo.0.max(size.photo.1);
                    assert!(long.abs_diff(target) <= 1, "{source:?} {c:?} {target}: {size:?}");
                }
                Err(SizeError::TooLarge { width, height }) => {
                    assert!(width * height > MAX_WORKING_PIXELS as f64)
                }
                Err(e) => panic!("{e}"),
            }
        }
        // Half-pixel edges.
        // Half-pixel edges: 12.5 and 37.5 in a 50 px wide frame.
        let half = resolve((100, 80), 1.0, Some(25), Some(&crop(0.25, 0.25, 0.5, 0.25, 0.0))).unwrap();
        assert_eq!(half.working, (50, 40));
        assert!(half.photo.0.max(half.photo.1).abs_diff(25) <= 1, "{half:?}");
    }

    #[test]
    fn enlargements_are_bounded_before_allocation() {
        // 12 MP at 3× (108 MP) is admitted; the limit itself is the boundary.
        assert_eq!(resolve((4000, 3000), 1.0, Some(12_000), None).unwrap().working, (12_000, 9000));
        assert_eq!(resolve((16_000, 8000), 1.0, None, None).unwrap().working, (16_000, 8000));
        assert!(resolve((8000, 4000), 1.0, Some(16_000), None).is_ok()); // exactly 128 MP
        assert!(matches!(resolve((8000, 4000), 1.0, Some(16_002), None), Err(SizeError::TooLarge { .. })));
        // The same target from a smaller crop needs a much larger frame.
        let small = crop(0.4, 0.4, 0.2, 0.2, 0.0);
        assert!(matches!(resolve((4000, 3000), 1.0, Some(6000), Some(&small)), Err(SizeError::TooLarge { .. })));
        // Legacy enlargements are bounded too.
        assert!(matches!(resolve((10_000, 10_000), 1.2, None, None), Err(SizeError::TooLarge { .. })));
        // Sources above the limit develop at their own size or smaller.
        assert_eq!(resolve((16_000, 10_000), 1.0, None, None).unwrap().working, (16_000, 10_000));
        assert_eq!(resolve((16_000, 10_000), 1.0, Some(15_000), None).unwrap().working, (15_000, 9375));
        // A crop of a vanishing extent can't overflow the frame's arithmetic.
        let speck = crop(0.5, 0.5, 1e-12, 1e-12, 0.0);
        assert!(matches!(resolve((6000, 4000), 1.0, Some(65_535), Some(&speck)), Err(SizeError::TooLarge { .. })));
        let message = resolve((4000, 3000), 1.0, Some(6000), Some(&small)).unwrap_err().to_string();
        assert!(message.contains("30000 × 22500 px frame (675 MP)") && message.contains("128 MP limit"), "{message}");
    }

    #[test]
    fn invalid_requests_fail() {
        assert_eq!(resolve((0, 10), 1.0, None, None), Err(SizeError::EmptySource));
        assert_eq!(resolve((10, 10), 1.0, Some(15), None), Err(SizeError::TargetOutOfRange(15)));
        assert_eq!(resolve((10, 10), 1.0, Some(65_536), None), Err(SizeError::TargetOutOfRange(65_536)));
        assert_eq!(resolve((10, 10), 0.5, Some(100), None), Err(SizeError::TargetWithFactor(0.5)));
        for factor in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(matches!(resolve((10, 10), factor, None, None), Err(SizeError::InvalidFactor(_))));
        }
        let outside = crop(0.6, 0.0, 0.5, 1.0, 0.0);
        assert!(matches!(resolve((10, 10), 1.0, Some(100), Some(&outside)), Err(SizeError::InvalidCrop(_))));
    }
}
