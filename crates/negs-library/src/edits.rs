//! A photograph's edits as the app holds them, and as a sidecar stores
//! them: the film and paper always, everything else as what differs from
//! the defaults table (`PhotoSidecar.encodeEdits`/`decodeEdits`,
//! `EditGroup`, `EditorModel.userEdits`, `newPhotoState`, `restoredState`).
//!
//! The engine's parameters stay JSON: the crate never needs their meaning,
//! only the defaults they are measured from (`EditDefaults`).

use crate::json::Json;
use crate::sidecar::{DefaultsTable, Problem};
use std::collections::{BTreeMap, BTreeSet};

/// Prep › Match camera brightness, on.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrightnessMatch {
    /// The solved exposure, once solved. `params.prep.exposure_ev` holds the
    /// total the engine sees: this plus the user's own exposure.
    pub ev: Option<f64>,
    /// The solve held exposure back for the highlights.
    pub highlight_limited: bool,
    /// `ev` was solved for these very edits, so opening the photograph
    /// doesn't solve it again. The app clears it when an edit the match
    /// depends on changes (macOS compares `matchInputs`).
    pub solved: bool,
}

/// A scanned negative printed as a frame of a darkroom roll.
#[derive(Clone, Debug, PartialEq)]
pub struct Roll {
    pub id: String,
    pub magenta: f64,
    pub yellow: f64,
    pub paper: Option<String>,
}

/// Everything a photograph shows: `EditState` without the preview limit,
/// which is a preference. `crop` and `border` are the defaults table's
/// shapes, whole; `params` is the engine's parameters, whole.
#[derive(Clone, Debug, PartialEq)]
pub struct EditState {
    pub film: String,
    pub paper: String,
    pub params: Json,
    pub crop: Json,
    pub border: Json,
    pub brightness_match: Option<BrightnessMatch>,
    pub roll: Option<Roll>,
}

impl EditState {
    /// The defaults table as it stands: what an edit is measured from.
    pub fn from_table(table: &DefaultsTable) -> EditState {
        EditState {
            film: table.film.clone(),
            paper: table.paper.clone(),
            params: table.params.clone(),
            crop: table.crop.clone(),
            border: table.border.clone(),
            brightness_match: None,
            roll: None,
        }
    }

    /// No parameters at all (Swift's `EditState()`): what a new photograph
    /// follows when nothing came before it.
    pub fn empty() -> EditState {
        EditState {
            film: "kodak_gold_200".into(),
            paper: "kodak_portra_endura".into(),
            params: Json::object(),
            crop: Json::object(),
            border: Json::object(),
            brightness_match: None,
            roll: None,
        }
    }

    /// The user's own edits: without the brightness match's solved
    /// exposure, which isn't one (`EditorModel.userEdits`).
    pub fn user_edits(&self) -> EditState {
        let mut edits = self.clone();
        if let Some(found) = &edits.brightness_match {
            let offset = edits.params.at("prep.exposure_ev").as_f64().unwrap_or(0.0) - found.ev.unwrap_or(0.0);
            edits.params.set_at("prep.exposure_ev", Json::Number((offset * 1e9).round() / 1e9));
            edits.brightness_match = Some(BrightnessMatch::default());
        }
        edits
    }
}

/// A photograph's edits as read from its sidecar.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedEdits {
    /// The defaults table with the stored changes laid over it, before the
    /// engine's own defaults fill in what the table lacks (`restored`).
    pub state: EditState,
    /// The stored brightness match was solved for these edits.
    pub match_solved: bool,
}

const CROP_ASPECTS: [&str; 9] = ["original", "freeform", "square", "16:9", "3:2", "4:3", "5:4", "7:5", "custom"];
const BORDER_STYLES: [&str; 4] = ["white", "film", "keyline", "35mm"];
/// `CropSettings.maxAngle`.
pub const MAX_CROP_ANGLE: f64 = 45.0;

fn finite(value: &Json) -> Option<f64> {
    value.as_f64().filter(|v| v.is_finite())
}

/// A point or size as `CGPoint`/`CGSize` decode it: the first two numbers.
fn pair(value: &Json) -> Option<[f64; 2]> {
    let items = value.as_array()?;
    Some([items.first()?.as_f64()?, items.get(1)?.as_f64()?])
}

