/// Filming stage: expose the digital image onto virtual film.
///
/// Full Hanatos2025 path: RGB → XYZ → xy chromaticity → tc coordinates →
/// 2D LUT lookup (spectra × sensitivity) → per-channel film raw exposure.
use rayon::prelude::*;
use spektrafilm_gpu::ComputeBackend;
use spektrafilm_math::colorspace::{self, RgbColourspace};
use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::{from_f32, from_f64, to_f64};
use spektrafilm_math::spectral::TcLut;
use std::time::Instant;

use crate::params::RuntimeParams;
use crate::profile::Profile;

fn stage_timings_enabled() -> bool {
    std::env::var_os("SPEKTRAFILM_STAGE_TIMINGS").is_some()
}

fn print_stage_timing(enabled: bool, stage: &str, start: Instant) {
    if enabled {
        eprintln!("stage {stage}: {} ms", start.elapsed().as_millis());
    }
}

/// `2^x` in f32, computed in f64 and rounded once. The f32 libm functions
/// differ by an ulp between platforms (MSVC vs Apple), and an exposure scale
/// one ulp off shifts every pixel of the image. The f64 result is far more
/// accurate than an f32 ulp, so it rounds to the same f32 everywhere. Apple's
/// `powf(2, x)` is not always correctly rounded either: on macOS the scale
/// moves by one ulp for about 0.6% of exposures. The auto-exposure scale is
/// rounded from f64 the same way.
fn exp2_f32(x: f32) -> f32 {
    (x as f64).exp2() as f32
}

/// Compute pixel size in micrometers from film format and image dimensions.
pub fn pixel_size_um(film_format_mm: f32, width: u32, height: u32) -> f32 {
    film_format_mm * 1000.0 / width.max(height) as f32
}

/// The pixel size the grain model sees, in f64 as upstream computes it: the
/// particle counts scale with its square.
pub fn grain_pixel_size_um(params: &RuntimeParams, width: u32, height: u32) -> f64 {
    params.camera.film_format_mm as f64 * 1000.0 / width.max(height) as f64
}


/// The auto-exposure metering frame: upstream's `_compute_small_preview`
/// (`runtime/services/resize.py`), which `ResizingService.crop_and_rescale`
/// takes from the full frame before any crop or resize, so the metered
/// exposure doesn't move with the crop or the working size. Frames with a
/// long side above 256 px go through `skimage.transform.rescale(order=0)`:
/// the output size rounds half to even, a Gaussian anti-aliasing filter of
/// `σ = (factor − 1)/2` per axis (`scipy.ndimage`, mirror edges, 4σ radius)
/// runs before the nearest-neighbour sampling, and the result is clipped to
/// the input's range. The filter is only evaluated at the sampled pixels.
pub fn metering_preview(image: &ImageBuf) -> ImageBuf {
    const MAX_SIZE: f64 = 256.0;
    let (w, h) = (image.width as usize, image.height as usize);
    let max_dim = w.max(h);
    if max_dim as f64 <= MAX_SIZE {
        return image.clone();
    }
    let scale = MAX_SIZE / max_dim as f64;
    let out_dim = |dim: usize| ((scale * dim as f64).round_ties_even() as usize).max(1);
    let (out_w, out_h) = (out_dim(w), out_dim(h));
    // scipy `zoom(grid_mode=True, order=0)`: the source of output `o` is
    // `floor((o + 0.5)·in/out − 0.5 + 0.5)`.
    let axis = |out: usize, dim: usize| -> (Vec<usize>, Vec<f64>) {
        let zoom = dim as f64 / out as f64;
        let index = (0..out)
            .map(|o| {
                let cc = (o as f64 + 0.5) * zoom - 0.5;
                mirror_index((cc + 0.5).floor() as isize, dim)
            })
            .collect();
        (index, gaussian_kernel(((zoom - 1.0) / 2.0).max(0.0)))
    };
    let (rows, row_kernel) = axis(out_h, h);
    let (cols, col_kernel) = axis(out_w, w);
    let pixel = |y: usize, x: usize, c: usize| to_f64(image.data[(y * w + x) * 3 + c]);
    // scipy `correlate1d` with a symmetric kernel: the centre tap, then the
    // pairs from the outermost in.
    let correlate = |kernel: &[f64], at: usize, dim: usize, sample: &dyn Fn(usize) -> f64| {
        let r = kernel.len() / 2;
        let mut acc = sample(at) * kernel[r];
        for j in (1..=r).rev() {
            let lo = mirror_index(at as isize - j as isize, dim);
            let hi = mirror_index(at as isize + j as isize, dim);
            acc += (sample(lo) + sample(hi)) * kernel[r - j];
        }
        acc
    };
    // Axis 0 (rows) at the sampled rows, every column.
    let filtered_rows: Vec<f64> = rows
        .par_iter()
        .flat_map_iter(|&y| {
            let correlate = &correlate;
            let row_kernel = &row_kernel;
            (0..w).flat_map(move |x| {
                (0..3).map(move |c| correlate(row_kernel, y, h, &|yy| pixel(yy, x, c)))
            })
        })
        .collect();
    let (lo, hi) = image
        .data
        .iter()
        .map(|&v| to_f64(v))
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
    let mut data = Vec::with_capacity(out_w * out_h * 3);
    for r in 0..out_h {
        let row = &filtered_rows[r * w * 3..(r + 1) * w * 3];
        for &x in &cols {
            for c in 0..3 {
                let v = correlate(&col_kernel, x, w, &|xx| row[xx * 3 + c]);
                data.push(from_f64(v.clamp(lo, hi)));
            }
        }
    }
    ImageBuf::from_data(out_w as u32, out_h as u32, data)
}

