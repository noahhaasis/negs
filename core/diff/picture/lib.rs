//! Phase 3b's oracles: the frozen windows/src modules (core/diff/oracle),
//! compiled as they are, with stand-ins for the windows crate's modules
//! they name. The stand-ins hold only what the oracles read; `json` is
//! windows/src/json.rs's helpers as they are.
#![allow(dead_code)]

#[path = "../oracle/border.rs"]
pub mod border;
#[path = "../oracle/crop.rs"]
pub mod crop;
#[path = "../oracle/histogram.rs"]
pub mod histogram;
#[path = "../oracle/negative.rs"]
pub mod negative;
#[path = "../oracle/thumb_compose.rs"]
pub mod thumb_compose;

/// windows/src/gpu.rs's matrix the software canvas and thumbnails use.
pub mod gpu {
    /// The shader's `REC2020_TO_SRGB`, row-major, for the software canvas.
    pub const REC2020_TO_SRGB: [[f32; 3]; 3] = [
        [1.6603176, -0.5875727, -0.0729166],
        [-0.1244067, 1.1328007, -0.0083489],
        [-0.0181114, -0.1005965, 1.1187665],
    ];
}

/// windows/src/json.rs's helpers.
pub mod json {
    use serde_json::{Map, Value};

    pub fn get<'a>(value: &'a Value, path: &str) -> &'a Value {
        static NULL: Value = Value::Null;
        let mut current = value;
        for key in path.split('.') {
            current = match current {
                Value::Object(map) => map.get(key).unwrap_or(&NULL),
                Value::Array(items) => key.parse::<usize>().ok().and_then(|i| items.get(i)).unwrap_or(&NULL),
                _ => &NULL,
            };
        }
        current
    }

    pub fn set(value: &mut Value, path: &str, new: Value) {
        let keys: Vec<&str> = path.split('.').collect();
        set_at(value, &keys, new);
    }

    fn set_at(value: &mut Value, keys: &[&str], new: Value) {
        let Some((key, rest)) = keys.split_first() else {
            *value = new;
            return;
        };
        match value {
            Value::Object(map) => {
                let child = map.entry(key.to_string()).or_insert_with(|| Value::Object(Map::new()));
                set_at(child, rest, new);
            }
            Value::Array(items) => {
                if let Some(child) = key.parse::<usize>().ok().and_then(|i| items.get_mut(i)) {
                    set_at(child, rest, new);
                }
            }
            _ => {}
        }
    }

    pub fn number(value: &Value) -> f64 {
        value.as_f64().unwrap_or(0.0)
    }

    pub fn boolean(value: &Value) -> bool {
        value.as_bool().unwrap_or(false)
    }

    pub fn string(value: &Value) -> &str {
        value.as_str().unwrap_or("")
    }
}

/// The parts of windows/src/state.rs the oracles read.
pub mod state {
    use crate::border::BorderSettings;
    use crate::crop::CropSettings;
    use crate::json;
    use serde_json::Value;

    #[derive(Clone, Debug, Default)]
    pub struct EditState {
        pub params: Value,
        pub film: String,
        pub paper: String,
        pub border: BorderSettings,
        pub crop: CropSettings,
        pub brightness_match: Option<()>,
        pub roll: Option<Roll>,
    }

    impl EditState {
        pub fn value(&self, path: &str) -> &Value {
            json::get(&self.params, path)
        }
        pub fn rotation(&self) -> i32 {
            json::number(self.value("io.rotate_cw")) as i32
        }
        pub fn flip(&self) -> bool {
            self.value("io.flip").as_bool().unwrap_or(false)
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    pub struct Roll {
        pub id: String,
        pub magenta: f64,
        pub yellow: f64,
    }

    impl Roll {
        pub fn filtered(&self, state: &EditState) -> bool {
            state.value("enlarger.m_filter_shift").as_f64() == Some(self.magenta) && state.value("enlarger.y_filter_shift").as_f64() == Some(self.yellow)
        }
    }

    pub struct Stock {
        pub paper: bool,
        pub positive: bool,
        pub bw: bool,
    }
}
