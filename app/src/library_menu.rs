//! The library's commands at the pointer (`windows/src/ui/library.rs`,
//! `MainMenu.swift`'s Photo menu): the strip's and grid's context menu
//! (ratings, flags, copy/paste edits, duplicate, reset, Trash) and the grid
//! sentence's filter menu, drawn as a darkroom popover over the window, and
//! Delete's question (`Delete.swift`) as the platform's alert.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Pixels, Point,
    PromptLevel, SharedString, StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px,
};
use negs_app::model::LibraryKey;
use negs_library::{EditsFilter, Filter, Flag, FlagFilter, Item, RatingFilter};

use crate::NegsApp;

/// One thing the menus can do.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// The menu's first item (ticket 46): "Compare 3 and 7", "Compare with
    /// 3" or "Compare with Original".
    CompareOffer,
    Rate(i64),
    Raise,
    Lower,
    Pick,
    Reject,
    Unflag,
    CopyEdits,
    PasteEdits,
    PastePrevious,
    Duplicate,
    ResetEdits,
    Trash,
    TrashRejected,
    Advance,
    ClearFilter,
    FilterRating(usize),
    FilterFlag(usize),
    FilterEdited,
    FilterUnedited,
    /// "Export 3 Photographs…": the export card (ticket 48).
    Export,
    Lightroom(negs_app::lightroom::Kind),
    /// The `index`th entry of the film's or paper's stock menu, with the
    /// tiles off (`stock_entries`); `read_as`: Read as's films.
    Stock { paper: bool, read_as: bool, index: usize },
    /// The film menu's Your negative.
    Negative,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub label: SharedString,
    pub hint: Option<SharedString>,
    pub enabled: bool,
    pub checked: bool,
    /// `None`: a rule between groups.
    pub command: Option<Command>,
}

fn entry(label: impl Into<SharedString>, hint: Option<&str>, enabled: bool, checked: bool, command: Command) -> Entry {
    Entry { label: label.into(), hint: hint.map(|h| SharedString::from(h.to_string())), enabled, checked, command: Some(command) }
}

fn rule() -> Entry {
    Entry { label: SharedString::default(), hint: None, enabled: false, checked: false, command: None }
}

#[derive(Clone, Debug)]
pub struct MenuState {
    /// Where it opened, in the window.
    pub at: Point<Pixels>,
    pub entries: Vec<Entry>,
    /// The photograph right-clicked (Copy Edits copies it, not the open one).
    pub from: Option<Item>,
}

const WIDTH: f32 = 236.0;
const ROW: f32 = 26.0;

fn modifier() -> &'static str {
    if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" }
}

/// The Photo menu's library items, enabled and ticked as now.
pub fn library_entries(app: &NegsApp) -> Vec<Entry> {
    let model = &app.model;
    let library = model.shelf.library.as_ref();
    let can = model.can_change_library();
    let summary = library.zip(model.rating_targets().first().cloned()).map(|(l, i)| l.summary(&i));
    let m = modifier();
    let mut out = vec![];
    for stars in (0..=5i64).rev() {
        let label = if stars == 0 { "No Rating".to_string() } else { "★".repeat(stars as usize) };
        out.push(entry(label, Some(&stars.to_string()), can, summary.as_ref().is_some_and(|s| s.rating as i64 == stars), Command::Rate(stars)));
    }
    out.push(rule());
    let flag = summary.as_ref().and_then(|s| s.flag);
    out.push(entry("Pick", Some("P"), can, flag == Some(Flag::Pick), Command::Pick));
    out.push(entry("Reject", Some("X"), can, flag == Some(Flag::Reject), Command::Reject));
    out.push(entry("Unflagged", Some("U"), can, summary.is_some() && flag.is_none(), Command::Unflag));
    out.push(rule());
    out.push(entry("Copy Edits", Some(&format!("{m}⇧C")), can, false, Command::CopyEdits));
    out.push(entry("Paste Edits", Some(&format!("{m}⇧V")), model.can_paste_edits(), false, Command::PasteEdits));
    out.push(entry("Paste from Previous", None, model.can_paste_from_previous(), false, Command::PastePrevious));
    out.push(rule());
    // Export and each Lightroom installed, between the edits and Duplicate
    // as on the Mac (`batch::export_items`).
    let targets = model.export_targets().len();
    out.push(entry(if targets > 1 { format!("Export {targets} Photographs…") } else { "Export…".into() }, None, model.can_export_any(), false, Command::Export));
    for kind in model.lightroom_kinds() {
        out.push(entry(negs_app::lightroom::menu_title(kind, targets), None, model.can_open_in_lightroom(kind), false, Command::Lightroom(kind)));
    }
    out.push(rule());
    out.push(entry("Duplicate", Some(&format!("{m}D")), can, false, Command::Duplicate));
    out.push(entry("Reset Edits", None, can, false, Command::ResetEdits));
    out.push(rule());
    let count = model.delete_targets().len();
    let trash = if cfg!(target_os = "macos") { "Trash" } else { "Recycle Bin" };
    let title = if count > 1 { format!("Move {count} Photographs to {trash}…") } else { format!("Move to {trash}…") };
    out.push(entry(title, Some(if cfg!(target_os = "macos") { "⌫" } else { "Del" }), can && count > 0, false, Command::Trash));
    out.push(entry("Delete Rejected Photographs…", None, can && !model.rejected().is_empty(), false, Command::TrashRejected));
    out.push(rule());
    out.push(entry("Advance After Rating", None, library.is_some(), model.prefs.library.advance_after_rating, Command::Advance));
    out
}

