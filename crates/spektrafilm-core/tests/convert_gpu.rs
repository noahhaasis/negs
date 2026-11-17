//! A scanned negative on the GPU (ticket 33): the resident chain's convert
//! front pass renders what the CPU's f64 converter renders, and keeps its
//! film side while only the print and scan change.
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{cpu_backend::CpuBackend, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::{from_f64, to_f64}};
use std::path::Path;

const ROUTE: &str = "input > convert-film > print > scan";

fn params() -> RuntimeParams {
    let mut params = RuntimeParams::default();
    params.workflow.route = ROUTE.into();
    // Glare draws its own random numbers per engine.
    params.print_render.glare.active = false;
    params.io.input_cctf_decoding = false;
    params
}

fn pipeline(params: &RuntimeParams) -> Pipeline {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, "kodak_portra_400").unwrap(),
        profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
        params.clone(),
        &dir,
    )
    .unwrap()
}

/// A scan of film the model made: densities across the film's range, clear
/// film down the left edge.
fn negative(width: u32, height: u32) -> ImageBuf {
    let converter = pipeline(&params()).film_converter();
    ImageBuf::from_data(
        width,
        height,
        (0..width * height)
            .flat_map(|i| {
                let (x, y) = ((i % width) as f64 / (width - 1) as f64, (i / width) as f64 / (height - 1) as f64);
                let cmy = if x < 0.08 { [0.0; 3] } else { [0.05 + 1.6 * x, 0.1 + 1.4 * x * y, 0.2 + 1.2 * y] };
                converter.forward(cmy)
            })
            .map(from_f64)
            .collect(),
    )
}

fn bits(image: &ImageBuf) -> Vec<u32> {
    image.data.iter().map(|v| v.to_bits()).collect()
}

#[test]
fn a_scanned_negative_prints_on_the_gpu_as_on_the_cpu() {
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let source = negative(96, 64);
    let pipeline = pipeline(&params());
    // The resident chain, not a per-stage fallback with the CPU's convert.
    let on_gpu = pipeline.process_resident_borrowed(&source, &gpu).expect("the resident chain converts");
    let (on_cpu, _) = pipeline.process_with_backend(source, &CpuBackend);
    let worst = on_gpu.data.iter().zip(&on_cpu.data).map(|(g, c)| (to_f64(*g) - to_f64(*c)).abs()).fold(0.0, f64::max);
    // f32 against f64, the convert and the whole print and scan: a fraction
    // of a 16-bit step's worth on the photograph.
    eprintln!("GPU against CPU: {worst}");
    assert!(worst < 2e-4, "the GPU's print is {worst} from the CPU's");
}

#[test]
fn a_scanned_negative_keeps_its_film_side_while_only_the_print_changes() {
    unsafe { std::env::set_var("SPEKTRAFILM_FILM_CACHE", "64") };
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter; skipping");
        return;
    };
    let source = negative(96, 64);
    let steps: Vec<(&str, bool, Box<dyn Fn(&mut RuntimeParams)>)> = vec![
        ("first render", false, Box::new(|_| {})),
        ("magenta filter", true, Box::new(|p| p.enlarger.m_filter_shift = 4.0)),
        ("print exposure", true, Box::new(|p| p.enlarger.print_exposure = 1.2)),
        ("scan light", false, Box::new(|p| p.film_render.convert.scan_illuminant = "D50".into())),
        ("scan exposure", false, Box::new(|p| p.film_render.convert.exposure_compensation_ev = -0.4)),
        ("film base", false, Box::new(|p| p.film_render.base.magenta = 0.9)),
        ("yellow filter", true, Box::new(|p| p.enlarger.y_filter_shift = -3.0)),
        // Not the convert route's: the film side doesn't change.
        ("grain", true, Box::new(|p| p.film_render.grain.active = !p.film_render.grain.active)),
        ("halation", true, Box::new(|p| p.film_render.halation.halation_amount = 0.4)),
        ("camera exposure", true, Box::new(|p| p.camera.exposure_compensation_ev = 0.5)),
        ("Prep", true, Box::new(|p| p.prep.shadows = -40.)),
    ];
    let mut params = params();
    let mut hits = gpu.film_cache_hits();
    for (what, expected, edit) in &steps {
        edit(&mut params);
        let kept = pipeline(&params).with_film_cache(0, 7).process_resident_borrowed(&source, &gpu).expect("resident render");
        let hit = gpu.film_cache_hits() > hits;
        hits = gpu.film_cache_hits();
        let whole = pipeline(&params).process_resident_borrowed(&source, &gpu).expect("resident render");
        assert!(bits(&kept) == bits(&whole), "{what}: kept film side renders differently");
        assert_eq!(hit, *expected, "{what}: hit {hit}, expected {expected}");
    }
}
