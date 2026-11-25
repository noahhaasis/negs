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
        self.menu = Some(MenuState { at, entries: library_entries(self), from: Some(item) });
        self.after_model_update(cx);
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
        let model = &mut self.model;
        match command {
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
    let height = menu.entries.iter().map(|e| if e.command.is_some() { ROW } else { 9.0 }).sum::<f32>() + 8.0;
    let x = f32::from(menu.at.x).min(f32::from(viewport.width) - WIDTH - 8.0).max(8.0);
    let y = f32::from(menu.at.y).min(f32::from(viewport.height) - height - 8.0).max(8.0);
    let hover = theme.subtle_hover;
    let mut list = div()
        .id("library-menu")
        .absolute()
        .left(px(x))
        .top(px(y))
        .w(px(WIDTH))
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
