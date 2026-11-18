//! Open in Lightroom (ticket 48, the Mac's `Lightroom.swift`): photographs
//! developed to 16-bit TIFFs by the batch exporter, then handed to
//! Lightroom in one launch, so it asks once ("Add 12 Photos").
//!
//! The cloud Lightroom copies what it adds into its own library, so its
//! TIFFs wait in Negs' cache folder and are deleted once Lightroom has its
//! copy, or after a week. Lightroom Classic adds files where they are, so
//! its TIFFs go to a folder in Pictures and are never deleted by Negs.
//!
//! On Windows both come from Adobe's Creative Cloud installer: they are
//! found from their uninstall entries (`DisplayName` "Adobe Lightroom" and
//! "Adobe Lightroom Classic", `InstallLocation`), then App Paths, then the
//! default folders under Program Files. Files are handed over as the
//! executable's arguments, as Explorer's Open With does.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Which Lightroom a send is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cloud,
    Classic,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Cloud, Kind::Classic];
    /// As a batch's journal keeps it (`batch::Options::lightroom`).
    pub fn key(self) -> &'static str {
        match self {
            Kind::Cloud => "cloud",
            Kind::Classic => "classic",
        }
    }
    pub fn from_key(key: &str) -> Kind {
        if key == "classic" { Kind::Classic } else { Kind::Cloud }
    }
    pub fn title(self) -> &'static str {
        crate::batch::lightroom_title(self.key())
    }
    /// Where its TIFFs are written.
    pub fn folder(self) -> PathBuf {
        match self {
            Kind::Cloud => crate::session::cache_dir().join("Lightroom"),
            Kind::Classic => pictures().join("Negs for Lightroom"),
        }
    }
}

/// The Lightroom executables installed, as found after launch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Apps {
    pub cloud: Option<PathBuf>,
    pub classic: Option<PathBuf>,
}

impl Apps {
    pub fn app(&self, kind: Kind) -> Option<&Path> {
        match kind {
            Kind::Cloud => self.cloud.as_deref(),
            Kind::Classic => self.classic.as_deref(),
        }
    }
    /// The items to offer, in menu order: those installed.
    pub fn kinds(&self) -> Vec<Kind> {
        Kind::ALL.into_iter().filter(|k| self.app(*k).is_some()).collect()
    }
}

/// `IMG_0412-Negs.tif`, as the batch's custom naming makes it.
pub const NAMING: &str = "{name}-Negs";
/// A cloud TIFF Lightroom never copied (Cancel, or a library elsewhere) is deleted after this.
pub const KEEP: Duration = Duration::from_secs(7 * 24 * 3600);

/// "Open in Lightroom", or "Open 12 Photographs in Lightroom Classic".
pub fn menu_title(kind: Kind, count: usize) -> String {
    if count > 1 { format!("Open {count} Photographs in {}", kind.title()) } else { format!("Open in {}", kind.title()) }
}

/// The user's Pictures folder.
pub fn pictures() -> PathBuf {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Com::CoTaskMemFree;
        use windows::Win32::UI::Shell::{FOLDERID_Pictures, KF_FLAG_DEFAULT, SHGetKnownFolderPath};
        if let Ok(path) = SHGetKnownFolderPath(&FOLDERID_Pictures, KF_FLAG_DEFAULT, None) {
            let found = path.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(path.0 as *const _));
            if let Some(found) = found {
                return found;
            }
        }
    }
    PathBuf::from(std::env::var_os("USERPROFILE").unwrap_or_default()).join("Pictures")
}

// MARK: Detection

/// Finds Lightroom and Lightroom Classic. A few registry reads: never on
/// the launch path.
pub fn detect() -> Apps {
    let mut apps = Apps::default();
    for (name, location, icon) in uninstall_entries() {
        let Some(kind) = kind_of(&name) else { continue };
        let exe = [location.map(|l| PathBuf::from(l).join("Lightroom.exe")), icon.map(|i| PathBuf::from(i.split(',').next().unwrap_or("").trim_matches('"')))]
            .into_iter()
            .flatten()
            .find(|p| p.file_name().is_some_and(|n| n.eq_ignore_ascii_case("lightroom.exe")) && p.is_file());
        let slot = if kind == Kind::Cloud { &mut apps.cloud } else { &mut apps.classic };
        if slot.is_none() {
            *slot = exe;
        }
    }
    let program_files = PathBuf::from(std::env::var_os("ProgramFiles").unwrap_or_else(|| "C:\\Program Files".into()));
    for (kind, folder) in [(Kind::Cloud, "Adobe Lightroom CC"), (Kind::Cloud, "Adobe Lightroom"), (Kind::Classic, "Adobe Lightroom Classic")] {
        let exe = program_files.join("Adobe").join(folder).join("Lightroom.exe");
        let slot = if kind == Kind::Cloud { &mut apps.cloud } else { &mut apps.classic };
        if slot.is_none() && exe.is_file() {
            *slot = Some(exe);
        }
    }
    if apps.cloud.is_none() && apps.classic.is_none() {
        // App Paths names one `lightroom.exe`: which one it is, by its folder.
        if let Some(exe) = app_path("lightroom.exe").filter(|p| p.is_file()) {
            let classic = exe.to_string_lossy().to_lowercase().contains("classic");
            if classic { apps.classic = Some(exe) } else { apps.cloud = Some(exe) }
        }
    }
    apps
}

