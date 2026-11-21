//! negs-library's values as the JSON negs_core.h's Library section passes
//! (shapes in the header), with negs-library's own `Json`.

use oracle::delete::DeletePlan;
use oracle::{
    BrightnessMatch, CopiedEdits, EditDefaults, EditGroup, EditState, EditsFilter, ExportEdits, Filter, Flag, FlagFilter, Item, Json, Note,
    Opened, Outcome, PhotoFile, Question, RatingFilter, Roll, Summary,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn parse(bytes: &[u8]) -> Json {
    Json::parse(bytes).unwrap_or_else(|e| panic!("the core sent JSON its reader refuses ({e:?}): {}", String::from_utf8_lossy(bytes)))
}

fn text(s: &str) -> Json {
    Json::string(s)
}

fn path_text(p: &Path) -> Json {
    Json::string(p.to_string_lossy())
}

fn opt_text(s: Option<&str>) -> Json {
    s.map(text).unwrap_or(Json::Null)
}

fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json {
    Json::from_pairs(pairs)
}

fn string(v: &Json) -> String {
    v.as_str().unwrap_or_else(|| panic!("a string, not {v:?}")).to_string()
}

fn opt_string(v: &Json) -> Option<String> {
    if v.is_null() { None } else { Some(string(v)) }
}

fn number(v: &Json) -> f64 {
    v.as_f64().unwrap_or_else(|| panic!("a number, not {v:?}"))
}

fn boolean(v: &Json) -> bool {
    v.as_bool().unwrap_or_else(|| panic!("a bool, not {v:?}"))
}

fn array(v: &Json) -> &Vec<Json> {
    v.as_array().unwrap_or_else(|| panic!("an array, not {v:?}"))
}

pub fn item(i: &Item) -> Json {
    match &i.variant {
        None => obj([("photo", path_text(&i.photo))]),
        Some(id) => obj([("photo", path_text(&i.photo)), ("variant", text(id))]),
    }
}

pub fn items(list: &[Item]) -> Json {
    Json::Array(list.iter().map(item).collect())
}

pub fn to_item(v: &Json) -> Item {
    Item { photo: PathBuf::from(string(v.get("photo"))), variant: opt_string(v.get("variant")) }
}

pub fn to_items(v: &Json) -> Vec<Item> {
    array(v).iter().map(to_item).collect()
}

pub fn strings(list: &[String]) -> Json {
    Json::Array(list.iter().map(|s| text(s)).collect())
}

pub fn to_strings(v: &Json) -> Vec<String> {
    array(v).iter().map(string).collect()
}

fn to_flag(v: &Json) -> Option<Flag> {
    if v.is_null() { None } else { Some(Flag::parse(&string(v)).expect("a flag")) }
}

pub fn flag_code(f: Option<Flag>) -> u8 {
    match f {
        None => 0,
        Some(Flag::Pick) => 1,
        Some(Flag::Reject) => 2,
    }
}

pub fn to_summary(v: &Json) -> Summary {
    Summary {
        rating: number(v.get("rating")) as u8,
        flag: to_flag(v.get("flag")),
        edited: boolean(v.get("edited")),
        problem: opt_string(v.get("problem")),
    }
}

pub fn state(s: &EditState) -> Json {
    let brightness_match = match &s.brightness_match {
        None => Json::Null,
        Some(m) => obj([
            ("ev", m.ev.map(Json::Number).unwrap_or(Json::Null)),
            ("highlightLimited", Json::Bool(m.highlight_limited)),
            ("solved", Json::Bool(m.solved)),
        ]),
    };
    let roll = match &s.roll {
        None => Json::Null,
        Some(r) => obj([
            ("id", text(&r.id)),
            ("magenta", Json::Number(r.magenta)),
            ("yellow", Json::Number(r.yellow)),
            ("paper", opt_text(r.paper.as_deref())),
        ]),
    };
    obj([
        ("film", text(&s.film)),
        ("paper", text(&s.paper)),
        ("params", s.params.clone()),
        ("crop", s.crop.clone()),
        ("border", s.border.clone()),
        ("brightnessMatch", brightness_match),
        ("roll", roll),
    ])
}

pub fn to_state(v: &Json) -> EditState {
    let m = v.get("brightnessMatch");
    let r = v.get("roll");
    EditState {
        film: string(v.get("film")),
        paper: string(v.get("paper")),
        params: v.get("params").clone(),
        crop: v.get("crop").clone(),
        border: v.get("border").clone(),
        brightness_match: (!m.is_null()).then(|| BrightnessMatch {
            ev: m.get("ev").as_f64(),
            highlight_limited: boolean(m.get("highlightLimited")),
            solved: boolean(m.get("solved")),
        }),
        roll: (!r.is_null()).then(|| Roll {
            id: string(r.get("id")),
            magenta: number(r.get("magenta")),
            yellow: number(r.get("yellow")),
            paper: opt_string(r.get("paper")),
        }),
    }
}

pub fn to_opt_state(v: &Json) -> Option<EditState> {
    (!v.is_null()).then(|| to_state(v))
}

pub fn defaults(d: &EditDefaults) -> Json {
    obj([
        ("params", d.params.clone()),
        ("nullable", strings(&d.nullable)),
        ("crop", d.crop.clone()),
        ("border", d.border.clone()),
        ("films", Json::Object(d.films.iter().map(|(k, v)| (k.clone(), Json::Bool(*v))).collect())),
        ("papers", Json::Array(d.papers.iter().map(|p| text(p)).collect())),
        ("tableFilm", text(&d.table_film)),
        ("tablePaper", text(&d.table_paper)),
        ("newPhotoFilm", text(&d.new_photo_film)),
        ("newPhotoPaper", text(&d.new_photo_paper)),
        ("matchNewPhotos", Json::Bool(d.match_new_photos)),
    ])
}

pub fn to_defaults(v: &Json) -> EditDefaults {
    let films: BTreeMap<String, bool> = v.get("films").as_object().expect("films").iter().map(|(k, f)| (k.clone(), boolean(f))).collect();
    EditDefaults {
        params: v.get("params").clone(),
        nullable: to_strings(v.get("nullable")),
        crop: v.get("crop").clone(),
        border: v.get("border").clone(),
        films,
        papers: to_strings(v.get("papers")).into_iter().collect(),
        table_film: string(v.get("tableFilm")),
        table_paper: string(v.get("tablePaper")),
        new_photo_film: string(v.get("newPhotoFilm")),
        new_photo_paper: string(v.get("newPhotoPaper")),
        match_new_photos: boolean(v.get("matchNewPhotos")),
    }
}

fn flag_filter_key(f: FlagFilter) -> &'static str {
    match f {
        FlagFilter::Pick => "pick",
        FlagFilter::Unflagged => "unflagged",
        FlagFilter::Reject => "reject",
    }
}

