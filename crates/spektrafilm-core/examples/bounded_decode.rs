//! Paired native harness: <legacy|bounded|force|compare> <RAW> [repetitions=1]
//! [output-prefix]. Timings include an instrumented allocator: NOT production
//! timing. Fresh processes measure peak; repetitions report warm distributions.
use std::{alloc::{GlobalAlloc, Layout, System}, sync::atomic::{AtomicUsize, Ordering::Relaxed}, time::Instant};
use spektrafilm_io::{bounded::{self, Mode}, load_raw_bytes_unoriented};
use spektrafilm_core::preview::downsample_area;
use spektrafilm_math::{image::ImageBuf, precision::to_f32};
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() { let live = LIVE.fetch_add(l.size(), Relaxed) + l.size(); PEAK.fetch_max(live, Relaxed); }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { LIVE.fetch_sub(l.size(), Relaxed); unsafe { System.dealloc(p, l) } }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(p, l, size) };
        if !new.is_null() {
            if size > l.size() { let live = LIVE.fetch_add(size - l.size(), Relaxed) + size - l.size(); PEAK.fetch_max(live, Relaxed); }
            else { LIVE.fetch_sub(l.size() - size, Relaxed); }
        }
        new
    }
}
#[global_allocator] static ALLOC: Counting = Counting;
fn decode(bytes: &[u8], mode: &str) -> (ImageBuf, (u32, u32), bool, Option<(f64, f64)>) {
    if mode == "legacy" {
        let (full, rotation) = load_raw_bytes_unoriented(bytes, Default::default()).unwrap();
        let size = (full.width, full.height);
        let target = bounded::kept_size(size.0, size.1, 0, 12e6);
        let small = if target == size { full } else {
            let small = downsample_area(&full, target);
            drop(full); // Match decode_photo: full RGB dies BEFORE orientation.
            small
        };
        (small.oriented(rotation), size, false, None)
    } else {
        let r = bounded::load_raw_bytes_kept(bytes, Default::default(), 0, 12e6,
            if mode == "force" { Mode::ForceBayer } else { Mode::LargeBayer }).unwrap();
        (r.image.oriented(r.orientation), r.full_size, r.bounded, Some((r.decode_ms, r.develop_ms)))
    }
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let mode = args.get(1).expect("mode");
    assert!(["legacy", "bounded", "force", "compare"].contains(&mode.as_str()));
    let bytes = std::fs::read(&args[2]).unwrap();
    let repetitions: usize = args.get(3).map(|s| s.parse().unwrap()).unwrap_or(1);
    for run in 0..repetitions {
        let base = LIVE.load(Relaxed); PEAK.store(base, Relaxed);
        let start = Instant::now();
        let (image, full, selected, stages) = decode(&bytes, if mode == "compare" { "bounded" } else { mode });
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let peak = PEAK.load(Relaxed).saturating_sub(base) as f64 / 1048576.0;
        let live = LIVE.load(Relaxed).saturating_sub(base) as f64 / 1048576.0;
        let hash = image.data.iter().fold(0xcbf29ce484222325u64, |h, &v| (h ^ to_f32(v).to_bits() as u64).wrapping_mul(0x100000001b3));
        println!("{}", serde_json::json!({"file":args[2],"mode":mode,"run":run,"full":full,"kept":[image.width,image.height],"bounded":selected,"fileBytes":bytes.len(),"ms":ms,"peakAboveFileMiB":peak,"liveMiB":live,"hash":format!("{hash:016x}"),"sensorAndDevelopMs":stages}));
        if mode == "compare" {
            let expected = decode(&bytes, "legacy").0;
            assert_eq!((image.width,image.height), (expected.width,expected.height));
            let mut max = 0f64; let mut sum = 0f64; let mut changed = 0usize; let mut heat = Vec::new();
            for (y, (a,b)) in image.data.chunks(image.width as usize * 3).zip(expected.data.chunks(image.width as usize * 3)).enumerate() {
                let mut row_max = 0f64;
                for (&a,&b) in a.iter().zip(b) {
                    assert!(a.is_finite() && b.is_finite());
                    let d = (f64::from(to_f32(a)) - f64::from(to_f32(b))).abs();
                    max = max.max(d); row_max = row_max.max(d); sum += d; changed += usize::from(to_f32(a).to_bits() != to_f32(b).to_bits());
                }
                heat.push(serde_json::json!({"row":y,"max":row_max}));
            }
            println!("{}",serde_json::json!({"comparison":true,"max":max,"mean":sum / image.data.len() as f64,"changedScalars":changed}));
            if let Some(prefix) = args.get(4) { std::fs::write(format!("{prefix}-rows.json"), serde_json::to_vec(&heat).unwrap()).unwrap(); }
            assert_eq!(changed, 0, "bounded output differs from existing core downsample");
        }
    }
}
