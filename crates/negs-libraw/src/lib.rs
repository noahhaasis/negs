//! LibRaw (vendor/libraw), for the sensor data of RAW files rawler can't
//! decode: unpacks the CFA mosaic and reads the metadata a develop needs.
//! LibRaw's own processing (demosaic, colour, output) isn't used.

use std::ffi::{CStr, c_char, c_int, c_void};

#[repr(C)]
struct Info {
    width: u32,
    height: u32,
    raw_width: u32,
    raw_height: u32,
    top_margin: u32,
    left_margin: u32,
    cfa_width: u32,
    cfa_height: u32,
    cfa: [u8; 36],
    colors: u32,
    black: [f32; 36],
    white: u32,
    cam_mul: [f32; 4],
    cam_xyz: [[f32; 3]; 4],
    make: [c_char; 64],
    model: [c_char; 64],
    flip: i32,
    crop_left: u32,
    crop_top: u32,
    crop_width: u32,
    crop_height: u32,
    decoder: [c_char; 64],
}

unsafe extern "C" {
    fn negs_libraw_new() -> *mut c_void;
    fn negs_libraw_free(handle: *mut c_void);
    fn negs_libraw_strerror(code: c_int) -> *const c_char;
    fn negs_libraw_version() -> *const c_char;
    fn negs_libraw_unpack(handle: *mut c_void, data: *const c_void, size: usize, info: *mut Info) -> c_int;
    fn negs_libraw_copy(handle: *mut c_void, out: *mut u16, whole: c_int) -> c_int;
}

/// LibRaw's version, e.g. "0.22.0-Devel202611".
pub fn version() -> String {
    unsafe { CStr::from_ptr(negs_libraw_version()) }.to_string_lossy().into_owned()
}

/// Why LibRaw didn't give a mosaic.
#[derive(Debug)]
pub enum Error {
    /// The file isn't one LibRaw reads, or not a CFA mosaic (Foveon, linear
    /// or 4-colour data): another decoder may take it.
    Unsupported(String),
    /// LibRaw read the file but it's damaged, or memory ran out.
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Unsupported(m) | Error::Failed(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for Error {}

/// Which part of the sensor [`unpack`] copies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Area {
    /// What LibRaw calls visible: the sensor without its masked margins.
    Visible,
    /// The whole sensor, margins included, as rawler lays it out.
    Whole,
}

/// A RAW's CFA mosaic and what developing it needs. Everything but `data`
/// describes the visible area.
#[derive(Clone, Debug)]
pub struct Mosaic {
    pub width: usize,
    pub height: usize,
    /// The whole sensor, and where the visible area lies on it.
    pub raw_width: usize,
    pub raw_height: usize,
    pub top_margin: usize,
    pub left_margin: usize,
    /// The [`Area`] asked for, row by row.
    pub data: Vec<u16>,
    /// The CFA as rows of colour letters (R, G, B, E), 2×2 or 6×6, from the
    /// visible area's top-left pixel.
    pub cfa: String,
    pub cfa_width: usize,
    pub cfa_height: usize,
    /// Black level per CFA cell, in the order of `cfa`.
    pub black: Vec<f32>,
    pub white: u32,
    /// As-shot white balance multipliers, R G B G2 (G2 may be 0: same as G).
    pub cam_mul: [f32; 4],
    /// XYZ (D65) to camera, one row per colour (R G B, then E or G2).
    pub cam_xyz: [[f32; 3]; 4],
    pub colors: u32,
    pub make: String,
    pub model: String,
    /// The EXIF orientation (1, 3, 6 or 8) LibRaw read.
    pub orientation: u16,
    /// The recommended crop in the mosaic, left, top, width, height.
    pub crop: Option<[usize; 4]>,
    /// LibRaw's decoder, e.g. "nikon_he_load_raw()".
    pub decoder: String,
}

struct Handle(*mut c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { negs_libraw_free(self.0) }
    }
}

fn text(chars: &[c_char]) -> String {
    let bytes: Vec<u8> = chars.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
    String::from_utf8_lossy(&bytes).trim().to_string()
}

fn strerror(code: c_int) -> String {
    unsafe { CStr::from_ptr(negs_libraw_strerror(code)) }.to_string_lossy().into_owned()
}

// LibRaw error codes (libraw_const.h).
const LIBRAW_FILE_UNSUPPORTED: c_int = -2;
const LIBRAW_NOT_IMPLEMENTED: c_int = -5;
const NOT_A_MOSAIC: c_int = -1000;

/// Unpacks the RAW in `bytes`.
pub fn unpack(bytes: &[u8], area: Area) -> Result<Mosaic, Error> {
    let handle = Handle(unsafe { negs_libraw_new() });
    if handle.0.is_null() {
        return Err(Error::Failed("LibRaw: out of memory".into()));
    }
    let mut info = std::mem::MaybeUninit::<Info>::zeroed();
    let rv = unsafe { negs_libraw_unpack(handle.0, bytes.as_ptr().cast(), bytes.len(), info.as_mut_ptr()) };
    match rv {
        0 => {}
        NOT_A_MOSAIC => return Err(Error::Unsupported("LibRaw: not a CFA mosaic".into())),
        LIBRAW_FILE_UNSUPPORTED | LIBRAW_NOT_IMPLEMENTED => {
            return Err(Error::Unsupported(format!("LibRaw: {}", strerror(rv))));
        }
        _ => return Err(Error::Failed(format!("LibRaw: {}", strerror(rv)))),
    }
    let info = unsafe { info.assume_init() };
    let (width, height) = (info.width as usize, info.height as usize);
    if width == 0 || height == 0 {
        return Err(Error::Failed("LibRaw: empty image".into()));
    }
    let samples = match area {
        Area::Visible => width * height,
        Area::Whole => info.raw_width as usize * info.raw_height as usize,
    };
    let mut data = Vec::new();
    data.try_reserve_exact(samples).map_err(|_| Error::Failed("LibRaw: out of memory".into()))?;
    data.resize(samples, 0);
    if unsafe { negs_libraw_copy(handle.0, data.as_mut_ptr(), (area == Area::Whole) as c_int) } != 0 {
        return Err(Error::Unsupported("LibRaw: not a CFA mosaic".into()));
    }
    let cells = (info.cfa_width * info.cfa_height) as usize;
    let cfa = info.cfa[..cells]
        .iter()
        .map(|&c| match c {
            0 => 'R',
            1 => 'G',
            2 => 'B',
            _ if info.colors == 3 => 'G',
            _ => 'E',
        })
        .collect();
    let orientation = match info.flip {
        3 => 3,
        5 => 8,
        6 => 6,
        _ => 1,
    };
    let crop = (info.crop_width > 0 && info.crop_height > 0)
        .then(|| [info.crop_left, info.crop_top, info.crop_width, info.crop_height].map(|v| v as usize));
    Ok(Mosaic {
        width,
        height,
        raw_width: info.raw_width as usize,
        raw_height: info.raw_height as usize,
        top_margin: info.top_margin as usize,
        left_margin: info.left_margin as usize,
        data,
        cfa,
        cfa_width: info.cfa_width as usize,
        cfa_height: info.cfa_height as usize,
        black: info.black[..cells].to_vec(),
        white: info.white,
        cam_mul: info.cam_mul,
        cam_xyz: info.cam_xyz,
        colors: info.colors,
        make: text(&info.make),
        model: text(&info.model),
        orientation,
        crop,
        decoder: text(&info.decoder),
    })
}
