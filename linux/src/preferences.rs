use crate::Editor;
use adw::prelude::*;
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, rc::Rc};

/// Starred for new users, as on the web and the Mac; once they change the
/// list, theirs is kept, even when empty.
pub const DEFAULT_FAVORITE_FILMS: [&str; 4] = [
    "kodak_gold_200",
    "kodak_portra_400",
    "kodak_ektar_100",
    "fujifilm_c200",
];

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub tab: String,
    pub expanded: Vec<String>,
    pub inspector: bool,
    /// The sidebar's page: "adjustments" or "presets".
    pub pane: String,
    pub strip: bool,
    pub appearance: u32,
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    pub sidebar_width: f64,
    pub export_engine: u32,
    pub output_format: u32,
    pub export_edge: u32,
    pub export_pixels: u32,
    pub metadata: bool,
    pub jpeg_quality: u32,
    /// The colour space chosen for saves, as colour names it.
    pub color_space: String,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
    pub last_save_folder: Option<PathBuf>,
    /// Film, paper and preset thumbnails rather than plain menus and lists.
    pub previews: bool,
    /// Changing the film leaves the paper as it is, rather than switching to
    /// the film's own paper.
    pub keep_paper: bool,
    pub tab_chosen: bool,
    pub fresh_edits: bool,
    pub match_new_photos: bool,
    pub limit_preview: Option<bool>,
    pub low_memory_export: Option<bool>,
    pub lab_all_controls: bool,
    pub scope_mode: String,
    pub scope_height: f64,
    pub tone_graph_channel: String,
    pub favorite_films: Vec<String>,
    pub favorite_papers: Vec<String>,
    pub viewer_background_custom: bool,
    pub viewer_background_color: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            tab: "prep".into(),
            expanded: vec!["Enlarger".into()],
            inspector: true,
            pane: "adjustments".into(),
            strip: true,
            appearance: 0,
            width: 1320,
            height: 900,
            maximized: false,
            sidebar_width: 340.,
            export_engine: 0,
            output_format: 0,
            export_edge: 0,
            export_pixels: 2048,
            metadata: true,
            jpeg_quality: 95,
            color_space: "sRGB".into(),
            png_bit_depth: 8,
            tiff_bit_depth: 16,
            last_save_folder: None,
            previews: true,
            keep_paper: false,
            tab_chosen: false,
            fresh_edits: true,
            match_new_photos: true,
            limit_preview: None,
            low_memory_export: None,
            lab_all_controls: false,
            scope_mode: "Histogram".into(),
            scope_height: 88.,
            tone_graph_channel: "RGB".into(),
            favorite_films: DEFAULT_FAVORITE_FILMS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            favorite_papers: Vec::new(),
            viewer_background_custom: false,
            viewer_background_color: "#242424".into(),
        }
    }
}
/// The colour space the preview develops in: the one Save and Export last
/// used, so editing shows the export's gamut. Shared with the render
/// threads, which build the helper's params and cache keys.
static PREVIEW_SPACE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn preview_space() -> String {
    let space = PREVIEW_SPACE.lock().unwrap();
    if COLOR_SPACES.contains(&space.as_str()) {
        space.clone()
    } else {
        COLOR_SPACES[0].into()
    }
}

/// Changes the preview space; true if it changed.
pub fn set_preview_space(space: &str) -> bool {
    let mut current = PREVIEW_SPACE.lock().unwrap();
    let changed = *current != space;
    *current = space.into();
    changed
}

/// The colour spaces a save offers, as colour names them.
pub const COLOR_SPACES: [&str; 4] = ["sRGB", "Display P3", "Adobe RGB (1998)", "ProPhoto RGB"];
/// ProPhoto RGB bands at 8 bits, so it needs 16.
pub const PROPHOTO: &str = "ProPhoto RGB";
const PROPHOTO_FALLBACK: &str = "Adobe RGB (1998)";

impl Preferences {
    /// A snapshot carries the current app options so cache keys and worker
    /// requests agree. Undo can restore photo edits, never older preferences.
    pub fn apply_to_state(&self, state: &mut crate::model::State) {
        state.choices.insert(
            "Limit preview to 12 MP".into(),
            self.limit_preview.unwrap_or(false).to_string(),
        );
        state.choices.insert(
            "Use less memory for export".into(),
            self.low_memory_export.unwrap_or(false).to_string(),
        );
    }