fn pair_json([a, b]: [f64; 2]) -> Json {
    Json::Array(vec![Json::Number(a), Json::Number(b)])
}

/// The crop as `CropSettings` decodes it and encodes it again: every field
/// there and of its type, others dropped.
fn decode_crop(crop: &Json) -> Option<Json> {
    let rect = crop.get("rect").as_array()?;
    let (origin, size) = (pair(rect.first()?)?, pair(rect.get(1)?)?);
    let aspect = crop.get("aspect").as_str()?;
    if !CROP_ASPECTS.contains(&aspect) {
        return None;
    }
    let custom = pair(crop.get("custom"))?;
    let angle = crop.get("angle").as_f64()?;
    Some(Json::from_pairs([
        ("rect", Json::Array(vec![pair_json(origin), pair_json(size)])),
        ("aspect", Json::string(aspect)),
        ("custom", pair_json(custom)),
        ("angle", Json::Number(angle)),
    ]))
}

/// A `#rrggbb` colour in lower case, as `BorderColor` writes it.
fn decode_color(value: &Json) -> Option<Json> {
    let text = value.as_str()?;
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(Json::String(format!("#{}", hex.to_ascii_lowercase())))
}

/// The border as `BorderSettings` decodes it: a field that's there must be
/// of its type; a missing one takes `defaults`' value.
fn decode_border(border: &Json, defaults: &Json) -> Option<Json> {
    let mut out = BTreeMap::new();
    for key in ["enabled", "export", "instagram"] {
        let value = border.as_object().and_then(|o| o.get(key)).unwrap_or(defaults.get(key));
        out.insert(key.to_string(), Json::Bool(value.as_bool()?));
    }
    for key in ["width", "filmWidth", "line"] {
        let value = border.as_object().and_then(|o| o.get(key)).unwrap_or(defaults.get(key));
        out.insert(key.to_string(), Json::Number(value.as_f64()?));
    }
    let style = border.as_object().and_then(|o| o.get("style")).unwrap_or(defaults.get("style"));
    if !BORDER_STYLES.contains(&style.as_str()?) {
        return None;
    }
    out.insert("style".into(), style.clone());
    for key in ["color", "lineColor"] {
        let value = border.as_object().and_then(|o| o.get(key)).unwrap_or(defaults.get(key));
        out.insert(key.to_string(), decode_color(value)?);
    }
    Some(Json::Object(out))
}

/// Values a saved edit can't have: a crop outside the photograph, a
/// rotation that isn't a quarter turn, a flip that isn't yes or no
/// (`EditState.validate`).
pub fn validate(state: &EditState) -> bool {
    let rect = state.crop.get("rect");
    let (Some([x, y]), Some([w, h])) = (pair(rect.at("0")), pair(rect.at("1"))) else { return false };
    let Some([cw, ch]) = pair(state.crop.get("custom")) else { return false };
    let Some(angle) = finite(state.crop.get("angle")) else { return false };
    let all_finite = [x, y, w, h, cw, ch].iter().all(|v| v.is_finite());
    let rotate = state.params.at("io.rotate_cw").as_f64().unwrap_or(0.0);
    let flip = state.params.at("io.flip");
    all_finite
        && x >= 0.0
        && y >= 0.0
        && w > 0.0
        && h > 0.0
        && x + w <= 1.000001
        && y + h <= 1.000001
        && angle.abs() <= MAX_CROP_ANGLE
        && cw > 0.0
        && ch > 0.0
        && state.params.as_object().is_some()
        && (0.0..=3.0).contains(&rotate)
        && rotate.round() == rotate
        && (flip.is_null() || flip.as_bool().is_some())
}

