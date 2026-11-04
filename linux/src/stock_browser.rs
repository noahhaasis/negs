//! Film and paper foldouts: a two-column grid of the photograph developed
//! with each stock and the current settings. Collapsed by default. Also
//! schedules the Presets list's thumbnails (`preset_ui`). Thumbnails render
//! only while they're on screen and the main preview is up to date.
//! Settings can turn them off, which restores the plain menus and list.
use crate::{Editor, catalog::Stock, engine::Frame, parameters};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

/// The thumbnail's long edge: two columns of a ~340 px inspector at 2× scale.
pub const LONG_SIDE: u32 = 320;

/// One thumbnail: a stock's tile or a preset's.
pub struct Thumb {
    pub picture: gtk::Picture,
    /// The source generation and request the shown thumbnail (or the
    /// failure that left it blank) was made for.
    shown: RefCell<Option<(u64, String)>>,
}
impl Thumb {
    pub fn new() -> Self {
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        Self {
            picture,
            shown: RefCell::new(None),
        }
    }
    /// Updates the look for the wanted thumbnail; true when it needs a new one.
    fn mark(&self, generation: u64, key: &str) -> bool {
        match self.shown.borrow().as_ref() {
            Some((_, k)) if k == key => {
                self.picture.set_opacity(1.);
                return false;
            }
            // Same photograph, other settings: keep it, dimmed.
            Some((g, _)) if *g == generation => self.picture.set_opacity(0.55),
            // Another photograph: a placeholder, never the old picture.
            _ => {
                self.picture.set_paintable(None::<&gdk::Paintable>);
                self.picture.set_opacity(1.);
            }
        }
        true
    }
    /// A finished thumbnail, or None when it failed (blank until the next edit).
    fn show(&self, texture: Option<&gdk::Texture>, generation: u64, key: String) {
        self.picture.set_paintable(texture);
        self.picture.set_opacity(1.);
        self.shown.replace(Some((generation, key)));
    }
    /// Drops the thumbnail; true if there was one.
    pub fn clear(&self) -> bool {
        self.picture.set_paintable(None::<&gdk::Paintable>);
        self.picture.set_opacity(1.);
        self.shown.take().is_some()
    }
}

struct Tile {
    stock: &'static Stock,
    button: gtk::Button,
    thumb: Rc<Thumb>,
}

struct Grid {
    row: adw::ExpanderRow,
    tiles: Vec<Tile>,
}

#[derive(Default)]
pub struct Browser {
    grids: RefCell<Vec<Rc<Grid>>>,
    /// Each stock's menu and grid; the preference shows one of them.
    pairs: RefCell<Vec<(gtk::Widget, gtk::Widget)>>,
    busy: Cell<bool>,
}

pub fn enabled(ui: &Editor) -> bool {
    ui.preferences.borrow().previews
}

/// What a click on `stock` would develop: the current state with that stock
/// chosen (a film brings its own presets, and its paper unless the setting
/// keeps the paper), cropped as shown.
fn request(ui: &Editor, stock: &Stock) -> Option<(u64, serde_json::Value, String)> {
    let r = ui.runtime.borrow();
    let (source, _) = r.source.clone()?;
    let mut state = ui.history.borrow().state.clone();
    crate::catalog::choose(&mut state, stock, ui.preferences.borrow().keep_paper);
    let params = parameters::runtime(&state, &r.defaults);
    let film = crate::catalog::selected(&state, false).id.clone();
    let paper = crate::catalog::selected(&state, true).id.clone();
    let crop = (state.crop.is_some() || state.straighten != 0.).then(|| parameters::crop(&state));
    let command = json!({"command":"thumbnail","input":source,"source_generation":r.source_generation,
        "film":film,"paper":paper,"params":params,"long_side":LONG_SIDE,"crop":crop});
    let key = command.to_string();
    Some((r.source_generation, command, key))
}

