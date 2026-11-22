//! Phase 3c: Media helpers: embedded previews, photo facts, tone graph, help (core/src/rules).

#[allow(unused_imports)]
use super::*;

// Embedded previews.

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_jpeg_t {
    pub offset: u64,
    pub length: u64,
    pub width: u32,
    pub height: u32,
}

#[repr(C)]
#[derive(Debug)]
pub struct negs_previews_t {
    pub jpegs: *mut negs_jpeg_t,
    pub count: usize,
    pub orientation: u16,
    pub has_orientation: u8,
    pub reserved: [u8; 5],
}

impl Default for negs_previews_t {
    fn default() -> Self {
        Self { jpegs: std::ptr::null_mut(), count: 0, orientation: 0, has_orientation: 0, reserved: [0; 5] }
    }
}

// Photo facts.

pub const NEGS_FACTS_NONE: u8 = 0;
pub const NEGS_FACTS_SOME: u8 = 1;
pub const NEGS_FACTS_PANICS: u8 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_photo_facts_t {
    pub camera: *const u8,
    pub camera_len: usize,
    pub has_camera: u8,
    pub has_iso: u8,
    pub has_exposure: u8,
    pub has_aperture: u8,
    pub has_focal_length: u8,
    pub reserved: [u8; 3],
    pub iso: f64,
    pub exposure: f64,
    pub aperture: f64,
    pub focal_length: f64,
}

