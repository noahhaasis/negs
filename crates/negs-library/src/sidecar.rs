//! The `.negs` file beside each photograph (`docs/sidecar-format.md`),
//! as `Sidecar.swift` reads, patches and writes it.

use crate::json::Json;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The schema this version reads and writes.
pub const SCHEMA: i64 = 1;
pub const EXTENSION: &str = "negs";
/// A larger file isn't Negs's and is left alone.
pub const MAXIMUM_SIZE: usize = 1_048_576;
/// New sidecars are stored against this defaults table
/// (`SidecarDefaults.latest`). `tests/fixtures.rs` fails when
/// `data/sidecar-defaults` gets a newer one.
pub const LATEST_DEFAULTS: i64 = 3;
/// The `edits` keys this version reads and writes; others are kept as found.
pub const EDIT_KEYS: [&str; 7] = ["film", "paper", "params", "crop", "border", "brightnessMatch", "roll"];
/// What the top of a file may hold for a write to remove it once its one
/// variant holds nothing.
const PLAIN_TOP_KEYS: [&str; 4] = ["negs", "app", "defaultsVersion", "variants"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Flag {
    Pick,
    Reject,
}

impl Flag {
    pub fn as_str(self) -> &'static str {
        match self {
            Flag::Pick => "pick",
            Flag::Reject => "reject",
        }
    }

    pub fn parse(text: &str) -> Option<Flag> {
        match text {
            "pick" => Some(Flag::Pick),
            "reject" => Some(Flag::Reject),
            _ => None,
        }
    }
}

/// Why a sidecar isn't used. It is then never written over either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    Unreadable(String),
    Newer,
    TooLarge,
}

impl Problem {
    /// What the app says, as on macOS. `name` is the photograph's file name.
    pub fn message(&self, name: &str) -> String {
        match self {
            Problem::Unreadable(reason) => format!(
                "The edits saved for {name} couldn’t be read ({reason}), so it opens with the defaults. Its .negs file is left as it is."
            ),
            Problem::Newer => format!(
                "{name} was edited in a newer version of Negs, so it opens with the defaults. Update Negs to see its edits; its .negs file is left as it is."
            ),
            Problem::TooLarge => format!(
                "The .negs file of {name} is too large to be one of Negs’s, so it opens with the defaults and the file is left as it is."
            ),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Problem::Unreadable(reason) => write!(f, "unreadable: {reason}"),
            Problem::Newer => f.write_str("from a newer version of Negs"),
            Problem::TooLarge => f.write_str("too large"),
        }
    }
}

impl std::error::Error for Problem {}

/// One copy of the photograph: the first is the photograph itself, the
/// others its duplicates.
#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    pub id: String,
    /// 0 (unrated) to 5 stars.
    pub rating: u8,
    pub flag: Option<Flag>,
    /// The stored edits, as in the file; `None` when there are none.
    pub edits: Option<Json>,
}

/// One sidecar file, decoded. `raw` is the whole file, kept so that keys
/// this version doesn't know are written back unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct Sidecar {
    pub raw: Json,
    pub defaults_version: i64,
    pub variants: Vec<Variant>,
}

