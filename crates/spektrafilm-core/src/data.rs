/// Re-export baked spectral constants from spektrafilm-math.
pub use spektrafilm_math::spectral::{
    CMF_X, CMF_Y, CMF_Z, ILLUMINANT_D50, ILLUMINANT_D55, ILLUMINANT_D65, N_WAVELENGTHS,
    WAVELENGTH_MAX, WAVELENGTH_MIN, WAVELENGTH_STEP,
};

/// Log exposure axis: 256 uniformly spaced values from -3 to +4 EV.
/// Matches Python `config.LOG_EXPOSURE = np.linspace(-3, 4, 256)`.
pub const LOG_EXPOSURE_MIN: f32 = -3.0;
pub const LOG_EXPOSURE_MAX: f32 = 4.0;
pub const LOG_EXPOSURE_LEN: usize = 256;
