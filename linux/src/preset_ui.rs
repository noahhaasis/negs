//! The Presets page of the sidebar (docs/presets-design.md). Clicking a
//! preset applies it; Enter, or clicking it again, applies it again after
//! other edits. File work and helper requests run on workers.
use crate::{
    Editor, parameters,
    presets::{self, Preset, Status},
};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

/// One listed preset's widgets.
struct Row {
    row: gtk::ListBoxRow,
    name: gtk::Stack,
    label: gtk::Label,
    entry: gtk::Entry,
    detail: gtk::Label,
}

pub struct Panel {
    pub page: gtk::Box,
    content: gtk::Stack,
    list: gtk::ListBox,
    viewport: gtk::Viewport,
    library: presets::Library,
    files: RefCell<Vec<Preset>>,
    rows: RefCell<Vec<Row>>,
    verdicts: RefCell<presets::Verdicts>,
    /// The preset whose name is being edited.
    renaming: RefCell<Option<PathBuf>>,
    /// Made by New Preset and not named yet: Escape removes it.
    unnamed: RefCell<Option<PathBuf>>,
    generation: Cell<u64>,
}

impl Panel {
    pub fn new() -> Self {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        page.add_css_class("inspector");
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        head.add_css_class("inspector-head");
        let title = gtk::Label::new(Some("PRESETS"));
        title.set_xalign(0.);
        title.set_hexpand(true);
        title.add_css_class("caption");
        title.add_css_class("inspector-title");
        head.append(&title);
        let add = crate::icon(
            "list-add-symbolic",
            "New Preset (Ctrl+Shift+S)",
            "win.new-preset",
        );
        add.add_css_class("flat");
        head.append(&add);
        let menu = gio::Menu::new();
        menu.append(Some("Import Presets…"), Some("win.import-presets"));
        menu.append(Some("Show Presets Folder"), Some("win.show-presets-folder"));
        let more = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .tooltip_text("Import presets or show their folder")
            .build();
        more.add_css_class("flat");
        more.update_property(&[gtk::accessible::Property::Label("More preset actions")]);
        head.append(&more);
        page.append(&head);

        let content = gtk::Stack::new();
        content.set_vexpand(true);
        let empty = adw::StatusPage::builder()
            .icon_name("view-list-bullet-symbolic")
            .title("No Presets")
            .description(
                "Save the film, paper and Lab settings with +, or import presets made in Negs or spektrafilm. You can also drop them here.",
            )
            .build();
        empty.add_css_class("compact");
        let import = gtk::Button::with_label("Import Presets…");
        import.set_halign(gtk::Align::Center);
        import.add_css_class("pill");
        import.set_action_name(Some("win.import-presets"));
        empty.set_child(Some(&import));
        content.add_named(&empty, Some("empty"));
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.add_css_class("navigation-sidebar");
        list.add_css_class("presets");
        list.update_property(&[gtk::accessible::Property::Label("Presets")]);
        let viewport = gtk::Viewport::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
        viewport.set_scroll_to_focus(true);
        viewport.set_child(Some(&list));
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&viewport)
            .build();
        content.add_named(&scroll, Some("list"));
        page.append(&content);
        Self {
            page,
            content,
            list,
            viewport,
            library: presets::Library::new(presets::folder()),
            files: RefCell::default(),
            rows: RefCell::default(),
            verdicts: RefCell::default(),
            renaming: RefCell::default(),
            unnamed: RefCell::default(),
            generation: Cell::new(0),
        }
    }
}