impl Sidecar {
    /// Checks a file's JSON as `PhotoSidecar(raw:)` does.
    pub fn from_raw(raw: Json) -> Result<Sidecar, Problem> {
        let unreadable = |reason: String| Problem::Unreadable(reason);
        let Json::Object(top) = &raw else { return Err(unreadable("not a JSON object".into())) };
        let schema = top.get("negs").and_then(Json::as_integer).ok_or_else(|| unreadable("no schema".into()))?;
        if schema > SCHEMA {
            return Err(Problem::Newer);
        }
        if schema < 1 {
            return Err(unreadable(format!("schema {schema}")));
        }
        let version = top
            .get("defaultsVersion")
            .and_then(Json::as_integer)
            .filter(|v| *v >= 1)
            .ok_or_else(|| unreadable("no defaults version".into()))?;
        if version > LATEST_DEFAULTS {
            return Err(Problem::Newer);
        }
        let mut variants: Vec<Variant> = Vec::new();
        let empty = Vec::new();
        for entry in top.get("variants").and_then(Json::as_array).unwrap_or(&empty) {
            let id = match entry.get("id") {
                Json::String(id) if entry.as_object().is_some() && !id.is_empty() && !variants.iter().any(|v| &v.id == id) => id.clone(),
                _ => return Err(unreadable("a variant without its own id".into())),
            };
            let rating = entry.get("rating").as_integer().unwrap_or(0);
            if !(0..=5).contains(&rating) {
                return Err(unreadable(format!("rating {rating}")));
            }
            let flag = match entry.get("flag") {
                Json::String(name) => Some(Flag::parse(name).ok_or_else(|| unreadable(format!("flag “{name}”")))?),
                _ => None,
            };
            let edits = entry.as_object().and_then(|o| o.get("edits")).cloned();
            if edits.as_ref().is_some_and(|e| e.as_object().is_none()) {
                return Err(unreadable("edits that aren’t an object".into()));
            }
            variants.push(Variant { id, rating: rating as u8, flag, edits });
        }
        Ok(Sidecar { raw, defaults_version: version, variants })
    }

    /// A file's bytes, checked.
    pub fn from_bytes(bytes: &[u8]) -> Result<Sidecar, Problem> {
        if bytes.len() > MAXIMUM_SIZE {
            return Err(Problem::TooLarge);
        }
        let raw = Json::parse(bytes).map_err(|_| Problem::Unreadable("not JSON".into()))?;
        Sidecar::from_raw(raw)
    }

    /// The variant with `id`, if any.
    pub fn variant(&self, id: &str) -> Option<&Variant> {
        self.variants.iter().find(|v| v.id == id)
    }
}

/// `IMG_0412.CR3` → `IMG_0412.CR3.negs`.
pub fn sidecar_path(photo: &Path) -> PathBuf {
    let mut name = photo.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(EXTENSION);
    photo.with_file_name(name)
}

/// The hidden file a write goes through (`.IMG_0412.CR3.negs.tmp`), which
/// folder listings skip.
pub fn temporary_path(photo: &Path) -> PathBuf {
    let mut name = std::ffi::OsString::from(".");
    name.push(sidecar_path(photo).file_name().unwrap_or_default());
    name.push(".tmp");
    photo.with_file_name(name)
}

/// The sidecar's bytes, or `None` if there's none.
pub fn read_bytes(photo: &Path) -> Result<Option<Vec<u8>>, Problem> {
    let file = match std::fs::File::open(sidecar_path(photo)) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(Problem::Unreadable(e.to_string())),
    };
    let mut bytes = Vec::new();
    file.take(MAXIMUM_SIZE as u64 + 1).read_to_end(&mut bytes).map_err(|e| Problem::Unreadable(e.to_string()))?;
    Ok(Some(bytes))
}

/// The sidecar's JSON, checked, or `None` if there's none.
pub fn read_raw(photo: &Path) -> Result<Option<Json>, Problem> {
    match read_bytes(photo)? {
        None => Ok(None),
        Some(bytes) => Sidecar::from_bytes(&bytes).map(|s| Some(s.raw)),
    }
}

/// The sidecar, or `None` if there's none.
pub fn read(photo: &Path) -> Result<Option<Sidecar>, Problem> {
    match read_bytes(photo)? {
        None => Ok(None),
        Some(bytes) => Sidecar::from_bytes(&bytes).map(Some),
    }
}

