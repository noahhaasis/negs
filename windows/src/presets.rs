//! Presets: files in `%APPDATA%\Negs\Presets` holding Python spektrafilm's
//! saved GUI state, named after the file (docs/presets-design.md). The engine
//! reads and writes the format; this only handles the files, as
//! `Presets.swift` does on the Mac.
//!
//! Writes never replace a file, and renames and deletes act only on the file
//! as it was listed. The user's order is `.order.json` in the folder,
//! `{"order": [file names]}`; presets it doesn't name, such as files copied in
//! by hand, follow in name order, and added presets go to the end.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Result, anyhow, bail};
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_SHARING_VIOLATION, ERROR_LOCK_VIOLATION};
use windows::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, MOVEFILE_WRITE_THROUGH, MoveFileExW};

use crate::platform::{pcwstr, wide};

/// A preset file as listed: its path as found on disk, size and
/// modification time, which renames and deletes check first.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PresetFile {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}

impl PresetFile {
    pub fn name(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

/// What the engine made of a preset file.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Asked, not answered yet.
    Checking,
    Valid { film: String, paper: String, scan: bool },
    Invalid(String),
}

/// One naming policy on every platform: names Windows, macOS and Linux all
/// accept as file names, compared without regard to case or Unicode form.
pub mod name {
    use std::collections::HashSet;

    pub const MAX_LENGTH: usize = 120;
    const FORBIDDEN: &str = "/\\:*?\"<>|";

    /// Why `name` can't name a preset, or None.
    pub fn problem(name: &str) -> Option<String> {
        if name.trim().is_empty() {
            return Some("Enter a name.".into());
        }
        if name.chars().count() > MAX_LENGTH {
            return Some(format!("Use at most {MAX_LENGTH} characters."));
        }
        if name.chars().any(|c| c.is_control() || FORBIDDEN.contains(c)) {
            return Some("A name can’t contain / \\ : * ? \" < > or |.".into());
        }
        if name.starts_with('.') {
            return Some("A name can’t start with a period.".into());
        }
        if name.ends_with('.') || name.ends_with(' ') || name.starts_with(' ') {
            return Some("A name can’t start with a space or end with a space or period.".into());
        }
        // Device names, also with an extension: "NUL.json" is NUL.
        let stem = name.split('.').next().unwrap_or("").trim().to_uppercase();
        let numbered = |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| n.len() == 1 && ('1'..='9').contains(&n.chars().next().unwrap()));
        if ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str()) || numbered("COM") || numbered("LPT") {
            return Some(format!("“{name}” is reserved on Windows."));
        }
        None
    }

    /// Names that are the same preset: NFC-normalized and case-folded. NTFS
    /// compares names without case but doesn't normalize them.
    pub fn key(name: &str) -> String {
        nfc(name).to_lowercase()
    }

    fn nfc(text: &str) -> String {
        use windows::Win32::Globalization::{NormalizationC, NormalizeString};
        let source: Vec<u16> = text.encode_utf16().collect();
        if source.is_empty() {
            return String::new();
        }
        // The estimate can be short; the call says how much it needs.
        let mut length = unsafe { NormalizeString(NormalizationC, &source, None) };
        for _ in 0..3 {
            if length <= 0 {
                break;
            }
            let mut buffer = vec![0u16; length as usize];
            let written = unsafe { NormalizeString(NormalizationC, &source, Some(&mut buffer)) };
            if written > 0 {
                return String::from_utf16_lossy(&buffer[..written as usize]);
            }
            length = -written;
        }
        text.to_owned()
    }

    /// `name`, or "name 2", "name 3"… the first whose key isn't `taken`.
    pub fn unique(name: &str, taken: &HashSet<String>) -> String {
        if !taken.contains(&key(name)) {
            return name.to_owned();
        }
        let mut n = 2;
        while taken.contains(&key(&format!("{name} {n}"))) {
            n += 1;
        }
        format!("{name} {n}")
    }

    /// A file's name as a preset name: invalid characters become spaces.
    pub fn from_file_name(file_name: &str) -> String {
        let trim = |s: &str| s.trim_matches(|c| c == ' ' || c == '.').to_owned();
        let replaced: String = file_name.chars().map(|c| if FORBIDDEN.contains(c) || c == '\n' || c == '\r' { ' ' } else { c }).collect();
        let mut name = trim(&replaced).replace("  ", " ");
        if name.chars().count() > MAX_LENGTH {
            name = trim(&name.chars().take(MAX_LENGTH).collect::<String>());
        }
        if problem(&name).is_none() { name } else { "Preset".into() }
    }
}