/// The film or paper menu's entries, with the tiles off (`windows/`'s
/// `stock_entries`): the favourites, starred, then each group's heading
/// (None: can't be chosen) and stocks. Read as lists the colour negatives.
pub fn stock_entries(model: &negs_app::model::Model, paper: bool, read_as: bool) -> Vec<(String, Option<(String, bool)>)> {
    let (films, papers) = (model.listed_films(), model.listed_papers());
    let groups = if paper { negs_app::state::paper_groups(&papers) } else { negs_app::state::film_groups(&films, &model.papers()) };
    let mut entries: Vec<(String, Option<(String, bool)>)> = model.favorite_stocks(paper).iter().map(|s| (format!("★ {}", s.name), Some((s.id.clone(), true)))).collect();
    for group in groups {
        entries.push((group.title.into(), None));
        entries.extend(group.stocks.iter().map(|s| (s.name.clone(), Some((s.id.clone(), false)))));
    }
    if read_as {
        let readable = |id: &str| model.stocks.iter().any(|s| s.id == id && negs_app::negative::readable(s));
        entries.retain(|(_, e)| e.as_ref().is_none_or(|(id, _)| readable(id)));
    }
    entries
}

/// The stock menu as the library menu's rows: headings dimmed, the chosen
/// stock ticked, and the film's ends with Your negative.
pub fn stock_menu(model: &negs_app::model::Model, paper: bool, read_as: bool) -> Vec<Entry> {
    let current = if paper { &model.state.paper } else { &model.state.film };
    let mut out: Vec<Entry> = stock_entries(model, paper, read_as)
        .into_iter()
        .enumerate()
        .map(|(index, (label, e))| {
            let command = Command::Stock { paper, read_as, index };
            match e {
                None => entry(label, None, false, false, command),
                Some((id, favourite)) => entry(label, None, true, &id == current && !favourite, command),
            }
        })
        .collect();
    if !paper && !read_as {
        out.push(rule());
        out.push(entry(negs_app::negative::NAME, None, true, model.is_negative(), Command::Negative));
    }
    out
}

/// The Filter submenu's items (also the grid sentence's menu).
pub fn filter_entries(app: &NegsApp) -> Vec<Entry> {
    let model = &app.model;
    let on = model.shelf.library.is_some();
    let filter = model.library_filter();
    let mut out = vec![];
    for (i, (rating, title)) in Filter::rating_choices().iter().enumerate() {
        out.push(entry(title.clone(), None, on, filter.rating == *rating, Command::FilterRating(i)));
    }
    out.push(rule());
    for (i, (flag, title)) in FlagFilter::CHOICES.iter().enumerate() {
        out.push(entry(*title, None, on, filter.flags.contains(flag), Command::FilterFlag(i)));
    }
    out.push(rule());
    out.push(entry("Edited", None, on, filter.edits == EditsFilter::Edited, Command::FilterEdited));
    out.push(entry("Unedited", None, on, filter.edits == EditsFilter::Unedited, Command::FilterUnedited));
    out.push(rule());
    out.push(entry("Clear Filter", None, filter.active(), false, Command::ClearFilter));
    out
}

impl NegsApp {
    /// Right click on a frame: it joins the selection (else it alone, as in
    /// Finder and Explorer), and the library's menu opens at the pointer.
    pub fn open_library_menu(&mut self, item: Item, at: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.model.shelf.selection.contains(&item) {
            self.model.shelf.selection.select(&item);
        }
        let offer = self.model.compare_offer(&item, self.grid.shown);
        let mut entries = vec![entry(offer.title.clone(), Some("C"), offer.enabled, false, Command::CompareOffer), rule()];
        self.model.offer_compare(offer);
        entries.extend(library_entries(self));
        self.menu = Some(MenuState { at, entries, from: Some(item) });
        self.after_model_update(cx);
        cx.notify();
    }

