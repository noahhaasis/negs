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
    /// The colour space chosen for saves, as colour names it.
    pub color_space: String,
    pub png_bit_depth: u32,
    pub tiff_bit_depth: u32,
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
            color_space: "sRGB".into(),
            png_bit_depth: 8,
            tiff_bit_depth: 16,
            last_save_folder: None,
        }
    }
}
/// The colour space the preview develops in: the one Save and Export last
/// used, so editing shows the export's gamut. Shared with the render
/// threads, which build the helper's params and cache keys.
static PREVIEW_SPACE: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub fn preview_space() -> String {
    let space = PREVIEW_SPACE.lock().unwrap();
    if COLOR_SPACES.contains(&space.as_str()) { space.clone() } else { COLOR_SPACES[0].into() }
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
            set_preview_space(&p.color_space);
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