/// The stored form of `state`: the film and paper always, and only what
/// differs from `table` otherwise (`PhotoSidecar.encodeEdits`).
pub fn encode_edits(state: &EditState, match_solved: bool, table: &DefaultsTable) -> Json {
    let mut edits = BTreeMap::from([("film".to_string(), Json::string(&state.film)), ("paper".to_string(), Json::string(&state.paper))]);
    if let Some(params) = state.params.changes_from(&table.params) {
        edits.insert("params".into(), params);
    }
    if let Some(crop) = state.crop.changes_from(&table.crop) {
        edits.insert("crop".into(), crop);
    }
    if let Some(border) = state.border.changes_from(&table.border) {
        edits.insert("border".into(), border);
    }
    if let Some(found) = &state.brightness_match {
        let mut stored = BTreeMap::from([("highlightLimited".to_string(), Json::Bool(found.highlight_limited))]);
        if let Some(ev) = found.ev {
            stored.insert("ev".into(), Json::Number(ev));
            if !match_solved {
                stored.insert("solved".into(), Json::Bool(false));
            }
        }
        edits.insert("brightnessMatch".into(), Json::Object(stored));
    }
    if let Some(roll) = &state.roll {
        let mut stored = BTreeMap::from([
            ("id".to_string(), Json::string(&roll.id)),
            ("magenta".to_string(), Json::Number(roll.magenta)),
            ("yellow".to_string(), Json::Number(roll.yellow)),
        ]);
        if let Some(paper) = &roll.paper {
            stored.insert("paper".into(), Json::string(paper));
        }
        edits.insert("roll".into(), Json::Object(stored));
    }
    Json::Object(edits)
}

/// Stored edits laid over `table` and checked (`PhotoSidecar.decodeEdits`).
pub fn decode_edits(edits: &Json, table: &DefaultsTable) -> Result<SavedEdits, Problem> {
    let unreadable = |reason: &str| Problem::Unreadable(reason.to_string());
    let Some(fields) = edits.as_object() else { return Err(unreadable("edits that aren’t an object")) };
    let name = |key: &str, fallback: &str| -> Result<String, Problem> {
        match fields.get(key) {
            None => Ok(fallback.to_string()),
            Some(Json::String(text)) if !text.is_empty() => Ok(text.clone()),
            Some(_) => Err(Problem::Unreadable(format!("{key} isn’t a name"))),
        }
    };
    let empty = Json::object();
    let film = name("film", &table.film)?;
    let paper = name("paper", &table.paper)?;
    let params = table.params.overlaid(fields.get("params").unwrap_or(&empty));
    let crop = decode_crop(&table.crop.overlaid(fields.get("crop").unwrap_or(&empty))).ok_or_else(|| unreadable("crop"))?;
    let border = decode_border(&table.border.overlaid(fields.get("border").unwrap_or(&empty)), &table.border).ok_or_else(|| unreadable("border"))?;
    let mut solved = false;
    let mut brightness_match = None;
    if let Some(stored) = fields.get("brightnessMatch") {
        let Json::Object(found) = stored else { return Err(unreadable("brightness match")) };
        let mut ev = None;
        if let Some(value) = found.get("ev") {
            match value {
                Json::Number(n) if n.is_finite() && (-3.0..=3.0).contains(n) => ev = Some(*n),
                _ => return Err(unreadable("brightness match")),
            }
        }
        solved = ev.is_some() && found.get("solved") != Some(&Json::Bool(false));
        brightness_match = Some(BrightnessMatch { ev, highlight_limited: found.get("highlightLimited").and_then(Json::as_bool).unwrap_or(false), solved });
    }
    let mut roll = None;
    if let Some(stored) = fields.get("roll") {
        let id = stored.get("id").as_str().filter(|s| !s.is_empty()).ok_or_else(|| unreadable("roll"))?;
        let magenta = finite(stored.get("magenta")).ok_or_else(|| unreadable("roll"))?;
        let yellow = finite(stored.get("yellow")).ok_or_else(|| unreadable("roll"))?;
        let paper = match stored.get("paper") {
            Json::Null => None,
            Json::String(name) if !name.is_empty() => Some(name.clone()),
            _ => return Err(unreadable("roll")),
        };
        roll = Some(Roll { id: id.to_string(), magenta, yellow, paper });
    }
    let state = EditState { film, paper, params, crop, border, brightness_match, roll };
    if !validate(&state) {
        return Err(unreadable("edits out of range"));
    }
    Ok(SavedEdits { state, match_solved: solved })
}

/// The parts of a photograph's edits Copy Edits… offers, and how a
/// parameter path falls into them (`EditGroup`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EditGroup {
    FilmAndPaper,
    Prep,
    Lab,
    Border,
    Output,
    CropAndRotation,
    File,
}

