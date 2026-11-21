use std::{fs, path::Path, process::Command};

#[test]
fn invalid_spectral_setup_fails_without_writing_a_fallback_render() {
    let directory =
        std::env::temp_dir().join(format!("spektrafilm-cli-failures-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let input = directory.join("input.png");
    image::RgbImage::from_pixel(1, 1, image::Rgb([128, 128, 128]))
        .save(&input)
        .unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    for (i, params) in [
        r#"{"io":{"input_gamut_compress":{"boundary":"typo"}}}"#,
        r#"{"settings":{"rgb_to_raw_method":"missing-method"}}"#,
    ]
    .into_iter()
    .enumerate()
    {
        let config = directory.join(format!("params-{i}.json"));
        let output = directory.join(format!("output-{i}.png"));
        fs::write(&config, params).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_spektrafilm"))
            .env("SPEKTRAFILM_BACKEND", "cpu")
            .arg("process")
            .arg(&input)
            .arg("--output")
            .arg(&output)
            .args(["--film", "kodak_doublex", "--paper", "kodak_2302"])
            .arg("--data-dir")
            .arg(&data)
            .arg("--params")
            .arg(config)
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("building spectral pipeline"));
        assert!(!output.exists());
    }
    fs::remove_dir_all(directory).unwrap();
}

/// `--negative` prints as the app does: the print exposure metered for the
/// frame (inside the picture the film's edges bound), or with --auto-print
/// a minilab's print of it.
#[test]
fn a_negative_meters_its_print_exposure_as_the_app_does() {
    let directory = std::env::temp_dir().join(format!("spektrafilm-cli-negative-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let input = directory.join("negative.png");
    // Clear film (Portra 400's orange base, scanned) down the left, then a
    // scatter of the densities a negative holds.
    let encode = |v: f64| (255.0 * if v <= 0.0031308 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }).round() as u8;
    let clear = [0.706165, 0.236752, 0.143739];
    let palette = [[0.415618, 0.114352, 0.067519], [0.276594, 0.116903, 0.068603], [0.246983, 0.056582, 0.072025],
                   [0.168065, 0.057386, 0.072414], [0.338250, 0.022854, 0.072495], [0.218356, 0.027397, 0.033256]];
    let image = image::RgbImage::from_fn(240, 160, |x, y| {
        let rgb = if x < 30 { clear } else { palette[((x / 7 + y / 5) % palette.len() as u32) as usize] };
        image::Rgb(rgb.map(encode))
    });
    image.save(&input).unwrap();
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
    let run = |extra: &[&str]| {
        let result = Command::new(env!("CARGO_BIN_EXE_spektrafilm"))
            .env("SPEKTRAFILM_BACKEND", "cpu")
            .arg("process")
            .arg(&input)
            .arg("--output")
            .arg(directory.join("print.png"))
            .args(["--film", "kodak_portra_400", "--paper", "kodak_portra_endura", "--negative"])
            .args(extra)
            .arg("--data-dir")
            .arg(&data)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
        assert!(result.status.success(), "{stderr}");
        stderr
    };
    let plain = run(&[]);
    assert!(plain.contains("Film base: found along the left edge"), "{plain}");
    assert!(plain.contains("Print exposure metered: "), "{plain}");
    let auto = run(&["--auto-print"]);
    assert!(auto.contains("Auto print: ") && !auto.contains("Print exposure metered"), "{auto}");
    fs::remove_dir_all(directory).unwrap();
}