/// Larger files aren't GUI states.
pub const MAX_FILE_SIZE: u64 = 1_048_576;

/// A file's text for the engine. Notepad may have added a byte order mark.
pub fn text(data: &[u8]) -> Result<String> {
    let data = data.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(data);
    String::from_utf8(data.to_vec()).map_err(|_| anyhow!("The file isn’t text."))
}

/// A file's bytes, within the size limit.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path).map_err(|e| file_error(e, path, "Couldn’t read"))?;
    if metadata.is_dir() {
        bail!("It’s a folder.");
    }
    if metadata.len() > MAX_FILE_SIZE {
        bail!("The file is too large to be a preset.");
    }
    std::fs::read(path).map_err(|e| file_error(e, path, "Couldn’t read"))
}

/// The Presets folder.
pub struct Library {
    pub folder: PathBuf,
}

impl Default for Library {
    fn default() -> Self {
        Self { folder: crate::session::settings_dir().join("Presets") }
    }
}

impl Library {
    #[cfg(test)]
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    fn order_file(&self) -> PathBuf {
        self.folder.join(".order.json")
    }

    /// The presets in the user's order. Hidden files (a leading period or the
    /// hidden attribute) and files other than `.json` aren't presets.
    pub fn list(&self) -> Result<Vec<PresetFile>> {
        let entries = match std::fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(anyhow!("Couldn’t list the presets: {error}.")),
        };
        let mut files: Vec<(String, PresetFile)> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let file_name = path.file_name()?.to_str()?;
                let json = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("json"));
                if file_name.starts_with('.') || !json {
                    return None;
                }
                let metadata = std::fs::metadata(&path).ok()?;
                if metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN.0 != 0 {
                    return None;
                }
                let file = file_at(&path).ok()?;
                Some((name::key(&file.name()), file))
            })
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut position = HashMap::new();
        for (index, name) in self.read_order().iter().enumerate() {
            position.entry(name::key(name)).or_insert(index);
        }
        // Stable: unordered files keep their name order after the ordered ones.
        files.sort_by_key(|(key, _)| position.get(key).copied().unwrap_or(usize::MAX));
        Ok(files.into_iter().map(|(_, file)| file).collect())
    }

    /// Remembers this order of the presets.
    pub fn set_order(&self, presets: &[PresetFile]) -> Result<()> {
        self.write_order(&presets.iter().map(PresetFile::name).collect::<Vec<_>>())
    }

    /// Writes `data` as a new preset named `name`, or "name 2"… if taken.
    pub fn add(&self, data: &[u8], name: &str) -> Result<PresetFile> {
        if let Some(problem) = name::problem(name) {
            bail!(problem);
        }
        std::fs::create_dir_all(&self.folder).map_err(|e| anyhow!("Couldn’t create the Presets folder: {e}."))?;
        let temporary = self.folder.join(format!(".{}.tmp", unique_id()));
        let written = (|| {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
            file.write_all(data)?;
            file.sync_all()
        })();
        if let Err(error) = written {
            let _ = std::fs::remove_file(&temporary);
            bail!("Couldn’t save the preset: {error}.");
        }
        let mut taken: HashSet<String> = self.list()?.iter().map(|p| name::key(&p.name())).collect();
        for _ in 0..1000 {
            let candidate = name::unique(name, &taken);
            let path = self.folder.join(format!("{candidate}.json"));
            // Never replaces: a file created meanwhile fails it.
            match move_no_replace(&temporary, &path) {
                Ok(()) => {
                    let mut order: Vec<String> = self.list()?.iter().map(PresetFile::name).filter(|n| name::key(n) != name::key(&candidate)).collect();
                    order.push(candidate);
                    self.write_order(&order)?;
                    return file_at(&path);
                }
                Err(error) if exists(&error) => {
                    taken.insert(name::key(&candidate));
                }
                Err(error) => {
                    let _ = std::fs::remove_file(&temporary);
                    bail!("Couldn’t save the preset: {}.", error.message());
                }
            }
        }
        let _ = std::fs::remove_file(&temporary);
        bail!("Couldn’t find a free name for the preset.")
    }

    pub fn rename(&self, preset: &PresetFile, name: &str) -> Result<PresetFile> {
        if let Some(problem) = name::problem(name) {
            bail!(problem);
        }
        self.verify(preset)?;
        let path = self.folder.join(format!("{name}.json"));
        let same = name::key(name) == name::key(&preset.name());
        // Another case of its own name is the same file there, which the
        // move renames; another preset with the name refuses it.
        if !same && self.list()?.iter().any(|p| name::key(&p.name()) == name::key(name)) {
            bail!("A preset named “{name}” already exists.");
        }
        move_no_replace(&preset.path, &path).map_err(|error| {
            if exists(&error) {
                anyhow!("A preset named “{name}” already exists.")
            } else if in_use(&error) {
                in_use_error(&preset.name())
            } else {
                anyhow!("Couldn’t rename the preset: {}.", error.message().trim_end_matches(['.', '\r', '\n']))
            }
        })?;
        let mut order = self.read_order();
        if let Some(index) = order.iter().position(|n| name::key(n) == name::key(&preset.name())) {
            order[index] = name.to_owned();
            self.write_order(&order)?;
        }
        file_at(&path)
    }

    pub fn delete(&self, preset: &PresetFile) -> Result<()> {
        self.verify(preset)?;
        std::fs::remove_file(&preset.path).map_err(|e| file_error(e, &preset.path, "Couldn’t delete"))?;
        let order: Vec<String> = self.read_order().into_iter().filter(|n| name::key(n) != name::key(&preset.name())).collect();
        self.write_order(&order)
    }

    pub fn read(&self, preset: &PresetFile) -> Result<Vec<u8>> {
        self.verify(preset)?;
        read_file(&preset.path)
    }

    /// Preset names in the user's order; none if the file is missing or unreadable.
    fn read_order(&self) -> Vec<String> {
        let Ok(data) = std::fs::read(self.order_file()) else { return vec![] };
        let Ok(value) = serde_json::from_slice::<serde_json::Value>(&data) else { return vec![] };
        value["order"]
            .as_array()
            .map(|names| names.iter().filter_map(|n| n.as_str()).map(|n| n.strip_suffix(".json").unwrap_or(n).to_owned()).collect())
            .unwrap_or_default()
    }

    fn write_order(&self, names: &[String]) -> Result<()> {
        let files: Vec<String> = names.iter().map(|n| format!("{n}.json")).collect();
        let data = serde_json::to_vec_pretty(&serde_json::json!({"order": files}))?;
        let path = self.order_file();
        crate::session::write_atomic(&path, &data).map_err(|e| anyhow!("Couldn’t save the order of the presets: {e:#}"))?;
        // Kept out of Explorer's way, as on the Mac.
        crate::platform::hide_file(&path);
        Ok(())
    }

    /// The file is still the one listed: not replaced, edited or removed since.
    fn verify(&self, preset: &PresetFile) -> Result<()> {
        match file_at(&preset.path) {
            Ok(current) if current.size == preset.size && current.modified == preset.modified => Ok(()),
            _ => bail!("“{}” changed on disk. The list has been updated; try again.", preset.name()),
        }
    }
}