impl EditGroup {
    pub const ALL: [EditGroup; 7] =
        [EditGroup::FilmAndPaper, EditGroup::Prep, EditGroup::Lab, EditGroup::Border, EditGroup::Output, EditGroup::CropAndRotation, EditGroup::File];

    /// What a plain Copy Edits copies: crop and rotation, and how the file
    /// was read, belong to each photograph.
    pub fn standard() -> BTreeSet<EditGroup> {
        BTreeSet::from([EditGroup::FilmAndPaper, EditGroup::Prep, EditGroup::Lab, EditGroup::Border, EditGroup::Output])
    }

    /// The tick box's words in Copy Edits….
    pub fn title(self) -> &'static str {
        match self {
            EditGroup::FilmAndPaper => "Film and paper",
            EditGroup::Prep => "Prep (white balance, exposure, Match camera brightness)",
            EditGroup::Lab => "Lab",
            EditGroup::Border => "Border",
            EditGroup::Output => "Image size and output",
            EditGroup::CropAndRotation => "Crop, rotation and flip",
            EditGroup::File => "Lens correction and input interpretation",
        }
    }

    /// The name macOS saves the choice under (`copyEditGroups`).
    pub fn key(self) -> &'static str {
        match self {
            EditGroup::FilmAndPaper => "filmAndPaper",
            EditGroup::Prep => "prep",
            EditGroup::Lab => "lab",
            EditGroup::Border => "border",
            EditGroup::Output => "output",
            EditGroup::CropAndRotation => "cropAndRotation",
            EditGroup::File => "file",
        }
    }

    pub fn from_key(key: &str) -> Option<EditGroup> {
        EditGroup::ALL.into_iter().find(|g| g.key() == key)
    }

    /// The group of a parameter: `io` keys one by one (any not listed is the
    /// file's own), `prep` is Prep, reading a negative goes with the film,
    /// and every other section is Lab.
    pub fn of(section: &str, key: &str) -> EditGroup {
        match section {
            "io" => match key {
                "scan_film" => EditGroup::FilmAndPaper,
                "raw_white_balance" => EditGroup::Prep,
                "output_gamut_compress" | "input_gamut_compress" => EditGroup::Lab,
                "photo_long_side_px" | "upscale_factor" | "output_color_space" | "output_cctf_encoding" => EditGroup::Output,
                "rotate_cw" | "flip" | "crop" | "crop_center" | "crop_size" => EditGroup::CropAndRotation,
                _ => EditGroup::File,
            },
            "prep" => EditGroup::Prep,
            "workflow" => EditGroup::FilmAndPaper,
            _ => EditGroup::Lab,
        }
    }

    /// `target` with `groups` taken from `source`. Rating and flag aren't
    /// edits and stay (`EditGroup.merge`).
    pub fn merge(groups: &BTreeSet<EditGroup>, source: &EditState, target: &EditState) -> EditState {
        let mut merged = target.clone();
        if groups.contains(&EditGroup::FilmAndPaper) {
            merged.film = source.film.clone();
            merged.paper = source.paper.clone();
            merged.roll = source.roll.clone();
        }
        if groups.contains(&EditGroup::Border) {
            merged.border = source.border.clone();
        }
        if groups.contains(&EditGroup::CropAndRotation) {
            merged.crop = source.crop.clone();
        }
        // Match camera brightness on or off goes with Prep; the match itself
        // is solved again for the photograph it lands on.
        if groups.contains(&EditGroup::Prep) {
            merged.brightness_match = source.brightness_match.as_ref().map(|_| BrightnessMatch::default());
        }
        let Some(sections) = source.params.as_object() else { return merged };
        for (section, values) in sections {
            match values {
                Json::Object(keys) if section == "io" || section == "prep" => {
                    for (key, value) in keys {
                        if groups.contains(&EditGroup::of(section, key)) {
                            merged.params.set_at(&format!("{section}.{key}"), value.clone());
                        }
                    }
                }
                _ => {
                    if groups.contains(&EditGroup::of(section, "")) {
                        merged.params.set_at(section, values.clone());
                    }
                }
            }
        }
        merged
    }
}

