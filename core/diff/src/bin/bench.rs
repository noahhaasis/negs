//! The Phase 0 / Phase 7 workloads (docs/shared-core-plan.md), on the Rust
//! side: `cargo run --release --bin bench -- <workload> [raw folder]`.
//! Prints one `name: median ms` line per measurement, then peak RSS.

use negs_core_diff::{embedded_preview, exact_fit, sheet_rules};
use negs_core_diff::{median_ms, peak_rss_mib};
use negs_library::{Config, Filter, FolderBin, Library, RatingFilter};
use std::hint::black_box;
use std::path::{Path, PathBuf};

const RUNS: usize = 7;

#[global_allocator]
static ALLOC: negs_core_diff::Counting = negs_core_diff::Counting;

/// One run of `f`, counted: allocations and bytes.
fn counted(label: &str, mut f: impl FnMut()) {
    negs_core_diff::take_allocations();
    f();
    let (n, bytes) = negs_core_diff::take_allocations();
    println!("  {label} allocations: {n} ({:.1} MiB)", bytes as f64 / 1048576.0);
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

fn open(photos: &Path, bin: &Path) -> Library {
    Library::new(
        photos,
        Config { data: repo().join("data"), app: "bench".into(), bin: Box::new(FolderBin::new(bin.to_path_buf())), pairing: true, sidecars: true, id_seed: Some(7) },
    )
    .unwrap()
}

fn library(scratch: &Path) {
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
    println!("library_open_500 (list + parse + summaries + looks): {open_ms:.2} ms");
    counted("library_open_500", || {
        let library = open(&photos, &bin);
        for item in &library.shown(None) {
            black_box(library.summary(item));
            black_box(library.look(item));
        }
    });
    let mut library = open(&photos, &bin);
    let items = library.items();
    let write_ms = median_ms(RUNS, || {
        black_box(library.set_rating(&items, 4));
        black_box(library.undo());
    });
    println!("library_rate_and_undo_500 (1000 sidecar writes): {write_ms:.2} ms");
    counted("library_rate_and_undo_500", || {
        black_box(library.set_rating(&items, 4));
        black_box(library.undo());
    });
    let filter_ms = median_ms(RUNS, || {
        for stars in 1..=5u8 {
            library.set_filter(Filter { rating: RatingFilter::AtLeast(stars), ..Filter::default() });
            black_box(library.shown(None));
        }
        library.set_filter(Filter::default());
    });
    println!("library_filter_sort_5x500: {filter_ms:.2} ms");
    let _ = std::fs::remove_dir_all(scratch);
}

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

fn fit() {
    let (w, h) = (6000u32, 4000u32);
    let rgba: Vec<u8> = (0..w as usize * h as usize * 4).map(|i| (i * 31 % 251) as u8).collect();
    let (tw, th) = exact_fit::target(1500.0, 1000.0, 1.0);
    let ms = median_ms(RUNS, || {
        black_box(exact_fit::resample(&rgba, w, h, tw, th));
    });
    println!("exact_fit_6000x4000_to_{tw}x{th}: {ms:.2} ms");
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

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let which = args.get(1).map(String::as_str).unwrap_or("all");
    let scratch = std::env::temp_dir().join(format!("negs-core-bench-{}", std::process::id()));
    match which {
        "library" => library(&scratch),
        "marks" => {
            marks();
            core_marks();
        }
        "sheet" => sheet(),
        "fit" => fit(),
        "embedded" => embedded(Path::new(args.get(2).expect("raw folder"))),
        "all" => {
            library(&scratch);
            marks();
            core_marks();
            sheet();
        }
        other => panic!("unknown workload {other}"),
    }
    println!("peak_rss: {:.1} MiB", peak_rss_mib());
}
