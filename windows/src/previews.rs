//! Film, paper and preset previews (`Previews.swift`, `stock_browser.rs`):
//! the photograph developed with each stock, and with each preset, as
//! thumbnails. The model schedules them (`Model::pump_previews`); this file
//! holds the pure rules, the tiles' state and the pixels they draw.
//! See docs/stock-previews.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Result, ensure};
use serde_json::{Value, json};

use crate::icc::Transform;
use crate::json;
use crate::state::{self, EditState, Stock};

/// The thumbnail's long edge: two columns of a ~340 px inspector at 2×.
pub const LONG_SIDE: u32 = 320;

/// The helper request for `stock`'s tile, without its scratch file. It
/// identifies the thumbnail: the photograph (path and generation), every
/// render setting with the stock chosen, and the crop and straighten.
/// `state` is the edit as rendered (`Model::render_state`); `keep_paper` as
/// for `state::choosing`, so a film's tile shows what a click would develop.
pub fn stock_command(state: &EditState, stock: &Stock, stocks: &[Stock], keep_paper: bool, source: &Path, generation: u64) -> Value {
    let chosen = state::choosing(state, &stock.id, stock.paper, keep_paper, stocks);
    let mut command = json!({
        "command": "thumbnail", "input": source, "source_generation": generation,
        "film": chosen.film, "paper": chosen.paper, "params": chosen.params, "long_side": LONG_SIDE,
    });
    if let Some(crop) = crop(&chosen) {
        command["crop"] = crop;
    }
    command
}

/// The helper request for a preset's tile, without its scratch file: the
/// helper applies the preset over `state` as `preset_import` does.
pub fn preset_command(text: &str, state: &EditState, source: &Path, generation: u64) -> Value {
    let mut command = json!({
        "command": "thumbnail", "input": source, "source_generation": generation,
        "film": state.film, "paper": state.paper, "params": state.params, "long_side": LONG_SIDE, "preset": text,
    });
    if let Some(crop) = crop(state) {
        command["crop"] = crop;
    }
    command
}

/// The crop and straighten, in `render`'s and `save`'s shape, when there is one.
fn crop(state: &EditState) -> Option<Value> {
    let crop = &state.crop;
    crop.cropped().then(|| {
        let r = crop.rect;
        json!({"x": r.x, "y": r.y, "width": r.width, "height": r.height, "angle": crop.angle})
    })
}

/// What identifies a thumbnail: the photograph and its request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub generation: u64,
    pub request: String,
}

impl Key {
    pub fn new(generation: u64, command: &Value) -> Self {
        Self { generation, request: command.to_string() }
    }
}

/// What identifies a preset's thumbnail: its command with only the settings
/// a preset keeps (`kept`, the catalog's `preset_kept`), since the preset
/// replaces the rest. A Lab edit then leaves the tiles alone. Without the
/// list (an older helper), every setting counts.
pub fn preset_key(command: &Value, kept: Option<&[String]>, generation: u64) -> Key {
    let Some(kept) = kept else { return Key::new(generation, command) };
    let mut request = command.clone();
    let mut subset = json!({});
    for path in kept {
        json::set(&mut subset, path, json::get(&command["params"], path).clone());
    }
    request["params"] = subset;
    // The preset brings its own film and paper.
    if let Some(object) = request.as_object_mut() {
        object.remove("film");
        object.remove("paper");
    }
    Key::new(generation, &request)
}

/// The white balance a RAW decode uses: the temperature and tint count only
/// in Custom mode (`RawWhiteBalance::effective`).
pub fn effective_white_balance(wb: &Value) -> Value {
    let mode = match json::string(&wb["mode"]) {
        "" => "as_shot",
        mode => mode,
    };
    if mode == "custom" {
        json!({"mode": mode, "temperature": wb["temperature"], "tint": wb["tint"]})
    } else {
        json!({"mode": mode})
    }
}

/// Whether a preset's RAW white balance makes the helper decode the RAW
/// again (~1.5 s for 60 MP).
pub fn needs_own_decode(preset: &Value, photo: &Value) -> bool {
    effective_white_balance(preset) != effective_white_balance(photo)
}

