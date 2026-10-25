//! Image Size for the pipeline's callers: resolve the request in
//! `io.upscale_factor`/`io.photo_long_side_px`, resize the source once with
//! [`spektrafilm_io::resize::catmull_rom`], and clear both so nothing resizes
//! it again. The CLI, the native helper and [`crate::pipeline::Pipeline`]
//! share this, so an export and a preview develop the same frame.
use spektrafilm_io::crop::Crop;
use spektrafilm_io::image_size::{self, ResolvedSize, SizeError};
use spektrafilm_io::resize;
use spektrafilm_math::image::ImageBuf;

use crate::params::RuntimeParams;

/// The working frame `params` asks for from a `source` (width, height), and
/// the photograph `crop` cuts from it. Allocates nothing.
pub fn resolve(
    params: &RuntimeParams,
    source: (u32, u32),
    crop: Option<&Crop>,
) -> Result<ResolvedSize, SizeError> {
    image_size::resolve(
        source,
        params.io.upscale_factor,
        params.io.photo_long_side_px,
        crop,
    )
}

/// Marks `params` as describing an image already at its working size.
pub fn clear(params: &mut RuntimeParams) {
    params.io.upscale_factor = 1.0;
    params.io.photo_long_side_px = None;
}

/// `image` at the working size `params` asks for, resampled at most once,
/// with `params` cleared to match. The request is validated before anything
/// is allocated at the new size, and `image` is released once resampled.
pub fn prepare(
    image: ImageBuf,
    params: &mut RuntimeParams,
    crop: Option<&Crop>,
) -> Result<(ImageBuf, ResolvedSize), SizeError> {
    let size = resolve(params, (image.width, image.height), crop)?;
    clear(params);
    if !size.resizes() {
        return Ok((image, size));
    }
    let working = resize::catmull_rom(&image, size.working);
    tracing::info!(
        from = format!("{}x{}", image.width, image.height),
        to = format!("{}x{}", size.working.0, size.working.1),
        "image size: resized the source"
    );
    Ok((working, size))
}

#[cfg(test)]
mod tests {
    use super::*;
    use spektrafilm_math::precision::from_f64;

    #[test]
    fn prepare_resizes_once_and_clears_the_request() {
        let image = ImageBuf::from_data(60, 40, vec![from_f64(1.75); 60 * 40 * 3]);
        let mut params = RuntimeParams::default();
        params.io.photo_long_side_px = Some(60);
        let crop = Crop { x: 0.2, y: 0.25, width: 0.5, height: 0.5, angle: 0.0 };
        let (working, size) = prepare(image, &mut params, Some(&crop)).unwrap();
        assert_eq!((working.width, working.height), (120, 80));
        assert_eq!(size.photo, (60, 40));
        assert_eq!((params.io.upscale_factor, params.io.photo_long_side_px), (1.0, None));
        // Highlights survive the enlargement.
        assert!(working.data.iter().all(|&v| (v as f64 - 1.75).abs() < 1e-5));
        // Prepared params ask for nothing more.
        let (again, _) = prepare(working.clone(), &mut params, Some(&crop)).unwrap();
        assert_eq!((again.width, again.height), (120, 80));
    }

    #[test]
    fn invalid_requests_leave_the_params_alone() {
        let image = ImageBuf::from_data(4, 4, vec![from_f64(0.5); 48]);
        let mut params = RuntimeParams::default();
        params.io.upscale_factor = 0.5;
        params.io.photo_long_side_px = Some(100);
        assert!(matches!(
            prepare(image, &mut params, None),
            Err(SizeError::TargetWithFactor(_))
        ));
        assert_eq!(params.io.photo_long_side_px, Some(100));
    }
}