/// Connects the panel to the editor: activation, keys, context menus and
/// drag and drop.
pub fn build(ui: &Rc<Editor>) {
    let list = &ui.presets.list;
    list.connect_row_activated(glib::clone!(
        #[weak]
        ui,
        move |_, row| {
            let index = row.index() as usize;
            let preset = ui.presets.files.borrow().get(index).cloned();
            if let Some(preset) = preset
                && ui.presets.renaming.borrow().is_none()
            {
                ui.apply_preset(&preset);
            }
        }
    ));
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, mods| {
            if ui.presets.renaming.borrow().is_some() {
                return glib::Propagation::Proceed;
            }
            let Some(row) = ui.presets.list.selected_row() else {
                return glib::Propagation::Proceed;
            };
            let Some(preset) = ui.presets.files.borrow().get(row.index() as usize).cloned() else {
                return glib::Propagation::Proceed;
            };
            match key {
                gdk::Key::Delete | gdk::Key::KP_Delete => ui.delete_preset(&preset),
                gdk::Key::F2 => ui.start_rename(&preset.path, false),
                gdk::Key::Menu => ui.preset_menu(&row, None),
                gdk::Key::F10 if mods.contains(gdk::ModifierType::SHIFT_MASK) => {
                    ui.preset_menu(&row, None)
                }
                _ => return glib::Propagation::Proceed,
            }
            glib::Propagation::Stop
        }
    ));
    list.add_controller(keys);

    // Rows dragged to sort them, and files dropped between rows, which
    // import there. Files dropped elsewhere on the page go to the end.
    let drop = gtk::DropTarget::new(
        glib::Type::INVALID,
        gdk::DragAction::COPY | gdk::DragAction::MOVE,
    );
    drop.set_types(&[glib::Type::U32, gdk::FileList::static_type()]);
    drop.connect_motion(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        gdk::DragAction::empty(),
        move |target, _, y| {
            match ui.presets.list.row_at_y(y as i32) {
                Some(row) => ui.presets.list.drag_highlight_row(&row),
                None => ui.presets.list.drag_unhighlight_row(),
            }
            let internal = target
                .current_drop()
                .is_some_and(|d| d.formats().contains_type(glib::Type::U32));
            if internal {
                gdk::DragAction::MOVE
            } else {
                gdk::DragAction::COPY
            }
        }
    ));
    drop.connect_leave(glib::clone!(
        #[weak]
        ui,
        move |_| ui.presets.list.drag_unhighlight_row()
    ));
    drop.connect_drop(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        false,
        move |_, value, _, y| {
            ui.presets.list.drag_unhighlight_row();
            let position = ui.presets.drop_position(y);
            if let Ok(from) = value.get::<u32>() {
                ui.move_preset(from as usize, position);
                true
            } else if let Ok(files) = value.get::<gdk::FileList>() {
                ui.import_presets(paths(&files), Some(position));
                true
            } else {
                false
            }
        }
    ));
    list.add_controller(drop);
    let page_drop = gtk::DropTarget::new(gdk::FileList::static_type(), gdk::DragAction::COPY);
    page_drop.connect_drop(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        false,
        move |_, value, _, _| {
            let Ok(files) = value.get::<gdk::FileList>() else {
                return false;
            };
            ui.import_presets(paths(&files), None);
            true
        }
    ));
    ui.presets.page.add_controller(page_drop);
}

fn paths(files: &gdk::FileList) -> Vec<PathBuf> {
    files.files().iter().filter_map(|f| f.path()).collect()
}

impl Panel {
    /// Where a drop at `y` lands: before the row's lower half, after its upper half.
    fn drop_position(&self, y: f64) -> usize {
        let count = self.files.borrow().len();
        self.list.row_at_y(y as i32).map_or(count, |row| {
            let index = row.index() as usize;
            let middle = row
                .compute_bounds(&self.list)
                .map_or(0., |b| (b.y() + b.height() / 2.) as f64);
            if y > middle { index + 1 } else { index }
        })
    }

    fn row_index(&self, path: &std::path::Path) -> Option<usize> {
        self.files.borrow().iter().position(|p| p.path == path)
    }

    fn detail(&self, preset: &Preset) -> (String, Option<String>) {
        match self.verdicts.borrow().get(preset) {
            Some(Status::Valid { film, paper, scan }) => {
                let name = |id: &str, paper: bool| {
                    crate::catalog::stocks(paper)
                        .into_iter()
                        .find(|s| s.id == id)
                        .map_or(id.to_owned(), |s| s.name.clone())
                };
                let film = name(film, false);
                let text = if *scan {
                    format!("{film} · film scan")
                } else {
                    format!("{film} → {}", name(paper, true))
                };
                (text, None)
            }
            Some(Status::Invalid(reason)) => (reason.clone(), Some(reason.clone())),
            None => (" ".into(), None),
        }
    }

