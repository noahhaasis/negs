//! Presets: files in `%APPDATA%\Negs\Presets` holding Python spektrafilm's
//! saved GUI state, named after the file (docs/presets-design.md). The engine
//! reads and writes the format; this only handles the files, as
//! `Presets.swift` does on the Mac.
//!
//! Writes never replace a file, and renames and deletes act only on the file
//! as it was listed. The user's order is `.order.json` in the folder,
//! `{"order": [file names]}`; presets it doesn't name, such as files copied in
//! by hand, follow in name order, and added presets go to the end.
//!
//! On the shared core: `negs_core_rules::presets` (core/src/settings/presets)
//! does all of it, including the Win32 calls; the Rust original is frozen in
//! core/diff/oracle/presets.rs.

#[allow(unused_imports)]
pub use negs_core_rules::presets::{Library, MAX_FILE_SIZE, PresetFile, Status, name, read_file, text};

#[cfg(test)]
use std::collections::HashSet;
#[cfg(test)]
use std::path::PathBuf;

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`, as the original made
/// it: the tests play another program that moves a file in.
#[cfg(test)]
fn move_no_replace(from: &std::path::Path, to: &std::path::Path) -> windows::core::Result<()> {
    use crate::platform::{pcwstr, wide};
    use windows::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    let (f, t) = (wide(from), wide(to));
    unsafe { MoveFileExW(pcwstr(&f), pcwstr(&t), MOVEFILE_WRITE_THROUGH) }
}

#[cfg(test)]
fn exists(error: &windows::core::Error) -> bool {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
    error.code() == ERROR_ALREADY_EXISTS.to_hresult() || error.code() == ERROR_FILE_EXISTS.to_hresult()
}

#[cfg(test)]
fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{:x}-{:x}-{:x}", std::process::id(), nanos, COUNTER.fetch_add(1, Ordering::Relaxed))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("negs-presets-{}", unique_id())))
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(presets: &[PresetFile]) -> Vec<String> {
        presets.iter().map(PresetFile::name).collect()
    }

    #[test]
    fn names_every_platform_accepts() {
        for n in ["Portra 400 warm", "Kodachrome – 1970s", "Trix (push 2)", "Ünïcødé", "v1.2", "COM0", "LPT10", "Console", "NULL"] {
            assert_eq!(name::problem(n), None, "{n}");
        }
        let long = "x".repeat(121);
        for n in [
            "", "   ", "a/b", "a\\b", "a:b", "what?", "a*", "\"quoted\"", "<x>", "a|b", ".hidden", "trailing.", "trailing ", " leading",
            "tab\there", "CON", "con", "Lpt1", "NUL.json", "COM9.backup", "aux.tar.gz", "prn .txt", &long,
        ] {
            assert!(name::problem(n).is_some(), "{n:?}");
        }
        assert_eq!(name::problem(&"x".repeat(120)), None);
    }

    #[test]
    fn names_compare_without_case_or_unicode_form() {
        assert_eq!(name::key("Ektachrome caf\u{E9}"), name::key("EKTACHROME CAFE\u{301}"));
        let taken: HashSet<String> = [name::key("Portra"), name::key("portra 2")].into();
        assert_eq!(name::unique("PORTRA", &taken), "PORTRA 3");
        assert_eq!(name::unique("Gold", &taken), "Gold");
    }

    #[test]
    fn file_names_become_preset_names() {
        assert_eq!(name::from_file_name("05 P400 natural"), "05 P400 natural");
        assert_eq!(name::from_file_name("a:b|c"), "a b c");
        assert_eq!(name::from_file_name("..."), "Preset");
        assert_eq!(name::from_file_name("CON"), "Preset");
        assert_eq!(name::from_file_name("lpt3.backup"), "Preset");
    }

    #[test]
    fn text_skips_a_byte_order_mark_and_refuses_binary() {
        assert_eq!(text(b"\xEF\xBB\xBF{}").unwrap(), "{}");
        assert!(text(b"\xFF\xFE{").is_err());
    }

    #[test]
    fn adding_never_replaces_and_renaming_checks_names() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let first = library.add(b"1", "Portra").unwrap();
        let second = library.add(b"2", "portra").unwrap();
        assert_eq!(second.name(), "portra 2");
        assert_eq!(std::fs::read(&first.path).unwrap(), b"1");
        // Hidden and temporary files and other files aren't presets.
        std::fs::write(folder.0.join(".partial.tmp"), b"").unwrap();
        std::fs::write(folder.0.join("notes.txt"), b"").unwrap();
        let hidden = folder.0.join("Hidden.json");
        std::fs::write(&hidden, b"{}").unwrap();
        crate::platform::hide_file(&hidden);
        assert_eq!(names(&library.list().unwrap()), ["Portra", "portra 2"]);

        let clash = library.rename(&second, "PORTRA").unwrap_err().to_string();
        assert!(clash.contains("already exists"), "{clash}");
        assert!(library.rename(&second, "a/b").is_err());
        assert!(library.rename(&second, "NUL").is_err());
        // Another case of its own name is the same file.
        let recased = library.rename(&first, "PORTRA").unwrap();
        assert_eq!(names(&library.list().unwrap()), ["PORTRA", "portra 2"]);
        let renamed = library.rename(&recased, "Portra warm").unwrap();
        assert_eq!(library.read(&renamed).unwrap(), b"1");
        // No temporary file is left behind.
        let leftovers: Vec<_> = std::fs::read_dir(&folder.0).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert_eq!(leftovers.len(), 1, "only the test's own .partial.tmp");
    }

    #[test]
    fn a_file_created_meanwhile_is_never_replaced() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let theirs = library.add(b"theirs", "Gold").unwrap();
        // Unlisted when the name was chosen: the move refuses it.
        let target = folder.0.join("Gold 2.json");
        std::fs::write(&target, b"copied in").unwrap();
        let temporary = folder.0.join(".x.tmp");
        std::fs::write(&temporary, b"mine").unwrap();
        assert!(move_no_replace(&temporary, &target).is_err_and(|e| exists(&e)));
        assert_eq!(std::fs::read(&target).unwrap(), b"copied in");
        let mine = library.add(b"mine", "gold").unwrap();
        assert_eq!(mine.name(), "gold 3");
        assert_eq!(std::fs::read(&theirs.path).unwrap(), b"theirs");
    }

    #[test]
    fn the_users_order_is_remembered_and_added_presets_go_last() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        for n in ["Zeta", "Alpha", "Mid"] {
            library.add(n.as_bytes(), n).unwrap();
        }
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Zeta", "Alpha", "Mid"]);
        library.set_order(&[listed[2].clone(), listed[0].clone(), listed[1].clone()]).unwrap();
        // Copied in by hand: after the ordered ones, in name order.
        std::fs::write(folder.0.join("Copied B.json"), b"x").unwrap();
        std::fs::write(folder.0.join("Copied A.json"), b"x").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Mid", "Zeta", "Alpha", "Copied A", "Copied B"]);
        // Renaming keeps the place; deleting forgets it; adding goes last.
        library.rename(&listed[1], "Omega").unwrap();
        library.delete(&listed[0]).unwrap();
        library.add(b"n", "Newest").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Omega", "Alpha", "Copied A", "Copied B", "Newest"]);
        // A fresh library reads the same order from the folder, in the Mac's format.
        assert_eq!(names(&Library::new(folder.0.clone()).list().unwrap()), names(&listed));
        let order: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.0.join(".order.json")).unwrap()).unwrap();
        assert_eq!(order["order"][0], "Omega.json");
        // And can write it again although it's hidden.
        library.set_order(&listed).unwrap();
    }

    #[test]
    fn files_changed_on_disk_are_left_alone() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"original", "Gold").unwrap();
        // Edited by hand after it was listed.
        std::fs::write(&preset.path, b"edited elsewhere").unwrap();
        let error = library.delete(&preset).unwrap_err().to_string();
        assert!(error.contains("changed on disk"), "{error}");
        assert!(library.rename(&preset, "Other").is_err());
        assert!(library.read(&preset).is_err());
        assert!(preset.path.exists());
        let current = library.list().unwrap().remove(0);
        library.delete(&current).unwrap();
        assert!(library.list().unwrap().is_empty());
    }

    #[test]
    fn a_file_open_elsewhere_is_reported_not_skipped() {
        use std::os::windows::fs::OpenOptionsExt;
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"x", "Busy").unwrap();
        // Shared for reading only, as a preview handler may hold it.
        let held = std::fs::OpenOptions::new().read(true).share_mode(1).open(&preset.path).unwrap();
        let error = library.delete(&preset).unwrap_err().to_string();
        assert!(error.contains("open in another program"), "{error}");
        let error = library.rename(&preset, "Free").unwrap_err().to_string();
        assert!(error.contains("open in another program"), "{error}");
        drop(held);
        assert_eq!(names(&library.list().unwrap()), ["Busy"]);
        library.delete(&preset).unwrap();
    }
}
