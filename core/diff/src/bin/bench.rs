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

// MARK: Phase 4a: the edit lists per sync

/// windows/src/edits.rs's Facts impl over the frozen InspectorFacts, as
/// the Windows app asks the core (host mode).
struct HostFacts<'a, 'b>(&'b negs_state_oracle::inspector_view::facts::InspectorFacts<'a>);

impl negs_core_rules::edits::Facts for HostFacts<'_, '_> {
    fn value(&self, path: &str) -> &serde_json::Value {
        self.0.value(path)
    }
    fn is_modified(&self, path: &str) -> bool {
        self.0.is_modified(path)
    }
    fn film_is_bw(&self) -> bool {
        self.0.film_is_bw()
    }
    fn film_positive(&self) -> bool {
        self.0.stocks().iter().find(|s| !s.paper && s.id == self.0.state().film).is_some_and(|s| s.positive)
    }
    fn border_enabled(&self) -> bool {
        self.0.state().border.enabled
    }
    fn matches_brightness(&self) -> bool {
        self.0.matches_brightness()
    }
    fn exposure_offset(&self) -> f64 {
        self.0.exposure_offset()
    }
    fn exposure_offset_modified(&self) -> bool {
        self.0.exposure_offset_modified()
    }
    fn chosen_color_filter_name(&self) -> Option<&str> {
        self.0.chosen_color_filter().map(|f| f.name.as_str())
    }
    fn prefs(&self) -> negs_core_rules::edits::Prefs<'_> {
        let p = &self.0.prefs;
        negs_core_rules::edits::Prefs {
            lab_all_controls: p.lab_all_controls,
            more_adjustments: p.more_adjustments,
            match_new_photos: p.match_new_photos,
            color_space: p.color_space,
        }
    }
    fn is_raw(&self) -> bool {
        self.0.is_raw
    }
    fn lens_supported(&self) -> bool {
        self.0.lens_supported
    }
}

fn s(text: &str) -> negs_core_sys::state::negs_str_t {
    negs_core_sys::state::negs_str_t { data: text.as_ptr(), len: text.len() }
}

