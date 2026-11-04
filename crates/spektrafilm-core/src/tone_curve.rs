//! The film-and-print tone curve, measured by developing a neutral ramp.
//!
//! The macOS zone readout asks how far the print's tones would move if the
//! scene moved: which exposure placement lands a picked tone on each zone.
//! The answer is the inverse of the whole development — film, print and the
//! finishing edits — so this module develops a ladder of neutral scene values
//! once per edit and returns the output L\* at each. Swift inverts it.
//!
//! The samples are log-spaced (1/16 EV apart, −16…+8 EV) so the shadows get
//! the resolution a 65K linear ramp would give at a fraction of the cost:
//! ten full CLI runs, including process start, took 0.4 s.

use crate::params::RuntimeParams;
use crate::pipeline::{clip_to_display, Pipeline};
use crate::post::{luminance_weights, PostParams, PostProcess};
use spektrafilm_gpu::cpu_backend::CpuBackend;
use spektrafilm_io::encoding::Encoding;
use spektrafilm_math::{colorspace::RgbColourspace, image::ImageBuf, precision::from_f64};

/// The first sample's scene value, in stops from 18 % grey.
pub const EV_MIN: f64 = -16.0;
/// The spacing between samples, in stops.
pub const EV_STEP: f64 = 0.0625;
/// The number of samples: −16 … +8 EV inclusive.
pub const SAMPLES: usize = 385;

/// One measurement of the curve: the output L\* at every sample, and the
/// output-encoded green channel, which the EV scale places its ticks by.
pub struct ToneRamp {
    pub lstar: Vec<f32>,
    /// The green channel of the developed ramp, output-encoded and clipped
    /// to [0, 1]. The scale ticks line up with the bars they describe.
    pub green: Vec<f32>,
}

/// The params the ramp develops with: the photograph's, without what is
/// spatial or random.
///
/// This is deliberately not [`crate::lut_export::per_pixel_params`]: that
/// clears the print filter shifts, the preflash and the scanner correction,
/// and turns off auto exposure, all of which change the print this curve
/// describes. The colour fields and LUT settings stay as they are, because
/// they belong to the input and are part of the preview's pipeline key.
pub fn ramp_params(mut params: RuntimeParams) -> RuntimeParams {
    params.camera.lens_blur_um = 0.0;
    params.camera.diffusion_filter.active = false;
    params.film_render.grain.active = false;
    // Halation's `boost_ev` is a per-pixel effect and stays.
    params.film_render.halation.active = false;
    params.film_render.dir_couplers.diffusion_size_um = 0.0;
    params.film_render.dir_couplers.diffusion_tail_um = 0.0;
    params.film_render.glare.active = false;
    params.enlarger.lens_blur = 0.0;
    params.enlarger.diffusion_filter.active = false;
    params.print_render.glare.active = false;
    params.scanner.lens_blur = 0.0;
    params.scanner.unsharp_mask = [0.0, 0.0];
    // The ramp is one column of pixels: the photograph's framing never
    // belongs to it. Image Size is a working-resolution setting, not tone.
    params.io.crop = false;
    params.io.rotate_cw = 0;
    params.io.upscale_factor = 1.0;
    params.io.photo_long_side_px = None;
    // The finishing edits are applied by `measure` after the scan, as the
    // preview applies them.
    params.post = PostParams::default();
    params
}

/// CIE Lab lightness from a linear Y, on the 216/24389 breakpoint.
fn lstar(y: f64) -> f64 {
    if y > 216.0 / 24389.0 {
        116.0 * y.cbrt() - 16.0
    } else {
        y * 24389.0 / 27.0
    }
}

/// The CIE Lab lightness of every sample of the neutral ramp, plus the
/// output-encoded green channel each sample develops to.
///
/// `auto_ev` is the frame's metered auto exposure: the ramp is a frame of the
/// same photograph, so it gets the same exposure. `post` is applied after the
/// scan is clipped, as the preview applies it.
pub fn measure(pipeline: &Pipeline, post: &PostParams, auto_ev: f64) -> ToneRamp {
    let params = ramp_params(pipeline.params.clone());
    // `with_params` re-derives the print exposure and the film presets, so
    // the ramp is developed exactly as the preview's pipeline is.
    let pipeline = pipeline.clone().with_params(params.clone());
    let encoding = Encoding::new(&params.io.output_color_space, params.io.output_cctf_encoding);
    let input = RgbColourspace::input(&params.io.input_color_space).cctf;
    let mut image = ImageBuf::new(SAMPLES as u32, 1);
    for index in 0..SAMPLES {
        let linear = 2f64.powf(EV_MIN + index as f64 * EV_STEP);
        // The loader hands the pipeline linear values unless the input is
        // read manually with its curve.
        let value = if params.io.input_cctf_decoding {
            input.encode(linear)
        } else {
            linear
        };
        let value = from_f64(value);
        image.set(index as u32, 0, [value, value, value]);
    }
    let mut output = pipeline.with_autoexposure_ev(auto_ev).process(image, &CpuBackend);
    clip_to_display(&mut output, encoding.encoded);
    if !post.is_identity() {
        PostProcess::new(post, encoding.encoded, &params.io.output_color_space).apply(&mut output);
    }
    let weights = luminance_weights(&params.io.output_color_space);
    let mut lstar_samples = Vec::with_capacity(SAMPLES);
    let mut green = Vec::with_capacity(SAMPLES);
    for index in 0..SAMPLES {
        let rgb = output.get(index as u32, 0);
        let linear = rgb.map(|v| {
            if encoding.encoded {
                encoding.space.cctf.decode(f64::from(v))
            } else {
                f64::from(v)
            }
        });
        let y = (0..3).map(|c| f64::from(weights[c]) * linear[c]).sum::<f64>();
        lstar_samples.push(lstar(y) as f32);
        green.push(rgb[1].clamp(0.0, 1.0));
    }
    ToneRamp { lstar: lstar_samples, green }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ramp_is_log_spaced_over_385_samples() {
        assert_eq!(SAMPLES, 385);
        assert!((EV_MIN + (SAMPLES - 1) as f64 * EV_STEP - 8.0).abs() < 1e-9);
    }

    #[test]
    fn lstar_follows_the_cie_breakpoint() {
        assert!((lstar(1.0) - 100.0).abs() < 1e-9);
        assert!((lstar(0.18) - 49.496).abs() < 0.01);
        assert!(lstar(0.0).abs() < 1e-12);
        let below = 0.008;
        assert!((lstar(below) - below * 24389.0 / 27.0).abs() < 1e-12);
    }
}
