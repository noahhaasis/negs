//! negs-library's Library, run by the core: a handle, its calls in JSON,
//! and what the borrowing accessors hand out cached until the next change.

use crate::codec;
use negs_core_sys::library::{self as sys, negs_bin_t, negs_library_config_t, NegsLibrary};
use negs_core_sys::{negs_buf_copy, negs_buf_t, take_buf};
use oracle::delete::{Bin, DeletePlan, Question};
use oracle::{Config, CopiedEdits, EditDefaults, EditGroup, EditState, ExportEdits, Filter, Flag, Item, Json, Opened, Outcome, PhotoFile, Summary};
use std::cell::OnceCell;
use std::collections::{BTreeSet, HashMap};
use std::ffi::c_void;
use std::path::{Path, PathBuf};

const NO_BUF: negs_buf_t = negs_buf_t { data: std::ptr::null_mut(), len: 0 };

fn buf_of(bytes: &[u8]) -> negs_buf_t {
    unsafe { negs_buf_copy(bytes.as_ptr(), bytes.len()) }
}

unsafe fn text_of<'a>(data: *const u8, len: usize) -> &'a str {
    if data.is_null() || len == 0 {
        return "";
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(data, len) }).expect("the core passes UTF-8")
}

unsafe fn bytes_of<'a>(data: *const u8, len: usize) -> &'a [u8] {
    if data.is_null() || len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(data, len) } }
}

// MARK: The bin, called back from the core

type HostBin = Box<dyn Bin + Send>;

unsafe fn host_bin<'a>(user: *mut c_void) -> &'a mut HostBin {
    unsafe { &mut *(user as *mut HostBin) }
}

unsafe extern "C" fn bin_name(user: *mut c_void) -> negs_buf_t {
    buf_of(unsafe { host_bin(user) }.name().as_bytes())
}

unsafe extern "C" fn bin_available(user: *mut c_void, folder: *const u8, len: usize) -> u8 {
    let folder = unsafe { text_of(folder, len) };
    unsafe { host_bin(user) }.available(Path::new(folder)) as u8
}

unsafe extern "C" fn bin_trash(user: *mut c_void, file: *const u8, len: usize, out: *mut negs_buf_t) -> u8 {
    let file = unsafe { text_of(file, len) };
    match unsafe { host_bin(user) }.trash(Path::new(file)) {
        Ok(to) => {
            unsafe { *out = buf_of(to.to_string_lossy().as_bytes()) };
            1
        }
        Err(e) => {
            unsafe { *out = buf_of(e.to_string().as_bytes()) };
            0
        }
    }
}

unsafe extern "C" fn bin_put_back(
    user: *mut c_void,
    trashed: *const u8,
    trashed_len: usize,
    original: *const u8,
    original_len: usize,
    error: *mut negs_buf_t,
) -> u8 {
    let (trashed, original) = unsafe { (text_of(trashed, trashed_len), text_of(original, original_len)) };
    match unsafe { host_bin(user) }.put_back(Path::new(trashed), Path::new(original)) {
        Ok(()) => 1,
        Err(e) => {
            unsafe { *error = buf_of(e.to_string().as_bytes()) };
            0
        }
    }
}

unsafe extern "C" fn bin_remove(user: *mut c_void, file: *const u8, len: usize, error: *mut negs_buf_t) -> u8 {
    let file = unsafe { text_of(file, len) };
    match unsafe { host_bin(user) }.remove(Path::new(file)) {
        Ok(()) => 1,
        Err(e) => {
            unsafe { *error = buf_of(e.to_string().as_bytes()) };
            0
        }
    }
}

unsafe extern "C" fn bin_release(user: *mut c_void) {
    drop(unsafe { Box::from_raw(user as *mut HostBin) });
}

// MARK: Change Each Edit's transform

unsafe extern "C" fn transform_trampoline<F: Fn(&Item, &EditState) -> EditState>(
    user: *mut c_void,
    item: *const u8,
    item_len: usize,
    state: *const u8,
    state_len: usize,
) -> negs_buf_t {
    let f = unsafe { &*(user as *const F) };
    let item = codec::to_item(&codec::parse(unsafe { bytes_of(item, item_len) }));
    let state = codec::to_state(&codec::parse(unsafe { bytes_of(state, state_len) }));
    buf_of(codec::state(&f(&item, &state)).to_compact_sorted().as_bytes())
}

