//! What the camera wrote down, for the caption over the photograph: camera,
//! ISO, shutter, aperture and focal length (`Caption.swift`'s `PhotoFacts`,
//! IMPLEMENTATION.md §4.1).

use std::path::Path;

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

impl PhotoFacts {
    /// Reads the file's EXIF (a few ms for a RAW). Off the UI thread.
    pub fn read(path: &Path) -> Option<Self> {
        let metadata = spektrafilm_io::read_metadata(path)?;
        let exif = &metadata.exif;
        let ratio = |r: Option<(u32, u32)>| r.filter(|&(_, d)| d != 0).map(|(n, d)| n as f64 / d as f64).filter(|v| *v > 0.0 && v.is_finite());
        let iso = exif.iso_speed_ratings.map(f64::from).or(exif.iso_speed.map(f64::from)).filter(|v| *v > 0.0);
        Some(Self {
            camera: Self::camera(metadata.make.as_deref(), metadata.model.as_deref()),
            iso,
            exposure: ratio(exif.exposure_time.as_ref().map(|r| (r.n, r.d))),
            aperture: ratio(exif.fnumber.as_ref().map(|r| (r.n, r.d))),
            focal_length: ratio(exif.focal_length.as_ref().map(|r| (r.n, r.d))),
        })
    }

    /// "Nikon Z f" from "NIKON CORPORATION" and "NIKON Z f"; "Sony ILCE-7M3"
    /// from "SONY" and "ILCE-7M3". The maker's first word, in title case if
    /// it's shouted, then the model without the maker repeated.
    pub fn camera(make: Option<&str>, model: Option<&str>) -> Option<String> {
        let model = model.map(str::trim).filter(|m| !m.is_empty());
        let Some(brand) = make.and_then(|m| m.split_whitespace().next()) else { return model.map(String::from) };
        let name = if brand == brand.to_uppercase() && brand.chars().count() > 2 {
            let mut chars = brand.chars();
            chars.next().map(|c| c.to_string()).unwrap_or_default() + &chars.as_str().to_lowercase()
        } else {
            brand.to_string()
        };
        let Some(mut model) = model.map(String::from) else { return Some(name) };
        if model.to_lowercase().starts_with(&brand.to_lowercase()) {
            model = model[brand.len()..].trim().to_string();
        }
        Some(if model.is_empty() { name } else { format!("{name} {model}") })
    }

    /// "1/1250" under a second (and any whole fraction), else "2 s" or "1,3 s".
    pub fn shutter(seconds: f64, decimal: char) -> String {
        if seconds < 1.0 {
            let denominator = 1.0 / seconds;
            if seconds < 0.3 || (denominator - denominator.round()).abs() < 0.05 {
                return format!("1/{}", denominator.round() as i64);
            }
        }
        format!("{} s", one_decimal(seconds, decimal))
    }

    /// The caption's items by part (`PhotoFacts.parts`), in `compare::PARTS` order.
    pub fn parts(&self, decimal: char) -> [Option<String>; 5] {
        [
            self.camera.clone(),
            self.iso.map(|iso| format!("ISO {}", iso.round() as i64)),
            self.exposure.map(|exposure| Self::shutter(exposure, decimal)),
            self.aperture.map(|aperture| format!("f/{}", one_decimal(aperture, decimal))),
            self.focal_length.map(|focal| format!("{} mm", one_decimal(focal, decimal))),
        ]
    }

    /// The caption's items, in order.
    pub fn items(&self, decimal: char) -> Vec<String> {
        self.parts(decimal).into_iter().flatten().collect()
    }
}

/// At most one decimal, none when whole ("4", "2,8").
fn one_decimal(value: f64, decimal: char) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.1}").replace('.', &decimal.to_string())
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
}