/// The presets in list order, except on a RAW: those needing their own
/// decode come last, grouped by white balance, so each decode is followed
/// by the presets it serves and the quick ones show first.
pub fn render_order<P>(presets: Vec<P>, raw: bool, white_balance: impl Fn(&P) -> Option<Value>, photo: &Value) -> Vec<P> {
    if !raw {
        return presets;
    }
    let mut groups: Vec<Value> = vec![];
    let mut ranked: Vec<(usize, usize, P)> = presets
        .into_iter()
        .enumerate()
        .map(|(index, preset)| {
            let rank = match white_balance(&preset) {
                Some(wb) if needs_own_decode(&wb, photo) => {
                    let effective = effective_white_balance(&wb);
                    let group = groups.iter().position(|g| *g == effective).unwrap_or_else(|| {
                        groups.push(effective);
                        groups.len() - 1
                    });
                    1 + group
                }
                _ => 0,
            };
            (rank, index, preset)
        })
        .collect();
    ranked.sort_by_key(|(rank, index, _)| (*rank, *index));
    ranked.into_iter().map(|(_, _, preset)| preset).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Display {
    Current,
    Dimmed,
    Placeholder,
}

/// How a tile shows while `wanted` is its thumbnail: as is, dimmed after an
/// edit to the same photograph, or blank for another photograph (never the
/// previous photograph's picture).
pub fn display(shown: Option<&Key>, wanted: &Key) -> Display {
    match shown {
        None => Display::Placeholder,
        Some(shown) if shown == wanted => Display::Current,
        Some(shown) if shown.generation == wanted.generation => Display::Dimmed,
        Some(_) => Display::Placeholder,
    }
}

/// A thumbnail's pixels, ready for GDI: top-down BGRA in the monitor's
/// encoding, turned like the photograph.
pub struct Bitmap {
    pub bgra: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

impl Bitmap {
    /// The helper's RGBA buffer (sRGB curve; Rec. 2020 primaries when
    /// `wide`) for the screen: through the monitor profile like the canvas,
    /// and turned `rotation` quarter turns clockwise.
    pub fn from_rgba(rgba: &[u8], width: u32, height: u32, wide: bool, cm: Option<&Transform>, rotation: i32) -> Result<Bitmap> {
        ensure!(width > 0 && height > 0 && width <= 4096 && height <= 4096, "Invalid thumbnail dimensions");
        ensure!(rgba.len() == width as usize * height as usize * 4, "The image engine returned an invalid thumbnail buffer.");
        let decode: Vec<f32> = (0..256).map(|i| srgb_decode(i as f32 / 255.0)).collect();
        let m = crate::gpu::REC2020_TO_SRGB;
        let shown: Vec<[u8; 3]> = rgba
            .chunks_exact(4)
            .map(|p| {
                let mut rgb = [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]];
                if wide {
                    let c = rgb;
                    rgb = std::array::from_fn(|i| m[i][0] * c[0] + m[i][1] * c[1] + m[i][2] * c[2]);
                }
                present(rgb, cm).map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)
            })
            .collect();
        let turns = rotation.rem_euclid(4);
        let (w, h) = (width as usize, height as usize);
        let (out_w, out_h) = if turns % 2 == 1 { (h, w) } else { (w, h) };
        let mut bgra = vec![0u8; out_w * out_h * 4];
        for y in 0..out_h {
            for x in 0..out_w {
                // The source pixel that lands at (x, y) after the turns.
                let (sx, sy) = match turns {
                    1 => (y, h - 1 - x),
                    2 => (w - 1 - x, h - 1 - y),
                    3 => (w - 1 - y, x),
                    _ => (x, y),
                };
                let [r, g, b] = shown[sy * w + sx];
                bgra[(y * out_w + x) * 4..][..4].copy_from_slice(&[b, g, r, 255]);
            }
        }
        Ok(Bitmap { bgra, width: out_w as u32, height: out_h as u32 })
    }
}

fn srgb_decode(x: f32) -> f32 {
    if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
}

fn srgb_encode(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x <= 0.0031308 { x * 12.92 } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 }
}

/// Linear sRGB to the monitor's encoding, as the canvas presents it.
fn present(rgb: [f32; 3], cm: Option<&Transform>) -> [f32; 3] {
    let Some(cm) = cm else { return rgb.map(srgb_encode) };
    std::array::from_fn(|c| {
        let m: f32 = (0..3).map(|j| cm.matrix[c][j] * rgb[j]).sum();
        let p = srgb_encode(m) * (cm.lut.len() - 1) as f32;
        let i = (p.floor() as usize).min(cm.lut.len() - 1);
        let j = (i + 1).min(cm.lut.len() - 1);
        cm.lut[i][c] + (cm.lut[j][c] - cm.lut[i][c]) * (p - i as f32)
    })
}