/// Writes `bytes` as the sidecar through a hidden temporary file and a
/// rename, or removes the sidecar (`None`). The bytes are written exactly.
pub fn write_bytes(photo: &Path, bytes: Option<&[u8]>) -> io::Result<()> {
    let file = sidecar_path(photo);
    let Some(bytes) = bytes else {
        return match std::fs::remove_file(&file) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        };
    };
    let temporary = temporary_path(photo);
    let result = std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, &file));
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Writes the file's JSON (`None` removes it), as `SidecarStore` does.
pub fn write_raw(photo: &Path, raw: Option<&Json>) -> io::Result<()> {
    write_bytes(photo, raw.map(Json::to_sidecar_bytes).as_deref())
}

/// A change to one variant of a sidecar, applied to the file as it is on
/// disk when written, so a rating set by another app meanwhile stays. Only
/// the fields it sets change (`SidecarPatch`).
#[derive(Clone, Debug, PartialEq)]
pub struct Patch {
    pub variant: String,
    /// The file's first variant, whatever its id, and a new one with
    /// `variant`'s id only if it has none: for a photograph that had no
    /// sidecar when it opened, whose file another app may have made since.
    pub primary: bool,
    /// The variant's new stored edits (`Some(None)` removes them).
    pub edits: Option<Option<Json>>,
    /// The new rating, 0 to 5.
    pub rating: Option<i64>,
    /// The new flag (`Some(None)` unflags).
    pub flag: Option<Option<Flag>>,
    /// What the writer knows of each stored group (`params`, `crop`,
    /// `border`): stored values outside them are a newer version's and stay.
    pub known: BTreeMap<String, Vec<Json>>,
    /// The whole file instead (`Some(None)` removes it).
    pub replacement: Option<Option<Json>>,
    /// Changes nothing unless the variant's stored edits are still these.
    pub only_if_edits: Option<Option<Json>>,
    /// The table `edits` is stored against; a new file gets it.
    pub defaults_version: i64,
    pub app: String,
}

impl Patch {
    pub fn new(variant: impl Into<String>, defaults_version: i64, app: impl Into<String>) -> Patch {
        Patch {
            variant: variant.into(),
            primary: false,
            edits: None,
            rating: None,
            flag: None,
            known: BTreeMap::new(),
            replacement: None,
            only_if_edits: None,
            defaults_version,
            app: app.into(),
        }
    }

    /// Whether the change gives a variant something to hold.
    fn adds(&self) -> bool {
        matches!(self.edits, Some(Some(_))) || self.rating.unwrap_or(0) > 0 || matches!(self.flag, Some(Some(_)))
    }

