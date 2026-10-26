use gtk::{gdk, glib, prelude::*};
use serde_json::json;
use std::{cell::RefCell, collections::BTreeMap, path::PathBuf, rc::Rc};

#[derive(Clone)]
pub struct Perf(Rc<Inner>);

struct Inner {
    start: i64,
    marks: RefCell<BTreeMap<String, f64>>,
    path: Option<PathBuf>,
    frames: RefCell<Vec<i64>>,
    slider_samples: RefCell<Vec<f64>>,
    details: RefCell<BTreeMap<String, serde_json::Value>>,
}

impl Perf {
    pub fn new() -> Self {
        let start = std::env::var("NEGS_LAUNCH_MONOTONIC_US")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(glib::monotonic_time);
        Self(Rc::new(Inner {
            start,
            marks: RefCell::new(BTreeMap::new()),
            path: std::env::var_os("NEGS_STARTUP_REPORT").map(PathBuf::from),
            frames: RefCell::new(Vec::new()),
            slider_samples: RefCell::new(Vec::new()),
            details: RefCell::new(BTreeMap::new()),
        }))
    }

    pub fn mark(&self, name: &str) {
        self.mark_at(name, glib::monotonic_time());
    }

    pub fn mark_at(&self, name: &str, when: i64) {
        self.0
            .marks
            .borrow_mut()
            .entry(name.into())
            .or_insert((when - self.0.start) as f64 / 1000.0);
    }

    pub fn watch(&self, window: &impl IsA<gtk::Widget>) {
        let perf = self.clone();
        window.connect_map(move |w| {
            perf.mark("window_mapped_ms");
            if let Some(clock) = w.frame_clock() {
                let perf2 = perf.clone();
                clock.connect_after_paint(move |clock| {
                    perf2.mark("first_frame_submitted_ms");
                    let mut frames = perf2.0.frames.borrow_mut();
                    if frames.len() < 4096 {
                        frames.push(clock.frame_time());
                    }
                });
                let perf2 = perf.clone();
                let first = clock.frame_counter();
                let tries = Rc::new(std::cell::Cell::new(0));
                glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
                    tries.set(tries.get() + 1);
                    for n in first..=clock.frame_counter().min(first + 8) {
                        if let Some(t) = clock.timings(n)
                            && t.is_complete()
                            && t.presentation_time() > 0
                        {
                            perf2.mark_at("first_frame_presented_ms", t.presentation_time());
                            perf2.write();
                            return glib::ControlFlow::Break;
                        }
                    }
                    if tries.get() >= 180 {
                        perf2.write();
                        return glib::ControlFlow::Break;
                    }
                    glib::ControlFlow::Continue
                });
            }
        });
    }

    pub fn input(&self) {
        if self
            .0
            .marks
            .borrow()
            .contains_key("first_input_received_ms")
        {
            return;
        }
        self.mark("first_input_received_ms");
        let p = self.clone();
        glib::idle_add_local_once(move || {
            p.mark("first_input_dispatch_complete_ms");
            p.write();
        });
    }

    pub fn watch_presentation(&self, clock: &gdk::FrameClock, frame: i64, name: &'static str) {
        let clock = clock.clone();
        let perf = self.clone();
        let tries = Rc::new(std::cell::Cell::new(0));
        glib::timeout_add_local(std::time::Duration::from_millis(16), move || {
            tries.set(tries.get() + 1);
            if let Some(t) = clock.timings(frame)
                && t.is_complete()
                && t.presentation_time() > 0
            {
                perf.mark_at(name, t.presentation_time());
                perf.write();
                return glib::ControlFlow::Break;
            }
            if tries.get() >= 180 {
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    }

    pub fn slider_sample(&self, ms: f64) {
        if self.0.path.is_some() {
            self.0.slider_samples.borrow_mut().push(ms);
        }
    }

    pub fn detail(&self, key: &str, value: impl Into<serde_json::Value>) {
        self.0.details.borrow_mut().insert(key.into(), value.into());
    }

    pub fn write(&self) {
        let Some(path) = &self.0.path else {
            return;
        };
        let report = json!({
            "kind": if crate::runtime::ui_only() {"gtk-ui-only"} else {"gtk-spektrafilm"}, "pid": std::process::id(),
            "gtk": format!("{}.{}.{}", gtk::major_version(), gtk::minor_version(), gtk::micro_version()),
            "libadwaita": format!("{}.{}.{}", adw::major_version(), adw::minor_version(), adw::micro_version()),
            "session": std::env::var("XDG_SESSION_TYPE").ok(),
            "renderer_requested": std::env::var("GSK_RENDERER").ok(),
            "display": gdk::Display::default().map(|d| d.type_().name().to_string()),
            "milestones_ms": *self.0.marks.borrow(),
            "frame_clock_times_us": *self.0.frames.borrow(),
            "slider_callback_ms": *self.0.slider_samples.borrow(),
            "details": *self.0.details.borrow(),
            "developed_image_ms": self.0.marks.borrow().get("developed_image_ms").copied(),
            "note": "Compositor presentation feedback is separate from frame submission. Cache and engine state are recorded separately."
        });
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("tmp");
        if let Ok(bytes) = serde_json::to_vec_pretty(&report)
            && std::fs::write(&tmp, bytes).is_ok()
        {
            let _ = std::fs::rename(tmp, path);
        }
    }
}
