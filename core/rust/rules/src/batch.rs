//! windows/src/batch.rs's rules on the shared core (core/src/caches/batch):
//! the names a batch gives, the matched exposure, the words of its status
//! line and popover, and its journal on disk.
//!
//! windows/src/batch.rs keeps its own types (Options, Item, Journal, Run):
//! they hold windows/src's EditState and model types, which Phase 4a and 5
//! move. They call these functions with plain data. Naming, Existing and
//! Status are here, their keys and titles the core's.
//!
//! Kept in Rust: `numbered`'s loop (it asks the host's `taken` for each
//! candidate; the core gives the candidates), the matched state's
//! `brightness_match` fields (Rust's own struct), and the journal's serde
//! text, which crosses as `serde_json::to_vec(&journal)`.

use std::path::Path;

use negs_core_sys::caches as sys;
use negs_core_sys::{negs_buf_t, take_buf};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::crop::static_str;

fn empty() -> negs_buf_t {
    negs_buf_t { data: std::ptr::null_mut(), len: 0 }
}

fn string(buf: negs_buf_t) -> String {
    String::from_utf8(take_buf(buf)).expect("the core writes UTF-8")
}

/// A path's bytes for the core: Unix's own, Windows' WTF-8 (UTF-8 for
/// every path Windows itself names).
pub(crate) fn path_bytes(path: &Path) -> &[u8] {
    path.as_os_str().as_encoded_bytes()
}

fn key_of(table: u32, index: usize) -> &'static str {
    static_str(unsafe { sys::negs_batch_key(table, index as u32) })
}

fn from_key(table: u32, key: &str) -> usize {
    unsafe { sys::negs_batch_from_key(table, key.as_ptr(), key.len()) as usize }
}

/// A batch file's name: `{name}_{film}` as a single export suggests,
/// `{name}`, or the user's own text with those tokens.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Naming {
    #[default]
    NameFilm,
    Name,
    Custom,
}

impl Naming {
    pub const ALL: [Naming; 3] = [Naming::NameFilm, Naming::Name, Naming::Custom];
    pub fn title(self) -> &'static str {
        key_of(1, self as usize)
    }
    /// As the Mac stores it (`batchNaming`).
    pub fn key(self) -> &'static str {
        key_of(0, self as usize)
    }
    pub fn from_key(key: &str) -> Naming {
        Self::ALL[from_key(0, key).min(2)]
    }
}

/// What a batch does when a file of its name is already there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Existing {
    #[default]
    Number,
    Replace,
    Skip,
}

impl Existing {
    pub const ALL: [Existing; 3] = [Existing::Number, Existing::Replace, Existing::Skip];
    pub fn title(self) -> &'static str {
        key_of(3, self as usize)
    }
    pub fn key(self) -> &'static str {
        key_of(2, self as usize)
    }
    pub fn from_key(key: &str) -> Existing {
        Self::ALL[from_key(2, key).min(2)]
    }
}

/// How a photograph of a batch went.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    #[default]
    Waiting,
    Done,
    Failed,
    Skipped,
}

/// SaveSettings' three numbers (`Options::save`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Save {
    pub bit_depth: u32,
    pub color_space: usize,
    pub jpeg_quality: u32,
}

/// `Options::save`: JPEG is 8 bits; ProPhoto at 8 bits is Adobe RGB.
pub fn save(format: &str, bit_depth: u32, color_space: usize, jpeg_quality: u32) -> Save {
    let mut out = sys::negs_batch_save_t::default();
    unsafe { sys::negs_batch_options_save(format.as_ptr(), format.len(), bit_depth, color_space as u64, jpeg_quality, &mut out) };
    Save { bit_depth: out.bit_depth, color_space: out.color_space as usize, jpeg_quality: out.jpeg_quality }
}

/// `Options::default()` as serde_json writes it.
pub fn options_default_json() -> Vec<u8> {
    take_buf(unsafe { sys::negs_batch_options_default() })
}