// MARK: The library

/// What the borrowing accessors show, read from the core in one call.
struct Snapshot {
    files: Vec<PhotoFile>,
    companions: HashMap<PathBuf, PathBuf>,
    duplicates: HashMap<PathBuf, Vec<String>>,
    summaries: HashMap<Item, Summary>,
    looks: HashMap<Item, String>,
    read_only: bool,
    failed: Option<String>,
    filter: Filter,
    open_item: Option<Item>,
    undo_name: Option<String>,
    redo_name: Option<String>,
    undo_depth: (usize, usize),
}

/// A folder's library in the core (crates/negs-library's `Library`).
pub struct Library {
    handle: *mut NegsLibrary,
    folder: PathBuf,
    snapshot: OnceCell<Snapshot>,
    defaults: OnceCell<EditDefaults>,
    state: OnceCell<Option<EditState>>,
    copied: OnceCell<Option<CopiedEdits>>,
    previous: OnceCell<Option<EditState>>,
}

// The handle is used by one thread at a time, as `&mut` makes sure; its
// bin is `Send`.
unsafe impl Send for Library {}

impl Drop for Library {
    fn drop(&mut self) {
        unsafe { sys::negs_library_free(self.handle) };
    }
}

fn items_json(items: &[Item]) -> String {
    codec::items(items).to_compact_sorted()
}

fn item_json(item: &Item) -> String {
    codec::item(item).to_compact_sorted()
}

impl Library {
    /// The library of `folder`, listed and its sidecars read.
    pub fn new(folder: impl Into<PathBuf>, config: Config) -> Result<Library, String> {
        crate::watch_for_leaks();
        let folder = folder.into();
        let folder_text = folder.to_string_lossy().into_owned();
        let data = config.data.to_string_lossy().into_owned();
        let user = Box::into_raw(Box::new(config.bin)) as *mut c_void;
        let c = negs_library_config_t {
            folder: folder_text.as_ptr(),
            folder_len: folder_text.len(),
            data: data.as_ptr(),
            data_len: data.len(),
            app: config.app.as_ptr(),
            app_len: config.app.len(),
            bin: negs_bin_t {
                user,
                name: Some(bin_name),
                available: Some(bin_available),
                trash: Some(bin_trash),
                put_back: Some(bin_put_back),
                remove: Some(bin_remove),
                release: Some(bin_release),
            },
            pairing: config.pairing as u8,
            sidecars: config.sidecars as u8,
            has_id_seed: config.id_seed.is_some() as u8,
            id_seed: config.id_seed.unwrap_or(0),
        };
        let mut error = NO_BUF;
        let handle = unsafe { sys::negs_library_new(&c, &mut error) };
        if handle.is_null() {
            return Err(String::from_utf8_lossy(&take_buf(error)).into_owned());
        }
        Ok(Library {
            handle,
            folder,
            snapshot: OnceCell::new(),
            defaults: OnceCell::new(),
            state: OnceCell::new(),
            copied: OnceCell::new(),
            previous: OnceCell::new(),
        })
    }

    /// Everything cached is read again after a change.
    fn changed(&mut self) {
        self.snapshot.take();
        self.defaults.take();
        self.state.take();
        self.copied.take();
        self.previous.take();
    }

    fn json(&self, buf: negs_buf_t) -> Json {
        let bytes = take_buf(buf);
        assert!(!bytes.is_empty(), "the core refused an argument");
        codec::parse(&bytes)
    }

    fn outcome(&mut self, buf: negs_buf_t) -> Outcome {
        let v = self.json(buf);
        self.changed();
        codec::to_outcome(&v)
    }

