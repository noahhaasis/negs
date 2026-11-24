//! The Phase 0 / Phase 7 workloads (docs/shared-core-plan.md), on the Rust
//! side: `cargo run --release --bin bench -- <workload> [raw folder]`.
//! Prints one `name: median ms` line per measurement, then peak RSS.

use negs_core_diff::{embedded_preview, exact_fit, sheet_rules};
use negs_core_diff::{median_ms, peak_rss_mib};
use std::hint::black_box;
use std::path::{Path, PathBuf};

const RUNS: usize = 7;

#[global_allocator]
static ALLOC: negs_core_diff::Counting = negs_core_diff::Counting;

/// One run of `f`, counted: Rust's allocations and bytes, and the core's
/// when it counts them (built with `--features track`).
fn counted(label: &str, mut f: impl FnMut()) {
    negs_core_diff::take_allocations();
    let core_before = negs_core_sys::alloc_stats();
    f();
    let (n, bytes) = negs_core_diff::take_allocations();
    let core_after = negs_core_sys::alloc_stats();
    if core_after.tracked == 1 && core_after.total_count != core_before.total_count {
        println!(
            "  {label} allocations: Rust {n} ({:.1} MiB), core {}",
            bytes as f64 / 1048576.0,
            core_after.total_count - core_before.total_count
        );
    } else {
        println!("  {label} allocations: {n} ({:.1} MiB)", bytes as f64 / 1048576.0);
    }
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// 500 photographs: half with edited sidecars, a quarter rated and rejected,
/// a quarter without.
fn make_folder(root: &Path) -> PathBuf {
    let _ = std::fs::remove_dir_all(root);
    let photos = root.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    let fixtures = repo().join("docs/sidecar-fixtures");
    let edited = std::fs::read_to_string(fixtures.join("edited.negs")).unwrap();
    let rated = std::fs::read_to_string(fixtures.join("rated-reject.negs")).unwrap();
    for i in 0..500 {
        let name = format!("IMG_{i:04}.JPG");
        std::fs::write(photos.join(&name), name.as_bytes()).unwrap();
        let sidecar = match i % 4 {
            0 | 1 => Some(edited.replace("9b1e44c0", &format!("{:08x}", 0x9b1e_0000u32 + i))),
            2 => Some(rated.clone()),
            _ => None,
        };
        if let Some(text) = sidecar {
            std::fs::write(photos.join(format!("{name}.negs")), text).unwrap();
        }
    }
    photos
}

/// The library workloads for one implementation: `$lib` is the crate
/// (negs_library_rust, the Rust oracle; negs_library, the Odin core).
macro_rules! library_workloads {
    ($name:ident, $lib:ident, $label:literal) => {
        fn $name(scratch: &Path) {
            use $lib::{Config, Filter, FolderBin, Library, RatingFilter};
            let open = |photos: &Path, bin: &Path| -> Library {
                Library::new(
                    photos,
                    Config {
                        data: repo().join("data"),
                        app: "bench".into(),
                        bin: Box::new(FolderBin::new(bin.to_path_buf())),
                        pairing: true,
                        sidecars: true,
                        id_seed: Some(7),
                    },
                )
                .unwrap()
            };
            let photos = make_folder(scratch);
            let bin = scratch.join("bin");
            let open_ms = median_ms(RUNS, || {
                let library = open(&photos, &bin);
                let shown = library.shown(None);
                for item in &shown {
                    black_box(library.summary(item));
                    black_box(library.look(item));
                    black_box(library.name(item));
                }
                assert_eq!(shown.len(), 500);
            });
            println!(concat!($label, "library_open_500 (list + parse + summaries + looks): {:.2} ms"), open_ms);
            counted(concat!($label, "library_open_500"), || {
                let library = open(&photos, &bin);
                for item in &library.shown(None) {
                    black_box(library.summary(item));
                    black_box(library.look(item));
                }
            });
            println!("  peak_rss after opening: {:.1} MiB", peak_rss_mib());
            let mut library = open(&photos, &bin);
            let items = library.items();
            let write_ms = median_ms(RUNS, || {
                black_box(library.set_rating(&items, 4));
                black_box(library.undo());
            });
            println!(concat!($label, "library_rate_and_undo_500 (1000 sidecar writes): {:.2} ms"), write_ms);
            counted(concat!($label, "library_rate_and_undo_500"), || {
                black_box(library.set_rating(&items, 4));
                black_box(library.undo());
            });
            println!("  peak_rss after rating: {:.1} MiB", peak_rss_mib());
            let core = negs_core_sys::alloc_stats();
            if core.tracked == 1 {
                println!("  core heap live: {:.1} MiB in {} allocations", core.live_bytes as f64 / 1048576.0, core.live_count);
            }
            let filter_ms = median_ms(RUNS, || {
                for stars in 1..=5u8 {
                    library.set_filter(Filter { rating: RatingFilter::AtLeast(stars), ..Filter::default() });
                    black_box(library.shown(None));
                }
                library.set_filter(Filter::default());
            });
            println!(concat!($label, "library_filter_sort_5x500: {:.2} ms"), filter_ms);
            counted(concat!($label, "library_filter_sort_5x500"), || {
                for stars in 1..=5u8 {
                    library.set_filter(Filter { rating: RatingFilter::AtLeast(stars), ..Filter::default() });
                    black_box(library.shown(None));
                }
                library.set_filter(Filter::default());
            });
            drop(library);
            let _ = std::fs::remove_dir_all(scratch);
        }
    };
}

library_workloads!(library, negs_library_rust, "");
library_workloads!(core_library, negs_library, "core_");

fn names(count: usize) -> Vec<String> {
    (0..count).map(|i| format!("DSC_{:04}.NEF", 1000 + i)).collect()
}

fn marks() {
    let names = names(1000);
    let ms = median_ms(RUNS, || {
        for name in &names {
            for index in 0..5 {
                black_box(negs_marks::star_path(name, index));
            }
            black_box(negs_marks::cross_strokes(200.0, 140.0, name));
            black_box(negs_marks::pick_box_grid(264.0, 176.0, name));
        }
    });
    println!("marks_1000_names (5 stars + cross + box each): {ms:.2} ms");
    counted("marks_1000_names", || {
        for name in &names {
            for index in 0..5 {
                black_box(negs_marks::star_path(name, index));
            }
            black_box(negs_marks::cross_strokes(200.0, 140.0, name));
            black_box(negs_marks::pick_box_grid(264.0, 176.0, name));
        }
    });
}

/// The same marks from the Odin core, through negs-core-marks (the
/// allocations counted are the wrapper's Vecs; the core's own heap isn't
/// Rust's).
fn core_marks() {
    let names = names(1000);
    let ms = median_ms(RUNS, || {
        for name in &names {
            for index in 0..5 {
                black_box(negs_core_marks::star_path(name, index));
            }
            black_box(negs_core_marks::cross_strokes(200.0, 140.0, name));
            black_box(negs_core_marks::pick_box_grid(264.0, 176.0, name));
        }
    });
    println!("core_marks_1000_names (5 stars + cross + box each): {ms:.2} ms");
}

fn sheet() {
    let ratios: Vec<f64> = (0..1000).map(|i| [1.5, 0.6667, 1.0, 1.7778, 1.25][i % 5]).collect();
    let ms = median_ms(RUNS, || {
        for _ in 0..100 {
            black_box(sheet_rules::SheetLayout::new(&ratios, 1440.0, 180.0));
        }
    });
    println!("sheet_layout_1000_items_x100: {ms:.2} ms");
}

/// The same layouts on the core (negs-core-rules' sheet_rules).
fn core_sheet() {
    let ratios: Vec<f64> = (0..1000).map(|i| [1.5, 0.6667, 1.0, 1.7778, 1.25][i % 5]).collect();
    let ms = median_ms(RUNS, || {
        for _ in 0..100 {
            black_box(negs_core_rules::sheet_rules::SheetLayout::new(&ratios, 1440.0, 180.0));
        }
    });
    println!("core_sheet_layout_1000_items_x100: {ms:.2} ms");
}

fn fit() {
    let (w, h) = (6000u32, 4000u32);
    let rgba: Vec<u8> = (0..w as usize * h as usize * 4).map(|i| (i * 31 % 251) as u8).collect();
    let (tw, th) = exact_fit::target(1500.0, 1000.0, 1.0);
    let ms = median_ms(RUNS, || {
        black_box(exact_fit::resample(&rgba, w, h, tw, th));
    });
    println!("exact_fit_6000x4000_to_{tw}x{th}: {ms:.2} ms");
    // The core's (Phase 3b): the same pixels (core/diff/tests/exact_fit.rs).
    let ms = median_ms(RUNS, || {
        black_box(negs_core_rules::exact_fit::resample(&rgba, w, h, tw, th));
    });
    println!("core_exact_fit_6000x4000_to_{tw}x{th}: {ms:.2} ms");
    counted("core_exact_fit", || {
        black_box(negs_core_rules::exact_fit::resample(&rgba, w, h, tw, th));
    });
}

fn embedded(folder: &Path) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect();
    files.sort();
    let mut found = 0;
    let ms = median_ms(RUNS, || {
        found = 0;
        for path in &files {
            if let Some(previews) = embedded_preview::find(path) {
                found += 1;
                if let Some(jpeg) = previews.largest() {
                    black_box(embedded_preview::bytes(path, &jpeg));
                }
            }
        }
    });
    println!("embedded_preview_{}_raws (find + read largest, warm cache): {ms:.2} ms ({found} with previews)", files.len());
}

