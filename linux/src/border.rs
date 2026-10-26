//! Presentation geometry mirrored from spektrafilm-io::border and the native clients.
//! The renderer/exporter will remain the authority when the helper is connected.
use gtk::gdk;
use serde::{Deserialize, Serialize};

pub mod asset;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Style {
    #[default]
    Solid,
    Film,
    Keyline,
    Film35,
}
impl Style {
    pub fn index(self) -> u32 {
        match self {
            Self::Solid => 0,
            Self::Film => 1,
            Self::Keyline => 2,
            Self::Film35 => 3,
        }
    }
    pub fn from_index(i: u32) -> Self {
        match i {
            1 => Self::Film,
            2 => Self::Keyline,
            3 => Self::Film35,
            _ => Self::Solid,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Color(pub [u8; 3]);
impl Color {
    pub const WHITE: Self = Self([255; 3]);
    pub const BLACK: Self = Self([0; 3]);
    pub fn rgba(self) -> gdk::RGBA {
        gdk::RGBA::new(
            self.0[0] as f32 / 255.,
            self.0[1] as f32 / 255.,
            self.0[2] as f32 / 255.,
            1.,
        )
    }
    pub fn from_rgba(c: gdk::RGBA) -> Self {
        Self([c.red(), c.green(), c.blue()].map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
    }
    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }
}
impl TryFrom<String> for Color {
    type Error = String;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        let s = value.strip_prefix('#').unwrap_or(&value);
        if s.len() != 6 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Use an sRGB color such as #ffffff".into());
        }
        let n = u32::from_str_radix(s, 16).map_err(|e| e.to_string())?;
        Ok(Self([(n >> 16) as u8, (n >> 8) as u8, n as u8]))
    }
}
impl From<Color> for String {
    fn from(c: Color) -> String {
        c.hex()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Settings {
    pub enabled: bool,
    pub style: Style,
    pub width: f64,
    pub line: f64,
    pub film_width: f64,
    pub color: Color,
    pub line_color: Color,
    pub instagram: bool,
    pub export: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            style: Style::Solid,
            width: 0.03,
            line: 0.004,
            film_width: 0.,
            color: Color::WHITE,
            line_color: Color::BLACK,
            instagram: false,
            export: true,
        }
    }
}
impl<'de> Deserialize<'de> for Settings {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(default)]
        struct Saved {
            enabled: bool,
            style: Style,
            width: f64,
            line: f64,
            film_width: f64,
            color: Color,
            line_color: Color,
            instagram: bool,
            export: bool,
        }
        impl Default for Saved {
            fn default() -> Self {
                let s = Settings::default();
                Self {
                    enabled: s.enabled,
                    style: s.style,
                    width: s.width,
                    line: s.line,
                    film_width: s.film_width,
                    color: s.color,
                    line_color: s.line_color,
                    instagram: s.instagram,
                    export: s.export,
                }
            }
        }
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Data {
            Legacy(u8),
            Current(Saved),
        }
        Ok(match Data::deserialize(d)? {
            Data::Legacy(n) => Self {
                enabled: n != 0,
                style: if n == 2 { Style::Keyline } else { Style::Solid },
                color: if n == 3 { Color::BLACK } else { Color::WHITE },
                ..Self::default()
            },
            Data::Current(s) => Self {
                enabled: s.enabled,
                style: s.style,
                width: s.width.clamp(0., 0.15),
                line: s.line.clamp(0.0005, 0.02),
                film_width: s.film_width.clamp(0., 0.15),
                color: s.color,
                line_color: s.line_color,
                instagram: s.instagram,
                export: s.export,
            },
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    pub frame_width: f64,
    pub frame_height: f64,
    pub inset: f64,
    pub line: f64,
}
pub fn format(width: f64, height: f64) -> Option<&'static str> {
    if width <= 0. || height <= 0. {
        return None;
    }
    let ratio = width.max(height) / width.min(height);
    if (ratio / 1.5 - 1.).abs() <= 0.01 {
        Some("full")
    } else if (ratio / (4. / 3.) - 1.).abs() <= 0.01 {
        Some("half")
    } else {
        None
    }
}
impl Settings {
    pub fn visible(&self, w: f64, h: f64) -> bool {
        self.enabled && (self.style != Style::Film35 || format(w, h).is_some())
    }
    pub fn layout(&self, w: f64, h: f64, strip: Option<(f64, f64, [f64; 4])>) -> Layout {
        let long = w.max(h);
        let line = if self.style == Style::Keyline {
            (long * self.line).round().max(1.)
        } else {
            0.
        };
        let inset = match self.style {
            Style::Solid => (long * self.width).floor(),
            Style::Keyline => (long * self.width).floor() + line,
            Style::Film => (w.min(h) * 0.003).round().max(1.) + (long * self.film_width).floor(),
            Style::Film35 => 0.,
        };
        let (fw, fh) = if self.style == Style::Film35 {
            if let Some((tw, th, window)) = strip {
                let turned = (window[2] * tw > window[3] * th) != (w > h);
                let (tw, th, gw, gh) = if turned {
                    (th, tw, window[3], window[2])
                } else {
                    (tw, th, window[2], window[3])
                };
                let k = (w / (gw * tw)).max(h / (gh * th));
                ((tw * k).round(), (th * k).round())
            } else {
                (w, h)
            }
        } else {
            (w + 2. * inset, h + 2. * inset)
        };
        let (width, height) = if self.instagram {
            let margin = if matches!(self.style, Style::Film | Style::Film35) {
                (fw.max(fh) * 0.03).floor()
            } else {
                0.
            };
            let (w, h) = (fw + 2. * margin, fh + 2. * margin);
            if w * 4. > h * 3. {
                (w, (w * 4. / 3.).ceil())
            } else {
                ((h * 3. / 4.).ceil(), h)
            }
        } else {
            (fw, fh)
        };
        Layout {
            width,
            height,
            frame_width: fw,
            frame_height: fh,
            inset,
            line,
        }
    }
}

pub const FILM_IDS: &[&str] = &[
    "kodak_portra_400",
    "kodak_gold_200",
    "kodak_ektar_100",
    "fujifilm_velvia_100",
    "kodak_trix400_d76_8min",
    "kodak_doublex",
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_preview_sessions_keep_the_selected_border() {
        let old: Settings = serde_json::from_str("3").unwrap();
        assert!(old.enabled);
        assert_eq!(old.color, Color::BLACK);
        let restored: Settings =
            serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        assert_eq!(old, restored);
    }
    #[test]
    fn dimensions_match_existing_export_geometry() {
        let s = Settings {
            enabled: true,
            style: Style::Keyline,
            width: 0.01,
            line: 0.02,
            ..Settings::default()
        };
        let l = s.layout(1000., 500., None);
        assert_eq!((l.width, l.height), (1060., 560.));
        let s = Settings {
            instagram: true,
            ..s
        };
        let l = s.layout(1000., 500., None);
        assert_eq!((l.width, l.height), (1060., 1414.));
        assert!(format(1000., 1000.).is_none());
        assert_eq!(format(600., 400.), Some("full"));
        assert_eq!(format(300., 400.), Some("half"));
    }
}