/// A sync's edit lists (the counts beside the three tabs and the open
/// tab's list, Lab) on a Portra 400 edit with ten controls changed, a
/// solved brightness match and the real catalog: the frozen Rust, the core
/// asked by the Rust facts (Windows today), and the core on its own facts
/// (4d), with what a facts update costs.
fn edits() {
    use negs_core_sys::state as sys;
    use negs_state_oracle::edits as oe;
    use negs_state_oracle::inspector_view::facts::{Brightness, ColorFilters, InspectorFacts, Params, Prefs};
    use negs_state_oracle::state::{self as os, Adjustment, BrightnessMatch, ColorFilter, EditState, Stock};
    use negs_core_rules::edits as ce;
    use serde_json::{Value, json};

    const SYNCS: usize = 1000;
    let catalog: Value = serde_json::from_str(include_str!("../../tests/state_common/catalog.json")).unwrap();
    let engine: Value = serde_json::from_str(include_str!("../../tests/state_common/engine_defaults.json")).unwrap();
    let defaults = engine["defaults"].clone();
    let stocks: Vec<Stock> = catalog["profiles"].as_array().unwrap().iter().map(Stock::new).collect();
    let filters: Vec<ColorFilter> = catalog["color_filters"].as_array().unwrap().iter().map(ColorFilter::new).collect();
    let mut state = EditState { params: defaults.clone(), film: "kodak_portra_400".into(), paper: "kodak_portra_endura".into(), ..Default::default() };
    let mut changed = 0;
    for g in os::ADJUSTMENT_GROUPS {
        if let Some(Adjustment::Slider { path, low, high, .. }) = g.controls.first()
            && changed < 10
        {
            negs_state_oracle::json::set(&mut state.params, path, json!(low + (high - low) * 0.3));
            changed += 1;
        }
    }
    state.brightness_match = Some(BrightnessMatch { ev: Some(0.7), inputs: None, highlight_limited: false });
    let inputs = os::match_inputs(&state);
    state.brightness_match.as_mut().unwrap().inputs = Some(inputs);
    let params = Params { state: &state, defaults: &defaults, stocks: &stocks };
    let f = InspectorFacts {
        params,
        color_filters: ColorFilters { params, filters: &filters, catalog: &catalog },
        brightness: Brightness { params, is_raw: true, failure: None, generation: 1, matching: false },
        prefs: Prefs { more_adjustments: "Grain,section:Film", color_space: "sRGB", filter_shift_link: "together", ..Default::default() },
        is_raw: true,
        lens_supported: true,
        has_source: true,
        photo_size: negs_state_oracle::crop::Size::new(6000.0, 4000.0),
        source_size: (6000, 4000),
        preview_scale: 0.25,
        image_shown: true,
        size_error: None,
        custom_film_format: false,
        balancing_print: false,
        can_balance_print: true,
        can_export: true,
        cropping: false,
        negative: None,
    };
    let counts = |c: [usize; 3], n: usize| println!("  counts {c:?}, Lab list {n} edits");

    let rust = median_ms(RUNS, || {
        for _ in 0..SYNCS {
            for tab in oe::Tab::ALL {
                black_box(oe::count(tab, &f));
            }
            black_box(oe::edits(oe::Tab::Lab, &f, '.'));
        }
    });
    println!("edits_rust_{SYNCS}_syncs (count x3 + edits(Lab)): {rust:.2} ms");
    counts(oe::Tab::ALL.map(|t| oe::count(t, &f)), oe::edits(oe::Tab::Lab, &f, '.').len());

    let host = HostFacts(&f);
    let core_host = median_ms(RUNS, || {
        for _ in 0..SYNCS {
            for tab in ce::Tab::ALL {
                black_box(ce::count(tab, &host));
            }
            black_box(ce::edits(ce::Tab::Lab, &host, '.'));
        }
    });
    println!("core_edits_host_{SYNCS}_syncs (Windows: the core asking the Rust facts): {core_host:.2} ms");
    counts(ce::Tab::ALL.map(|t| ce::count(t, &host)), ce::edits(ce::Tab::Lab, &host, '.').len());
    counted("core_edits_host one sync", || {
        for tab in ce::Tab::ALL {
            black_box(ce::count(tab, &host));
        }
        black_box(ce::edits(ce::Tab::Lab, &host, '.'));
    });

    // Native (4d): the core's catalog, state and facts.
    let defaults_text = serde_json::to_vec(&defaults).unwrap();
    let rules: Vec<negs_core_rules::state::Stock> = catalog["profiles"].as_array().unwrap().iter().map(negs_core_rules::state::Stock::new).collect();
    let grains: Vec<Vec<u8>> = rules.iter().map(|s| serde_json::to_vec(&s.grain).unwrap()).collect();
    let c_stocks: Vec<sys::negs_stock_t> = rules.iter().zip(&grains).map(|(s, g)| negs_core_rules::state::sys_stock_with_grain(s, g)).collect();
    let c_filters: Vec<sys::negs_color_filter_t> =
        filters.iter().map(|f| sys::negs_color_filter_t { key: s(&f.key), name: s(&f.name), glass: s(&f.glass), colour: f.colour }).collect();
    let catalog_text = serde_json::to_vec(&catalog).unwrap();
    let state_text = serde_json::to_vec(&state).unwrap();
    let mut input: sys::negs_facts_input_t = unsafe { std::mem::zeroed() };
    input.prefs.more_adjustments = s("Grain,section:Film");
    input.prefs.color_space = s("sRGB");
    input.prefs.filter_shift_link = s("together");
    input.photo_size = [6000.0, 4000.0];
    input.preview_scale = 0.25;
    input.generation = 1;
    input.source_width = 6000;
    input.source_height = 4000;
    (input.is_raw, input.lens_supported, input.has_source, input.image_shown, input.can_balance_print, input.can_export) = (1, 1, 1, 1, 1, 1);
    unsafe {
        let cat = sys::negs_catalog_new(
            defaults_text.as_ptr(),
            defaults_text.len(),
            c_stocks.as_ptr(),
            c_stocks.len(),
            c_filters.as_ptr(),
            c_filters.len(),
            catalog_text.as_ptr(),
            catalog_text.len(),
        );
        let h = sys::negs_facts_new(cat);
        let st = sys::negs_state_parse(state_text.as_ptr(), state_text.len());
        let native = median_ms(RUNS, || {
            for _ in 0..SYNCS {
                sys::negs_facts_update(h, st, &input, sys::NEGS_FACTS_BORROW);
                for tab in 0..3 {
                    black_box(sys::negs_edits_count(h, tab));
                }
                let mut n = 0;
                black_box(sys::negs_edits_list(h, 1, '.' as u32, &mut n));
            }
        });
        println!("core_edits_native_{SYNCS}_syncs (4d: update + count x3 + edits(Lab)): {native:.2} ms");
        sys::negs_facts_update(h, st, &input, sys::NEGS_FACTS_BORROW);
        let mut n = 0;
        sys::negs_edits_list(h, 1, '.' as u32, &mut n);
        counts([0, 1, 2].map(|t| sys::negs_edits_count(h, t)), n);
        let borrow = median_ms(RUNS, || {
            for _ in 0..SYNCS {
                sys::negs_facts_update(h, st, &input, sys::NEGS_FACTS_BORROW);
            }
        });
        let copy = median_ms(RUNS, || {
            for _ in 0..SYNCS {
                sys::negs_facts_update(h, st, &input, 0);
            }
        });
        println!("core_facts_update_{SYNCS} (borrowed strings): {borrow:.3} ms, copied: {copy:.3} ms");
        let parse = median_ms(RUNS, || {
            for _ in 0..SYNCS {
                let p = sys::negs_state_parse(state_text.as_ptr(), state_text.len());
                sys::negs_state_free(black_box(p));
            }
        });
        println!("  for scale, core_state_parse_{SYNCS} ({} bytes): {parse:.2} ms", state_text.len());
        counted("core_facts_update one", || sys::negs_facts_update(h, st, &input, 0));
        sys::negs_facts_free(h);
        sys::negs_state_free(st);
        sys::negs_catalog_free(cat);
    }
    ce::release_thread_facts();
    println!("  ({SYNCS} syncs: each ms above is µs per sync)");
}

