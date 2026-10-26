use crate::Editor;
use adw::prelude::*;
use gtk::{gio, glib};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, rc::Rc};

#[derive(Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub tab: String,
    pub expanded: Vec<String>,
    pub inspector: bool,
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
    pub last_save_folder: Option<PathBuf>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            tab: "lab".into(),
            expanded: vec!["Enlarger".into()],
            inspector: true,
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
            last_save_folder: None,
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
        appearance.connect_selected_notify(glib::clone!(
            #[weak(rename_to=ui)]
            self,
            move |r| {
                ui.preferences.borrow_mut().appearance = r.selected();
                apply_appearance(r.selected());
            }
        ));
        group.add(&appearance);
        let recent = adw::ActionRow::builder()
            .title("Recent photographs")
            .subtitle("Paths are stored locally in your session")
            .build();
        let clear = gtk::Button::with_label("Clear");
        clear.set_valign(gtk::Align::Center);
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
        toolbar.set_content(Some(&b));
        d.set_child(Some(&toolbar));
        d.present(Some(&self.window));
    }
    pub fn restore_preferences(&self, session: &serde_json::Value) {
        if let Some(value) = session.get("preferences")
            && let Ok(mut p) = serde_json::from_value::<Preferences>(value.clone())
        {
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
            self.strip_scroll.set_visible(p.strip);
            if std::env::var_os("NEGS_APPEARANCE").is_none() {
                apply_appearance(p.appearance);
            }
            self.preferences.replace(p);
        }
        if let Some(values) = session.get("recent")
            && let Ok(mut recent) = serde_json::from_value::<Vec<PathBuf>>(values.clone())
        {
            recent.truncate(10);
            self.recent.replace(recent);
        }
        self.refresh_recent();
    }
}
pub fn apply_appearance(index: u32) {
    adw::StyleManager::default().set_color_scheme(match index {
        1 => adw::ColorScheme::ForceLight,
        2 => adw::ColorScheme::ForceDark,
        _ => adw::ColorScheme::Default,
    });
}
