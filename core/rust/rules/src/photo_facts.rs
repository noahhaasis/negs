//! What the camera wrote down, for the caption over the photograph: camera,
//! ISO, shutter, aperture and focal length (windows/src/photo_facts.rs's
//! API; `Caption.swift`'s `PhotoFacts`). Reading the EXIF is the engine's
//! (`spektrafilm_io`, the `io` feature) and stays here; what's made of it
//! runs in core/src/rules/facts.

#[cfg(feature = "io")]
use std::path::Path;

use negs_core_sys::media as sys;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhotoFacts {
    pub camera: Option<String>,
    pub iso: Option<f64>,
    /// Seconds.
    pub exposure: Option<f64>,
    pub aperture: Option<f64>,
    /// Millimetres.
    pub focal_length: Option<f64>,
}

/// The EXIF values `PhotoFacts::read` takes from the engine's metadata;
/// rationals are (numerator, denominator).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ExifFacts<'a> {
    pub make: Option<&'a str>,
    pub model: Option<&'a str>,
    pub iso_speed_ratings: Option<u16>,
    pub iso_speed: Option<u32>,
    pub exposure_time: Option<(u32, u32)>,
    pub fnumber: Option<(u32, u32)>,
    pub focal_length: Option<(u32, u32)>,
}

fn text(s: Option<&str>) -> (*const u8, usize, u8) {
    s.map_or((std::ptr::null(), 0, 0), |s| (s.as_ptr(), s.len(), 1))
}

fn rational(r: Option<(u32, u32)>) -> (u8, [u32; 2]) {
    r.map_or((0, [0, 0]), |(n, d)| (1, [n, d]))
}

/// Where the Rust original slices a model off a char boundary.
fn boundary_panic() -> ! {
    panic!("photo_facts: the model's byte index is not a char boundary")
}