pub fn filter(f: &Filter) -> Json {
    let rating = match f.rating {
        RatingFilter::Any => obj([("kind", text("any"))]),
        RatingFilter::AtLeast(stars) => obj([("kind", text("atLeast")), ("stars", Json::Number(stars as f64))]),
        RatingFilter::Unrated => obj([("kind", text("unrated"))]),
    };
    let edits = match f.edits {
        EditsFilter::Any => "any",
        EditsFilter::Edited => "edited",
        EditsFilter::Unedited => "unedited",
    };
    obj([
        ("rating", rating),
        ("flags", Json::Array(f.flags.iter().map(|g| text(flag_filter_key(*g))).collect())),
        ("edits", text(edits)),
    ])
}

pub fn to_filter(v: &Json) -> Filter {
    let r = v.get("rating");
    let rating = match string(r.get("kind")).as_str() {
        "atLeast" => RatingFilter::AtLeast(number(r.get("stars")) as u8),
        "unrated" => RatingFilter::Unrated,
        _ => RatingFilter::Any,
    };
    let flags = to_strings(v.get("flags"))
        .iter()
        .map(|k| match k.as_str() {
            "pick" => FlagFilter::Pick,
            "unflagged" => FlagFilter::Unflagged,
            _ => FlagFilter::Reject,
        })
        .collect();
    let edits = match string(v.get("edits")).as_str() {
        "edited" => EditsFilter::Edited,
        "unedited" => EditsFilter::Unedited,
        _ => EditsFilter::Any,
    };
    Filter { rating, flags, edits }
}