/// `scipy.ndimage` "mirror" extension (`d c b | a b c d | c b a`).
fn mirror_index(i: isize, dim: usize) -> usize {
    if dim == 1 {
        return 0;
    }
    let period = 2 * (dim as isize - 1);
    let i = i.rem_euclid(period);
    (if i >= dim as isize { period - i } else { i }) as usize
}

/// `scipy.ndimage._gaussian_kernel1d` at radius `int(4σ + 0.5)`; the identity
/// for σ ≤ 1e-15, which `gaussian_filter` skips.
fn gaussian_kernel(sigma: f64) -> Vec<f64> {
    if sigma <= 1e-15 {
        return vec![1.0];
    }
    let radius = (4.0 * sigma + 0.5) as i64;
    let factor = -0.5 / (sigma * sigma);
    let phi: Vec<f64> = (-radius..=radius).map(|x| (factor * (x * x) as f64).exp()).collect();
    let sum: f64 = phi.iter().sum();
    phi.iter().map(|v| v / sum).collect()
}

/// Auto-exposure compensation on film raw, upstream experimental's
/// `measure_raw_autoexposure_ev` (`utils/autoexposure.py`): the field is the
/// mean of the raw channels, metered with `method`, and the EV brings it to
/// `target` (1, where a mid-grey subject lands). `method` is one of
/// `average`, `median`, `center_weighted`, `partial`, `matrix`,
/// `multi_zone`, `highlight_weighted`; anything else, or a meter reading
/// that isn't positive and finite, gives 0 EV.
pub fn measure_raw_autoexposure_ev(raw: &ImageBuf, method: &str, target: f64) -> f64 {
    let field: Vec<f64> = raw
        .data
        .par_chunks_exact(3)
        .map(|p| raw_field_value([to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]))
        .collect();
    measure_field_autoexposure_ev(&field, raw.width as usize, raw.height as usize, method, target)
}

/// The metering field value of one raw pixel: the channel mean, summed in
/// numpy's order.
pub fn raw_field_value(raw: [f64; 3]) -> f64 {
    (raw[0] + raw[1] + raw[2]) / 3.0
}

/// [`measure_raw_autoexposure_ev`] on a ready `width` × `height` field
/// (upstream `_exposure_compensation_ev(_metered_value(field, method))`).
pub fn measure_field_autoexposure_ev(
    field: &[f64],
    width: usize,
    height: usize,
    method: &str,
    target: f64,
) -> f64 {
    let Some(metered) = meter_field(field, width, height, method) else {
        return 0.0;
    };
    let ev = -(metered / target).log2();
    if ev.is_finite() { ev } else { 0.0 }
}

