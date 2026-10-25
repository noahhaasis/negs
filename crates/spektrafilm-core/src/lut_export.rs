//! A portable, per-pixel version of the current film/paper look.
use std::{fmt::Write, path::Path};

use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_math::{image::ImageBuf, precision::from_f64};

use crate::{params::RuntimeParams, pipeline::Pipeline, profile};

/// Keep color and tone controls, removing operations that depend on an image's
/// dimensions, other pixels, or random noise. DIR's color inhibition is retained;
/// only its spatial diffusion is removed. Post-tab edits are outside the film look.
pub fn per_pixel_params(mut params: RuntimeParams) -> RuntimeParams {
    params.camera.auto_exposure = false;
    params.camera.lens_blur_um = 0.0;
    params.camera.diffusion_filter.active = false;
    params.film_render.grain.active = false;
    params.film_render.halation.active = false;
    params.film_render.halation.boost_ev = 0.0;
    params.film_render.dir_couplers.diffusion_size_um = 0.0;
    params.film_render.dir_couplers.diffusion_tail_um = 0.0;
    params.film_render.glare.active = false;
    params.enlarger.lens_blur = 0.0;
    params.enlarger.diffusion_filter.active = false;
    params.print_render.glare.active = false;
    params.scanner.lens_blur = 0.0;
    params.scanner.unsharp_mask = [0.0, 0.0];
    params.io.input_color_space = "sRGB".into();
    // Like the PNG loader, cube() decodes sRGB before calling the pipeline.
    // The engine's input_cctf_decoding field does not perform that conversion.
    params.io.input_cctf_decoding = false;
    params.io.output_color_space = "sRGB".into();
    params.io.output_cctf_encoding = true;
    params.io.upscale_factor = 1.0;
    params.io.photo_long_side_px = None;
    params.io.crop = false;
    params.io.rotate_cw = 0;
    params.settings.preview_mode = false;
    params.post = Default::default();
    params
}

fn grid(size: u32) -> Result<ImageBuf, String> {
    if !(2..=129).contains(&size) {
        return Err("LUT size must be between 2 and 129".into());
    }
    let mut image = ImageBuf::new(size, size * size);
    // CUBE order: red changes fastest, then green, then blue.
    for (i, pixel) in image.pixels_mut().enumerate() {
        let s = size as usize;
        let rgb = [i % s, (i / s) % s, i / (s * s)];
        for c in 0..3 {
            pixel[c] = from_f64(rgb[c] as f64 / (size - 1) as f64);
        }
    }
    Ok(image)
}

/// Build one spectral pipeline and render the complete grid in one pass.
/// No photograph or GPU is required. The caller commits the completed text to disk.
pub fn cube(
    data: &Path,
    film: &str,
    paper: &str,
    params: RuntimeParams,
    size: u32,
) -> Result<String, String> {
    let mut input = grid(size)?;
    for value in &mut input.data {
        *value = spektrafilm_math::precision::srgb_decode(*value);
    }
    let params = per_pixel_params(params);
    let scan_film = params.io.scan_film;
    let print = if scan_film { film } else { paper };
    let pipeline = Pipeline::new_with_spectral(
        profile::load_profile_by_name(data, film).map_err(|e| e.to_string())?,
        profile::load_profile_by_name(data, print).map_err(|e| e.to_string())?,
        params,
        data,
    )?;
    let output = pipeline.process(input, &CpuBackend);
    let title = format!(
        "Negs {film} - {}",
        if scan_film { "direct scan" } else { paper }
    )
    .replace(['"', '\n', '\r'], " ");
    let mut text = format!(
        "# Created by Negs / spektrafilm\n\
         # Input: sRGB encoded, range 0..1. Output: sRGB encoded.\n\
         # Uses the current film/paper color and tone settings, including manual exposure.\n\
         # Excludes auto exposure, grain, halation, scatter, highlight boost, glare,\n\
         # diffusion, lens blur, sharpening, RAW development, resizing and Post-tab edits.\n\
         # DIR color inhibition is retained without spatial diffusion.\n\
         TITLE \"{title}\"\nLUT_3D_SIZE {size}\n\
         DOMAIN_MIN 0.0 0.0 0.0\nDOMAIN_MAX 1.0 1.0 1.0\n\n"
    );
    for pixel in output.pixels() {
        if pixel.iter().any(|v| !v.is_finite()) {
            return Err("The selected settings produced non-finite LUT values".into());
        }
        writeln!(text, "{:.8} {:.8} {:.8}", pixel[0], pixel[1], pixel[2]).unwrap();
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cube_grid_has_red_fastest_and_rejects_unsafe_sizes() {
        for size in [0, 1, 130, u32::MAX] {
            assert!(grid(size).is_err());
        }
        let grid = grid(3).unwrap();
        assert_eq!((grid.width, grid.height), (3, 9));
        assert_eq!(grid.get(0, 0), [from_f64(0.0); 3]);
        assert_eq!(
            grid.get(1, 0),
            [from_f64(0.5), from_f64(0.0), from_f64(0.0)]
        );
        assert_eq!(
            grid.get(0, 1),
            [from_f64(0.0), from_f64(0.5), from_f64(0.0)]
        );
        assert_eq!(
            grid.get(0, 3),
            [from_f64(0.0), from_f64(0.0), from_f64(0.5)]
        );
        assert_eq!(grid.get(2, 8), [from_f64(1.0); 3]);
    }
}
