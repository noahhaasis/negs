/// Runtime parameters for the film simulation pipeline.
///
/// Mirrors Python `params_schema.py`. Every field has a sensible default
/// matching the Python implementation.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffusionFilterParams {
    #[serde(default)]
    pub active: bool,
    #[serde(default = "default_bpm")]
    pub filter_family: String,
    #[serde(default = "default_half")]
    pub strength: f32,
    #[serde(default = "default_one")]
    pub spatial_scale: f32,
    #[serde(default)]
    pub halo_warmth: f32,
    #[serde(default = "default_one")]
    pub core_intensity: f32,
    #[serde(default = "default_one")]
    pub core_size: f32,
    #[serde(default = "default_one")]
    pub halo_intensity: f32,
    #[serde(default = "default_one")]
    pub halo_size: f32,
    #[serde(default = "default_one")]
    pub bloom_intensity: f32,
    #[serde(default = "default_one")]
    pub bloom_size: f32,
}

impl DiffusionFilterParams {
    /// Borrowed view as the model crate's `DiffusionFilter` (f64), for the
    /// CPU diffusion-filter apply. `family` borrows `self.filter_family`.
    pub fn to_model(&self) -> spektrafilm_model::diffusion::DiffusionFilter<'_> {
        spektrafilm_model::diffusion::DiffusionFilter {
            family: &self.filter_family,
            strength: self.strength as f64,
            spatial_scale: self.spatial_scale as f64,
            halo_warmth: self.halo_warmth as f64,
            core_intensity: self.core_intensity as f64,
            core_size: self.core_size as f64,
            halo_intensity: self.halo_intensity as f64,
            halo_size: self.halo_size as f64,
            bloom_intensity: self.bloom_intensity as f64,
            bloom_size: self.bloom_size as f64,
        }
    }
}

