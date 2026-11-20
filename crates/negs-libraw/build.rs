//! Builds the vendored LibRaw (vendor/libraw, without OpenMP, zlib, JPEG or
//! LCMS) and the shim with the `cc` crate: no system LibRaw, so every
//! platform decodes with the same version.

use std::path::{Path, PathBuf};

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "cpp") {
            out.push(path);
        }
    }
}

fn main() {
    let root = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../vendor/libraw");
    let mut files = Vec::new();
    sources(&root.join("src"), &mut files);
    // Empty without USE_RAWSPEED, USE_X3FTOOLS and USE_LCMS: skip them.
    files.retain(|f| {
        let name = f.file_name().unwrap().to_string_lossy();
        !(name == "rawspeed_glue.cpp" || name == "apply_profile.cpp" || name.starts_with("x3f_"))
    });
    files.sort();
    let mut build = cc::Build::new();
    build
        .cpp(true)
        .std("c++14")
        .include(&root)
        .define("LIBRAW_NODLL", None)
        .files(&files)
        .file("src/shim.cpp")
        .warnings(false);
    let target = std::env::var("TARGET").unwrap();
    if target.contains("apple") && std::env::var_os("CXX").is_none() {
        // The Command Line Tools' clang++ doesn't find libc++'s headers in an
        // Xcode SDK (SDKROOT); the active toolchain's does.
        if let Ok(out) = std::process::Command::new("xcrun").args(["--find", "clang++"]).output()
            && out.status.success()
        {
            build.compiler(String::from_utf8_lossy(&out.stdout).trim());
        }
    }
    if target.contains("msvc") {
        build.flag("/EHsc").flag("/bigobj").define("_CRT_SECURE_NO_WARNINGS", None).define("WIN32", None);
    } else {
        build.flag_if_supported("-w");
    }
    build.compile("negs_libraw");
    if target.contains("windows") {
        // LibRaw's datastream uses winsock's byte order helpers.
        println!("cargo:rustc-link-lib=ws2_32");
    }
    println!("cargo:rerun-if-changed=src/shim.cpp");
    println!("cargo:rerun-if-changed=../../vendor/libraw");
}
