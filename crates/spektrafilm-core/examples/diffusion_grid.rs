//! The GPU diffusion FFT's memory by frame size, for the default filter and
//! the extremes test's: `cargo run --release -p spektrafilm-core --example diffusion_grid`.
use spektrafilm_core::params::RuntimeParams;

fn main() {
    if std::env::args().nth(1).as_deref() == Some("terms") {
        return print_terms();
    }
    let cases: [(&str, fn(&mut RuntimeParams)); 3] = [
        ("pro_mist default", |p| p.camera.diffusion_filter.active = true),
        ("spatial scale 3", |p| {
            p.camera.diffusion_filter.active = true;
            p.camera.diffusion_filter.spatial_scale = 3.0;
        }),
        ("scale 3, bloom 3", |p| {
            p.camera.diffusion_filter.active = true;
            p.camera.diffusion_filter.spatial_scale = 3.0;
            p.camera.diffusion_filter.bloom_size = 3.0;
        }),
    ];
    for (label, edit) in cases {
        let mut p = RuntimeParams::default();
        edit(&mut p);
        for (w, h) in [(1212, 808), (2048, 1365), (4243, 2828), (6240, 4160), (9520, 6336)] {
            let pitch = spektrafilm_core::stages::filming::pixel_size_um(p.camera.film_format_mm, w, h);
            let spec = spektrafilm_model::diffusion::diffusion_psf_spec(&p.camera.diffusion_filter.to_model(), pitch as f64, w, h);
            let r = spec.map_or(0, |s| s.radius as u64);
            // (h + 2r) × (w + 2r) cells, 20 bytes each (two complex grids, a spectrum),
            // before smoothing to an FFT size.
            let cells = (h as u64 + 2 * r) * (w as u64 + 2 * r);
            println!("{label:18} {w:5}x{h:<5} radius {r:5}  grid {:6.0} MiB  image buffer {:5.0} MiB", (cells * 20) as f64 / 1048576.0, (w as u64 * h as u64 * 12) as f64 / 1048576.0);
        }
    }
}

/// The default filter's PSF terms at 12 MP: decay length in pixels, and the
/// share of the scattered light in each (green channel), by unit volume
/// (an exponential `w/(2πl²)·e^(-r/l)` holds `w`).
fn print_terms() {
    let mut p = RuntimeParams::default();
    p.camera.diffusion_filter.active = true;
    let (w, h) = (4243, 2828);
    let pitch = spektrafilm_core::stages::filming::pixel_size_um(p.camera.film_format_mm, w, h);
    let spec = spektrafilm_model::diffusion::diffusion_psf_spec(&p.camera.diffusion_filter.to_model(), pitch as f64, w, h).unwrap();
    let total: f64 = spec.components[1].iter().map(|&(l, a)| a * 2.0 * std::f64::consts::PI * l * l).sum();
    println!("p_s {:.3}, radius {} px", spec.p_s, spec.radius);
    for &(l, a) in &spec.components[1] {
        println!("  length {l:8.1} px  share {:5.1}%", 100.0 * a * 2.0 * std::f64::consts::PI * l * l / total);
    }
}