    /// The file after this change, or `None` to remove it: a single variant
    /// with nothing in it and nothing else in the file means the photograph
    /// has no rating, flag or edits left. A newer or unreadable file is
    /// never changed.
    pub fn applied(&self, raw: Option<&Json>) -> Result<Option<Json>, Problem> {
        if let Some(replacement) = &self.replacement {
            return Ok(replacement.clone());
        }
        let mut top = match raw.and_then(Json::as_object) {
            Some(top) => top.clone(),
            None => BTreeMap::from([
                ("negs".to_string(), Json::from(SCHEMA)),
                ("defaultsVersion".to_string(), Json::from(self.defaults_version)),
            ]),
        };
        if raw.is_some() {
            // Decoding checks the schema and versions.
            let sidecar = Sidecar::from_raw(Json::Object(top.clone()))?;
            // Ratings and flags don't depend on the table; edits are stored against it.
            if self.edits.is_some() && sidecar.defaults_version != self.defaults_version {
                return Err(Problem::Unreadable("its defaults version changed while it was open".into()));
            }
        }
        let mut variants = top.get("variants").and_then(Json::as_array).cloned().unwrap_or_default();
        let mut index = if self.primary && !variants.is_empty() {
            Some(0)
        } else {
            variants.iter().position(|v| v.get("id").as_str() == Some(self.variant.as_str()))
        };
        if index.is_none() {
            if !self.adds() {
                return Ok(raw.cloned());
            }
            variants.push(Json::from_pairs([("id", Json::string(&self.variant))]));
            index = Some(variants.len() - 1);
        }
        let index = index.expect("a variant");
        top.insert("app".into(), Json::string(&self.app));
        let mut fields = variants[index].as_object().cloned().unwrap_or_default();
        if let Some(expected) = &self.only_if_edits {
            if fields.get("edits") != expected.as_ref() {
                return Ok(raw.cloned());
            }
        }
        if let Some(edits) = &self.edits {
            let previous = fields.get("edits").and_then(Json::as_object).cloned().unwrap_or_default();
            // Keep what this version doesn't know, then lay what it does over it.
            let mut stored: BTreeMap<String, Json> =
                previous.iter().filter(|(k, _)| !EDIT_KEYS.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
            for (group, known) in &self.known {
                let known: Vec<&Json> = known.iter().collect();
                if let Some(unknown) = previous.get(group).and_then(|p| p.unknown_to(&known)) {
                    stored.insert(group.clone(), unknown);
                }
            }
            if let Some(Json::Object(values)) = edits {
                for (key, value) in values {
                    let merged = match stored.get(key) {
                        Some(old) => old.overlaid(value),
                        None => value.clone(),
                    };
                    stored.insert(key.clone(), merged);
                }
            }
            if stored.is_empty() {
                fields.remove("edits");
            } else {
                fields.insert("edits".into(), Json::Object(stored));
            }
        }
        if let Some(rating) = self.rating {
            if rating > 0 {
                fields.insert("rating".into(), Json::from(rating.min(5)));
            } else {
                fields.remove("rating");
            }
        }
        if let Some(flag) = self.flag {
            match flag {
                Some(flag) => fields.insert("flag".into(), Json::string(flag.as_str())),
                None => fields.remove("flag"),
            };
        }
        let only_id = fields.len() == 1 && fields.contains_key("id");
        variants[index] = Json::Object(fields);
        let count = variants.len();
        top.insert("variants".into(), Json::Array(variants));
        let plain = count == 1 && only_id && top.keys().all(|k| PLAIN_TOP_KEYS.contains(&k.as_str()));
        Ok(if plain { None } else { Some(Json::Object(top)) })
    }
}

/// `raw` without the variant `id`; `None` when nothing is left in it
/// (`PhotoSidecar.removing`). A newer file is never changed.
pub fn removing_variant(id: &str, raw: &Json) -> Result<Option<Json>, Problem> {
    Sidecar::from_raw(raw.clone())?;
    let (Some(top), Some(variants)) = (raw.as_object(), raw.get("variants").as_array()) else { return Ok(Some(raw.clone())) };
    let mut top = top.clone();
    let variants: Vec<Json> = variants.iter().filter(|v| v.get("id").as_str() != Some(id)).cloned().collect();
    let plain = variants.is_empty()
        || variants.len() == 1 && variants[0].as_object().map(|o| o.keys().map(String::as_str).collect::<BTreeSet<_>>()) == Some(BTreeSet::from(["id"]));
    top.insert("variants".into(), Json::Array(variants));
    let plain_top = top.keys().all(|k| PLAIN_TOP_KEYS.contains(&k.as_str()));
    Ok(if plain && plain_top { None } else { Some(Json::Object(top)) })
}

/// A frozen defaults table (`data/sidecar-defaults/v<n>.json`): what a
/// sidecar's edits are stored against.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultsTable {
    pub version: i64,
    pub film: String,
    pub paper: String,
    pub params: Json,
    pub border: Json,
    pub crop: Json,
}

impl DefaultsTable {
    pub fn from_json(json: &Json, version: i64) -> Result<DefaultsTable, String> {
        let damaged = || format!("The defaults table v{version} is damaged. Reinstall Negs.");
        if json.get("defaultsVersion").as_integer() != Some(version) {
            return Err(damaged());
        }
        let (Some(film), Some(paper)) = (json.get("film").as_str(), json.get("paper").as_str()) else { return Err(damaged()) };
        for key in ["params", "border", "crop"] {
            if json.get(key).as_object().is_none() {
                return Err(damaged());
            }
        }
        Ok(DefaultsTable {
            version,
            film: film.to_string(),
            paper: paper.to_string(),
            params: json.get("params").clone(),
            border: json.get("border").clone(),
            crop: json.get("crop").clone(),
        })
    }

