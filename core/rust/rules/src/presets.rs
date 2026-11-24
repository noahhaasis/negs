//! windows/src/presets.rs on the shared core (core/src/settings/presets):
//! preset files in `%APPDATA%\Negs\Presets` holding Python spektrafilm's
//! saved GUI state, named after the file. The folder's operations, the
//! order file, the name rules and the file calls (MoveFileExW without
//! replacing, the hidden attribute, sharing and lock violations reported
//! as "open in another program") run in Odin; PresetFile and Status stay
//! Rust data. Names are compared through the host's NFC and lower case,
//! which this module registers with the core (`text::register`).
//!
//! Kept in Rust: `PresetFile::name` (a Path accessor) and `text`'s String.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use anyhow::{Result, anyhow, bail};
use negs_core_sys::settings as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde_json::Value;

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
    /// With the file's text and RAW white balance, for its preview.
    Valid { film: String, paper: String, scan: bool, text: String, white_balance: Value },
    Invalid(String),
}

/// One naming policy on every platform: names Windows, macOS and Linux all
/// accept as file names, compared without regard to case or Unicode form.
pub mod name {
    use super::{bytes, text, utf8};
    use negs_core_sys::settings as sys;
    use std::collections::HashSet;

    pub const MAX_LENGTH: usize = 120;

    /// Why `name` can't name a preset, or None.
    pub fn problem(name: &str) -> Option<String> {
        let mut out = super::empty_buf();
        let bad = unsafe { sys::negs_presets_name_problem(name.as_ptr(), name.len(), &mut out) } != 0;
        bad.then(|| utf8(bytes(out)))
    }

    /// Names that are the same preset: NFC-normalized and case-folded. NTFS
    /// compares names without case but doesn't normalize them.
    pub fn key(name: &str) -> String {
        text::register();
        utf8(unsafe { super::take(sys::negs_presets_name_key(name.as_ptr(), name.len())) })
    }

    /// `name`, or "name 2", "name 3"… the first whose key isn't `taken`.
    pub fn unique(name: &str, taken: &HashSet<String>) -> String {
        text::register();
        let keys: Vec<sys::negs_str_t> = taken.iter().map(|k| sys::negs_str_t { data: k.as_ptr(), len: k.len() }).collect();
        utf8(unsafe { super::take(sys::negs_presets_name_unique(name.as_ptr(), name.len(), keys.as_ptr(), keys.len())) })
    }

    /// A file's name as a preset name: invalid characters become spaces.
    pub fn from_file_name(file_name: &str) -> String {
        utf8(unsafe { super::take(sys::negs_presets_name_from_file_name(file_name.as_ptr(), file_name.len())) })
    }
}

/// Larger files aren't GUI states.
pub const MAX_FILE_SIZE: u64 = 1_048_576;

/// A file's text for the engine. Notepad may have added a byte order mark.
pub fn text(data: &[u8]) -> Result<String> {
    let start = unsafe { sys::negs_presets_text_start(data.as_ptr(), data.len()) };
    if start < 0 {
        bail!("The file isn’t text.");
    }
    String::from_utf8(data[start as usize..].to_vec()).map_err(|_| anyhow!("The file isn’t text."))
}

