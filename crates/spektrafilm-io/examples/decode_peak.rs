//! The RAW decode's peak heap use, as the browser engine decodes it:
//! `cargo run --release -p spektrafilm-io --example decode_peak -- <file.raf>`.
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let n = LIVE.fetch_add(l.size(), Relaxed) + l.size();
        PEAK.fetch_max(n, Relaxed);
        unsafe { System.alloc(l) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        LIVE.fetch_sub(l.size(), Relaxed);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, s: usize) -> *mut u8 {
        if s > l.size() { let n = LIVE.fetch_add(s - l.size(), Relaxed) + s - l.size(); PEAK.fetch_max(n, Relaxed); } else { LIVE.fetch_sub(l.size() - s, Relaxed); }
        unsafe { System.realloc(p, l, s) }
    }
}
#[global_allocator]
static A: Counting = Counting;
fn mib(n: usize) -> f64 { n as f64 / 1048576.0 }
fn main() {
    let path = std::env::args().nth(1).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let started = std::time::Instant::now();
    let img = spektrafilm_io::load_raw_bytes(&bytes, Default::default()).unwrap();
    let ms = started.elapsed().as_secs_f64() * 1000.0;
    // FNV-1a over the pixels' bits: a change in the decode shows.
    let hash = img.data.iter().fold(0xcbf29ce484222325u64, |h, v| (h ^ v.to_bits() as u64).wrapping_mul(0x100000001b3));
    println!("file {:.0} MiB; image {}x{} = {:.0} MiB; decode peak above file {:.0} MiB; live after {:.0} MiB; {ms:.0} ms; pixels {hash:016x}",
        mib(bytes.len()), img.width, img.height, mib(img.data.len() * 4), mib(PEAK.load(Relaxed) - base), mib(LIVE.load(Relaxed) - base));
}
