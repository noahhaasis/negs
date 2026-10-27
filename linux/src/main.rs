mod actions;
mod border;
mod border_ui;
mod canvas;
mod catalog;
mod crop_geometry;
mod crop_ui;
mod engine;
mod engine_validation;
mod export_ui;
mod exports;
mod filmstrip;
mod fixtures;
mod image_size;
mod inspector;
mod model;
mod parameters;
mod perf;
mod photo;
mod preferences;
mod preview_cache;
mod runtime;
mod validation;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

type StateRule = Box<dyn Fn(&model::State)>;

pub(crate) struct Editor {
    owner: RefCell<std::rc::Weak<Editor>>,
    runtime: RefCell<runtime::Runtime>,
    shell_painted: Cell<bool>,
    cache: std::sync::Arc<preview_cache::Cache>,
    pending_view: RefCell<Option<serde_json::Value>>,
    window: adw::ApplicationWindow,
    canvas: canvas::Canvas,
    split: adw::OverlaySplitView,
    inspector: gtk::Box,
    history: RefCell<model::History>,
    syncing: Cell<bool>,
    adjustments: RefCell<Vec<(String, gtk::Adjustment)>>,
    choices: RefCell<Vec<(String, adw::ComboRow)>>,
    toggles: RefCell<Vec<(String, bool, adw::SwitchRow)>>,
    border: RefCell<Option<border_ui::Controls>>,
    hist: RefCell<Option<[Vec<u32>; 3]>>,
    histogram: RefCell<Option<gtk::DrawingArea>>,
    title: adw::WindowTitle,
    dimensions: gtk::Label,
    /// How the photograph's colours were read (Input Colour).
    input_summary: gtk::Label,
    status: gtk::Label,
    badge: gtk::Label,
    export_progress: gtk::Box,
    export_elapsed: gtk::Label,
    export_cancel: gtk::Button,
    strip_scroll: gtk::ScrolledWindow,
    strip_folder: RefCell<Option<PathBuf>>,
    strip_generation: Cell<u64>,
    strip_paths: RefCell<Vec<PathBuf>>,
    strip_model: RefCell<Option<gtk::StringList>>,
    strip_selection: RefCell<Option<gtk::SingleSelection>>,
    strip_view: RefCell<Option<gtk::ListView>>,
    thumbnail_cache: RefCell<std::collections::VecDeque<filmstrip::Cached>>,
    thumbnail_jobs: RefCell<std::collections::VecDeque<filmstrip::Job>>,
    thumbnail_workers: Cell<u32>,
    changed_paths: RefCell<std::collections::BTreeSet<PathBuf>>,
    current_stamp: Cell<Option<filmstrip::Stamp>>,
    last_color: RefCell<Option<(bool, std::time::Instant, model::State)>>,
    stage: gtk::Stack,
    toast: adw::ToastOverlay,
    crop_bar: gtk::Box,
    generation: Cell<u64>,
    current_path: RefCell<Option<PathBuf>>,
    pending_path: RefCell<Option<PathBuf>>,
    perf: perf::Perf,
    undo: gio::SimpleAction,
    redo: gio::SimpleAction,
    tabs: RefCell<Option<gtk::Stack>>,
    rules: RefCell<Vec<StateRule>>,
    history_labels: RefCell<(String, String)>,
    crop_before: RefCell<Option<model::State>>,
    recent: RefCell<Vec<PathBuf>>,
    recent_menu: gio::Menu,
    empty_recent: gtk::Box,
    history_menu: gio::Menu,
    folder_monitor: RefCell<Option<gio::FileMonitor>>,
    folder_debounce: RefCell<Option<glib::SourceId>>,
    preferences: RefCell<preferences::Preferences>,
}

fn icon(name: &str, tooltip: &str, action: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(name);
    b.set_tooltip_text(Some(tooltip));
    b.set_action_name(Some(action));
    b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    b
}
fn gap() -> gtk::Box {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    b.set_hexpand(true);
    b
}

