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
    widget: gtk::Overlay,
    favorite: gtk::Button,
    thumb: Rc<Thumb>,
}

struct Grid {
    row: adw::ExpanderRow,
    paper: bool,
    body: gtk::Box,
    viewport: gtk::Viewport,
    scroll: gtk::ScrolledWindow,
    search: Option<gtk::SearchEntry>,
    order: RefCell<Vec<(String, bool)>>,
    tiles: Vec<Tile>,
}
struct Menu {
    paper: bool,
    row: adw::ComboRow,
    model: gtk::StringList,
    ids: RefCell<Vec<String>>,
    favorite: gtk::Button,
    updating: Cell<bool>,
}

#[derive(Default)]
pub struct Browser {
    grids: RefCell<Vec<Rc<Grid>>>,
    menus: RefCell<Vec<Rc<Menu>>>,
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
    ui.stock_browser
        .pairs
        .borrow_mut()
        .push((menu, grid.clone().upcast()));
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
            let _ = client.request(
                json!({"command":"release_thumbnails"}),
                Duration::from_secs(10),
            );
        });
    }
}

/// Catalog ordering within each category is stable; favourites lead all
/// categories, in the same order the catalog uses.
pub fn ordered_stocks(ui: &Editor, paper: bool) -> Vec<&'static Stock> {
    let mut stocks = crate::catalog::stocks(paper);
    let preferences = ui.preferences.borrow();
    let favorites = if paper {
        &preferences.favorite_papers
    } else {
        &preferences.favorite_films
    };
    stocks.sort_by_key(|stock| !favorites.contains(&stock.id));
    stocks
}

pub fn is_favorite(ui: &Editor, paper: bool, id: &str) -> bool {
    let preferences = ui.preferences.borrow();
    (if paper {
        &preferences.favorite_papers
    } else {
        &preferences.favorite_films
    })
    .iter()
    .any(|s| s == id)
}

pub fn toggle_favorite(ui: &Rc<Editor>, paper: bool, id: &str) {
    {
        let mut preferences = ui.preferences.borrow_mut();
        let favorites = if paper {
            &mut preferences.favorite_papers
        } else {
            &mut preferences.favorite_films
        };
        if favorites.iter().any(|s| s == id) {
            favorites.retain(|s| s != id);
        } else {
            favorites.push(id.to_string());
        }
    }
    refresh_favorites(ui);
    pump_soon(ui);
}

fn favorite_button(ui: &Rc<Editor>, stock: &'static Stock) -> gtk::Button {
    let button = gtk::Button::from_icon_name("non-starred-symbolic");
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Start);
    button.set_halign(gtk::Align::End);
    favorite_label(&button, stock, is_favorite(ui, stock.paper, &stock.id));
    button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| toggle_favorite(&ui, stock.paper, &stock.id)
    ));
    button
}

fn favorite_label(button: &gtk::Button, stock: &Stock, on: bool) {
    button.set_icon_name(if on {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    });
    let action = format!(
        "{} {} {} favourites",
        if on { "Remove" } else { "Add" },
        stock.name,
        if on { "from" } else { "to" }
    );
    button.set_tooltip_text(Some(&action));
    button.update_property(&[
        gtk::accessible::Property::Label(&action),
        gtk::accessible::Property::Description(crate::help::FAVORITE),
    ]);
}

fn favorite_menu(ui: &Rc<Editor>, stock: &'static Stock, widget: &gtk::Widget, x: f64, y: f64) {
    let popover = gtk::Popover::new();
    popover.set_parent(widget);
    popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
    let button = gtk::Button::with_label(if is_favorite(ui, stock.paper, &stock.id) {
        "Remove from Favourites"
    } else {
        "Add to Favourites"
    });
    button.set_tooltip_text(Some(crate::help::FAVORITE));
    button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        #[weak]
        popover,
        move |_| {
            popover.popdown();
            toggle_favorite(&ui, stock.paper, &stock.id);
        }
    ));
    popover.set_child(Some(&button));
    popover.connect_closed(|p| {
        let p = p.clone();
        glib::idle_add_local_once(move || p.unparent());
    });
    popover.popup();
}