impl Default for negs_photo_facts_t {
    fn default() -> Self {
        Self {
            camera: std::ptr::null(),
            camera_len: 0,
            has_camera: 0,
            has_iso: 0,
            has_exposure: 0,
            has_aperture: 0,
            has_focal_length: 0,
            reserved: [0; 3],
            iso: 0.0,
            exposure: 0.0,
            aperture: 0.0,
            focal_length: 0.0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_exif_facts_t {
    pub make: *const u8,
    pub make_len: usize,
    pub model: *const u8,
    pub model_len: usize,
    pub has_make: u8,
    pub has_model: u8,
    pub has_iso_speed_ratings: u8,
    pub has_iso_speed: u8,
    pub has_exposure_time: u8,
    pub has_fnumber: u8,
    pub has_focal_length: u8,
    pub reserved: u8,
    pub iso_speed_ratings: u16,
    pub reserved2: u16,
    pub iso_speed: u32,
    pub exposure_time: [u32; 2],
    pub fnumber: [u32; 2],
    pub focal_length: [u32; 2],
}

// Tone graph.

pub const NEGS_TONE_MIDDLE: u32 = 0;
pub const NEGS_TONE_SHADOWS: u32 = 1;
pub const NEGS_TONE_HIGHLIGHTS: u32 = 2;
pub const NEGS_TONE_CURVE: u32 = 3;

pub const NEGS_TONE_SET_NONE: u32 = 0;
pub const NEGS_TONE_SET_PRINT_EXPOSURE: u32 = 1;
pub const NEGS_TONE_SET_GAMMA: u32 = 2;
pub const NEGS_TONE_SET_FAST: u32 = 3;
pub const NEGS_TONE_SET_SLOW: u32 = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tone_series_t {
    pub data: *const f64,
    pub len: usize,
}

impl negs_tone_series_t {
    pub fn of(s: &[f64]) -> Self {
        Self { data: s.as_ptr(), len: s.len() }
    }
    pub const EMPTY: Self = Self { data: std::ptr::null(), len: 0 };
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_tone_curve_t {
    pub stops: negs_tone_series_t,
    pub rgb: [negs_tone_series_t; 3],
    pub paper: [negs_tone_series_t; 3],
    pub has_paper: u8,
    pub reserved: [u8; 7],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_tone_controls_t {
    pub print_exposure: f64,
    pub gamma: f64,
    pub fast: f64,
    pub slow: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct negs_tone_geometry_t {
    pub width: f64,
    pub height: f64,
    pub inset: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct negs_tone_point_t {
    pub kind: u32,
    pub reserved: u32,
    pub x: f64,
    pub y: f64,
}

// Help.

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_help_str_t {
    pub data: *const u8,
    pub len: usize,
}

impl Default for negs_help_str_t {
    fn default() -> Self {
        Self { data: std::ptr::null(), len: 0 }
    }
}

unsafe extern "C" {
    pub fn negs_embedded_find_bytes(data: *const u8, len: usize, out: *mut negs_previews_t) -> u8;
    pub fn negs_embedded_find(path: *const u8, path_len: usize, out: *mut negs_previews_t) -> u8;
    pub fn negs_previews_free(previews: *mut negs_previews_t);
    pub fn negs_embedded_bytes(path: *const u8, path_len: usize, offset: u64, length: u64, out: *mut negs_buf_t) -> u8;
    pub fn negs_embedded_read(path: *const u8, path_len: usize, offset: u64, length: u64, dst: *mut u8) -> u8;
    pub fn negs_previews_largest(jpegs: *const negs_jpeg_t, count: usize, out: *mut negs_jpeg_t) -> u8;
    pub fn negs_previews_for_size(jpegs: *const negs_jpeg_t, count: usize, pixels: u32, out: *mut negs_jpeg_t) -> u8;
    pub fn negs_previews_ratio(jpegs: *const negs_jpeg_t, count: usize, orientation: u16, has_orientation: u8, out: *mut f64) -> u8;

    pub fn negs_photo_facts_camera(
        make: *const u8,
        make_len: usize,
        has_make: u8,
        model: *const u8,
        model_len: usize,
        has_model: u8,
        out: *mut negs_buf_t,
    ) -> u8;
    pub fn negs_photo_facts_from_exif(exif: *const negs_exif_facts_t, out: *mut negs_photo_facts_t, camera: *mut negs_buf_t) -> u8;
    pub fn negs_photo_facts_shutter(seconds: f64, decimal: u32) -> negs_buf_t;
    pub fn negs_photo_facts_parts(facts: *const negs_photo_facts_t, decimal: u32, lengths: *mut [usize; 5]) -> negs_buf_t;

    pub fn negs_tone_index_of_decreasing(series: *const f64, len: usize, value: f64) -> f64;
    pub fn negs_tone_index_of_increasing(series: *const f64, len: usize, value: f64) -> f64;
    pub fn negs_tone_interpolate(series: *const f64, len: usize, index: f64) -> f64;
    pub fn negs_tone_luma(r: f64, g: f64, b: f64) -> f64;
    pub fn negs_tone_combine(three: *const [negs_tone_series_t; 3], luma: u8, out: *mut f64);
    pub fn negs_tone_predict(
        curve: *const negs_tone_curve_t,
        controls: negs_tone_controls_t,
        next: negs_tone_controls_t,
        rgb_out: *const [*mut f64; 3],
        paper_out: *const [*mut f64; 3],
    ) -> u8;
    pub fn negs_tone_handles(display: *const negs_tone_curve_t, channel: i32, out: *mut [f64; 3]) -> u8;
    pub fn negs_tone_value(display: *const negs_tone_curve_t, series: *const f64, len: usize, stops: f64) -> f64;
    pub fn negs_tone_x(geometry: negs_tone_geometry_t, stops: f64) -> f64;
    pub fn negs_tone_stops(geometry: negs_tone_geometry_t, x: f64) -> f64;
    pub fn negs_tone_y(geometry: negs_tone_geometry_t, brightness: f64) -> f64;
    pub fn negs_tone_brightness(geometry: negs_tone_geometry_t, y: f64) -> f64;
    pub fn negs_tone_handle_points(
        geometry: negs_tone_geometry_t,
        display: *const negs_tone_curve_t,
        channel: i32,
        out: *mut [negs_tone_point_t; 3],
    ) -> usize;
    pub fn negs_tone_hit(
        geometry: negs_tone_geometry_t,
        display: *const negs_tone_curve_t,
        channel: i32,
        x: f64,
        y: f64,
        radius: f64,
        tolerance: f64,
        kind: *mut u32,
        tone: *mut f64,
    ) -> u8;
    pub fn negs_tone_drag_tone(kind: u32, tone: f64, display: *const negs_tone_curve_t, channel: i32, out: *mut f64) -> u8;
    pub fn negs_tone_drag_value(
        kind: u32,
        tone: f64,
        base: *const negs_tone_curve_t,
        start: negs_tone_controls_t,
        current: negs_tone_controls_t,
        channel: i32,
        geometry: negs_tone_geometry_t,
        x: f64,
        y: f64,
        out: *mut f64,
    ) -> u32;
    pub fn negs_tone_snap(value: f64, low: f64, high: f64, step: f64) -> f64;
    pub fn negs_tone_handle_title(kind: u32) -> *const c_char;
    pub fn negs_tone_handle_hint(kind: u32) -> *const c_char;

    pub fn negs_help_text(
        path: *const u8,
        path_len: usize,
        scan_illuminant: *const u8,
        scan_len: usize,
        grain_fields: *const negs_help_str_t,
        grain_count: usize,
        out: *mut negs_help_str_t,
    ) -> u8;
    pub fn negs_help_group(title: *const u8, title_len: usize, out: *mut negs_help_str_t) -> u8;
    pub fn negs_help_constant(which: u32, out: *mut negs_help_str_t) -> u8;
    pub fn negs_help_row(table: u32, index: usize, key: *mut negs_help_str_t, text: *mut negs_help_str_t) -> u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    #[test]
    fn layouts_match_the_core() {
        macro_rules! size {
            ($t:ident) => {
                assert_eq!(layout(stringify!($t)), size_of::<$t>() as i64, stringify!($t));
            };
        }
        macro_rules! field {
            ($t:ident, $f:ident) => {
                assert_eq!(layout(concat!(stringify!($t), ".", stringify!($f))), offset_of!($t, $f) as i64, concat!(stringify!($t), ".", stringify!($f)));
            };
        }
        size!(negs_jpeg_t);
        field!(negs_jpeg_t, length);
        field!(negs_jpeg_t, width);
        field!(negs_jpeg_t, height);
        size!(negs_previews_t);
        field!(negs_previews_t, count);
        field!(negs_previews_t, orientation);
        field!(negs_previews_t, has_orientation);
        size!(negs_photo_facts_t);
        field!(negs_photo_facts_t, camera_len);
        field!(negs_photo_facts_t, has_camera);
        field!(negs_photo_facts_t, has_focal_length);
        field!(negs_photo_facts_t, iso);
        field!(negs_photo_facts_t, focal_length);
        size!(negs_exif_facts_t);
        field!(negs_exif_facts_t, model);
        field!(negs_exif_facts_t, has_make);
        field!(negs_exif_facts_t, has_focal_length);
        field!(negs_exif_facts_t, iso_speed_ratings);
        field!(negs_exif_facts_t, iso_speed);
        field!(negs_exif_facts_t, exposure_time);
        field!(negs_exif_facts_t, fnumber);
        field!(negs_exif_facts_t, focal_length);
        size!(negs_tone_series_t);
        field!(negs_tone_series_t, len);
        size!(negs_tone_curve_t);
        field!(negs_tone_curve_t, rgb);
        field!(negs_tone_curve_t, paper);
        field!(negs_tone_curve_t, has_paper);
        size!(negs_tone_controls_t);
        field!(negs_tone_controls_t, slow);
        size!(negs_tone_geometry_t);
        field!(negs_tone_geometry_t, inset);
        size!(negs_tone_point_t);
        field!(negs_tone_point_t, x);
        field!(negs_tone_point_t, y);
        size!(negs_help_str_t);
        field!(negs_help_str_t, len);
    }
}