    fn snap(&self) -> &Snapshot {
        self.snapshot.get_or_init(|| {
            let v = self.json(unsafe { sys::negs_library_snapshot(self.handle) });
            let pairs = |key: &str| codec::array_of(v.get(key)).clone();
            Snapshot {
                files: codec::array_of(v.get("files")).iter().map(codec::to_file).collect(),
                companions: codec::path_pairs(v.get("companions")),
                duplicates: pairs("duplicates")
                    .iter()
                    .map(|p| (PathBuf::from(codec::string_of(p.at("0"))), codec::to_strings(p.at("1"))))
                    .collect(),
                summaries: codec::item_pairs(v.get("summaries"), codec::to_summary),
                looks: codec::item_pairs(v.get("looks"), codec::string_of),
                read_only: codec::bool_of(v.get("readOnly")),
                failed: codec::opt_string_of(v.get("failed")),
                filter: codec::to_filter(v.get("filter")),
                open_item: (!v.get("openItem").is_null()).then(|| codec::to_item(v.get("openItem"))),
                undo_name: codec::opt_string_of(v.get("undoName")),
                redo_name: codec::opt_string_of(v.get("redoName")),
                undo_depth: (codec::number_of(v.at("undoDepth.0")) as usize, codec::number_of(v.at("undoDepth.1")) as usize),
            }
        })
    }

    // MARK: What it shows

    pub fn folder(&self) -> &Path {
        &self.folder
    }

    pub fn files(&self) -> &[PhotoFile] {
        &self.snap().files
    }

    pub fn items(&self) -> Vec<Item> {
        codec::to_items(&self.json(unsafe { sys::negs_library_items(self.handle) }))
    }

    pub fn file(&self, item: &Item) -> Option<&PhotoFile> {
        self.snap().files.iter().find(|f| f.path == item.photo)
    }

    pub fn name(&self, item: &Item) -> String {
        let i = item_json(item);
        String::from_utf8(take_buf(unsafe { sys::negs_library_name(self.handle, i.as_ptr(), i.len()) })).expect("UTF-8")
    }

