use crate::{Editor, fixtures};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{path::PathBuf, rc::Rc};

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
            h.commit();
            drop(h);
            ui.sync_state();
            ui.canvas.fit();
        });
        self.action("inspector", |ui| {
            ui.split.set_show_sidebar(!ui.split.shows_sidebar())
        });
        self.action("strip", |ui| {
            ui.strip_scroll.set_visible(!ui.strip_scroll.is_visible())
        });
        self.action("reset", |ui| {
            let mut h = ui.history.borrow_mut();
            h.begin("Reset Adjustments");
            let s = fixtures::state();
            h.state.values = s.values;
            h.state.choices.clear();
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
                ui.canvas.begin_crop();
                ui.crop_bar.set_visible(true);
                ui.canvas.grab_focus();
            }
        });
        self.action("crop-cancel", |ui| ui.finish_crop(false));
        self.action("crop-done", |ui| ui.finish_crop(true));
        self.action("crop-original", |ui| ui.canvas.crop_ratio(1.5));
        self.action("crop-3-2", |ui| ui.canvas.crop_ratio(1.5));
        self.action("crop-square", |ui| ui.canvas.crop_ratio(1.));
        self.action("export", |ui| ui.export_dialog(false));
        self.action("lut", |ui| ui.export_dialog(true));
        self.action("appearance", |ui| ui.appearance_dialog());
        self.action("about",|ui|{let d=adw::AboutDialog::builder().application_name("Negs").application_icon("camera-photo-symbolic").version("0.7.0 · Linux UI preview").developer_name("Negs contributors").comments("A native GTK4/libadwaita editor.\nFilm processing is not connected in this preview.").license_type(gtk::License::Gpl30).build();d.present(Some(&ui.window));});
        for (action, keys) in [
            ("win.open", vec!["<Control>o"]),
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
                if !mods.is_empty() {
                    return glib::Propagation::Proceed;
                }
                match key {
                    gdk::Key::c => {
                        let _ =
                            gtk::prelude::WidgetExt::activate_action(&ui.window, "win.crop", None);
                    }
                    gdk::Key::Escape if ui.canvas.cropping() => ui.finish_crop(false),
                    gdk::Key::Return if ui.canvas.cropping() => ui.finish_crop(true),
                    _ => return glib::Propagation::Proceed,
                }
                glib::Propagation::Stop
            }
        ));
        self.canvas.add_controller(keys);
    }
    fn finish_crop(&self, apply: bool) {
        if !self.canvas.cropping() {
            return;
        }
        let crop = self.canvas.end_crop();
        self.crop_bar.set_visible(false);
        if apply {
            let mut h = self.history.borrow_mut();
            h.begin("Crop");
            h.state.crop = Some(crop);
            h.commit();
        }
        self.sync_state();
    }
    fn open_dialog(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("Photographs"));
        for ext in [
            "jpg", "jpeg", "png", "tif", "tiff", "nef", "dng", "arw", "cr2",
        ] {
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
        let path = gio::File::for_path(&path).path().unwrap_or(path);
        if self.pending_path.borrow().as_ref() == Some(&path) {
            return;
        }
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        if self.current_path.borrow().as_ref() == Some(&path) {
            // Also cancel a different in-flight open if the displayed photo is chosen again.
            self.pending_path.take();
            self.select_strip(&path);
            self.status
                .set_text("Camera preview · engine not connected");
            return;
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
                        ui.status.set_text("Loading camera preview…");
                    }
                }
            ),
        );
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let path2 = path.clone();
            let result = gio::spawn_blocking(move || crate::photo::decode(&path2, 2400)).await;
            if ui.generation.get() != generation {
                return;
            }
            ui.pending_path.take();
            match result {
                Ok(Ok(photo)) => {
                    let format = if photo.alpha {
                        gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gdk::MemoryFormat::R8g8b8
                    };
                    let texture = gdk::MemoryTexture::new(
                        photo.width,
                        photo.height,
                        format,
                        &glib::Bytes::from_owned(photo.bytes),
                        photo.stride,
                    );
                    if ui.current_path.borrow().is_some() {
                        let mut state = ui.history.borrow().state.clone();
                        state.crop = None;
                        state.rotation = 0;
                        ui.history.replace(crate::model::History::new(state));
                    }
                    ui.canvas.set_texture(texture.upcast_ref(), photo.source);
                    ui.sync_state();
                    ui.hist.replace(Some(photo.histogram));
                    if let Some(hist) = ui.histogram.borrow().as_ref() {
                        hist.queue_draw();
                    }
                    ui.dimensions
                        .set_text(&format!("{} × {} px", photo.source.0, photo.source.1));
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    ui.title.set_title(&name);
                    ui.title.set_subtitle("Camera preview · Negs");
                    ui.window
                        .set_title(Some(&format!("{name} — Negs UI Preview")));
                    ui.status.set_text("Camera preview · engine not connected");
                    ui.stage.set_visible_child_name("photo");
                    ui.strip_scroll.set_visible(true);
                    ui.current_path.replace(Some(path.clone()));
                    ui.perf.mark("preview_texture_ready_ms");
                    let p = ui.perf.clone();
                    ui.canvas.add_tick_callback(move |widget, clock| {
                        p.mark("preview_frame_requested_ms");
                        p.watch_presentation(
                            clock,
                            clock.frame_counter(),
                            "preview_frame_presented_ms",
                        );
                        p.detail("preview_surface_mapped", widget.is_mapped());
                        p.write();
                        glib::ControlFlow::Break
                    });
                    ui.load_strip(path);
                    ui.perf.write();
                }
                Ok(Err(e)) => {
                    ui.status.set_text("Could not open photograph");
                    ui.toast.add_toast(adw::Toast::new(&e));
                    if let Some(path) = ui.current_path.borrow().as_ref() {
                        ui.select_strip(path);
                    }
                }
                Err(_) => {
                    ui.status.set_text("Image decoder stopped");
                    if let Some(path) = ui.current_path.borrow().as_ref() {
                        ui.select_strip(path);
                    }
                }
            }
        });
    }
    fn select_strip(&self, path: &std::path::Path) {
        for (item, button) in self.strip_entries.borrow().iter() {
            if item == path {
                button.add_css_class("selected");
            } else {
                button.remove_css_class("selected");
            }
        }
    }
    fn load_strip(self: &Rc<Self>, path: PathBuf) {
        self.select_strip(&path);
        let folder = path.parent().map(std::path::Path::to_path_buf);
        if self.strip_folder.borrow().as_ref() == folder.as_ref() {
            return;
        }
        self.strip_folder.replace(folder);
        let ui = self.clone();
        // A photo switch must not cancel thumbnail work in the same folder.
        let generation = self.strip_generation.get() + 1;
        self.strip_generation.set(generation);
        glib::MainContext::default().spawn_local(async move {
            let paths = gio::spawn_blocking(move || crate::photo::folder(&path))
                .await
                .unwrap_or_default();
            if ui.strip_generation.get() != generation {
                return;
            }
            while let Some(child) = ui.strip.first_child() {
                ui.strip.remove(&child);
            }
            ui.strip_entries.borrow_mut().clear();
            let mut thumbnails = Vec::new();
            // Bound this visual spike to 24 files. The production strip will be virtualized.
            for path in paths.into_iter().take(24) {
                let button = gtk::Button::new();
                let cell = gtk::Box::new(gtk::Orientation::Vertical, 5);
                let picture = gtk::Picture::new();
                picture.set_size_request(104, 68);
                picture.set_content_fit(gtk::ContentFit::Contain);
                cell.append(&picture);
                let label = gtk::Label::new(Some(
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                ));
                label.set_max_width_chars(16);
                label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                cell.append(&label);
                button.set_child(Some(&cell));
                button.set_tooltip_text(Some(&path.to_string_lossy()));
                button.update_property(&[gtk::accessible::Property::Label(
                    &path.file_name().unwrap_or_default().to_string_lossy(),
                )]);
                let p = path.clone();
                button.connect_clicked(glib::clone!(
                    #[weak]
                    ui,
                    move |_| ui.open(p.clone())
                ));
                ui.strip.append(&button);
                ui.strip_entries.borrow_mut().push((path.clone(), button));
                thumbnails.push((path, picture));
            }
            if let Some(path) = ui
                .pending_path
                .borrow()
                .as_ref()
                .or(ui.current_path.borrow().as_ref())
            {
                ui.select_strip(path);
            }
            for (path, picture) in thumbnails {
                let decoded = gio::spawn_blocking(move || crate::photo::decode(&path, 192)).await;
                if ui.strip_generation.get() != generation {
                    return;
                }
                if let Ok(Ok(photo)) = decoded {
                    let format = if photo.alpha {
                        gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gdk::MemoryFormat::R8g8b8
                    };
                    let texture = gdk::MemoryTexture::new(
                        photo.width,
                        photo.height,
                        format,
                        &glib::Bytes::from_owned(photo.bytes),
                        photo.stride,
                    );
                    picture.set_paintable(Some(&texture));
                }
            }
        });
    }
    fn export_dialog(&self, lut: bool) {
        let dialog = adw::Dialog::builder()
            .title(if lut {
                "Export LUT"
            } else {
                "Export Photograph"
            })
            .content_width(440)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
        body.set_margin_start(24);
        body.set_margin_end(24);
        body.set_margin_top(16);
        body.set_margin_bottom(24);
        let group = adw::PreferencesGroup::new();
        for (title, options) in if lut {
            vec![
                ("Format", vec!["Cube (.cube)"]),
                ("Grid size", vec!["33 × 33 × 33", "65 × 65 × 65"]),
            ]
        } else {
            vec![
                ("Engine", vec!["Fast (GPU / f32)", "Reference (CPU / f64)"]),
                (
                    "Format",
                    vec!["TIFF · 16-bit", "PNG · 16-bit", "JPEG · 8-bit"],
                ),
                ("Size", vec!["Full developed size", "Longest side…"]),
            ]
        } {
            group.add(
                &adw::ComboRow::builder()
                    .title(title)
                    .model(&gtk::StringList::new(&options))
                    .build(),
            );
        }
        if !lut {
            group.add(
                &adw::SwitchRow::builder()
                    .title("Include border")
                    .active(true)
                    .build(),
            );
            group.add(
                &adw::SwitchRow::builder()
                    .title("Copy photograph metadata")
                    .active(true)
                    .build(),
            );
        }
        body.append(&group);
        let hint = gtk::Label::new(Some(
            "The export interface is ready to review. Saving will be enabled when the Rust engine is connected.",
        ));
        hint.set_wrap(true);
        hint.set_xalign(0.);
        hint.add_css_class("dim-label");
        body.append(&hint);
        let save = gtk::Button::with_label(if lut {
            "Export LUT…"
        } else {
            "Export Photograph…"
        });
        save.add_css_class("suggested-action");
        save.set_sensitive(false);
        body.append(&save);
        toolbar.set_content(Some(&body));
        dialog.set_child(Some(&toolbar));
        dialog.present(Some(&self.window));
    }
    fn appearance_dialog(&self) {
        let d = adw::Dialog::builder()
            .title("Appearance")
            .content_width(360)
            .build();
        let t = adw::ToolbarView::new();
        t.add_top_bar(&adw::HeaderBar::new());
        let b = gtk::Box::new(gtk::Orientation::Vertical, 16);
        b.set_margin_top(24);
        b.set_margin_bottom(24);
        b.set_margin_start(24);
        b.set_margin_end(24);
        let row = adw::ComboRow::builder()
            .title("Appearance")
            .model(&gtk::StringList::new(&["Follow system", "Light", "Dark"]))
            .build();
        row.connect_selected_notify(|r| {
            adw::StyleManager::default().set_color_scheme(match r.selected() {
                1 => adw::ColorScheme::ForceLight,
                2 => adw::ColorScheme::ForceDark,
                _ => adw::ColorScheme::Default,
            })
        });
        b.append(&row);
        t.set_content(Some(&b));
        d.set_child(Some(&t));
        d.present(Some(&self.window));
    }
}
