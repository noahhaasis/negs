//! The Windows editor's state as the library crate holds it, and back: the
//! edge between `state::EditState` (serde values, Windows' own crop and
//! border types, a brightness match that remembers its inputs) and
//! `negs_library::EditState` (the sidecar's JSON shapes, a match that is
//! solved or not). No Win32 here, so `windows/library-map` tests it on any
//! OS against the Mac's fixtures.
//!
//! The round trip library → Windows → library is exact: opening a
//! photograph and doing nothing must never write its sidecar.

use std::collections::{BTreeMap, BTreeSet};

use negs_library::{EditDefaults, EditGroup, Filter, Json};
use serde_json::{Map, Value};

use crate::json;
use crate::state::{self, EditState, Stock};

/// A serde value as the crate's JSON. Numbers are doubles there, as on the Mac.
pub fn to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Number(n) => Json::Number(n.as_f64().unwrap_or(0.0)),
        Value::String(s) => Json::String(s.clone()),
        Value::Array(items) => Json::Array(items.iter().map(to_json).collect()),
        Value::Object(map) => Json::Object(map.iter().map(|(k, v)| (k.clone(), to_json(v))).collect()),
    }
}

/// The crate's JSON as a serde value. Whole numbers become integers, as the
/// Mac's encoder writes them, so the engine's integer parameters read them;
/// −0 stays a double so its sign survives.
pub fn to_value(json: &Json) -> Value {
    match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) if *n == 0.0 && n.is_sign_negative() => serde_json::Number::from_f64(*n).map_or(Value::Null, Value::Number),
        Json::Number(n) => json::num(*n),
        Json::String(s) => Value::String(s.clone()),
        Json::Array(items) => Value::Array(items.iter().map(to_value).collect()),
        Json::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), to_value(v))).collect::<Map<String, Value>>()),
    }
}

/// The editor's state as the library keeps it.
pub fn to_library(win: &EditState) -> negs_library::EditState {
    let roll = win.roll.as_ref().map(|r| negs_library::Roll { id: r.id.clone(), magenta: r.magenta, yellow: r.yellow, paper: r.paper.clone() });
    let brightness_match = win.brightness_match.as_ref().map(|m| negs_library::BrightnessMatch {
        ev: m.ev,
        highlight_limited: m.highlight_limited,
        // Solved for these very edits: the inputs it was solved for are current.
        solved: m.ev.is_some() && m.inputs.as_ref().is_some_and(|inputs| json::same(inputs, &state::match_inputs(win))),
    });
    negs_library::EditState {
        film: win.film.clone(),
        paper: win.paper.clone(),
        params: to_json(&win.params),
        crop: to_json(&serde_json::to_value(win.crop).unwrap_or(Value::Null)),
        border: to_json(&serde_json::to_value(win.border).unwrap_or(Value::Null)),
        brightness_match,
        roll,
    }
}

/// The library's state as the editor shows it. `limit_preview` is the
/// preference, not an edit.
pub fn from_library(edits: &negs_library::EditState, limit_preview: bool) -> Result<EditState, String> {
    let crop = serde_json::from_value(to_value(&edits.crop)).map_err(|e| format!("crop: {e}"))?;
    let border = serde_json::from_value(to_value(&edits.border)).map_err(|e| format!("border: {e}"))?;
    let mut win = EditState {
        params: to_value(&edits.params),
        film: edits.film.clone(),
        paper: edits.paper.clone(),
        limit_preview,
        border,
        crop,
        brightness_match: None,
        roll: edits.roll.as_ref().map(|r| state::Roll { id: r.id.clone(), magenta: r.magenta, yellow: r.yellow, paper: r.paper.clone() }),
    };
    if let Some(found) = &edits.brightness_match {
        let inputs = (found.solved && found.ev.is_some()).then(|| state::match_inputs(&win));
        win.brightness_match = Some(state::BrightnessMatch { ev: found.ev, inputs, highlight_limited: found.highlight_limited });
    }
    Ok(win)
}

/// What the library measures edits from, from the editor's catalog: its
/// defaults (lens corrections on, as `apply_catalog` sets them) and
/// nullable paths, its films (and which are positives) and papers, and
/// Settings' choices for new photographs. `table` is the newest defaults
/// table's own (`EditDefaults::for_table`), which names the fallbacks.
pub fn edit_defaults(
    table: &EditDefaults,
    defaults: &Value,
    nullable: &[String],
    stocks: &[Stock],
    new_photo: (Option<&str>, Option<&str>),
    match_new_photos: bool,
) -> EditDefaults {
    let films: BTreeMap<String, bool> = stocks.iter().filter(|s| !s.paper).map(|s| (s.id.clone(), s.positive)).collect();
    let papers: BTreeSet<String> = stocks.iter().filter(|s| s.paper).map(|s| s.id.clone()).collect();
    let (film, paper) = new_photo_stocks(new_photo, (&table.table_film, &table.table_paper), stocks);
    EditDefaults {
        params: to_json(defaults),
        nullable: nullable.to_vec(),
        crop: table.crop.clone(),
        border: table.border.clone(),
        films,
        papers,
        table_film: table.table_film.clone(),
        table_paper: table.table_paper.clone(),
        new_photo_film: film,
        new_photo_paper: paper,
        match_new_photos,
    }
}

