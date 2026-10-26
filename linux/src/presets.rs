//! Presets: files in the app's presets folder holding Python spektrafilm's
//! saved GUI state, named after the file (docs/presets-design.md). The helper
//! reads and writes the format; this handles the files and writes the
//! helper's params back into the inspector's controls.
use crate::{model::State, parameters};
use gtk::glib;
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::SystemTime,
};

pub type Result<T> = std::result::Result<T, String>;

/// A preset file as listed: later changes on disk are detected by size and
/// modification time.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Preset {
    pub path: PathBuf,
    pub size: u64,
    pub modified: SystemTime,
}
impl Preset {
    pub fn name(&self) -> String {
        self.path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// What the helper made of a preset file.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Valid {
        film: String,
        paper: String,
        scan: bool,
    },
    Invalid(String),
}

pub const MAX_NAME: usize = 120;
/// Larger files aren't GUI states.
pub const MAX_FILE_SIZE: u64 = 1_048_576;
const FORBIDDEN: &str = "/\\:*?\"<>|";

/// Why `name` can't name a preset, or None. One policy on every platform:
/// names Windows, macOS and Linux all accept as file names.
pub fn problem(name: &str) -> Option<String> {
    if name.trim().is_empty() {
        return Some("Enter a name.".into());
    }
    if name.chars().count() > MAX_NAME {
        return Some(format!("Use at most {MAX_NAME} characters."));
    }
    if name
        .chars()
        .any(|c| c.is_control() || FORBIDDEN.contains(c))
    {
        return Some("A name can’t contain / \\ : * ? \" < > or |.".into());
    }
    if name.starts_with('.') {
        return Some("A name can’t start with a period.".into());
    }
    if name.ends_with(['.', ' ']) || name.starts_with(' ') {
        return Some("A name can’t start with a space or end with a space or period.".into());
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim()
        .to_uppercase();
    let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
        || ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix)
                .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
        });
    reserved.then(|| format!("“{name}” is reserved on Windows."))
}

/// Names that are the same preset: NFC-normalized and case-folded.
pub fn key(name: &str) -> String {
    let compose = glib::NormalizeMode::DefaultCompose;
    glib::normalize(glib::casefold(glib::normalize(name, compose)), compose).into()
}

/// `name`, or "name 2", "name 3"… the first whose key isn't `taken`.
pub fn unique(name: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(&key(name)) {
        return name.into();
    }
    (2..)
        .map(|n| format!("{name} {n}"))
        .find(|candidate| !taken.contains(&key(candidate)))
        .unwrap()
}

/// A file's name as a preset name: invalid characters become spaces.
pub fn from_file_name(file_name: &str) -> String {
    let replaced: String = file_name
        .chars()
        .map(|c| {
            if FORBIDDEN.contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect();
    let trim = |s: &str| s.trim_matches([' ', '.']).to_owned();
    let mut name = trim(
        &replaced
            .split(' ')
            .filter(|w| !w.is_empty())
            .collect::<Vec<_>>()
            .join(" "),
    );
    if name.chars().count() > MAX_NAME {
        name = trim(&name.chars().take(MAX_NAME).collect::<String>());
    }
    if problem(&name).is_none() {
        name
    } else {
        "Preset".into()
    }
}

/// The presets folder: `$NEGS_PROFILE_DIR/presets` when isolated, like the
/// session, else `$XDG_DATA_HOME/negs/presets`.
pub fn folder() -> PathBuf {
    std::env::var_os("NEGS_PROFILE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            glib::user_data_dir().join(if crate::runtime::ui_only() {
                "negs/ui-preview"
            } else {
                "negs"
            })
        })
        .join("presets")
}