pub const IMAGE_SIZE_PATH: &str = "io.photo_long_side_px";
pub const IMAGE_SIZE_FACTOR_PATH: &str = "io.upscale_factor";
pub const LENS_CORRECTION_PATH: &str = "io.lens_correction.enabled";

/// What working out a photograph's edits needs from the engine's catalog
/// and the app's settings. Plain data: the front end fills it from the
/// catalog once it has arrived (`for_table` stands in until then).
#[derive(Clone, Debug, PartialEq)]
pub struct EditDefaults {
    /// The catalog's `defaults`, with `io.lens_correction.enabled` on, as
    /// the app sets it for new edits.
    pub params: Json,
    /// The catalog's `nullable_paths`.
    pub nullable: Vec<String>,
    /// A new photograph's crop and border: the newest table's.
    pub crop: Json,
    pub border: Json,
    /// The catalog's films, and whether each is a positive (slide film,
    /// scanned directly). Empty while the catalog isn't known.
    pub films: BTreeMap<String, bool>,
    /// The catalog's papers.
    pub papers: BTreeSet<String>,
    /// The table a film or paper this version lacks falls back to.
    pub table_film: String,
    pub table_paper: String,
    /// Settings › New photographs.
    pub new_photo_film: String,
    pub new_photo_paper: String,
    /// Settings › Match camera brightness for new photographs.
    pub match_new_photos: bool,
}

impl EditDefaults {
    /// The defaults of `table` (the newest), with no catalog: films and
    /// papers aren't checked.
    pub fn for_table(table: &DefaultsTable) -> EditDefaults {
        EditDefaults {
            params: table.params.clone(),
            nullable: Vec::new(),
            crop: table.crop.clone(),
            border: table.border.clone(),
            films: BTreeMap::new(),
            papers: BTreeSet::new(),
            table_film: table.film.clone(),
            table_paper: table.paper.clone(),
            new_photo_film: table.film.clone(),
            new_photo_paper: table.paper.clone(),
            match_new_photos: false,
        }
    }

    /// The state a photograph without saved edits opens with: the
    /// defaults, with the film and paper of Settings › New photographs.
    /// Image Size and the output colour follow `previous`, as a workflow's
    /// choices; its Prep, Lab, border, crop, rotation and how its file was
    /// read do not (`EditorModel.newPhotoState`).
    pub fn new_photo_state(&self, previous: &EditState) -> EditState {
        let mut next = previous.clone();
        let retained: Vec<(&str, Json)> = [IMAGE_SIZE_PATH, "io.output_color_space", "io.output_cctf_encoding"]
            .into_iter()
            .map(|path| (path, previous.params.at(path).clone()))
            .collect();
        next.film = self.new_photo_film.clone();
        next.paper = self.new_photo_paper.clone();
        next.params = self.params.clone();
        for (path, value) in retained {
            if !value.is_null() {
                next.params.set_at(path, value);
            }
        }
        next.params.set_at("io.scan_film", Json::Bool(self.films.get(&next.film).copied().unwrap_or(false)));
        next.border = self.border.clone();
        next.params.set_at("io.rotate_cw", Json::Number(0.0));
        next.params.set_at("io.flip", Json::Bool(false));
        let interpretation = self.params.at("io.input_interpretation").clone();
        if !interpretation.is_null() {
            next.params.set_at("io.input_interpretation", interpretation);
        }
        next.params.set_at(LENS_CORRECTION_PATH, self.params.at(LENS_CORRECTION_PATH).clone());
        next.crop = self.crop.clone();
        next.brightness_match = if self.match_new_photos { Some(BrightnessMatch::default()) } else { None };
        next.roll = None;
        next
    }

    /// A photograph without edits, not following any other (`plainState`).
    pub fn plain_state(&self) -> EditState {
        self.new_photo_state(&EditState::empty())
    }

    /// Saved edits under the engine's current defaults
    /// (`EditorModel.restoredState`).
    pub fn restored(&self, saved: &EditState) -> EditState {
        let mut restored = saved.clone();
        restored.params = self.params.restoring(&saved.params, &self.nullable);
        // Older edits may carry the old working-resolution factor.
        let factor = self.params.at(IMAGE_SIZE_FACTOR_PATH);
        if !factor.is_null() {
            restored.params.set_at(IMAGE_SIZE_FACTOR_PATH, factor.clone());
        }
        // The inspector no longer edits Post tone or colour.
        let post = self.params.get("post");
        if !post.is_null() {
            restored.params.set_at("post", post.clone());
        }
        restored
    }

