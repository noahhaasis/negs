//! `NEGS_TRACE_UI=<file>`: the UI thread's time per mouse-drag step and per
//! render, in microseconds, one line each ("move 812", "render 2304"), to
//! find what holds a frame back. Nothing is written without the variable.

use std::io::Write;
use std::sync::Mutex;
use std::time::Instant;

static FILE: Mutex<Option<Option<std::io::BufWriter<std::fs::File>>>> = Mutex::new(None);

pub fn on() -> bool {
    let mut f = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if f.is_none() {
        *f = Some(std::env::var_os("NEGS_TRACE_UI").and_then(|p| std::fs::File::create(p).ok()).map(std::io::BufWriter::new));
    }
    f.as_ref().is_some_and(|f| f.is_some())
}

pub fn record(what: &str, since: Instant) {
    let mut f = FILE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(Some(w)) = f.as_mut() {
        let _ = writeln!(w, "{what} {}", since.elapsed().as_micros());
        let _ = w.flush();
    }
}
