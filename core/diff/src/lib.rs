//! Shared helpers for the core's differential tests and benchmarks.

// windows/src's pure modules as they were before Phase 3 moved them onto
// the core: frozen copies (core/diff/oracle), the benches' Rust side.
#[allow(dead_code)]
#[path = "../oracle/exact_fit.rs"]
pub mod exact_fit;
#[allow(dead_code)]
#[path = "../oracle/embedded_preview.rs"]
pub mod embedded_preview;
#[allow(dead_code)]
#[path = "../oracle/sheet_rules.rs"]
pub mod sheet_rules;
#[allow(dead_code)]
#[path = "../oracle/tray_rules.rs"]
pub mod tray_rules;
#[allow(dead_code)]
#[path = "../oracle/compare.rs"]
pub mod compare;
/// model/compare.rs's pure helpers, copied verbatim (tests/compare.rs checks).
#[allow(dead_code)]
#[path = "../oracle/model_compare_pure.rs"]
pub mod model_compare;

use std::time::Instant;

/// The median of `runs` timings of `f`, in milliseconds.
pub fn median_ms(runs: usize, mut f: impl FnMut()) -> f64 {
    let mut times: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    times.sort_by(|a, b| a.partial_cmp(b).unwrap());
    times[times.len() / 2]
}

/// Peak resident memory of this process so far, in MiB: the peak working
/// set on Windows.
#[cfg(windows)]
pub fn peak_rss_mib() -> f64 {
    #[repr(C)]
    #[derive(Default)]
    struct Counters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        rest: [usize; 7],
    }
    unsafe extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Counters, cb: u32) -> i32;
    }
    let mut c = Counters { cb: std::mem::size_of::<Counters>() as u32, ..Default::default() };
    unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    c.peak_working_set_size as f64 / 1048576.0
}

/// Peak resident memory of this process so far, in MiB (getrusage).
#[cfg(not(windows))]
pub fn peak_rss_mib() -> f64 {
    #[repr(C)]
    struct Timeval {
        sec: i64,
        usec: i64,
    }
    #[repr(C)]
    struct Rusage {
        utime: Timeval,
        stime: Timeval,
        maxrss: i64,
        rest: [i64; 13],
    }
    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }
    let mut usage: Rusage = unsafe { std::mem::zeroed() };
    unsafe { getrusage(0, &mut usage) };
    // macOS reports bytes, Linux kibibytes.
    if cfg!(target_os = "macos") { usage.maxrss as f64 / 1048576.0 } else { usage.maxrss as f64 / 1024.0 }
}

/// A global allocator that counts, for the benchmarks' allocation numbers.
pub struct Counting;

pub static ALLOCATIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub static ALLOCATED_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(size as u64, std::sync::atomic::Ordering::Relaxed);
        unsafe { std::alloc::System.realloc(ptr, layout, size) }
    }
}

/// Allocations and bytes since the last call.
pub fn take_allocations() -> (u64, u64) {
    use std::sync::atomic::Ordering::Relaxed;
    (ALLOCATIONS.swap(0, Relaxed), ALLOCATED_BYTES.swap(0, Relaxed))
}
