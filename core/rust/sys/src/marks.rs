//! negs_core.h, "Marks".

use std::os::raw::c_char;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_pt_t {
    pub x: f64,
    pub y: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_rect_t {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub const NEGS_SEG_MOVE: u32 = 0;
pub const NEGS_SEG_LINE: u32 = 1;
pub const NEGS_SEG_CUBIC: u32 = 2;
pub const NEGS_SEG_CLOSE: u32 = 3;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_seg_t {
    pub kind: u32,
    pub reserved: u32,
    pub p: [negs_pt_t; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_radii_t {
    pub horizontal: [f64; 4],
    pub vertical: [f64; 4],
    pub relative: u8,
}

pub const NEGS_RADII_BOX: u32 = 0;
pub const NEGS_RADII_NUMBER: u32 = 1;
pub const NEGS_CROSS_LOOSE: u32 = 1;
pub const NEGS_CROSS_TWICE: u32 = 2;
pub const NEGS_BOX_FOUR: u32 = 1;
pub const NEGS_BOX_LOOP: u32 = 2;
pub const NEGS_BOX_TWO: u32 = 3;

#[repr(C)]
#[derive(Debug)]
pub struct negs_strokes_t {
    pub points: *mut negs_pt_t,
    pub counts: *mut u32,
    pub opacities: *mut f64,
    pub stroke_count: usize,
    pub point_count: usize,
    pub kind: u32,
    pub reserved: u32,
    pub tilt: f64,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct negs_marks_constants_t {
    pub pencil_width: f64,
    pub pencil_height: f64,
    pub highlighter_width: f64,
    pub ring_line_width: f64,
    pub ring_outset_w: f64,
    pub ring_outset_h: f64,
    pub star_size: f64,
    pub star_gap: f64,
    pub reach: f64,
}

unsafe extern "C" {
    pub fn negs_marks_constants(out: *mut negs_marks_constants_t);
    pub fn negs_marks_radii(which: u32, out: *mut negs_radii_t);
    pub fn negs_marks_pencil(rect: negs_rect_t, plain: u8, out: *mut negs_seg_t, cap: usize) -> usize;
    pub fn negs_marks_ring(rect: negs_rect_t, radii: *const negs_radii_t, plain: u8, out: *mut negs_seg_t, cap: usize) -> usize;
    pub fn negs_marks_ring_tilt(degrees: f64, plain: u8) -> f64;
    pub fn negs_marks_path_bounds(segs: *const negs_seg_t, count: usize, out: *mut negs_rect_t) -> u8;
    pub fn negs_marks_hash(name: *const c_char, len: usize) -> u32;
    pub fn negs_marks_random_next(state: *mut u32) -> f64;
    pub fn negs_marks_star_seed(name: *const c_char, len: usize, index: u32) -> u32;
    pub fn negs_marks_star(name: *const c_char, len: usize, index: u32) -> negs_strokes_t;
    pub fn negs_marks_cross_kind(name: *const c_char, len: usize) -> u32;
    pub fn negs_marks_cross(w: f64, h: f64, name: *const c_char, len: usize) -> negs_strokes_t;
    pub fn negs_marks_box_kind(name: *const c_char, len: usize) -> u32;
    pub fn negs_marks_pick_box(w: f64, h: f64, outset: f64, name: *const c_char, len: usize, tight: u8, weight: f64) -> negs_strokes_t;
    pub fn negs_marks_pick_box_grid(w: f64, h: f64, name: *const c_char, len: usize) -> negs_strokes_t;
    pub fn negs_marks_pick_box_strip(w: f64, h: f64, name: *const c_char, len: usize) -> negs_strokes_t;
    pub fn negs_marks_pick_box_selection(w: f64, h: f64, name: *const c_char, len: usize) -> negs_strokes_t;
    pub fn negs_strokes_free(strokes: *mut negs_strokes_t);
}
