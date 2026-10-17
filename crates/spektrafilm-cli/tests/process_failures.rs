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
