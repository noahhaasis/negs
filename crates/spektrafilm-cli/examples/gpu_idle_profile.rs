//! Keep a decoded input and the GPU backend alive across three resident renders.
//! Each output is dropped before the next render. The final pause lets an
//! external sampler measure retained idle footprint with the backend alive.
use std::{path::Path, thread, time::Duration};

use spektrafilm_core::{params::RuntimeParams, pipeline::Pipeline, profile};
use spektrafilm_gpu::select_backend;
use spektrafilm_io::{Formats, load_image_with_wb};

fn main() {
    let path = std::env::args().nth(1).expect("pass an image path");
    let path = Path::new(&path);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data");
    let film = profile::load_profile_by_name(&dir, "kodak_gold_200").unwrap();
    let print = profile::load_profile_by_name(&dir, "kodak_portra_endura").unwrap();
    let mut params = RuntimeParams::default();
    params.io.input_color_space = "sRGB".into();
    params.io.input_cctf_decoding = false;
    let image = load_image_with_wb(path, Formats::TiffPngRaw, params.io.raw_white_balance).unwrap();
    let backend = select_backend();
    assert!(backend.is_gpu(), "GPU unavailable");
    let pipeline = Pipeline::new_with_spectral(film, print, params, &dir).unwrap();
    for i in 0..3 {
        let output = pipeline
            .process_resident_borrowed(&image, backend.as_ref())
            .unwrap();
        drop(output);
        eprintln!("render {i} complete; output dropped");
    }
    eprintln!("idle start; input, pipeline and backend retained");
    thread::sleep(Duration::from_secs(3));
    eprintln!("idle end");
}