/// One stock's or preset's thumbnail.
#[derive(Default)]
pub struct Thumb {
    pub image: Option<Arc<Bitmap>>,
    pub dimmed: bool,
    /// What the image (or the failure that left it blank) was made for.
    shown: Option<Key>,
}

impl Thumb {
    /// Updates the look for `wanted`: whether it needs a new thumbnail, and
    /// whether its look changed.
    pub fn mark(&mut self, wanted: &Key) -> (bool, bool) {
        let before = (self.image.is_some(), self.dimmed);
        let needs = match display(self.shown.as_ref(), wanted) {
            Display::Current => {
                self.dimmed = false;
                false
            }
            Display::Dimmed => {
                self.dimmed = true;
                true
            }
            Display::Placeholder => {
                self.image = None;
                self.dimmed = false;
                true
            }
        };
        (needs, before != (self.image.is_some(), self.dimmed))
    }

    /// A finished thumbnail, or None when it failed (blank until the next edit).
    pub fn show(&mut self, image: Option<Arc<Bitmap>>, key: Key) {
        self.image = image;
        self.shown = Some(key);
        self.dimmed = false;
    }

    /// Drops the thumbnail; true if there was one.
    pub fn clear(&mut self) -> bool {
        let had = self.shown.is_some() || self.image.is_some();
        *self = Thumb::default();
        had
    }
}

/// Which thumbnail a request is for.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    Stock { id: String, paper: bool },
    Preset(PathBuf),
}

/// The tiles and the foldouts' open state. The setting itself is
/// `Preferences::previews`.
#[derive(Default)]
pub struct Previews {
    /// Collapsed at every launch.
    pub film_open: bool,
    pub paper_open: bool,
    /// The Film grid's search field, as typed: for this session, kept while
    /// the foldout closes and opens again, as the Mac's `StockFoldout` keeps
    /// its query. Not a preference.
    pub film_query: String,
    thumbs: HashMap<Target, Thumb>,
    /// A thumbnail request is with the helper.
    pub busy: bool,
    /// The canvas monitor's profile, which the thumbnails are drawn through.
    pub color: Option<Arc<Transform>>,
    /// For the scratch files' names.
    pub requests: u64,
}

impl Previews {
    pub fn open(&self, paper: bool) -> bool {
        if paper { self.paper_open } else { self.film_open }
    }

    pub fn thumb(&self, target: &Target) -> Option<&Thumb> {
        self.thumbs.get(target)
    }

    pub fn thumb_mut(&mut self, target: &Target) -> &mut Thumb {
        self.thumbs.entry(target.clone()).or_default()
    }

    /// Keeps only the presets still listed.
    pub fn retain_presets(&mut self, listed: &[PathBuf]) {
        self.thumbs.retain(|target, _| match target {
            Target::Preset(path) => listed.contains(path),
            Target::Stock { .. } => true,
        });
    }

    /// A renamed preset keeps its thumbnail.
    pub fn move_preset(&mut self, old: &Path, new: &Path) {
        if let Some(thumb) = self.thumbs.remove(&Target::Preset(old.to_path_buf())) {
            self.thumbs.insert(Target::Preset(new.to_path_buf()), thumb);
        }
    }