/// `Options::file_name`: `format` the output format's key.
pub fn file_name(naming: Naming, custom: &str, name: &str, film: &str, format: &str) -> String {
    string(unsafe {
        sys::negs_batch_file_name(
            naming as u32,
            custom.as_ptr(),
            custom.len(),
            name.as_ptr(),
            name.len(),
            film.as_ptr(),
            film.len(),
            format.as_ptr(),
            format.len(),
        )
    })
}

/// The name `{name}` stands for: the file's without its extension, a
/// duplicate's number after it (`IMG_0412 (2)`).
pub fn batch_name(photo: &Path, title: &str) -> String {
    let p = path_bytes(photo);
    string(unsafe { sys::negs_batch_name(p.as_ptr(), p.len(), title.as_ptr(), title.len()) })
}

/// `name.ext`, then `name 2.ext`, `name 3.ext`… until `taken` says no.
pub fn numbered(file_name: &str, taken: impl Fn(&str) -> bool) -> String {
    if !taken(file_name) {
        return file_name.to_string();
    }
    let mut n: u64 = 2;
    loop {
        let name = string(unsafe { sys::negs_batch_numbered_name(file_name.as_ptr(), file_name.len(), n) });
        if !taken(&name) {
            return name;
        }
        n += 1;
    }
}

/// The batch's own names made unique among themselves, in order, ignoring
/// case as Windows does (a RAW+JPEG pair would share one).
pub fn unique(names: &[String]) -> Vec<String> {
    let list = crate::json::strs(names);
    let mut out = empty();
    let ok = unsafe { sys::negs_batch_unique(list.as_ptr(), list.len(), &mut out) };
    let bytes = take_buf(out);
    assert!(ok == 1);
    serde_json::from_slice(&bytes).expect("the core writes a JSON array of strings")
}

/// `matched`'s exposure: the new total at PREP_EXPOSURE (`exposure` the
/// value there now, `old` the match's ev) when the match moved by 0.05 EV
/// or more; None when the values stay.
pub fn matched_exposure(exposure: &Value, old: Option<f64>, ev: f64) -> Option<Value> {
    let text = crate::json::text(exposure);
    let mut out = empty();
    let set = unsafe {
        sys::negs_batch_matched_exposure(text.as_ptr(), text.len(), old.is_some() as u8, old.unwrap_or(0.0), ev, &mut out)
    };
    let bytes = take_buf(out);
    (set == 1).then(|| crate::json::read(&bytes))
}

/// `matched` on a state's serde text: the matched state's, None without
/// the match on (or for text serde wouldn't read as an EditState).
pub fn matched_json(state: &[u8], ev: f64, limited: bool, inputs: &Value) -> Option<Vec<u8>> {
    let inputs = crate::json::text(inputs);
    let mut out = empty();
    let ok = unsafe {
        sys::negs_batch_matched(state.as_ptr(), state.len(), ev, limited as u8, inputs.as_ptr(), inputs.len(), &mut out)
    };
    let bytes = take_buf(out);
    (ok == 1).then_some(bytes)
}

// MARK: Run

/// A batch's counts as Run's methods give them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Counts {
    pub total: usize,
    pub remaining: usize,
    pub finished: usize,
    pub exported: usize,
    pub failed: usize,
    pub skipped: usize,
    pub fraction: f64,
    pub rule_parts: (f64, f64),
    pub needs_attention: bool,
}

/// A run's photographs by status, for the core.
pub struct RunView {
    statuses: Vec<u8>,
    running: bool,
    cancelled: bool,
}

impl RunView {
    pub fn new(statuses: impl IntoIterator<Item = Status>, running: bool, cancelled: bool) -> Self {
        Self { statuses: statuses.into_iter().map(|s| s as u8).collect(), running, cancelled }
    }