/// The film or paper control: a menu and a preview grid, one of them shown.
pub fn rows(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool, menu: gtk::Widget) -> gtk::Widget {
    let grid = row(ui, list, paper);
    ui.stock_browser.pairs.borrow_mut().push((menu, grid.clone().upcast()));
    apply(ui);
    grid.upcast()
}

/// Shows the grids or the menus as the preference says. Turning previews off
/// frees their thumbnails here and in the engine.
pub fn apply(ui: &Rc<Editor>) {
    let on = enabled(ui);
    let scan = crate::inspector::flag(&ui.history.borrow().state, "Scan film directly", false);
    for (index, (menu, grid)) in ui.stock_browser.pairs.borrow().iter().enumerate() {
        // The second pair is the paper, hidden while scanning the film.
        let shown = index == 0 || !scan;
        menu.set_visible(shown && !on);
        grid.set_visible(shown && on);
    }
    ui.presets_show_thumbnails(on);
    if on {
        pump(ui);
        return;
    }
    let mut cached = ui.clear_preset_thumbnails();
    for grid in ui.stock_browser.grids.borrow().iter() {
        for tile in &grid.tiles {
            cached |= tile.thumb.clear();
        }
    }
    let client = ui.runtime.borrow().client.clone();
    if cached && let Some(client) = client {
        gio::spawn_blocking(move || {
            let _ = client.request(json!({"command":"release_thumbnails"}), Duration::from_secs(10));
        });
    }
}

fn row(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(if paper { "Paper" } else { "Film" })
        .build();
    let current = gtk::Label::new(None);
    current.add_css_class("dim-label");
    current.set_ellipsize(gtk::pango::EllipsizeMode::End);
    current.set_max_width_chars(20);
    row.add_suffix(&current);

    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.add_css_class("stock-grid");
    let mut tiles = Vec::new();
    let mut group = String::new();
    let mut flow: Option<gtk::FlowBox> = None;
    for stock in crate::catalog::stocks(paper) {
        if stock.group != group || flow.is_none() {
            group = stock.group.clone();
            let heading = gtk::Label::new(Some(&group));
            heading.set_xalign(0.);
            heading.add_css_class("caption-heading");
            heading.add_css_class("dim-label");
            body.append(&heading);
            let f = gtk::FlowBox::new();
            f.set_selection_mode(gtk::SelectionMode::None);
            f.set_homogeneous(true);
            f.set_min_children_per_line(2);
            f.set_max_children_per_line(2);
            f.set_column_spacing(6);
            f.set_row_spacing(6);
            body.append(&f);
            flow = Some(f);
        }
        let thumb = Rc::new(Thumb::new());
        let picture = &thumb.picture;
        picture.set_size_request(-1, 92);
        picture.add_css_class("stock-thumb");
        let name = gtk::Label::new(Some(&stock.name));
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_xalign(0.);
        name.add_css_class("caption");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 3);
        content.append(picture);
        content.append(&name);
        let button = gtk::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("flat");
        button.add_css_class("stock-tile");
        button.set_tooltip_text(Some(&stock.name));
        button.connect_clicked(glib::clone!(
            #[weak]
            ui,
            move |_| {
                if ui.syncing.get() {
                    return;
                }
                let mut h = ui.history.borrow_mut();
                h.begin(if paper { "Change Paper" } else { "Change Film" });
                crate::catalog::choose(&mut h.state, stock, ui.preferences.borrow().keep_paper);
                h.commit();
                drop(h);
                ui.sync_state();
            }
        ));
        flow.as_ref().unwrap().append(&button);
        tiles.push(Tile {
            stock,
            button,
            thumb,
        });
    }
    let holder = gtk::ListBoxRow::new();
    holder.set_activatable(false);
    holder.set_selectable(false);
    holder.set_child(Some(&body));
    row.add_row(&holder);

    let grid = Rc::new(Grid {
        row: row.clone(),
        tiles,
    });
    ui.stock_browser.grids.borrow_mut().push(grid.clone());
    row.connect_expanded_notify(glib::clone!(
        #[weak]
        ui,
        move |_| pump(&ui)
    ));
    // Another inspector tab or the Presets page hides it.
    row.connect_map(glib::clone!(
        #[weak]
        ui,
        move |_| pump_soon(&ui)
    ));
    ui.rules.borrow_mut().push(Box::new(glib::clone!(
        #[weak]
        current,
        #[weak]
        ui,
        move |s| {
            let selected = crate::catalog::selected(s, paper);
            current.set_text(&selected.name);
            for tile in &grid.tiles {
                if tile.stock.id == selected.id {
                    tile.button.add_css_class("selected-stock");
                } else {
                    tile.button.remove_css_class("selected-stock");
                }
            }
            // After the state settles; the main preview goes first.
            glib::idle_add_local_once(move || apply(&ui));
        }
    )));
    list.append(&row);
    row
}

