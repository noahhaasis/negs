//! Embeds the manifest (common controls 6, per-monitor DPI v2), the icon and
//! the version resource. Without the Windows SDK's rc.exe the manifest is
//! embedded by the linker and the icon is left out.

use std::path::{Path, PathBuf};

fn rc_exe() -> Option<PathBuf> {
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(kits).ok()?.flatten().map(|e| e.path()).filter(|p| p.join("x64").join("rc.exe").exists()).collect();
    versions.sort();
    versions.pop().map(|v| v.join("x64").join("rc.exe"))
}

fn main() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let manifest = dir.join("negs.manifest");
    let png = dir.join("../assets/negs-icon.png");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rerun-if-changed={}", png.display());
    println!("cargo:rerun-if-changed=build.rs");
    for var in ["NEGS_APPCAST_URL", "NEGS_ED_PUBLIC_KEY"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let Some(rc) = rc_exe() else {
        println!("cargo:rustc-link-arg-bins=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-bins=/MANIFESTINPUT:{}", manifest.display());
        return;
    };
    // A multi-size icon from the app's PNG.
    let icon = out.join("negs.ico");
    let source = image::open(&png).expect("assets/negs-icon.png");
    let mut frames = vec![];
    for size in [16, 20, 24, 32, 40, 48, 64, 256] {
        let resized = source.resize_exact(size, size, image::imageops::FilterType::Lanczos3).to_rgba8();
        frames.push(image::codecs::ico::IcoFrame::as_png(resized.as_raw(), size, size, image::ExtendedColorType::Rgba8).unwrap());
    }
    image::codecs::ico::IcoEncoder::new(std::fs::File::create(&icon).unwrap()).encode_images(&frames).unwrap();
    let version = env!("CARGO_PKG_VERSION");
    let numbers = version.replace('.', ",") + ",0";
    let script = out.join("negs.rc");
    std::fs::write(
        &script,
        format!(
            r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {numbers}
PRODUCTVERSION {numbers}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Negs"
      VALUE "FileDescription", "Negs"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "Negs"
      VALUE "LegalCopyright", "GPL-3.0"
      VALUE "OriginalFilename", "Negs.exe"
      VALUE "ProductName", "Negs"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
            icon = icon.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"),
            manifest = manifest.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"),
        ),
    )
    .unwrap();
    let res = out.join("negs.res");
    let status = std::process::Command::new(rc).arg("/nologo").arg("/fo").arg(&res).arg(&script).status().expect("rc.exe");
    assert!(status.success(), "rc.exe failed");
    println!("cargo:rustc-link-arg-bins={}", res.display());
    println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
}