    /// Drops every thumbnail; true if there were any.
    pub fn clear(&mut self) -> bool {
        let had = self.thumbs.values_mut().fold(false, |had, thumb| thumb.clear() | had);
        self.thumbs.clear();
        had
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(id: &str, paper: bool, positive: bool, target: &str) -> Stock {
        Stock { id: id.into(), name: id.into(), paper, positive, bw: false, target: target.into(), times: vec![], grain: Value::Null }
    }

    fn stocks() -> Vec<Stock> {
        vec![
            stock("portra", false, false, "endura"),
            stock("velvia", false, true, ""),
            stock("vision", false, false, "print_film"),
            stock("endura", true, false, ""),
            stock("crystal", true, false, ""),
            stock("print_film", true, false, ""),
        ]
    }

    fn edit() -> EditState {
        EditState {
            film: "portra".into(),
            paper: "crystal".into(),
            params: json!({
                "io": {"scan_film": false},
                "film_render": {"development_time": 7.0, "grain": {"rms_granularity": [1.0, 2.0, 3.0]},
                    "dir_couplers": {"gamma_samelayer_rgb": 0.5}, "halation": {"halation_strength": [0.1, 0.0, 0.0]}},
                "print_render": {"development_time": 3.0},
                "camera": {"exposure_compensation_ev": 1.0},
            }),
            ..Default::default()
        }
    }

    #[test]
    fn a_film_tile_pairs_its_paper_and_resets_the_films_presets() {
        let chosen = state::choosing(&edit(), "vision", false, false, &stocks());
        assert_eq!((chosen.film.as_str(), chosen.paper.as_str()), ("vision", "print_film"));
        assert!(chosen.value("film_render.development_time").is_null());
        assert!(chosen.value("print_render.development_time").is_null());
        assert!(chosen.value("film_render.grain.rms_granularity").is_null());
        assert!(chosen.value("film_render.dir_couplers.gamma_samelayer_rgb").is_null());
        assert!(chosen.value("film_render.halation.halation_strength").is_null());
        assert_eq!(chosen.value("camera.exposure_compensation_ev"), &json!(1.0));
        let slide = state::choosing(&edit(), "velvia", false, false, &stocks());
        assert_eq!(slide.value("io.scan_film"), &json!(true));
        // No target print: the paper stays.
        assert_eq!(slide.paper, "crystal");
    }

    /// Settings › Keep paper when changing film (Linux `keep_paper_leaves_the_paper_alone`).
    #[test]
    fn keep_paper_leaves_the_paper_alone() {
        for keep in [false, true] {
            let chosen = state::choosing(&edit(), "vision", false, keep, &stocks());
            assert_eq!(chosen.paper, if keep { "crystal" } else { "print_film" });
            let time = chosen.value("print_render.development_time").clone();
            assert_eq!(time, if keep { json!(3.0) } else { Value::Null });
            // Everything else a film change does still happens.
            assert_eq!(chosen.film, "vision");
            assert!(chosen.value("film_render.development_time").is_null());
            assert!(chosen.value("film_render.grain.rms_granularity").is_null());
            assert_eq!(state::choosing(&edit(), "velvia", false, keep, &stocks()).value("io.scan_film"), &json!(true));
        }
    }

    #[test]
    fn a_film_tile_follows_the_keep_paper_setting() {
        let stocks = stocks();
        let photo = Path::new("a.jpg");
        let vision = &stocks[2];
        let kept = stock_command(&edit(), vision, &stocks, true, photo, 1);
        let paired = stock_command(&edit(), vision, &stocks, false, photo, 1);
        assert_eq!((kept["paper"].as_str(), paired["paper"].as_str()), (Some("crystal"), Some("print_film")));
        assert_ne!(Key::new(1, &kept), Key::new(1, &paired));
        // A paper tile doesn't depend on it.
        let endura = &stocks[3];
        assert_eq!(stock_command(&edit(), endura, &stocks, true, photo, 1), stock_command(&edit(), endura, &stocks, false, photo, 1));
    }

    #[test]
    fn a_paper_tile_keeps_the_film() {
        let chosen = state::choosing(&edit(), "endura", true, false, &stocks());
        assert_eq!((chosen.film.as_str(), chosen.paper.as_str()), ("portra", "endura"));
        assert!(chosen.value("print_render.development_time").is_null());
        assert_eq!(chosen.value("film_render.development_time"), &json!(7.0));
        assert_eq!(chosen.value("film_render.grain.rms_granularity"), &json!([1.0, 2.0, 3.0]));
    }

    #[test]
    fn the_tile_key_changes_with_the_crop_and_the_photograph() {
        let stocks = stocks();
        let photo = Path::new("C:\\photos\\a.jpg");
        let key = |state: &EditState, source: &Path, generation| Key::new(generation, &stock_command(state, &stocks[0], &stocks, false, source, generation));
        let plain = key(&edit(), photo, 1);
        assert!(stock_command(&edit(), &stocks[0], &stocks, false, photo, 1).get("crop").is_none());
        let mut cropped = edit();
        cropped.crop.rect.width = 0.8;
        let with_crop = key(&cropped, photo, 1);
        assert_ne!(with_crop, plain);
        assert_eq!(stock_command(&cropped, &stocks[0], &stocks, false, photo, 1)["crop"]["width"], json!(0.8));
        let mut straightened = edit();
        straightened.crop.angle = 3.0;
        assert_ne!(key(&straightened, photo, 1), plain);
        assert_ne!(key(&edit(), Path::new("C:\\photos\\b.jpg"), 2), plain);
        assert_ne!(key(&edit(), photo, 2), plain);
        // The border isn't part of the thumbnail.
        let mut bordered = edit();
        bordered.border.enabled = true;
        assert_eq!(key(&bordered, photo, 1), plain);
    }

    #[test]
    fn preset_tiles_ignore_lab_edits() {
        let kept = vec!["io.rotate_cw".to_string(), "post".to_string()];
        let photo = Path::new("a.nef");
        let key = |state: &EditState| preset_key(&preset_command("{}", state, photo, 1), Some(&kept), 1);
        let mut edited = edit();
        edited.film = "velvia".into();
        json::set(&mut edited.params, "camera.exposure_compensation_ev", json!(2.0));
        assert_eq!(key(&edited), key(&edit()));
        let mut turned = edit();
        json::set(&mut turned.params, "io.rotate_cw", json!(1));
        assert_ne!(key(&turned), key(&edit()));
        let mut cropped = edit();
        cropped.crop.angle = 2.0;
        assert_ne!(key(&cropped), key(&edit()));
        // An older helper without the list: every setting counts.
        assert_ne!(preset_key(&preset_command("{}", &edited, photo, 1), None, 1), preset_key(&preset_command("{}", &edit(), photo, 1), None, 1));
    }

    #[test]
    fn stale_tiles_dim_and_another_photograph_blanks_them() {
        let shown = Key { generation: 1, request: "a".into() };
        assert_eq!(display(None, &shown), Display::Placeholder);
        assert_eq!(display(Some(&shown), &shown), Display::Current);
        assert_eq!(display(Some(&shown), &Key { generation: 1, request: "b".into() }), Display::Dimmed);
        assert_eq!(display(Some(&shown), &Key { generation: 2, request: "a".into() }), Display::Placeholder);

        let mut thumb = Thumb::default();
        let bitmap = Arc::new(Bitmap { bgra: vec![0; 4], width: 1, height: 1 });
        thumb.show(Some(bitmap), shown.clone());
        assert_eq!(thumb.mark(&shown), (false, false));
        assert_eq!(thumb.mark(&Key { generation: 1, request: "b".into() }), (true, true));
        assert!(thumb.image.is_some() && thumb.dimmed);
        assert_eq!(thumb.mark(&Key { generation: 2, request: "b".into() }), (true, true));
        assert!(thumb.image.is_none() && !thumb.dimmed);
        // A failure stays blank, without asking again, until the next edit.
        let failed = Key { generation: 2, request: "c".into() };
        thumb.show(None, failed.clone());
        assert_eq!(thumb.mark(&failed), (false, false));
    }

    #[test]
    fn presets_with_another_white_balance_render_last_grouped() {
        let as_shot = json!({"mode": "as_shot", "temperature": 5000.0, "tint": 1.0});
        let presets = vec![
            ("a", Some(json!({"mode": "daylight"}))),
            ("b", Some(json!({"mode": "as_shot", "temperature": 6500.0}))),
            ("c", Some(json!({"mode": "custom", "temperature": 4000.0, "tint": 1.0}))),
            ("d", Some(json!({"mode": "daylight", "temperature": 9999.0}))),
            ("e", None),
        ];
        let order = |raw| render_order(presets.clone(), raw, |p| p.1.clone(), &as_shot).into_iter().map(|p| p.0).collect::<String>();
        assert_eq!(order(true), "beadc");
        assert_eq!(order(false), "abcde");
    }

    #[test]
    fn thumbnails_turn_with_the_photograph() {
        // Two pixels side by side, red then blue.
        let rgba = [255, 0, 0, 255, 0, 0, 255, 255];
        let bitmap = Bitmap::from_rgba(&rgba, 2, 1, false, None, 1).unwrap();
        assert_eq!((bitmap.width, bitmap.height), (1, 2));
        // A clockwise turn puts the left (red) pixel on top.
        assert_eq!(&bitmap.bgra[..4], &[0, 0, 255, 255]);
        assert_eq!(&bitmap.bgra[4..], &[255, 0, 0, 255]);
        let plain = Bitmap::from_rgba(&rgba, 2, 1, false, None, 0).unwrap();
        assert_eq!(&plain.bgra[..4], &[0, 0, 255, 255]);
        assert!(Bitmap::from_rgba(&rgba, 3, 1, false, None, 0).is_err());
    }
}