    /// With the tiles off, the film's or paper's stock menu at `at`.
    pub fn open_stock_menu(&mut self, paper: bool, read_as: bool, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.menu = Some(MenuState { at, entries: stock_menu(&self.model, paper, read_as), from: None });
        cx.notify();
    }

    pub fn open_filter_menu(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        self.menu = Some(MenuState { at, entries: filter_entries(self), from: None });
        cx.notify();
    }

    pub fn close_menu(&mut self) -> bool {
        self.menu.take().is_some()
    }

    pub fn run_command(&mut self, command: Command, from: Option<Item>, window: &mut Window, cx: &mut Context<Self>) {
        if command == Command::CompareOffer {
            crate::compare_view::take_offer(self);
            self.after_model_update(cx);
            cx.notify();
            return;
        }
        let model = &mut self.model;
        match command {
            Command::CompareOffer => {}
            Command::Export => return crate::export_card::open(self, true, cx),
            Command::Lightroom(kind) => model.open_in_lightroom(kind),
            Command::Negative => model.choose_negative(),
            Command::Stock { paper, read_as, index } => {
                if let Some(Some((id, _))) = stock_entries(model, paper, read_as).get(index).map(|(_, e)| e.clone()) {
                    if paper {
                        model.choose_paper(&id)
                    } else if read_as {
                        model.choose_negative_film(&id)
                    } else {
                        model.choose_film(&id)
                    }
                }
            }
            Command::Rate(stars) => model.library_key(LibraryKey::Rate(stars)),
            Command::Raise => model.library_key(LibraryKey::Raise),
            Command::Lower => model.library_key(LibraryKey::Lower),
            Command::Pick => model.library_key(LibraryKey::Pick),
            Command::Reject => model.library_key(LibraryKey::Reject),
            Command::Unflag => model.library_key(LibraryKey::Unflag),
            Command::CopyEdits => model.copy_edits(from, None),
            Command::PasteEdits => model.paste_edits(),
            Command::PastePrevious => model.paste_from_previous(),
            Command::Duplicate => model.duplicate_photos(),
            Command::ResetEdits => model.reset_photo_edits(),
            Command::Trash => self.request_delete(None, true, window, cx),
            Command::TrashRejected => {
                let rejected = self.model.rejected();
                self.request_delete(Some(rejected), true, window, cx);
            }
            Command::Advance => model.toggle_advance_after_rating(),
            Command::ClearFilter => model.set_library_filter(Filter::default()),
            Command::FilterRating(i) => {
                let mut filter = model.library_filter();
                filter.rating = Filter::rating_choices().get(i).map_or(RatingFilter::Any, |(r, _)| *r);
                model.set_library_filter(filter);
            }
            Command::FilterFlag(i) => {
                let mut filter = model.library_filter();
                filter.toggle_flag(FlagFilter::CHOICES[i].0);
                model.set_library_filter(filter);
            }
            Command::FilterEdited | Command::FilterUnedited => {
                let mut filter = model.library_filter();
                filter.toggle_edits(if command == Command::FilterEdited { EditsFilter::Edited } else { EditsFilter::Unedited });
                model.set_library_filter(filter);
            }
        }
        self.after_model_update(cx);
        cx.notify();
    }