    fn update_row(&self, index: usize) {
        let (files, rows) = (self.files.borrow(), self.rows.borrow());
        let (Some(preset), Some(row)) = (files.get(index), rows.get(index)) else {
            return;
        };
        let (text, invalid) = self.detail(preset);
        row.detail.set_text(&text);
        row.detail.set_lines(if invalid.is_some() { 3 } else { 1 });
        row.row.set_tooltip_text(invalid.as_deref());
        if invalid.is_some() {
            row.label.add_css_class("dim-label");
        } else {
            row.label.remove_css_class("dim-label");
        }
    }
}

impl Editor {
    pub fn pane(&self) -> String {
        self.sidebar_pages
            .visible_child_name()
            .map_or("adjustments".into(), |n| n.to_string())
    }
    /// The Adjustments and Presets toggles: show that page, or hide the
    /// sidebar if it already shows it.
    pub fn toggle_pane(self: &Rc<Self>, name: &str) {
        if self.split.shows_sidebar() && self.pane() == name {
            self.split.set_show_sidebar(false);
        } else {
            self.show_pane(name);
        }
    }
    pub fn show_pane(self: &Rc<Self>, name: &str) {
        let opening = self.pane() != name || !self.split.shows_sidebar();
        self.sidebar_pages.set_visible_child_name(name);
        self.split.set_show_sidebar(true);
        self.preferences.borrow_mut().pane = name.into();
        self.sync_panes();
        if opening && name == "presets" {
            self.refresh_presets();
        }
    }
    pub fn sync_panes(&self) {
        let shown = self.split.shows_sidebar();
        let pane = self.pane();
        for (action, name) in [("inspector", "adjustments"), ("presets", "presets")] {
            if let Some(action) = self.window.lookup_action(action) {
                action.change_state(&(shown && pane == name).to_variant());
            }
        }
    }
    fn can_change_presets(&self) -> bool {
        let r = self.runtime.borrow();
        r.ready && r.client.is_some() && !self.canvas.cropping()
    }