/// Which Lightroom an uninstall entry's name is, if either.
pub fn kind_of(display_name: &str) -> Option<Kind> {
    let name = display_name.trim().to_lowercase();
    if !name.starts_with("adobe lightroom") && !name.starts_with("lightroom") {
        return None;
    }
    Some(if name.contains("classic") { Kind::Classic } else { Kind::Cloud })
}

#[cfg(windows)]
fn uninstall_entries() -> Vec<(String, Option<String>, Option<String>)> {
    use windows::Win32::System::Registry::*;
    let mut found = vec![];
    for (root, path) in [
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_LOCAL_MACHINE, "SOFTWARE\\WOW6432Node\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
        (HKEY_CURRENT_USER, "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall"),
    ] {
        let Some(key) = open_key(root, path) else { continue };
        let mut index = 0;
        loop {
            let mut name = [0u16; 256];
            let mut length = name.len() as u32;
            let status = unsafe { RegEnumKeyExW(key, index, windows::core::PWSTR(name.as_mut_ptr()), &mut length, None, windows::core::PWSTR::null(), None, None) };
            if status.is_err() {
                break;
            }
            index += 1;
            let sub = String::from_utf16_lossy(&name[..length as usize]);
            let Some(entry) = open_key(key, &sub) else { continue };
            if let Some(display) = string_value(entry, "DisplayName") {
                found.push((display, string_value(entry, "InstallLocation"), string_value(entry, "DisplayIcon")));
            }
            unsafe {
                let _ = RegCloseKey(entry);
            }
        }
        unsafe {
            let _ = RegCloseKey(key);
        }
    }
    found
}

#[cfg(windows)]
fn open_key(root: windows::Win32::System::Registry::HKEY, path: &str) -> Option<windows::Win32::System::Registry::HKEY> {
    use windows::Win32::System::Registry::*;
    let wide = crate::platform::wide(path);
    let mut key = HKEY::default();
    unsafe { RegOpenKeyExW(root, windows::core::PCWSTR(wide.as_ptr()), 0, KEY_READ, &mut key) }.is_ok().then_some(key)
}

#[cfg(windows)]
fn string_value(key: windows::Win32::System::Registry::HKEY, name: &str) -> Option<String> {
    use windows::Win32::System::Registry::*;
    let wide = crate::platform::wide(name);
    let mut size = 0u32;
    unsafe {
        RegGetValueW(key, None, windows::core::PCWSTR(wide.as_ptr()), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, None, None, Some(&mut size)).ok().ok()?;
        let mut buffer = vec![0u16; (size as usize).div_ceil(2) + 1];
        RegGetValueW(key, None, windows::core::PCWSTR(wide.as_ptr()), RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ, None, Some(buffer.as_mut_ptr().cast()), Some(&mut size)).ok().ok()?;
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        let text = String::from_utf16_lossy(&buffer[..end]);
        (!text.trim().is_empty()).then_some(text)
    }
}