    /// Deletes the rating targets, or `items`, asking first when `asking`
    /// or when their volume has no Trash (`requestDelete`). Undo puts them
    /// back.
    pub fn request_delete(&mut self, items: Option<Vec<negs_library::Item>>, asking: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.model.can_change_library() {
            return;
        }
        let items = items.unwrap_or_else(|| self.model.delete_targets());
        let Some((plan, question, permanent)) = self.model.delete_question(&items) else { return };
        if !(asking || permanent) {
            self.model.delete_photos(plan);
            self.after_model_update(cx);
            cx.notify();
            return;
        }
        let answer = window.prompt(PromptLevel::Warning, &question.message, Some(&question.information), &[question.button.as_str(), "Cancel"], cx);
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(0) {
                let _ = this.update(cx, |app, cx| {
                    app.model.delete_photos(plan);
                    app.after_model_update(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }
}

/// The open menu, over everything, kept inside the window.
pub fn render_menu(app: &NegsApp, window: &Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    let menu = app.menu.as_ref()?;
    let theme = &app.theme;
    let viewport = window.viewport_size();
    // A long menu (the stocks) scrolls inside the window.
    let height = (menu.entries.iter().map(|e| if e.command.is_some() { ROW } else { 9.0 }).sum::<f32>() + 8.0).min(f32::from(viewport.height) - 16.0);
    let x = f32::from(menu.at.x).min(f32::from(viewport.width) - WIDTH - 8.0).max(8.0);
    let y = f32::from(menu.at.y).min(f32::from(viewport.height) - height - 8.0).max(8.0);
    let hover = theme.subtle_hover;
    let mut list = div()
        .id("library-menu")
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(WIDTH))
        .max_h(px(height))
        .overflow_y_scroll()
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(theme.panel)
        .border_1()
        .border_color(theme.track)
        .shadow_md()
        .flex()
        .flex_col()
        .font_family(theme.ui_family.clone())
        .on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _window, cx| {
            if this.close_menu() {
                cx.notify();
            }
        }))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
    for (i, e) in menu.entries.iter().enumerate() {
        let Some(command) = e.command else {
            list = list.child(div().my(px(4.0)).w_full().h(px(1.0)).bg(theme.line));
            continue;
        };
        let color = if e.enabled { theme.paper } else { theme.dim.opacity(0.45) };
        let from = menu.from.clone();
        let row = div()
            .id(("library-menu-row", i))
            .flex()
            .items_center()
            .w_full()
            .h(px(ROW))
            .px(px(10.0))
            .gap(px(6.0))
            .text_size(px(12.5))
            .text_color(color)
            .child(div().w(px(12.0)).text_color(theme.accent).child(if e.checked { "✓" } else { "" }))
            .child(div().flex_1().min_w_0().truncate().child(e.label.clone()))
            .when_some(e.hint.clone(), |el, h| el.child(div().text_size(px(11.0)).text_color(theme.dim).child(h)))
            .when(e.enabled, |el| {
                el.cursor_pointer().hover(move |s| s.bg(hover)).on_click(cx.listener(move |this, _, window, cx| {
                    this.menu = None;
                    this.run_command(command, from.clone(), window, cx);
                }))
            });
        list = list.child(row);
    }
    Some(deferred(list).with_priority(10).into_any_element())
}

#[cfg(test)]
mod stock_menu_tests {
    use super::*;
    use negs_app::session::Preferences;
    use negs_app::state::Stock;

    fn stock(id: &str, name: &str, paper: bool, bw: bool) -> Stock {
        Stock {
            id: id.into(),
            name: name.into(),
            paper,
            positive: false,
            bw,
            target: if paper { String::new() } else { "kodak_portra_endura".into() },
            times: vec![1.0, 2.0],
            grain: serde_json::Value::Null,
            fotufilm: false,
            manufacturer_data: false,
            negs_built: false,
            cine_print: false,
        }
    }

    fn model() -> negs_app::model::Model {
        let mut model = negs_app::model::Model::new(Preferences::default());
        model.stocks = vec![stock("kodak_gold_200", "Kodak Gold 200", false, false), stock("ilford_hp5", "Ilford HP5 Plus", false, true), stock("kodak_portra_endura", "Kodak Portra Endura", true, false)];
        model.state.film = "kodak_gold_200".into();
        model
    }

    #[test]
    fn the_stock_menu_ticks_the_film_and_ends_with_your_negative() {
        let model = model();
        let menu = stock_menu(&model, false, false);
        let names: Vec<_> = menu.iter().filter(|e| e.enabled).map(|e| e.label.to_string()).collect();
        assert!(names.contains(&"Kodak Gold 200".to_string()) && names.contains(&"Ilford HP5 Plus".to_string()), "{names:?}");
        assert!(menu.iter().any(|e| e.checked && e.label.as_ref() == "Kodak Gold 200"));
        assert_eq!(menu.last().map(|e| e.label.to_string()), Some(negs_app::negative::NAME.to_string()));
        // Headings can't be chosen.
        assert!(menu.iter().any(|e| !e.enabled && e.command.is_some()));
    }

    #[test]
    fn read_as_lists_only_colour_negatives() {
        let model = model();
        let entries = stock_entries(&model, false, true);
        assert!(entries.iter().all(|(_, e)| e.as_ref().is_none_or(|(id, _)| id != "ilford_hp5")));
        assert!(entries.iter().any(|(_, e)| e.as_ref().is_some_and(|(id, _)| id == "kodak_gold_200")));
        let papers = stock_entries(&model, true, false);
        assert!(papers.iter().any(|(_, e)| e.as_ref().is_some_and(|(id, _)| id == "kodak_portra_endura")));
    }
}