impl Default for DiffusionFilterParams {
    fn default() -> Self {
        Self {
            active: false,
            filter_family: "black_pro_mist".into(),
            strength: 0.5,
            spatial_scale: 1.0,
            halo_warmth: 0.0,
            core_intensity: 1.0,
            core_size: 1.0,
            halo_intensity: 1.0,
            halo_size: 1.0,
            bloom_intensity: 1.0,
            bloom_size: 1.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CameraParams {
    #[serde(default)]
    pub exposure_compensation_ev: f32,
    #[serde(default = "default_true")]
    pub auto_exposure: bool,
    #[serde(default = "default_center_weighted")]
    pub auto_exposure_method: String,
    #[serde(default)]
    pub lens_blur_um: f32,
    #[serde(default = "default_35")]
    pub film_format_mm: f32,
    /// Camera taking filter, a key of
    /// [`crate::color_filters::COLOR_FILTER_LIBRARY`] or `"none"`. Older
    /// params files' `filter_uv`/`filter_ir`, which nothing used and upstream
    /// removed, are ignored.
    #[serde(default = "default_no_color_filter")]
    pub color_filter: String,
    #[serde(default)]
    pub diffusion_filter: DiffusionFilterParams,
}

impl Default for CameraParams {
    fn default() -> Self {
        Self {
            exposure_compensation_ev: 0.0,
            auto_exposure: true,
            auto_exposure_method: "center_weighted".into(),
            lens_blur_um: 0.0,
            film_format_mm: 35.0,
            color_filter: default_no_color_filter(),
            diffusion_filter: DiffusionFilterParams::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnlargerParams {
    #[serde(default = "default_th_kg3")]
    pub illuminant: String,
    #[serde(default = "default_one")]
    pub print_exposure: f32,
    #[serde(default = "default_true")]
    pub print_exposure_compensation: bool,
    #[serde(default = "default_true")]
    pub normalize_print_exposure: bool,
    #[serde(default)]
    pub y_filter_shift: f32,
    #[serde(default)]
    pub m_filter_shift: f32,
    #[serde(default = "default_55")]
    pub y_filter_neutral: f32,
    #[serde(default = "default_65")]
    pub m_filter_neutral: f32,
    #[serde(default)]
    pub c_filter_neutral: f32,
    #[serde(default)]
    pub lens_blur: f32,
    #[serde(default)]
    pub diffusion_filter: DiffusionFilterParams,
    #[serde(default)]
    pub preflash_exposure: f32,
    #[serde(default)]
    pub preflash_y_filter_shift: f32,
    #[serde(default)]
    pub preflash_m_filter_shift: f32,
}

impl Default for EnlargerParams {
    fn default() -> Self {
        Self {
            illuminant: "TH-KG3".into(),
            print_exposure: 1.0,
            print_exposure_compensation: true,
            normalize_print_exposure: true,
            y_filter_shift: 0.0,
            m_filter_shift: 0.0,
            y_filter_neutral: 55.0,
            m_filter_neutral: 65.0,
            c_filter_neutral: 0.0,
            lens_blur: 0.0,
            diffusion_filter: DiffusionFilterParams::default(),
            preflash_exposure: 0.0,
            preflash_y_filter_shift: 0.0,
            preflash_m_filter_shift: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannerParams {
    #[serde(default)]
    pub lens_blur: f32,
    #[serde(default)]
    pub white_correction: bool,
    #[serde(default)]
    pub black_correction: bool,
    #[serde(default = "default_098")]
    pub white_level: f32,
    #[serde(default = "default_001")]
    pub black_level: f32,
    #[serde(default = "default_unsharp")]
    pub unsharp_mask: [f32; 2],
}

impl Default for ScannerParams {
    fn default() -> Self {
        Self {
            lens_blur: 0.0,
            white_correction: false,
            black_correction: false,
            white_level: 0.98,
            black_level: 0.01,
            unsharp_mask: [0.7, 0.7],
        }
    }
}

/// Upstream experimental's `GrainParams` (`runtime/params_schema.py`). The
/// per-stock fields are `None` by default: the pipeline fills them from the
/// film's entry in `data/presets/grain.toml` (upstream `_apply_grain_preset`,
/// see `grain_presets.rs`). A value set here overrides the preset.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GrainParams {
    #[serde(default = "default_true")]
    pub active: bool,
    /// Per channel, σ_D × 1000 at a 48 µm aperture and net density 1.0.
    #[serde(default)]
    pub rms_granularity: Option<[f64; 3]>,
    #[serde(default)]
    pub density_min: Option<[f64; 3]>,
    #[serde(default)]
    pub uniformity: Option<[f64; 3]>,
    /// Relative particle area per emulsion sub-layer, coarsest = 1.
    #[serde(default)]
    pub particle_scale_sublayers: Option<[f64; 3]>,
    /// Pixel blur σ in pixels.
    #[serde(default = "default_grain_blur")]
    pub blur: f64,
    #[serde(default = "default_mult_usm_sigma")]
    pub mult_usm_sigma: f64,
    #[serde(default = "default_mult_usm_amount")]
    pub mult_usm_amount: f64,
    #[serde(default = "default_blur_dye_clouds_um")]
    pub blur_dye_clouds_um: f64,
    /// (blur in µm, clump size in nm).
    #[serde(default = "default_micro_structure")]
    pub micro_structure: [f64; 2],
    /// One shared noise field across all channels instead of independent
    /// per-channel RNG streams. Set by the pipeline for B&W films
    /// (upstream n_channels==1 has a single emulsion); not user-facing.
    #[serde(default)]
    pub monochrome: bool,
}

fn default_grain_blur() -> f64 {
    0.89
}
fn default_mult_usm_sigma() -> f64 {
    0.7
}
fn default_mult_usm_amount() -> f64 {
    1.5
}
fn default_blur_dye_clouds_um() -> f64 {
    2.0
}
fn default_micro_structure() -> [f64; 2] {
    [0.2, 30.0]
}

impl Default for GrainParams {
    fn default() -> Self {
        Self {
            active: true,
            rms_granularity: None,
            density_min: None,
            uniformity: None,
            particle_scale_sublayers: None,
            blur: default_grain_blur(),
            mult_usm_sigma: default_mult_usm_sigma(),
            mult_usm_amount: default_mult_usm_amount(),
            blur_dye_clouds_um: default_blur_dye_clouds_um(),
            micro_structure: default_micro_structure(),
            monochrome: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HalationParams {
    #[serde(default = "default_true")]
    pub active: bool,
    // f64 to match Python's `np.asarray(..., dtype=np.float64)` —
    // f32 storage truncates ~7 decimals which shifts every sigma/lambda
    // by ~3e-8, accumulating through Gaussian/exponential kernels.
    #[serde(default = "default_one_f64")]
    pub scatter_amount: f64,
    #[serde(default = "default_one_f64")]
    pub scatter_spatial_scale: f64,
    #[serde(default = "default_one_f64")]
    pub halation_amount: f64,
    #[serde(default = "default_one_f64")]
    pub halation_spatial_scale: f64,
    #[serde(default = "default_scatter_core_f64")]
    pub scatter_core_um: [f64; 3],
    #[serde(default = "default_scatter_tail_f64")]
    pub scatter_tail_um: [f64; 3],
    #[serde(default = "default_scatter_tail_weight_f64")]
    pub scatter_tail_weight: [f64; 3],
    #[serde(default)]
    pub boost_ev: f32,
    #[serde(default = "default_03")]
    pub boost_range: f32,
    #[serde(default = "default_4")]
    pub protect_ev: f32,
    #[serde(default = "default_halation_strength_f64")]
    pub halation_strength: [f64; 3],
    #[serde(default = "default_halation_sigma_f64")]
    pub halation_first_sigma_um: [f64; 3],
    #[serde(default = "default_3i")]
    pub halation_n_bounces: u32,
    #[serde(default = "default_half_f64")]
    pub halation_bounce_decay: f64,
    #[serde(default = "default_true")]
    pub halation_renormalize: bool,
}

fn default_one_f64() -> f64 {
    1.0
}
fn default_half_f64() -> f64 {
    0.5
}
fn default_scatter_core_f64() -> [f64; 3] {
    [2.2, 2.0, 1.6]
}
fn default_scatter_tail_f64() -> [f64; 3] {
    [9.3, 9.7, 9.1]
}
fn default_scatter_tail_weight_f64() -> [f64; 3] {
    [0.78, 0.65, 0.67]
}
fn default_halation_strength_f64() -> [f64; 3] {
    [0.05, 0.015, 0.0]
}
fn default_halation_sigma_f64() -> [f64; 3] {
    [65.0, 65.0, 65.0]
}

impl Default for HalationParams {
    fn default() -> Self {
        Self {
            active: true,
            scatter_amount: 1.0,
            scatter_spatial_scale: 1.0,
            halation_amount: 1.0,
            halation_spatial_scale: 1.0,
            scatter_core_um: [2.2, 2.0, 1.6],
            scatter_tail_um: [9.3, 9.7, 9.1],
            scatter_tail_weight: [0.78, 0.65, 0.67],
            boost_ev: 0.0,
            boost_range: 0.3,
            protect_ev: 4.0,
            halation_strength: [0.05, 0.015, 0.0],
            halation_first_sigma_um: [65.0, 65.0, 65.0],
            halation_n_bounces: 3,
            halation_bounce_decay: 0.5,
            halation_renormalize: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirCouplersParams {
    #[serde(default = "default_true")]
    pub active: bool,
    // f64 throughout — Python reads these as JSON floats (f64). The
    // f32 truncation of values like 0.341 (→ 0.3409999907... in f32 vs
    // 0.341 = 0.34100000000000003 in f64) shifts every coupler weight
    // and diffusion sigma by ~3e-8 and amplifies through the per-channel
    // density correction.
    #[serde(default = "default_one_f64")]
    pub amount: f64,
    #[serde(default = "default_one_f64")]
    pub inhibition_samelayer: f64,
    #[serde(default = "default_one_f64")]
    pub inhibition_interlayer: f64,
    #[serde(default = "default_gamma_same_f64")]
    pub gamma_samelayer_rgb: [f64; 3],
    #[serde(default = "default_gamma_r_gb_f64")]
    pub gamma_interlayer_r_to_gb: [f64; 2],
    #[serde(default = "default_gamma_g_rb_f64")]
    pub gamma_interlayer_g_to_rb: [f64; 2],
    #[serde(default = "default_gamma_b_rg_f64")]
    pub gamma_interlayer_b_to_rg: [f64; 2],
    /// Langmuir knee of the donor saturation (negatives), per channel, as a
    /// fraction of the channel's d_max. Infinity is the linear law; JSON
    /// writes it as `null` and reads `null` back as infinity.
    #[serde(default = "default_langmuir_k", deserialize_with = "langmuir_k_rgb")]
    pub langmuir_donor_k_rgb: [f64; 3],
    /// Langmuir knee of the receiver saturation (positives), per channel, as
    /// a fraction of the arrived-inhibitor ceiling. Infinity is linear.
    #[serde(default = "default_langmuir_k", deserialize_with = "langmuir_k_rgb")]
    pub langmuir_receiver_k_rgb: [f64; 3],
    #[serde(default = "default_20_f64")]
    pub diffusion_size_um: f64,
    #[serde(default = "default_200_f64")]
    pub diffusion_tail_um: f64,
    #[serde(default = "default_003_f64")]
    pub diffusion_tail_weight: f64,
}

fn default_gamma_same_f64() -> [f64; 3] {
    [0.341, 0.324, 0.273]
}
fn default_gamma_r_gb_f64() -> [f64; 2] {
    [0.355, 0.305]
}
fn default_gamma_g_rb_f64() -> [f64; 2] {
    [0.154, 0.358]
}
fn default_gamma_b_rg_f64() -> [f64; 2] {
    [0.171, 0.225]
}
fn default_20_f64() -> f64 {
    20.0
}
fn default_200_f64() -> f64 {
    200.0
}
fn default_003_f64() -> f64 {
    0.03
}
fn default_langmuir_k() -> [f64; 3] {
    [1.0; 3]
}
fn langmuir_k_rgb<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[f64; 3], D::Error> {
    let k = <[Option<f64>; 3]>::deserialize(deserializer)?;
    Ok(k.map(|v| v.unwrap_or(f64::INFINITY)))
}

impl Default for DirCouplersParams {
    fn default() -> Self {
        Self {
            active: true,
            amount: 1.0,
            inhibition_samelayer: 1.0,
            inhibition_interlayer: 1.0,
            gamma_samelayer_rgb: [0.341, 0.324, 0.273],
            gamma_interlayer_r_to_gb: [0.355, 0.305],
            gamma_interlayer_g_to_rb: [0.154, 0.358],
            gamma_interlayer_b_to_rg: [0.171, 0.225],
            langmuir_donor_k_rgb: default_langmuir_k(),
            langmuir_receiver_k_rgb: default_langmuir_k(),
            diffusion_size_um: 20.0,
            diffusion_tail_um: 200.0,
            diffusion_tail_weight: 0.03,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlareParams {
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_003")]
    pub percent: f32,
    #[serde(default = "default_07")]
    pub roughness: f32,
    #[serde(default = "default_half")]
    pub blur: f32,
}

impl Default for GlareParams {
    fn default() -> Self {
        Self {
            active: true,
            percent: 0.03,
            roughness: 0.7,
            blur: 0.5,
        }
    }
}

/// Film base tuning — upstream experimental's `FilmBaseParams`
/// (`runtime/params_schema.py`, `model/develop.py`): a tilt and per-channel
/// density scales of the film base + fog (the orange mask), around the
/// status-M peaks. Applied wherever the film is lit: the print exposure, its
/// mid-grey reference and the film scan. Neutral values (scale 1, tilt 0,
/// C/M/Y 1) are the identity. See `crate::bases`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilmBaseParams {
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_one_f64")]
    pub scale: f64,
    #[serde(default)]
    pub tilt: f64,
    #[serde(default = "default_one_f64")]
    pub cyan: f64,
    #[serde(default = "default_one_f64")]
    pub magenta: f64,
    #[serde(default = "default_one_f64")]
    pub yellow: f64,
}

impl Default for FilmBaseParams {
    fn default() -> Self {
        Self { active: true, scale: 1.0, tilt: 0.0, cyan: 1.0, magenta: 1.0, yellow: 1.0 }
    }
}

/// Print base tuning — upstream experimental's `PrintBaseParams`: an overall
/// scale and per-channel scales of the paper base around the status-A peaks,
/// applied when the print is scanned. Neutral values are the identity. See
/// `crate::bases`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrintBaseParams {
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default = "default_one_f64")]
    pub scale: f64,
    #[serde(default = "default_one_f64")]
    pub cyan: f64,
    #[serde(default = "default_one_f64")]
    pub magenta: f64,
    #[serde(default = "default_one_f64")]
    pub yellow: f64,
}

impl Default for PrintBaseParams {
    fn default() -> Self {
        Self { active: true, scale: 1.0, cyan: 1.0, magenta: 1.0, yellow: 1.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilmRenderingParams {
    /// Development time (minutes) for B&W development-time families —
    /// forward-port of upstream dev's `chemistry.development_time`. Selects
    /// the nearest entry of the profile's family; `None` picks the
    /// floor-middle entry. Ignored by colour profiles.
    #[serde(default)]
    pub development_time: Option<f64>,
    #[serde(default)]
    pub grain: GrainParams,
    #[serde(default)]
    pub halation: HalationParams,
    #[serde(default)]
    pub dir_couplers: DirCouplersParams,
    #[serde(default)]
    pub glare: GlareParams,
    /// Film counterpart of the print-curve morph — forward-port of upstream
    /// experimental's `film_render.chemistry` (minus `development_time`,
    /// above), which replaced the old `density_curve_gamma`. The curves are
    /// always re-evaluated from the fitted model; inactive or identity
    /// settings reproduce it. See `Pipeline::apply_film_curves_morph`.
    #[serde(default)]
    pub density_curves_morph: CurvesMorphParams,
    #[serde(default)]
    pub base: FilmBaseParams,
}

impl Default for FilmRenderingParams {
    fn default() -> Self {
        Self {
            development_time: None,
            grain: GrainParams::default(),
            halation: HalationParams::default(),
            dir_couplers: DirCouplersParams::default(),
            glare: GlareParams::default(),
            density_curves_morph: CurvesMorphParams::default(),
            base: FilmBaseParams::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintRenderingParams {
    /// Development time (minutes) for B&W print stocks (e.g. kodak_2302) —
    /// same semantics as `FilmRenderingParams::development_time`.
    #[serde(default)]
    pub development_time: Option<f64>,
    #[serde(default)]
    pub glare: GlareParams,
    #[serde(default)]
    pub density_curves_morph: CurvesMorphParams,
    #[serde(default)]
    pub base: PrintBaseParams,
}

impl Default for PrintRenderingParams {
    fn default() -> Self {
        Self {
            development_time: None,
            glare: GlareParams::default(),
            density_curves_morph: CurvesMorphParams::default(),
            base: PrintBaseParams::default(),
        }
    }
}

/// User-facing controls for the s023 density-curve morph (see
/// `crate::print_morph`), shared by the film and print curves. Kept in f64 —
/// the morph is a parity-sensitive f64 computation. `active` defaults to
/// `false`, matching upstream 0.3.4 — but note that (also matching upstream)
/// the print develop always evaluates the profile's fitted
/// `density_curves_model` when one is present; `active` only enables the
/// coupled-gamma morphing of that model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurvesMorphParams {
    #[serde(default)]
    pub active: bool,
    #[serde(default = "default_one_f64")]
    pub gamma_factor: f64,
    #[serde(default = "default_one_f64")]
    pub gamma_factor_fast: f64,
    #[serde(default = "default_one_f64")]
    pub gamma_factor_slow: f64,
    #[serde(default = "default_one_f64")]
    pub gamma_factor_red: f64,
    #[serde(default = "default_one_f64")]
    pub gamma_factor_green: f64,
    #[serde(default = "default_one_f64")]
    pub gamma_factor_blue: f64,
    #[serde(default)]
    pub developer_exhaustion: f64,
}

impl Default for CurvesMorphParams {
    fn default() -> Self {
        Self {
            active: false,
            gamma_factor: 1.0,
            gamma_factor_fast: 1.0,
            gamma_factor_slow: 1.0,
            gamma_factor_red: 1.0,
            gamma_factor_green: 1.0,
            gamma_factor_blue: 1.0,
            developer_exhaustion: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IoParams {
    /// Input RAW development only; independent of scanner white correction.
    #[serde(default)]
    pub raw_white_balance: spektrafilm_io::RawWhiteBalance,
    /// How the loaders read a non-RAW file's values (see
    /// `spektrafilm_io::colour`). Params JSON without it, but with either
    /// field below, is read as manual: see [`infer_input_interpretation`].
    #[serde(default)]
    pub input_interpretation: spektrafilm_io::InputInterpretation,
    /// The colour space of the pipeline's input values, and whether they
    /// still carry its transfer function. A loaded file sets these to the
    /// engine's representation (linear sRGB) unless the interpretation is
    /// manual; see [`IoParams::apply_input_colour`].
    #[serde(default = "default_prophoto")]
    pub input_color_space: String,
    #[serde(default)]
    pub input_cctf_decoding: bool,
    #[serde(default = "default_srgb")]
    pub output_color_space: String,
    #[serde(default = "default_true")]
    pub output_cctf_encoding: bool,
    #[serde(default)]
    pub crop: bool,
    #[serde(default = "default_crop_center")]
    pub crop_center: [f32; 2],
    #[serde(default = "default_crop_size")]
    pub crop_size: [f32; 2],
    /// The legacy working-resolution factor. 1 unless `photo_long_side_px`
    /// is unset; never combined with it.
    #[serde(default = "default_one")]
    pub upscale_factor: f32,
    /// Image Size's Longest side: the cropped photograph's longest side in
    /// pixels, before borders and export reductions. The source is resized
    /// before the film so grain and halation develop at this size. `None`
    /// uses `upscale_factor`. See `spektrafilm_io::image_size`.
    #[serde(default)]
    pub photo_long_side_px: Option<u32>,
    #[serde(default)]
    pub scan_film: bool,
    #[serde(default)]
    pub output_gamut_compress: OutputGamutCompressParams,
    #[serde(default)]
    pub input_gamut_compress: InputGamutCompressParams,
    /// User-requested rotation in 90° clockwise steps (mod 4), applied
    /// to the final output image after the pipeline runs. This is on
    /// top of the EXIF orientation, which the loaders apply at decode
    /// time so the pipeline always sees an upright image.
    #[serde(default)]
    pub rotate_cw: i32,
}

impl Default for IoParams {
    fn default() -> Self {
        Self {
            raw_white_balance: spektrafilm_io::RawWhiteBalance::default(),
            input_interpretation: spektrafilm_io::InputInterpretation::default(),
            input_color_space: "ProPhoto RGB".into(),
            input_cctf_decoding: false,
            output_color_space: "sRGB".into(),
            output_cctf_encoding: true,
            crop: false,
            crop_center: [0.5, 0.5],
            crop_size: [0.1, 0.1],
            upscale_factor: 1.0,
            photo_long_side_px: None,
            scan_film: false,
            output_gamut_compress: OutputGamutCompressParams::default(),
            input_gamut_compress: InputGamutCompressParams::default(),
            rotate_cw: 0,
        }
    }
}

impl IoParams {
    /// Sets the pipeline's input fields for an image the loader read as
    /// `colour`: the engine's representation (linear sRGB, unclamped), unless
    /// the values were passed through for these fields (manual mode).
    pub fn apply_input_colour(&mut self, colour: &spektrafilm_io::InputColour) {
        if colour.linear_srgb {
            self.input_color_space = "sRGB".into();
            self.input_cctf_decoding = false;
        }
    }
}

/// Params JSON (a whole params object, or one with an `io` object) from
/// before `io.input_interpretation`: if it sets `io.input_color_space` or
/// `io.input_cctf_decoding`, those described its input files, so it keeps
/// that meaning (manual); otherwise files are read automatically. Params
/// that state an interpretation are left alone. The apps always send one
/// (the defaults carry it), so their saved sessions read files
/// automatically.
pub fn infer_input_interpretation(params: &mut serde_json::Value) {
    let Some(io) = params.get_mut("io").and_then(|io| io.as_object_mut()) else { return };
    if !io.contains_key("input_interpretation")
        && (io.contains_key("input_color_space") || io.contains_key("input_cctf_decoding"))
    {
        io.insert(
            "input_interpretation".into(),
            serde_json::to_value(spektrafilm_io::InputInterpretation::manual()).expect("serializable"),
        );
    }
}

/// Input gamut compression config — mirrors upstream `InputGamutCompressSpec`.
/// Baked into the tc_lut at build time, so the per-pixel path stays
/// compression-agnostic. `"xy"` applies the ACES-RGC-style radial compression
/// toward the boundary; `algorithm = "off"` passes input chromaticities
/// through unchanged. The defaults are experimental 28bf883's: the inscribed
/// hull at detail 5, a D65 center and the ACES RGC knee `(0.815, 1, 1.2)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputGamutCompressParams {
    #[serde(default = "default_xy")]
    pub algorithm: String,
    #[serde(default = "default_input_gamut_knee")]
    pub knee: [f32; 3],
    /// `"inscribed_hull"`, a smooth hull inside the spectral locus, or
    /// `"locus"`, the raw locus polygon (upstream main's boundary).
    #[serde(default = "default_gamut_boundary")]
    pub boundary: String,
    /// Gaussian Fourier width (modes kept) for the inscribed hull.
    #[serde(default = "default_hull_detail")]
    pub hull_detail: f64,
    /// `"D65"`, experimental's fixed compression center, or `"reference"`,
    /// the upsampler's reference white (upstream main).
    #[serde(default = "default_gamut_center")]
    pub center: String,
}

impl Default for InputGamutCompressParams {
    fn default() -> Self {
        Self {
            algorithm: "xy".into(),
            knee: default_input_gamut_knee(),
            boundary: default_gamut_boundary(),
            hull_detail: default_hull_detail(),
            center: default_gamut_center(),
        }
    }
}

/// Output gamut compression config — mirrors upstream `OutputGamutCompressSpec`.
/// `"oklch"` (the default, as in experimental 28bf883) applies the OkLCh
/// chroma knee + one-sided lightness roll-off; `"off"` passes RGB through
/// unchanged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputGamutCompressParams {
    #[serde(default = "default_oklch")]
    pub algorithm: String,
    #[serde(default = "default_output_gamut_knee")]
    pub knee: [f32; 3],
    #[serde(default = "default_gamut_lightness")]
    pub lightness_compression: Option<[f32; 3]>,
}

impl Default for OutputGamutCompressParams {
    fn default() -> Self {
        Self {
            algorithm: default_oklch(),
            knee: default_output_gamut_knee(),
            lightness_compression: default_gamut_lightness(),
        }
    }
}

fn default_xy() -> String {
    "xy".into()
}
fn default_gamut_boundary() -> String {
    "inscribed_hull".into()
}
fn default_hull_detail() -> f64 {
    5.0
}
fn default_gamut_center() -> String {
    "D65".into()
}
fn default_oklch() -> String {
    "oklch".into()
}
fn default_input_gamut_knee() -> [f32; 3] {
    [0.815, 1.0, 1.2]
}
fn default_output_gamut_knee() -> [f32; 3] {
    [0.95, 1.0, 1.6]
}
fn default_gamut_lightness() -> Option<[f32; 3]> {
    Some([0.95, 1.0, 1.6])
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsParams {
    #[serde(default = "default_hanatos")]
    pub rgb_to_raw_method: String,
    #[serde(default = "default_true")]
    pub apply_hanatos2025_adaptation_window: bool,
    #[serde(default)]
    pub apply_hanatos2025_adaptation_surface: bool,
    #[serde(default)]
    pub spectral_gaussian_blur: f32,
    #[serde(default)]
    pub use_enlarger_lut: bool,
    #[serde(default)]
    pub use_scanner_lut: bool,
    /// Process CPU grain channels sequentially to reduce peak memory.
    /// The GUI applies this only to exports; previews keep parallel grain.
    #[serde(default)]
    pub serial_grain: bool,
    #[serde(default = "default_17")]
    pub lut_resolution: u32,
    #[serde(default)]
    pub use_fast_stats: bool,
    #[serde(default = "default_640")]
    pub preview_max_size: u32,
    #[serde(default)]
    pub preview_mode: bool,
    #[serde(default = "default_true")]
    pub neutral_print_filters_from_database: bool,
    /// Use CAT16 (instead of CAT02) for the input RGB→tc chromatic adaptation
    /// that feeds the Hanatos spectral upsampling. Default true matches
    /// upstream 0.3.4 (`_rgb_to_tc_b` hard-codes CAT16); false restores the
    /// CAT02 0.3.2 behavior.
    #[serde(default = "default_true")]
    pub use_cat16: bool,
    /// How the scan's white reaches an output colour space whose white is
    /// not D65 (ProPhoto RGB, DCI-P3, ACES); see [`OutputAdaptation`].
    #[serde(default)]
    pub output_adaptation: OutputAdaptation,
    /// Input values no light can produce, at the spectral model's input; see
    /// [`SpectralBoundary`].
    #[serde(default)]
    pub spectral_boundary: SpectralBoundary,
}

/// The engine's input is linear and unclamped, so it can hold colours beyond
/// the xy triangle (wide camera gamuts such as V-Gamut or DaVinci Wide
/// Gamut, and noise) and no light at all.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpectralBoundary {
    /// No exposure for X + Y + Z <= 0 or non-finite values; a chromaticity
    /// beyond the triangle moves toward the white onto it at the same
    /// X + Y + Z (see `spektrafilm_math::spectral::adapted_xyz_to_tc_b`).
    /// Values with no negative X, Y or Z are unaffected.
    #[default]
    Guard,
    /// Upstream's arithmetic, which folds x > 1 back inside the triangle. For
    /// reference comparisons.
    Upstream,
}

/// The scan is adapted from its viewing illuminant's white with CAT02 (as
/// upstream does), and is then encoded in the output colour space.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputAdaptation {
    /// CAT02 to D65, the white the photograph is judged in, then Bradford to
    /// the output space's white, as an ICC-managed viewer undoes it: the same
    /// edit looks the same in every output space (within each gamut). Outputs
    /// with a D65 white (sRGB, Display P3, Adobe RGB, Rec. 2020) are
    /// unaffected.
    #[default]
    ViaD65,
    /// Upstream: CAT02 straight to the output space's white. A ProPhoto RGB
    /// file then differs from the sRGB one by up to ~1.4 ΔE00 once a viewer
    /// adapts it back to D65. For reference comparisons.
    Direct,
}

impl Default for SettingsParams {
    fn default() -> Self {
        Self {
            rgb_to_raw_method: "hanatos2025".into(),
            apply_hanatos2025_adaptation_window: true,
            apply_hanatos2025_adaptation_surface: false,
            spectral_gaussian_blur: 0.0,
            use_enlarger_lut: false,
            use_scanner_lut: false,
            serial_grain: false,
            lut_resolution: 17,
            use_fast_stats: false,
            preview_max_size: 640,
            preview_mode: false,
            neutral_print_filters_from_database: true,
            use_cat16: true,
            output_adaptation: OutputAdaptation::ViaD65,
            spectral_boundary: SpectralBoundary::Guard,
        }
    }
}

/// Top-level runtime parameters. Combines all sub-parameter groups.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeParams {
    #[serde(default)]
    pub camera: CameraParams,
    #[serde(default)]
    pub enlarger: EnlargerParams,
    #[serde(default)]
    pub scanner: ScannerParams,
    #[serde(default)]
    pub film_render: FilmRenderingParams,
    #[serde(default)]
    pub print_render: PrintRenderingParams,
    #[serde(default)]
    pub io: IoParams,
    #[serde(default)]
    pub settings: SettingsParams,
    /// Finishing edits on the scan; see [`crate::post`].
    #[serde(default)]
    pub post: crate::post::PostParams,
}

impl Default for RuntimeParams {
    fn default() -> Self {
        Self {
            camera: CameraParams::default(),
            enlarger: EnlargerParams::default(),
            scanner: ScannerParams::default(),
            film_render: FilmRenderingParams::default(),
            print_render: PrintRenderingParams::default(),
            io: IoParams::default(),
            settings: SettingsParams::default(),
            post: crate::post::PostParams::default(),
        }
    }
}

// Default value helpers
fn default_bpm() -> String {
    "black_pro_mist".into()
}
fn default_half() -> f32 {
    0.5
}
fn default_one() -> f32 {
    1.0
}
fn default_true() -> bool {
    true
}
fn default_center_weighted() -> String {
    "center_weighted".into()
}
fn default_35() -> f32 {
    35.0
}
fn default_no_color_filter() -> String {
    crate::color_filters::NO_COLOR_FILTER.into()
}
fn default_th_kg3() -> String {
    "TH-KG3".into()
}
fn default_55() -> f32 {
    55.0
}
fn default_65() -> f32 {
    65.0
}
fn default_098() -> f32 {
    0.98
}
fn default_001() -> f32 {
    0.01
}
fn default_unsharp() -> [f32; 2] {
    [0.7, 0.7]
}
fn default_03() -> f32 {
    0.3
}
fn default_4() -> f32 {
    4.0
}
fn default_3i() -> u32 {
    3
}
fn default_003() -> f32 {
    0.03
}
fn default_07() -> f32 {
    0.7
}
fn default_prophoto() -> String {
    "ProPhoto RGB".into()
}
fn default_srgb() -> String {
    "sRGB".into()
}
fn default_crop_center() -> [f32; 2] {
    [0.5, 0.5]
}
fn default_crop_size() -> [f32; 2] {
    [0.1, 0.1]
}
fn default_hanatos() -> String {
    "hanatos2025".into()
}
fn default_17() -> u32 {
    17
}
fn default_640() -> u32 {
    640
}

#[cfg(test)]
mod input_interpretation_tests {
    use super::*;
    use serde_json::json;
    use spektrafilm_io::InputMode;

    fn parse(mut value: serde_json::Value) -> RuntimeParams {
        infer_input_interpretation(&mut value);
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn legacy_input_fields_keep_their_meaning() {
        let p = parse(json!({"io": {"input_color_space": "sRGB", "input_cctf_decoding": false}}));
        assert_eq!(p.io.input_interpretation.mode, InputMode::Manual);
        assert_eq!(p.io.input_color_space, "sRGB");
        assert_eq!(parse(json!({"io": {"input_cctf_decoding": true}})).io.input_interpretation.mode, InputMode::Manual);
    }

    #[test]
    fn params_without_input_fields_read_files_automatically() {
        assert_eq!(parse(json!({})).io.input_interpretation.mode, InputMode::Auto);
        assert_eq!(parse(json!({"io": {"scan_film": true}})).io.input_interpretation.mode, InputMode::Auto);
        assert_eq!(RuntimeParams::default().io.input_interpretation.mode, InputMode::Auto);
    }

    #[test]
    fn a_stated_interpretation_wins() {
        // What the apps send: their defaults carry an interpretation, and an
        // old session's input fields come along without effect.
        let p = parse(json!({"io": {
            "input_interpretation": {"mode": "auto", "color_space": "sRGB", "linear": false},
            "input_color_space": "ProPhoto RGB", "input_cctf_decoding": false,
        }}));
        assert_eq!(p.io.input_interpretation.mode, InputMode::Auto);
        let p = parse(json!({"io": {"input_interpretation": {"mode": "assume", "color_space": "Display P3"}}}));
        assert_eq!(p.io.input_interpretation, spektrafilm_io::InputInterpretation::assume("Display P3", false));
    }

    #[test]
    fn loaded_colour_sets_the_pipeline_input() {
        let mut io = IoParams::default();
        io.apply_input_colour(&spektrafilm_io::InputColour::raw("rawler"));
        assert_eq!((io.input_color_space.as_str(), io.input_cctf_decoding), ("sRGB", false));
        // Manual pass-through leaves the fields to the params.
        let dir = std::env::temp_dir().join(format!("negs-io-manual-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("in.tiff");
        let img = spektrafilm_math::image::ImageBuf::new(2, 2);
        spektrafilm_io::save::save_image(&img, &path, None).unwrap();
        let loaded = spektrafilm_io::load_image_colour(
            &path,
            spektrafilm_io::Formats::Any,
            Default::default(),
            &spektrafilm_io::InputInterpretation::manual(),
        )
        .unwrap();
        let mut io = IoParams { input_color_space: "ACES2065-1".into(), input_cctf_decoding: true, ..IoParams::default() };
        io.apply_input_colour(&loaded.colour);
        assert_eq!((io.input_color_space.as_str(), io.input_cctf_decoding), ("ACES2065-1", true));
        std::fs::remove_dir_all(&dir).ok();
    }
}