#[cfg(windows)]
fn app_path(exe: &str) -> Option<PathBuf> {
    use windows::Win32::System::Registry::*;
    for root in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        if let Some(key) = open_key(root, &format!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\App Paths\\{exe}")) {
            let path = string_value(key, "");
            unsafe {
                let _ = RegCloseKey(key);
            }
            if let Some(path) = path {
                return Some(PathBuf::from(path.trim_matches('"')));
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn uninstall_entries() -> Vec<(String, Option<String>, Option<String>)> {
    vec![]
}

#[cfg(not(windows))]
fn app_path(_: &str) -> Option<PathBuf> {
    None
}

// MARK: Lightroom's library

/// Each catalog's `originals` folder in every `*.lrlibrary` in Pictures:
/// `Lightroom Library.lrlibrary/<catalog id>/originals`. Internal and
/// undocumented, so only a hint for cleanup.
pub fn libraries(pictures: &Path) -> Vec<PathBuf> {
    let Ok(bundles) = std::fs::read_dir(pictures) else { return vec![] };
    bundles
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("lrlibrary")))
        .flat_map(|bundle| std::fs::read_dir(bundle).into_iter().flatten().flatten().map(|e| e.path().join("originals")).filter(|o| o.is_dir()).collect::<Vec<_>>())
        .collect()
}

/// A capture date: year, month, day.
pub type Day = (i32, u32, u32);

/// The folders under `originals` a file captured on `capture` lands in:
/// `2026/2026-11-15`, and the day either side for time zones. A file
/// without a capture date goes to `0000/0000-00-00`.
pub fn date_folders(capture: Option<Day>) -> Vec<String> {
    let Some((year, month, day)) = capture else { return vec!["0000/0000-00-00".into()] };
    let days = days_from_civil(year, month, day);
    [0, -1, 1]
        .into_iter()
        .map(|offset| {
            let (y, m, d) = civil_from_days(days + offset);
            format!("{y:04}/{y:04}-{m:02}-{d:02}")
        })
        .collect()
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = i64::from(y) - i64::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (i64::from(m) + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> Day {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((yoe + era * 400 + i64::from(m <= 2)) as i32, m, d)
}

/// `2026:11:15 14:03:22` → 2026-11-15.
pub fn exif_date(text: &str) -> Option<Day> {
    let parts: Vec<i64> = text.get(..10)?.split(':').filter_map(|p| p.parse().ok()).collect();
    match parts[..] {
        [y, m, d] if y > 0 && (1..=12).contains(&m) && (1..=31).contains(&d) => Some((y as i32, m as u32, d as u32)),
        _ => None,
    }
}

/// The EXIF capture date of a TIFF as written (DateTimeOriginal, then
/// DateTimeDigitized, then the TIFF's DateTime), without a time zone.
pub fn capture_date(file: &Path) -> Option<Day> {
    use std::io::{Read, Seek, SeekFrom};
    let f = std::cell::RefCell::new(std::fs::File::open(file).ok()?);
    let read = |at: u64, n: usize| -> Option<Vec<u8>> {
        let mut f = f.borrow_mut();
        f.seek(SeekFrom::Start(at)).ok()?;
        let mut buffer = vec![0; n];
        f.read_exact(&mut buffer).ok()?;
        Some(buffer)
    };
    let head = read(0, 8)?;
    let little = match &head[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |b: &[u8]| if little { u16::from_le_bytes([b[0], b[1]]) } else { u16::from_be_bytes([b[0], b[1]]) };
    let u32_at = |b: &[u8]| if little { u32::from_le_bytes([b[0], b[1], b[2], b[3]]) } else { u32::from_be_bytes([b[0], b[1], b[2], b[3]]) };
    if u16_at(&head[2..4]) != 42 {
        return None;
    }
    // (tag, count, value or offset) of one IFD.
    let entries = |offset: u32| -> Option<Vec<(u16, u32, u32)>> {
        let count = u16_at(&read(offset as u64, 2)?) as usize;
        let table = read(offset as u64 + 2, count.min(4096) * 12)?;
        Some(table.chunks(12).map(|e| (u16_at(&e[0..2]), u32_at(&e[4..8]), u32_at(&e[8..12]))).collect())
    };
    let ifd0 = entries(u32_at(&head[4..8]))?;
    let ascii = |entry: Option<&(u16, u32, u32)>| -> Option<Day> {
        let &(_, count, offset) = entry?;
        if !(10..=64).contains(&count) {
            return None;
        }
        let bytes = read(offset as u64, count as usize)?;
        exif_date(std::str::from_utf8(&bytes).ok()?.trim_end_matches('\0'))
    };
    let exif = ifd0.iter().find(|e| e.0 == 0x8769).and_then(|e| entries(e.2));
    if let Some(exif) = &exif {
        for tag in [0x9003, 0x9004] {
            if let Some(day) = ascii(exif.iter().find(|e| e.0 == tag)) {
                return Some(day);
            }
        }
    }
    ascii(ifd0.iter().find(|e| e.0 == 0x0132))
}

/// Whether Lightroom has its copy: the same name and size in one of the
/// capture date's folders of any library.
pub fn copied(name: &str, size: u64, capture: Option<Day>, libraries: &[PathBuf]) -> bool {
    libraries.iter().any(|library| date_folders(capture).iter().any(|folder| std::fs::metadata(library.join(folder).join(name)).is_ok_and(|m| m.len() == size)))
}

/// A cloud TIFF gets a name its capture-date folders in Lightroom's
/// library don't have yet, so finding that name there later means
/// Lightroom copied this file (and a photograph sent again is a new one).
pub fn unique_in_library(file: &Path, libraries: &[PathBuf]) -> PathBuf {
    let capture = capture_date(file);
    let folders: Vec<PathBuf> = libraries.iter().flat_map(|l| date_folders(capture).into_iter().map(move |f| l.join(f))).collect();
    let Some(directory) = file.parent() else { return file.to_path_buf() };
    let own = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let name = crate::batch::numbered(&own, |candidate| (candidate != own && directory.join(candidate).exists()) || folders.iter().any(|f| f.join(candidate).exists()));
    if name == own {
        return file.to_path_buf();
    }
    let renamed = directory.join(name);
    if std::fs::rename(file, &renamed).is_ok() { renamed } else { file.to_path_buf() }
}

// MARK: Cleanup

/// What a cleanup pass found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Cleanup {
    /// Deleted because Lightroom has a copy, or because they're older than a week.
    pub copied: usize,
    pub expired: usize,
    /// Still waiting for Lightroom.
    pub waiting: usize,
}

/// Deletes the cloud folder's TIFFs that Lightroom has copied, and those
/// older than a week. Off the UI thread; cheap when nothing waits.
pub fn clean_up(folder: &Path, libraries: Option<&[PathBuf]>, now: SystemTime) -> Cleanup {
    let mut result = Cleanup::default();
    let Ok(files) = std::fs::read_dir(folder) else { return result };
    let files: Vec<PathBuf> = files.flatten().map(|e| e.path()).filter(|p| p.is_file() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.'))).collect();
    if files.is_empty() {
        return result;
    }
    let found;
    let libraries = match libraries {
        Some(l) => l,
        None => {
            found = self::libraries(&pictures());
            &found
        }
    };
    for file in files {
        let Ok(meta) = std::fs::metadata(&file) else { continue };
        let old = meta.modified().ok().and_then(|m| now.duration_since(m).ok()).is_some_and(|age| age > KEEP);
        let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if old {
            if std::fs::remove_file(&file).is_ok() {
                result.expired += 1;
            }
        } else if copied(&name, meta.len(), capture_date(&file), libraries) {
            if std::fs::remove_file(&file).is_ok() {
                result.copied += 1;
            }
        } else {
            result.waiting += 1;
        }
    }
    result
}

/// The bytes the files in `folder` take; None if it's missing.
pub fn size_of(folder: &Path) -> Option<u64> {
    let files = std::fs::read_dir(folder).ok()?;
    Some(files.flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum())
}

/// Deletes the TIFFs in `folder` (only those Negs makes), keeping the folder.
pub fn delete_sent(folder: &Path) {
    for file in std::fs::read_dir(folder).into_iter().flatten().flatten().map(|e| e.path()) {
        if file.extension().is_some_and(|e| e.eq_ignore_ascii_case("tif") || e.eq_ignore_ascii_case("tiff")) {
            let _ = std::fs::remove_file(file);
        }
    }
}

// MARK: Opening

/// Hands `files` to Lightroom at `app` in one launch, so it asks once. A
/// Lightroom that isn't running is started first and given its window and
/// 3 s more, as the Mac does (a Lightroom starting up dropped files handed
/// to it on the Mac); `starting` says so. Blocks: run it off the UI thread.
#[cfg(windows)]
pub fn open(files: &[PathBuf], app: &Path, starting: impl FnOnce()) -> anyhow::Result<()> {
    use std::os::windows::process::CommandExt;
    if !has_main_window(app) {
        starting();
        std::process::Command::new(app).spawn().map_err(|e| anyhow::anyhow!("{e}"))?;
        let start = std::time::Instant::now();
        while !has_main_window(app) && start.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(200));
        }
        std::thread::sleep(Duration::from_secs(3));
    }
    // Lightroom's own quoting: each path in quotes, as Explorer passes them.
    let arguments = files.iter().map(|f| format!("\"{}\"", f.display())).collect::<Vec<_>>().join(" ");
    std::process::Command::new(app).raw_arg(arguments).spawn().map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

/// A visible top-level window over 300 px tall of a process running `app`.
#[cfg(windows)]
pub fn has_main_window(app: &Path) -> bool {
    use windows::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, RECT};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible};
    struct Search {
        app: PathBuf,
        found: bool,
    }
    extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = unsafe { &mut *(data.0 as *mut Search) };
        unsafe {
            let mut r = RECT::default();
            if !IsWindowVisible(hwnd).as_bool() || GetWindowRect(hwnd, &mut r).is_err() || r.bottom - r.top <= 300 {
                return BOOL(1);
            }
            let mut pid = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut name = [0u16; 1024];
                let mut length = name.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(name.as_mut_ptr()), &mut length).is_ok() {
                    let path = PathBuf::from(String::from_utf16_lossy(&name[..length as usize]));
                    if crate::platform::same_file(&path, &search.app) {
                        search.found = true;
                    }
                }
                let _ = CloseHandle(process);
            }
        }
        BOOL(if search.found { 0 } else { 1 })
    }
    let mut search = Search { app: app.to_path_buf(), found: false };
    unsafe {
        let _ = EnumWindows(Some(each), LPARAM(&mut search as *mut _ as isize));
    }
    search.found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_menu_titles() {
        assert_eq!(kind_of("Adobe Lightroom Classic"), Some(Kind::Classic));
        assert_eq!(kind_of("Adobe Lightroom"), Some(Kind::Cloud));
        assert_eq!(kind_of("Adobe Lightroom CC"), Some(Kind::Cloud));
        assert_eq!(kind_of("Adobe Photoshop 2026"), None);
        assert_eq!(menu_title(Kind::Cloud, 1), "Open in Lightroom");
        assert_eq!(menu_title(Kind::Classic, 12), "Open 12 Photographs in Lightroom Classic");
        assert_eq!(Kind::from_key(Kind::Classic.key()), Kind::Classic);
        let apps = Apps { cloud: None, classic: Some(PathBuf::from("x")) };
        assert_eq!(apps.kinds(), [Kind::Classic]);
        assert!(Apps::default().kinds().is_empty());
    }

    #[test]
    fn date_folders_hold_the_day_either_side() {
        assert_eq!(exif_date("2026:11:15 14:03:22"), Some((2026, 11, 15)));
        assert_eq!(exif_date("0000:00:00 00:00:00"), None);
        assert_eq!(exif_date("garbage"), None);
        assert_eq!(date_folders(Some((2026, 11, 15))), ["2026/2026-11-15", "2026/2026-11-14", "2026/2026-11-16"]);
        assert_eq!(date_folders(Some((2026, 1, 1))), ["2026/2026-01-01", "2025/2025-12-31", "2026/2026-01-02"]);
        assert_eq!(date_folders(Some((2024, 2, 28)))[2], "2024/2024-02-29");
        assert_eq!(date_folders(None), ["0000/0000-00-00"]);
    }

    /// The Mac's cleanup: a copied file goes, an old one goes, the rest wait;
    /// a name Lightroom's library has is numbered before it's sent.
    #[test]
    fn cleanup_deletes_what_lightroom_copied_and_what_is_old() {
        let dir = std::env::temp_dir().join(format!("negs-lightroom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (folder, library) = (dir.join("cache"), dir.join("Lightroom Library.lrlibrary").join("abc").join("originals"));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(library.join("0000/0000-00-00")).unwrap();
        std::fs::write(folder.join("a-Negs.tif"), b"aaaa").unwrap();
        std::fs::write(folder.join("b-Negs.tif"), b"bbbb").unwrap();
        std::fs::write(folder.join("c-Negs.tif"), b"cccc").unwrap();
        std::fs::write(library.join("0000/0000-00-00/a-Negs.tif"), b"aaaa").unwrap();
        // Same name, another size: not this file's copy.
        std::fs::write(library.join("0000/0000-00-00/b-Negs.tif"), b"bb").unwrap();
        assert_eq!(libraries(&dir), [library.clone()]);
        let renamed = unique_in_library(&folder.join("b-Negs.tif"), std::slice::from_ref(&library));
        assert_eq!(renamed.file_name().unwrap(), "b-Negs 2.tif");
        let later = SystemTime::now() + Duration::from_secs(3600);
        let cleanup = clean_up(&folder, Some(std::slice::from_ref(&library)), later);
        assert_eq!(cleanup, Cleanup { copied: 1, expired: 0, waiting: 2 });
        let week = SystemTime::now() + KEEP + Duration::from_secs(60);
        assert_eq!(clean_up(&folder, Some(&[]), week), Cleanup { copied: 0, expired: 2, waiting: 0 });
        // Settings › Lightroom: what's left, and Delete Sent Copies (TIFFs only).
        std::fs::write(folder.join("d-Negs.tif"), b"dddddd").unwrap();
        std::fs::write(folder.join("notes.txt"), b"nn").unwrap();
        assert_eq!(size_of(&folder), Some(8));
        delete_sent(&folder);
        assert_eq!(size_of(&folder), Some(2));
        assert_eq!(size_of(&dir.join("missing")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
