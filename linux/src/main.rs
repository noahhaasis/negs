mod actions;
mod canvas;
mod fixtures;
mod inspector;
mod model;
mod perf;
mod photo;
mod validation;

use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
};

pub(crate) struct Editor {
    window: adw::ApplicationWindow,
    canvas: canvas::Canvas,
    split: adw::OverlaySplitView,
    inspector: gtk::Box,
    history: RefCell<model::History>,
    syncing: Cell<bool>,
    adjustments: RefCell<Vec<(String, gtk::Adjustment)>>,
    choices: RefCell<Vec<(String, adw::ComboRow)>>,
    toggles: RefCell<Vec<(String, bool, adw::SwitchRow)>>,
    border: RefCell<Option<adw::ComboRow>>,
    hist: RefCell<Option<[Vec<u32>; 3]>>,
    histogram: RefCell<Option<gtk::DrawingArea>>,
    title: adw::WindowTitle,
    dimensions: gtk::Label,
    status: gtk::Label,
    strip: gtk::Box,
    strip_scroll: gtk::ScrolledWindow,
    stage: gtk::Stack,
    toast: adw::ToastOverlay,
    crop_bar: gtk::Box,
    generation: Cell<u64>,
    current_path: RefCell<Option<PathBuf>>,
    perf: perf::Perf,
    undo: gio::SimpleAction,
    redo: gio::SimpleAction,
    tabs: RefCell<Option<gtk::Stack>>,
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
            .title("Negs — UI Preview")
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
        menu.append(Some("Save Preview…"), Some("win.export"));
        menu.append(Some("Export LUT…"), Some("win.lut"));
        menu.append(Some("Appearance…"), Some("win.appearance"));
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
        split.set_sidebar(Some(&inspector));
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
        let badge = gtk::Label::new(Some("UI preview"));
        badge.add_css_class("preview-badge");
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
        let empty=adw::StatusPage::builder().icon_name("camera-photo-symbolic").title("Open a Photograph")
            .description("Your photograph, a little chemistry.\nOpen a RAW preview, TIFF, PNG, or JPEG to explore the editor.").build();
        let emptyopen = gtk::Button::with_label("Open Photograph…");
        emptyopen.set_halign(gtk::Align::Center);
        emptyopen.add_css_class("suggested-action");
        emptyopen.add_css_class("pill");
        emptyopen.set_action_name(Some("win.open"));
        empty.set_child(Some(&emptyopen));
        stage.add_named(&empty, Some("empty"));
        let well = gtk::Box::new(gtk::Orientation::Vertical, 0);
        well.add_css_class("photo-well");
        well.append(&canvas);
        let crop_bar = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        crop_bar.set_halign(gtk::Align::Center);
        crop_bar.add_css_class("crop-hint");
        crop_bar.set_visible(false);
        let hint = gtk::Label::new(Some("Drag corners to crop"));
        crop_bar.append(&hint);
        for (label, action) in [
            ("Original", "win.crop-original"),
            ("3:2", "win.crop-3-2"),
            ("Square", "win.crop-square"),
            ("Cancel", "win.crop-cancel"),
            ("Done", "win.crop-done"),
        ] {
            let b = gtk::Button::with_label(label);
            b.set_action_name(Some(action));
            if label == "Done" {
                b.add_css_class("suggested-action");
            }
            crop_bar.append(&b);
        }
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
        let status = gtk::Label::new(Some("Camera preview · engine not connected"));
        status.add_css_class("dim-label");
        statusbar.append(&status);
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_max_width_chars(36);
        dimensions.set_ellipsize(gtk::pango::EllipsizeMode::End);
        dimensions.set_max_width_chars(22);
        let original = gtk::Button::with_label("Original");
        original.add_css_class("flat");
        original.set_tooltip_text(Some(
            "This UI preview displays the camera original; film processing is not connected.",
        ));
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
            status,
            strip,
            strip_scroll,
            stage,
            toast,
            crop_bar,
            generation: Cell::new(0),
            current_path: RefCell::new(None),
            perf,
            undo: gio::SimpleAction::new("undo", None),
            redo: gio::SimpleAction::new("redo", None),
            tabs: RefCell::new(None),
        });
        ui.actions(app);
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
            if let Some(ui) = hold.borrow_mut().take() {
                ui.save_session();
                ui.perf.write();
            }
            glib::Propagation::Proceed
        });
        ui
    }
    fn sync_history(&self) {
        let h = self.history.borrow();
        self.undo.set_enabled(h.can_undo());
        self.redo.set_enabled(h.can_redo());
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
        if let Some(row) = self.border.borrow().as_ref() {
            row.set_selected(h.state.border as u32);
        }
        self.canvas.apply(&h.state);
        drop(h);
        self.syncing.set(false);
        self.sync_history();
    }
    fn session_path() -> PathBuf {
        std::env::var_os("NEGS_PROFILE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| glib::user_config_dir().join("negs/ui-preview"))
            .join("session.json")
    }
    fn save_session(&self) {
        let path = Self::session_path();
        let value = serde_json::json!({"ui_only":true,"photo":*self.current_path.borrow(),"state":self.history.borrow().state});
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(bytes) = serde_json::to_vec_pretty(&value)
            && std::fs::write(path.with_extension("tmp"), bytes).is_ok()
        {
            let _ = std::fs::rename(path.with_extension("tmp"), path);
        }
    }
}

fn main() -> glib::ExitCode {
    let perf = perf::Perf::new();
    perf.mark("main_entered_ms");
    let args: Vec<_> = std::env::args().skip(1).collect();
    let app = adw::Application::builder()
        .application_id("dev.negs.app.UiPreview")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
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
        let session = std::fs::read(Editor::session_path())
            .ok()
            .and_then(|s| serde_json::from_slice::<serde_json::Value>(&s).ok());
        if !args.iter().any(|s| s == "--fresh")
            && let Some(state) = session
                .as_ref()
                .and_then(|s| s.get("state"))
                .and_then(|v| serde_json::from_value(v.clone()).ok())
        {
            ui.history.borrow_mut().state = state;
        }
        ui.window.present();
        ui.perf.mark("present_called_ms");
        let deferred = Rc::new(Cell::new(false));
        if let Some(clock) = ui.window.frame_clock() {
            clock.connect_after_paint(glib::clone!(
                #[weak]
                ui,
                move |_| {
                    if !deferred.replace(true) {
                        glib::idle_add_local_once(move || {
                            inspector::build(&ui);
                            ui.sync_state();
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
    app.run_with_args(&["negs"])
}