    /// Bits per channel for `format` (PNG 0, JPEG 1, TIFF 2).
    pub fn bit_depth(&self, format: u32) -> u32 {
        match format {
            1 => 8,
            2 => self.tiff_bit_depth,
            _ => self.png_bit_depth,
        }
    }
    pub fn set_bit_depth(&mut self, format: u32, depth: u32) {
        match format {
            1 => {}
            2 => self.tiff_bit_depth = depth,
            _ => self.png_bit_depth = depth,
        }
    }
    /// The colour space a save in `format` uses: the chosen one, except
    /// ProPhoto RGB at 8 bits, which falls back to Adobe RGB. Switching back
    /// to 16 bits restores ProPhoto.
    pub fn color_space(&self, format: u32) -> &str {
        let chosen = COLOR_SPACES
            .into_iter()
            .find(|cs| *cs == self.color_space)
            .unwrap_or(COLOR_SPACES[0]);
        if chosen == PROPHOTO && self.bit_depth(format) == 8 {
            PROPHOTO_FALLBACK
        } else {
            chosen
        }
    }
}

impl Editor {
    pub fn refresh_recent(&self) {
        self.recent_menu.remove_all();
        while let Some(child) = self.empty_recent.first_child() {
            self.empty_recent.remove(&child);
        }
        for path in self.recent.borrow().iter().take(5) {
            let button =
                gtk::Button::with_label(&path.file_name().unwrap_or_default().to_string_lossy());
            button.add_css_class("flat");
            button.set_tooltip_text(Some(&path.to_string_lossy()));
            button.set_action_name(Some("win.open-recent"));
            button.set_action_target_value(Some(&path.to_string_lossy().to_string().to_variant()));
            self.empty_recent.append(&button);
        }
        for path in self.recent.borrow().iter() {
            let item = gio::MenuItem::new(
                Some(&path.file_name().unwrap_or_default().to_string_lossy()),
                None,
            );
            item.set_action_and_target_value(
                Some("win.open-recent"),
                Some(&path.to_string_lossy().to_string().to_variant()),
            );
            self.recent_menu.append_item(&item);
        }
        let clear = gio::Menu::new();
        clear.append(Some("Clear Recent Photographs"), Some("win.clear-recent"));
        self.recent_menu.append_section(None, &clear);
    }
    pub fn remember(&self, path: PathBuf) {
        let mut recent = self.recent.borrow_mut();
        recent.retain(|p| p != &path);
        recent.insert(0, path);
        recent.truncate(10);
        drop(recent);
        self.refresh_recent();
    }
    pub fn settings(self: &Rc<Self>) {
        let d = adw::Dialog::builder()
            .title("Settings")
            .content_width(440)
            .build();
        let toolbar = adw::ToolbarView::new();
        toolbar.add_top_bar(&adw::HeaderBar::new());
        let b = gtk::Box::new(gtk::Orientation::Vertical, 16);
        b.set_margin_top(20);
        b.set_margin_bottom(24);
        b.set_margin_start(24);
        b.set_margin_end(24);
        let group = adw::PreferencesGroup::new();
        let appearance = adw::ComboRow::builder()
            .title("Appearance")
            .model(&gtk::StringList::new(&["Follow system", "Light", "Dark"]))
            .selected(self.preferences.borrow().appearance)
            .build();
        appearance.set_tooltip_text(Some(
            "Use the system appearance, or choose light or dark for Negs.",
        ));
        appearance.connect_selected_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |r| {
                ui.preferences.borrow_mut().appearance = r.selected();
                apply_appearance(r.selected());
            }
        ));
        group.add(&appearance);
        let previews = adw::SwitchRow::builder()
            .title("Film, paper and preset previews")
            .subtitle("Show each film, paper and preset on your photograph. Turn off to use simple menus and lists on slower computers.")
            .active(self.preferences.borrow().previews)
            .build();
        previews.set_tooltip_text(Some("Show the current photograph with each film, paper and preset. Off, use simple menus and lists."));
        previews.connect_active_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |r| {
                ui.preferences.borrow_mut().previews = r.is_active();
                crate::stock_browser::apply(&ui);
            }
        ));
        group.add(&previews);
        let keep_paper = adw::SwitchRow::builder()
            .title("Keep paper when changing film")
            .subtitle("Leave the paper as it is when you choose another film, instead of switching to the paper made for that film.")
            .active(self.preferences.borrow().keep_paper)
            .build();
        keep_paper.set_tooltip_text(Some("Keep the selected paper when choosing a film. Off, choose the paper made for the film."));
        keep_paper.connect_active_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |r| {
                ui.preferences.borrow_mut().keep_paper = r.is_active();
                // Film thumbnails show what a click would develop.
                crate::stock_browser::pump(&ui);
            }
        ));
        group.add(&keep_paper);
        let fresh = adw::ComboRow::builder()
            .title("New photographs")
            .subtitle("Film, paper, Image Size and output colour stay either way")
            .model(&gtk::StringList::new(&[
                "Start fresh",
                "Keep the previous edits",
            ]))
            .selected(if self.preferences.borrow().fresh_edits {
                0
            } else {
                1
            })
            .build();
        fresh.set_tooltip_text(Some("Start fresh resets Prep and Lab settings and removes the border. Keeping edits carries them to the next photograph, apart from crop, rotation, lens corrections and input interpretation."));
        fresh.connect_selected_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |row| {
                ui.preferences.borrow_mut().fresh_edits = row.selected() == 0;
            }
        ));
        group.add(&fresh);
        preference_switch(
            self,
            &group,
            "Match new photographs to camera brightness",
            "New RAWs match the camera preview; JPEG, PNG and TIFF match the original file.",
            self.preferences.borrow().match_new_photos,
            |p, value| p.match_new_photos = value,
        );
        preference_switch(
            self,
            &group,
            "Limit preview to 12 MP",
            "Develops large previews faster. Export keeps the full Image Size.",
            self.preferences.borrow().limit_preview.unwrap_or(false),
            |p, value| p.limit_preview = Some(value),
        );
        preference_switch(
            self,
            &group,
            "Use less memory for export",
            "Develops grain one colour at a time. Same result, less memory, longer export.",
            self.preferences.borrow().low_memory_export.unwrap_or(false),
            |p, value| p.low_memory_export = Some(value),
        );
        let custom = adw::SwitchRow::builder()
            .title("Custom viewer background")
            .subtitle("Off, use the standard viewer background")
            .active(self.preferences.borrow().viewer_background_custom)
            .build();
        custom.set_tooltip_text(Some("Choose the colour behind the photograph."));
        let choose = gtk::Button::with_label("Choose Colour…");
        choose.set_valign(gtk::Align::Center);
        choose.set_sensitive(custom.is_active());
        choose.set_tooltip_text(Some("Choose the colour behind the photograph."));
        custom.add_suffix(&choose);
        custom.connect_active_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            #[weak]
            choose,
            move |row| {
                ui.preferences.borrow_mut().viewer_background_custom = row.is_active();
                choose.set_sensitive(row.is_active());
                ui.apply_viewer_background();
            }
        ));
        choose.connect_clicked(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |_| {
                let ui = ui.clone();
                glib::MainContext::default().spawn_local(async move {
                    let dialog = gtk::ColorDialog::builder()
                        .title("Viewer background colour")
                        .with_alpha(false)
                        .build();
                    let initial =
                        gtk::gdk::RGBA::parse(&ui.preferences.borrow().viewer_background_color)
                            .ok();
                    if let Ok(color) = dialog
                        .choose_rgba_future(Some(&ui.window), initial.as_ref())
                        .await
                    {
                        ui.preferences.borrow_mut().viewer_background_color = format!(
                            "#{:02X}{:02X}{:02X}",
                            (color.red() * 255.).round() as u8,
                            (color.green() * 255.).round() as u8,
                            (color.blue() * 255.).round() as u8
                        );
                        ui.apply_viewer_background();
                    }
                });
            }
        ));
        group.add(&custom);
        let recent = adw::ActionRow::builder()
            .title("Recent photographs")
            .subtitle("Paths are stored locally in your session")
            .build();
        let clear = gtk::Button::with_label("Clear");
        clear.set_valign(gtk::Align::Center);
        clear.set_tooltip_text(Some(
            "Clear the list of recent photographs. The files stay on disk.",
        ));
        clear.set_action_name(Some("win.clear-recent"));
        recent.add_suffix(&clear);
        group.add(&recent);
        b.append(&group);
        let updates = adw::PreferencesGroup::builder()
            .title("Updates")
            .description(
                "Updates arrive through your distribution package manager or a local installation.",
            )
            .build();
        b.append(&updates);
        let location = gtk::Label::new(Some(&format!(
            "Session: {}",
            Self::session_path().display()
        )));
        location.set_wrap(true);
        location.set_selectable(true);
        location.set_xalign(0.);
        location.add_css_class("caption");
        b.append(&location);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .child(&b)
            .build();
        toolbar.set_content(Some(&scroll));
        d.set_child(Some(&toolbar));
        d.present(Some(&self.window));
    }
    pub fn restore_preferences(&self, session: &serde_json::Value) {
        if let Some(value) = session.get("preferences")
            && let Ok(mut p) = serde_json::from_value::<Preferences>(value.clone())
        {
            p.tab = match p.tab.as_str() {
                "post" | "output" => "output",
                "lab" if p.tab_chosen => "lab",
                _ => "prep",
            }
            .into();
            if p.limit_preview.is_none() {
                p.limit_preview = Some(
                    session["state"]["choices"]["Limit preview to 12 MP"].as_str() == Some("true"),
                );
            }
            if p.low_memory_export.is_none() {
                p.low_memory_export = Some(
                    session["state"]["choices"]["Use less memory for export"].as_str()
                        == Some("true")
                        || session["state"]["params"]["settings"]["serial_grain"] == true,
                );
            }
            p.scope_height = if p.scope_height.is_finite() {
                p.scope_height.clamp(88., 420.)
            } else {
                88.
            };
            p.width = p.width.clamp(360, 7680);
            p.height = p.height.clamp(480, 4320);
            p.appearance = p.appearance.min(2);
            if !crate::runtime::ui_only() {
                p.inspector = true;
                p.strip = true;
            }
            self.window.set_default_size(p.width, p.height);
            self.resize_inspector(p.sidebar_width);
            self.window.set_maximized(p.maximized);
            self.split.set_show_sidebar(p.inspector);
            if p.pane != "presets" {
                p.pane = "adjustments".into();
            }
            self.sidebar_pages.set_visible_child_name(&p.pane);
            self.strip_scroll.set_visible(p.strip);
            if std::env::var_os("NEGS_APPEARANCE").is_none() {
                apply_appearance(p.appearance);
            }
            set_preview_space(&p.color_space);
            self.preferences.replace(p);
            self.apply_viewer_background();
            self.update_window_minimum();
        }
        if let Some(values) = session.get("recent")
            && let Ok(mut recent) = serde_json::from_value::<Vec<PathBuf>>(values.clone())
        {
            recent.truncate(10);
            self.recent.replace(recent);
        }
        self.refresh_recent();
    }
    pub fn apply_viewer_background(&self) {
        let p = self.preferences.borrow();
        self.canvas
            .set_viewer_background(p.viewer_background_custom, &p.viewer_background_color);
    }
}

