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
    orientation: u32,
    oriented: u32,
    full_width: u32,
    full_height: u32,
    major: i32,
    declined: [c_char; 768],
    version: [c_char; 32],
    error: [c_char; 768],
}

unsafe extern "C" {
    fn sf_apple_raw_decode(
        path: *const c_char,
        decoder_major: i32,
        orient: i32,
        as_shot: i32,
        lens_correction: i32,
        defer_orientation: i32,
        max_long_side: u32,
        own_lens_profile: i32,
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

/// An exact generation for compatibility diagnostics. Normal imports use
/// AppleRawDecoder, which tries 9 and then 8 for every individual file.
pub(crate) struct AppleRawVersion(pub i32);

/// The generation a decode chose (0: none yet) and, when it chose 8 because
/// the file has no RAW 9, why 9 was declined.
#[derive(Default)]
struct Attempt {
    used: i32,
    declined: Option<String>,
}

fn generation_name(major: i32) -> &'static str {
    if major == 9 { "Apple RAW 9" } else { "Apple RAW 8" }
}

impl RawDecoder for AppleRawDecoder {
    fn name(&self) -> &'static str {
        "apple"
    }

    /// RAW 9 where this file's filter offers it, else RAW 8, chosen from one
    /// filter: asking for 9 first opened the file twice whenever it had no
    /// RAW 9 (~50 ms warm, ~180 ms cold). Otherwise as
    /// `decode_compatible(&[9, 8])`: a RAW 9 decode that fails later is tried
    /// with 8, fatal errors stop, and the failures read the same.
    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<RawImage> {
        let mut attempt = Attempt::default();
        let first = decode_generation(0, path, options, &mut attempt);
        let mut failures = Vec::new();
        if let Some(declined) = attempt.declined {
            let error: anyhow::Error = UnsupportedRaw(declined).into();
            failures.push(format!("{}: {error:#}", generation_name(9)));
        }
        // Before a generation was chosen it failed as asking for 9 would.
        let tried = if attempt.used == 8 { 8 } else { 9 };
        match first {
            Ok(raw) => return Ok(raw),
            Err(error) if error.is::<UnsupportedRaw>() => failures.push(format!("{}: {error:#}", generation_name(tried))),
            Err(error) => return Err(error.context(format!("{} failed", generation_name(tried)))),
        }
        if tried == 9 {
            match AppleRawVersion(8).decode(path, options) {
                Ok(raw) => return Ok(raw),
                Err(error) if error.is::<UnsupportedRaw>() => failures.push(format!("{}: {error:#}", generation_name(8))),
                Err(error) => return Err(error.context(format!("{} failed", generation_name(8)))),
            }
        }
        Err(UnsupportedRaw(failures.join("; ")).into())
    }
}

impl RawDecoder for AppleRawVersion {
    fn name(&self) -> &'static str {
        generation_name(self.0)
    }

    fn decode(&self, path: &Path, options: RawDecodeOptions) -> Result<RawImage> {
        decode_generation(self.0, path, options, &mut Attempt::default())
    }
}

/// Apple's own lens correction where it supports the file; otherwise the
/// corrections Negs reads itself ([`crate::lens`]), applied in sensor
/// coordinates before orientation as on the rawler path. `major` 9 or 8, or
/// 0 for 9 if the file offers it, else 8 (`attempt` says which).
fn decode_generation(major: i32, path: &Path, options: RawDecodeOptions, attempt: &mut Attempt) -> Result<RawImage> {
    options.white_balance.validate()?;
    // Read before decoding, so Apple can leave such a file unoriented, and
    // so a reduced read stays whole for a file Negs corrects itself.
    let profile_read = options.lens_correction.enabled || options.max_long_side > 0;
    let profile = if profile_read { crate::lens::read_profile(path) } else { None };
    let own_lens = profile.is_some() && options.lens_correction.enabled;
    let file = path;
    let path = CString::new(path.as_os_str().as_bytes())?;
    let mut native = NativeImage {
        pixels: std::ptr::null_mut(),
        width: 0,
        height: 0,
        monochrome: 0,
        lens_correction_supported: 0,
        orientation: 1,
        oriented: 0,
        full_width: 0,
        full_height: 0,
        major: 0,
        declined: [0; 768],
        version: [0; 32],
        error: [0; 768],
    };
    let status = unsafe {
        sf_apple_raw_decode(
            path.as_ptr(),
            major,
            options.apply_orientation.into(),
            (options.white_balance.mode == crate::RawWhiteBalanceMode::AsShot).into(),
            options.lens_correction.enabled.into(),
            own_lens.into(),
            options.max_long_side,
            profile.is_some().into(),
            &mut native,
        )
    };
    // The native strings are bounded and snprintf-terminated.
    attempt.used = native.major;
    if native.declined[0] != 0 {
        attempt.declined = Some(unsafe { CStr::from_ptr(native.declined.as_ptr()) }.to_string_lossy().into_owned());
    }
    if status != 0 {
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
    if !rgba.par_chunks_exact(4).all(|p| p[..3].iter().all(|v| v.is_finite())) {
        return Err(UnsupportedRaw("Apple RAW returned non-finite RGB values".into()).into());
    }
    let mut rgb = vec![0.0f32; count * 3];
    rgb.par_chunks_exact_mut(3)
        .zip(rgba.par_chunks_exact(4))
        .for_each(|(out, pixel)| out.copy_from_slice(&pixel[..3]));
    let (width, height, monochrome) = (native.width, native.height, native.monochrome != 0);
    let mut lens_correction_supported = native.lens_correction_supported != 0;
    // Orientation still to apply: Apple left it to us (deferred) or the
    // caller asked for none.
    let pending = if options.apply_orientation && native.oriented == 0 { native.orientation as u16 } else { 1 };
    // Apple rotated the pixels by a quarter turn, so its unoriented extent
    // is transposed.
    let transposed = native.oriented != 0 && native.orientation >= 5;
    // Upright, as the image will be once `pending` is applied.
    let full = if pending >= 5 { (native.full_height, native.full_width) } else { (native.full_width, native.full_height) };
    let reduced_from = ((native.full_width, native.full_height) != (native.width, native.height)).then_some(full);
    let decoder_version = Some(unsafe { CStr::from_ptr(native.version.as_ptr()) }
        .to_string_lossy().into_owned());
    drop(native);
    // Unclamped: colours outside sRGB arrive as negative coordinates and
    // the spectral model takes them; only non-finite values are refused.
    crate::white_balance::finish_rgb(&mut rgb, options.white_balance, monochrome)?;
    #[cfg(not(feature = "precision-f64"))]
    let data: Vec<Scalar> = rgb;
    #[cfg(feature = "precision-f64")]
    let data: Vec<Scalar> = rgb.into_par_iter().map(from_f32).collect();
    let mut image = ImageBuf::from_data(width, height, data);
    if !lens_correction_supported {
        let profile = if profile_read { profile } else { crate::lens::read_profile(file) };
        if let Some(profile) = profile {
            let (w, h) = (image.width as usize, image.height as usize);
            let size = if transposed { (h, w) } else { (w, h) };
            if let Some(area) = crate::lens_placement(file, &profile, size) {
                lens_correction_supported = true;
                // Enabled with a profile means Apple left the image unoriented.
                if options.lens_correction.enabled {
                    crate::lens::apply(&mut image, &profile, area);
                }
            }
        }
    }
    Ok(RawImage {
        image: image.oriented(pending),
        reduced_from,
        lens_correction_supported,
        decoder_version,
    })
}
