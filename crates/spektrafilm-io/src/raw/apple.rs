use std::{
    ffi::{CStr, CString, c_char},
    os::unix::ffi::OsStrExt,
    path::Path,
};

use anyhow::{Result, bail, ensure};
use rayon::prelude::*;
#[cfg(feature = "precision-f64")]
use spektrafilm_math::precision::from_f32;
use spektrafilm_math::{image::ImageBuf, precision::Scalar};

use super::{RawDecodeOptions, RawDecoder, RawImage, UnsupportedRaw};

#[repr(C)]
struct NativeImage {
    pixels: *mut f32,
    width: u32,
    height: u32,
    monochrome: u32,
    lens_correction_supported: u32,
    version: [c_char; 32],
    error: [c_char; 768],
}

unsafe extern "C" {
    fn sf_apple_raw_decode(
        path: *const c_char,
        orient: i32,
        as_shot: i32,
        lens_correction: i32,
        output: *mut NativeImage,
    ) -> i32;
    fn sf_apple_raw_free(pixels: *mut f32);
}

impl Drop for NativeImage {
    fn drop(&mut self) {
        // The bridge always initializes this pointer to a malloc-owned buffer
        // or null; ownership never transfers to Rust's allocator.
        unsafe { sf_apple_raw_free(self.pixels) };
    }
}

pub struct AppleRawDecoder;

impl RawDecoder for AppleRawDecoder {
    fn name(&self) -> &'static str {
        "apple"
    }

    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<RawImage> {
        options.white_balance.validate()?;
        let path = CString::new(path.as_os_str().as_bytes())?;
        let mut native = NativeImage {
            pixels: std::ptr::null_mut(),
            width: 0,
            height: 0,
            monochrome: 0,
            lens_correction_supported: 0,
            version: [0; 32],
            error: [0; 768],
        };
        let status = unsafe {
            sf_apple_raw_decode(
                path.as_ptr(),
                options.apply_orientation.into(),
                (options.white_balance.mode == crate::RawWhiteBalanceMode::AsShot).into(),
                options.lens_correction.enabled.into(),
                &mut native,
            )
        };
        if status != 0 {
            // Both native strings are bounded and snprintf-terminated.
            let error = unsafe { CStr::from_ptr(native.error.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            if status == 1 {
                return Err(UnsupportedRaw(error).into());
            }
            bail!("Apple RAW decode failed: {error}");
        }
        let count = (native.width as usize)
            .checked_mul(native.height as usize)
            .filter(|&n| n > 0 && n <= 200_000_000)
            .ok_or_else(|| anyhow::anyhow!("Invalid Apple RAW dimensions"))?;
        ensure!(!native.pixels.is_null(), "Apple RAW returned no pixels");
        // Successful native rendering owns exactly width*height*4 float32
        // components. Drop releases the buffer on every Rust error path.
        let rgba = unsafe { std::slice::from_raw_parts(native.pixels, count * 4) };
        ensure!(
            rgba.par_chunks_exact(4)
                .all(|p| p[..3].iter().all(|v| v.is_finite())),
            "Apple RAW returned non-finite RGB values"
        );
        let mut rgb = vec![0.0f32; count * 3];
        rgb.par_chunks_exact_mut(3)
            .zip(rgba.par_chunks_exact(4))
            .for_each(|(out, pixel)| out.copy_from_slice(&pixel[..3]));
        let (width, height, monochrome) = (native.width, native.height, native.monochrome != 0);
        let lens_correction_supported = native.lens_correction_supported != 0;
        drop(native);
        // Unclamped: colours outside sRGB arrive as negative coordinates and
        // the spectral model takes them; only non-finite values are refused.
        crate::white_balance::finish_rgb(&mut rgb, options.white_balance, monochrome)?;
        #[cfg(not(feature = "precision-f64"))]
        let data: Vec<Scalar> = rgb;
        #[cfg(feature = "precision-f64")]
        let data: Vec<Scalar> = rgb.into_par_iter().map(from_f32).collect();
        Ok(RawImage {
            image: ImageBuf::from_data(width, height, data),
            lens_correction_supported,
        })
    }
}
