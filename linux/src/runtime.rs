//! GTK-side render scheduling. The serial helper and all file IO run on workers.
use crate::{
    Editor,
    engine::{Engine, Frame},
    parameters,
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::{Value, json};
use std::{path::PathBuf, rc::Rc, sync::Arc, time::Duration};

#[derive(Default)]
pub struct Runtime {
    pub client: Option<Arc<Engine>>,
    pub defaults: Value,
    pub ready: bool,
    pub starting: bool,
    pub closing: bool,
    pub rendering: bool,
    pub interacting: bool,
    pub revision: u64,
    pub source_generation: u64,
    pub source: Option<(PathBuf, crate::filmstrip::Stamp)>,
    pub desired: Value,
    pub requested_revision: u64,
    pub current_revision: Option<u64>,
    pub retained_id: Option<u64>,
    pub developed: Option<Arc<Frame>>,
    pub original: Option<Arc<Frame>>,
    pub showing_original: bool,
    pub original_loading: bool,
    pub hist_generation: u64,
    pub exporting: Option<Arc<crate::engine::Job>>,
    pub preview_saving: bool,
    pub block_helper_render: bool,
    pub close_confirmed: bool,
    pub cached_restoring: bool,
    pub original_key: Value,
    pub histogram_crop: Value,
    pub interactive_frames: u64,
}
pub fn ui_only() -> bool {
    std::env::var_os("NEGS_UI_ONLY").is_some()
}

impl Editor {
    pub fn start_engine(self: &Rc<Self>) {
        if ui_only() {
            self.maybe_build_inspector();
            return;
        }
        if self.runtime.borrow().starting {
            return;
        }
        self.runtime.borrow_mut().starting = true;
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let setup = gio::spawn_blocking(|| {
                let engine = Arc::new(Engine::new()?);
                let (_, catalog) =
                    engine.request(json!({"command":"catalog"}), Duration::from_secs(60))?;
                Ok::<_, String>((engine, catalog))
            })
            .await;
            ui.runtime.borrow_mut().starting=false;
            if ui.runtime.borrow().closing {
                return;
            }
            match setup {
                Ok(Ok((client, catalog))) => {
                    let result = (|| {
                        if catalog["protocol"] != 1 || !catalog["defaults"].is_object() {
                            return Err(
                                "The image engine version does not match this application.".into(),
                            );
                        }
                        crate::catalog::install(&catalog)?;
                        parameters::validate(&catalog["defaults"])?;
                        if catalog["capabilities"]["save_options"] != true || catalog["capabilities"]["crop_after_post"] != true {
                            return Err("The installed image helper is older than this Linux editor. Reinstall the matching Negs package.".into());
                        }
                        Ok::<_, String>(())
                    })();
                    if let Err(error) = result {
                        client.stop();
                        ui.engine_error(&error);
                        return;
                    }
                    let defaults = catalog["defaults"].clone();
                    {
                        let mut h = ui.history.borrow_mut();
                        parameters::install_defaults(&mut h.state, &defaults, false);
                        h.state.params = parameters::merge(&defaults, &h.state.params);
                    }
                    {
                        let mut runtime = ui.runtime.borrow_mut();
                        runtime.client = Some(client);
                        runtime.defaults = defaults;
                        runtime.ready = true;
                    }
                    ui.perf.mark("engine_catalog_ready_ms");
                    ui.maybe_build_inspector();
                    ui.sync_state();
                }
                Ok(Err(error)) => ui.engine_error(&error),
                Err(_) => ui.engine_error(
                    "The image worker stopped unexpectedly. Open the photograph again to retry.",
                ),
            }
        });
    }
    pub fn maybe_build_inspector(self: &Rc<Self>) {
        if self.shell_painted.get()
            && self.tabs.borrow().is_none()
            && (ui_only() || self.runtime.borrow().ready)
        {
            crate::inspector::build(self);
            self.sync_state();
        }
    }
    pub fn engine_error(&self, message: &str) {
        eprintln!("Engine: {message}");
        if !ui_only() {
            self.badge.set_visible(false);
        }
        self.status.set_text("Could not complete the operation");
        let dialog = adw::AlertDialog::builder()
            .heading("Negs")
            .body(message)
            .build();
        dialog.add_response("close", "Close");
        dialog.present(Some(&self.window));
    }
    /// Called after edits and source changes. Presentation-only changes reuse
    /// the float render; Image Size includes only the crop's extent in its key.
    pub fn observe_edit(&self) {
        if ui_only() || self.runtime.borrow().closing || !self.runtime.borrow().ready {
            return;
        }
        let Some(source) = self.current_path.borrow().clone() else {
            return;
        };
        let state = self.history.borrow().state.clone();
        let stamp = self
            .current_stamp
            .get()
            .unwrap_or_else(|| crate::filmstrip::stamp(&source));
        let mut r = self.runtime.borrow_mut();
        let params = parameters::runtime(&state, &r.defaults);
        let film = crate::catalog::selected(&state, false).id.clone();
        let paper = crate::catalog::selected(&state, true).id.clone();
        let extent = (!params["io"]["photo_long_side_px"].is_null())
            .then(|| state.crop.map(|c| [c[2], c[3]]));
        let desired = json!({"params":params,"film":film,"paper":paper,"limit_preview":crate::inspector::flag(&state,"Limit preview to 12 MP",false),"extent":extent,"interactive":r.interacting});
        let identity = Some((source.clone(), stamp));
        if r.source != identity {
            r.source = identity;
            r.source_generation += 1;
            r.hist_generation += 1;
            r.original = None;
            r.original_loading = false;
            r.developed = None;
            r.current_revision = None;
            r.showing_original = false;
            r.desired = Value::Null;
        }
        let original_key = json!([params["io"]["raw_white_balance"], desired["limit_preview"]]);
        let release_original = r.showing_original && r.original_key != original_key;
        if r.original_key != original_key {
            r.original_key = original_key;
            r.original = None;
            r.original_loading = false;
        }
        let histogram_crop = json!([state.crop, state.straighten]);
        let histogram_changed = r.histogram_crop != histogram_crop;
        r.histogram_crop = histogram_crop;
        if r.desired == desired {
            drop(r);
            if release_original && let Some(ui) = self.owner.borrow().upgrade() {
                ui.show_original(false);
            }
            if histogram_changed && let Some(ui) = self.owner.borrow().upgrade() {
                ui.update_engine_histogram();
            }
            return;
        }
        if !(r.cached_restoring && r.desired.is_null()) {
            r.cached_restoring = false;
            self.cache.invalidate();
        }
        r.desired = desired;
        r.revision += 1;
        r.retained_id = None;
        drop(r);
        if let Some(ui) = self.owner.borrow().upgrade() {
            if release_original {
                ui.show_original(false);
            }
            glib::idle_add_local_once(move || ui.render_next());
        }
    }
    pub(crate) fn render_next(self: &Rc<Self>) {
        let state = self.history.borrow().state.clone();
        let mut r = self.runtime.borrow_mut();
        if !r.ready
            || r.closing
            || r.rendering
            || r.block_helper_render
            || r.requested_revision == r.revision
        {
            return;
        }
        let (Some(client), Some((source, _))) = (r.client.clone(), r.source.clone()) else {
            return;
        };
        let (revision, generation) = (r.revision, r.source_generation);
        let desired = r.desired.clone();
        let cache_defaults = r.defaults.clone();
        let cache_state = state.clone();
        let cache_source = source.clone();
        let cache_paths = client.paths.clone();
        let buffer = client
            .scratch
            .directory
            .join(format!("preview-{revision}.rgba"));
        let interactive = r
            .interacting
            .then(|| self.canvas.interactive_scale().min(1.) as f32);
        let mut command = json!({"command":"render","input":source,"source_generation":generation,
            "film":desired["film"],"paper":desired["paper"],"params":desired["params"],
            "limit_preview":desired["limit_preview"],"preview_path":buffer});
        if let Some(scale) = interactive {
            command["interactive_scale"] = json!(scale);
        }
        if !desired["params"]["io"]["photo_long_side_px"].is_null() {
            command["crop"] = parameters::crop(&state);
        }
        r.rendering = true;
        r.requested_revision = revision;
        drop(r);
        self.sync_history();
        let ui = self.clone();
        glib::timeout_add_local_once(
            Duration::from_millis(400),
            glib::clone!(
                #[weak]
                ui,
                move || {
                    let r = ui.runtime.borrow();
                    if r.rendering
                        && r.requested_revision == revision
                        && !r.interacting
                        && !r.cached_restoring
                    {
                        ui.status.set_text(if r.developed.is_some() {
                            "Updating…"
                        } else {
                            "Developing…"
                        });
                        ui.badge.set_label(if r.developed.is_some() {
                            "Updating…"
                        } else {
                            "Developing…"
                        });
                        ui.badge.set_visible(true);
                    }
                }
            ),
        );
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let cache_key=if interactive.is_none(){crate::preview_cache::key(&cache_source,&cache_state,&cache_defaults,&cache_paths)}else{None};
                let rendered = client.request(command,Duration::from_secs(180)).and_then(|(id,result)|Frame::read(&buffer,result).map(|frame|(id,Arc::new(frame),cache_key)));
                let _ = std::fs::remove_file(&buffer);
                rendered
            }).await;
            let mut r = ui.runtime.borrow_mut();
            r.rendering = false;
            if r.closing { return; }
            let current = generation == r.source_generation && (revision == r.revision || r.interacting);
            match result {
                Ok(Ok((id,frame,cache_key))) if current => {
                    r.developed = Some(frame.clone());
                    if interactive.is_none() && revision == r.revision { r.retained_id = Some(id); r.current_revision = Some(revision); r.cached_restoring=false;ui.perf.mark("saveable_render_ready_ms");if let Some(key)=cache_key {ui.cache.save(key,frame.clone());} }
                    if interactive.is_some() { r.interactive_frames += 1; ui.perf.detail("interactive_frames_presented", r.interactive_frames); }
                    let show = !r.showing_original;
                    drop(r);
                    if show { ui.present_engine_frame(&frame,true); }
                    ui.update_engine_histogram();
                }
                Ok(Ok(_)) => { drop(r); }
                Ok(Err(error)) if current => { r.retained_id = None; drop(r); ui.engine_error(&error); }
                Err(_) if current => { r.retained_id = None; drop(r); ui.engine_error("The image worker stopped unexpectedly. Open the photograph again to retry."); }
                _ => { drop(r); }
            }
            ui.sync_history();
            ui.render_next();
        });
    }
    fn present_engine_frame(self: &Rc<Self>, frame: &Frame, developed: bool) {
        let texture = crate::canvas::srgb_texture(
            frame.width,
            frame.height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(frame.pixels.clone()),
            frame.width as usize * 4,
        );
        let dimension = |key: &str, fallback: i32| {
            frame.result[key]
                .as_i64()
                .and_then(|v| i32::try_from(v).ok())
                .filter(|v| *v > 0)
                .unwrap_or(fallback)
        };
        let source = (
            dimension("source_width", frame.width),
            dimension("source_height", frame.height),
        );
        let photo = (
            dimension("photo_width", frame.width),
            dimension("photo_height", frame.height),
        );
        self.canvas
            .update_texture(texture.upcast_ref(), source, photo);
        if let Some(view) = self.pending_view.take() {
            self.canvas.restore_view(&view);
        }
        if developed {
            self.badge.set_visible(false);
            let state = self.history.borrow().state.clone();
            let film = crate::catalog::selected(&state, false);
            let paper = crate::catalog::selected(&state, true);
            self.title.set_subtitle(
                &if crate::inspector::flag(&state, "Scan film directly", false) {
                    film.name.clone()
                } else {
                    format!("{} → {}", film.name, paper.name)
                },
            );
            let width = dimension("working_width", photo.0);
            let height = dimension("working_height", photo.1);
            let crop = state.crop.unwrap_or([0., 0., 1., 1.]);
            self.dimensions.set_text(&format!(
                "{} × {} px",
                (width as f64 * crop[2]).round() as i64,
                (height as f64 * crop[3]).round() as i64
            ));
            let backend = frame.result["backend"]
                .as_str()
                .unwrap_or("unknown backend");
            let milliseconds = frame.result["elapsed_ms"].as_f64().unwrap_or(0.);
            self.status.set_text(&format!(
                "Developed in {:.2} s · {backend}",
                milliseconds / 1000.
            ));
            self.perf.detail("engine_backend", backend);
            self.perf.mark("developed_texture_ready_ms");
            if let Some(clock) = self.window.frame_clock() {
                let perf = self.perf.clone();
                let handler = Rc::new(std::cell::RefCell::new(None));
                let pending = handler.clone();
                let id = clock.connect_after_paint(move |clock| {
                    perf.watch_presentation(clock, clock.frame_counter(), "developed_image_ms");
                    if let Some(id) = pending.borrow_mut().take() {
                        clock.disconnect(id);
                    }
                });
                handler.replace(Some(id));
            }
        }
    }
    pub fn show_original(self: &Rc<Self>, show: bool) {
        if ui_only() {
            if show {
                self.toast.add_toast(adw::Toast::new(
                    "The camera original is already displayed in the UI preview.",
                ));
            }
            return;
        }
        let mut r = self.runtime.borrow_mut();
        if !show && !r.showing_original {
            return;
        }
        if r.developed.is_none() || self.canvas.cropping() {
            return;
        }
        r.showing_original = show;
        let frame = if show {
            r.original.clone()
        } else {
            r.developed.clone()
        };
        if let Some(frame) = frame {
            // Original pixels can be reused after Image Size changes, but its
            // source/working geometry must follow the current developed frame.
            let context = r.developed.as_ref().map(|f| f.result.clone());
            drop(r);
            if show && let Some(context) = context {
                self.present_engine_frame(
                    &Frame {
                        width: frame.width,
                        height: frame.height,
                        pixels: frame.pixels.clone(),
                        result: context,
                    },
                    false,
                );
            } else {
                self.present_engine_frame(&frame, !show);
            }
            self.update_engine_histogram();
            if show {
                self.badge.set_label("Original");
                self.badge.set_visible(true);
            } else {
                self.badge.set_visible(false);
            }
            return;
        }
        if r.original_loading {
            return;
        }
        let (Some(client), Some((source, _))) = (r.client.clone(), r.source.clone()) else {
            return;
        };
        let generation = r.source_generation;
        let original_key = r.original_key.clone();
        let context = r.developed.as_ref().unwrap().result.clone();
        let params = parameters::runtime(&self.history.borrow().state, &r.defaults);
        let limit = r.desired["limit_preview"].clone();
        r.original_loading = true;
        drop(r);
        self.badge.set_label("Loading Original…");
        self.badge.set_visible(true);
        let buffer = client
            .scratch
            .directory
            .join(format!("original-{}.rgba", crate::engine::unique()));
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let frame = client.request(json!({"command":"original","input":source,"params":params,"limit_preview":limit,"preview_path":buffer}),Duration::from_secs(60))
                    .and_then(|(_,result)|{
                        let mut combined = context;
                        combined["width"] = result["width"].clone(); combined["height"] = result["height"].clone();
                        Frame::read(&buffer,combined).map(Arc::new)
                    });
                let _ = std::fs::remove_file(buffer); frame
            }).await;
            let mut r = ui.runtime.borrow_mut();
            if r.closing || r.source_generation != generation || r.original_key!=original_key {return;}
            r.original_loading = false;
            match result {
                Ok(Ok(frame)) => {r.original = Some(frame);let show=r.showing_original;drop(r);if show {ui.show_original(true);}}
                Ok(Err(error)) => {drop(r);ui.show_original(false);ui.engine_error(&error);}
                Err(_) => {drop(r);ui.show_original(false);ui.engine_error("Could not load the original photograph.");}
            }
        });
    }
    pub fn update_engine_histogram(self: &Rc<Self>) {
        let state = self.history.borrow().state.clone();
        let mut r = self.runtime.borrow_mut();
        let Some(frame) = (if r.showing_original {
            r.original.clone()
        } else {
            r.developed.clone()
        }) else {
            return;
        };
        r.hist_generation += 1;
        let generation = r.hist_generation;
        drop(r);
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let bins = gio::spawn_blocking(move || histogram(&frame, &state)).await;
            if ui.runtime.borrow().hist_generation == generation
                && let Ok(bins) = bins
            {
                ui.hist.replace(Some(bins));
                if let Some(hist) = ui.histogram.borrow().as_ref() {
                    hist.queue_draw();
                }
            }
        });
    }
}
fn histogram(frame: &Frame, state: &crate::model::State) -> [Vec<u32>; 3] {
    let mut bins = [vec![0; 256], vec![0; 256], vec![0; 256]];
    let [x, y, w, h] = state.crop.unwrap_or([0., 0., 1., 1.]);
    let width = frame.width as f64;
    let height = frame.height as f64;
    let step = ((width * height * w * h / 1_000_000.).sqrt().ceil() as usize).max(1);
    let (sin, cos) = (-state.straighten.to_radians()).sin_cos();
    for py in ((y * height).max(0.) as usize..((y + h) * height).min(height) as usize).step_by(step)
    {
        for px in
            ((x * width).max(0.) as usize..((x + w) * width).min(width) as usize).step_by(step)
        {
            let (dx, dy) = (px as f64 - width / 2., py as f64 - height / 2.);
            let (sx, sy) = (
                (width / 2. + dx * cos - dy * sin).round() as i32,
                (height / 2. + dx * sin + dy * cos).round() as i32,
            );
            if sx < 0 || sy < 0 || sx >= frame.width || sy >= frame.height {
                continue;
            }
            let p = (sy as usize * frame.width as usize + sx as usize) * 4;
            for c in 0..3 {
                bins[c][frame.pixels[p + c] as usize] += 1;
            }
        }
    }
    bins
}