/// Settings › New photographs' film and paper (`newPhotoFilm`): the chosen
/// stock, else the newest defaults table's, else the catalog's first. Before
/// the catalog nothing can be checked, and the choice or the table's stands.
pub fn new_photo_stocks(chosen: (Option<&str>, Option<&str>), table: (&str, &str), stocks: &[Stock]) -> (String, String) {
    let pick = |chosen: Option<&str>, tabled: &str, paper: bool| -> String {
        let known = |id: &str| stocks.iter().any(|s| s.paper == paper && s.id == id);
        if !stocks.is_empty() {
            if let Some(id) = [chosen, Some(tabled)].into_iter().flatten().find(|id| known(id)) {
                return id.to_string();
            }
            // The first by id, as the library's sorted tables have it.
            if let Some(first) = stocks.iter().filter(|s| s.paper == paper).map(|s| &s.id).min() {
                return first.clone();
            }
        }
        chosen.unwrap_or(tabled).to_string()
    };
    (pick(chosen.0, table.0, false), pick(chosen.1, table.1, true))
}

/// The library's settings, kept with the app's preferences under the Mac's
/// keys: the filter (in `LibraryFilter`'s form), Copy Edits…'s groups and
/// Advance After Rating.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LibraryPreferences {
    pub library_filter: Option<Value>,
    pub copy_edit_groups: Option<Vec<String>>,
    pub advance_after_rating: bool,
    /// Settings › New photographs › Film and Paper; the table's until chosen.
    pub new_photo_film: Option<String>,
    pub new_photo_paper: Option<String>,
    /// Settings › RAW + JPEG › Show camera JPEGs separately: off, a RAW's
    /// camera JPEG of the same name goes with it as one photograph.
    #[serde(rename = "separateCameraJPEGs")]
    pub separate_camera_jpegs: bool,
    /// Settings › Edits › Save ratings and edits next to photos; on until
    /// turned off.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sidecars: Option<bool>,
}

impl LibraryPreferences {
    pub fn sidecars(&self) -> bool {
        self.sidecars.unwrap_or(true)
    }

    pub fn filter(&self) -> Filter {
        self.library_filter.as_ref().map(|v| Filter::from_json(&to_json(v))).unwrap_or_default()
    }

    pub fn set_filter(&mut self, filter: &Filter) {
        self.library_filter = filter.active().then(|| to_value(&filter.to_json()));
    }

    /// Copy Edits…'s last choice; a plain Copy Edits' groups until one is made.
    pub fn copy_groups(&self) -> BTreeSet<EditGroup> {
        match &self.copy_edit_groups {
            Some(keys) => keys.iter().filter_map(|k| EditGroup::from_key(k)).collect(),
            None => EditGroup::standard(),
        }
    }

    pub fn set_copy_groups(&mut self, groups: &BTreeSet<EditGroup>) {
        self.copy_edit_groups = Some(groups.iter().map(|g| g.key().to_string()).collect());
    }
}

/// A key typed while no text field has focus, as the library reads it: the
/// rating keys, `[` and `]`, and P X U. By the character, so `[` is AltGr+8
/// on a German keyboard as it is on the Mac's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LibraryKey {
    Rate(i64),
    Raise,
    Lower,
    Pick,
    Reject,
    Unflag,
}

pub fn library_key(character: char) -> Option<LibraryKey> {
    Some(match character {
        '0'..='5' => LibraryKey::Rate(character as i64 - '0' as i64),
        ']' => LibraryKey::Raise,
        '[' => LibraryKey::Lower,
        'p' | 'P' => LibraryKey::Pick,
        'x' | 'X' => LibraryKey::Reject,
        'u' | 'U' => LibraryKey::Unflag,
        _ => return None,
    })
}

/// One step of the editor's undo stack: an edit of the open photograph (the
/// state before or after it), or a library action, undone by the library
/// itself. The library's steps interleave with the editor's in one stack, as
/// both register with one `NSUndoManager` on the Mac.
#[derive(Clone, Debug)]
pub enum Step {
    Edit(EditState),
    /// The open photograph before the action and after it (after advancing,
    /// say): undone at once, the view goes back too.
    Library { before: Option<negs_library::Item>, after: Option<negs_library::Item> },
}

impl Step {
    pub fn is_library(&self) -> bool {
        matches!(self, Step::Library { .. })
    }
}

/// A note's action as the status line says it: "Edits Pasted" becomes
/// "Edits pasted"; stars and one-word notes stay (`StatusText.action`).
pub fn note_action(text: &str) -> String {
    match text.find(' ') {
        Some(space) => format!("{}{}", &text[..space], text[space..].to_lowercase()),
        None => text.to_string(),
    }
}

/// What follows it: how many photographs, and why it wasn't saved.
pub fn note_detail(note: &negs_library::Note) -> String {
    let mut parts = vec![];
    if note.count > 1 {
        parts.push(format!("{} photographs", note.count));
    }
    if let Some(detail) = &note.detail {
        parts.push(detail.clone());
    }
    parts.join(" · ")
}

/// A note shows for a second, or two and a half when it gives a reason.
pub fn note_duration(note: &negs_library::Note) -> std::time::Duration {
    std::time::Duration::from_millis(if note.detail.is_none() { 1000 } else { 2500 })
}

/// The editor's history for the open photograph goes when its edits were
/// replaced or another opens; the library's steps stay (`removeAllActions(
/// withTarget: self)`).
pub fn drop_editor_steps<T>(steps: &mut Vec<(Step, T)>) {
    steps.retain(|(step, _)| step.is_library());
}
