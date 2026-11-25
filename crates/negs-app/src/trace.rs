//! Pure JSONL render timing trace. Off unless `NEGS_RENDER_TRACE=<file>` is
//! set, costing one atomic load per event when off.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use serde_json::{Value, json};

use crate::model::FrameTag;

static TRACE: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

fn file() -> Option<&'static Mutex<std::fs::File>> {
    TRACE
        .get_or_init(|| {
            let path = std::env::var_os("NEGS_RENDER_TRACE")?;
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).ok()?;
            let start = json!({
                "event": "start",
                "qpc": crate::platform::monotonic_ticks(),
                "qpc_frequency": crate::platform::ticks_per_second(),
                "pid": std::process::id(),
            });
            let _ = writeln!(file, "{start}");
            Some(Mutex::new(file))
        })
        .as_ref()
}

/// Whether `NEGS_RENDER_TRACE` is set.
pub fn enabled() -> bool {
    file().is_some()
}

/// Appends an event when tracing; `fields` (an object) runs only then.
pub fn event(name: &str, fields: impl FnOnce() -> Value) {
    let Some(file) = file() else { return };
    let qpc = crate::platform::monotonic_ticks();
    let mut line = fields();
    if !line.is_object() {
        line = json!({"value": line});
    }
    line["event"] = json!(name);
    line["qpc"] = json!(qpc);
    let mut text = line.to_string();
    text.push('\n');
    if let Ok(mut file) = file.lock() {
        let _ = file.write_all(text.as_bytes());
    }
}

pub fn frame_json(frame: Option<FrameTag>) -> Value {
    match frame {
        Some(f) => json!({"request": f.request, "revision": f.revision, "generation": f.generation, "kind": f.kind.name()}),
        None => Value::Null,
    }
}

/// Whether the host UI's render benchmark is currently driving the app.
pub fn bench_running() -> bool {
    crate::host::bench_running()
}