    pub fn path(data: &Path, version: i64) -> PathBuf {
        data.join("sidecar-defaults").join(format!("v{version}.json"))
    }

    pub fn load(data: &Path, version: i64) -> Result<DefaultsTable, String> {
        let bytes = std::fs::read(Self::path(data, version)).map_err(|e| format!("The defaults table v{version} can’t be read: {e}"))?;
        let json = Json::parse(&bytes).map_err(|_| format!("The defaults table v{version} is damaged. Reinstall Negs."))?;
        DefaultsTable::from_json(&json, version)
    }

    /// What edits stored against this table know of each group: stored
    /// values outside them are a newer version's and stay. `engine` is the
    /// engine's own defaults, when known.
    pub fn known(&self, engine: Option<&Json>) -> BTreeMap<String, Vec<Json>> {
        let mut params = vec![self.params.clone()];
        if let Some(engine) = engine {
            params.push(engine.clone());
        }
        BTreeMap::from([
            ("params".to_string(), params),
            ("crop".to_string(), vec![self.crop.clone()]),
            ("border".to_string(), vec![self.border.clone()]),
        ])
    }
}

/// The defaults tables of a data folder, each read once.
#[derive(Debug)]
pub struct Tables {
    data: PathBuf,
    loaded: Mutex<HashMap<i64, Arc<DefaultsTable>>>,
}

impl Tables {
    /// `data` is the engine's data folder, which holds `sidecar-defaults/`.
    pub fn new(data: impl Into<PathBuf>) -> Tables {
        Tables { data: data.into(), loaded: Mutex::new(HashMap::new()) }
    }

    pub fn get(&self, version: i64) -> Result<Arc<DefaultsTable>, String> {
        let mut loaded = self.loaded.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(table) = loaded.get(&version) {
            return Ok(table.clone());
        }
        let table = Arc::new(DefaultsTable::load(&self.data, version)?);
        loaded.insert(version, table.clone());
        Ok(table)
    }