    /// After pasting `groups` from a photograph without edits: `pasted` has
    /// none either when everything else is as a photograph without edits
    /// has it. The film and paper must be among `groups`, since a film kept
    /// in a sidecar is an edit, whichever it is (`holdsNoEdits`).
    pub fn holds_no_edits(&self, pasted: &EditState, groups: &BTreeSet<EditGroup>) -> bool {
        if !groups.contains(&EditGroup::FilmAndPaper) {
            return false;
        }
        pasted.user_edits() == EditGroup::merge(groups, pasted, &self.plain_state()).user_edits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> DefaultsTable {
        let json = Json::parse(
            br##"{"defaultsVersion": 1, "film": "kodak_gold_200", "paper": "kodak_portra_endura",
            "params": {"prep": {"exposure_ev": 0, "highlights": 0, "shadows": 0}, "io": {"rotate_cw": 0, "photo_long_side_px": 0, "scan_film": false, "lens_correction": {"enabled": true}}, "enlarger": {"print_exposure": 1}},
            "crop": {"angle": 0, "aspect": "original", "custom": [5, 4], "rect": [[0, 0], [1, 1]]},
            "border": {"color": "#ffffff", "enabled": false, "export": true, "filmWidth": 0, "instagram": false, "line": 0.004, "lineColor": "#000000", "style": "white", "width": 0.03}}"##,
        )
        .unwrap();
        DefaultsTable::from_json(&json, 1).unwrap()
    }

