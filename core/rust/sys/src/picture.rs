//! Phase 3b: Picture rules: crop, border, exact fit, thumbnail compose, histogram, negative (core/src/rules).

#[allow(unused_imports)]
use super::*;
pub use super::marks::{negs_pt_t, negs_rect_t};
use std::os::raw::c_char;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_size_t {
    pub width: f64,
    pub height: f64,
}

pub const NEGS_CROP_ORIGINAL: u32 = 0;
pub const NEGS_CROP_FREEFORM: u32 = 1;
pub const NEGS_CROP_SQUARE: u32 = 2;
pub const NEGS_CROP_16_9: u32 = 3;
pub const NEGS_CROP_3_2: u32 = 4;
pub const NEGS_CROP_4_3: u32 = 5;
pub const NEGS_CROP_5_4: u32 = 6;
pub const NEGS_CROP_7_5: u32 = 7;
pub const NEGS_CROP_CUSTOM: u32 = 8;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_crop_settings_t {
    pub rect: negs_rect_t,
    pub custom: negs_size_t,
    pub angle: f64,
    pub aspect: u32,
    pub reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_orientation_t {
    pub turns: i32,
    pub flip: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_crop_handle_t {
    pub x: i32,
    pub y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_crop_session_t {
    pub rect: negs_rect_t,
    pub custom: negs_size_t,
    pub photo: negs_size_t,
    pub intended: negs_rect_t,
    pub angle: f64,
    pub rotation: i32,
    pub aspect: u32,
    pub portrait: u8,
    pub flip: u8,
    pub straightening: u8,
    pub has_intended: u8,
    pub reserved: u32,
}

pub const NEGS_BORDER_SOLID: u32 = 0;
pub const NEGS_BORDER_FILM: u32 = 1;
pub const NEGS_BORDER_KEYLINE: u32 = 2;
pub const NEGS_BORDER_FILM35: u32 = 3;
pub const NEGS_FILM_FULL: u32 = 0;
pub const NEGS_FILM_HALF: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_border_settings_t {
    pub width: f64,
    pub film_width: f64,
    pub line: f64,
    pub style: u32,
    pub color: [u8; 3],
    pub line_color: [u8; 3],
    pub enabled: u8,
    pub export_border: u8,
    pub instagram: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct negs_histogram_t {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
}

impl Default for negs_histogram_t {
    fn default() -> Self {
        Self { red: [0; 256], green: [0; 256], blue: [0; 256] }
    }
}

pub const NEGS_NEGATIVE_ROUTE: u32 = 0;
pub const NEGS_NEGATIVE_SCENE_ROUTE: u32 = 1;
pub const NEGS_NEGATIVE_ROUTE_PATH: u32 = 2;
pub const NEGS_NEGATIVE_DEFAULT_FILM: u32 = 3;
pub const NEGS_NEGATIVE_STOCK_ID: u32 = 4;
pub const NEGS_NEGATIVE_NAME: u32 = 5;
pub const NEGS_NEGATIVE_TILE_TEXT: u32 = 6;
pub const NEGS_NEGATIVE_SCAN_ILLUMINANT: u32 = 7;
pub const NEGS_NEGATIVE_FLAT_FIELD: u32 = 8;
pub const NEGS_NEGATIVE_FLAT_FIELD_HELP: u32 = 9;
pub const NEGS_NEGATIVE_OWN_PATHS: u32 = 0;
pub const NEGS_NEGATIVE_FOUND_PATHS: u32 = 1;
pub const NEGS_NEGATIVE_PRESET_KEEPS: u32 = 2;
pub const NEGS_NEGATIVE_SCAN_READING: u32 = 3;
pub const NEGS_PICK_BASE: u32 = 0;
pub const NEGS_PICK_NEUTRAL: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_scan_light_t {
    pub value: [f64; 3],
    pub has: [u8; 3],
    pub reserved: [u8; 5],
}

unsafe extern "C" {
    // Crop
    pub fn negs_crop_max_angle() -> f64;
    pub fn negs_crop_aspect_title(aspect: u32) -> *const c_char;
    pub fn negs_crop_aspect_format(aspect: u32) -> *const c_char;
    pub fn negs_crop_aspect_ratio(aspect: u32, photo: negs_size_t, out: *mut f64) -> u8;
    pub fn negs_crop_aspect_turns(aspect: u32) -> u8;
    pub fn negs_crop_cropped(crop: *const negs_crop_settings_t) -> u8;
    pub fn negs_crop_engine_pixels(crop: *const negs_crop_settings_t, width: u32, height: u32, out: *mut [u32; 4]);
    pub fn negs_crop_straightened_size(crop: *const negs_crop_settings_t, width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_crop_pixels(crop: *const negs_crop_settings_t, size: negs_size_t, out: *mut negs_rect_t);
    pub fn negs_crop_size_of(crop: *const negs_crop_settings_t, size: negs_size_t, out: *mut negs_size_t);
    pub fn negs_crop_shown(rect: negs_rect_t, rotation: i32, flip: u8, out: *mut negs_rect_t);
    pub fn negs_crop_unrotated(rect: negs_rect_t, rotation: i32, flip: u8, out: *mut negs_rect_t);
    pub fn negs_crop_mirrored(rect: negs_rect_t, out: *mut negs_rect_t);
    pub fn negs_rect_inset(rect: negs_rect_t, dx: f64, dy: f64, out: *mut negs_rect_t);
    pub fn negs_rect_contains(rect: negs_rect_t, p: negs_pt_t) -> u8;
    pub fn negs_rect_intersection(a: negs_rect_t, b: negs_rect_t, out: *mut negs_rect_t);
    pub fn negs_orientation_new(turns: i32, flip: u8, out: *mut negs_orientation_t);
    pub fn negs_orientation_flipped_as_shown(o: negs_orientation_t, out: *mut negs_orientation_t);
    pub fn negs_orientation_shown(o: negs_orientation_t, fraction: negs_pt_t, out: *mut negs_pt_t);
    pub fn negs_orientation_unshown(o: negs_orientation_t, fraction: negs_pt_t, out: *mut negs_pt_t);
    pub fn negs_crop_unshown_point(
        o: negs_orientation_t,
        crop: *const negs_crop_settings_t,
        turned: negs_size_t,
        x: f64,
        y: f64,
        out: *mut negs_pt_t,
    ) -> u8;
    pub fn negs_crop_fit(ratio: f64, center: negs_pt_t, bounds: negs_rect_t, out: *mut negs_rect_t);
    pub fn negs_crop_move_rect(rect: negs_rect_t, dx: f64, dy: f64, bounds: negs_rect_t, out: *mut negs_rect_t);
    pub fn negs_crop_resize(
        rect: negs_rect_t,
        handle: negs_crop_handle_t,
        point: negs_pt_t,
        has_ratio: u8,
        ratio: f64,
        bounds: negs_rect_t,
        minimum: f64,
        out: *mut negs_rect_t,
    );
    pub fn negs_crop_handle_at(point: negs_pt_t, frame: negs_rect_t, scale: f64, out: *mut negs_crop_handle_t) -> u8;
    pub fn negs_crop_turn(point: negs_pt_t, degrees: f64, center: negs_pt_t, out: *mut negs_pt_t);
    pub fn negs_crop_corners(photo: negs_size_t, angle: f64, out: *mut [negs_pt_t; 4]);
    pub fn negs_crop_bounds(photo: negs_size_t, angle: f64, out: *mut negs_rect_t);
    pub fn negs_crop_fits(rect: negs_rect_t, angle: f64, photo: negs_size_t) -> u8;
    pub fn negs_crop_constrain(start: negs_rect_t, target: negs_rect_t, angle: f64, photo: negs_size_t, out: *mut negs_rect_t);
    pub fn negs_crop_inscribe(rect: negs_rect_t, angle: f64, photo: negs_size_t, out: *mut negs_rect_t);
    pub fn negs_crop_session_new(start: *const negs_crop_settings_t, photo: negs_size_t, rotation: i32, flip: u8, out: *mut negs_crop_session_t);
    pub fn negs_crop_session_settings(s: *const negs_crop_session_t, out: *mut negs_crop_settings_t);
    pub fn negs_crop_session_modified(s: *const negs_crop_session_t) -> u8;
    pub fn negs_crop_session_orientation_portrait(s: *const negs_crop_session_t) -> u8;
    pub fn negs_crop_session_ratio(s: *const negs_crop_session_t, out: *mut f64) -> u8;
    pub fn negs_crop_session_shown(s: *const negs_crop_session_t, out: *mut negs_rect_t);
    pub fn negs_crop_session_set_shown(s: *mut negs_crop_session_t, value: negs_rect_t);
    pub fn negs_crop_session_bounds(s: *const negs_crop_session_t, out: *mut negs_rect_t);
    pub fn negs_crop_session_update(s: *mut negs_crop_session_t, photo: negs_size_t, rotation: i32, flip: u8);
    pub fn negs_crop_session_choose(s: *mut negs_crop_session_t, aspect: u32);
    pub fn negs_crop_session_orient(s: *mut negs_crop_session_t, portrait: u8);
    pub fn negs_crop_session_set_custom(s: *mut negs_crop_session_t, has_width: u8, width: f64, has_height: u8, height: f64);
    pub fn negs_crop_session_reset(s: *mut negs_crop_session_t);
    pub fn negs_crop_session_begin_straightening(s: *mut negs_crop_session_t);
    pub fn negs_crop_session_straighten(s: *mut negs_crop_session_t, degrees: f64);
    pub fn negs_crop_session_end_straightening(s: *mut negs_crop_session_t);

    // Border
    pub fn negs_border_style_title(style: u32) -> *const c_char;
    pub fn negs_border_style_help(style: u32) -> *const c_char;
    pub fn negs_border_style_raw_value(style: u32) -> *const c_char;
    pub fn negs_border_styles(out: *mut [u32; 4]);
    pub fn negs_border_fits(style: u32, width: u32, height: u32) -> u8;
    pub fn negs_film_format_of(width: u32, height: u32) -> i32;
    pub fn negs_film_format_name(format: u32) -> *const c_char;
    pub fn negs_border_margin(width: u32, height: u32, fraction: f64) -> u32;
    pub fn negs_border_keyline_width(width: u32, height: u32, fraction: f64) -> u32;
    pub fn negs_border_film_geometry(width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_border_instagram_canvas(width: u32, height: u32, style: u32, out: *mut [u32; 2]);
    pub fn negs_strip_turned(format: u32, width: u32, height: u32) -> u8;
    pub fn negs_strip_canvas(format: u32, width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_border_turned(width: u32, height: u32) -> u8;
    pub fn negs_border_color_hex(color: *const [u8; 3], out: *mut [u8; 8]);
    pub fn negs_border_color_from_hex(text: *const u8, len: usize, out: *mut [u8; 3]) -> u8;
    pub fn negs_border_color_rgba(color: *const [u8; 3], out: *mut [f32; 4]);
    pub fn negs_border_shown(border: *const negs_border_settings_t, width: u32, height: u32) -> u8;
    pub fn negs_border_exported(border: *const negs_border_settings_t, width: u32, height: u32) -> u8;
    pub fn negs_border_style_width(border: *const negs_border_settings_t, out: *mut f64) -> u8;
    pub fn negs_border_keyline(border: *const negs_border_settings_t, width: u32, height: u32) -> u32;
    pub fn negs_border_canvas(border: *const negs_border_settings_t, width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_border_instagram(border: *const negs_border_settings_t, width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_border_document(border: *const negs_border_settings_t, width: u32, height: u32, out: *mut [u32; 2]);
    pub fn negs_border_arguments(border: *const negs_border_settings_t) -> negs_buf_t;
    pub fn negs_rust_display_f64(v: f64) -> negs_buf_t;

    // Exact fit
    pub fn negs_fit_target(w: f64, h: f64, z: f64, out: *mut [u32; 2]);
    pub fn negs_fit_worth_it(z: f64, texture_w: u32, texture_h: u32, target_w: u32, target_h: u32) -> u8;
    pub fn negs_fit_coefficients(input: u32, output: u32, starts: *mut u32, counts: *mut u32, weights: *mut f32, weights_cap: usize) -> usize;
    pub fn negs_fit_resample(rgba: *const u8, rgba_len: usize, w: u32, h: u32, tw: u32, th: u32, out: *mut u8, out_len: usize) -> u8;

    // Thumbnail compose
    pub fn negs_compose_reduced_size(width: u32, height: u32, crop_w: f64, crop_h: f64, long_side: u32, out: *mut [u32; 2]) -> u8;
    pub fn negs_compose_to_srgb(rgba: *mut u8, len: usize, m: *const [[f32; 3]; 3]);

    // Histogram
    pub fn negs_histogram_peak(h: *const negs_histogram_t) -> u32;
    pub fn negs_scopes_wave_width(width: usize, height: usize, crop: *const negs_crop_settings_t, o: negs_orientation_t, max_samples: usize) -> usize;
    pub fn negs_scopes_count(
        rgba: *const u8,
        rgba_len: usize,
        width: usize,
        height: usize,
        crop: *const negs_crop_settings_t,
        o: negs_orientation_t,
        max_samples: usize,
        h: *mut negs_histogram_t,
        red: *mut u32,
        green: *mut u32,
        blue: *mut u32,
        wave_len: usize,
    ) -> usize;
    pub fn negs_waveform_raster(width: usize, red: *const u32, green: *const u32, blue: *const u32, len: usize, out: *mut [u8; 4], out_pixels: usize) -> u8;

    // Negative
    pub fn negs_negative_text(which: u32) -> *const c_char;
    pub fn negs_negative_path(list: u32, index: usize) -> *const c_char;
    pub fn negs_negative_scan_stops(frame: negs_scan_light_t, reference: negs_scan_light_t, out: *mut f64) -> u8;
    pub fn negs_negative_exposure_ev(roll: f64, has_stops: u8, stops: f64) -> f64;
    pub fn negs_negative_skipped(count: u64) -> negs_buf_t;
    pub fn negs_negative_is_route(route: *const u8, len: usize) -> u8;
    pub fn negs_negative_readable(paper: u8, positive: u8, bw: u8) -> u8;
    pub fn negs_negative_is_found(path: *const u8, len: usize) -> u8;
    pub fn negs_negative_picture_crop(x0: f64, y0: f64, x1: f64, y1: f64, angle: f64, out: *mut negs_crop_settings_t) -> u8;
    pub fn negs_flat_field_value(path: *const u8, len: usize) -> negs_buf_t;
    pub fn negs_flat_field_hint(
        path: *const u8,
        len: usize,
        used: i32,
        has_reason: u8,
        reason: *const u8,
        reason_len: usize,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_negative_not_negative(raw: u8) -> *const c_char;
    pub fn negs_negative_shown_edge(o: negs_orientation_t, edge: *const u8, len: usize) -> *const c_char;
    pub fn negs_base_status_guessed(found: u8, picked: u8, by_hand: u8, from_roll: u8) -> u8;
    pub fn negs_base_status_sentence(
        found: u8,
        picked: u8,
        by_hand: u8,
        from_roll: u8,
        has_edge: u8,
        edge: *const u8,
        edge_len: usize,
        o: negs_orientation_t,
    ) -> negs_buf_t;
    pub fn negs_pick_prompt(pick: u32) -> *const c_char;
    pub fn negs_negative_same_number(p: f64, q: f64) -> u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_core() {
        let checks: &[(&str, usize)] = &[
            ("negs_size_t", size_of::<negs_size_t>()),
            ("negs_size_t.height", offset_of!(negs_size_t, height)),
            ("negs_crop_settings_t", size_of::<negs_crop_settings_t>()),
            ("negs_crop_settings_t.custom", offset_of!(negs_crop_settings_t, custom)),
            ("negs_crop_settings_t.angle", offset_of!(negs_crop_settings_t, angle)),
            ("negs_crop_settings_t.aspect", offset_of!(negs_crop_settings_t, aspect)),
            ("negs_orientation_t", size_of::<negs_orientation_t>()),
            ("negs_orientation_t.flip", offset_of!(negs_orientation_t, flip)),
            ("negs_crop_handle_t", size_of::<negs_crop_handle_t>()),
            ("negs_crop_handle_t.y", offset_of!(negs_crop_handle_t, y)),
            ("negs_crop_session_t", size_of::<negs_crop_session_t>()),
            ("negs_crop_session_t.custom", offset_of!(negs_crop_session_t, custom)),
            ("negs_crop_session_t.photo", offset_of!(negs_crop_session_t, photo)),
            ("negs_crop_session_t.intended", offset_of!(negs_crop_session_t, intended)),
            ("negs_crop_session_t.angle", offset_of!(negs_crop_session_t, angle)),
            ("negs_crop_session_t.rotation", offset_of!(negs_crop_session_t, rotation)),
            ("negs_crop_session_t.aspect", offset_of!(negs_crop_session_t, aspect)),
            ("negs_crop_session_t.portrait", offset_of!(negs_crop_session_t, portrait)),
            ("negs_crop_session_t.flip", offset_of!(negs_crop_session_t, flip)),
            ("negs_crop_session_t.straightening", offset_of!(negs_crop_session_t, straightening)),
            ("negs_crop_session_t.has_intended", offset_of!(negs_crop_session_t, has_intended)),
            ("negs_border_settings_t", size_of::<negs_border_settings_t>()),
            ("negs_border_settings_t.line", offset_of!(negs_border_settings_t, line)),
            ("negs_border_settings_t.style", offset_of!(negs_border_settings_t, style)),
            ("negs_border_settings_t.color", offset_of!(negs_border_settings_t, color)),
            ("negs_border_settings_t.line_color", offset_of!(negs_border_settings_t, line_color)),
            ("negs_border_settings_t.enabled", offset_of!(negs_border_settings_t, enabled)),
            ("negs_border_settings_t.export_border", offset_of!(negs_border_settings_t, export_border)),
            ("negs_border_settings_t.instagram", offset_of!(negs_border_settings_t, instagram)),
            ("negs_histogram_t", size_of::<negs_histogram_t>()),
            ("negs_histogram_t.blue", offset_of!(negs_histogram_t, blue)),
            ("negs_scan_light_t", size_of::<negs_scan_light_t>()),
            ("negs_scan_light_t.has", offset_of!(negs_scan_light_t, has)),
        ];
        for (name, value) in checks {
            assert_eq!(layout(name), *value as i64, "{name}");
        }
    }
}
