//! Production-allocator timing (separate from bounded_decode's heap counter).
//! <RAW> [repetitions=12]. Alternating order, one warmup for each path.
use spektrafilm_core::preview::downsample_area;
use spektrafilm_io::bounded::{self, Mode};
use std::time::Instant;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let bytes = std::fs::read(&args[1]).unwrap();
    let n: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(12);
    for run in 0..n {
        let modes = if run % 2 == 0 { [false, true] } else { [true, false] };
        for bounded in modes {
            let started = Instant::now();
            let image = if bounded {
                let kept = bounded::load_raw_bytes_kept(&bytes, Default::default(), 0, 12e6, Mode::LargeBayer).unwrap();
                kept.image.oriented(kept.orientation)
            } else {
                let (full, orientation) = spektrafilm_io::load_raw_bytes_unoriented(&bytes, Default::default()).unwrap();
                let target = bounded::kept_size(full.width, full.height, 0, 12e6);
                let small = downsample_area(&full, target);
                drop(full);
                small.oriented(orientation)
            };
            std::hint::black_box(&image);
            println!("{}", serde_json::json!({"file":args[1], "run":run, "warmup":run==0, "mode":if bounded {"bounded"} else {"legacy"}, "ms": started.elapsed().as_secs_f64()*1000.0}));
        }
    }
}
