//! Run the normal CLI with a counting allocator. This measures requested live
//! Rust heap bytes, excluding allocator overhead, stacks and driver/BLAS memory.
//! cargo run --release -p spektrafilm-cli --example memory_profile \
//!   --features precision-f64 -- process ...
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};

struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn add(bytes: usize) {
    let live = LIVE.fetch_add(bytes, Relaxed) + bytes;
    PEAK.fetch_max(live, Relaxed);
}
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // Subtract before freeing: another thread may immediately reuse it.
        LIVE.fetch_sub(layout.size(), Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, old: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, old, size) };
        if !new.is_null() {
            if size >= old.size() {
                add(size - old.size());
            } else {
                LIVE.fetch_sub(old.size() - size, Relaxed);
            }
        }
        new
    }
}
#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;
mod cli {
    include!("../src/main.rs");
    pub fn run() -> anyhow::Result<()> {
        main()
    }
}
fn main() -> anyhow::Result<()> {
    let (tx, rx) = std::sync::mpsc::channel();
    let sampler = std::thread::spawn(move || {
        while matches!(
            rx.recv_timeout(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ) {
            eprintln!(
                "heap live={} peak={}",
                LIVE.load(Relaxed),
                PEAK.load(Relaxed)
            );
        }
    });
    let result = cli::run();
    let _ = tx.send(());
    let _ = sampler.join();
    eprintln!("heap final_peak={}", PEAK.load(Relaxed));
    result
}
