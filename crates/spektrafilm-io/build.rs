fn main() {
    println!("cargo:rerun-if-changed=src/raw/apple_raw.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos")
        && std::env::var_os("CARGO_FEATURE_APPLE_RAW").is_some()
    {
        cc::Build::new()
            .file("src/raw/apple_raw.m")
            .flag("-fobjc-arc")
            .flag("-fmodules")
            .flag("-mmacosx-version-min=14.0")
            .compile("spektrafilm_apple_raw");
        for framework in ["Foundation", "CoreImage", "CoreGraphics", "ImageIO"] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
}