pub fn to_outcome(v: &Json) -> Outcome {
    let note = v.get("note");
    Outcome {
        note: (!note.is_null()).then(|| Note {
            text: string(note.get("text")),
            detail: opt_string(note.get("detail")),
            count: number(note.get("count")) as usize,
        }),
        error: opt_string(v.get("error")),
        open_changed: boolean(v.get("openChanged")),
        open_gone: boolean(v.get("openGone")),
        open: (!v.get("open").is_null()).then(|| to_item(v.get("open"))),
        select: to_items(v.get("select")),
    }
}

pub fn to_opened(v: &Json) -> Opened {
    Opened { state: to_state(v.get("state")), problem: opt_string(v.get("problem")) }
}

pub fn groups(g: &BTreeSet<EditGroup>) -> Json {
    Json::Array(g.iter().map(|g| text(g.key())).collect())
}

pub fn to_copied(v: &Json) -> Option<CopiedEdits> {
    (!v.is_null()).then(|| CopiedEdits {
        state: to_state(v.get("state")),
        groups: to_strings(v.get("groups")).iter().map(|k| EditGroup::from_key(k).expect("a group")).collect(),
        edited: boolean(v.get("edited")),
    })
}

pub fn to_export(v: &Json) -> Option<ExportEdits> {
    (!v.is_null()).then(|| ExportEdits {
        state: to_state(v.get("state")),
        match_solved: boolean(v.get("matchSolved")),
        missing: to_strings(v.get("missing")),
        look: string(v.get("look")),
    })
}

pub fn plan(p: &DeletePlan) -> Json {
    let mut companions: Vec<(&PathBuf, &PathBuf)> = p.companions.iter().collect();
    companions.sort();
    obj([
        ("items", items(&p.items)),
        ("files", Json::Array(p.files.iter().map(|f| path_text(f)).collect())),
        ("duplicates", items(&p.duplicates)),
        ("following", Json::Number(p.following as f64)),
        ("companions", Json::Array(companions.into_iter().map(|(f, c)| Json::Array(vec![path_text(f), path_text(c)])).collect())),
        ("renamed", Json::Array(p.renamed.iter().map(|(a, b)| Json::Array(vec![item(a), item(b)])).collect())),
    ])
}

pub fn to_plan(v: &Json) -> DeletePlan {
    DeletePlan {
        items: to_items(v.get("items")),
        files: to_strings(v.get("files")).into_iter().map(PathBuf::from).collect(),
        duplicates: to_items(v.get("duplicates")),
        following: number(v.get("following")) as usize,
        companions: array(v.get("companions")).iter().map(|c| (PathBuf::from(string(c.at("0"))), PathBuf::from(string(c.at("1"))))).collect(),
        renamed: array(v.get("renamed")).iter().map(|r| (to_item(r.at("0")), to_item(r.at("1")))).collect(),
    }
}

pub fn to_question(v: &Json) -> Question {
    Question { message: string(v.get("message")), information: string(v.get("information")), button: string(v.get("button")) }
}

fn to_time(v: &Json) -> Option<SystemTime> {
    if v.is_null() {
        return None;
    }
    let secs = number(v.at("0")) as i64;
    let nanos = number(v.at("1")) as u32;
    Some(if secs >= 0 {
        UNIX_EPOCH + Duration::new(secs as u64, nanos)
    } else {
        UNIX_EPOCH - Duration::from_secs(secs.unsigned_abs()) + Duration::from_nanos(nanos as u64)
    })
}

pub fn to_file(v: &Json) -> PhotoFile {
    PhotoFile {
        path: PathBuf::from(string(v.get("path"))),
        name: string(v.get("name")),
        modified: to_time(v.get("modified")),
        size: v.get("size").as_f64().map(|s| s as u64),
    }
}

pub fn path_pairs(v: &Json) -> HashMap<PathBuf, PathBuf> {
    array(v).iter().map(|p| (PathBuf::from(string(p.at("0"))), PathBuf::from(string(p.at("1"))))).collect()
}

pub fn item_pairs<T>(v: &Json, value: impl Fn(&Json) -> T) -> HashMap<Item, T> {
    array(v).iter().map(|p| (to_item(p.at("0")), value(p.at("1")))).collect()
}

pub fn opt_string_of(v: &Json) -> Option<String> {
    opt_string(v)
}

pub fn string_of(v: &Json) -> String {
    string(v)
}

pub fn number_of(v: &Json) -> f64 {
    number(v)
}

pub fn bool_of(v: &Json) -> bool {
    boolean(v)
}

pub fn array_of(v: &Json) -> &Vec<Json> {
    array(v)
}