    #[test]
    fn edits_round_trip_through_the_stored_form() {
        let table = table();
        let mut state = EditState::from_table(&table);
        state.film = "kodak_portra_400".into();
        state.params.set_at("prep.highlights", Json::Number(-40.0));
        state.crop.set_at("rect", Json::parse(b"[[0.1, 0.05], [0.8, 0.9]]").unwrap());
        state.border.set_at("enabled", Json::Bool(true));
        state.brightness_match = Some(BrightnessMatch { ev: Some(0.45), highlight_limited: false, solved: true });
        let stored = encode_edits(&state, true, &table);
        assert_eq!(
            stored,
            Json::parse(br#"{"film": "kodak_portra_400", "paper": "kodak_portra_endura", "params": {"prep": {"highlights": -40}},
                "crop": {"rect": [[0.1, 0.05], [0.8, 0.9]]}, "border": {"enabled": true}, "brightnessMatch": {"ev": 0.45, "highlightLimited": false}}"#)
            .unwrap()
        );
        let back = decode_edits(&stored, &table).unwrap();
        assert_eq!(back.state, state);
        assert!(back.match_solved);
        let stale = encode_edits(&state, false, &table);
        assert_eq!(stale.at("brightnessMatch.solved"), &Json::Bool(false));
        assert!(!decode_edits(&stale, &table).unwrap().match_solved);
        // A new photograph's edits store only the film and paper.
        let plain = EditState::from_table(&table);
        assert_eq!(encode_edits(&plain, false, &table), Json::parse(br#"{"film": "kodak_gold_200", "paper": "kodak_portra_endura"}"#).unwrap());
    }

    #[test]
    fn edits_out_of_range_are_refused() {
        let table = table();
        for bad in [
            &br#"{"crop": {"rect": [[0.5, 0], [0.9, 1]]}}"#[..],
            br#"{"params": {"io": {"rotate_cw": 1.5}}}"#,
            br#"{"params": {"io": {"rotate_cw": 4}}}"#,
            br#"{"params": {"io": {"flip": 1}}}"#,
            br#"{"film": 3}"#,
            br#"{"film": ""}"#,
            br#"{"crop": {"aspect": "2:1"}}"#,
            br#"{"crop": {"angle": 46}}"#,
            br#"{"border": {"color": "white"}}"#,
            br#"{"border": {"style": "gold"}}"#,
            br#"{"brightnessMatch": {"ev": 4}}"#,
            br#"{"brightnessMatch": true}"#,
            br#"{"roll": {"id": "", "magenta": 1, "yellow": 1}}"#,
            br#"{"roll": {"id": "r", "magenta": 1, "yellow": 1, "paper": 3}}"#,
        ] {
            assert!(decode_edits(&Json::parse(bad).unwrap(), &table).is_err(), "{}", String::from_utf8_lossy(bad));
        }
        assert!(decode_edits(&Json::parse(br##"{"border": {"color": "#F4EFE6"}}"##).unwrap(), &table).is_ok());
        let roll = decode_edits(&Json::parse(br#"{"roll": {"id": "r", "magenta": 10, "yellow": 20, "paper": null}}"#).unwrap(), &table).unwrap();
        assert_eq!(roll.state.roll, Some(Roll { id: "r".into(), magenta: 10.0, yellow: 20.0, paper: None }));
    }

    #[test]
    fn groups_take_their_parameters() {
        let table = table();
        let mut source = EditState::from_table(&table);
        source.film = "kodak_portra_400".into();
        source.params.set_at("prep.highlights", Json::Number(-40.0));
        source.params.set_at("io.rotate_cw", Json::Number(1.0));
        source.params.set_at("io.photo_long_side_px", Json::Number(4000.0));
        source.params.set_at("enlarger.print_exposure", Json::Number(1.5));
        source.crop.set_at("angle", Json::Number(3.0));
        let target = EditState::from_table(&table);
        let merged = EditGroup::merge(&EditGroup::standard(), &source, &target);
        assert_eq!(merged.film, "kodak_portra_400");
        assert_eq!(merged.params.at("prep.highlights"), &Json::Number(-40.0));
        assert_eq!(merged.params.at("io.photo_long_side_px"), &Json::Number(4000.0));
        assert_eq!(merged.params.at("enlarger.print_exposure"), &Json::Number(1.5));
        assert_eq!(merged.params.at("io.rotate_cw"), &Json::Number(0.0), "rotation belongs to each photograph");
        assert_eq!(merged.crop, target.crop);
        let all: BTreeSet<EditGroup> = EditGroup::ALL.into_iter().collect();
        let merged = EditGroup::merge(&all, &source, &target);
        assert_eq!(merged.params.at("io.rotate_cw"), &Json::Number(1.0));
        assert_eq!(merged.crop, source.crop);
        assert_eq!(EditGroup::of("io", "lens_correction"), EditGroup::File);
        assert_eq!(EditGroup::of("workflow", ""), EditGroup::FilmAndPaper);
        assert_eq!(EditGroup::of("film_render", ""), EditGroup::Lab);
    }

    #[test]
    fn user_edits_leave_out_the_solved_match() {
        let table = table();
        let mut state = EditState::from_table(&table);
        state.params.set_at("prep.exposure_ev", Json::Number(0.65));
        state.brightness_match = Some(BrightnessMatch { ev: Some(0.45), highlight_limited: true, solved: true });
        let user = state.user_edits();
        assert_eq!(user.params.at("prep.exposure_ev"), &Json::Number(0.2));
        assert_eq!(user.brightness_match, Some(BrightnessMatch::default()));
    }

    #[test]
    fn a_new_photograph_follows_the_previous_ones_output_only() {
        let table = table();
        let mut defaults = EditDefaults::for_table(&table);
        defaults.new_photo_film = "fujifilm_provia_100f".into();
        defaults.films.insert("fujifilm_provia_100f".into(), true);
        let mut previous = EditState::from_table(&table);
        previous.params.set_at("io.photo_long_side_px", Json::Number(2000.0));
        previous.params.set_at("prep.highlights", Json::Number(-40.0));
        previous.params.set_at("io.rotate_cw", Json::Number(2.0));
        let next = defaults.new_photo_state(&previous);
        assert_eq!(next.film, "fujifilm_provia_100f");
        assert_eq!(next.params.at("io.scan_film"), &Json::Bool(true));
        assert_eq!(next.params.at("io.photo_long_side_px"), &Json::Number(2000.0));
        assert_eq!(next.params.at("prep.highlights"), &Json::Number(0.0));
        assert_eq!(next.params.at("io.rotate_cw"), &Json::Number(0.0));
        assert_eq!(next.params.at("io.flip"), &Json::Bool(false));
        assert_eq!(next.brightness_match, None);
    }
}