fn favorite_context(ui: &Rc<Editor>, stock: &'static Stock, button: &gtk::Button) {
    let click = gtk::GestureClick::new();
    click.set_button(3);
    click.connect_pressed(glib::clone!(
        #[weak]
        ui,
        #[weak]
        button,
        move |click, _, x, y| {
            click.set_state(gtk::EventSequenceState::Claimed);
            favorite_menu(&ui, stock, button.upcast_ref(), x, y);
        }
    ));
    button.add_controller(click);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        ui,
        #[weak]
        button,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, state| {
            if key == gdk::Key::Menu
                || (key == gdk::Key::F10 && state.contains(gdk::ModifierType::SHIFT_MASK))
            {
                favorite_menu(&ui, stock, button.upcast_ref(), 0., button.height() as f64);
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    button.add_controller(keys);
}

/// Refreshes grids and plain menus without rebuilding controls or dropping
/// rendered thumbnails. A preference-only change creates no edit/undo entry.
pub fn refresh_favorites(ui: &Rc<Editor>) {
    for grid in ui.stock_browser.grids.borrow().iter() {
        arrange(ui, grid);
    }
    for menu in ui.stock_browser.menus.borrow().iter() {
        refresh_menu(ui, menu);
    }
}

fn arrange(ui: &Editor, grid: &Grid) {
    let query = grid.search.as_ref().map(|s| s.text()).unwrap_or_default();
    let searching = !query.trim().is_empty();
    let stocks = if searching {
        crate::catalog::stocks(grid.paper)
            .into_iter()
            .filter(|s| crate::catalog::matches(s, &query))
            .collect()
    } else {
        ordered_stocks(ui, grid.paper)
    };
    let order: Vec<_> = stocks
        .iter()
        .map(|s| {
            (
                s.id.clone(),
                !searching && is_favorite(ui, grid.paper, &s.id),
            )
        })
        .collect();
    for tile in &grid.tiles {
        favorite_label(
            &tile.favorite,
            tile.stock,
            is_favorite(ui, grid.paper, &tile.stock.id),
        );
    }
    if *grid.order.borrow() == order {
        return;
    }
    for tile in &grid.tiles {
        if let Some(child) = tile.widget.parent().and_downcast::<gtk::FlowBoxChild>() {
            // Event dispatch can retain a removed wrapper. Detach the tile
            // explicitly before appending it to the rebuilt grid.
            child.set_child(None::<&gtk::Widget>);
            if let Some(flow) = child.parent().and_downcast::<gtk::FlowBox>() {
                flow.remove(&child);
            }
        }
    }
    while let Some(child) = grid.body.first_child() {
        grid.body.remove(&child);
    }
    if stocks.is_empty() {
        let empty = gtk::Label::new(Some("No films found"));
        empty.add_css_class("dim-label");
        empty.set_margin_top(12);
        empty.set_margin_bottom(12);
        grid.body.append(&empty);
    }
    let mut group = String::new();
    let mut flow: Option<gtk::FlowBox> = None;
    for stock in stocks {
        let heading = if !searching && is_favorite(ui, grid.paper, &stock.id) {
            "Favourites"
        } else {
            &stock.group
        };
        if group != heading || flow.is_none() {
            group = heading.to_string();
            let title = gtk::Label::new(Some(heading));
            title.set_xalign(0.);
            title.add_css_class("caption-heading");
            title.add_css_class("dim-label");
            grid.body.append(&title);
            let f = gtk::FlowBox::new();
            f.set_selection_mode(gtk::SelectionMode::None);
            f.set_homogeneous(true);
            f.set_min_children_per_line(2);
            f.set_max_children_per_line(2);
            f.set_column_spacing(6);
            f.set_row_spacing(6);
            grid.body.append(&f);
            flow = Some(f);
        }
        if let Some(tile) = grid.tiles.iter().find(|t| t.stock.id == stock.id) {
            flow.as_ref().unwrap().append(&tile.widget);
        }
    }
    grid.order.replace(order);
}

fn scroll_to_result(ui: &Editor, grid: &Grid) {
    let searching = grid
        .search
        .as_ref()
        .is_some_and(|s| !s.text().trim().is_empty());
    let id = if searching {
        grid.order.borrow().first().map(|(id, _)| id.clone())
    } else {
        Some(
            crate::catalog::selected(&ui.history.borrow().state, grid.paper)
                .id
                .clone(),
        )
    };
    if let Some(tile) = grid.tiles.iter().find(|t| Some(&t.stock.id) == id.as_ref()) {
        let button = tile.button.clone();
        let viewport = grid.viewport.clone();
        glib::idle_add_local_once(move || viewport.scroll_to(&button, None));
    } else {
        grid.scroll.vadjustment().set_value(0.);
    }
}

fn row(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(if paper { "Paper" } else { "Film" })
        .build();
    row.set_tooltip_text(Some(if paper {
        crate::help::PAPER
    } else {
        crate::help::FILM
    }));
    row.set_focusable(true);
    row.update_state(&[gtk::accessible::State::Expanded(Some(false))]);
    row.update_property(&[
        gtk::accessible::Property::Label(if paper { "Paper" } else { "Film" }),
        gtk::accessible::Property::Description(if paper {
            crate::help::PAPER
        } else {
            crate::help::FILM
        }),
    ]);
    // GTK's ExpanderRow exports its expanded state but no AT-SPI action.
    // Give the header its own keyboard path; a focused favourite button
    // keeps its normal Space/Enter behavior instead of toggling the grid.
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        row,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if row.has_focus()
                && matches!(key, gdk::Key::space | gdk::Key::Return | gdk::Key::KP_Enter)
            {
                row.set_expanded(!row.is_expanded());
                glib::Propagation::Stop
            } else {
                glib::Propagation::Proceed
            }
        }
    ));
    row.add_controller(keys);
    let current = gtk::Label::new(None);
    current.add_css_class("dim-label");
    current.set_ellipsize(gtk::pango::EllipsizeMode::End);
    current.set_max_width_chars(20);
    row.add_suffix(&current);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    body.add_css_class("stock-grid");
    let mut tiles = Vec::new();
    for stock in crate::catalog::stocks(paper) {
        let thumb = Rc::new(Thumb::new());
        thumb.picture.set_size_request(-1, 92);
        thumb.picture.add_css_class("stock-thumb");
        let name = gtk::Label::new(Some(&stock.name));
        name.set_ellipsize(gtk::pango::EllipsizeMode::End);
        name.set_xalign(0.);
        name.add_css_class("caption");
        let content = gtk::Box::new(gtk::Orientation::Vertical, 3);
        content.append(&thumb.picture);
        content.append(&name);
        let button = gtk::Button::new();
        button.set_child(Some(&content));
        button.add_css_class("flat");
        button.add_css_class("stock-tile");
        button.set_tooltip_text(Some(&stock.name));
        button.update_property(&[gtk::accessible::Property::Label(&format!(
            "Choose {}",
            stock.name
        ))]);
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
        favorite_context(ui, stock, &button);
        let favorite = favorite_button(ui, stock);
        let widget = gtk::Overlay::new();
        widget.set_child(Some(&button));
        widget.add_overlay(&favorite);
        tiles.push(Tile {
            stock,
            button,
            widget,
            favorite,
            thumb,
        });
    }
    let viewport = gtk::Viewport::new(None::<&gtk::Adjustment>, None::<&gtk::Adjustment>);
    viewport.set_scroll_to_focus(true);
    viewport.set_child(Some(&body));
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .propagate_natural_height(true)
        .max_content_height(290)
        .child(&viewport)
        .build();
    let holder = gtk::ListBoxRow::new();
    holder.set_activatable(false);
    holder.set_selectable(false);
    let content = gtk::Box::new(gtk::Orientation::Vertical, 6);
    let search = (!paper).then(|| {
        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Search films"));
        entry.update_property(&[gtk::accessible::Property::Label("Search films")]);
        entry.set_margin_start(6);
        entry.set_margin_end(6);
        entry.set_margin_top(6);
        content.append(&entry);
        entry
    });
    content.append(&scroll);
    holder.set_child(Some(&content));
    row.add_row(&holder);
    let grid = Rc::new(Grid {
        row: row.clone(),
        paper,
        body,
        viewport,
        scroll,
        search,
        order: RefCell::new(Vec::new()),
        tiles,
    });
    arrange(ui, &grid);
    if let Some(search) = &grid.search {
        search.connect_changed(glib::clone!(
            #[weak]
            ui,
            #[weak]
            grid,
            move |_| {
                arrange(&ui, &grid);
                scroll_to_result(&ui, &grid);
                pump_soon(&ui);
            }
        ));
        search.connect_activate(glib::clone!(
            #[weak]
            grid,
            move |_| {
                let id = grid.order.borrow().first().map(|(id, _)| id.clone());
                if let Some(tile) = grid.tiles.iter().find(|t| Some(&t.stock.id) == id.as_ref()) {
                    tile.button.emit_clicked();
                }
            }
        ));
        search.connect_stop_search(|entry| entry.set_text(""));
    }
    ui.stock_browser.grids.borrow_mut().push(grid.clone());
    row.connect_expanded_notify(glib::clone!(
        #[weak]
        ui,
        #[weak]
        grid,
        move |row| {
            row.update_state(&[gtk::accessible::State::Expanded(Some(row.is_expanded()))]);
            if row.is_expanded() {
                // With the stock controls pinned above the tabs, two open
                // grids would crowd the adjustment controls out of view.
                for other in ui.stock_browser.grids.borrow().iter() {
                    if other.row != *row {
                        other.row.set_expanded(false);
                    }
                }
                scroll_to_result(&ui, &grid);
            }
            pump(&ui);
        }
    ));
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
            glib::idle_add_local_once(move || apply(&ui));
        }
    )));
    list.append(&row);
    row
}