/// The same on the core (Phase 3c), through negs-core-rules as windows/ calls it.
fn core_embedded(folder: &Path) {
    use negs_core_rules::embedded_preview as core;
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder).unwrap().filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.is_file()).collect();
    files.sort();
    let mut found = 0;
    let ms = median_ms(RUNS, || {
        found = 0;
        for path in &files {
            if let Some(previews) = core::find(path) {
                found += 1;
                if let Some(jpeg) = previews.largest() {
                    black_box(core::bytes(path, &jpeg));
                }
            }
        }
    });
    println!("core_embedded_preview_{}_raws (find + read largest, warm cache): {ms:.2} ms ({found} with previews)", files.len());
}

/// Phase 4c: rendered_preview's key on the real data folder (data/, every
/// file stamped) for an edit state with the engine's real parameters, and
/// the checksum of a 12 MP RGBA frame; the frozen Rust, then the core.
fn caches() {
    let defaults: serde_json::Value = serde_json::from_str(include_str!("../../tests/state_common/engine_defaults.json")).unwrap();
    let state = negs_state_oracle::state::EditState { params: defaults["defaults"].clone(), ..Default::default() };
    let repo = repo();
    let source = repo.join("README.md");
    let paths = negs_state_oracle::engine::EnginePaths {
        helper: repo.join("core/include/negs_core.h"),
        exporter: repo.join("Cargo.toml"),
        data: repo.join("data"),
        fast_exporter: None,
    };
    let files = walk(&paths.data);
    let rust = negs_state_oracle::rendered_preview::key(&source, &state, &paths).unwrap();
    let ms = median_ms(RUNS, || {
        black_box(negs_state_oracle::rendered_preview::key(&source, &state, &paths));
    });
    println!("rendered_preview_key_{files}_data_files: {ms:.2} ms ({} bytes)", rust.len());
    counted("rendered_preview_key", || {
        black_box(negs_state_oracle::rendered_preview::key(&source, &state, &paths));
    });
    let core_key = || {
        // As the app's wrapper: the state's serde text, then the core.
        let json = serde_json::to_vec(&state).unwrap();
        negs_core_rules::rendered_preview::key(&source, &paths.helper, &paths.data, negs_core_rules::rendered_preview::KeyState::State(&json))
    };
    assert_eq!(core_key(), Ok(Some(rust)), "the core's key is the Rust's");
    let ms = median_ms(RUNS, || {
        black_box(core_key()).ok();
    });
    println!("core_rendered_preview_key_{files}_data_files: {ms:.2} ms");
    counted("core_rendered_preview_key", || {
        black_box(core_key()).ok();
    });

    let pixels: Vec<u8> = (0..4000usize * 3000 * 4).map(|i| (i * 31 % 251) as u8).collect();
    let rust_sum = || {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        pixels.hash(&mut hash);
        hash.finish()
    };
    assert_eq!(rust_sum(), negs_core_rules::rendered_preview::checksum(&pixels), "the core's checksum is the Rust's");
    let ms = median_ms(RUNS, || {
        black_box(rust_sum());
    });
    println!("preview_checksum_12mp_rgba: {ms:.2} ms");
    let ms = median_ms(RUNS, || {
        black_box(negs_core_rules::rendered_preview::checksum(black_box(&pixels)));
    });
    println!("core_preview_checksum_12mp_rgba: {ms:.2} ms");
}

fn walk(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| match e.file_type() {
                    Ok(t) if t.is_dir() => walk(&e.path()),
                    Ok(t) if t.is_file() => 1,
                    _ => 0,
                })
                .sum()
        })
        .unwrap_or(0)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(String::as_str).unwrap_or("all");
    let scratch = std::env::temp_dir().join(format!("negs-core-bench-{}", std::process::id()));
    match which {
        "library" => {
            library(&scratch);
            core_library(&scratch);
        }
        // One implementation per process, for its own peak RSS.
        "library-rust" => library(&scratch),
        "library-core" => core_library(&scratch),
        "marks" => {
            marks();
            core_marks();
        }
        "sheet" => {
            sheet();
            core_sheet();
        }
        "fit" => fit(),
        "caches" => caches(),
        "embedded" => {
            let folder = Path::new(args.get(2).expect("raw folder"));
            embedded(folder);
            core_embedded(folder);
        }
        "all" => {
            library(&scratch);
            core_library(&scratch);
            marks();
            core_marks();
            sheet();
            core_sheet();
        }
        other => panic!("unknown workload {other}"),
    }
    println!("peak_rss: {:.1} MiB", peak_rss_mib());
}