impl Editor {
    fn new(app: &adw::Application, perf: perf::Perf) -> Rc<Self> {
        let window = adw::ApplicationWindow::builder()
            .application(app)
            .title(if runtime::ui_only() {
                "Negs — UI Preview"
            } else {
                "Negs"
            })
            .default_width(1320)
            .default_height(900)
            .width_request(360)
            .height_request(480)
            .build();
        let toolbar = adw::ToolbarView::new();
        let header = adw::HeaderBar::new();
        let title = adw::WindowTitle::new("Negs", "A digital darkroom");
        header.set_title_widget(Some(&title));
        let open = gtk::Button::builder()
            .label("Open")
            .icon_name("document-open-symbolic")
            .action_name("win.open")
            .build();
        open.set_tooltip_text(Some("Open Photograph (Ctrl+O)"));
        header.pack_start(&open);
        let undos = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        undos.add_css_class("linked");
        undos.append(&icon("edit-undo-symbolic", "Undo (Ctrl+Z)", "win.undo"));
        undos.append(&icon(
            "edit-redo-symbolic",
            "Redo (Ctrl+Shift+Z)",
            "win.redo",
        ));
        header.pack_start(&undos);
        let menu = gio::Menu::new();
        menu.append(Some("Open Photograph…"), Some("win.open"));
        let recent_menu = gio::Menu::new();
        menu.append_submenu(Some("Open Recent"), &recent_menu);
        menu.append(Some("Save Preview…"), Some("win.save-preview"));
        menu.append(Some("Export Photograph…"), Some("win.export"));
        menu.append(Some("Export LUT…"), Some("win.lut"));
        let edit = gio::Menu::new();
        edit.append(Some("Undo"), Some("win.undo"));
        edit.append(Some("Redo"), Some("win.redo"));
        edit.append(Some("Reset Adjustments"), Some("win.reset"));
        menu.append_section(None, &edit);
        let view = gio::Menu::new();
        for (title, action) in [
            ("Fit Photograph", "fit"),
            ("Actual Size", "actual"),
            ("Show Original", "original"),
            ("Crop…", "crop"),
            ("Rotate Clockwise", "rotate"),
            ("Rotate Counterclockwise", "rotate-left"),
            ("Toggle Film Strip", "strip"),
            ("Toggle Adjustments", "inspector"),
            ("Previous Photograph", "previous"),
            ("Next Photograph", "next"),
        ] {
            view.append(Some(title), Some(&format!("win.{action}")));
        }
        menu.append_submenu(Some("View"), &view);
        menu.append(Some("Settings…"), Some("win.settings"));
        menu.append(Some("Keyboard Shortcuts"), Some("win.shortcuts"));
        menu.append(Some("Quit"), Some("win.quit"));
        menu.append(Some("About Negs"), Some("win.about"));
        let menub = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Menu")
            .build();
        header.pack_end(&menub);
        header.pack_end(&icon(
            "sidebar-show-right-symbolic",
            "Show Adjustments (F7)",
            "win.inspector",
        ));
        let export = gtk::Button::with_label("Export…");
        export.add_css_class("suggested-action");
        export.set_action_name(Some("win.export"));
        header.pack_end(&export);
        toolbar.add_top_bar(&header);
        let split = adw::OverlaySplitView::builder()
            .sidebar_position(gtk::PackType::End)
            .min_sidebar_width(320.)
            .max_sidebar_width(360.)
            .sidebar_width_fraction(0.27)
            .show_sidebar(true)
            .build();
        let inspector = gtk::Box::new(gtk::Orientation::Vertical, 0);
        inspector.add_css_class("inspector");
        let sidebar = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let resize = gtk::Button::new();
        resize.add_css_class("inspector-resize");
        resize.add_css_class("flat");
        resize.set_width_request(7);
        resize.set_cursor_from_name(Some("ew-resize"));
        resize.set_tooltip_text(Some(
            "Resize adjustments · drag or use Left/Right arrow keys",
        ));
        resize.update_property(&[gtk::accessible::Property::Label("Resize Adjustments")]);
        sidebar.append(&resize);
        sidebar.append(&inspector);
        inspector.set_hexpand(true);
        split.set_sidebar(Some(&sidebar));
        let main = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let tools = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        tools.add_css_class("canvas-toolbar");
        tools.append(&icon(
            "view-grid-symbolic",
            "Toggle Film Strip (F6)",
            "win.strip",
        ));
        tools.append(&gtk::Separator::new(gtk::Orientation::Vertical));
        let fit = gtk::Button::with_label("Fit");
        fit.set_action_name(Some("win.fit"));
        fit.set_tooltip_text(Some("Fit Photograph (Ctrl+0)"));
        tools.append(&fit);
        let actual = gtk::Button::with_label("100%");
        actual.set_action_name(Some("win.actual"));
        actual.set_tooltip_text(Some("Actual Size (Ctrl+1)"));
        tools.append(&actual);
        tools.append(&icon(
            "zoom-out-symbolic",
            "Zoom Out (Ctrl+-)",
            "win.zoom-out",
        ));
        tools.append(&icon("zoom-in-symbolic", "Zoom In (Ctrl++)", "win.zoom-in"));
        tools.append(&gap());
        let badge = gtk::Label::new(None);
        badge.add_css_class("preview-badge");
        badge.set_visible(false);
        tools.append(&badge);
        tools.append(&gap());
        tools.append(&icon(
            "object-rotate-right-symbolic",
            "Rotate Clockwise (Ctrl+R)",
            "win.rotate",
        ));
        tools.append(&icon("edit-cut-symbolic", "Crop (C)", "win.crop"));
        main.append(&tools);
        let canvas = canvas::Canvas::new();
        let stage = gtk::Stack::new();
        stage.set_vexpand(true);
        let empty = adw::StatusPage::builder()
            .icon_name("camera-photo-symbolic")
            .title("Open a Photograph")
            .description(
                "Your photograph, a little chemistry.\nOpen a RAW, TIFF, PNG, or JPEG to begin.",
            )
            .build();
        let emptyopen = gtk::Button::with_label("Open Photograph…");
        emptyopen.set_halign(gtk::Align::Center);
        emptyopen.add_css_class("suggested-action");
        emptyopen.add_css_class("pill");
        emptyopen.set_action_name(Some("win.open"));
        let empty_content = gtk::Box::new(gtk::Orientation::Vertical, 12);
        empty_content.append(&emptyopen);
        let empty_recent = gtk::Box::new(gtk::Orientation::Vertical, 4);
        empty_content.append(&empty_recent);
        empty.set_child(Some(&empty_content));
        stage.add_named(&empty, Some("empty"));
        let well = gtk::Box::new(gtk::Orientation::Vertical, 0);
        well.add_css_class("photo-well");
        well.append(&canvas);
        let crop_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        crop_bar.set_halign(gtk::Align::Center);
        crop_bar.add_css_class("crop-hint");
        crop_bar.set_visible(false);
        well.append(&crop_bar);
        stage.add_named(&well, Some("photo"));
        main.append(&stage);
        let strip = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        strip.add_css_class("filmstrip");
        let strip_scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .height_request(118)
            .child(&strip)
            .visible(false)
            .build();
        main.append(&strip_scroll);
        let statusbar = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        statusbar.add_css_class("statusbar");
        let dimensions = gtk::Label::new(Some("No photograph open"));
        dimensions.add_css_class("dim-label");
        statusbar.append(&dimensions);
        statusbar.append(&gap());
        let status = gtk::Label::new(Some(if runtime::ui_only() {
            "Camera preview · engine not connected"
        } else {
            "Preparing image engine…"
        }));
        status.add_css_class("dim-label");
        status.set_selectable(true);
        dimensions.set_selectable(true);
        statusbar.append(&status);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_max_width_chars(36);
        let export_progress = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let spinner = gtk::Spinner::new();
        spinner.start();
        export_progress.append(&spinner);
        let export_elapsed = gtk::Label::new(None);
        export_progress.append(&export_elapsed);
        let export_cancel = gtk::Button::with_label("Cancel");
        export_cancel.set_action_name(Some("win.cancel-export"));
        export_progress.append(&export_cancel);
        export_progress.set_visible(false);
        statusbar.append(&export_progress);
        dimensions.set_ellipsize(gtk::pango::EllipsizeMode::End);
        dimensions.set_max_width_chars(22);
        let original = gtk::Button::with_label("Original");
        original.add_css_class("flat");
        original.set_tooltip_text(Some("Hold to show Original (or hold \\)"));
        statusbar.append(&original);
        main.append(&statusbar);
        split.set_content(Some(&main));
        toolbar.set_content(Some(&split));
        let toast = adw::ToastOverlay::new();
        toast.set_child(Some(&toolbar));
        window.set_content(Some(&toast));
        let breakpoint =
            adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 900sp").unwrap());
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        breakpoint.connect_apply(glib::clone!(
            #[weak]
            split,
            move |_| split.set_show_sidebar(false)
        ));
        window.add_breakpoint(breakpoint);
        let history = model::History::new(fixtures::state());
        let ui = Rc::new(Self {
            owner: RefCell::new(std::rc::Weak::new()),
            runtime: RefCell::new(runtime::Runtime::default()),
            shell_painted: Cell::new(false),
            cache: std::sync::Arc::new(preview_cache::Cache::new(preview_cache::location())),
            pending_view: RefCell::new(None),
            window,
            canvas,
            split,
            inspector,
            history: RefCell::new(history),
            syncing: Cell::new(false),
            adjustments: RefCell::new(vec![]),
            choices: RefCell::new(vec![]),
            toggles: RefCell::new(vec![]),
            border: RefCell::new(None),
            hist: RefCell::new(None),
            histogram: RefCell::new(None),
            title,
            dimensions,
            input_summary: gtk::Label::new(None),
            status,
            badge,
            export_progress,
            export_elapsed,
            export_cancel,
            strip_scroll,
            strip_folder: RefCell::new(None),
            strip_generation: Cell::new(0),
            strip_paths: RefCell::new(Vec::new()),
            strip_model: RefCell::new(None),
            strip_selection: RefCell::new(None),
            strip_view: RefCell::new(None),
            thumbnail_cache: RefCell::new(std::collections::VecDeque::new()),
            thumbnail_jobs: RefCell::new(std::collections::VecDeque::new()),
            thumbnail_workers: Cell::new(0),
            changed_paths: RefCell::new(std::collections::BTreeSet::new()),
            current_stamp: Cell::new(None),
            last_color: RefCell::new(None),
            stage,
            toast,
            crop_bar,
            generation: Cell::new(0),
            current_path: RefCell::new(None),
            pending_path: RefCell::new(None),
            perf,
            undo: gio::SimpleAction::new("undo", None),
            redo: gio::SimpleAction::new("redo", None),
            tabs: RefCell::new(None),
            rules: RefCell::new(Vec::new()),
            history_labels: RefCell::new((String::new(), String::new())),
            crop_before: RefCell::new(None),
            recent: RefCell::new(Vec::new()),
            recent_menu,
            empty_recent,
            history_menu: edit,
            folder_monitor: RefCell::new(None),
            folder_debounce: RefCell::new(None),
            preferences: RefCell::new(preferences::Preferences::default()),
        });
        ui.owner.replace(Rc::downgrade(&ui));
        let hold_original = gtk::GestureClick::new();
        hold_original.set_propagation_phase(gtk::PropagationPhase::Capture);
        hold_original.connect_pressed(glib::clone!(
            #[weak]
            ui,
            move |gesture, _, _, _| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                ui.show_original(true);
            }
        ));
        hold_original.connect_released(glib::clone!(
            #[weak]
            ui,
            move |_, _, _, _| ui.show_original(false)
        ));
        hold_original.connect_cancel(glib::clone!(
            #[weak]
            ui,
            move |_, _| ui.show_original(false)
        ));
        original.add_controller(hold_original);
        let original_keys = gtk::EventControllerKey::new();
        original_keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        original_keys.connect_key_pressed(glib::clone!(
            #[weak]
            ui,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if matches!(key, gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter) {
                    ui.show_original(true);
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        original_keys.connect_key_released(glib::clone!(
            #[weak]
            ui,
            move |_, key, _, _| {
                if matches!(key, gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter) {
                    ui.show_original(false);
                }
            }
        ));
        original.add_controller(original_keys);
        let original_focus = gtk::EventControllerFocus::new();
        original_focus.connect_leave(glib::clone!(
            #[weak]
            ui,
            move |_| ui.show_original(false)
        ));
        original.add_controller(original_focus);
        ui.window.connect_is_active_notify(glib::clone!(
            #[weak]
            ui,
            move |w| if !w.is_active() {
                ui.show_original(false);
            }
        ));
        crop_ui::build(&ui);
        filmstrip::build(&ui);
        let resize_origin = Rc::new(Cell::new(340.));
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            ui,
            #[strong]
            resize_origin,
            move |_, _, _| resize_origin.set(ui.inspector.width() as f64)
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            ui,
            move |_, dx, _| ui.resize_inspector(resize_origin.get() - dx)
        ));
        resize.add_controller(drag);
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            ui,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                let delta = match key {
                    gdk::Key::Left => 16.,
                    gdk::Key::Right => -16.,
                    _ => return glib::Propagation::Proceed,
                };
                ui.resize_inspector(ui.inspector.width() as f64 + delta);
                glib::Propagation::Stop
            }
        ));
        resize.add_controller(keys);
        ui.actions(app);
        // Keep availability in sync without activating the menu's toggle action
        // after the hold gesture releases.
        ui.window
            .lookup_action("original")
            .unwrap()
            .bind_property("enabled", &original, "sensitive")
            .sync_create()
            .build();
        ui.sync_history();
        ui.perf.watch(&ui.window);
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak]
            ui,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, _, _, _| {
                ui.perf.input();
                glib::Propagation::Proceed
            }
        ));
        ui.window.add_controller(keys);
        let clicks = gtk::GestureClick::new();
        clicks.set_propagation_phase(gtk::PropagationPhase::Capture);
        clicks.connect_pressed(glib::clone!(
            #[weak]
            ui,
            move |_, _, _, _| ui.perf.input()
        ));
        ui.window.add_controller(clicks);
        let drop = gtk::DropTarget::new(gio::File::static_type(), gdk::DragAction::COPY);
        drop.connect_drop(glib::clone!(
            #[weak]
            ui,
            #[upgrade_or]
            false,
            move |_, value, _, _| {
                if let Ok(file) = value.get::<gio::File>()
                    && let Some(path) = file.path()
                {
                    ui.open(path);
                    return true;
                }
                false
            }
        ));
        ui.stage.add_controller(drop);
        let hold = Rc::new(RefCell::new(Some(ui.clone())));
        ui.window.connect_close_request(move |_| {
            if let Some(ui)=hold.borrow().clone() {
                if ui.runtime.borrow().preview_saving {ui.toast.add_toast(adw::Toast::new("Wait for Save Preview to finish before quitting."));return glib::Propagation::Stop;}
                if ui.runtime.borrow().exporting.is_some() && !ui.runtime.borrow().close_confirmed {
                    let dialog=adw::AlertDialog::builder().heading("An export is still in progress.")
                        .body("Quitting cancels it. Any existing file at the destination is left unchanged.").build();
                    dialog.add_response("continue","Continue Exporting");dialog.add_response("quit","Quit");
                    dialog.set_default_response(Some("continue"));dialog.set_close_response("continue");
                    dialog.connect_response(None,glib::clone!(#[weak] ui,move |_,response|if response=="quit" {ui.runtime.borrow_mut().close_confirmed=true;ui.window.close();}));
                    dialog.present(Some(&ui.window));return glib::Propagation::Stop;
                }
            }
            let ui = hold.borrow_mut().take();
            if let Some(ui) = ui {
                if let Err(error) = ui.save_session() {
                    ui.toast.add_toast(adw::Toast::new(&format!(
                        "Could not save the UI session: {error}"
                    )));
                    hold.borrow_mut().replace(ui);
                    return glib::Propagation::Stop;
                }
                ui.perf.write();
                let mut runtime = ui.runtime.borrow_mut();
                runtime.closing = true;
                if let Some(job)=&runtime.exporting {job.cancel();}
                if let Some(client) = &runtime.client { client.stop(); }
                std::mem::drop(runtime);
                ui.window.set_sensitive(false);
                let cache=ui.cache.clone();
                glib::MainContext::default().spawn_local(async move {
                    let _=gio::spawn_blocking(move ||cache.flush()).await;
                    let app=ui.window.application();ui.window.destroy();
                    if let Some(app)=app{app.quit();}
                });
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        });
        ui
    }
    fn resize_inspector(&self, width: f64) {
        let width = width.clamp(280., 560.);
        self.split.set_min_sidebar_width(width);
        self.split.set_max_sidebar_width(width);
        self.preferences.borrow_mut().sidebar_width = width;
    }
    fn sync_history(&self) {
        let h = self.history.borrow();
        self.undo.set_enabled(h.can_undo());
        self.redo.set_enabled(h.can_redo());
        let labels = (
            h.undo_name()
                .map(|s| format!("Undo {s}"))
                .unwrap_or("Undo".into()),
            h.redo_name()
                .map(|s| format!("Redo {s}"))
                .unwrap_or("Redo".into()),
        );
        if *self.history_labels.borrow() != labels {
            self.history_menu.remove_all();
            self.history_menu.append(Some(&labels.0), Some("win.undo"));
            self.history_menu.append(Some(&labels.1), Some("win.redo"));
            self.history_menu
                .append(Some("Reset Adjustments"), Some("win.reset"));
            self.history_labels.replace(labels);
        }
        let has_photo = self.current_path.borrow().is_some();
        let cropping = self.canvas.cropping();
        for key in [
            "fit",
            "actual",
            "reset",
            "original",
            "export",
            "save-preview",
            "previous",
            "next",
            "strip",
        ] {
            if let Some(a) = self
                .window
                .lookup_action(key)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(!cropping && (has_photo || key == "reset"));
            }
        }
        for key in ["rotate", "rotate-left", "crop", "zoom-in", "zoom-out"] {
            if let Some(a) = self
                .window
                .lookup_action(key)
                .and_downcast::<gio::SimpleAction>()
            {
                a.set_enabled(has_photo);
            }
        }
        if !runtime::ui_only() {
            let r = self.runtime.borrow();
            for (key, enabled) in [
                (
                    "export",
                    has_photo && r.ready && !cropping && r.exporting.is_none(),
                ),
                (
                    "save-preview",
                    has_photo && r.retained_id.is_some() && !cropping && r.exporting.is_none(),
                ),
                ("lut", r.ready && !cropping && r.exporting.is_none()),
                ("original", r.developed.is_some() && !cropping),
            ] {
                if let Some(action) = self
                    .window
                    .lookup_action(key)
                    .and_downcast::<gio::SimpleAction>()
                {
                    action.set_enabled(enabled);
                }
            }
        }
        self.strip_scroll.set_sensitive(!cropping);
        self.inspector.set_sensitive(!cropping);
        self.undo.set_enabled(!cropping && h.can_undo());
        self.redo.set_enabled(!cropping && h.can_redo());
        let syncing = self.syncing.replace(true);
        for rule in self.rules.borrow().iter() {
            rule(&h.state);
        }
        self.syncing.set(syncing);
        drop(h);
        self.observe_edit();
    }
    fn sync_state(&self) {
        self.syncing.set(true);
        let h = self.history.borrow();
        for (key, a) in self.adjustments.borrow().iter() {
            if let Some(v) = h.state.values.get(key) {
                a.set_value(*v);
            }
        }
        for (key, row) in self.choices.borrow().iter() {
            row.set_selected(
                h.state
                    .choices
                    .get(key)
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            );
        }
        for (key, default, row) in self.toggles.borrow().iter() {
            row.set_active(
                h.state
                    .choices
                    .get(key)
                    .map(|v| v == "true")
                    .unwrap_or(*default),
            );
        }
        if let Some(controls) = self.border.borrow().as_ref() {
            controls.sync(&h.state.border);
        }
        self.canvas.apply(&h.state);
        drop(h);
        self.syncing.set(false);
        self.sync_history();
    }
    fn session_path() -> PathBuf {
        std::env::var_os("NEGS_PROFILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                glib::user_config_dir().join(if runtime::ui_only() {
                    "negs/ui-preview"
                } else {
                    "negs"
                })
            })
            .join("session.json")
    }
    fn save_session(&self) -> Result<(), String> {
        let path = Self::session_path();
        let mut prefs = self.preferences.borrow_mut();
        prefs.inspector = self.split.shows_sidebar();
        prefs.strip = self.strip_scroll.is_visible();
        prefs.width = self.window.width();
        prefs.height = self.window.height();
        prefs.maximized = self.window.is_maximized();
        let mut state = self.history.borrow().state.clone();
        if self.canvas.cropping() {
            state.crop = Some(self.canvas.crop_draft());
            state.straighten = self.canvas.draft_angle();
        }
        let value = serde_json::json!({"schema":1,"ui_only":runtime::ui_only(),"photo":*self.current_path.borrow(),"state":state,"preferences":*prefs,"recent":*self.recent.borrow(),"view":self.canvas.view()});
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let bytes = serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?;
        std::fs::write(path.with_extension("tmp"), bytes).map_err(|e| e.to_string())?;
        std::fs::rename(path.with_extension("tmp"), path).map_err(|e| e.to_string())
    }
}