// MARK: Phase 4b: presets and preferences

/// A Presets folder of 200 GUI-default presets in a user's order, then
/// listing it and loading and saving preferences (NEGS_PROFILE_DIR set to
/// a scratch folder), the frozen Rust and the core side by side.
fn settings(scratch: &Path) {
    use negs_state_oracle::{model::LibraryPreferences, presets as oracle_presets, session as oracle_session};
    let folder = scratch.join("Presets");
    let _ = std::fs::remove_dir_all(scratch);
    let gui = std::fs::read(repo().join("data/presets/spektrafilm-gui-default-28bf883.json")).unwrap();
    let oracle_library = oracle_presets::Library { folder: folder.clone() };
    for i in 0..200 {
        oracle_library.add(&gui, &format!("Preset {i}")).unwrap();
    }
    let core_library = negs_core_rules::presets::Library { folder: folder.clone() };
    let rust = median_ms(RUNS, || {
        black_box(oracle_library.list().unwrap());
    });
    let core = median_ms(RUNS, || {
        black_box(core_library.list().unwrap());
    });
    println!("presets_list_200 rust: {rust:.3} ms");
    println!("presets_list_200 core: {core:.3} ms");
    counted("presets_list_200 core", || {
        black_box(core_library.list().unwrap());
    });

    unsafe { std::env::set_var("NEGS_PROFILE_DIR", scratch) };
    let mut prefs = oracle_session::Preferences::default();
    for i in 0..10 {
        prefs.note_recent(&scratch.join(format!("photo {i}.NEF")));
    }
    prefs.library.library_filter = Some(serde_json::json!({"rating": 3, "flag": "picked", "films": ["kodak_gold_200", "kodak_portra_400"]}));
    prefs.save();
    let rust_load = median_ms(RUNS * 3, || {
        black_box(oracle_session::Preferences::load());
    });
    let core_load = median_ms(RUNS * 3, || {
        black_box(negs_core_rules::session::Preferences::<LibraryPreferences>::load());
    });
    let core_prefs = negs_core_rules::session::Preferences::<LibraryPreferences>::load();
    let rust_save = median_ms(RUNS * 3, || prefs.save());
    let core_save = median_ms(RUNS * 3, || core_prefs.save());
    println!("preferences_load rust: {rust_load:.3} ms");
    println!("preferences_load core: {core_load:.3} ms");
    println!("preferences_save rust: {rust_save:.3} ms");
    println!("preferences_save core: {core_save:.3} ms");
    unsafe { std::env::remove_var("NEGS_PROFILE_DIR") };
    let _ = std::fs::remove_dir_all(scratch);
}

// MARK: Phase 4c: caches

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
        // Phase 4a: count x3 + edits(tab) per sync, and a facts update.
        "edits" => edits(),
        "settings" => settings(&scratch),
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
            edits();
        }
        other => panic!("unknown workload {other}"),
    }
    println!("peak_rss: {:.1} MiB", peak_rss_mib());
}