/// Normalized pixel coordinate along an axis: `(i/dim - 0.5) * (dim/maxdim)`,
/// so the long edge spans [-0.5, 0.5] (Python `_normalized_coords`).
fn norm_coord(i: usize, dim: usize, max_dim: usize) -> f64 {
    (i as f64 / dim as f64 - 0.5) * (dim as f64 / max_dim as f64)
}

/// Meter a field with the named pattern (upstream `_metered_value`), `None`
/// for an unknown method.
fn meter_field(lum: &[f64], sw: usize, sh: usize, method: &str) -> Option<f64> {
    let max_dim = sw.max(sh);
    let value = match method {
        "average" => lum.iter().sum::<f64>() / lum.len() as f64,

        "median" => {
            let mut sorted = lum.to_vec();
            sorted.sort_by(|a, b| a.total_cmp(b));
            let n = sorted.len();
            if n % 2 == 1 {
                sorted[n / 2]
            } else {
                0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
            }
        }

        "center_weighted" => {
            let sigma = 0.2f64;
            let inv_2sigma2 = 1.0 / (2.0 * sigma * sigma);
            let mut weighted = 0.0;
            let mut total = 0.0;
            for y in 0..sh {
                let ny = norm_coord(y, sh, max_dim);
                let ny2 = ny * ny;
                for x in 0..sw {
                    let nx = norm_coord(x, sw, max_dim);
                    let w = (-(nx * nx + ny2) * inv_2sigma2).exp();
                    weighted += lum[y * sw + x] * w;
                    total += w;
                }
            }
            weighted / total
        }

        "partial" => {
            // Hard circular region, ~15% radius (Canon Partial).
            let mut sum = 0.0;
            let mut count = 0usize;
            for y in 0..sh {
                let ny = norm_coord(y, sh, max_dim);
                for x in 0..sw {
                    let nx = norm_coord(x, sw, max_dim);
                    if (nx * nx + ny * ny).sqrt() < 0.15 {
                        sum += lum[y * sw + x];
                        count += 1;
                    }
                }
            }
            if count == 0 {
                lum.iter().sum::<f64>() / lum.len() as f64
            } else {
                sum / count as f64
            }
        }

        "matrix" => {
            // 5×5 grid; each cell weighted by a raised-cosine of its distance
            // from centre so corner zones contribute less.
            let (n_rows, n_cols) = (5usize, 5usize);
            let cell_h = sh / n_rows;
            let cell_w = sw / n_cols;
            let mut means = Vec::with_capacity(n_rows * n_cols);
            let mut weights = Vec::with_capacity(n_rows * n_cols);
            for r in 0..n_rows {
                for c in 0..n_cols {
                    if cell_h == 0 || cell_w == 0 {
                        continue;
                    }
                    let mut sum = 0.0;
                    for yy in r * cell_h..(r + 1) * cell_h {
                        for xx in c * cell_w..(c + 1) * cell_w {
                            sum += lum[yy * sw + xx];
                        }
                    }
                    means.push(sum / (cell_h * cell_w) as f64);
                    let dy = (r as f64 - (n_rows - 1) as f64 / 2.0) / ((n_rows - 1) as f64 / 2.0);
                    let dx = (c as f64 - (n_cols - 1) as f64 / 2.0) / ((n_cols - 1) as f64 / 2.0);
                    let dist = (dx * dx + dy * dy).sqrt() / 2.0f64.sqrt();
                    weights.push(0.5 * (1.0 + (std::f64::consts::PI * dist).cos()));
                }
            }
            let wsum: f64 = weights.iter().sum();
            means.iter().zip(&weights).map(|(m, w)| m * w / wsum).sum()
        }

        "multi_zone" => {
            // Three concentric rings weighted 50/30/20.
            let rings = [(0.00, 0.05, 0.50), (0.05, 0.25, 0.30), (0.25, 0.50, 0.20)];
            let mut weighted_sum = 0.0;
            let mut weight_total = 0.0;
            for &(r_min, r_max, weight) in &rings {
                let mut sum = 0.0;
                let mut count = 0usize;
                for y in 0..sh {
                    let ny = norm_coord(y, sh, max_dim);
                    for x in 0..sw {
                        let nx = norm_coord(x, sw, max_dim);
                        let radius = (nx * nx + ny * ny).sqrt();
                        if radius >= r_min && radius < r_max {
                            sum += lum[y * sw + x];
                            count += 1;
                        }
                    }
                }
                if count == 0 {
                    continue;
                }
                weighted_sum += weight * (sum / count as f64);
                weight_total += weight;
            }
            if weight_total > 0.0 {
                weighted_sum / weight_total
            } else {
                lum.iter().sum::<f64>() / lum.len() as f64
            }
        }

        "highlight_weighted" => {
            // Bias toward bright pixels (weight = Y²) to protect highlights.
            let mut weighted = 0.0;
            let mut total = 0.0;
            for &y in lum {
                let w = y * y;
                weighted += y * w;
                total += w;
            }
            if total < 1e-12 {
                lum.iter().sum::<f64>() / lum.len() as f64
            } else {
                weighted / total
            }
        }

        // Python's fallthrough returns None: no compensation.
        _ => return None,
    };
    Some(value)
}