/// A file's bytes, within the size limit.
pub fn read_file(path: &Path) -> Result<Vec<u8>> {
    let p = path_text(path);
    let (mut out, mut error) = (empty_buf(), empty_buf());
    let ok = unsafe { sys::negs_presets_read_file(p.as_ptr(), p.len(), &mut out, &mut error) } != 0;
    if ok { Ok(bytes(out)) } else { Err(anyhow!(utf8(bytes(error)))) }
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
    #[cfg(any(test, feature = "host-tests"))]
    pub fn new(folder: PathBuf) -> Self {
        Self { folder }
    }

    /// The presets in the user's order. Hidden files (a leading period or the
    /// hidden attribute) and files other than `.json` aren't presets.
    pub fn list(&self) -> Result<Vec<PresetFile>> {
        text::register();
        let folder = path_text(&self.folder);
        let (mut out, mut error) = (sys::negs_preset_list_t::default(), empty_buf());
        let ok = unsafe { sys::negs_presets_list(folder.as_ptr(), folder.len(), &mut out, &mut error) } != 0;
        if ok { Ok(take_list(out)) } else { Err(anyhow!(utf8(bytes(error)))) }
    }

    /// Remembers this order of the presets.
    pub fn set_order(&self, presets: &[PresetFile]) -> Result<()> {
        let folder = path_text(&self.folder);
        let held: Vec<Held> = presets.iter().map(Held::new).collect();
        let files: Vec<sys::negs_preset_file_t> = held.iter().map(Held::raw).collect();
        let mut error = empty_buf();
        let ok = unsafe { sys::negs_presets_set_order(folder.as_ptr(), folder.len(), files.as_ptr(), files.len(), &mut error) } != 0;
        if ok { Ok(()) } else { Err(anyhow!(utf8(bytes(error)))) }
    }

    /// Writes `data` as a new preset named `name`, or "name 2"… if taken.
    pub fn add(&self, data: &[u8], name: &str) -> Result<PresetFile> {
        text::register();
        let folder = path_text(&self.folder);
        let (mut out, mut error) = (sys::negs_preset_list_t::default(), empty_buf());
        let ok = unsafe {
            sys::negs_presets_add(folder.as_ptr(), folder.len(), data.as_ptr(), data.len(), name.as_ptr(), name.len(), &mut out, &mut error)
        } != 0;
        one(ok, out, error)
    }

    pub fn rename(&self, preset: &PresetFile, name: &str) -> Result<PresetFile> {
        text::register();
        let folder = path_text(&self.folder);
        let held = Held::new(preset);
        let raw = held.raw();
        let (mut out, mut error) = (sys::negs_preset_list_t::default(), empty_buf());
        let ok =
            unsafe { sys::negs_presets_rename(folder.as_ptr(), folder.len(), &raw, name.as_ptr(), name.len(), &mut out, &mut error) } != 0;
        one(ok, out, error)
    }

    pub fn delete(&self, preset: &PresetFile) -> Result<()> {
        text::register();
        let folder = path_text(&self.folder);
        let held = Held::new(preset);
        let raw = held.raw();
        let mut error = empty_buf();
        let ok = unsafe { sys::negs_presets_delete(folder.as_ptr(), folder.len(), &raw, &mut error) } != 0;
        if ok { Ok(()) } else { Err(anyhow!(utf8(bytes(error)))) }
    }

    pub fn read(&self, preset: &PresetFile) -> Result<Vec<u8>> {
        let held = Held::new(preset);
        let raw = held.raw();
        let (mut out, mut error) = (empty_buf(), empty_buf());
        let ok = unsafe { sys::negs_presets_read(&raw, &mut out, &mut error) } != 0;
        if ok { Ok(bytes(out)) } else { Err(anyhow!(utf8(bytes(error)))) }
    }
}

// MARK: Crossing

fn empty_buf() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

fn bytes(buf: negs_buf_t) -> Vec<u8> {
    take_buf(buf)
}

unsafe fn take(buf: negs_buf_t) -> Vec<u8> {
    take_buf(buf)
}

fn utf8(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// A path as the core takes it (UTF-8; a Windows path with an unpaired
/// surrogate goes lossily).
pub(crate) fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A modification time as seconds since 1970 (negative before) and
/// nanoseconds, and back.
fn split_time(t: SystemTime) -> (i64, u32) {
    match t.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => (d.as_secs() as i64, d.subsec_nanos()),
        Err(e) => {
            let d = e.duration();
            if d.subsec_nanos() == 0 { (-(d.as_secs() as i64), 0) } else { (-(d.as_secs() as i64) - 1, 1_000_000_000 - d.subsec_nanos()) }
        }
    }
}

fn join_time(seconds: i64, nanos: u32) -> SystemTime {
    if seconds >= 0 {
        SystemTime::UNIX_EPOCH + Duration::new(seconds as u64, nanos)
    } else {
        SystemTime::UNIX_EPOCH - Duration::new(seconds.unsigned_abs(), 0) + Duration::new(0, nanos)
    }
}

/// A PresetFile going in: its path's text, held for the call.
struct Held {
    path: String,
    size: u64,
    time: (i64, u32),
}

impl Held {
    fn new(f: &PresetFile) -> Self {
        Self { path: path_text(&f.path), size: f.size, time: split_time(f.modified) }
    }