    pub fn duplicates(&self, photo: &Path) -> &[String] {
        self.snap().duplicates.get(photo).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn summary(&self, item: &Item) -> Summary {
        self.snap().summaries.get(item).cloned().unwrap_or_default()
    }

    pub fn look(&self, item: &Item) -> Option<&str> {
        self.snap().looks.get(item).map(String::as_str)
    }

    pub fn thumbnail_state(&self, item: &Item) -> Option<(EditState, bool)> {
        let i = item_json(item);
        let v = self.json(unsafe { sys::negs_library_thumbnail_state(self.handle, i.as_ptr(), i.len()) });
        (!v.is_null()).then(|| (codec::to_state(v.get("state")), codec::bool_of(v.get("matchSolved"))))
    }

    pub fn export_edits(&self, item: &Item) -> Option<ExportEdits> {
        let i = item_json(item);
        codec::to_export(&self.json(unsafe { sys::negs_library_export_edits(self.handle, i.as_ptr(), i.len()) }))
    }

    pub fn store_solved_match(&mut self, item: &Item, look: &str, state: &EditState) -> bool {
        let i = item_json(item);
        let s = codec::state(state).to_compact_sorted();
        let stored = unsafe { sys::negs_library_store_solved_match(self.handle, i.as_ptr(), i.len(), look.as_ptr(), look.len(), s.as_ptr(), s.len()) };
        self.changed();
        stored != 0
    }

    pub fn unedited_state(&self) -> EditState {
        codec::to_state(&self.json(unsafe { sys::negs_library_unedited_state(self.handle) }))
    }

    pub fn companion(&self, photo: &Path) -> Option<&Path> {
        self.snap().companions.get(photo).map(PathBuf::as_path)
    }

    pub fn read_only(&self) -> bool {
        self.snap().read_only
    }

    pub fn failed(&self) -> Option<&str> {
        self.snap().failed.as_deref()
    }

    pub fn filter(&self) -> &Filter {
        &self.snap().filter
    }

    pub fn set_filter(&mut self, filter: Filter) {
        let f = codec::filter(&filter).to_compact_sorted();
        let set = unsafe { sys::negs_library_set_filter(self.handle, f.as_ptr(), f.len()) };
        assert!(set != 0, "the core refused the filter {f}");
        self.changed();
    }

    pub fn shown(&self, current: Option<&Item>) -> Vec<Item> {
        let c = current.map(item_json).unwrap_or_default();
        codec::to_items(&self.json(unsafe { sys::negs_library_shown(self.handle, c.as_ptr(), c.len()) }))
    }

    pub fn rejected(&self) -> Vec<Item> {
        codec::to_items(&self.json(unsafe { sys::negs_library_rejected(self.handle) }))
    }

    pub fn defaults(&self) -> &EditDefaults {
        self.defaults.get_or_init(|| codec::to_defaults(&self.json(unsafe { sys::negs_library_defaults(self.handle) })))
    }

    pub fn copied(&self) -> Option<&CopiedEdits> {
        self.copied.get_or_init(|| codec::to_copied(&self.json(unsafe { sys::negs_library_copied(self.handle) }))).as_ref()
    }

    pub fn previous_edits(&self) -> Option<&EditState> {
        self.previous.get_or_init(|| codec::to_opt_state(&self.json(unsafe { sys::negs_library_previous_edits(self.handle) }))).as_ref()
    }

    pub fn undo_name(&self) -> Option<&str> {
        self.snap().undo_name.as_deref()
    }

    pub fn redo_name(&self) -> Option<&str> {
        self.snap().redo_name.as_deref()
    }

    pub fn undo_depth(&self) -> (usize, usize) {
        self.snap().undo_depth
    }

    // MARK: Settings

    pub fn set_defaults(&mut self, defaults: EditDefaults) -> Outcome {
        let d = codec::defaults(&defaults).to_compact_sorted();
        self.outcome(unsafe { sys::negs_library_set_defaults(self.handle, d.as_ptr(), d.len()) })
    }

    pub fn set_pairing(&mut self, pairing: bool) {
        unsafe { sys::negs_library_set_pairing(self.handle, pairing as u8) };
        self.changed();
    }

    pub fn set_sidecars(&mut self, on: bool) {
        unsafe { sys::negs_library_set_sidecars(self.handle, on as u8) };
        self.changed();
    }

    pub fn refresh(&mut self, interacting: bool) -> (Vec<PathBuf>, Outcome) {
        let v = self.json(unsafe { sys::negs_library_refresh(self.handle, interacting as u8) });
        self.changed();
        (codec::to_strings(v.get("changed")).into_iter().map(PathBuf::from).collect(), codec::to_outcome(v.get("outcome")))
    }

    // MARK: The open photograph

    pub fn open_item(&self) -> Option<&Item> {
        self.snap().open_item.as_ref()
    }

    pub fn state(&self) -> Option<&EditState> {
        self.state.get_or_init(|| codec::to_opt_state(&self.json(unsafe { sys::negs_library_state(self.handle) }))).as_ref()
    }

    pub fn open_edited(&self) -> bool {
        unsafe { sys::negs_library_open_edited(self.handle) != 0 }
    }

    pub fn open(&mut self, item: &Item) -> Opened {
        let i = item_json(item);
        let v = self.json(unsafe { sys::negs_library_open(self.handle, i.as_ptr(), i.len()) });
        self.changed();
        codec::to_opened(&v)
    }

    fn result(&mut self, done: u8, error: negs_buf_t) -> Result<(), String> {
        self.changed();
        if done != 0 {
            take_buf(error);
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&take_buf(error)).into_owned())
        }
    }

    pub fn close(&mut self) -> Result<(), String> {
        let mut error = NO_BUF;
        let done = unsafe { sys::negs_library_close(self.handle, &mut error) };
        self.result(done, error)
    }

    pub fn edit(&mut self, state: EditState) {
        let s = codec::state(&state).to_compact_sorted();
        let taken = unsafe { sys::negs_library_edit(self.handle, s.as_ptr(), s.len()) };
        assert!(taken != 0, "the core refused an EditState");
        self.changed();
    }

    pub fn unsaved(&self) -> bool {
        unsafe { sys::negs_library_unsaved(self.handle) != 0 }
    }

    pub fn save(&mut self) -> Result<(), String> {
        let mut error = NO_BUF;
        let done = unsafe { sys::negs_library_save(self.handle, &mut error) };
        self.result(done, error)
    }

    // MARK: Actions

    pub fn set_rating(&mut self, items: &[Item], stars: i64) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_set_rating(self.handle, i.as_ptr(), i.len(), stars) })
    }

    pub fn change_rating(&mut self, items: &[Item], by: i64) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_change_rating(self.handle, i.as_ptr(), i.len(), by) })
    }

    pub fn set_flag(&mut self, items: &[Item], flag: Option<Flag>) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_set_flag(self.handle, i.as_ptr(), i.len(), codec::flag_code(flag)) })
    }

    pub fn copy_edits(&mut self, item: &Item, groups: BTreeSet<EditGroup>) -> Outcome {
        let i = item_json(item);
        let g = codec::groups(&groups).to_compact_sorted();
        self.outcome(unsafe { sys::negs_library_copy_edits(self.handle, i.as_ptr(), i.len(), g.as_ptr(), g.len()) })
    }

    pub fn paste_edits(&mut self, items: &[Item]) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_paste_edits(self.handle, i.as_ptr(), i.len()) })
    }

    pub fn paste_from_previous(&mut self, items: &[Item]) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_paste_from_previous(self.handle, i.as_ptr(), i.len()) })
    }

    pub fn reset_edits(&mut self, items: &[Item]) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_reset_edits(self.handle, i.as_ptr(), i.len()) })
    }

    pub fn change_each_edit(&mut self, items: &[Item], action: &str, note: &str, transform: impl Fn(&Item, &EditState) -> EditState) -> Outcome {
        fn call<F: Fn(&Item, &EditState) -> EditState>(library: &mut Library, items: &[Item], action: &str, note: &str, f: &F) -> Outcome {
            let i = items_json(items);
            let buf = unsafe {
                sys::negs_library_change_each_edit(
                    library.handle,
                    i.as_ptr(),
                    i.len(),
                    action.as_ptr(),
                    action.len(),
                    note.as_ptr(),
                    note.len(),
                    transform_trampoline::<F>,
                    f as *const F as *mut c_void,
                )
            };
            library.outcome(buf)
        }
        call(self, items, action, note, &transform)
    }

    pub fn opening_state(&mut self, item: &Item) -> Option<EditState> {
        let i = item_json(item);
        let v = self.json(unsafe { sys::negs_library_opening_state(self.handle, i.as_ptr(), i.len()) });
        self.changed();
        codec::to_opt_state(&v)
    }

    pub fn new_id(&mut self) -> String {
        let id = String::from_utf8(take_buf(unsafe { sys::negs_library_new_id(self.handle) })).expect("UTF-8");
        self.changed();
        id
    }

    pub fn duplicate(&mut self, items: &[Item]) -> Outcome {
        let i = items_json(items);
        self.outcome(unsafe { sys::negs_library_duplicate(self.handle, i.as_ptr(), i.len()) })
    }

    // MARK: Deleting

    pub fn delete_plan(&self, items: &[Item]) -> DeletePlan {
        let i = items_json(items);
        codec::to_plan(&self.json(unsafe { sys::negs_library_delete_plan(self.handle, i.as_ptr(), i.len()) }))
    }

    pub fn delete_question(&self, plan: &DeletePlan) -> Question {
        let p = codec::plan(plan).to_compact_sorted();
        codec::to_question(&self.json(unsafe { sys::negs_library_delete_question(self.handle, p.as_ptr(), p.len()) }))
    }

    pub fn delete_is_permanent(&self, plan: &DeletePlan) -> bool {
        let p = codec::plan(plan).to_compact_sorted();
        unsafe { sys::negs_library_delete_is_permanent(self.handle, p.as_ptr(), p.len()) != 0 }
    }

    pub fn delete(&mut self, plan: &DeletePlan) -> Outcome {
        let p = codec::plan(plan).to_compact_sorted();
        self.outcome(unsafe { sys::negs_library_delete(self.handle, p.as_ptr(), p.len()) })
    }

    // MARK: Undo

    pub fn undo(&mut self) -> Option<Outcome> {
        let v = self.json(unsafe { sys::negs_library_undo(self.handle) });
        self.changed();
        (!v.is_null()).then(|| codec::to_outcome(&v))
    }

    pub fn redo(&mut self) -> Option<Outcome> {
        let v = self.json(unsafe { sys::negs_library_redo(self.handle) });
        self.changed();
        (!v.is_null()).then(|| codec::to_outcome(&v))
    }
}