/// The film raw exposure of `rgb`, before any exposure scale: the method's
/// TC LUT sampled under the front illuminant (CAT-adapted) when one is
/// provided, else `None` (the simplified pipeline).
pub fn rgb_to_film_raw(
    rgb: &ImageBuf,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
    tc_lut: Option<&TcLut>,
    front_illuminant: &[f32],
) -> Option<ImageBuf> {
    tc_lut.map(|lut| {
        backend.hanatos2025_rgb_to_raw(
            rgb,
            lut,
            &params.io.input_color_space,
            front_illuminant,
            params.settings.use_cat16,
        )
    })
}

/// Decode the input CCTF in place when `io.input_cctf_decoding` asks for it,
/// as `colour.RGB_to_XYZ(..., apply_cctf_decoding=True)` does in upstream's
/// `_rgb_to_tc_b`: after the resize, before any exposure scale.
pub fn decode_input_cctf(rgb: &mut ImageBuf, params: &RuntimeParams) {
    if !params.io.input_cctf_decoding {
        return;
    }
    let cctf = RgbColourspace::input(&params.io.input_color_space).cctf;
    if cctf != colorspace::Cctf::Linear {
        rgb.data
            .par_iter_mut()
            .for_each(|v| *v = from_f64(cctf.decode(to_f64(*v))));
    }
}

/// [`rgb_to_film_raw`] for an image that still carries the input CCTF
/// (the auto-exposure metering frame).
pub fn encoded_rgb_to_film_raw(
    rgb: &ImageBuf,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
    tc_lut: Option<&TcLut>,
    front_illuminant: &[f32],
) -> Option<ImageBuf> {
    if !params.io.input_cctf_decoding {
        return rgb_to_film_raw(rgb, params, backend, tc_lut, front_illuminant);
    }
    let mut decoded = rgb.clone();
    decode_input_cctf(&mut decoded, params);
    rgb_to_film_raw(&decoded, params, backend, tc_lut, front_illuminant)
}

