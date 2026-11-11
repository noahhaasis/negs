//! The GPU's kept film side (ticket 41): a chain that reuses it renders bit
//! for bit what the whole chain renders, and only print, scan and later
//! settings reuse it.
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::{ComputeBackend, wgpu_backend::WgpuBackend};
use spektrafilm_math::{image::ImageBuf, precision::from_f64};
use std::path::Path;

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

fn pipeline(film: &str, params: &RuntimeParams) -> Pipeline {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    Pipeline::new_with_spectral(
        profile::load_profile_by_name(&dir, film).unwrap(),
        profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap(),
        params.clone(),
        &dir,
    )
    .unwrap()
}

fn bits(image: &ImageBuf) -> Vec<u32> {
    image.data.iter().map(|v| v.to_bits()).collect()
}

struct Step {
    what: &'static str,
    edit: Box<dyn Fn(&mut RuntimeParams)>,
    film: &'static str,
    /// The caller's frame name and slot.
    frame: u64,
    slot: u32,
    size: (u32, u32),
    hit: bool,
}

fn step(what: &'static str, hit: bool, edit: impl Fn(&mut RuntimeParams) + 'static) -> Step {
    Step { what, edit: Box::new(edit), film: "kodak_portra_400", frame: 1, slot: 0, size: (96, 64), hit }
}

#[test]
fn a_kept_film_side_renders_what_the_whole_chain_renders() {
    // Read when the backend is made; a budget, so this runs on every platform.
    unsafe { std::env::set_var("SPEKTRAFILM_FILM_CACHE", "64") };
    let Some(gpu) = WgpuBackend::new() else {
        eprintln!("no GPU adapter available; skipping the film-side cache test");
        return;
    };
    // Defaults: halation, DIR couplers, grain and print glare all on.
    let mut steps = vec![
        step("first render", false, |_| {}),
        step("the same render", true, |_| {}),
        step("print exposure", true, |p| p.enlarger.print_exposure = 1.3),
        step("magenta filter", true, |p| p.enlarger.m_filter_shift = 0.05),
        step("yellow filter", true, |p| p.enlarger.y_filter_shift = -0.04),
        step("print glare", true, |p| p.print_render.glare.percent = 0.05),
        step("scanner unsharp mask", true, |p| p.scanner.unsharp_mask = [0.6, 0.5]),
        step("scanner lens blur", true, |p| p.scanner.lens_blur = 0.7),
        step("linear output", true, |p| p.io.output_cctf_encoding = false),
        step("scan the film", true, |p| p.io.scan_film = true),
        step("print exposure, scanning the film", true, |p| p.enlarger.print_exposure = 0.8),
        step("print again", true, |p| p.io.scan_film = false),
        step("exposure compensation", false, |p| p.camera.exposure_compensation_ev = 0.3),
        step("print exposure after a miss", true, |p| p.enlarger.print_exposure = 1.1),
        step("halation", false, |p| p.film_render.halation.halation_amount = 0.5),
        step("scatter", false, |p| p.film_render.halation.scatter_amount = 0.7),
        step("DIR couplers", false, |p| p.film_render.dir_couplers.amount = 0.7),
        step("grain blur", false, |p| p.film_render.grain.blur = 0.8),
        step("print exposure, grain kept", true, |p| p.enlarger.print_exposure = 1.2),
        step("grain off", false, |p| p.film_render.grain.active = false),
        step("camera lens blur", false, |p| p.camera.lens_blur_um = 2.0),
        step("highlight boost", false, |p| p.film_render.halation.boost_ev = 0.5),
        step("auto exposure off", false, |p| p.camera.auto_exposure = false),
        step("input decoding", false, |p| p.io.input_cctf_decoding = true),
        step("Prep", false, |p| p.prep.shadows = -40.),
        step("print exposure with Prep", true, |p| p.enlarger.print_exposure = 0.9),
        step("Prep exposure only", false, |p| p.prep = spektrafilm_core::prep::PrepParams { exposure_ev: 0.4, ..Default::default() }),
        step("print exposure with Prep exposure", true, |p| p.enlarger.print_exposure = 1.0),
    ];
    steps.push(Step { film: "kodak_gold_200", ..step("another film", false, |_| {}) });
    steps.push(Step { film: "kodak_gold_200", ..step("its print exposure", true, |p| p.enlarger.print_exposure = 1.4) });
    steps.push(Step { film: "kodak_gold_200", frame: 2, ..step("another frame name", false, |_| {}) });
    steps.push(Step { film: "kodak_gold_200", frame: 2, slot: 1, ..step("another slot", false, |_| {}) });
    steps.push(Step { film: "kodak_gold_200", frame: 2, ..step("slot 0 kept beside slot 1", true, |_| {}) });
    steps.push(Step { film: "kodak_gold_200", frame: 2, size: (80, 64), ..step("another size", false, |_| {}) });
    steps.push(Step { film: "kodak_gold_200", frame: 2, size: (80, 64), ..step("that size again", true, |_| {}) });

    let mut params = RuntimeParams::default();
    params.camera.auto_exposure = true;
    let mut hits = gpu.film_cache_hits();
    for step in &steps {
        (step.edit)(&mut params);
        let source = image(step.size.0, step.size.1);
        let kept = pipeline(step.film, &params)
            .with_film_cache(step.slot, step.frame)
            .process_resident_borrowed(&source, &gpu)
            .expect("resident render");
        let hit = gpu.film_cache_hits() > hits;
        hits = gpu.film_cache_hits();
        // The whole chain, as without the cache; a render that doesn't ask
        // for the cache (a thumbnail) leaves the kept one alone.
        let whole = pipeline(step.film, &params).process_resident_borrowed(&source, &gpu).expect("resident render");
        assert_eq!(gpu.film_cache_hits(), hits, "{}: an uncached render hit", step.what);
        assert!(bits(&kept) == bits(&whole), "{}: kept film side renders differently", step.what);
        assert_eq!(hit, step.hit, "{}: hit {hit}, expected {}", step.what, step.hit);
    }

    // Another photograph: nothing is kept.
    gpu.release_film_cache();
    let source = image(80, 64);
    pipeline("kodak_gold_200", &params).with_film_cache(0, 2).process_resident_borrowed(&source, &gpu).unwrap();
    assert_eq!(gpu.film_cache_hits(), hits, "a released film side was reused");
}
