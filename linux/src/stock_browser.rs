//! Film and paper foldouts: a two-column grid of the photograph developed
//! with each stock and the current settings. Collapsed by default; thumbnails
//! render only while a grid is open and the main preview is up to date.
use crate::{Editor, catalog::Stock, engine::Frame, parameters};
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};

/// The thumbnail's long edge: two columns of a ~340 px inspector at 2× scale.
fn long_side() -> u32 {
    std::env::var("NEGS_STOCK_THUMB_PX")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|v| (16..=1024).contains(v))
        .unwrap_or(320)
}

struct Tile {
    stock: &'static Stock,
    button: gtk::Button,
    picture: gtk::Picture,
    /// The request the shown thumbnail was rendered for.
    shown: RefCell<Option<String>>,
}

struct Grid {
    paper: bool,
    row: adw::ExpanderRow,
    tiles: Vec<Tile>,
}

#[derive(Default)]
pub struct Browser {
    grids: RefCell<Vec<Rc<Grid>>>,
    busy: Cell<bool>,
    /// Since the first stale thumbnail of the current batch was requested.
    batch: Cell<Option<(Instant, u32)>>,
}

/// What a click on `stock` would develop: the current state with that stock
/// chosen (a film brings its own paper and presets).
fn request(ui: &Editor, stock: &Stock) -> Option<(serde_json::Value, String)> {
    let r = ui.runtime.borrow();
    let (source, _) = r.source.clone()?;
    let mut state = ui.history.borrow().state.clone();
    crate::catalog::choose(&mut state, stock);
    let params = parameters::runtime(&state, &r.defaults);
    let film = crate::catalog::selected(&state, false).id.clone();
    let paper = crate::catalog::selected(&state, true).id.clone();
    let command = json!({"command":"thumbnail","input":source,"source_generation":r.source_generation,
        "film":film,"paper":paper,"params":params,"long_side":long_side()});
    let key = command.to_string();
    Some((command, key))
}

pub fn row(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(if paper { "Paper" } else { "Film" })
        .build();
    // Measurement hook: NEGS_STOCK_EXPAND=film,paper opens the grids.
    row.set_expanded(std::env::var("NEGS_STOCK_EXPAND").is_ok_and(|v| {
        v.split(',').any(|k| k == if paper { "paper" } else { "film" })
    }));
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
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture.set_size_request(-1, 92);
        picture.add_css_class("stock-thumb");
        let name = gtk::Label::new(Some(&stock.name));
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_xalign(0.);
        name.add_css_class("caption");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 3);
        content.append(&picture);
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
                crate::catalog::choose(&mut h.state, stock);
                h.commit();
                drop(h);
                ui.sync_state();
            }
        ));
        flow.as_ref().unwrap().append(&button);
        tiles.push(Tile {
            stock,
            button,
            picture,
            shown: RefCell::new(None),
        });
    }
    let holder = gtk::ListBoxRow::new();
    holder.set_activatable(false);
    holder.set_selectable(false);
    holder.set_child(Some(&body));
    row.add_row(&holder);

    let grid = Rc::new(Grid { paper, row: row.clone(), tiles });
    ui.stock_browser.grids.borrow_mut().push(grid.clone());
    row.connect_expanded_notify(glib::clone!(
        #[weak]
        ui,
        move |_| pump(&ui)
    ));
    ui.rules.borrow_mut().push(Box::new(glib::clone!(
        #[weak]
        current,
        #[weak]
        ui,
        move |s| {
            let selected = crate::catalog::selected(s, grid.paper);
            current.set_text(&selected.name);
            for tile in &grid.tiles {
                if tile.stock.id == selected.id {
                    tile.button.add_css_class("selected-stock");
                } else {
                    tile.button.remove_css_class("selected-stock");
                }
            }
            // After the state settles; the main preview goes first.
            glib::idle_add_local_once(move || pump(&ui));
        }
    )));
    list.append(&row);
    row
}

/// Renders the next stale thumbnail of an open grid, one helper request at a
/// time, and only while the main preview has nothing to do.
pub fn pump(ui: &Rc<Editor>) {
    let browser = &ui.stock_browser;
    if browser.busy.get() || crate::runtime::ui_only() {
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
    let mut next = None;
    for grid in browser.grids.borrow().iter() {
        if !grid.row.is_expanded() || !grid.row.is_visible() {
            continue;
        }
        for (index, tile) in grid.tiles.iter().enumerate() {
            let Some((command, key)) = request(ui, tile.stock) else {
                return;
            };
            if tile.shown.borrow().as_deref() != Some(key.as_str()) {
                tile.picture.set_opacity(0.55);
                if next.is_none() {
                    next = Some((grid.clone(), index, command, key));
                }
            }
        }
    }
    let Some((grid, index, mut command, key)) = next else {
        if let Some((started, count)) = browser.batch.take() {
            let ms = started.elapsed().as_secs_f64() * 1000.;
            ui.perf.detail("stock_thumbnails_last_batch", json!({"count":count,"ms":ms}));
            eprintln!("stock thumbnails: {count} in {ms:.0} ms ({:.0} ms each)", ms / count.max(1) as f64);
        }
        return;
    };
    let (started, count) = browser.batch.get().unwrap_or((Instant::now(), 0));
    browser.batch.set(Some((started, count + 1)));
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
        if ui.runtime.borrow().closing {
            return;
        }
        let tile = &grid.tiles[index];
        match result {
            Ok(Ok(frame)) => {
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
                tile.picture.set_paintable(Some(&texture));
                tile.picture.set_opacity(1.);
                tile.shown.replace(Some(key));
                if std::env::var_os("NEGS_STOCK_THUMB_LOG").is_some() {
                    eprintln!(
                        "thumb {} {}: {:.0} ms {}",
                        tile.stock.id, grid.paper, frame.result["elapsed_ms"].as_f64().unwrap_or(0.),
                        frame.result["timing_ms"]
                    );
                }
            }
            // Leave it stale; a later edit retries. Don't spin on a failure.
            Ok(Err(error)) => {
                eprintln!("stock thumbnail {}: {error}", tile.stock.id);
                tile.shown.replace(Some(key));
            }
            Err(_) => {
                tile.shown.replace(Some(key));
            }
        }
        pump(&ui);
    });
}
