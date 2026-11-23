// Copied verbatim from b05e49e (windows/src/model.rs, model/library_map.rs,
// engine.rs, sheet_rules.rs, picture.rs): the items the frozen Phase 4
// oracles reach through `crate::` (see lib.rs).

/// windows/src/model.rs (OutputFormat through ExportSize) and
/// model/library_map.rs (LibraryPreferences and its `sidecars`).
pub mod model {
    #![allow(dead_code)]
    use serde_json::{Value, json};

    /// The image format a save or export writes; the engine picks the encoder
    /// from the file extension.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum OutputFormat {
        Png,
        Jpeg,
        Tiff,
    }

    impl OutputFormat {
        pub const ALL: [OutputFormat; 3] = [OutputFormat::Png, OutputFormat::Jpeg, OutputFormat::Tiff];
        pub fn key(self) -> &'static str {
            match self {
                Self::Png => "png",
                Self::Jpeg => "jpeg",
                Self::Tiff => "tiff",
            }
        }
        pub fn from_key(key: &str) -> Self {
            Self::ALL.into_iter().find(|f| f.key() == key).unwrap_or(Self::Png)
        }
        pub fn extension(self) -> &'static str {
            match self {
                Self::Png => "png",
                Self::Jpeg => "jpg",
                Self::Tiff => "tif",
            }
        }
        pub fn title(self) -> &'static str {
            match self {
                Self::Png => "PNG (lossless)",
                Self::Jpeg => "JPEG (smaller)",
                Self::Tiff => "TIFF (lossless)",
            }
        }
        pub fn pattern(self) -> &'static str {
            match self {
                Self::Png => "*.png",
                Self::Jpeg => "*.jpg;*.jpeg",
                Self::Tiff => "*.tif;*.tiff",
            }
        }
    }

    /// The colour spaces a save offers, as colour names them.
    pub const COLOR_SPACES: [&str; 4] = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"];
    /// ProPhoto RGB bands at 8 bits, so it needs 16; Adobe RGB stands in.
    pub const PROPHOTO: usize = 3;
    pub const PROPHOTO_FALLBACK: usize = 2;
    pub const QUALITY_RANGE: (u32, u32) = (1, 100);

    /// How a save encodes the file: bits per channel, colour space and JPEG
    /// quality. The colour space is the one the file is in; ProPhoto RGB at
    /// 8 bits is already replaced by Adobe RGB (`effective_space`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct SaveSettings {
        pub bit_depth: u32,
        pub color_space: usize,
        pub jpeg_quality: u32,
    }

    impl SaveSettings {
        /// The colour space a save with `bit_depth` uses when `chosen` was picked.
        pub fn effective_space(chosen: usize, bit_depth: u32) -> usize {
            let chosen = chosen.min(COLOR_SPACES.len() - 1);
            if chosen == PROPHOTO && bit_depth == 8 { PROPHOTO_FALLBACK } else { chosen }
        }
        pub fn space_name(self) -> &'static str {
            COLOR_SPACES[self.color_space.min(COLOR_SPACES.len() - 1)]
        }
        pub fn is_srgb(self) -> bool {
            self.color_space == 0
        }
        /// The `save_options` of a save command.
        pub fn json(self) -> Value {
            json!({"jpeg_quality": self.jpeg_quality, "bit_depth": self.bit_depth, "color_space": self.space_name()})
        }
        pub fn arguments(self) -> Vec<String> {
            vec![
                "--jpeg-quality".into(), self.jpeg_quality.to_string(),
                "--bit-depth".into(), self.bit_depth.to_string(),
                "--color-space".into(), self.space_name().into(),
            ]
        }
        /// For the save dialog's message; None for sRGB in an sRGB preview. The
        /// preview develops in `preview_space`; afterwards it follows this save.
        pub fn summary(self, export: bool, preview_space: &str) -> Option<String> {
            let space = self.space_name();
            let follows = if space == preview_space { String::new() } else { format!("; the preview then shows {space}") };
            if export {
                return (!self.is_srgb() || !follows.is_empty()).then(|| format!("Develops in {space} and embeds its profile{follows}"));
            }
            if space == preview_space {
                return (!self.is_srgb()).then(|| format!("Keeps the preview's {space} colours and embeds its profile"));
            }
            Some(format!("Converts the {preview_space} preview to {space}, clipping colours outside the smaller space{follows}"))
        }
    }

    /// The size a save or export is fitted to, like `Fit` in
    /// `spektrafilm-io::resize`: full size, or a number of pixels on the long or
    /// short edge of the finished file, border included. Never enlarged.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct ExportSize {
        pub edge: Edge,
        pub pixels: u32,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Edge {
        Full,
        Long,
        Short,
    }

    impl Edge {
        pub const ALL: [Edge; 3] = [Edge::Full, Edge::Long, Edge::Short];
        pub fn title(self) -> &'static str {
            match self {
                Edge::Full => "Full Size",
                Edge::Long => "Long Edge",
                Edge::Short => "Short Edge",
            }
        }
        pub fn key(self) -> &'static str {
            match self {
                Edge::Full => "full",
                Edge::Long => "long",
                Edge::Short => "short",
            }
        }
        pub fn from_key(key: &str) -> Self {
            Self::ALL.into_iter().find(|e| e.key() == key).unwrap_or(Edge::Full)
        }
    }

    pub const PIXEL_RANGE: (u32, u32) = (16, 65_535);

    impl ExportSize {
        pub const FULL: ExportSize = ExportSize { edge: Edge::Full, pixels: 2048 };

        /// The `fit` of a save command; None at full size.
        pub fn json(self) -> Option<Value> {
            match self.edge {
                Edge::Full => None,
                Edge::Long => Some(json!({"long_edge": self.pixels})),
                Edge::Short => Some(json!({"short_edge": self.pixels})),
            }
        }
        pub fn arguments(self) -> Vec<String> {
            match self.edge {
                Edge::Full => vec![],
                Edge::Long => vec!["--long-edge".into(), self.pixels.to_string()],
                Edge::Short => vec!["--short-edge".into(), self.pixels.to_string()],
            }
        }
        /// For the save dialog's message; None at full size.
        pub fn summary(self) -> Option<String> {
            (self.edge != Edge::Full).then(|| format!("Fits {} px on the {} edge", self.pixels, if self.edge == Edge::Long { "long" } else { "short" }))
        }
    }

    /// The library's settings, kept with the app's preferences under the Mac's
    /// keys: the filter (in `LibraryFilter`'s form), Copy Edits…'s groups and
    /// Advance After Rating.
    #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(default, rename_all = "camelCase")]
    pub struct LibraryPreferences {
        pub library_filter: Option<Value>,
        pub copy_edit_groups: Option<Vec<String>>,
        pub advance_after_rating: bool,
        /// Settings › New photographs › Film and Paper; the table's until chosen.
        pub new_photo_film: Option<String>,
        pub new_photo_paper: Option<String>,
        /// Settings › RAW + JPEG › Show camera JPEGs separately: off, a RAW's
        /// camera JPEG of the same name goes with it as one photograph.
        #[serde(rename = "separateCameraJPEGs")]
        pub separate_camera_jpegs: bool,
        /// Settings › Edits › Save ratings and edits next to photos; on until
        /// turned off.
        #[serde(skip_serializing_if = "Option::is_none")]
        pub sidecars: Option<bool>,
    }

    impl LibraryPreferences {
        pub fn sidecars(&self) -> bool {
            self.sidecars.unwrap_or(true)
        }

    }
}

