//! Derive neutral print filters (C, M, Y) for film/paper pairs, the way
//! `scripts/derive_neutral_print_filters.py` does with upstream's Python
//! solver: grain and glare off, auto exposure off, the database lookup off,
//! the TH-KG3 enlarger lamp, started from upstream's fallback C0 M50 Y50
//! with C fixed at 0. Prints one JSON object per pair.
//!
//! Usage: solve_neutral_filters <data_dir> <paper> <film> [<film> ...]

use std::path::PathBuf;

use spektrafilm_core::params::RuntimeParams;
use spektrafilm_core::print_balance::{solve_neutral_filter_shifts, MIDGRAY};
use spektrafilm_core::profile;

const START: [f64; 3] = [0.0, 50.0, 50.0];

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() < 3 {
        eprintln!("usage: solve_neutral_filters <data_dir> <paper> <film> [<film> ...]");
        std::process::exit(2);
    }
    let data_dir = PathBuf::from(&args[0]);
    let paper = &args[1];
    for film_name in &args[2..] {
        let mut params = RuntimeParams::default();
        params.film_render.grain.active = false;
        params.film_render.glare.active = false;
        params.print_render.glare.active = false;
        params.camera.auto_exposure = false;
        params.settings.neutral_print_filters_from_database = false;
        params.enlarger.illuminant = "TH-KG3".into();
        params.enlarger.c_filter_neutral = START[0] as f32;
        params.enlarger.m_filter_neutral = START[1] as f32;
        params.enlarger.y_filter_neutral = START[2] as f32;
        params.enlarger.m_filter_shift = 0.0;
        params.enlarger.y_filter_shift = 0.0;
        let film = profile::load_profile_by_name(&data_dir, film_name).expect("film profile");
        let print = profile::load_profile_by_name(&data_dir, paper).expect("paper profile");
        match solve_neutral_filter_shifts(film, print, &params, &data_dir, MIDGRAY) {
            Ok((m, y)) => println!(
                "{{\"paper\":\"{paper}\",\"film\":\"{film_name}\",\"cmy\":[{},{:.6},{:.6}]}}",
                START[0],
                START[1] + m,
                START[2] + y
            ),
            Err(e) => println!("{{\"paper\":\"{paper}\",\"film\":\"{film_name}\",\"error\":{e:?}}}"),
        }
    }
}