    fn raw(&self) -> sys::negs_preset_file_t {
        sys::negs_preset_file_t { path: self.path.as_ptr(), path_len: self.path.len(), size: self.size, modified_s: self.time.0, modified_ns: self.time.1 }
    }
}

fn take_list(list: sys::negs_preset_list_t) -> Vec<PresetFile> {
    if list.files.is_null() {
        return vec![];
    }
    let files = unsafe { std::slice::from_raw_parts(list.files, list.count) };
    let out = files
        .iter()
        .map(|f| {
            let path = unsafe { std::slice::from_raw_parts(f.path, f.path_len) };
            PresetFile { path: PathBuf::from(utf8(path.to_vec())), size: f.size, modified: join_time(f.modified_s, f.modified_ns) }
        })
        .collect();
    unsafe { sys::negs_preset_list_free(list) };
    out
}

fn one(ok: bool, out: sys::negs_preset_list_t, error: negs_buf_t) -> Result<PresetFile> {
    if !ok {
        return Err(anyhow!(utf8(bytes(error))));
    }
    take_list(out).into_iter().next().ok_or_else(|| anyhow!("Couldn’t read the preset."))
}

/// The host's text functions for preset names: Rust's `to_lowercase`, and
/// NFC as presets.rs made it on Windows (NormalizeString); elsewhere the
/// core leaves the form as it is (the differential tests register ICU's).
pub mod text {
    use std::sync::Once;

    static REGISTER: Once = Once::new();

    /// Registers them with the core, once.
    pub fn register() {
        REGISTER.call_once(|| unsafe {
            negs_core_sys::settings::negs_settings_text_functions(nfc_fn(), Some(lowercase), std::ptr::null_mut());
        });
    }

    /// Writes `result` into the core's buffer as a `negs_text_fn` does.
    pub fn give(result: &str, out: *mut u8, cap: usize) -> usize {
        if !out.is_null() && result.len() <= cap {
            unsafe { std::ptr::copy_nonoverlapping(result.as_ptr(), out, result.len()) };
        }
        result.len()
    }

    /// The core's text, borrowed for the call.
    pub fn take<'a>(text: *const u8, len: usize) -> std::borrow::Cow<'a, str> {
        if text.is_null() || len == 0 {
            return "".into();
        }
        String::from_utf8_lossy(unsafe { std::slice::from_raw_parts(text, len) })
    }

    /// `negs_text_fn` for `str::to_lowercase`.
    pub unsafe extern "C" fn lowercase(_user: *mut std::ffi::c_void, text: *const u8, len: usize, out: *mut u8, cap: usize) -> usize {
        give(&take(text, len).to_lowercase(), out, cap)
    }

    #[cfg(windows)]
    fn nfc_fn() -> Option<negs_core_sys::settings::negs_text_fn> {
        Some(nfc)
    }

    #[cfg(not(windows))]
    fn nfc_fn() -> Option<negs_core_sys::settings::negs_text_fn> {
        None
    }

    #[cfg(windows)]
    #[link(name = "normaliz", kind = "raw-dylib")]
    unsafe extern "system" {
        fn NormalizeString(form: i32, source: *const u16, source_len: i32, destination: *mut u16, destination_len: i32) -> i32;
    }