/// The presets folder. Writes never replace an existing file; renames and
/// deletes act only on the file as it was listed, one at a time.
///
/// The user's order is `.order.json` in the folder: `{"order": [file names]}`,
/// the format the macOS app writes. Presets it doesn't name, such as files
/// copied in by hand, follow in name order; added presets go to the end.
#[derive(Clone)]
pub struct Library {
    pub folder: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl Library {
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            lock: Arc::default(),
        }
    }
    fn order_file(&self) -> PathBuf {
        self.folder.join(".order.json")
    }

    pub fn list(&self) -> Result<Vec<Preset>> {
        let entries = match std::fs::read_dir(&self.folder) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(error) => return Err(format!("Couldn’t list the presets: {error}.")),
        };
        let mut files: Vec<_> = entries
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                let name = path.file_name()?.to_str()?;
                let json = Path::new(name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("json"));
                (json && !name.starts_with('.'))
                    .then(|| file(&path).ok())
                    .flatten()
            })
            .map(|preset| (key(&preset.name()), preset))
            .collect();
        files.sort_by(|a, b| a.0.cmp(&b.0));
        let mut position = HashMap::new();
        for (index, name) in self.read_order().iter().enumerate() {
            position.entry(key(name)).or_insert(index);
        }
        // Stable: unordered files keep their name order after the ordered ones.
        files.sort_by_key(|(key, _)| position.get(key).copied().unwrap_or(usize::MAX));
        Ok(files.into_iter().map(|(_, preset)| preset).collect())
    }

    /// Remembers this order of the presets.
    pub fn set_order(&self, presets: &[Preset]) -> Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        self.write_order(&presets.iter().map(Preset::name).collect::<Vec<_>>())
    }

    /// Writes `data` as a new preset named `name`, or "name 2"… if taken.
    pub fn add(&self, data: &[u8], name: &str) -> Result<Preset> {
        if let Some(problem) = problem(name) {
            return Err(problem);
        }
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        std::fs::create_dir_all(&self.folder)
            .map_err(|e| format!("Couldn’t create the presets folder: {e}."))?;
        let temporary = Temporary(
            self.folder
                .join(format!(".{}.tmp", crate::engine::unique())),
        );
        {
            use std::io::Write;
            let mut out = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary.0)
                .map_err(|e| format!("Couldn’t save the preset: {e}."))?;
            out.write_all(data)
                .and_then(|_| out.sync_all())
                .map_err(|e| format!("Couldn’t save the preset: {e}."))?;
        }
        let mut taken: BTreeSet<_> = self.list()?.iter().map(|p| key(&p.name())).collect();
        for _ in 0..1000 {
            let candidate = unique(name, &taken);
            let path = self.folder.join(format!("{candidate}.json"));
            // A hard link never replaces: a file created meanwhile fails it.
            match std::fs::hard_link(&temporary.0, &path) {
                Ok(()) => {
                    let mut names: Vec<_> = self
                        .list()?
                        .iter()
                        .map(Preset::name)
                        .filter(|n| key(n) != key(&candidate))
                        .collect();
                    names.push(candidate);
                    self.write_order(&names)?;
                    return file(&path);
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    taken.insert(key(&candidate));
                }
                Err(error) => return Err(format!("Couldn’t save the preset: {error}.")),
            }
        }
        Err("Couldn’t find a free name for the preset.".into())
    }

    pub fn rename(&self, preset: &Preset, name: &str) -> Result<Preset> {
        if let Some(problem) = problem(name) {
            return Err(problem);
        }
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        verify(preset)?;
        let target = self.folder.join(format!("{name}.json"));
        if target == preset.path {
            return Ok(preset.clone());
        }
        let exists = || format!("A preset named “{name}” already exists.");
        // Another case of its own name is the same preset. Linux file names
        // are case-sensitive, so that rename mustn't replace either.
        if self
            .list()?
            .iter()
            .any(|p| p.path != preset.path && key(&p.name()) == key(name))
        {
            return Err(exists());
        }
        rename_no_replace(&preset.path, &target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                exists()
            } else {
                format!("Couldn’t rename the preset: {error}.")
            }
        })?;
        let mut order = self.read_order();
        if let Some(entry) = order.iter_mut().find(|n| key(n) == key(&preset.name())) {
            *entry = name.into();
            self.write_order(&order)?;
        }
        file(&target)
    }

    pub fn delete(&self, preset: &Preset) -> Result<()> {
        let _guard = self.lock.lock().unwrap_or_else(|e| e.into_inner());
        verify(preset)?;
        std::fs::remove_file(&preset.path)
            .map_err(|e| format!("Couldn’t delete the preset: {e}."))?;
        let order = self.read_order();
        let kept: Vec<_> = order
            .iter()
            .filter(|n| key(n) != key(&preset.name()))
            .cloned()
            .collect();
        if kept.len() != order.len() {
            self.write_order(&kept)?;
        }
        Ok(())
    }

    /// Copies each file at `paths` that `check` accepts (given its text)
    /// under its own name, byte for byte. They go to the end, or before the
    /// preset listed at `position` (a drop between rows). Returns the added
    /// presets' paths and "file name: reason" for the others.
    pub fn import(
        &self,
        paths: &[PathBuf],
        position: Option<usize>,
        check: impl Fn(&str) -> Result<()>,
    ) -> (Vec<PathBuf>, Vec<String>) {
        let (mut added, mut failures) = (Vec::new(), Vec::new());
        for path in paths {
            let imported = read(path).and_then(|bytes| {
                check(&text(bytes.clone())?)?;
                let stem = path.file_stem().unwrap_or_default().to_string_lossy();
                self.add(&bytes, &from_file_name(&stem))
            });
            match imported {
                Ok(preset) => added.push(preset.path),
                Err(error) => failures.push(format!(
                    "{}: {error}",
                    path.file_name().unwrap_or_default().to_string_lossy()
                )),
            }
        }
        if let Some(position) = position
            && !added.is_empty()
        {
            let placed = self.list().and_then(|listed| {
                let (moved, mut order): (Vec<_>, Vec<_>) = listed
                    .iter()
                    .cloned()
                    .partition(|p| added.contains(&p.path));
                let before = listed[..position.min(listed.len())]
                    .iter()
                    .filter(|p| !added.contains(&p.path))
                    .count();
                order.splice(before..before, moved);
                self.set_order(&order)
            });
            if let Err(error) = placed {
                failures.push(error);
            }
        }
        (added, failures)
    }

    /// The listed file's bytes, if it hasn't changed since.
    pub fn read(&self, preset: &Preset) -> Result<Vec<u8>> {
        verify(preset)?;
        read(&preset.path)
    }

    /// Preset names in the user's order; none if the file is missing or unreadable.
    fn read_order(&self) -> Vec<String> {
        std::fs::read(self.order_file())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            .and_then(|value| {
                value["order"].as_array().map(|files| {
                    files
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|f| f.strip_suffix(".json").unwrap_or(f).to_owned())
                        .collect()
                })
            })
            .unwrap_or_default()
    }

    fn write_order(&self, names: &[String]) -> Result<()> {
        let files: Vec<_> = names.iter().map(|n| format!("{n}.json")).collect();
        let bytes = serde_json::to_vec_pretty(&json!({ "order": files })).unwrap();
        let temporary = Temporary(
            self.folder
                .join(format!(".order-{}.tmp", crate::engine::unique())),
        );
        std::fs::write(&temporary.0, bytes)
            .and_then(|_| std::fs::rename(&temporary.0, self.order_file()))
            .map_err(|e| format!("Couldn’t save the order of the presets: {e}."))
    }
}