/// Expose: convert RGB to film raw exposure.
///
/// `autoexposure_ev` is the metered compensation (0 with auto exposure off;
/// see [`measure_raw_autoexposure_ev`]). Upstream scales the film raw; the
/// upsampler is homogeneous in the input RGB, so scaling the RGB first is
/// the same.
#[allow(clippy::too_many_arguments)]
pub fn expose(
    image: ImageBuf,
    _film: &Profile,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
    tc_lut: Option<&TcLut>,
    front_illuminant: &[f32],
    bw_filming_correction: f64,
    autoexposure_ev: f64,
) -> ImageBuf {
    let pix_um = pixel_size_um(params.camera.film_format_mm, image.width, image.height);

    let mut raw = {
        // Consume the input so optical scratch does not overlap an RGB copy.
        let mut rgb = image;
        decode_input_cctf(&mut rgb, params);
        if autoexposure_ev != 0.0 {
            let scale = from_f64(autoexposure_ev.exp2());
            rgb.data.par_iter_mut().for_each(|v| *v *= scale);
        }

        // Exposure compensation
        let exp_comp = from_f32(exp2_f32(params.camera.exposure_compensation_ev));
        rgb.data.par_iter_mut().for_each(|v| *v *= exp_comp);

        // RGB → film raw exposure; the simplified fallback treats RGB values
        // as proportional to raw exposure.
        rgb_to_film_raw(&rgb, params, backend, tc_lut, front_illuminant)
            .unwrap_or(rgb)
    };
    crate::pipeline::dump_if_env("SPEKTRAFILM_DUMP_FILM_RAW", &raw);

    // Order mirrors Python filming: boost → diffusion → lens_blur → halation.

    // Highlight boost: reconstruct pre-clip highlight irradiance before the
    // optical-scatter effects. No-op when boost_ev == 0.
    let hal_boost = &params.film_render.halation;
    if hal_boost.boost_ev != 0.0 {
        raw = spektrafilm_model::diffusion::boost_highlights(
            &raw,
            hal_boost.boost_ev as f64,
            hal_boost.boost_range as f64,
            hal_boost.protect_ev as f64,
        );
    }

    // Diffusion filter (camera): lens diffusion-filter PSF on linear raw.
    // GPU uses a downsampled sum-of-Gaussians (fast preview); CPU keeps the
    // exact FFT convolution (export parity).
    let df = &params.camera.diffusion_filter;
    if df.active {
        let dm = df.to_model();
        raw = if backend.is_gpu() {
            spektrafilm_model::diffusion::apply_diffusion_filter_blur(
                &raw,
                &dm,
                pix_um as f64,
                backend,
            )
        } else {
            spektrafilm_model::diffusion::apply_diffusion_filter_um(&raw, &dm, pix_um as f64)
        };
    }

    // Lens blur
    if params.camera.lens_blur_um > 0.0 {
        raw = spektrafilm_model::diffusion::apply_gaussian_blur_um(
            &raw,
            params.camera.lens_blur_um,
            pix_um,
            backend,
        );
    }

    // Halation (on linear raw)
    let halation = &params.film_render.halation;
    if halation.active {
        raw = spektrafilm_model::diffusion::apply_halation_um(
            &raw,
            pix_um,
            halation.scatter_amount,
            halation.scatter_spatial_scale,
            halation.scatter_core_um,
            halation.scatter_tail_um,
            halation.scatter_tail_weight,
            halation.halation_amount,
            halation.halation_spatial_scale,
            halation.halation_strength,
            halation.halation_first_sigma_um,
            halation.halation_n_bounces,
            halation.halation_bounce_decay,
            halation.halation_renormalize,
            backend,
        );
    }

    // B&W / slide scanner exposure correction (Python applies this last,
    // before the log10). No-op at factor 1.0.
    if bw_filming_correction != 1.0 {
        let f = from_f64(bw_filming_correction);
        raw.data.par_iter_mut().for_each(|v| *v *= f);
    }

    // Convert to log10 exposure. Mirror Python's
    // `np.log10(np.fmax(raw, 0.0) + 1e-10)` exactly — adding 1e-10
    // after the floor-at-zero shifts every value by a constant in
    // log-space and is *not* the same as `log10(max(raw, 1e-10))`.
    crate::pipeline::dump_if_env("SPEKTRAFILM_DUMP_FILM_PRE_LOG", &raw);
    let zero = from_f64(0.0);
    let eps = from_f64(1e-10);
    raw.data.par_iter_mut().for_each(|v| {
        *v = ((*v).max(zero) + eps).log10();
    });

    raw
}

/// The DIR couplers' matrix, Langmuir terms and inverted curves for `dir`.
/// `norm_curves` are the normalized film curves. Shared with the GPU set-up
/// in `pipeline.rs`.
pub fn dir_couplers_model(
    dir: &crate::params::DirCouplersParams,
    norm_curves: &[[f64; 3]],
    log_exposure: &[f64],
    positive: bool,
) -> spektrafilm_model::couplers::DirCouplersModel {
    let matrix = spektrafilm_model::couplers::compute_dir_couplers_matrix(
        dir.gamma_samelayer_rgb,
        dir.gamma_interlayer_r_to_gb,
        dir.gamma_interlayer_g_to_rb,
        dir.gamma_interlayer_b_to_rg,
        dir.inhibition_samelayer,
        dir.inhibition_interlayer,
    );
    spektrafilm_model::couplers::DirCouplersModel::new(
        norm_curves,
        log_exposure,
        &matrix,
        dir.amount,
        dir.langmuir_donor_k_rgb,
        dir.langmuir_receiver_k_rgb,
        positive,
    )
}

