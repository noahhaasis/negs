//! Capacity rejection must use CPU for the entire pipeline, preserve the
//! working-size rescale exactly once, and avoid all GPU stage allocations.
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{ComputeBackend, FilmChainParams, Lut3D, cpu_backend::CpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::{
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

struct RejectGpu {
    reject_image: bool,
    checked_width: AtomicUsize,
}

impl ComputeBackend for RejectGpu {
    fn image_capacity(&self, width: u32, _: u32) -> Result<(), String> {
        self.checked_width.store(width as usize, Ordering::Relaxed);
        if self.reject_image {
            Err("image exceeds test budget".into())
        } else {
            Ok(())
        }
    }
    fn film_chain_capacity(&self, _: &FilmChainParams<'_>) -> Result<(), String> {
        Err("FFT exceeds test budget".into())
    }
    fn try_run_film_chain(&self, _: &FilmChainParams<'_>) -> Option<ImageBuf> {
        panic!("rejected resident work reached the GPU")
    }
    fn colorspace_convert(&self, _: &ImageBuf, _: &[[f32; 3]; 3]) -> ImageBuf {
        panic!("GPU stage")
    }
    fn cctf_encode_srgb(&self, _: &ImageBuf) -> ImageBuf {
        panic!("GPU stage")
    }
    fn cctf_decode_srgb(&self, _: &ImageBuf) -> ImageBuf {
        panic!("GPU stage")
    }
    fn gaussian_blur(&self, _: &ImageBuf, _: f32) -> ImageBuf {
        panic!("GPU stage")
    }
    fn table_lookup(&self, _: &ImageBuf, _: &[f32], _: &[[f32; 3]]) -> ImageBuf {
        panic!("GPU stage")
    }
    fn lut3d_interp(&self, _: &ImageBuf, _: &Lut3D) -> ImageBuf {
        panic!("GPU stage")
    }
    fn is_gpu(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "rejected test GPU"
    }
}

#[test]
fn capacity_rejections_match_explicit_cpu_without_double_rescale() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let film = profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap();
    let paper = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
    let mut params = RuntimeParams::default();
    params.io.input_color_space = "sRGB".into();
    params.io.input_cctf_decoding = false;
    params.io.upscale_factor = 1.5;
    let pipeline = Pipeline::new_with_spectral(film, paper, params, &dir).unwrap();
    let mut image = ImageBuf::new(32, 24);
    for (i, value) in image.data.iter_mut().enumerate() {
        *value = from_f64(0.02 + (i % 100) as f64 / 100.0);
    }
    let expected = pipeline.process(image.clone(), &CpuBackend);
    assert_eq!((expected.width, expected.height), (48, 36));
    for reject_image in [true, false] {
        let backend = RejectGpu {
            reject_image,
            checked_width: AtomicUsize::new(0),
        };
        assert!(
            pipeline
                .process_resident_borrowed(&image, &backend)
                .is_none()
        );
        let (actual, used_backend) = pipeline.process_with_backend(image.clone(), &backend);
        assert_eq!(used_backend, "CPU (rayon)");
        assert_eq!(backend.checked_width.load(Ordering::Relaxed), 48);
        assert_eq!(
            (actual.width, actual.height),
            (expected.width, expected.height)
        );
        assert_eq!(actual.data, expected.data);
    }
}
