//! The film-side cache on Windows (ticket 41): its budget is an eighth of
//! the GPU admission budget, and a kept film side gives way to a render that
//! fits the GPU only without it, rather than sending that render to the CPU.
#![cfg(target_os = "windows")]
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{ComputeBackend, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::Path;

const MIB: u64 = 1 << 20;
/// `SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB`: the admission budget for this test.
const LIMIT: u64 = 1024 * MIB;

fn image(width: u32, height: u32) -> ImageBuf {
    ImageBuf::from_data(
        width,
        height,
        (0..width * height)
            .flat_map(|i| {
                let x = (i % width) as f64 / (width - 1) as f64;
                let y = (i / width) as f64 / (height - 1) as f64;
                [0.02 + x * 0.9, 0.01 + x * y * 1.4, 0.05 + y * 0.5]
            })
            .map(from_f64)
            .collect(),
    )
}

/// `capacity::image`'s estimate: four RGB float images and a fixed reserve.
fn estimate(width: u32, height: u32) -> u64 {
    u64::from(width) * u64::from(height) * 12 * 4 + 128 * MIB
}

/// The tallest 4096-wide image whose estimate is at most `bytes`.
fn tallest(bytes: u64) -> (u32, u32) {
    let height = ((bytes - 128 * MIB) / (4096 * 48)) as u32;
    assert!(estimate(4096, height) <= bytes && estimate(4096, height + 1) > bytes);
    (4096, height)
}

#[test]
fn a_kept_film_side_gives_way_to_a_render_that_needs_its_memory() {
    // Read when the backend is made: no budget override, so the default
    // applies, from an admission budget lowered to 1 GiB.
    unsafe {
        std::env::remove_var("SPEKTRAFILM_FILM_CACHE");
        std::env::set_var("SPEKTRAFILM_WGPU_MEMORY_LIMIT_MB", (LIMIT / MIB).to_string());
    }
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping the film-side admission test");
        return;
    };
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let mut params = RuntimeParams::default();
    params.camera.auto_exposure = true;
    let pipeline = || {
        Pipeline::new_with_spectral(
            profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
            profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
            params.clone(),
            &dir,
        )
        .unwrap()
        .with_film_cache(0, 1)
    };
    let source = image(1024, 1024);
    pipeline().process_resident_borrowed(&source, &gpu).expect("resident render");
    let kept = gpu.film_cache_bytes();
    assert_eq!(kept, 1024 * 1024 * 12, "a 12 MiB film side fits the default budget, 128 MiB");
    let hits = gpu.film_cache_hits();
    pipeline().process_resident_borrowed(&source, &gpu).expect("resident render");
    assert_eq!(gpu.film_cache_hits(), hits + 1, "and is reused");

    // Too big even alone: refused, and the kept film side stays.
    let (w, h) = tallest(LIMIT);
    assert!(gpu.image_capacity(w, h + 1).is_err());
    assert_eq!(gpu.film_cache_bytes(), kept, "a refused render lets nothing go");
    // Fits beside the kept film side: it stays.
    let (w, h) = tallest(LIMIT - kept);
    assert!(gpu.image_capacity(w, h).is_ok());
    assert_eq!(gpu.film_cache_bytes(), kept, "a render that fits beside it keeps it");
    // Fits only without it: admitted, and the film side is let go.
    assert!(estimate(w, h + 1) <= LIMIT);
    assert!(gpu.image_capacity(w, h + 1).is_ok(), "admitted once the film side is gone");
    assert_eq!(gpu.film_cache_bytes(), 0, "the film side gave way");

    // The next render misses and keeps it again.
    pipeline().process_resident_borrowed(&source, &gpu).expect("resident render");
    assert_eq!(gpu.film_cache_hits(), hits + 1, "a released film side was reused");
    assert_eq!(gpu.film_cache_bytes(), kept, "kept again");
}