/// Removed when dropped, if still there.
struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// A file's bytes, within the size limit.
pub fn read(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if metadata.len() > MAX_FILE_SIZE {
        return Err("The file is too large to be a preset.".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

pub fn text(bytes: Vec<u8>) -> Result<String> {
    String::from_utf8(bytes).map_err(|_| "The file isn’t text.".into())
}

/// A regular file's size and modification time, read now.
fn file(path: &Path) -> Result<Preset> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() {
        return Err(format!("“{}” isn’t a file.", path.display()));
    }
    Ok(Preset {
        path: path.into(),
        size: metadata.len(),
        modified: metadata.modified().map_err(|e| e.to_string())?,
    })
}

/// The file is still the one listed: not replaced, edited or removed since.
fn verify(preset: &Preset) -> Result<()> {
    match file(&preset.path) {
        Ok(current) if current.size == preset.size && current.modified == preset.modified => Ok(()),
        _ => Err(format!(
            "“{}” changed on disk. The list has been updated; try again.",
            preset.name()
        )),
    }
}

/// `renameat2(RENAME_NOREPLACE)`, or a hard link and unlink where the file
/// system doesn't offer it. Neither replaces an existing file.
fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: i32,
            oldpath: *const std::ffi::c_char,
            newdirfd: i32,
            newpath: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    const AT_FDCWD: i32 = -100;
    const RENAME_NOREPLACE: u32 = 1;
    let c =
        |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (old, new) = (c(from)?, c(to)?);
    if unsafe {
        renameat2(
            AT_FDCWD,
            old.as_ptr(),
            AT_FDCWD,
            new.as_ptr(),
            RENAME_NOREPLACE,
        )
    } == 0
    {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    // EINVAL or ENOSYS: unsupported here.
    if !matches!(error.raw_os_error(), Some(22 | 38)) {
        return Err(error);
    }
    std::fs::hard_link(from, to)?;
    std::fs::remove_file(from)
}

/// The keys a preset writes through: every control backed by a param, except
/// Post (a preset keeps it) and Image Size (no param).
fn value_keys() -> impl Iterator<Item = String> {
    crate::fixtures::GROUPS
        .iter()
        .flat_map(|group| {
            group
                .knobs
                .iter()
                .map(move |knob| format!("{}.{}", group.name, knob.0))
        })
        .chain(["RAW.Temperature".to_owned(), "RAW.Tint".to_owned()])
}

/// Writes the helper's `preset_import` result into `state`: control-backed
/// paths into `values` and `choices`, the rest into `params`, and film and
/// paper without the film handler's side effects (which would discard the
/// preset's grain, DIR and halation values). Afterwards
/// `parameters::runtime(state, defaults)` gives back `params`.
pub fn apply(state: &mut State, film: &str, paper: &str, params: &Value) -> Result<()> {
    let known = |id: &str, paper: bool| crate::catalog::stocks(paper).iter().any(|s| s.id == id);
    if !known(film, false) || !known(paper, true) {
        return Err("This version of Negs doesn’t have the preset’s film or paper.".into());
    }
    let mut next = state.clone();
    next.choices.insert("Film ID".into(), film.into());
    next.choices.insert("Paper ID".into(), paper.into());
    for key in value_keys() {
        let path = parameters::path(&key).unwrap();
        let value = parameters::get(params, &path)
            .as_f64()
            .ok_or_else(|| format!("The preset has no value for {key}."))?;
        next.values.insert(key, value);
    }
    for &(key, path) in parameters::TOGGLES {
        let value = parameters::get(params, path)
            .as_bool()
            .ok_or_else(|| format!("The preset has no value for {key}."))?;
        next.choices.insert(key.into(), value.to_string());
    }
    for &(key, path, options) in parameters::CHOICES {
        let value = parameters::get(params, path).as_str().unwrap_or_default();
        let index = options
            .iter()
            .position(|&option| option == value)
            .ok_or_else(|| format!("The Linux app has no {key} option “{value}”."))?;
        next.choices.insert(key.into(), index.to_string());
    }
    // The development dropdowns list "Profile default" and then the stock's times.
    for (key, path, paper) in [
        ("Development", "film_render.development_time", false),
        ("Print development", "print_render.development_time", true),
    ] {
        let times = &crate::catalog::selected(&next, paper).times;
        let index = parameters::get(params, path).as_f64().and_then(|time| {
            (0..times.len())
                .min_by(|&a, &b| (times[a] - time).abs().total_cmp(&(times[b] - time).abs()))
                .map(|i| i + 1)
        });
        match index {
            Some(index) => next.choices.insert(key.into(), index.to_string()),
            None => next.choices.remove(key),
        };
    }
    next.params = params.clone();
    *state = next;
    Ok(())
}

/// The helper's verdict on a listed file, kept while the file is unchanged.
#[derive(Default)]
pub struct Verdicts(HashMap<PathBuf, (u64, SystemTime, Status)>);
impl Verdicts {
    pub fn get(&self, preset: &Preset) -> Option<&Status> {
        self.0
            .get(&preset.path)
            .filter(|(size, modified, _)| *size == preset.size && *modified == preset.modified)
            .map(|(_, _, status)| status)
    }
    pub fn insert(&mut self, preset: &Preset, status: Status) {
        self.0
            .insert(preset.path.clone(), (preset.size, preset.modified, status));
    }
    pub fn retain(&mut self, presets: &[Preset]) {
        self.0
            .retain(|path, _| presets.iter().any(|p| &p.path == path));
    }
}

/// The verdict a `preset_import` reply gives.
pub fn status(reply: Result<Value>) -> Status {
    match reply {
        Ok(result) => Status::Valid {
            film: result["film"].as_str().unwrap_or_default().into(),
            paper: result["paper"].as_str().unwrap_or_default().into(),
            scan: result["params"]["io"]["scan_film"] == true,
        },
        Err(reason) => Status::Invalid(reason),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Folder(PathBuf);
    impl Folder {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("negs-presets-{}", crate::engine::unique()));
            Self(path)
        }
    }
    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn names(presets: &[Preset]) -> Vec<String> {
        presets.iter().map(Preset::name).collect()
    }

    #[test]
    fn names_every_platform_accepts() {
        for name in [
            "Portra 400 warm",
            "Kodachrome – 1970s",
            "Trix (push 2)",
            "Ünïcødé",
            "v1.2",
            "COM0",
            "LPT10",
        ] {
            assert_eq!(problem(name), None, "{name}");
        }
        let long = "x".repeat(121);
        for name in [
            "",
            "   ",
            "a/b",
            "a\\b",
            "a:b",
            "what?",
            "a*",
            "\"quoted\"",
            "<x>",
            "a|b",
            ".hidden",
            "trailing.",
            "trailing ",
            " leading",
            "tab\there",
            "CON",
            "con",
            "Lpt1",
            "NUL.json",
            "COM9.backup",
            &long,
        ] {
            assert!(problem(name).is_some(), "{name:?}");
        }
        assert_eq!(problem(&"é".repeat(120)), None);
    }

    #[test]
    fn names_compare_without_case_or_unicode_form() {
        assert_eq!(key("Ektachrome caf\u{E9}"), key("EKTACHROME CAFE\u{301}"));
        let taken = BTreeSet::from([key("Portra"), key("portra 2")]);
        assert_eq!(unique("PORTRA", &taken), "PORTRA 3");
        assert_eq!(unique("Gold", &taken), "Gold");
    }

    #[test]
    fn file_names_become_preset_names() {
        assert_eq!(from_file_name("05 P400 natural"), "05 P400 natural");
        assert_eq!(from_file_name("a:b|c"), "a b c");
        assert_eq!(from_file_name("..."), "Preset");
        assert_eq!(from_file_name("CON"), "Preset");
        assert_eq!(from_file_name(" .Velvia\n100. "), "Velvia 100");
    }

    #[test]
    fn adding_never_replaces_and_renaming_checks_names() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let first = library.add(b"1", "Portra").unwrap();
        let second = library.add(b"2", "portra").unwrap();
        assert_eq!(second.name(), "portra 2");
        assert_eq!(std::fs::read(&first.path).unwrap(), b"1");
        // Hidden temporary files and other files aren't presets.
        std::fs::write(folder.0.join(".partial.tmp"), b"").unwrap();
        std::fs::write(folder.0.join(".hidden.json"), b"").unwrap();
        std::fs::write(folder.0.join("notes.txt"), b"").unwrap();
        std::fs::create_dir(folder.0.join("folder.json")).unwrap();
        assert_eq!(names(&library.list().unwrap()), ["Portra", "portra 2"]);

        let error = library.rename(&second, "PORTRA").unwrap_err();
        assert!(error.contains("already exists"), "{error}");
        assert!(library.rename(&second, "a/b").is_err());
        // Another case of its own name is the same file.
        let recased = library.rename(&first, "PORTRA").unwrap();
        assert_eq!(names(&library.list().unwrap()), ["PORTRA", "portra 2"]);
        let renamed = library.rename(&recased, "Portra warm").unwrap();
        assert_eq!(library.read(&renamed).unwrap(), b"1");
        // Nothing left behind but the presets and the order.
        let mut files: Vec<_> = std::fs::read_dir(&folder.0)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                ".hidden.json",
                ".order.json",
                ".partial.tmp",
                "Portra warm.json",
                "folder.json",
                "notes.txt",
                "portra 2.json"
            ]
        );
    }

    #[test]
    fn renaming_never_replaces_a_file_created_meanwhile() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"mine", "Gold").unwrap();
        // Not listed yet when renaming: renameat2 must still refuse.
        std::fs::write(folder.0.join("Other.json"), b"theirs").unwrap();
        let target = folder.0.join("Other.json");
        let error = rename_no_replace(&preset.path, &target).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&target).unwrap(), b"theirs");
        assert!(
            library
                .rename(&preset, "Other")
                .unwrap_err()
                .contains("already exists")
        );
        assert_eq!(std::fs::read(&preset.path).unwrap(), b"mine");
    }

    #[test]
    fn the_users_order_is_remembered_and_added_presets_go_last() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        for name in ["Zeta", "Alpha", "Mid"] {
            library.add(name.as_bytes(), name).unwrap();
        }
        let listed = library.list().unwrap();
        assert_eq!(names(&listed), ["Zeta", "Alpha", "Mid"]);
        library
            .set_order(&[listed[2].clone(), listed[0].clone(), listed[1].clone()])
            .unwrap();
        // Copied in by hand: after the ordered ones, in name order.
        std::fs::write(folder.0.join("Copied B.json"), b"x").unwrap();
        std::fs::write(folder.0.join("copied a.json"), b"x").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(
            names(&listed),
            ["Mid", "Zeta", "Alpha", "copied a", "Copied B"]
        );
        // Renaming keeps the place; deleting forgets it; adding goes last.
        library.rename(&listed[1], "Omega").unwrap();
        library.delete(&listed[0]).unwrap();
        library.add(b"n", "Newest").unwrap();
        let listed = library.list().unwrap();
        assert_eq!(
            names(&listed),
            ["Omega", "Alpha", "copied a", "Copied B", "Newest"]
        );
        // The macOS app's format, read the same by a fresh library.
        let order: Value =
            serde_json::from_slice(&std::fs::read(folder.0.join(".order.json")).unwrap()).unwrap();
        assert_eq!(
            order,
            json!({"order": ["Omega.json", "Alpha.json", "copied a.json", "Copied B.json", "Newest.json"]})
        );
        assert_eq!(
            names(&Library::new(folder.0.clone()).list().unwrap()),
            names(&listed)
        );
    }

    #[test]
    fn files_changed_on_disk_are_left_alone() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"original", "Gold").unwrap();
        // Edited by hand after it was listed.
        std::fs::write(&preset.path, b"edited elsewhere").unwrap();
        let error = library.delete(&preset).unwrap_err();
        assert!(error.contains("changed on disk"), "{error}");
        assert!(library.rename(&preset, "Other").is_err());
        assert!(library.read(&preset).is_err());
        assert!(preset.path.exists());
        let current = library.list().unwrap().remove(0);
        library.delete(&current).unwrap();
        assert!(library.list().unwrap().is_empty());
    }

    #[test]
    fn renaming_to_another_case_never_replaces_a_file_in_that_case() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"mine", "Gold").unwrap();
        // Linux can hold both spellings; one copied in by hand.
        std::fs::write(folder.0.join("GOLD.json"), b"theirs").unwrap();
        assert!(
            library
                .rename(&preset, "GOLD")
                .unwrap_err()
                .contains("already exists")
        );
        assert_eq!(
            std::fs::read(folder.0.join("GOLD.json")).unwrap(),
            b"theirs"
        );
        assert_eq!(std::fs::read(&preset.path).unwrap(), b"mine");
    }

    #[test]
    fn imports_copy_the_bytes_and_go_where_dropped() {
        let folder = Folder::new();
        let library = Library::new(folder.0.join("presets"));
        for name in ["A", "B", "C"] {
            library.add(name.as_bytes(), name).unwrap();
        }
        let outside = folder.0.join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        let file = |name: &str, text: &str| {
            let path = outside.join(name);
            std::fs::write(&path, text).unwrap();
            path
        };
        let paths = [
            file("Velvia: 50.json", "{\"look\": 1}\n"),
            file("refused.json", "{\"refuse\": true}"),
            file("a.json", "{\"look\": 2}"),
        ];
        std::fs::write(outside.join("binary.json"), [0xff, 0xfe]).unwrap();
        let check = |text: &str| {
            if text.contains("refuse") {
                Err("Not a preset.".to_owned())
            } else {
                Ok(())
            }
        };
        // Dropped between A and B.
        let (added, failures) = library.import(
            &[paths.to_vec(), vec![outside.join("binary.json")]].concat(),
            Some(1),
            check,
        );
        assert_eq!(added.len(), 2);
        assert_eq!(
            failures,
            [
                "refused.json: Not a preset.",
                "binary.json: The file isn’t text."
            ]
        );
        assert_eq!(
            names(&library.list().unwrap()),
            ["A", "Velvia 50", "a 2", "B", "C"]
        );
        assert_eq!(std::fs::read(&added[0]).unwrap(), b"{\"look\": 1}\n");
        // From the Import dialog: at the end.
        let (added, _) = library.import(&paths[..1], None, check);
        assert_eq!(added[0].file_name().unwrap(), "Velvia 50 2.json");
        assert_eq!(
            names(&library.list().unwrap()).last().unwrap(),
            "Velvia 50 2"
        );
    }

    #[test]
    fn verdicts_are_kept_while_the_file_is_unchanged() {
        let folder = Folder::new();
        let library = Library::new(folder.0.clone());
        let preset = library.add(b"{}", "Gold").unwrap();
        let mut verdicts = Verdicts::default();
        verdicts.insert(&preset, Status::Invalid("no".into()));
        assert_eq!(verdicts.get(&preset), Some(&Status::Invalid("no".into())));
        std::fs::write(&preset.path, b"{\"changed\": 1}").unwrap();
        assert_eq!(verdicts.get(&library.list().unwrap()[0]), None);
    }

    /// A helper connection for the live tests.
    struct Helper {
        child: std::process::Child,
        input: std::process::ChildStdin,
        output: std::io::BufReader<std::process::ChildStdout>,
        id: u64,
    }
    impl Helper {
        fn start() -> Self {
            let helper = std::env::var_os("NEGS_SCHEMA_HELPER")
                .expect("Set NEGS_SCHEMA_HELPER to the built spektrafilm-native executable");
            let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../data");
            let mut child = std::process::Command::new(helper)
                .arg("--data-dir")
                .arg(data)
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .expect("Start the native helper");
            let input = child.stdin.take().unwrap();
            let output = std::io::BufReader::new(child.stdout.take().unwrap());
            Self {
                child,
                input,
                output,
                id: 0,
            }
        }
        fn request(&mut self, mut command: Value) -> Result<Value> {
            use std::io::{BufRead, Write};
            self.id += 1;
            command["id"] = json!(self.id);
            writeln!(self.input, "{command}").unwrap();
            let mut line = String::new();
            self.output.read_line(&mut line).unwrap();
            let reply: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(reply["id"], self.id);
            match reply["error"].as_str() {
                Some(error) => Err(error.into()),
                None => Ok(reply["result"].clone()),
            }
        }
        /// The live catalog installed, and its defaults.
        fn catalog(&mut self) -> Value {
            let catalog = self.request(json!({"command": "catalog"})).unwrap();
            assert_eq!(catalog["capabilities"]["presets"], true);
            crate::catalog::install(&catalog).unwrap();
            parameters::set_nullable_paths(&catalog);
            catalog["defaults"].clone()
        }
    }
    impl Drop for Helper {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    /// The first difference between two params, comparing numbers by value
    /// (the controls write 2.0 as 2).
    fn difference(a: &Value, b: &Value, path: &str) -> Option<String> {
        match (a, b) {
            (Value::Number(x), Value::Number(y)) if x.as_f64() == y.as_f64() => None,
            (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
                x.iter().find_map(|(k, v)| {
                    let child = if path.is_empty() {
                        k.clone()
                    } else {
                        format!("{path}.{k}")
                    };
                    y.get(k)
                        .map_or(Some(child.clone()), |w| difference(v, w, &child))
                })
            }
            (Value::Array(x), Value::Array(y)) if x.len() == y.len() => x
                .iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (v, w))| difference(v, w, &format!("{path}.{i}"))),
            _ if a == b => None,
            _ => Some(format!("{path}: {a} ≠ {b}")),
        }
    }

    /// A photograph's state with settings a preset never changes.
    fn photograph(defaults: &Value) -> State {
        let mut state = crate::fixtures::state();
        parameters::install_defaults(&mut state, defaults, true);
        state.params = defaults.clone();
        state.rotation = 1;
        state.crop = Some([0.1, 0.1, 0.8, 0.8]);
        state.border.enabled = true;
        state.values.insert("Image Size.Longest side".into(), 3000.);
        state.choices.insert("Develop at".into(), "1".into());
        state.values.insert("Post.Contrast".into(), 20.);
        state.values.insert("Camera.Exposure".into(), 1.5);
        for (key, value) in [
            ("CAT16 adaptation", "false"),
            ("Encode output as sRGB", "false"),
            ("Use less memory for export", "true"),
            ("Limit preview to 12 MP", "true"),
        ] {
            state.choices.insert(key.into(), value.into());
        }
        state
    }

    /// Preset files: one written here, and the local test folder's when present
    /// (`NEGS_PRESETS_TEST_DIR`, else `~/Work/negs-presets-test`). The community
    /// files there are unlicensed and never part of the repository.
    fn test_presets() -> Vec<(String, String)> {
        let mut files = vec![(
            "Double-X".to_owned(),
            r#"{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"},
                "film_chemistry": {"development_time": 8.0}, "couplers": {"langmuir_donor_k_rgb": [Infinity, 1.0, 1.0]},
                "camera": {"exposure_compensation_ev": -0.5}, "load_raw": {"white_balance": "daylight"}}"#
                .to_owned(),
        )];
        let folder = std::env::var_os("NEGS_PRESETS_TEST_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .map(|home| PathBuf::from(home).join("Work/negs-presets-test"))
            });
        let mut found: Vec<_> = folder
            .and_then(|folder| std::fs::read_dir(folder).ok())
            .into_iter()
            .flatten()
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|e| e == "json"))
            .collect();
        found.sort();
        if found.is_empty() {
            eprintln!("No local test presets; only the generated one is checked.");
        }
        for path in found {
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            files.push((name, text(read(&path).unwrap()).unwrap()));
        }
        files
    }

    #[test]
    #[ignore = "Requires a built native helper (NEGS_SCHEMA_HELPER)"]
    fn live_presets_apply_through_the_controls() {
        let mut helper = Helper::start();
        let defaults = helper.catalog();
        let before = photograph(&defaults);
        let films = crate::catalog::stocks(false);
        let presets = test_presets();
        for (name, text) in &presets {
            let result = helper
                .request(json!({"command": "preset_import", "text": text, "params": parameters::runtime(&before, &defaults)}))
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let (film, paper) = (
                result["film"].as_str().unwrap(),
                result["paper"].as_str().unwrap(),
            );
            let mut applied = before.clone();
            apply(&mut applied, film, paper, &result["params"])
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            // The adapter's invariant: the controls give back the helper's params.
            let runtime = parameters::runtime(&applied, &defaults);
            assert_eq!(difference(&runtime, &result["params"], ""), None, "{name}");
            assert_eq!(crate::catalog::selected(&applied, false).id, film, "{name}");
            assert_eq!(crate::catalog::selected(&applied, true).id, paper, "{name}");

            // The photograph's own settings stay.
            assert_eq!(
                (applied.rotation, applied.crop, &applied.border),
                (before.rotation, before.crop, &before.border)
            );
            for key in ["Image Size.Longest side", "Post.Contrast"] {
                assert_eq!(applied.values[key], before.values[key], "{name}: {key}");
            }
            for key in [
                "Develop at",
                "CAT16 adaptation",
                "Encode output as sRGB",
                "Use less memory for export",
                "Limit preview to 12 MP",
            ] {
                assert_eq!(applied.choices[key], before.choices[key], "{name}: {key}");
            }

            // One undo step, named after the preset.
            let mut history = crate::model::History::new(before.clone());
            history.begin(&format!("Apply Preset “{name}”"));
            history.state = applied.clone();
            history.commit();
            assert_eq!(
                history.undo().as_deref(),
                Some(format!("Apply Preset “{name}”").as_str())
            );
            assert_eq!(history.state, before);
            history.redo();
            assert_eq!(history.state, applied);

            // A session saved now restores the same params.
            let session = serde_json::to_value(
                json!({"schema": parameters::SESSION_SCHEMA, "state": applied}),
            )
            .unwrap();
            let mut restored: State = serde_json::from_value(session["state"].clone()).unwrap();
            parameters::migrate_session(session["schema"].as_u64().unwrap(), &mut restored);
            restored.sanitize();
            restored.params = parameters::merge(&defaults, &restored.params);
            let after_restart = parameters::runtime(&restored, &defaults);
            assert_eq!(
                difference(&after_restart, &result["params"], ""),
                None,
                "{name}: restored"
            );

            // Film A → preset → B → A is the same as choosing A after the
            // preset: the film brings its own stock presets, development time,
            // scan mode and target paper; the preset's other settings stay.
            let a = films
                .iter()
                .find(|s| s.id == crate::catalog::selected(&before, false).id)
                .unwrap();
            let b = films.iter().find(|s| s.id != a.id && s.id != film).unwrap();
            let mut there_and_back = applied.clone();
            crate::catalog::choose(&mut there_and_back, b);
            crate::catalog::choose(&mut there_and_back, a);
            let mut direct = applied.clone();
            crate::catalog::choose(&mut direct, a);
            assert_eq!(there_and_back, direct, "{name}");
            let mut expected = result["params"].clone();
            for path in parameters::STOCK_PRESET_PATHS {
                parameters::set(&mut expected, path, Value::Null);
            }
            parameters::set(&mut expected, "film_render.development_time", Value::Null);
            if !a.target.is_empty() {
                parameters::set(&mut expected, "print_render.development_time", Value::Null);
            }
            parameters::set(&mut expected, "io.scan_film", json!(a.positive));
            let back = parameters::runtime(&there_and_back, &defaults);
            assert_eq!(
                difference(&back, &expected, ""),
                None,
                "{name}: back to {}",
                a.id
            );
        }
        eprintln!("{} presets applied", presets.len());
    }

    #[test]
    #[ignore = "Requires a built native helper (NEGS_SCHEMA_HELPER)"]
    fn live_presets_export_and_refuse() {
        let mut helper = Helper::start();
        let defaults = helper.catalog();
        let mut state = photograph(&defaults);
        // New Preset's text reads back as the same settings.
        let film = crate::catalog::stocks(false)
            .into_iter()
            .find(|s| s.id == "kodak_portra_400")
            .unwrap();
        crate::catalog::choose(&mut state, film);
        state.values.insert("Enlarger.Magenta shift".into(), 3.5);
        let params = parameters::runtime(&state, &defaults);
        let paper = crate::catalog::selected(&state, true).id.clone();
        let exported = helper
            .request(json!({"command": "preset_export", "film": film.id, "paper": paper, "params": params}))
            .unwrap();
        let text = exported["text"].as_str().unwrap();
        assert_eq!(
            status(helper.request(json!({"command": "preset_import", "text": text}))),
            Status::Valid {
                film: film.id.clone(),
                paper: paper.clone(),
                scan: false
            }
        );
        let result = helper
            .request(json!({"command": "preset_import", "text": text, "params": params}))
            .unwrap();
        let mut applied = state.clone();
        apply(&mut applied, &film.id, &paper, &result["params"]).unwrap();
        assert_eq!(applied.values["Enlarger.Magenta shift"], 3.5);
        assert_eq!(
            applied.values["Camera.Exposure"],
            state.values["Camera.Exposure"]
        );

        // Files the helper refuses carry the reason.
        let Status::Invalid(reason) = status(helper.request(json!({"command": "preset_import", "text": r#"{"output_gamut_compress": {"algorithm": "jzazbz"}}"#}))) else {
            panic!("Accepted jzazbz");
        };
        assert!(reason.contains("jzazbz"), "{reason}");
        let Status::Invalid(reason) = status(helper.request(json!({"command": "preset_import", "text": r#"{"simulation": {"film_stock": "kodak_unknown"}}"#}))) else {
            panic!("Accepted an unknown film");
        };
        assert!(reason.contains("kodak_unknown"), "{reason}");
        // A stock this app doesn't list is refused before anything changes.
        let mut unchanged = state.clone();
        assert!(apply(&mut unchanged, "kodak_unknown", &paper, &result["params"]).is_err());
        assert_eq!(unchanged, state);
    }
}
