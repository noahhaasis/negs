//! Ticket 29: what a calibrated pipeline costs to build, clone and keep.
//! `cargo run --release -p spektrafilm-core --example pipeline_build -- <data-dir> [reps=5]`
//! Prints one JSON line per stock and a summary. Heap is counted by a global
//! allocator, so "retained" is what one cached `Pipeline` holds.
use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile, spectral_service};
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::Path;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::time::Instant;

struct Counting;
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let now = LIVE.fetch_add(l.size() as isize, Ordering::Relaxed) + l.size() as isize;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size() as isize, Ordering::Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, n: usize) -> *mut u8 {
        let now = LIVE.fetch_add(n as isize - l.size() as isize, Ordering::Relaxed) + n as isize - l.size() as isize;
        PEAK.fetch_max(now, Ordering::Relaxed);
        unsafe { System.realloc(p, l, n) }
    }
}
#[global_allocator]
static A: Counting = Counting;

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}
fn min(v: &[f64]) -> f64 {
    v.iter().copied().fold(f64::INFINITY, f64::min)
}
fn median(v: &[f64]) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

fn build(data: &Path, film: &str, paper: &str, params: &RuntimeParams) -> (Pipeline, f64, f64) {
    let t = Instant::now();
    let f = profile::load_profile_by_name(data, film).unwrap();
    let p = profile::load_profile_by_name(data, paper).unwrap();
    let loaded = ms(t.elapsed());
    let pipeline = Pipeline::new_with_spectral(f, p, params.clone(), data).unwrap();
    (pipeline, loaded, ms(t.elapsed()))
}

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let data = Path::new(&args[1]);
    let reps: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(5);
    let paper = "kodak_portra_endura";
    let mut films: Vec<String> = std::fs::read_dir(data.join("profiles"))
        .unwrap()
        .map(|e| e.unwrap().path().file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    films.sort();
    let params = RuntimeParams::default();
    split(data, "kodak_portra_400");

    // Parts of a build that don't depend on the stock.
    let mut spectra_ms = Vec::new();
    for _ in 0..reps {
        let t = Instant::now();
        let m = spectral_service::upsampling_method(&params.settings.rgb_to_raw_method).unwrap();
        std::hint::black_box(spectral_service::load_method_lut(data, m).unwrap());
        spectra_ms.push(ms(t.elapsed()));
    }
    let m = spectral_service::upsampling_method(&params.settings.rgb_to_raw_method).unwrap();
    let before = LIVE.load(Ordering::Relaxed);
    let spectra = spectral_service::load_method_lut(data, m).unwrap();
    let spectra_bytes = LIVE.load(Ordering::Relaxed) - before;
    drop(spectra);

    let (mut builds, mut loads, mut clones, mut with_params, mut retained, mut peaks) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut kept = Vec::new();
    for film in &films {
        let (mut b, mut l, mut c, mut w) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let mut bytes = 0;
        let mut peak = 0;
        for rep in 0..reps {
            let before = LIVE.load(Ordering::Relaxed);
            PEAK.store(before, Ordering::Relaxed);
            let (pipeline, loaded, total) = build(data, film, paper, &params);
            peak = PEAK.load(Ordering::Relaxed) - before;
            bytes = LIVE.load(Ordering::Relaxed) - before;
            l.push(loaded);
            b.push(total);
            let t = Instant::now();
            let copy = pipeline.clone();
            c.push(ms(t.elapsed()));
            let t = Instant::now();
            std::hint::black_box(copy.with_params(params.clone()));
            w.push(ms(t.elapsed()));
            if rep == 0 {
                kept.push(pipeline);
            }
        }
        println!(
            "{}",
            serde_json::json!({"film": film, "build_ms_min": min(&b), "build_ms_median": median(&b),
                "profiles_ms": min(&l), "clone_ms": min(&c), "with_params_ms": min(&w),
                "retained_kb": bytes as f64 / 1024.0, "peak_kb": peak as f64 / 1024.0})
        );
        builds.push(min(&b));
        loads.push(min(&l));
        clones.push(min(&c));
        with_params.push(min(&w));
        retained.push(bytes as f64);
        peaks.push(peak as f64);
    }
    let kb: Vec<f64> = retained.iter().map(|b| b / 1024.0).collect();
    println!(
        "{}",
        serde_json::json!({"summary": true, "stocks": films.len(), "reps": reps,
            "build_ms_median": median(&builds), "build_ms_min": min(&builds),
            "build_ms_max": builds.iter().copied().fold(0.0, f64::max),
            "profiles_ms_median": median(&loads), "spectra_lut_load_ms_min": min(&spectra_ms),
            "spectra_lut_kb": spectra_bytes as f64 / 1024.0,
            "clone_ms_median": median(&clones), "with_params_ms_median": median(&with_params),
            "retained_kb_median": median(&kb), "retained_kb_max": kb.iter().copied().fold(0.0, f64::max),
            "peak_kb_median": median(&peaks) / 1024.0,
            "all_kept_mb": (kept.len() as f64 * median(&kb)) / 1024.0})
    );
}

/// The stock-independent and per-stock parts of a hanatos2025 build, for one film.
fn split(data: &Path, film: &str) {
    let params = RuntimeParams::default();
    let m = spectral_service::upsampling_method(&params.settings.rgb_to_raw_method).unwrap();
    let lut = spectral_service::load_method_lut(data, m).unwrap();
    let f = profile::load_profile_by_name(data, film).unwrap();
    let sens: Vec<[f64; 3]> = f.log_sensitivity_f64().iter().map(|r| r.map(|v| { let x = 10f64.powf(v); if x.is_nan() { 0.0 } else { x } })).collect();
    let mut t_neutral = Vec::new();
    let mut t_lut = Vec::new();
    for _ in 0..7 {
        let t = Instant::now();
        let neutral = spectral_service::neutral_spectrum(&lut, (0.3127, 0.3290));
        t_neutral.push(ms(t.elapsed()));
        let t = Instant::now();
        std::hint::black_box(spectral_service::compute_tc_lut_with_window(&lut, &sens, &f.data.hanatos2025_adaptation_window_params, &neutral));
        t_lut.push(ms(t.elapsed()));
    }
    let tc = spectral_service::compute_tc_lut(&lut, &sens);
    let gamut = spektrafilm_core::input_gamut::InputGamutCompress::from_params(&params.io.input_gamut_compress).unwrap();
    let mut t_remap = Vec::new();
    for _ in 0..7 {
        let t = Instant::now();
        std::hint::black_box(gamut.remap(&tc, [0.3324, 0.3474]));
        t_remap.push(ms(t.elapsed()));
    }
    println!("{}", serde_json::json!({"active": gamut.is_active(), "remap_ms_min": min(&t_remap), "remap_ms_median": median(&t_remap)}));
    println!("{}", serde_json::json!({"split": film, "neutral_ms": min(&t_neutral), "tc_lut_window_ms": min(&t_lut)}));
}