/// The plain-menu variant, refreshed in place when favourites change.
pub fn menu(ui: &Rc<Editor>, paper: bool) -> adw::ComboRow {
    let model = gtk::StringList::new(&[]);
    let sections = gtk::SortListModel::new(Some(model.clone()), None::<gtk::Sorter>);
    sections.set_section_sorter(Some(&gtk::CustomSorter::new(|a, b| {
        let rank = |o: &glib::Object| {
            let text = o.downcast_ref::<gtk::StringObject>().unwrap().string();
            let heading = text.split_once(" · ").unwrap().0;
            if heading == "Favourites" {
                0
            } else {
                crate::catalog::group_order(heading) + 1
            }
        };
        rank(a).cmp(&rank(b)).into()
    })));
    let row = adw::ComboRow::builder()
        .title(if paper { "Paper" } else { "Film" })
        .model(&sections)
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        ))
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .build();
    row.set_tooltip_text(Some(if paper {
        crate::help::PAPER
    } else {
        crate::help::FILM
    }));
    let factory = |popup: bool| {
        let f = gtk::SignalListItemFactory::new();
        f.connect_setup(move |_, o| {
            let item = o.downcast_ref::<gtk::ListItem>().unwrap();
            let label = gtk::Label::new(None);
            label.set_xalign(0.);
            label.set_wrap(popup);
            if !popup {
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(23);
            }
            item.set_child(Some(&label));
        });
        f.connect_bind(|_, o| {
            let item = o.downcast_ref::<gtk::ListItem>().unwrap();
            let text = item
                .item()
                .and_downcast::<gtk::StringObject>()
                .unwrap()
                .string();
            item.child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .set_text(text.split_once(" · ").unwrap().1);
        });
        f
    };
    row.set_factory(Some(&factory(false)));
    row.set_list_factory(Some(&factory(true)));
    let headers = gtk::SignalListItemFactory::new();
    headers.connect_setup(|_, o| {
        let header = o.downcast_ref::<gtk::ListHeader>().unwrap();
        let label = gtk::Label::new(None);
        label.set_xalign(0.);
        label.add_css_class("heading");
        label.add_css_class("dim-label");
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(8);
        label.set_margin_bottom(4);
        header.set_child(Some(&label));
    });
    headers.connect_bind(|_, o| {
        let header = o.downcast_ref::<gtk::ListHeader>().unwrap();
        let text = header
            .item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        header
            .child()
            .and_downcast::<gtk::Label>()
            .unwrap()
            .set_text(text.split_once(" · ").unwrap().0);
    });
    row.set_header_factory(Some(&headers));
    let favorite = gtk::Button::from_icon_name("non-starred-symbolic");
    favorite.add_css_class("flat");
    favorite.set_valign(gtk::Align::Center);
    row.add_suffix(&favorite);
    favorite.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| {
            let selected = crate::catalog::selected(&ui.history.borrow().state, paper);
            toggle_favorite(&ui, paper, &selected.id);
        }
    ));
    let menu = Rc::new(Menu {
        paper,
        row: row.clone(),
        model,
        ids: RefCell::new(Vec::new()),
        favorite,
        updating: Cell::new(false),
    });
    refresh_menu(ui, &menu);
    row.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[weak]
        menu,
        move |row| {
            if ui.syncing.get() || menu.updating.get() {
                return;
            }
            let stock = menu
                .ids
                .borrow()
                .get(row.selected() as usize)
                .and_then(|id| {
                    crate::catalog::stocks(paper)
                        .into_iter()
                        .find(|s| s.id == *id)
                });
            let Some(stock) = stock else {
                return;
            };
            let mut h = ui.history.borrow_mut();
            h.begin(if paper { "Change Paper" } else { "Change Film" });
            crate::catalog::choose(&mut h.state, stock, ui.preferences.borrow().keep_paper);
            h.commit();
            drop(h);
            ui.sync_state();
        }
    ));
    ui.rules.borrow_mut().push(Box::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        menu,
        move |state| {
            select_menu(&ui, &menu, crate::catalog::selected(state, paper));
        }
    )));
    ui.stock_browser.menus.borrow_mut().push(menu);
    row
}