/// windows/src/engine.rs: EnginePaths and ExportEngine.
pub mod engine {
    #![allow(dead_code)]
    use anyhow::{Result, anyhow};
    use std::path::PathBuf;

    #[derive(Clone, Debug)]
    pub struct EnginePaths {
        pub helper: PathBuf,
        /// The reference exporter, `spektrafilm-f64`.
        pub exporter: PathBuf,
        pub data: PathBuf,
        /// The f32 command-line tool `spektrafilm`, for Fast (GPU) export. The
        /// app still starts without it; only a fast export then fails.
        pub fast_exporter: Option<PathBuf>,
    }

    /// Which engine renders an export. Both use the same parameters, crop and border.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum ExportEngine {
        /// `spektrafilm-f64` on the CPU, the reference implementation.
        Reference,
        /// `spektrafilm` (32-bit) on the GPU through wgpu/Vulkan, like the preview.
        Fast,
    }

    impl ExportEngine {
        pub const ALL: [ExportEngine; 2] = [ExportEngine::Reference, ExportEngine::Fast];

        pub fn title(self) -> &'static str {
            match self {
                Self::Reference => "Reference (CPU, 64-bit)",
                Self::Fast => "Fast (GPU)",
            }
        }

        pub fn key(self) -> &'static str {
            match self {
                Self::Reference => "reference",
                Self::Fast => "fast",
            }
        }

        pub fn from_key(key: &str) -> Self {
            if key == "reference" { Self::Reference } else { Self::Fast }
        }

        /// `SPEKTRAFILM_BACKEND` for the exporter process.
        pub fn backend(self) -> &'static str {
            match self {
                Self::Reference => "cpu",
                Self::Fast => "wgpu",
            }
        }

        pub fn executable(self, paths: &EnginePaths) -> Result<PathBuf> {
            match self {
                Self::Reference => Ok(paths.exporter.clone()),
                Self::Fast => paths.fast_exporter.clone().ok_or_else(|| {
                    anyhow!("Fast (GPU) export needs spektrafilm.exe next to Negs (or SPEKTRAFILM_F32_CLI). Export with Reference (CPU, 64-bit) instead, or install Negs again.")
                }),
            }
        }

        /// The exporter prints the backend that actually rendered, after the
        /// render. A fast export that didn't fit the GPU reports `CPU (rayon)`.
        pub fn ran_on_gpu_log(log: &str) -> bool {
            log.contains("Backend: wgpu")
        }

        /// Whether the helper's reported backend is the wgpu GPU backend.
        pub fn ran_on_gpu(backend: &str) -> bool {
            backend.starts_with("wgpu")
        }
    }

}

/// windows/src/sheet_rules.rs: the grid's default size.
pub mod sheet_rules {
    pub const GRID_DEFAULT: f64 = 176.0;
}

/// windows/src/picture.rs: the wide display space.
pub mod picture {
    pub const WIDE_DISPLAY_SPACE: &str = "Rec. 2020 (sRGB transfer)";
}