    fn raw(&self) -> sys::negs_batch_run_t {
        sys::negs_batch_run_t {
            statuses: self.statuses.as_ptr(),
            count: self.statuses.len(),
            running: self.running as u8,
            cancelled: self.cancelled as u8,
        }
    }

    pub fn counts(&self) -> Counts {
        let mut c = sys::negs_batch_counts_t::default();
        unsafe { sys::negs_batch_run_counts(&self.raw(), &mut c) };
        Counts {
            total: c.total as usize,
            remaining: c.remaining as usize,
            finished: c.finished as usize,
            exported: c.exported as usize,
            failed: c.failed as usize,
            skipped: c.skipped as usize,
            fraction: c.fraction,
            rule_parts: (c.exported_part, c.other_part),
            needs_attention: c.needs_attention != 0,
        }
    }

    /// The popover's title.
    pub fn title(&self) -> String {
        string(unsafe { sys::negs_batch_run_title(&self.raw()) })
    }

    /// The status line's and the grid foot's words while it runs;
    /// `lightroom` the batch's Lightroom key.
    pub fn progress_words(&self, lightroom: Option<&str>) -> String {
        let (key, has) = lightroom.map_or(("", 0), |k| (k, 1));
        string(unsafe { sys::negs_batch_run_progress(&self.raw(), key.as_ptr(), key.len(), has) })
    }

    /// "Exported 10 of 12 · cancelled", then "· 2 failed" in bold.
    pub fn outcome_words(&self) -> (String, Option<String>) {
        let (mut words, mut failed) = (empty(), empty());
        let has = unsafe { sys::negs_batch_run_outcome(&self.raw(), &mut words, &mut failed) };
        let failed = string(failed);
        (string(words), (has == 1).then_some(failed))
    }

    /// The status once it ends, naming the destination folder.
    pub fn finished_words(&self, destination: &Path) -> String {
        let d = path_bytes(destination);
        string(unsafe { sys::negs_batch_run_finished(&self.raw(), d.as_ptr(), d.len()) })
    }
}

/// Lightroom's name by the key a batch keeps (`Options::lightroom`).
pub fn lightroom_title(key: &str) -> &'static str {
    static_str(unsafe { sys::negs_batch_lightroom_title(key.as_ptr(), key.len()) })
}

/// "Exporting 5 of 12 photographs", or "Sending 1 of 2 photographs to Lightroom".
pub fn exporting(finished: usize, total: usize, lightroom: Option<&str>) -> String {
    let (title, has) = lightroom.map_or(("", 0), |t| (t, 1));
    string(unsafe { sys::negs_batch_exporting(finished as u64, total as u64, title.as_ptr(), title.len(), has) })
}

/// The dialog's message: `engine` an ExportEngine's index in its ALL, `edge`
/// an Edge's.
pub fn message(count: usize, engine: usize, edge: usize, pixels: u32, save: Save, preview_space: &str) -> String {
    let save = sys::negs_batch_save_t { bit_depth: save.bit_depth, color_space: save.color_space as u64, jpeg_quality: save.jpeg_quality };
    string(unsafe {
        sys::negs_batch_message(count as u64, engine as u32, edge as u32, pixels, &save, preview_space.as_ptr(), preview_space.len())
    })
}

// MARK: The journal

/// The journal serde reads from `text`, as serde_json writes it; None when
/// it isn't one.
pub fn journal_rewrite(text: &[u8]) -> Option<Vec<u8>> {
    let mut out = empty();
    let ok = unsafe { sys::negs_batch_journal_rewrite(text.as_ptr(), text.len(), &mut out) };
    let bytes = take_buf(out);
    (ok == 1).then_some(bytes)
}