fn select_menu(ui: &Editor, menu: &Menu, selected: &Stock) {
    menu.updating.set(true);
    menu.row.set_selected(
        menu.ids
            .borrow()
            .iter()
            .position(|id| *id == selected.id)
            .unwrap_or(0) as u32,
    );
    favorite_label(
        &menu.favorite,
        selected,
        is_favorite(ui, menu.paper, &selected.id),
    );
    menu.updating.set(false);
}

fn refresh_menu(ui: &Editor, menu: &Menu) {
    let stocks = ordered_stocks(ui, menu.paper);
    let labels: Vec<_> = stocks
        .iter()
        .map(|stock| {
            format!(
                "{} · {}",
                if is_favorite(ui, menu.paper, &stock.id) {
                    "Favourites"
                } else {
                    &stock.group
                },
                stock.name
            )
        })
        .collect();
    menu.updating.set(true);
    menu.ids
        .replace(stocks.iter().map(|stock| stock.id.clone()).collect());
    menu.model.splice(
        0,
        menu.model.n_items(),
        &labels.iter().map(String::as_str).collect::<Vec<_>>(),
    );
    menu.updating.set(false);
    select_menu(
        ui,
        menu,
        crate::catalog::selected(&ui.history.borrow().state, menu.paper),
    );
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
        for (id, _) in grid.order.borrow().iter() {
            let tile = grid.tiles.iter().find(|tile| tile.stock.id == *id).unwrap();
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
    // Mark/blank stale tiles first, but don't queue optional work ahead of
    // a live brightness solve or its final preview. This reads runtime on
    // its own, before we take the scheduler's runtime borrow below.
    if browser.busy.get() || crate::brightness::pending(ui) {
        return;
    }
    let client = {
        let r = ui.runtime.borrow();
        if !r.ready
            || r.closing
            || r.rendering
            || r.interacting
            || r.block_helper_render
            || r.current_revision != Some(r.revision)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_grid_tiles(grid: &Grid) {
        let mut ids = Vec::new();
        let mut section = grid.body.first_child();
        while let Some(widget) = section {
            section = widget.next_sibling();
            let Ok(flow) = widget.downcast::<gtk::FlowBox>() else {
                continue;
            };
            let mut child = flow.first_child();
            while let Some(widget) = child {
                child = widget.next_sibling();
                let content = widget
                    .downcast::<gtk::FlowBoxChild>()
                    .unwrap()
                    .child()
                    .expect("Every grid slot must contain a stock tile");
                let tile = grid
                    .tiles
                    .iter()
                    .find(|tile| tile.widget.upcast_ref::<gtk::Widget>() == &content)
                    .expect("Every grid slot must contain an existing stock tile");
                ids.push(tile.stock.id.clone());
            }
        }
        assert_eq!(
            ids,
            grid.order
                .borrow()
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>(),
            "The displayed tiles must match the complete stock order"
        );
    }

    fn toggle_and_check_grid(grid: &Grid, tile: &Tile) {
        // GTK can retain the clicked tile's wrapper through event dispatch.
        let previous_parent = tile.widget.parent().unwrap();
        tile.favorite.emit_clicked();
        assert_grid_tiles(grid);
        drop(previous_parent);
    }

    #[test]
    #[ignore = "Requires a desktop and isolated NEGS_PROFILE_DIR; run separately with --ignored --test-threads=1"]
    fn film_search_filters_without_editing_and_keeps_tiles() {
        assert!(std::env::var_os("NEGS_PROFILE_DIR").is_some());
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.negs.FilmSearchTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Editor::new(&app, crate::perf::Perf::new());
        let list = gtk::ListBox::new();
        ui.inspector.append(&list);
        row(&ui, &list, false);
        row(&ui, &list, true);
        ui.window.present();
        let film = ui.stock_browser.grids.borrow()[0].clone();
        let paper = ui.stock_browser.grids.borrow()[1].clone();
        assert!(paper.search.is_none());
        film.row.set_expanded(true);
        let search = film.search.as_ref().unwrap();
        let before = ui.history.borrow().state.clone();
        let tile = film
            .tiles
            .iter()
            .find(|t| t.stock.id == "kodak_portra_400")
            .unwrap();
        let texture = crate::canvas::srgb_texture(
            1,
            1,
            gdk::MemoryFormat::R8g8b8,
            &glib::Bytes::from_owned(vec![128; 3]),
            3,
        );
        tile.thumb.show(Some(&texture), 7, "retained".into());
        search.set_text("PORTRA 400");
        assert_eq!(
            film.order.borrow().as_slice(),
            &[("kodak_portra_400".into(), false)]
        );
        assert_grid_tiles(&film);
        assert_eq!(ui.history.borrow().state, before);
        assert!(!ui.history.borrow().can_undo());
        assert!(tile.thumb.picture.paintable().is_some());
        tile.favorite.emit_clicked();
        assert_eq!(film.order.borrow().len(), 1);
        assert_eq!(
            film.order.borrow()[0].1,
            false,
            "Search uses catalog groups, once each"
        );
        search.emit_by_name::<()>("activate", &[]);
        assert_eq!(
            crate::catalog::selected(&ui.history.borrow().state, false).id,
            tile.stock.id
        );
        assert!(ui.history.borrow().can_undo());
        search.set_text("not-a-film");
        assert!(film.order.borrow().is_empty());
        assert_eq!(
            film.body
                .first_child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .text(),
            "No films found"
        );
        let chosen = ui.history.borrow().state.clone();
        search.emit_by_name::<()>("activate", &[]);
        assert_eq!(ui.history.borrow().state, chosen);
        search.emit_by_name::<()>("stop-search", &[]);
        assert!(search.text().is_empty());
        assert_grid_tiles(&film);
        assert_eq!(
            film.order.borrow().len(),
            crate::catalog::stocks(false).len()
        );
        assert!(tile.thumb.picture.paintable().is_some());
        paper.row.set_expanded(true);
        assert!(!film.row.is_expanded());
        ui.window.destroy();
        while glib::MainContext::default().iteration(false) {}
    }

    #[test]
    #[ignore = "Requires a desktop and an isolated NEGS_PROFILE_DIR; run explicitly with --ignored --test-threads=1"]
    fn native_stock_favourites_preserve_edits_thumbnails_and_foldout_space() {
        use std::time::Instant;
        assert!(
            std::env::var_os("NEGS_PROFILE_DIR").is_some(),
            "Use an isolated NEGS_PROFILE_DIR"
        );
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.negs.StockBrowserTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Editor::new(&app, crate::perf::Perf::new());
        // This ordering test starts with no stars. Default favourite migration
        // is covered separately; new profiles now already contain four films.
        ui.preferences.borrow_mut().favorite_films.clear();
        ui.preferences.borrow_mut().favorite_papers.clear();
        let list = gtk::ListBox::new();
        ui.inspector.append(&list);
        for paper in [false, true] {
            let picker = menu(&ui, paper);
            list.append(&picker);
            rows(&ui, &list, paper, picker.upcast());
        }
        ui.window.present();
        let settle = || {
            let deadline = Instant::now() + Duration::from_millis(150);
            while Instant::now() < deadline {
                while glib::MainContext::default().iteration(false) {}
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        settle();
        let film = ui.stock_browser.grids.borrow()[0].clone();
        let paper = ui.stock_browser.grids.borrow()[1].clone();
        assert!(film.row.is_mapped() && paper.row.is_mapped());
        film.row.set_expanded(true);
        assert!(film.row.is_expanded() && !paper.row.is_expanded());
        paper.row.set_expanded(true);
        assert!(paper.row.is_expanded() && !film.row.is_expanded());
        film.row.set_expanded(true);
        assert!(film.row.is_expanded() && !paper.row.is_expanded());
        assert!(film.row.is_focusable() && paper.row.is_focusable());
        let before = ui.history.borrow().state.clone();
        let current = crate::catalog::selected(&before, false).id.clone();
        let tile = film
            .tiles
            .iter()
            .rev()
            .find(|t| t.stock.id != current)
            .unwrap();
        let texture = crate::canvas::srgb_texture(
            1,
            1,
            gdk::MemoryFormat::R8g8b8,
            &glib::Bytes::from_owned(vec![128u8; 3]),
            3,
        );
        tile.thumb
            .show(Some(&texture), 7, "kept-stock-thumbnail".into());
        toggle_and_check_grid(&film, tile);
        assert!(is_favorite(&ui, false, &tile.stock.id));
        assert_eq!(ordered_stocks(&ui, false)[0].id, tile.stock.id);
        assert_eq!(film.order.borrow()[0], (tile.stock.id.clone(), true));
        let picker = ui.stock_browser.menus.borrow()[0].clone();
        assert_eq!(picker.ids.borrow()[0], tile.stock.id);
        assert!(picker.model.string(0).unwrap().starts_with("Favourites · "));
        assert_eq!(picker.ids.borrow()[picker.row.selected() as usize], current);
        for grid in [&film, &paper] {
            let tile = grid.tiles.last().unwrap();
            for _ in 0..4 {
                toggle_and_check_grid(grid, tile);
            }
        }
        assert_eq!(
            ui.history.borrow().state,
            before,
            "Starring a stock is a preference, not a photo edit"
        );
        assert!(
            !ui.history.borrow().can_undo(),
            "Starring must not create an undo step"
        );
        assert_eq!(
            *tile.thumb.shown.borrow(),
            Some((7, "kept-stock-thumbnail".into()))
        );
        assert!(
            tile.thumb.picture.paintable().is_some(),
            "Reordering must retain the rendered tile"
        );
        // Both display variants update immediately and use the same ordering.
        ui.preferences.borrow_mut().previews = false;
        apply(&ui);
        assert!(picker.row.is_visible() && !film.row.is_visible());
        assert_eq!(picker.ids.borrow()[0], tile.stock.id);
        toggle_and_check_grid(&film, tile);
        assert!(!is_favorite(&ui, false, &tile.stock.id));
        assert_eq!(
            picker.ids.borrow().as_slice(),
            crate::catalog::stocks(false)
                .iter()
                .map(|s| s.id.clone())
                .collect::<Vec<_>>()
        );
        toggle_and_check_grid(&film, tile);
        ui.save_session().unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(Editor::session_path()).unwrap()).unwrap();
        assert!(
            saved["preferences"]["favorite_films"]
                .as_array()
                .unwrap()
                .contains(&json!(tile.stock.id))
        );
        assert_eq!(ui.history.borrow().state, before);
        ui.window.destroy();
        settle();
    }
}
