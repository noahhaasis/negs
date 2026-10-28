use crate::{Editor, fixtures};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{path::PathBuf, rc::Rc};

enum Opened {
    Camera(crate::photo::Photo),
    Cached(std::sync::Arc<crate::engine::Frame>),
    EngineOnly,
}

impl Editor {
    fn action(self: &Rc<Self>, name: &str, callback: impl Fn(&Rc<Self>) + 'static) {
        let action = gio::SimpleAction::new(name, None);
        action.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, _| callback(&ui)
        ));
        self.window.add_action(&action);
    }
    pub fn actions(self: &Rc<Self>, app: &adw::Application) {
        self.undo.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, _| {
                ui.history.borrow_mut().undo();
                ui.sync_state();
            }
        ));
        self.window.add_action(&self.undo);
        self.redo.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, _| {
                ui.history.borrow_mut().redo();
                ui.sync_state();
            }
        ));
        self.window.add_action(&self.redo);
        self.action("open", |ui| ui.open_dialog());
        let recent = gio::SimpleAction::new("open-recent", Some(glib::VariantTy::STRING));
        recent.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, value| {
                if let Some(path) = value.and_then(|v| v.str()) {
                    let path = PathBuf::from(path);
                    if path.is_file() {
                        ui.open(path);
                    } else {
                        ui.recent.borrow_mut().retain(|p| p != &path);
                        ui.refresh_recent();
                        ui.toast.add_toast(adw::Toast::new(
                            "This photograph is no longer available and was removed from Recent.",
                        ));
                    }
                }
            }
        ));
        self.window.add_action(&recent);
        let open_path = gio::SimpleAction::new("open-path", Some(glib::VariantTy::STRING));
        open_path.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, value| {
                if let Some(path) = value.and_then(|v| v.str()) {
                    ui.open(PathBuf::from(path));
                }
            }
        ));
        self.window.add_action(&open_path);
        self.action("clear-recent", |ui| {
            ui.recent.borrow_mut().clear();
            ui.refresh_recent();
        });
        self.action("quit", |ui| ui.window.close());
        self.action("settings", |ui| ui.settings());
        self.action("fullscreen", |ui| {
            ui.window.set_fullscreened(!ui.window.is_fullscreen())
        });
        self.action("shortcuts", |ui| ui.shortcuts());
        self.action("previous", |ui| ui.navigate(-1));
        self.action("next", |ui| ui.navigate(1));
        self.action("original", |ui| {
            let show = !ui.runtime.borrow().showing_original;
            ui.show_original(show);
        });
        self.refresh_recent();
        self.action("fit", |ui| ui.canvas.fit());
        self.action("actual", |ui| ui.canvas.actual_size());
        self.action("zoom-in", |ui| {
            ui.canvas.zoom_at(
                1.25,
                (
                    ui.canvas.width() as f64 / 2.,
                    ui.canvas.height() as f64 / 2.,
                ),
            )
        });
        self.action("zoom-out", |ui| {
            ui.canvas.zoom_at(
                0.8,
                (
                    ui.canvas.width() as f64 / 2.,
                    ui.canvas.height() as f64 / 2.,
                ),
            )
        });
        self.action("rotate", |ui| {
            let mut h = ui.history.borrow_mut();
            h.begin("Rotate");
            h.state.rotation = (h.state.rotation + 1) % 4;
            if ui.canvas.cropping() {
                rotate_crop_options(&mut h.state);
            }
            if !ui.canvas.cropping() {
                h.commit();
            }
            drop(h);
            ui.sync_state();
            ui.canvas.fit();
        });
        self.action("rotate-left", |ui| {
            let mut h = ui.history.borrow_mut();
            h.begin("Rotate");
            h.state.rotation = (h.state.rotation + 3) % 4;
            if ui.canvas.cropping() {
                rotate_crop_options(&mut h.state);
            }
            if !ui.canvas.cropping() {
                h.commit();
            }
            drop(h);
            ui.sync_state();
            ui.canvas.fit();
        });
        self.action("inspector", |ui| {
            ui.split.set_show_sidebar(!ui.split.shows_sidebar())
        });
        self.action("strip", |ui| {
            ui.strip_scroll.set_visible(!ui.strip_scroll.is_visible());
            ui.preferences.borrow_mut().strip = ui.strip_scroll.is_visible();
        });
        self.action("reset", |ui| {
            let mut h = ui.history.borrow_mut();
            h.begin("Reset Adjustments");
            let s = fixtures::state();
            let kept_values: Vec<_> = h
                .state
                .values
                .iter()
                .filter(|(k, _)| k.starts_with("Image Size.") || k.starts_with("Crop "))
                .map(|(k, v)| (k.clone(), *v))
                .collect();
            let kept_choices: Vec<_> = h
                .state
                .choices
                .iter()
                .filter(|(k, _)| {
                    [
                        "Film",
                        "Paper",
                        "Film ID",
                        "Paper ID",
                        "Develop at",
                        "Crop aspect",
                        "Crop portrait",
                    ]
                    .contains(&k.as_str())
                })
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            h.state.values = s.values;
            h.state.values.extend(kept_values);
            h.state.choices = kept_choices.into_iter().collect();
            if !crate::runtime::ui_only() {
                let defaults = ui.runtime.borrow().defaults.clone();
                crate::parameters::install_defaults(&mut h.state, &defaults, true);
                h.state.params = defaults;
            }
            let positive = crate::catalog::selected(&h.state, false).positive;
            h.state
                .choices
                .insert("Scan film directly".into(), positive.to_string());
            h.commit();
            drop(h);
            ui.sync_state();
        });
        self.action("crop", |ui| {
            if ui.current_path.borrow().is_none() {
                return;
            }
            if ui.canvas.cropping() {
                ui.finish_crop(true);
            } else {
                ui.crop_before
                    .replace(Some(ui.history.borrow().state.clone()));
                ui.history.borrow_mut().begin("Crop");
                ui.show_original(false);
                ui.canvas.begin_crop();
                ui.crop_bar.set_visible(true);
                ui.canvas.grab_focus();
                ui.sync_history();
            }
        });
        self.action("crop-cancel", |ui| ui.finish_crop(false));
        self.action("crop-done", |ui| ui.finish_crop(true));
        self.action("crop-original", |ui| {
            let (w, h) = ui.canvas.source_size();
            ui.canvas.crop_ratio(w.max(1) as f64 / h.max(1) as f64);
        });
        self.action("crop-3-2", |ui| ui.canvas.crop_ratio(1.5));
        self.action("crop-square", |ui| ui.canvas.crop_ratio(1.));
        self.action("export", |ui| ui.export_dialog("photo"));
        self.action("save-preview", |ui| ui.export_dialog("preview"));
        self.action("cancel-export", |ui| ui.cancel_export());
        self.action("lut", |ui| ui.export_dialog("lut"));
        self.action("appearance", |ui| ui.settings());
        self.action("about", |ui| {
            let d = adw::AboutDialog::builder().application_name("Negs")
                .application_icon("dev.negs.app").version(if crate::runtime::ui_only() { "0.10.0 · Linux UI preview" } else { "0.10.0 · Linux" })
                .developer_name("Negs contributors")
                .comments("Film simulation with a native GTK4/libadwaita editor and the Spektrafilm engine.")
                .license_type(gtk::License::Gpl30).build();
            d.present(Some(&ui.window));
        });
        for (action, keys) in [
            ("win.open", vec!["<Control>o"]),
            ("win.quit", vec!["<Control>q"]),
            ("win.settings", vec!["<Control>comma"]),
            ("win.shortcuts", vec!["<Control>question"]),
            ("win.save-preview", vec!["<Control>s"]),
            ("win.lut", vec!["<Control><Shift>l"]),
            ("win.fullscreen", vec!["F11"]),
            // Ctrl+Shift+R resets, as ⇧⌘R and Ctrl+Shift+R do on macOS and
            // Windows; counterclockwise takes Photos' ⌥⌘R.
            ("win.reset", vec!["<Control><Shift>r"]),
            ("win.rotate-left", vec!["<Control><Alt>r"]),
            ("win.previous", vec!["<Alt>Left"]),
            ("win.next", vec!["<Alt>Right"]),
            ("win.undo", vec!["<Control>z"]),
            ("win.redo", vec!["<Control><Shift>z", "<Control>y"]),
            ("win.fit", vec!["<Control>0"]),
            ("win.actual", vec!["<Control>1"]),
            ("win.rotate", vec!["<Control>r"]),
            ("win.inspector", vec!["F7"]),
            ("win.strip", vec!["F6"]),
            ("win.export", vec!["<Control><Shift>e"]),
            ("win.zoom-in", vec!["<Control>plus", "<Control>equal"]),
            ("win.zoom-out", vec!["<Control>minus"]),
        ] {
            app.set_accels_for_action(action, &keys);
        }
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, mods| {
                if key == gdk::Key::backslash {
                    ui.show_original(true);
                    return glib::Propagation::Stop;
                }
                if !mods.is_empty() {
                    return glib::Propagation::Proceed;
                }
                match key {
                    gdk::Key::backslash => {
                        let _ = gtk::prelude::WidgetExt::activate_action(
                            &ui.window,
                            "win.original",
                            None,
                        );
                    }
                    gdk::Key::c => {
                        let _ =
                            gtk::prelude::WidgetExt::activate_action(&ui.window, "win.crop", None);
                    }
                    gdk::Key::Escape if ui.canvas.cropping() => ui.finish_crop(false),
                    gdk::Key::Return if ui.canvas.cropping() => ui.finish_crop(true),
                    gdk::Key::Left if ui.canvas.cropping() => ui.canvas.nudge_crop(-0.001, 0.),
                    gdk::Key::Right if ui.canvas.cropping() => ui.canvas.nudge_crop(0.001, 0.),
                    gdk::Key::Up if ui.canvas.cropping() => ui.canvas.nudge_crop(0., -0.001),
                    gdk::Key::Down if ui.canvas.cropping() => ui.canvas.nudge_crop(0., 0.001),
                    gdk::Key::Left => ui.navigate(-1),
                    gdk::Key::Right => ui.navigate(1),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        ));
        keys.connect_key_released(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_, key, _, _| if key == gdk::Key::backslash {
                ui.show_original(false);
            }
        ));
        self.canvas.add_controller(keys);
        let release = gtk::EventControllerFocus::new();
        release.connect_leave(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_| ui.show_original(false)
        ));
        self.canvas.add_controller(release);
    }
    fn finish_crop(&self, apply: bool) {
        if !self.canvas.cropping() {
            return;
        }
        let angle = self.canvas.draft_angle();
        if apply
            && self
                .history
                .borrow()
                .state
                .choices
                .get("Develop at")
                .map(String::as_str)
                == Some("1")
        {
            let target = self
                .history
                .borrow()
                .state
                .values
                .get("Image Size.Longest side")
                .copied()
                .unwrap_or(2048.);
            if let Err(error) = crate::image_size::validate(
                &format!("{target:.0}"),
                self.canvas.source_size(),
                Some(self.canvas.crop_draft()),
            ) {
                self.toast.add_toast(adw::Toast::new(&error));
                return;
            }
        }
        let crop = self.canvas.end_crop();
        self.crop_bar.set_visible(false);
        if apply {
            let mut h = self.history.borrow_mut();
            h.begin("Crop");
            h.state.crop = if crop == [0., 0., 1., 1.] {
                None
            } else {
                Some(crop)
            };
            h.state.straighten = angle;
            h.commit();
        } else if let Some(before) = self.crop_before.take() {
            let mut h = self.history.borrow_mut();
            h.state = before;
            h.commit();
        }
        self.crop_before.take();
        self.sync_state();
    }
    fn open_dialog(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Photographs"));
        for ext in ["jpg", "jpeg", "png", "tif", "tiff"]
            .iter()
            .chain(crate::photo::RAW_EXTENSIONS)
        {
            filter.add_suffix(ext);
        }
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Open Photograph")
            .filters(&filters)
            .build();
        if let Some(parent) = self.current_path.borrow().as_ref().and_then(|p| p.parent()) {
            dialog.set_initial_folder(Some(&gio::File::for_path(parent)));
        }
        dialog.open(
            Some(&self.window),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[weak(rename_to=ui)]
                self,
                move |result| {
                    if let Ok(file) = result
                        && let Some(path) = file.path()
                    {
                        ui.open(path);
                    }
                }
            ),
        );
    }
    pub fn open(self: &Rc<Self>, path: PathBuf) {
        if self.runtime.borrow().closing {
            return;
        }
        let path = gio::File::for_path(&path).path().unwrap_or(path);
        if self.pending_path.borrow().as_ref() == Some(&path) {
            return;
        }
        self.generation.set(self.generation.get() + 1);
        let generation = self.generation.get();
        if self.current_path.borrow().as_ref() == Some(&path)
            && self.current_stamp.get() == Some(crate::filmstrip::stamp(&path))
        {
            self.pending_path.take();
            self.select_strip(&path);
            if crate::runtime::ui_only() {
                self.status
                    .set_text("Camera preview · engine not connected");
            }
            if !crate::runtime::ui_only()
                && self.runtime.borrow().retained_id.is_none()
                && !self.runtime.borrow().rendering
            {
                self.runtime.borrow_mut().requested_revision = 0;
                if self.runtime.borrow().ready {
                    self.render_next();
                } else {
                    self.start_engine();
                }
            }
            return;
        }
        if self.canvas.cropping() {
            self.finish_crop(false);
        }
        self.pending_path.replace(Some(path.clone()));
        self.select_strip(&path);
        glib::timeout_add_local_once(
            std::time::Duration::from_millis(400),
            glib::clone!(
                #[weak(rename_to=ui)]
                self,
                move || {
                    if ui.generation.get() == generation && ui.pending_path.borrow().is_some() {
                        ui.status.set_text("Loading photograph…");
                    }
                }
            ),
        );
        let mut requested = self.history.borrow().state.clone();
        if self
            .current_path
            .borrow()
            .as_ref()
            .is_some_and(|p| p != &path)
        {
            requested.crop = None;
            requested.rotation = 0;
            requested.straighten = 0.;
        }
        let requested_identity =
            crate::preview_cache::identity(&requested, &self.runtime.borrow().defaults);
        let defaults = self.runtime.borrow().defaults.clone();
        let cache = self.cache.file.clone();
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let path2 = path.clone();
            let result = gio::spawn_blocking(move || {
                if !crate::runtime::ui_only()
                    && let Ok(paths) = crate::engine::Paths::locate()
                    && let Some(key) =
                        crate::preview_cache::key(&path2, &requested, &defaults, &paths)
                    && let Some(frame) = crate::preview_cache::read(&cache, &key)
                {
                    return Ok(Opened::Cached(frame));
                }
                match crate::photo::decode(&path2, 2400) {
                    Ok(photo) => Ok(Opened::Camera(photo)),
                    Err(_)
                        if !crate::runtime::ui_only()
                            && path2.is_file()
                            && crate::photo::supported(&path2) =>
                    {
                        Ok(Opened::EngineOnly)
                    }
                    Err(error) => Err(error),
                }
            })
            .await;
            if ui.generation.get() != generation || ui.runtime.borrow().closing {
                return;
            }
            ui.pending_path.take();
            match result {
                Ok(Ok(opened)) => {
                    if ui
                        .current_path
                        .borrow()
                        .as_ref()
                        .is_some_and(|p| p != &path)
                    {
                        let mut state = ui.history.borrow().state.clone();
                        state.crop = None;
                        state.rotation = 0;
                        state.straighten = 0.;
                        ui.history.replace(crate::model::History::new(state));
                    }
                    if matches!(opened, Opened::Cached(_))
                        && crate::preview_cache::identity(
                            &ui.history.borrow().state,
                            &ui.runtime.borrow().defaults,
                        ) != requested_identity
                    {
                        ui.open(path);
                        return;
                    }
                    let mut cached = None;
                    let has_pixels = !matches!(opened, Opened::EngineOnly);
                    match opened {
                        Opened::Camera(photo) => {
                            let format = if photo.alpha {
                                gdk::MemoryFormat::R8g8b8a8
                            } else {
                                gdk::MemoryFormat::R8g8b8
                            };
                            let texture = crate::canvas::srgb_texture(
                                photo.width,
                                photo.height,
                                format,
                                &glib::Bytes::from_owned(photo.bytes),
                                photo.stride,
                            );
                            ui.canvas.set_texture(texture.upcast_ref(), photo.source);
                            ui.hist.replace(Some(photo.histogram));
                            ui.dimensions
                                .set_text(&format!("{} × {} px", photo.source.0, photo.source.1));
                        }
                        Opened::Cached(frame) => {
                            let texture = crate::canvas::srgb_texture(
                                frame.width,
                                frame.height,
                                gdk::MemoryFormat::R8g8b8a8,
                                &glib::Bytes::from_owned(frame.pixels.clone()),
                                frame.width as usize * 4,
                            );
                            let source = (
                                frame.result["source_width"]
                                    .as_i64()
                                    .unwrap_or(frame.width as i64)
                                    as i32,
                                frame.result["source_height"]
                                    .as_i64()
                                    .unwrap_or(frame.height as i64)
                                    as i32,
                            );
                            let photo = (
                                frame.result["photo_width"]
                                    .as_i64()
                                    .unwrap_or(frame.width as i64)
                                    as i32,
                                frame.result["photo_height"]
                                    .as_i64()
                                    .unwrap_or(frame.height as i64)
                                    as i32,
                            );
                            ui.canvas.set_texture(texture.upcast_ref(), source);
                            ui.canvas
                                .update_texture(texture.upcast_ref(), source, photo);
                            ui.runtime.borrow_mut().cached_restoring = true;
                            cached = Some(frame);
                            ui.perf.mark("cached_texture_ready_ms");
                        }
                        Opened::EngineOnly => {
                            ui.canvas.fit();
                            ui.hist.take();
                        }
                    }
                    ui.current_path.replace(Some(path.clone()));
                    if has_pixels && let Some(view) = ui.pending_view.take() {
                        ui.canvas.restore_view(&view);
                    }
                    ui.current_stamp.set(Some(crate::filmstrip::stamp(&path)));
                    ui.sync_state();
                    let restored = cached.is_some();
                    if let Some(frame) = cached {
                        ui.runtime.borrow_mut().set_developed(frame, false);
                        ui.sync_history();
                        ui.update_engine_histogram();
                    }
                    if let Some(hist) = ui.histogram.borrow().as_ref() {
                        hist.queue_draw();
                    }
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    ui.title.set_title(&name);
                    ui.title.set_subtitle(if restored {
                        "Restored developed preview"
                    } else {
                        "Camera preview · Negs"
                    });
                    ui.window.set_title(Some(&format!(
                        "{name} — Negs{}",
                        if crate::runtime::ui_only() {
                            " UI Preview"
                        } else {
                            ""
                        }
                    )));
                    ui.status.set_text(if restored {
                        "Restored preview · preparing saveable pixels…"
                    } else if crate::runtime::ui_only() {
                        "Camera preview · engine not connected"
                    } else {
                        "Developing…"
                    });
                    ui.stage.set_visible_child_name("photo");
                    ui.strip_scroll.set_visible(ui.preferences.borrow().strip);
                    ui.perf.mark("preview_texture_ready_ms");
                    let perf = ui.perf.clone();
                    ui.canvas.add_tick_callback(move |widget, clock| {
                        perf.mark("preview_frame_requested_ms");
                        perf.watch_presentation(
                            clock,
                            clock.frame_counter(),
                            if restored {
                                "cached_frame_presented_ms"
                            } else {
                                "preview_frame_presented_ms"
                            },
                        );
                        perf.detail("preview_surface_mapped", widget.is_mapped());
                        perf.write();
                        glib::ControlFlow::Break
                    });
                    ui.remember(path.clone());
                    ui.load_strip(path);
                    ui.perf.write();
                }
                Ok(Err(error)) => {
                    ui.status.set_text("Could not open photograph");
                    ui.recent.borrow_mut().retain(|p| p != &path);
                    ui.refresh_recent();
                    ui.toast.add_toast(adw::Toast::new(&error));
                    if let Some(path) = ui.current_path.borrow().as_ref() {
                        ui.select_strip(path);
                    }
                }
                Err(_) => {
                    ui.status.set_text("Image decoder stopped");
                }
            }
        });
    }
    fn select_strip(&self, path: &std::path::Path) {
        crate::filmstrip::select(self, path);
    }
    fn load_strip(self: &Rc<Self>, path: PathBuf) {
        crate::filmstrip::load(self, path, false);
    }
    fn navigate(self: &Rc<Self>, offset: isize) {
        if self.canvas.cropping() {
            return;
        }
        let entries = self.strip_paths.borrow();
        let path = self
            .pending_path
            .borrow()
            .clone()
            .or_else(|| self.current_path.borrow().clone());
        let next = entries
            .iter()
            .position(|p| Some(p) == path.as_ref())
            .and_then(|index| index.checked_add_signed(offset))
            .and_then(|index| entries.get(index))
            .cloned();
        drop(entries);
        if let Some(path) = next {
            self.open(path);
        }
    }
    fn shortcuts(&self) {
        let dialog=adw::AlertDialog::builder().heading("Keyboard Shortcuts").body("Ctrl+O  Open photograph\nCtrl+S  Save Preview options\nCtrl+Shift+E  Export options\nCtrl+Z / Ctrl+Shift+Z  Undo / Redo\nCtrl+0 / Ctrl+1  Fit / Actual Size\nCtrl++ / Ctrl+−  Zoom\nCtrl+R / Ctrl+Alt+R  Rotate clockwise / counterclockwise\nCtrl+Shift+R  Reset Adjustments\nC  Crop (canvas focused)\nEnter / Escape  Apply / Cancel crop\nArrow keys  Move crop / Previous and next photo\nAlt+Left / Alt+Right  Previous / Next photograph\nF6 / F7  Film Strip / Adjustments\nCtrl+,  Settings\nCtrl+Q  Quit").build();
        dialog.add_response("close", "Close");
        dialog.present(Some(&self.window));
    }
    fn export_dialog(self: &Rc<Self>, kind: &str) {
        crate::export_ui::present(self, kind);
    }
}

fn rotate_crop_options(state: &mut crate::model::State) {
    let portrait = crate::inspector::flag(state, "Crop portrait", false);
    state
        .choices
        .insert("Crop portrait".into(), (!portrait).to_string());
    if crate::inspector::choice(state, "Crop aspect") == 8 {
        let w = state.values.get("Crop width").copied().unwrap_or(5.);
        let h = state.values.get("Crop height").copied().unwrap_or(4.);
        state.values.insert("Crop width".into(), h);
        state.values.insert("Crop height".into(), w);
    }
}