/// `Journal::write` of `json` (`serde_json::to_vec(&journal)`): staged as
/// `{path}.json.tmp`-style beside it and moved over it. A journal the core
/// doesn't read as one (a NaN serde wrote as null) is written as the bytes
/// given, the same way.
pub fn journal_write(path: &Path, json: &[u8]) -> Result<(), String> {
    let p = path_bytes(path);
    let mut error = empty();
    let mut result = unsafe { sys::negs_batch_journal_write(p.as_ptr(), p.len(), json.as_ptr(), json.len(), &mut error) };
    if result == 2 {
        let _ = take_buf(error);
        error = empty();
        result = unsafe { sys::negs_batch_journal_write_bytes(p.as_ptr(), p.len(), json.as_ptr(), json.len(), &mut error) };
    }
    let message = string(error);
    if result == 0 { Ok(()) } else { Err(message) }
}

/// `Journal::read`'s journal as serde_json writes it, None when there's
/// none (or not of this schema).
pub fn journal_read(path: &Path) -> Option<Vec<u8>> {
    let p = path_bytes(path);
    let mut out = empty();
    let ok = unsafe { sys::negs_batch_journal_read(p.as_ptr(), p.len(), &mut out) };
    let bytes = take_buf(out);
    (ok == 1).then_some(bytes)
}

pub fn journal_remove(path: &Path) {
    let p = path_bytes(path);
    unsafe { sys::negs_batch_journal_remove(p.as_ptr(), p.len()) }
}

/// std::path's rules as the core has them, for the differential tests:
/// `op` 0 file_name, 1 file_stem, 2 extension, 3 parent, 4 with_extension,
/// 5 join, 6 display; `style` 0 this platform's, 1 Unix, 2 Windows.
pub fn path_rule(op: u32, style: u32, path: &[u8], arg: &str) -> Option<String> {
    let mut out = empty();
    let ok = unsafe { sys::negs_caches_path(op, style, path.as_ptr(), path.len(), arg.as_ptr(), arg.len(), &mut out) };
    let text = string(out);
    (ok == 1).then_some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_words() {
        assert_eq!(file_name(Naming::NameFilm, "", "IMG_0412 (2)", "kodak_gold_200", "jpeg"), "IMG_0412 (2)_kodak_gold_200.jpg");
        assert_eq!(file_name(Naming::Custom, "a:b\\c*{name}?", "A", "f", "tiff"), "a-b-c-A-.tif");
        assert_eq!(unique(&["a.jpg", "b.jpg", "A.jpg", "a.jpg"].map(String::from)), ["a.jpg", "b.jpg", "A 2.jpg", "a 3.jpg"]);
        assert_eq!(numbered("a.jpg", |n| ["a.jpg", "a 2.jpg"].contains(&n)), "a 3.jpg");
        assert_eq!(batch_name(Path::new("/p/IMG.CR3"), "IMG.CR3 (2)"), "IMG (2)");
        assert_eq!(Naming::from_key("custom"), Naming::Custom);
        assert_eq!(Naming::Custom.title(), "Custom");
        assert_eq!(Existing::from_key("?"), Existing::Number);
        assert_eq!(Existing::Number.title(), "Add a Number");
        assert_eq!(lightroom_title("classic"), "Lightroom Classic");
        assert_eq!(exporting(0, 2, Some("Lightroom")), "Sending 1 of 2 photographs to Lightroom");
        let run = RunView::new([Status::Done, Status::Failed, Status::Skipped, Status::Waiting], false, false);
        assert_eq!(run.counts().rule_parts, (0.25, 0.5));
        assert_eq!(run.finished_words(Path::new("/out/Trip")), "Exported 1 photograph to “Trip”; 1 failed; 1 skipped");
        assert_eq!(run.outcome_words(), ("Exported 1 of 4".into(), Some("1 failed".into())));
        assert_eq!(save("jpeg", 16, 3, 90), Save { bit_depth: 8, color_space: 2, jpeg_quality: 90 });
        assert_eq!(matched_exposure(&serde_json::json!(1), Some(0.5), 1.0), Some(serde_json::json!(1.5)));
        assert_eq!(matched_exposure(&serde_json::json!(1), Some(0.5), 0.52), None);
    }
}