/// A regular file's size and modification time, read now.
fn file_at(path: &Path) -> Result<PresetFile> {
    let metadata = std::fs::metadata(path).map_err(|e| file_error(e, path, "Couldn’t read"))?;
    if !metadata.is_file() {
        bail!("“{}” isn’t a file.", path.file_name().unwrap_or_default().to_string_lossy());
    }
    Ok(PresetFile { path: path.to_path_buf(), size: metadata.len(), modified: metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH) })
}

/// `MoveFileExW` without `MOVEFILE_REPLACE_EXISTING`: it fails if the target
/// exists, unlike `std::fs::rename`, and on every file system (a hard link
/// needs NTFS).
fn move_no_replace(from: &Path, to: &Path) -> windows::core::Result<()> {
    let (f, t) = (wide(from), wide(to));
    unsafe { MoveFileExW(pcwstr(&f), pcwstr(&t), MOVEFILE_WRITE_THROUGH) }
}

fn exists(error: &windows::core::Error) -> bool {
    error.code() == ERROR_ALREADY_EXISTS.to_hresult() || error.code() == ERROR_FILE_EXISTS.to_hresult()
}

fn in_use(error: &windows::core::Error) -> bool {
    error.code() == ERROR_SHARING_VIOLATION.to_hresult() || error.code() == ERROR_LOCK_VIOLATION.to_hresult()
}

/// Explorer's preview pane, an editor or a virus scanner has it open.
fn in_use_error(name: &str) -> anyhow::Error {
    anyhow!("“{name}” is open in another program, such as Explorer’s preview pane. Close it and try again.")
}

fn file_error(error: std::io::Error, path: &Path, what: &str) -> anyhow::Error {
    let name = path.file_stem().unwrap_or_default().to_string_lossy();
    match error.raw_os_error() {
        Some(code) if code == ERROR_SHARING_VIOLATION.0 as i32 || code == ERROR_LOCK_VIOLATION.0 as i32 => in_use_error(&name),
        _ => anyhow!("{what} “{name}”: {error}."),
    }
}

fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
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
