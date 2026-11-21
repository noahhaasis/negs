//! negs_core.h, "Theme".

use std::os::raw::c_char;

pub const NEGS_THEME_DARKROOM: u32 = 0;
pub const NEGS_THEME_CONTACT_SHEET: u32 = 1;
pub const NEGS_THEME_LIGHT_TABLE: u32 = 2;
pub const NEGS_THEME_DAYLIGHT: u32 = 3;
pub const NEGS_THEME_PLAIN: u32 = 4;
pub const NEGS_THEME_PLAIN_DARK: u32 = 5;

pub const NEGS_PLATFORM_MACOS: u32 = 0;
pub const NEGS_PLATFORM_WINDOWS: u32 = 1;
pub const NEGS_PLATFORM_LINUX: u32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct negs_theme_hex_t {
    pub bg: u32,
    pub panel: u32,
    pub film: u32,
    pub grid: u32,
    pub line: u32,
    pub track: u32,
    pub dim: u32,
    pub label: u32,
    pub paper: u32,
    pub accent: u32,
    pub fill: u32,
    pub on_fill: u32,
    pub warn: u32,
    pub blank: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct negs_theme_t {
    pub hex: negs_theme_hex_t,
    pub film: negs_theme_hex_t,
    pub dark: u8,
    pub highlighter: u8,
    pub plain: u8,
    pub reserved: u8,
    pub id: u32,
    pub key: *const c_char,
    pub title: *const c_char,
}

impl Default for negs_theme_t {
    fn default() -> Self {
        Self {
            hex: Default::default(),
            film: Default::default(),
            dark: 0,
            highlighter: 0,
            plain: 0,
            reserved: 0,
            id: 0,
            key: std::ptr::null(),
            title: std::ptr::null(),
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_theme_constants_t {
    pub highlighter_ink: u32,
    pub crop_shade: u32,
    pub highlighter_opacity: f64,
    pub selection_opacity: f64,
    pub crop_shade_opacity: f64,
    pub disabled_opacity: f64,
    pub strip_dim: f64,
    pub shadow_dark: f64,
    pub shadow_light: f64,
    pub dimming_dark: [f64; 4],
    pub dimming_light: [f64; 4],
}

unsafe extern "C" {
    pub fn negs_theme_count() -> u32;
    pub fn negs_theme_get(id: u32, out: *mut negs_theme_t) -> u8;
    pub fn negs_theme_hint(id: u32, platform: u32) -> *const c_char;
    pub fn negs_theme_parse(text: *const c_char, len: usize) -> i32;
    pub fn negs_theme_luminance(rgb: u32) -> f64;
    pub fn negs_theme_contrast(a: u32, b: u32) -> f64;
    pub fn negs_theme_mix(a: u32, b: u32, t: f64) -> u32;
    pub fn negs_theme_constants(out: *mut negs_theme_constants_t);
}
