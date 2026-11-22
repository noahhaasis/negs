//! What the camera wrote down, for the caption over the photograph: camera,
//! ISO, shutter, aperture and focal length (`Caption.swift`'s `PhotoFacts`,
//! IMPLEMENTATION.md §4.1).
//!
//! The naming and formatting run in the shared core (core/src/rules/facts,
//! through `negs_core_rules`, feature `io` for `PhotoFacts::read`, which
//! still asks spektrafilm-io for the EXIF); this module keeps its API and
//! tests.

pub use negs_core_rules::photo_facts::*;

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
}
