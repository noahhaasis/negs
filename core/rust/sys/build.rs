//! Builds the Odin core with `core/build.py lib` into OUT_DIR and links it,
//! unless NEGS_CORE_LIB_DIR names a folder that already holds it. When the
//! target isn't the host's OS (`cargo check --target x86_64-pc-windows-msvc`
//! on a Mac) nothing is built: checking doesn't link, and Odin can't
//! cross-link for MSVC anyway.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let core = manifest.join("../..");
    println!("cargo:rerun-if-changed={}", core.join("src").display());
    println!("cargo:rerun-if-changed={}", core.join("include").display());
    println!("cargo:rerun-if-changed={}", core.join("build.py").display());
    println!("cargo:rerun-if-changed={}", core.join("ODIN_VERSION").display());
    println!("cargo:rerun-if-env-changed=NEGS_CORE_LIB_DIR");
    println!("cargo:rerun-if-env-changed=NEGS_ODIN");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap();
    let windows = target_os == "windows";
    if windows {
        // The Windows library is the core's object alone (core/build.py):
        // the CRT and kernel32 come with Rust's link; Odin's runtime also
        // calls BCryptGenRandom.
        println!("cargo:rustc-link-lib=bcrypt");
    }
    if let Ok(dir) = std::env::var("NEGS_CORE_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rustc-link-lib=static=negs_core");
        return;
    }
    let host_os = if cfg!(target_os = "macos") { "macos" } else if cfg!(windows) { "windows" } else { "linux" };
    if target_os != host_os {
        println!("cargo:warning=negs-core-sys: not building the Odin core for {target_os} on {host_os} (checking only)");
        return;
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let lib = out.join(if windows { "negs_core.lib" } else { "libnegs_core.a" });
    let python = std::env::var("PYTHON").unwrap_or_else(|_| if windows { "python".into() } else { "python3".into() });
    let mut cmd = Command::new(python);
    cmd.arg(core.join("build.py")).arg("lib").arg("--out").arg(&lib);
    if std::env::var("CARGO_FEATURE_TRACK").is_ok() {
        cmd.arg("--track");
    }
    let target_arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let odin_target = match (target_os.as_str(), target_arch.as_str()) {
        ("macos", "aarch64") => "darwin_arm64",
        ("macos", "x86_64") => "darwin_amd64",
        ("windows", _) => "windows_amd64",
        ("linux", "aarch64") => "linux_arm64",
        _ => "linux_amd64",
    };
    cmd.arg("--target").arg(odin_target);
    let status = cmd.status().expect("run core/build.py (python3 on PATH, or set PYTHON)");
    assert!(status.success(), "core/build.py lib failed");
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=negs_core");
}
