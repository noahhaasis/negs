//! Inspect Windows admission decisions without allocating large images.
//! This does not measure live free VRAM or prove watchdog safety.
//! cargo run --release -p spektrafilm-gpu --example windows_capacity

#[cfg(all(target_os = "windows", feature = "wgpu-backend"))]
fn main() {
    use spektrafilm_gpu::{ComputeBackend, wgpu_backend::WgpuBackend};
    let backend = WgpuBackend::new().expect("a Vulkan adapter is required");
    for (width, height) in [
        (6048, 4024),
        (12000, 8000),
        (14400, 10000),
        (15000, 11930),
        (15000, 12000),
    ] {
        match backend.image_capacity(width, height) {
            Ok(()) => println!("{width}x{height}: admitted before effect-specific checks"),
            Err(reason) => println!("{width}x{height}: CPU fallback: {reason}"),
        }
    }
}

#[cfg(not(all(target_os = "windows", feature = "wgpu-backend")))]
fn main() {
    eprintln!("This diagnostic requires Windows and the wgpu-backend feature.");
}