    pub fn refresh_presets(self: &Rc<Self>) {
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move { ui.reload_presets().await });
    }

    /// Lists the folder again, so files copied in by hand appear, and asks
    /// the helper about files it hasn't seen.
    async fn reload_presets(self: &Rc<Self>) {
        let panel = &self.presets;
        let generation = panel.generation.get() + 1;
        panel.generation.set(generation);
        let library = panel.library.clone();
        let listed = gio::spawn_blocking(move || library.list())
            .await
            .unwrap_or_else(|_| Err("The presets folder couldn’t be read.".into()));
        if panel.generation.get() != generation {
            return;
        }
        let files = listed.unwrap_or_else(|error| {
            self.preset_error("Couldn’t list the presets", &error);
            vec![]
        });
        panel.verdicts.borrow_mut().retain(&files);
        self.show_presets(files);
        let unchecked: Vec<_> = panel
            .files
            .borrow()
            .iter()
            .filter(|p| panel.verdicts.borrow().get(p).is_none())
            .cloned()
            .collect();
        let Some(client) = self.runtime.borrow().client.clone() else {
            return;
        };
        if unchecked.is_empty() || !self.runtime.borrow().ready {
            return;
        }
        let library = panel.library.clone();
        let checked = gio::spawn_blocking(move || {
            unchecked
                .into_iter()
                .map(|preset| {
                    let reply = library
                        .read(&preset)
                        .and_then(presets::text)
                        .and_then(|text| {
                            client
                                .request(
                                    json!({"command": "preset_import", "text": text}),
                                    Duration::from_secs(30),
                                )
                                .map(|(_, result)| result)
                        });
                    (preset, presets::status(reply))
                })
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        for (preset, status) in checked {
            panel.verdicts.borrow_mut().insert(&preset, status);
            if let Some(index) = panel.row_index(&preset.path) {
                panel.update_row(index);
            }
        }
    }

    /// Rebuilds the rows for `files`, keeping the selection and a rename.
    fn show_presets(self: &Rc<Self>, files: Vec<Preset>) {
        let panel = &self.presets;
        let selected = panel.list.selected_row().and_then(|row| {
            panel
                .files
                .borrow()
                .get(row.index() as usize)
                .map(|p| p.path.clone())
        });
        while let Some(child) = panel.list.first_child() {
            panel.list.remove(&child);
        }
        panel
            .content
            .set_visible_child_name(if files.is_empty() { "empty" } else { "list" });
        let rows: Vec<_> = files
            .iter()
            .enumerate()
            .map(|(index, preset)| self.preset_row(index, preset))
            .collect();
        for row in &rows {
            panel.list.append(&row.row);
        }
        panel.files.replace(files);
        panel.rows.replace(rows);
        for index in 0..panel.files.borrow().len() {
            panel.update_row(index);
        }
        if let Some(index) = selected.and_then(|path| panel.row_index(&path)) {
            panel.list.select_row(Some(&panel.rows.borrow()[index].row));
        }
        let renaming = panel.renaming.borrow().clone();
        if let Some(path) = renaming {
            panel.renaming.replace(None);
            if panel.row_index(&path).is_some() {
                let unnamed = panel.unnamed.borrow().as_ref() == Some(&path);
                self.start_rename(&path, unnamed);
            }
        }
    }

    fn preset_row(self: &Rc<Self>, index: usize, preset: &Preset) -> Row {
        let row = gtk::ListBoxRow::new();
        let body = gtk::Box::new(gtk::Orientation::Vertical, 2);
        body.add_css_class("preset-row");
        let name = gtk::Stack::new();
        name.set_hhomogeneous(false);
        name.set_vhomogeneous(false);
        let label = gtk::Label::new(Some(&preset.name()));
        label.set_xalign(0.);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.add_named(&label, Some("label"));
        let entry = gtk::Entry::new();
        entry.update_property(&[gtk::accessible::Property::Label("Preset name")]);
        name.add_named(&entry, Some("entry"));
        body.append(&name);
        let detail = gtk::Label::new(Some(" "));
        detail.set_xalign(0.);
        detail.add_css_class("caption");
        detail.add_css_class("dim-label");
        detail.set_wrap(true);
        detail.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        detail.set_ellipsize(gtk::pango::EllipsizeMode::End);
        detail.set_lines(1);
        body.append(&detail);
        row.set_child(Some(&body));
        row.update_property(&[gtk::accessible::Property::Label(&preset.name())]);

        let path = preset.path.clone();
        entry.connect_activate(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[strong]
            path,
            move |entry| ui.finish_rename(&path, Some(entry.text().to_string()))
        ));
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[strong]
            path,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, _| {
                if key == gdk::Key::Escape {
                    ui.finish_rename(&path, None);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        entry.add_controller(keys);
        let focus = gtk::EventControllerFocus::new();
        focus.connect_leave(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[weak]
            entry,
            #[strong]
            path,
            move |_| {
                let (ui, entry, path) = (ui.clone(), entry.clone(), path.clone());
                // After the key handlers, so Escape and Enter come first.
                glib::idle_add_local_once(move || {
                    ui.finish_rename(&path, Some(entry.text().to_string()))
                });
            }
        ));
        entry.add_controller(focus);

        let actions = gio::SimpleActionGroup::new();
        for name in ["apply", "rename", "export", "delete"] {
            let action = gio::SimpleAction::new(name, None);
            action.connect_activate(glib::clone!(
                #[weak(rename_to=ui)]
                self,
                #[strong]
                path,
                move |action, _| {
                    let Some(preset) = ui
                        .presets
                        .files
                        .borrow()
                        .iter()
                        .find(|p| p.path == path)
                        .cloned()
                    else {
                        return;
                    };
                    match action.name().as_str() {
                        "apply" => ui.apply_preset(&preset),
                        "rename" => ui.start_rename(&preset.path, false),
                        "export" => ui.export_preset(&preset),
                        _ => ui.delete_preset(&preset),
                    }
                }
            ));
            actions.add_action(&action);
        }
        row.insert_action_group("preset", Some(&actions));
        let menu_click = gtk::GestureClick::new();
        menu_click.set_button(gdk::BUTTON_SECONDARY);
        menu_click.connect_pressed(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[weak]
            row,
            move |gesture, _, x, y| {
                gesture.set_state(gtk::EventSequenceState::Claimed);
                ui.presets.list.select_row(Some(&row));
                ui.preset_menu(&row, Some((x, y)));
            }
        ));
        row.add_controller(menu_click);

        let drag = gtk::DragSource::new();
        drag.set_actions(gdk::DragAction::MOVE);
        drag.connect_prepare(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[upgrade_or]
            None,
            move |_, _, _| {
                ui.presets
                    .renaming
                    .borrow()
                    .is_none()
                    .then(|| gdk::ContentProvider::for_value(&(index as u32).to_value()))
            }
        ));
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            row,
            move |source, _| {
                source.set_icon(Some(&gtk::WidgetPaintable::new(Some(&row))), 0, 0);
            }
        ));
        row.add_controller(drag);
        Row {
            row,
            name,
            label,
            entry,
            detail,
        }
    }

    fn preset_menu(&self, row: &gtk::ListBoxRow, at: Option<(f64, f64)>) {
        let index = row.index() as usize;
        let invalid = self.presets.files.borrow().get(index).is_some_and(|p| {
            matches!(
                self.presets.verdicts.borrow().get(p),
                Some(Status::Invalid(_))
            )
        });
        let menu = gio::Menu::new();
        if !invalid {
            menu.append(Some("Apply"), Some("preset.apply"));
        }
        menu.append(Some("Rename"), Some("preset.rename"));
        menu.append(Some("Export…"), Some("preset.export"));
        let destructive = gio::Menu::new();
        destructive.append(Some("Delete…"), Some("preset.delete"));
        menu.append_section(None, &destructive);
        let popover = gtk::PopoverMenu::from_model(Some(&menu));
        popover.set_parent(row);
        popover.set_has_arrow(false);
        popover.set_halign(gtk::Align::Start);
        let (x, y) = at.unwrap_or((24., row.height() as f64));
        popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
        popover.connect_closed(|popover| {
            let popover = popover.clone();
            glib::idle_add_local_once(move || popover.unparent());
        });
        popover.popup();
    }

    /// The film, paper and Lab settings of a preset, as one undo step. The
    /// photograph's own settings stay: Image Size, rotation, crop, border,
    /// output encoding, the preview limit and the Post tab.
    pub fn apply_preset(self: &Rc<Self>, preset: &Preset) {
        if !self.can_change_presets() {
            return;
        }
        if let Some(Status::Invalid(reason)) = self.presets.verdicts.borrow().get(preset) {
            self.status.set_text(reason);
            return;
        }
        let Some(client) = self.runtime.borrow().client.clone() else {
            return;
        };
        let (library, preset, ui) = (self.presets.library.clone(), preset.clone(), self.clone());
        glib::MainContext::default().spawn_local(async move {
            let name = preset.name();
            let reader = library.clone();
            let file = preset.clone();
            let text = gio::spawn_blocking(move || reader.read(&file).and_then(presets::text))
                .await
                .unwrap_or_else(|_| Err("The preset couldn’t be read.".into()));
            let text = match text {
                Ok(text) => text,
                Err(error) => {
                    ui.preset_error("Couldn’t apply the preset", &error);
                    ui.refresh_presets();
                    return;
                }
            };
            // The helper keeps the photograph's settings from the params it's
            // given; ask again if they changed while it answered.
            for _ in 0..3 {
                let snapshot = ui.history.borrow().state.clone();
                let params = parameters::runtime(&snapshot, &ui.runtime.borrow().defaults);
                let command = json!({"command": "preset_import", "text": text, "params": params});
                let client = client.clone();
                let reply =
                    gio::spawn_blocking(move || client.request(command, Duration::from_secs(30)))
                        .await
                        .unwrap_or_else(|_| Err("The image worker stopped unexpectedly.".into()));
                if ui.runtime.borrow().closing {
                    return;
                }
                if ui.history.borrow().state != snapshot || ui.canvas.cropping() {
                    continue;
                }
                let applied = reply.and_then(|(_, result)| {
                    let mut next = snapshot.clone();
                    presets::apply(
                        &mut next,
                        result["film"].as_str().unwrap_or_default(),
                        result["paper"].as_str().unwrap_or_default(),
                        &result["params"],
                    )?;
                    Ok(next)
                });
                match applied {
                    Ok(next) => {
                        let mut h = ui.history.borrow_mut();
                        h.commit();
                        h.begin(&format!("Apply Preset “{name}”"));
                        h.state = next;
                        h.commit();
                        drop(h);
                        ui.sync_state();
                        ui.status.set_text(&format!("Applied preset “{name}”"));
                        // Also when applied from the context menu.
                        if let Some(index) = ui.presets.row_index(&preset.path) {
                            ui.presets
                                .list
                                .select_row(Some(&ui.presets.rows.borrow()[index].row));
                        }
                    }
                    Err(error) => {
                        ui.preset_error("Couldn’t apply the preset", &error);
                        ui.refresh_presets();
                    }
                }
                return;
            }
        });
    }

    /// Saves the film, paper and Lab settings as a new preset at the end of
    /// the list, named after the film, and starts renaming it.
    pub fn new_preset(self: &Rc<Self>) {
        if !self.can_change_presets() {
            return;
        }
        let Some(client) = self.runtime.borrow().client.clone() else {
            return;
        };
        self.show_pane("presets");
        let state = self.history.borrow().state.clone();
        let params = parameters::runtime(&state, &self.runtime.borrow().defaults);
        let film = crate::catalog::selected(&state, false);
        let paper = crate::catalog::selected(&state, true);
        let command = json!({"command": "preset_export", "film": film.id, "paper": paper.id, "params": params});
        let name = presets::from_file_name(&film.name);
        let (library, ui) = (self.presets.library.clone(), self.clone());
        glib::MainContext::default().spawn_local(async move {
            let added = gio::spawn_blocking(move || {
                let (_, result) = client.request(command, Duration::from_secs(30))?;
                let text = result["text"]
                    .as_str()
                    .ok_or("The engine returned no preset.")?;
                library.add(text.as_bytes(), &name)
            })
            .await
            .unwrap_or_else(|_| Err("The image worker stopped unexpectedly.".into()));
            match added {
                Ok(preset) => {
                    ui.presets.unnamed.replace(Some(preset.path.clone()));
                    ui.presets.renaming.replace(Some(preset.path.clone()));
                    ui.reload_presets().await;
                }
                Err(error) => ui.preset_error("Couldn’t save the preset", &error),
            }
        });
    }

    /// Edits the preset's name in place; `unnamed` marks one New Preset made.
    fn start_rename(self: &Rc<Self>, path: &std::path::Path, unnamed: bool) {
        let panel = &self.presets;
        let Some(index) = panel.row_index(path) else {
            return;
        };
        // Another name being edited is kept, as when leaving its field.
        let previous = panel.renaming.borrow().clone();
        if let Some(previous) = previous
            && previous != path
        {
            let text = panel
                .row_index(&previous)
                .map(|i| panel.rows.borrow()[i].entry.text().to_string());
            self.finish_rename(&previous, text);
        }
        let rows = panel.rows.borrow();
        let row = &rows[index];
        panel.renaming.replace(Some(path.into()));
        if unnamed {
            panel.unnamed.replace(Some(path.into()));
        }
        panel.list.select_row(Some(&row.row));
        row.entry.set_text(&panel.files.borrow()[index].name());
        row.name.set_visible_child_name("entry");
        row.entry.grab_focus();
        row.entry.select_region(0, -1);
        let (viewport, target) = (panel.viewport.clone(), row.row.clone());
        glib::idle_add_local_once(move || viewport.scroll_to(&target, None));
    }

    /// Enter or leaving the field keeps `name` (an invalid one keeps the old
    /// name and says why); Escape (`None`) keeps the old name, or removes a
    /// preset New Preset just made.
    fn finish_rename(self: &Rc<Self>, path: &std::path::Path, name: Option<String>) {
        let panel = &self.presets;
        if panel.renaming.borrow().as_deref() != Some(path) {
            return;
        }
        panel.renaming.replace(None);
        let was_unnamed = panel.unnamed.borrow().as_deref() == Some(path);
        panel.unnamed.replace(None);
        let Some(index) = panel.row_index(path) else {
            return;
        };
        let preset = panel.files.borrow()[index].clone();
        {
            let rows = panel.rows.borrow();
            rows[index].name.set_visible_child_name("label");
            rows[index].row.grab_focus();
        }
        let library = panel.library.clone();
        let ui = self.clone();
        let Some(name) = name.map(|n| n.trim().to_owned()) else {
            if was_unnamed {
                glib::MainContext::default().spawn_local(async move {
                    let result = gio::spawn_blocking(move || library.delete(&preset))
                        .await
                        .unwrap_or_else(|_| Err("The preset couldn’t be removed.".into()));
                    if let Err(error) = result {
                        ui.preset_error("Couldn’t remove the preset", &error);
                    }
                    ui.reload_presets().await;
                });
            }
            return;
        };
        if name == preset.name() {
            return;
        }
        if let Some(problem) = presets::problem(&name) {
            if !name.is_empty() {
                self.preset_error(
                    &format!("“{name}” can’t be used as a preset name."),
                    &problem,
                );
            }
            return;
        }
        panel.rows.borrow()[index].label.set_text(&name);
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || library.rename(&preset, &name))
                .await
                .unwrap_or_else(|_| Err("The preset couldn’t be renamed.".into()));
            if let Err(error) = result {
                ui.preset_error("Couldn’t rename the preset", &error);
            }
            ui.reload_presets().await;
        });
    }

    fn delete_preset(self: &Rc<Self>, preset: &Preset) {
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Delete the preset “{}”?", preset.name()))
            .body("This can’t be undone. Photographs edited with it keep their settings.")
            .build();
        dialog.add_response("cancel", "Cancel");
        dialog.add_response("delete", "Delete");
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        let (library, preset, ui) = (self.presets.library.clone(), preset.clone(), self.clone());
        dialog.connect_response(None, move |_, response| {
            if response != "delete" {
                return;
            }
            let (library, preset, ui) = (library.clone(), preset.clone(), ui.clone());
            glib::MainContext::default().spawn_local(async move {
                let result = gio::spawn_blocking(move || library.delete(&preset))
                    .await
                    .unwrap_or_else(|_| Err("The preset couldn’t be deleted.".into()));
                if let Err(error) = result {
                    ui.preset_error("Couldn’t delete the preset", &error);
                }
                ui.reload_presets().await;
            });
        });
        dialog.present(Some(&self.window));
    }

    /// Moves the preset at `from` to before `to`, and remembers the order.
    fn move_preset(self: &Rc<Self>, from: usize, to: usize) {
        let mut order = self.presets.files.borrow().clone();
        if from >= order.len() {
            return;
        }
        let preset = order.remove(from);
        order.insert(if to > from { to - 1 } else { to }.min(order.len()), preset);
        if order == *self.presets.files.borrow() {
            return;
        }
        self.show_presets(order.clone());
        let (library, ui) = (self.presets.library.clone(), self.clone());
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || library.set_order(&order))
                .await
                .unwrap_or_else(|_| Err("The order couldn’t be saved.".into()));
            if let Err(error) = result {
                ui.preset_error("Couldn’t save the order of the presets", &error);
            }
            ui.reload_presets().await;
        });
    }

    pub fn import_presets_dialog(self: &Rc<Self>) {
        if !self.can_change_presets() {
            return;
        }
        let presets = gtk::FileFilter::new();
        presets.set_name(Some("Presets"));
        presets.add_suffix("json");
        presets.add_mime_type("application/json");
        presets.add_mime_type("text/plain");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&presets);
        let dialog = gtk::FileDialog::builder()
            .title("Import Presets")
            .accept_label("Import")
            .filters(&filters)
            .build();
        dialog.open_multiple(
            Some(&self.window),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[weak(rename_to=ui)]
                self,
                move |result| {
                    if let Ok(files) = result {
                        let paths = (0..files.n_items())
                            .filter_map(|i| files.item(i).and_downcast::<gio::File>()?.path())
                            .collect();
                        ui.import_presets(paths, None);
                    }
                }
            ),
        );
    }

    /// Copies each file the helper reads into the presets folder, under its
    /// own name, and lists the ones it refuses. They go to the end, or
    /// before the preset at `position` (a drop between rows).
    pub fn import_presets(self: &Rc<Self>, paths: Vec<PathBuf>, position: Option<usize>) {
        if paths.is_empty() || !self.can_change_presets() {
            return;
        }
        let Some(client) = self.runtime.borrow().client.clone() else {
            return;
        };
        self.show_pane("presets");
        let (library, ui) = (self.presets.library.clone(), self.clone());
        glib::MainContext::default().spawn_local(async move {
            let (added, failures) = gio::spawn_blocking(move || {
                let (added, failures) = library.import(&paths, position, |text| {
                    let command = json!({"command": "preset_import", "text": text});
                    client.request(command, Duration::from_secs(30)).map(|_| ())
                });
                (added.len(), failures)
            })
            .await
            .unwrap_or_else(|_| (0, vec!["The import stopped unexpectedly.".into()]));
            ui.reload_presets().await;
            if added > 0 {
                ui.status.set_text(&if added == 1 {
                    "Imported 1 preset".to_owned()
                } else {
                    format!("Imported {added} presets")
                });
            }
            if !failures.is_empty() {
                ui.preset_error(
                    if added == 0 {
                        "No presets were imported."
                    } else {
                        "Some files weren’t imported."
                    },
                    &failures.join("\n\n"),
                );
            }
        });
    }

    /// Writes a copy wherever the user picks, byte for byte.
    fn export_preset(self: &Rc<Self>, preset: &Preset) {
        let dialog = gtk::FileDialog::builder()
            .title("Export Preset")
            .accept_label("Export")
            .initial_name(format!("{}.json", preset.name()))
            .build();
        let preset = preset.clone();
        dialog.save(
            Some(&self.window),
            None::<&gio::Cancellable>,
            glib::clone!(
                #[weak(rename_to=ui)]
                self,
                move |result| {
                    let Some(destination) = result.ok().and_then(|f| f.path()) else {
                        return;
                    };
                    let library = ui.presets.library.clone();
                    glib::MainContext::default().spawn_local(async move {
                        let result = gio::spawn_blocking(move || {
                            let bytes = library.read(&preset)?;
                            let stage = destination.with_file_name(format!(
                                ".{}.{}.tmp",
                                destination
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy(),
                                crate::engine::unique()
                            ));
                            std::fs::write(&stage, bytes)
                                .and_then(|_| std::fs::rename(&stage, &destination))
                                .map_err(|error| {
                                    let _ = std::fs::remove_file(&stage);
                                    error.to_string()
                                })
                        })
                        .await
                        .unwrap_or_else(|_| Err("The export stopped unexpectedly.".into()));
                        match result {
                            Ok(()) => ui.status.set_text("Exported the preset"),
                            Err(error) => {
                                ui.preset_error("Couldn’t export the preset", &error);
                                ui.refresh_presets();
                            }
                        }
                    });
                }
            ),
        );
    }

    pub fn show_presets_folder(&self) {
        let folder = self.presets.library.folder.clone();
        let opened = std::fs::create_dir_all(&folder)
            .map_err(|e| e.to_string())
            .and_then(|_| {
                gio::AppInfo::launch_default_for_uri(
                    &gio::File::for_path(&folder).uri(),
                    Some(&gtk::prelude::WidgetExt::display(&self.window).app_launch_context()),
                )
                .map_err(|e| e.to_string())
            });
        if let Err(error) = opened {
            self.preset_error("Couldn’t show the presets folder", &error);
        }
    }

    fn preset_error(&self, heading: &str, body: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .build();
        dialog.add_response("close", "Close");
        dialog.present(Some(&self.window));
    }
}