    /// presets.rs's `nfc`: NormalizeString(NormalizationC), asked for the
    /// length first; the text as it is when that fails.
    #[cfg(windows)]
    unsafe extern "C" fn nfc(_user: *mut std::ffi::c_void, text: *const u8, len: usize, out: *mut u8, cap: usize) -> usize {
        const NORMALIZATION_C: i32 = 1;
        let text = take(text, len);
        let source: Vec<u16> = text.encode_utf16().collect();
        if source.is_empty() {
            return give("", out, cap);
        }
        let mut length = unsafe { NormalizeString(NORMALIZATION_C, source.as_ptr(), source.len() as i32, std::ptr::null_mut(), 0) };
        for _ in 0..3 {
            if length <= 0 {
                break;
            }
            let mut buffer = vec![0u16; length as usize];
            let written = unsafe { NormalizeString(NORMALIZATION_C, source.as_ptr(), source.len() as i32, buffer.as_mut_ptr(), length) };
            if written > 0 {
                return give(&String::from_utf16_lossy(&buffer[..written as usize]), out, cap);
            }
            length = -written;
        }
        give(&text, out, cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    struct Folder(PathBuf);
    impl Folder {
        fn new(tag: &str) -> Self {
            let nanos = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_nanos();
            Self(std::env::temp_dir().join(format!("negs-presets-{tag}-{}-{nanos:x}", std::process::id())))
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
        assert_eq!(name::MAX_LENGTH as u64, unsafe { sys::negs_presets_limit(0) });
        assert_eq!(MAX_FILE_SIZE, unsafe { sys::negs_presets_limit(1) });
    }

    #[test]
    fn names_compare_without_case_or_unicode_form() {
        // NFC is the host's: Windows' NormalizeString (no stand-in here).
        #[cfg(windows)]
        assert_eq!(name::key("Ektachrome caf\u{E9}"), name::key("EKTACHROME CAFE\u{301}"));
        assert_eq!(name::key("ΣΑΣ"), "ΣΑΣ".to_lowercase());
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
    fn times_cross_both_ways() {
        for t in [
            SystemTime::UNIX_EPOCH,
            SystemTime::UNIX_EPOCH + Duration::new(1_700_000_000, 123_456_700),
            SystemTime::UNIX_EPOCH - Duration::new(5, 300),
            SystemTime::UNIX_EPOCH - Duration::new(5, 0),
        ] {
            let (s, n) = split_time(t);
            assert_eq!(join_time(s, n), t);
        }
    }

    #[test]
    fn adding_never_replaces_and_renaming_checks_names() {
        let folder = Folder::new("add");
        let library = Library::new(folder.0.clone());
        let first = library.add(b"1", "Portra").unwrap();
        let second = library.add(b"2", "portra").unwrap();
        assert_eq!(second.name(), "portra 2");
        assert_eq!(std::fs::read(&first.path).unwrap(), b"1");
        std::fs::write(folder.0.join(".partial.tmp"), b"").unwrap();
        std::fs::write(folder.0.join("notes.txt"), b"").unwrap();
        assert_eq!(names(&library.list().unwrap()), ["Portra", "portra 2"]);
        let clash = library.rename(&second, "PORTRA").unwrap_err().to_string();
        assert!(clash.contains("already exists"), "{clash}");
        assert!(library.rename(&second, "a/b").is_err());
        assert!(library.rename(&second, "NUL").is_err());
        let recased = library.rename(&first, "PORTRA").unwrap();
        assert_eq!(names(&library.list().unwrap()), ["PORTRA", "portra 2"]);
        let renamed = library.rename(&recased, "Portra warm").unwrap();
        assert_eq!(library.read(&renamed).unwrap(), b"1");
        let leftovers: Vec<_> =
            std::fs::read_dir(&folder.0).unwrap().filter_map(|e| e.ok()).filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).collect();
        assert_eq!(leftovers.len(), 1, "only the test's own .partial.tmp");
    }

    #[test]
    fn the_users_order_is_remembered_and_added_presets_go_last() {
        let folder = Folder::new("order");
        let library = Library::new(folder.0.clone());
        for n in ["Zeta", "Alpha", "Mid"] {
            library.add(n.as_bytes(), n).unwrap();
        }
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Zeta", "Alpha", "Mid"]);
        library.set_order(&[listed[2].clone(), listed[0].clone(), listed[1].clone()]).unwrap();
        std::fs::write(folder.0.join("Copied B.json"), b"x").unwrap();
        std::fs::write(folder.0.join("Copied A.json"), b"x").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Mid", "Zeta", "Alpha", "Copied A", "Copied B"]);
        library.rename(&listed[1], "Omega").unwrap();
        library.delete(&listed[0]).unwrap();
        library.add(b"n", "Newest").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Omega", "Alpha", "Copied A", "Copied B", "Newest"]);
        assert_eq!(names(&Library::new(folder.0.clone()).list().unwrap()), names(&listed));
        let order: serde_json::Value = serde_json::from_slice(&std::fs::read(folder.0.join(".order.json")).unwrap()).unwrap();
        assert_eq!(order["order"][0], "Omega.json");
        library.set_order(&listed).unwrap();
    }

    #[test]
    fn files_changed_on_disk_are_left_alone() {
        let folder = Folder::new("changed");
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"original", "Gold").unwrap();
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
}