    pub fn latest(&self) -> Result<Arc<DefaultsTable>, String> {
        self.get(LATEST_DEFAULTS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json(text: &str) -> Json {
        Json::parse(text.as_bytes()).unwrap()
    }

    fn patch(rating: Option<i64>, flag: Option<Option<Flag>>, variant: &str, primary: bool) -> Patch {
        Patch { rating, flag, primary, ..Patch::new(variant, 1, "test") }
    }

    #[test]
    fn ratings_and_flags_change_only_their_own_fields() {
        let file = json(
            r#"{"negs": 1, "defaultsVersion": 1, "app": "old", "future": {"x": 1}, "variants": [{"id": "v1", "edits": {"film": "kodak_portra_400", "paper": "kodak_portra_endura", "newer": true}, "tags": ["a"]}]}"#,
        );
        let rated = patch(Some(3), None, "v1", false).applied(Some(&file)).unwrap().unwrap();
        let first = |j: &Json| j.get("variants").as_array().unwrap()[0].clone();
        assert_eq!(first(&rated).get("rating"), &Json::Number(3.0));
        assert_eq!(first(&rated).get("edits"), first(&file).get("edits"), "edits, unknown keys included, are untouched");
        assert_eq!(first(&rated).get("tags"), first(&file).get("tags"));
        assert_eq!(rated.get("future"), file.get("future"));
        assert_eq!(rated.get("app"), &Json::string("test"));
        let flagged = patch(None, Some(Some(Flag::Reject)), "v1", false).applied(Some(&rated)).unwrap().unwrap();
        assert_eq!(first(&flagged).get("flag"), &Json::string("reject"));
        assert_eq!(first(&flagged).get("rating"), &Json::Number(3.0), "a 3-star reject");
        let cleared = patch(Some(0), Some(None), "v1", false).applied(Some(&flagged)).unwrap().unwrap();
        assert!(first(&cleared).as_object().unwrap().get("rating").is_none());
        assert!(first(&cleared).as_object().unwrap().get("flag").is_none());
        assert!(first(&cleared).as_object().unwrap().get("edits").is_some());
        let sidecar = Sidecar::from_raw(flagged).unwrap();
        assert_eq!((sidecar.variants[0].rating, sidecar.variants[0].flag), (3, Some(Flag::Reject)));
    }

    #[test]
    fn a_rating_alone_makes_a_file_and_clearing_it_removes_it() {
        assert_eq!(patch(Some(0), None, "v1", false).applied(None).unwrap(), None, "nothing to hold, no file");
        assert_eq!(patch(None, Some(None), "v1", false).applied(None).unwrap(), None);
        let made = patch(Some(2), None, "v1", false).applied(None).unwrap().unwrap();
        assert_eq!(made.get("negs"), &Json::Number(1.0));
        assert_eq!(made.get("defaultsVersion"), &Json::Number(1.0));
        assert_eq!(made.get("variants"), &json(r#"[{"id": "v1", "rating": 2}]"#));
        assert_eq!(patch(Some(0), None, "v1", false).applied(Some(&made)).unwrap(), None, "the last rating gone, the file goes");
    }

    #[test]
    fn edits_and_ratings_merge_with_the_file_as_it_is_now() {
        let rated = json(r#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "v1", "rating": 4, "flag": "pick"}]}"#);
        let edits = Patch { edits: Some(Some(json(r#"{"film": "kodak_gold_200", "paper": "kodak_portra_endura"}"#))), ..Patch::new("v1", 1, "test") };
        let both = edits.applied(Some(&rated)).unwrap().unwrap();
        let v = &Sidecar::from_raw(both.clone()).unwrap().variants[0];
        assert_eq!((v.rating, v.flag, v.edits.is_some()), (4, Some(Flag::Pick), true));
        let removed = Patch { edits: Some(None), ..Patch::new("v1", 1, "test") }.applied(Some(&both)).unwrap().unwrap();
        let v = &Sidecar::from_raw(removed).unwrap().variants[0];
        assert_eq!((v.rating, v.flag, v.edits.is_some()), (4, Some(Flag::Pick), false), "edits removed, the rating stays");
    }

    #[test]
    fn a_new_photographs_patches_go_to_the_first_variant_another_app_made() {
        let other = json(r#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "abc", "edits": {"film": "kodak_gold_200"}}]}"#);
        let rated = patch(Some(5), None, "new00001", true).applied(Some(&other)).unwrap().unwrap();
        assert_eq!(rated.get("variants").as_array().unwrap().len(), 1);
        assert_eq!(rated.at("variants.0.id"), &Json::string("abc"));
        assert_eq!(rated.at("variants.0.rating"), &Json::Number(5.0));
        let fresh = patch(Some(1), None, "new00001", true).applied(None).unwrap().unwrap();
        assert_eq!(fresh.at("variants.0.id"), &Json::string("new00001"));
    }

    #[test]
    fn a_rating_never_writes_over_a_newer_file() {
        let newer = json(r#"{"negs": 2, "defaultsVersion": 1, "variants": []}"#);
        assert_eq!(patch(Some(3), None, "v1", false).applied(Some(&newer)), Err(Problem::Newer));
        let unknown_table = json(r#"{"negs": 1, "defaultsVersion": 99, "variants": []}"#);
        assert_eq!(patch(Some(3), None, "v1", false).applied(Some(&unknown_table)), Err(Problem::Newer));
    }

    #[test]
    fn damaged_newer_and_oversized_sidecars_are_refused() {
        assert!(matches!(Sidecar::from_bytes(b"not json"), Err(Problem::Unreadable(_))));
        assert_eq!(Sidecar::from_bytes(br#"{"negs": 2, "defaultsVersion": 1, "variants": []}"#), Err(Problem::Newer));
        assert_eq!(Sidecar::from_bytes(br#"{"negs": 1, "defaultsVersion": 99, "variants": []}"#), Err(Problem::Newer));
        for bad in [
            &br#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "rating": 9}]}"#[..],
            br#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a"}, {"id": "a"}]}"#,
            br#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "flag": "maybe"}]}"#,
            br#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": ""}]}"#,
            br#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "edits": 3}]}"#,
            br#"{"negs": 0, "defaultsVersion": 1}"#,
            br#"{"defaultsVersion": 1}"#,
            br#"{"negs": 1}"#,
            br#"[1]"#,
        ] {
            assert!(matches!(Sidecar::from_bytes(bad), Err(Problem::Unreadable(_))), "{}", String::from_utf8_lossy(bad));
        }
        assert_eq!(Sidecar::from_bytes(&vec![b' '; MAXIMUM_SIZE + 1]), Err(Problem::TooLarge));
    }

    #[test]
    fn edits_writes_keep_what_this_version_does_not_know() {
        let table = json(r#"{"params": {"prep": {"highlights": 0, "shadows": 0}}, "crop": {"rect": [[0, 0], [1, 1]]}, "border": {"enabled": false}}"#);
        let raw = json(
            r#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "e5", "edits": {"film": "a", "paper": "b", "futureGroup": {"amount": 1},
            "params": {"future_stage": {"strength": 2}, "prep": {"future_knob": 3, "highlights": -20}}, "crop": {"futureField": true, "rect": [[0, 0], [1, 0.8]]}}}]}"#,
        );
        let known = BTreeMap::from([
            ("params".to_string(), vec![table.get("params").clone()]),
            ("crop".to_string(), vec![table.get("crop").clone()]),
            ("border".to_string(), vec![table.get("border").clone()]),
        ]);
        let edits = json(r#"{"film": "c", "paper": "b", "params": {"prep": {"shadows": 15}}}"#);
        let written = Patch { edits: Some(Some(edits)), known, ..Patch::new("e5", 1, "x") }.applied(Some(&raw)).unwrap().unwrap();
        let e = written.at("variants.0.edits");
        assert_eq!(e.at("futureGroup.amount"), &Json::Number(1.0));
        assert_eq!(e.at("params.future_stage.strength"), &Json::Number(2.0));
        assert_eq!(e.at("params.prep.future_knob"), &Json::Number(3.0));
        assert_eq!(e.at("params.prep.highlights"), &Json::Null, "what this version knows is its own");
        assert_eq!(e.at("params.prep.shadows"), &Json::Number(15.0));
        assert_eq!(e.at("crop.futureField"), &Json::Bool(true));
        assert_eq!(e.at("crop.rect"), &Json::Null);
        assert_eq!(e.at("film"), &Json::string("c"));
    }

    #[test]
    fn removing_a_variant() {
        let two = json(r#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a"}, {"id": "b", "rating": 2}]}"#);
        assert_eq!(removing_variant("b", &two).unwrap(), None, "the photograph alone holds nothing");
        let rated = json(r#"{"negs": 1, "defaultsVersion": 1, "variants": [{"id": "a", "rating": 1}, {"id": "b"}]}"#);
        assert_eq!(removing_variant("b", &rated).unwrap().unwrap().get("variants"), &json(r#"[{"id": "a", "rating": 1}]"#));
        assert!(removing_variant("b", &json(r#"{"negs": 2, "defaultsVersion": 1}"#)).is_err());
    }

    #[test]
    fn names_beside_the_photograph() {
        let photo = Path::new("/photos/IMG_0412.CR3");
        assert_eq!(sidecar_path(photo), Path::new("/photos/IMG_0412.CR3.negs"));
        assert_eq!(temporary_path(photo), Path::new("/photos/.IMG_0412.CR3.negs.tmp"));
    }
}
