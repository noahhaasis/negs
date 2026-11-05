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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameID {
    revision: u64,
    generation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RenderStep {
    Settled,
    Drag(f32),
    Quick(f32),
}
impl RenderStep {
    fn scale(self) -> Option<f32> {
        match self {
            Self::Settled => None,
            Self::Drag(scale) | Self::Quick(scale) => Some(scale),
        }
    }
    fn quick(self) -> bool {
        matches!(self, Self::Quick(_))
    }
}

fn render_step(
    interacting: bool,
    scale: f32,
    pixels: f64,
    has_frame: bool,
    quick_done: bool,
    reuses_output: bool,
) -> RenderStep {
    let scale = (scale > 0. && scale < 1.).then_some(scale);
    if interacting {
        return scale.map(RenderStep::Drag).unwrap_or(RenderStep::Settled);
    }
    match scale {
        Some(scale)
            if has_frame
                && !quick_done
                && !reuses_output
                && pixels >= 4.5e6
                && scale * scale <= 0.5 =>
        {
            RenderStep::Quick(scale)
        }
        _ => RenderStep::Settled,
    }
}

fn reuse_key(mut command: Value) -> Value {
    command.as_object_mut().unwrap().remove("preview_path");
    command["params"]["post"] = Value::Null;
    command
}

struct CompletedFrame {
    identity: FrameID,
    step: RenderStep,
    id: u64,
    frame: Arc<Frame>,
    cache_key: Option<String>,
}

#[derive(Default)]
pub struct Runtime {
    pub client: Option<Arc<Engine>>,
    pub defaults: Value,
    pub ready: bool,
    pub supports_tone_curve: bool,
    pub supports_brightness_reference: bool,
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
    /// The settled preview's fraction of the working frame. Drag frames leave
    /// this unchanged so the Image Size notice doesn't resize the inspector.
    pub preview_scale: Option<f64>,
    pub original: Option<Arc<Frame>>,
    pub showing_original: bool,
    pub original_loading: bool,
    pub hist_generation: u64,
    pub exporting: Option<Arc<crate::engine::Job>>,
    pub preview_saving: bool,
    pub balancing_print: bool,
    pub block_helper_render: bool,
    pub close_confirmed: bool,
    pub cached_restoring: bool,
    pub original_key: Value,
    pub histogram_crop: Value,
    pub interactive_frames: u64,
    quick_step: Option<FrameID>,
    quick_frame: Option<FrameID>,
    held: Option<CompletedFrame>,
    helper_output: Option<(u64, Value)>,
    showing_interactive: bool,
    /// Restored launch-cache pixels do not establish a helper decode.
    live_frame: bool,
    #[cfg(test)]
    test_scale: Option<f32>,
    #[cfg(test)]
    render_log: Vec<(FrameID, RenderStep)>,
    #[cfg(test)]
    fail_quick: bool,
    #[cfg(test)]
    fail_settled: bool,
    #[cfg(test)]
    errors: Vec<String>,
}
impl Runtime {
    fn identity(&self) -> FrameID {
        FrameID {
            revision: self.revision,
            generation: self.source_generation,
        }
    }
    /// Matching can use a quick/held decode before anything is saveable.
    pub fn matching_frame(&self) -> Option<Arc<Frame>> {
        if let Some(held) = &self.held
            && held.identity == self.identity()
        {
            return Some(held.frame.clone());
        }
        if self.current_revision == Some(self.revision) || self.quick_frame == Some(self.identity())
        {
            self.developed.clone()
        } else {
            None
        }
    }
    pub fn set_developed(&mut self, frame: Arc<Frame>, interactive: bool) {
        if !interactive {
            let full = frame.result["working_width"]
                .as_f64()
                .filter(|width| *width > 0.)
                .unwrap_or(frame.width as f64);
            self.preview_scale = Some((frame.width as f64 / full).min(1.));
        }
        self.developed = Some(frame);
    }
}
pub fn ui_only() -> bool {
    std::env::var_os("NEGS_UI_ONLY").is_some()
}
pub const APP_ID: &str = "io.github.noahhaasis.negs";
pub const UI_PREVIEW_ID: &str = "io.github.noahhaasis.negs.UiPreview";
/// For the About dialog: which Linux package is running.
pub fn package_format() -> &'static str {
    if std::env::var_os("FLATPAK_ID").is_some() {
        "Linux Flatpak"
    } else if std::env::var_os("APPIMAGE").is_some() {
        "Linux AppImage"
    } else {
        "Linux"
    }
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
                        parameters::validate(&catalog)?;
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
                    let mut defaults = catalog["defaults"].clone();
                    // The engine leaves lens corrections off for params from
                    // before them, so their crops keep their place; the app
                    // corrects every new edit (as on macOS).
                    parameters::set(&mut defaults, parameters::LENS_CORRECTION_PATH, json!(true));
                    parameters::set_nullable_paths(&catalog);
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
                        runtime.supports_tone_curve = catalog["capabilities"]["tone_curve"] == true;
                        runtime.supports_brightness_reference = catalog["capabilities"]["brightness_reference"] == true;
                    }
                    ui.perf.mark("engine_catalog_ready_ms");
                    ui.maybe_build_inspector();
                    ui.sync_state();
                    // Presets listed before the engine started get their verdicts.
                    if ui.pane() == "presets" {
                        ui.refresh_presets();
                    }
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
    pub fn can_balance_print(&self, state: &crate::model::State) -> bool {
        let r = self.runtime.borrow();
        !ui_only()
            && r.ready
            && r.client.is_some()
            && !r.closing
            && !r.balancing_print
            && !self.canvas.cropping()
            && !crate::inspector::flag(state, "Scan film directly", false)
    }
    /// Solves the enlarger's magenta and yellow shifts so a mid-grey prints
    /// neutral with the current settings (upstream's print balance), and
    /// sets them as one undoable edit. Doesn't depend on the photograph.
    pub fn balance_print(self: &Rc<Self>) {
        let state = self.history.borrow().state.clone();
        if !self.can_balance_print(&state) {
            return;
        }
        let mut r = self.runtime.borrow_mut();
        let Some(client) = r.client.clone() else {
            return;
        };
        let command = json!({"command":"balance_print",
            "film":crate::catalog::selected(&state,false).id,
            "paper":crate::catalog::selected(&state,true).id,
            "params":parameters::runtime(&state,&r.defaults)});
        r.balancing_print = true;
        drop(r);
        self.status.set_text("Balancing print…");
        self.sync_history();
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result =
                gio::spawn_blocking(move || client.request(command, Duration::from_secs(20 * 60)))
                    .await;
            ui.runtime.borrow_mut().balancing_print = false;
            if ui.runtime.borrow().closing {
                return;
            }
            match result {
                Ok(Ok((_, reply))) => match (
                    reply["m_filter_shift"].as_f64().filter(|v| v.is_finite()),
                    reply["y_filter_shift"].as_f64().filter(|v| v.is_finite()),
                ) {
                    (Some(m), Some(y)) => {
                        let mut h = ui.history.borrow_mut();
                        h.commit();
                        h.begin("Balance Print");
                        h.state.values.insert("Enlarger.Magenta shift".into(), m);
                        h.state.values.insert("Enlarger.Yellow shift".into(), y);
                        h.commit();
                        drop(h);
                        ui.status
                            .set_text(&format!("Print balanced: magenta {m:.2}, yellow {y:.2}"));
                        ui.sync_state();
                    }
                    _ => ui.engine_error("The print balance returned no filter shifts."),
                },
                Ok(Err(error)) => ui.engine_error(&error),
                Err(_) => ui.engine_error(
                    "The image worker stopped unexpectedly. Open the photograph again to retry.",
                ),
            }
            ui.sync_history();
        });
    }
    pub fn engine_error(&self, message: &str) {
        #[cfg(test)]
        self.runtime.borrow_mut().errors.push(message.into());
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
        let desired = json!({"params":params,"film":film,"paper":paper,"limit_preview":self.preferences.borrow().limit_preview.unwrap_or(false),"extent":extent,"interactive":r.interacting});
        let identity = Some((source.clone(), stamp));
        let source_changed = r.source != identity;
        if source_changed {
            // The first decoded source may restore an already-solved session.
            // A subsequent source refresh needs a new solution for its pixels.
            if r.source.is_some()
                && let Some(matched) = self.history.borrow_mut().state.brightness_match.as_mut()
            {
                matched.inputs = None;
            }
            r.source = identity;
            r.source_generation += 1;
            r.hist_generation += 1;
            r.original = None;
            r.original_loading = false;
            r.developed = None;
            r.live_frame = false;
            r.preview_scale = None;
            r.current_revision = None;
            r.showing_original = false;
            r.helper_output = None;
            r.quick_frame = None;
            r.desired = Value::Null;
            self.lens_supported.set(false);
        }
        // The original view follows the decode and the preview space.
        let original_key = json!([
            params["io"]["raw_white_balance"],
            params["io"]["lens_correction"],
            desired["limit_preview"],
            params["io"]["input_interpretation"],
            params["io"]["input_color_space"],
            params["io"]["input_cctf_decoding"],
            params["io"]["output_color_space"]
        ]);
        let release_original = r.showing_original && r.original_key != original_key;
        if r.original_key != original_key {
            r.original_key = original_key;
            r.original = None;
            r.original_loading = false;
        }
        let histogram_crop = json!([state.crop, state.straighten, state.rotation]);
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
        let released_drag = r.desired["interactive"] == true && !r.interacting;
        r.desired = desired;
        r.revision += 1;
        r.retained_id = None;
        r.held = None;
        // Drag frames already filled the canvas; release goes straight to settled.
        r.quick_step = released_drag.then(|| r.identity());
        drop(r);
        if let Some(ui) = self.owner.borrow().upgrade() {
            if source_changed {
                crate::brightness::clear(&ui);
                crate::tone_graph::clear(&ui);
                crate::scopes::clear(&ui);
            }
            if release_original {
                ui.show_original(false);
            }
            glib::idle_add_local_once(move || ui.render_next());
        }
    }
    pub(crate) fn render_next(self: &Rc<Self>) {
        let state = self.history.borrow().state.clone();
        let mut r = self.runtime.borrow_mut();
        let identity = r.identity();
        if !r.ready
            || r.closing
            || r.rendering
            || r.block_helper_render
            || r.held.is_some()
            || crate::brightness::running(self)
            || (r.requested_revision == r.revision && r.quick_step != Some(identity))
        {
            return;
        }
        let (Some(client), Some((source, _))) = (r.client.clone(), r.source.clone()) else {
            return;
        };
        let desired = r.desired.clone();
        let cache_defaults = r.defaults.clone();
        let cache_state = state.clone();
        let cache_source = source.clone();
        let cache_paths = client.paths.clone();
        let buffer = client
            .scratch
            .directory
            .join(format!("preview-{}.rgba", identity.revision));
        let mut command = json!({"command":"render","input":source,"source_generation":identity.generation,
            "film":desired["film"],"paper":desired["paper"],"params":desired["params"],
            "limit_preview":desired["limit_preview"],"preview_path":buffer});
        if !desired["params"]["io"]["photo_long_side_px"].is_null() {
            command["crop"] = parameters::crop(&state);
        }
        let key = reuse_key(command.clone());
        let pixels = r
            .developed
            .as_ref()
            .map(|f| {
                f.result["photo_width"].as_f64().unwrap_or(f.width as f64)
                    * f.result["photo_height"].as_f64().unwrap_or(f.height as f64)
            })
            .unwrap_or(0.);
        let scale = self.canvas.interactive_scale() as f32;
        #[cfg(test)]
        let scale = r.test_scale.unwrap_or(scale);
        let step = render_step(
            r.interacting,
            scale,
            pixels,
            r.developed.is_some() && r.live_frame && !r.cached_restoring,
            r.quick_step == Some(identity),
            r.helper_output
                .as_ref()
                .is_some_and(|(epoch, old)| *epoch == client.connection_epoch() && *old == key),
        );
        if let Some(scale) = step.scale() {
            command["interactive_scale"] = json!(scale);
        }
        #[cfg(test)]
        if step.quick() && std::mem::take(&mut r.fail_quick) {
            command["film"] = json!("__test_unknown_film__");
        }
        #[cfg(test)]
        if step == RenderStep::Settled && std::mem::take(&mut r.fail_settled) {
            command["film"] = json!("__test_unknown_film__");
        }
        r.rendering = true;
        r.requested_revision = identity.revision;
        // Every render replaces the helper's kept output, including failures.
        r.helper_output = None;
        #[cfg(test)]
        r.render_log.push((identity, step));
        drop(r);
        self.sync_history();
        crate::stock_browser::pump(self);
        let ui = self.clone();
        glib::timeout_add_local_once(
            Duration::from_millis(400),
            glib::clone!(
                #[weak]
                ui,
                move || {
                    let r = ui.runtime.borrow();
                    if r.rendering
                        && r.identity() == identity
                        && !r.interacting
                        && !r.cached_restoring
                        && !r.showing_original
                    {
                        let label = if r.developed.is_some() {
                            "Updating…"
                        } else {
                            "Developing…"
                        };
                        ui.status.set_text(label);
                        ui.badge.set_label(label);
                        ui.badge.set_visible(true);
                    }
                }
            ),
        );
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || {
                let cache_key = if step == RenderStep::Settled {
                    crate::preview_cache::key(&cache_source, &cache_state, &cache_defaults, &cache_paths)
                } else { None };
                let rendered = client.request(command, Duration::from_secs(180))
                    .and_then(|(id, result)| Frame::read(&buffer, result).map(|frame| (id, Arc::new(frame), cache_key, client.connection_epoch())));
                let _ = std::fs::remove_file(&buffer);
                rendered
            }).await;
            let hold_for_match = crate::brightness::needs_solution(&ui);
            let mut r = ui.runtime.borrow_mut();
            r.rendering = false;
            if r.closing { return; }
            let same_source = identity.generation == r.source_generation;
            let current = identity == r.identity();
            let showable = same_source && (current || r.interacting || step.quick());
            if current {
                // A failed quick step advances too; a failed settled step stops.
                r.quick_step = step.quick().then_some(identity);
            }
            match result {
                Ok(Ok((id, frame, cache_key, epoch))) => {
                    if same_source && step == RenderStep::Settled {
                        r.helper_output = Some((epoch, key));
                    }
                    if showable {
                        let completed = CompletedFrame { identity, step, id, frame, cache_key };
                        if current && !matches!(step, RenderStep::Drag(_)) && !r.interacting
                            && !ui.canvas.cropping() && hold_for_match {
                            r.held = Some(completed);
                            drop(r);
                        } else {
                            drop(r);
                            ui.publish_frame(completed);
                        }
                    } else { drop(r); }
                }
                Ok(Err(error)) if current && !step.quick() => {
                    r.retained_id = None;
                    drop(r);
                    ui.engine_error(&error);
                }
                Err(_) if current && !step.quick() => {
                    r.retained_id = None;
                    drop(r);
                    ui.engine_error("The image worker stopped unexpectedly. Open the photograph again to retry.");
                }
                _ => { drop(r); }
            }
            ui.sync_history();
            // Match using the held/quick decode before requesting a large frame.
            crate::brightness::pump(&ui);
            ui.render_next();
            crate::tone_graph::pump(&ui);
            crate::stock_browser::pump(&ui);
        });
    }
    pub(crate) fn release_held_frame(self: &Rc<Self>) {
        let mut r = self.runtime.borrow_mut();
        let Some(held) = r.held.take() else {
            return;
        };
        let current = held.identity == r.identity() && !r.closing;
        drop(r);
        if current {
            self.publish_frame(held);
            self.sync_history();
        }
    }
    fn publish_frame(self: &Rc<Self>, completed: CompletedFrame) {
        let CompletedFrame {
            identity,
            step,
            id,
            frame,
            cache_key,
        } = completed;
        let mut r = self.runtime.borrow_mut();
        if r.closing || identity.generation != r.source_generation {
            return;
        }
        let settled = step == RenderStep::Settled;
        r.set_developed(frame.clone(), !settled);
        r.live_frame = true;
        r.showing_interactive = !settled;
        r.quick_frame = step.quick().then_some(identity);
        self.input_summary
            .set_text(&input_summary(&frame.result["input_colour"]));
        self.lens_supported
            .set(frame.result["lens_correction_supported"] == true);
        if settled && identity == r.identity() {
            r.retained_id = Some(id);
            r.current_revision = Some(identity.revision);
            r.cached_restoring = false;
            self.perf.mark("saveable_render_ready_ms");
            if let Some(key) = cache_key {
                self.cache.save(key, frame.clone());
            }
        }
        if !settled {
            r.interactive_frames += 1;
            self.perf
                .detail("interactive_frames_presented", r.interactive_frames);
        }
        let show = !r.showing_original;
        drop(r);
        if show {
            self.present_engine_frame(&frame, true);
        }
        crate::tone_graph::rendered(self, &frame.result);
        self.update_engine_histogram();
    }
    fn present_engine_frame(self: &Rc<Self>, frame: &Frame, developed: bool) {
        let texture = crate::canvas::texture(
            frame.width,
            frame.height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(frame.pixels.clone()),
            frame.width as usize * 4,
            &crate::canvas::display_color_state(
                frame.result["display_color_space"].as_str().unwrap_or(""),
            ),
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
            let preview = self.runtime.borrow().showing_interactive;
            self.status.set_text(&if preview {
                format!(
                    "Preview · {} × {} · developed in {:.2} s · {backend}",
                    frame.width,
                    frame.height,
                    milliseconds / 1000.
                )
            } else {
                format!("Developed in {:.2} s · {backend}", milliseconds / 1000.)
            });
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
                        combined["display_color_space"] = result["display_color_space"].clone();
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
        crate::scopes::update(self);
    }
}
/// How the helper read the photograph's colours, for the inspector: its
/// summary, the automatic reading an override replaces, and any warning.
pub(crate) fn input_summary(colour: &Value) -> String {
    let text = |key: &str| colour[key].as_str().unwrap_or_default();
    let mut lines = vec![text("summary").to_string()];
    if text("automatic") != text("summary") {
        lines.push(format!("Automatic: {}", text("automatic")));
    }
    if !text("warning").is_empty() {
        lines.push(text("warning").to_string());
    }
    lines.retain(|l| !l.is_empty());
    lines.join("\n")
}

#[cfg(test)]
#[path = "render_tests.rs"]
mod tests;