/// Develop: log_raw → density_cmy via density curves + DIR couplers + grain.
pub fn develop(
    log_raw: ImageBuf,
    film: &Profile,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
) -> ImageBuf {
    let stage_timings = stage_timings_enabled();
    let pix_um = pixel_size_um(params.camera.film_format_mm, log_raw.width, log_raw.height);
    // f64 chain for Python parity — curves are f64 in the profile JSON.
    let log_exposure_f64 = film.log_exposure_f64();
    let density_curves_f64 = film.density_curves_f64();

    // Filming.develop uses NORMALIZED curves (Python `develop` subtracts nanmin).
    let norm_curves_f64 =
        spektrafilm_model::density_curves::normalize_density_curves_f64(&density_curves_f64);
    let t = Instant::now();
    // Unit gamma: the Film Curves morph is baked into the curves
    // (`Pipeline::apply_film_curves_morph`), as upstream experimental does.
    let mut density_cmy =
        backend.density_curve_interp(&log_raw, &log_exposure_f64, &norm_curves_f64, 1.0);
    print_stage_timing(stage_timings, "filming_develop.density_interp", t);

    // DIR couplers
    let dir = &params.film_render.dir_couplers;
    if dir.active {
        let t = Instant::now();
        let model =
            dir_couplers_model(dir, &norm_curves_f64, &log_exposure_f64, film.is_positive());
        density_cmy = spektrafilm_model::couplers::apply_density_correction(
            density_cmy,
            log_raw,
            pix_um,
            &log_exposure_f64,
            &model,
            dir.diffusion_size_um,
            dir.diffusion_tail_um,
            dir.diffusion_tail_weight,
            1.0,
            backend,
        );
        print_stage_timing(stage_timings, "filming_develop.dir_couplers", t);
    } else {
        drop(log_raw);
    }

    // Log exposure has been consumed/released before allocating grain scratch.
    crate::pipeline::dump_if_env("SPEKTRAFILM_DUMP_FILM_PRE_GRAIN", &density_cmy);
    // Grain: upstream experimental's multilayer model on the per-sub-layer
    // curves (`crate::grain` resolves the stock preset and the curves).
    if crate::grain::grain_active(params) {
        let t = Instant::now();
        let pixel_size_um = grain_pixel_size_um(params, density_cmy.width, density_cmy.height);
        density_cmy = spektrafilm_model::grain::apply_grain(
            density_cmy,
            pixel_size_um,
            &crate::grain::grain_settings(film, &params.film_render.grain),
            &crate::grain::grain_curves(film),
            film.is_positive(),
            params.film_render.grain.monochrome,
            params.settings.serial_grain,
        );
        print_stage_timing(stage_timings, "filming_develop.grain", t);
    }

    density_cmy
}

/// Full filming stage: expose + develop.
pub fn process(
    image: ImageBuf,
    film: &Profile,
    params: &RuntimeParams,
    backend: &dyn ComputeBackend,
    tc_lut: Option<&TcLut>,
) -> ImageBuf {
    let ref_illuminant = crate::illuminants::select_illuminant(&film.info.reference_illuminant);
    let autoexposure_ev = if params.camera.auto_exposure {
        let preview = metering_preview(&image);
        let raw = encoded_rgb_to_film_raw(&preview, params, backend, tc_lut, &ref_illuminant);
        measure_raw_autoexposure_ev(raw.as_ref().unwrap_or(&preview), &params.camera.auto_exposure_method, 1.0)
    } else {
        0.0
    };
    let log_raw = expose(
        image,
        film,
        params,
        backend,
        tc_lut,
        &ref_illuminant,
        1.0,
        autoexposure_ev,
    );
    develop(log_raw, film, params, backend)
}