fn data_root() -> PathBuf {
    if let Some(path) =
        std::env::var_os("SPEKTRAFILM_DATA_DIR").or_else(|| std::env::var_os("NEGS_DATA_DIR"))
    {
        return PathBuf::from(path);
    }
    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        for root in [
            directory.join("../share/negs/data"),
            directory.join("../data"),
        ] {
            if root.is_dir() {
                return root;
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../data")
}

fn main() -> glib::ExitCode {
    let perf = perf::Perf::new();
    perf.mark("main_entered_ms");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let app = adw::Application::builder()
        .application_id(if runtime::ui_only() {
            "dev.negs.app.UiPreview"
        } else {
            "dev.negs.app"
        })
        .flags(
            if std::env::var_os("NEGS_PROFILE_DIR").is_some()
                || args.iter().any(|a| a == "--fresh" || a == "--demo")
            {
                gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN
            } else {
                gio::ApplicationFlags::HANDLES_OPEN
            },
        )
        .build();
    app.connect_open(|app, files, _| {
        app.activate();
        if let Some(window) = app.active_window() {
            if let Some(path) = files.first().and_then(|f| f.path()) {
                let _ = gtk::prelude::WidgetExt::activate_action(
                    &window,
                    "win.open-path",
                    Some(&path.to_string_lossy().to_string().to_variant()),
                );
            }
            window.present();
        }
    });
    let launch_files: Vec<String> = std::iter::once("negs".to_string())
        .chain(args.iter().filter(|s| !s.starts_with('-')).cloned())
        .collect();
    app.connect_activate(move |app| {
        if let Some(window) = app.active_window() {
            window.present();
            return;
        }
        if let Some(display) = gdk::Display::default() {
            let css = gtk::CssProvider::new();
            css.load_from_string(include_str!("../style.css"));
            gtk::style_context_add_provider_for_display(
                &display,
                &css,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
        }
        // Supply missing editor glyphs locally; preserve the desktop's selected theme.
        if let Some(display) = gdk::Display::default() {
            let icons = gtk::IconTheme::for_display(&display);
            for directory in glib::system_data_dirs() {
                let path = directory.join("icons/Adwaita/symbolic/actions");
                if path.is_dir() {
                    icons.add_search_path(path);
                }
            }
        }
        if let Ok(theme) = std::env::var("NEGS_APPEARANCE") {
            adw::StyleManager::default().set_color_scheme(match theme.as_str() {
                "light" => adw::ColorScheme::ForceLight,
                "dark" => adw::ColorScheme::ForceDark,
                _ => adw::ColorScheme::Default,
            });
        }
        let ui = Editor::new(app, perf.clone());
        let session = std::fs::metadata(Editor::session_path())
            .ok()
            .filter(|m| m.len() <= 1_048_576)
            .and_then(|_| std::fs::read(Editor::session_path()).ok())
            .and_then(|s| serde_json::from_slice::<serde_json::Value>(&s).ok());
        if !args
            .iter()
            .any(|s| s == "--fresh" || s == "--demo" || !s.starts_with('-'))
            && let Some(state) = session
                .as_ref()
                .and_then(|s| s.get("state"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            ui.history.borrow_mut().state = state;
            ui.history.borrow_mut().state.sanitize();
            ui.pending_view
                .replace(session.as_ref().and_then(|s| s.get("view")).cloned());
        }
        if !args.iter().any(|s| s == "--fresh")
            && let Some(session) = session.as_ref()
        {
            ui.restore_preferences(session);
        }
        ui.window.present();
        ui.start_engine();
        ui.perf.mark("present_called_ms");
        let deferred = Rc::new(Cell::new(false));
        if let Some(clock) = ui.window.frame_clock() {
            clock.connect_after_paint(glib::clone!(
                #[weak]
                ui,
                move |_| {
                    if !deferred.replace(true) {
                        glib::idle_add_local_once(move || {
                            ui.shell_painted.set(true);
                            ui.maybe_build_inspector();
                        });
                    }
                }
            ));
        }
        let path = args
            .iter()
            .find(|s| !s.starts_with('-'))
            .map(PathBuf::from)
            .or_else(|| {
                if args.iter().any(|s| s == "--demo") {
                    Some(
                        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                            .join("../target/linux-validation/ui-demo/DSC_4990.jpg"),
                    )
                } else {
                    None
                }
            })
            .or_else(|| {
                if !args.iter().any(|s| s == "--fresh") {
                    session
                        .as_ref()
                        .and_then(|s| s.get("photo"))
                        .and_then(|s| s.as_str())
                        .map(PathBuf::from)
                } else {
                    None
                }
            });
        if let Some(path) = path {
            ui.open(path);
        }
        validation::schedule(&ui);
        if let Some(report) = std::env::var_os("NEGS_ENGINE_AUDIT") {
            engine_validation::schedule(&ui, report.into());
        }
        if let Ok(ms) = std::env::var("NEGS_QUIT_AFTER_MS")
            .unwrap_or_default()
            .parse::<u64>()
        {
            glib::timeout_add_local_once(
                std::time::Duration::from_millis(ms),
                glib::clone!(
                    #[weak]
                    ui,
                    move || {
                        if let Some(dialog) = ui.window.visible_dialog() {
                            dialog.force_close();
                        }
                        ui.window.close();
                    }
                ),
            );
        }
    });
    app.run_with_args(&launch_files)
}