impl PhotoFacts {
    /// Reads the file's EXIF (a few ms for a RAW). Off the UI thread.
    #[cfg(feature = "io")]
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = spektrafilm_io::read_metadata(path)?;
        let exif = &metadata.exif;
        Some(Self::from_exif(ExifFacts {
            make: metadata.make.as_deref(),
            model: metadata.model.as_deref(),
            iso_speed_ratings: exif.iso_speed_ratings,
            iso_speed: exif.iso_speed,
            exposure_time: exif.exposure_time.as_ref().map(|r| (r.n, r.d)),
            fnumber: exif.fnumber.as_ref().map(|r| (r.n, r.d)),
            focal_length: exif.focal_length.as_ref().map(|r| (r.n, r.d)),
        }))
    }

    /// What `read` makes of the EXIF's values: ratios with a zero
    /// denominator or not above zero are absent, as is an ISO of 0.
    pub fn from_exif(exif: ExifFacts) -> Self {
        let (make, make_len, has_make) = text(exif.make);
        let (model, model_len, has_model) = text(exif.model);
        let (has_exposure_time, exposure_time) = rational(exif.exposure_time);
        let (has_fnumber, fnumber) = rational(exif.fnumber);
        let (has_focal_length, focal_length) = rational(exif.focal_length);
        let raw = sys::negs_exif_facts_t {
            make,
            make_len,
            model,
            model_len,
            has_make,
            has_model,
            has_iso_speed_ratings: exif.iso_speed_ratings.is_some() as u8,
            has_iso_speed: exif.iso_speed.is_some() as u8,
            has_exposure_time,
            has_fnumber,
            has_focal_length,
            reserved: 0,
            iso_speed_ratings: exif.iso_speed_ratings.unwrap_or(0),
            reserved2: 0,
            iso_speed: exif.iso_speed.unwrap_or(0),
            exposure_time,
            fnumber,
            focal_length,
        };
        let mut out = sys::negs_photo_facts_t::default();
        let mut camera = negs_core_sys::negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        let outcome = unsafe { sys::negs_photo_facts_from_exif(&raw, &mut out, &mut camera) };
        let camera = negs_core_sys::take_buf(camera);
        if outcome == sys::NEGS_FACTS_PANICS {
            boundary_panic();
        }
        let some = |has: u8, v: f64| (has != 0).then_some(v);
        Self {
            camera: (out.has_camera != 0).then(|| String::from_utf8(camera).expect("UTF-8")),
            iso: some(out.has_iso, out.iso),
            exposure: some(out.has_exposure, out.exposure),
            aperture: some(out.has_aperture, out.aperture),
            focal_length: some(out.has_focal_length, out.focal_length),
        }
    }

    /// "Nikon Z f" from "NIKON CORPORATION" and "NIKON Z f"; "Sony ILCE-7M3"
    /// from "SONY" and "ILCE-7M3". The maker's first word, in title case if
    /// it's shouted, then the model without the maker repeated.
    pub fn camera(make: Option<&str>, model: Option<&str>) -> Option<String> {
        let (make, make_len, has_make) = text(make);
        let (model, model_len, has_model) = text(model);
        let mut out = negs_core_sys::negs_buf_t { data: std::ptr::null_mut(), len: 0 };
        let outcome = unsafe { sys::negs_photo_facts_camera(make, make_len, has_make, model, model_len, has_model, &mut out) };
        let bytes = negs_core_sys::take_buf(out);
        match outcome {
            sys::NEGS_FACTS_SOME => Some(String::from_utf8(bytes).expect("UTF-8")),
            sys::NEGS_FACTS_PANICS => boundary_panic(),
            _ => None,
        }
    }

    /// "1/1250" under a second (and any whole fraction), else "2 s" or "1,3 s".
    pub fn shutter(seconds: f64, decimal: char) -> String {
        let buf = unsafe { sys::negs_photo_facts_shutter(seconds, decimal as u32) };
        String::from_utf8(negs_core_sys::take_buf(buf)).expect("UTF-8")
    }

    /// The caption's items by part (`PhotoFacts.parts`), in `compare::PARTS` order.
    pub fn parts(&self, decimal: char) -> [Option<String>; 5] {
        let (camera, camera_len, has_camera) = text(self.camera.as_deref());
        let value = |v: Option<f64>| v.map_or((0, 0.0), |v| (1, v));
        let (has_iso, iso) = value(self.iso);
        let (has_exposure, exposure) = value(self.exposure);
        let (has_aperture, aperture) = value(self.aperture);
        let (has_focal_length, focal_length) = value(self.focal_length);
        let raw = sys::negs_photo_facts_t {
            camera,
            camera_len,
            has_camera,
            has_iso,
            has_exposure,
            has_aperture,
            has_focal_length,
            reserved: [0; 3],
            iso,
            exposure,
            aperture,
            focal_length,
        };
        let mut lengths = [0usize; 5];
        let buf = unsafe { sys::negs_photo_facts_parts(&raw, decimal as u32, &mut lengths) };
        let bytes = negs_core_sys::take_buf(buf);
        let mut at = 0;
        lengths.map(|n| {
            (n != usize::MAX).then(|| {
                let part = String::from_utf8(bytes[at..at + n].to_vec()).expect("UTF-8");
                at += n;
                part
            })
        })
    }

    /// The caption's items, in order.
    pub fn items(&self, decimal: char) -> Vec<String> {
        self.parts(decimal).into_iter().flatten().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cameras_read_as_on_the_mac() {
        assert_eq!(PhotoFacts::camera(Some("NIKON CORPORATION"), Some("NIKON Z f")).as_deref(), Some("Nikon Z f"));
        assert_eq!(PhotoFacts::camera(Some("SONY"), Some("ILCE-7M3")).as_deref(), Some("Sony ILCE-7M3"));
        assert_eq!(PhotoFacts::camera(Some("NIKON CORPORATION"), Some("NIKON D750")).as_deref(), Some("Nikon D750"));
        assert_eq!(PhotoFacts::camera(Some("Canon"), Some("Canon EOS R5")).as_deref(), Some("Canon EOS R5"));
        assert_eq!(PhotoFacts::camera(None, Some("X100V")).as_deref(), Some("X100V"));
        assert_eq!(PhotoFacts::camera(Some("FUJIFILM"), None).as_deref(), Some("Fujifilm"));
        assert_eq!(PhotoFacts::camera(None, None), None);
    }

    #[test]
    fn items_are_formatted_with_the_locales_decimal() {
        let facts = PhotoFacts { camera: Some("Nikon D750".into()), iso: Some(400.0), exposure: Some(1.0 / 160.0), aperture: Some(4.0), focal_length: Some(100.0) };
        assert_eq!(facts.items(','), ["Nikon D750", "ISO 400", "1/160", "f/4", "100 mm"]);
        let facts = PhotoFacts { aperture: Some(2.8), exposure: Some(1.3), focal_length: Some(23.0), ..Default::default() };
        assert_eq!(facts.items(','), ["1,3 s", "f/2,8", "23 mm"]);
        assert_eq!(PhotoFacts::shutter(2.0, '.'), "2 s");
        assert_eq!(PhotoFacts::shutter(0.5, '.'), "1/2");
        assert_eq!(PhotoFacts::shutter(0.4, '.'), "0.4 s");
    }

    #[test]
    fn from_exif_drops_what_isnt_a_value() {
        let facts = PhotoFacts::from_exif(ExifFacts { make: Some("SONY"), iso_speed_ratings: Some(0), iso_speed: Some(800), exposure_time: Some((1, 0)), fnumber: Some((28, 10)), focal_length: Some((0, 1)), ..Default::default() });
        assert_eq!(facts, PhotoFacts { camera: Some("Sony".into()), aperture: Some(2.8), ..Default::default() });
    }
}