fn preference_switch(
    ui: &Rc<Editor>,
    group: &adw::PreferencesGroup,
    title: &str,
    subtitle: &str,
    active: bool,
    change: impl Fn(&mut Preferences, bool) + 'static,
) {
    let row = adw::SwitchRow::builder()
        .title(title)
        .subtitle(subtitle)
        .active(active)
        .build();
    row.set_tooltip_text(Some(subtitle));
    row.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| {
            change(&mut ui.preferences.borrow_mut(), row.is_active());
            ui.sync_state();
        }
    ));
    group.add(&row);
}
pub fn apply_appearance(index: u32) {
    adw::StyleManager::default().set_color_scheme(match index {
        1 => adw::ColorScheme::ForceLight,
        2 => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
}

#[cfg(test)]
mod port_tests {
    use super::*;
    #[test]
    fn default_favourites_preserve_missing_custom_and_explicitly_empty_lists() {
        let defaults = Preferences::default();
        assert_eq!(defaults.favorite_films, DEFAULT_FAVORITE_FILMS);
        assert!(defaults.favorite_papers.is_empty());
        let mut saved = serde_json::to_value(defaults).unwrap();
        saved.as_object_mut().unwrap().remove("favorite_films");
        assert_eq!(
            serde_json::from_value::<Preferences>(saved.clone())
                .unwrap()
                .favorite_films,
            DEFAULT_FAVORITE_FILMS
        );
        for list in [vec![], vec!["ilford_hp5_plus"]] {
            saved["favorite_films"] = serde_json::json!(list);
            assert_eq!(
                serde_json::from_value::<Preferences>(saved.clone())
                    .unwrap()
                    .favorite_films,
                list
            );
        }
    }
    #[test]
    fn current_preferences_survive_undo_and_reset_and_key_the_cached_preview() {
        let mut preferences = Preferences::default();
        let mut history = crate::model::History::new(crate::fixtures::state());
        preferences.apply_to_state(&mut history.state);
        history.begin("Change Contrast");
        history.state.values.insert("Prep.Contrast".into(), 30.);
        history.commit();
        preferences.limit_preview = Some(true);
        preferences.low_memory_export = Some(true);
        preferences.apply_to_state(&mut history.state);
        history.undo();
        preferences.apply_to_state(&mut history.state);
        assert_eq!(history.state.values["Prep.Contrast"], 0.);
        assert_eq!(
            crate::preview_cache::identity(&history.state, &serde_json::Value::Null)["limit"],
            true
        );
        history.state = crate::fixtures::state();
        preferences.apply_to_state(&mut history.state);
        assert_eq!(
            crate::preview_cache::identity(&history.state, &serde_json::Value::Null)["limit"],
            true
        );
        let params = serde_json::json!({"settings":{"serial_grain":true}});
        assert_eq!(
            crate::parameters::runtime(&history.state, &params)["settings"]["serial_grain"],
            false
        );
    }
}