/// `pump` once the current signal handler has returned.
pub fn pump_soon(ui: &Rc<Editor>) {
    let ui = ui.clone();
    glib::idle_add_local_once(move || pump(&ui));
}

/// A thumbnail to render: the tile, the photograph's generation, the helper
/// request, what identifies the result, and a name for logs.
pub struct Wanted {
    pub thumb: Rc<Thumb>,
    pub generation: u64,
    pub command: serde_json::Value,
    pub key: String,
    pub name: String,
}

/// Marks out-of-date thumbnails (dimmed, or blank for another photograph)
/// and renders the next one of an open grid or the Presets list: one helper
/// request at a time, and only while the main preview has nothing to do.
pub fn pump(ui: &Rc<Editor>) {
    let browser = &ui.stock_browser;
    if crate::runtime::ui_only() || !enabled(ui) {
        return;
    }
    let mut wanted = Vec::new();
    for grid in browser.grids.borrow().iter() {
        if !grid.row.is_expanded() || !grid.row.is_mapped() {
            continue;
        }
        for tile in &grid.tiles {
            let Some((generation, command, key)) = request(ui, tile.stock) else {
                return;
            };
            wanted.push(Wanted {
                thumb: tile.thumb.clone(),
                generation,
                command,
                key,
                name: tile.stock.id.clone(),
            });
        }
    }
    wanted.extend(ui.wanted_preset_thumbnails());
    let mut next = None;
    for item in wanted {
        if item.thumb.mark(item.generation, &item.key) && next.is_none() {
            next = Some(item);
        }
    }
    if browser.busy.get() {
        return;
    }
    let client = {
        let r = ui.runtime.borrow();
        if !r.ready
            || r.closing
            || r.rendering
            || r.interacting
            || r.block_helper_render
            || r.requested_revision != r.revision
        {
            return;
        }
        let Some(client) = r.client.clone() else {
            return;
        };
        client
    };
    let Some(Wanted {
        thumb,
        generation,
        mut command,
        key,
        name,
    }) = next
    else {
        return;
    };
    browser.busy.set(true);
    let buffer = client
        .scratch
        .directory
        .join(format!("stock-{}.rgba", crate::engine::unique()));
    command["preview_path"] = json!(buffer);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let result = gio::spawn_blocking(move || {
            let frame = client
                .request(command, Duration::from_secs(60))
                .and_then(|(_, result)| Frame::read(&buffer, result));
            let _ = std::fs::remove_file(&buffer);
            frame
        })
        .await;
        ui.stock_browser.busy.set(false);
        if ui.runtime.borrow().closing || !enabled(&ui) {
            return;
        }
        // The photograph changed meanwhile: drop it, the next pump retries.
        let current = ui.runtime.borrow().source_generation == generation;
        match result {
            Ok(Ok(frame)) if current => {
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
                thumb.show(Some(&texture), generation, key);
            }
            Ok(Ok(_)) => {}
            // Leave the placeholder; a later edit retries. Don't spin on a failure.
            Ok(Err(error)) => {
                eprintln!("thumbnail {name}: {error}");
                thumb.show(None, generation, key);
            }
            Err(_) => thumb.show(None, generation, key),
        }
        pump(&ui);
    });
}
