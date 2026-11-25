//! The menu bar in the title bar, off the Mac: GPUI keeps the menus it's
//! given on Windows and Linux but draws none, so their words sit after
//! "Negs" as the Windows app's do (`ui/titlebar.rs`), and each opens its
//! menu under it. The items are `cx.get_menus()`'s, the same as the Mac's
//! menu bar, enabled, ticked and titled as the model says (`menu_state`),
//! and choosing one dispatches its action. Photo › Filter is the grid
//! sentence's menu.
//!
//! From the keyboard, as Windows menus: Alt (alone) or F10 puts the bar in
//! focus with the access keys underlined; Alt+letter opens that menu; ← →
//! move between menus, ↑ ↓ between items, → and ← in and out of a submenu,
//! Return or Space chooses, a letter chooses the item it underlines, Esc
//! steps back.

use gpui::{
    Action, AnyElement, App, Context, FontWeight, HighlightStyle, InteractiveElement, IntoElement, KeyDownEvent, OwnedMenuItem,
    ParentElement, StatefulInteractiveElement, StyledText, Styled, UnderlineStyle, Window, deferred, div, prelude::FluentBuilder, px,
};

use crate::NegsApp;
use crate::library_menu::{self, Command};

/// The Photo menu's Filter submenu: its rows come from the library.
pub const FILTER: &str = "F&ilter";

/// Which menu is open, and which of its rows.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Open {
    pub menu: usize,
    /// The menu's items show (false: only its word is highlighted, after Alt or F10).
    pub dropped: bool,
    /// The row the keyboard is on.
    pub hot: Option<usize>,
    pub sub: Option<usize>,
    pub sub_hot: Option<usize>,
    /// Opened from the keyboard: the access keys are underlined.
    pub keys: bool,
}

impl Open {
    pub fn mouse(menu: usize) -> Self {
        Open { menu, dropped: true, ..Open::default() }
    }
}

/// Alt pressed on its own, waiting to see whether another key follows.
#[derive(Default, Clone, Copy, Debug)]
pub struct AltState {
    pub down: bool,
    pub armed: bool,
}

pub enum Act {
    Action(Box<dyn Action>),
    Command(Command),
    Sub(Vec<Row>),
    Rule,
}

pub struct Row {
    /// With `&` before the access key.
    pub label: String,
    pub hint: Option<String>,
    pub enabled: bool,
    pub checked: bool,
    pub act: Act,
}

impl Row {
    fn rule() -> Self {
        Row { label: String::new(), hint: None, enabled: false, checked: false, act: Act::Rule }
    }
    fn selectable(&self) -> bool {
        !matches!(self.act, Act::Rule)
    }
}

/// "ctrl-shift-e" as Windows writes it: "Ctrl+Shift+E".
pub fn hint(unparsed: &str) -> String {
    let (mods, key) = match unparsed.strip_suffix("--") {
        Some(rest) => (rest, "-"),
        None => unparsed.rsplit_once('-').unwrap_or(("", unparsed)),
    };
    let mut parts: Vec<String> = mods
        .split('-')
        .filter(|m| !m.is_empty())
        .map(|m| match m {
            "ctrl" => "Ctrl".into(),
            "shift" => "Shift".into(),
            "alt" => "Alt".into(),
            "cmd" | "super" | "win" => "Win".into(),
            other => other.to_string(),
        })
        .collect();
    let key = match key {
        "=" | "+" => "=".to_string(),
        k if k.len() == 1 => k.to_uppercase(),
        k if k.starts_with('f') && k[1..].parse::<u8>().is_ok() => k.to_uppercase(),
        "backspace" => "Backspace".into(),
        "delete" => "Del".into(),
        k => {
            let mut c = k.chars();
            c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
        }
    };
    parts.push(key);
    parts.join("+")
}

fn key_hint(action: &dyn Action, window: &Window) -> Option<String> {
    let binding = window.highest_precedence_binding_for_action(action)?;
    let keys = binding.keystrokes();
    (keys.len() == 1).then(|| hint(&keys[0].unparse()))
}

/// The access key: the letter after `&`, or the first letter.
pub fn access_key(label: &str) -> Option<char> {
    let mut chars = label.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                // "&&" is an ampersand.
                Some('&') => continue,
                Some(c) => return Some(c.to_ascii_lowercase()),
                None => break,
            }
        }
    }
    label.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_ascii_lowercase())
}

/// The words without `&`, and the byte range of the access key in them.
pub fn plain(label: &str) -> (String, Option<std::ops::Range<usize>>) {
    let mut out = String::with_capacity(label.len());
    let mut range = None;
    let mut chars = label.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '&' {
            if let Some(&next) = chars.peek() {
                chars.next();
                if next != '&' && range.is_none() {
                    range = Some(out.len()..out.len() + next.len_utf8());
                }
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    (out, range)
}

/// A menu's rows as they are now.
fn rows_of(app: &NegsApp, items: &[OwnedMenuItem], window: &Window, cx: &mut App) -> Vec<Row> {
    let mut out = vec![];
    for item in items {
        match item {
            OwnedMenuItem::Separator => out.push(Row::rule()),
            OwnedMenuItem::Action { name, action, .. } => {
                let state = crate::menu_state::state(app, action.as_ref(), window, cx);
                let enabled = state.as_ref().map_or_else(|| window.is_action_available(action.as_ref(), cx), |s| s.enabled);
                out.push(Row {
                    label: state.as_ref().and_then(|s| s.title.clone()).unwrap_or_else(|| name.to_string()),
                    hint: key_hint(action.as_ref(), window),
                    enabled,
                    checked: state.is_some_and(|s| s.checked),
                    act: Act::Action(action.boxed_clone()),
                });
            }
            OwnedMenuItem::Submenu(menu) if menu.name.as_ref() == FILTER => {
                let rows = library_menu::filter_entries(app)
                    .into_iter()
                    .map(|e| match e.command {
                        None => Row::rule(),
                        Some(command) => Row { label: e.label.to_string(), hint: None, enabled: e.enabled, checked: e.checked, act: Act::Command(command) },
                    })
                    .collect();
                out.push(Row { label: menu.name.to_string(), hint: None, enabled: app.model.shelf.library.is_some(), checked: false, act: Act::Sub(rows) });
            }
            OwnedMenuItem::Submenu(menu) => {
                let rows = rows_of(app, &menu.items, window, cx);
                out.push(Row { label: menu.name.to_string(), hint: None, enabled: true, checked: false, act: Act::Sub(rows) });
            }
            OwnedMenuItem::SystemMenu(_) => {}
        }
    }
    out
}

fn menus(app: &NegsApp, window: &Window, cx: &mut App) -> Vec<(String, Vec<Row>)> {
    let all = cx.get_menus().unwrap_or_default();
    all.iter().map(|m| (m.name.to_string(), rows_of(app, &m.items, window, cx))).collect()
}

pub fn close(app: &mut NegsApp) -> bool {
    app.menu_bar.take().is_some()
}

fn run(app: &mut NegsApp, act: Act, window: &mut Window, cx: &mut Context<NegsApp>) {
    app.menu_bar = None;
    cx.notify();
    window.focus(&app.focus_handle.clone());
    match act {
        Act::Action(action) => window.dispatch_action(action, cx),
        Act::Command(command) => app.run_command(command, None, window, cx),
        Act::Sub(_) | Act::Rule => {}
    }
}

/// The next selectable row from `from` in direction `step`, wrapping.
fn next_row(rows: &[Row], from: Option<usize>, step: isize) -> Option<usize> {
    let n = rows.len() as isize;
    if n == 0 {
        return None;
    }
    let mut i = from.map_or(if step > 0 { -1 } else { n }, |f| f as isize);
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if rows[i as usize].selectable() {
            return Some(i as usize);
        }
    }
    None
}

/// Alt pressed and released on its own: the bar takes focus, or lets go.
pub fn modifiers_changed(app: &mut NegsApp, modifiers: &gpui::Modifiers, window: &mut Window, cx: &mut Context<NegsApp>) {
    if cfg!(target_os = "macos") {
        return;
    }
    let alt_only = modifiers.alt && !modifiers.control && !modifiers.shift && !modifiers.platform;
    if alt_only && !app.alt.down {
        app.alt = AltState { down: true, armed: true };
        cx.notify();
    } else if !modifiers.alt && app.alt.down {
        let armed = app.alt.armed;
        app.alt = AltState::default();
        if armed {
            toggle_focus(app, window, cx);
        }
        cx.notify();
    } else if !alt_only {
        app.alt.armed = false;
    }
}

fn toggle_focus(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) {
    if app.menu_bar.take().is_none() && app.export_card.is_none() {
        app.menu_bar = Some(Open { keys: true, ..Open::default() });
        window.focus(&app.focus_handle.clone());
    }
    cx.notify();
}

/// The bar's keys; true if the key was the bar's.
pub fn key(app: &mut NegsApp, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<NegsApp>) -> bool {
    if cfg!(target_os = "macos") {
        return false;
    }
    app.alt.armed = false;
    let key = event.keystroke.key.as_str();
    let m = &event.keystroke.modifiers;
    let Some(mut open) = app.menu_bar else {
        // Closed: F10 focuses the bar, Alt+letter opens a menu.
        if key == "f10" && !m.control && !m.shift && app.export_card.is_none() {
            toggle_focus(app, window, cx);
            return true;
        }
        if m.alt && !m.control && !m.platform && app.export_card.is_none() {
            let all = menus(app, window, cx);
            if let Some(i) = all.iter().position(|(name, _)| access_key(name).is_some_and(|k| key.len() == 1 && key.starts_with(k))) {
                let hot = next_row(&all[i].1, None, 1);
                app.menu_bar = Some(Open { menu: i, dropped: true, hot, keys: true, ..Open::default() });
                window.focus(&app.focus_handle.clone());
                cx.notify();
                return true;
            }
        }
        return false;
    };
    let mut all = menus(app, window, cx);
    if all.is_empty() {
        return false;
    }
    let count = all.len();
    open.menu = open.menu.min(count - 1);
    open.keys = true;
    let rows = &all[open.menu].1;
    let sub_rows: Option<&Vec<Row>> = open.sub.and_then(|s| match &rows.get(s)?.act {
        Act::Sub(r) => Some(r),
        _ => None,
    });
    let switch = |open: &mut Open, step: isize, all: &[(String, Vec<Row>)]| {
        open.menu = (open.menu as isize + step).rem_euclid(count as isize) as usize;
        open.sub = None;
        open.sub_hot = None;
        open.hot = if open.dropped { next_row(&all[open.menu].1, None, 1) } else { None };
    };
    match key {
        "escape" => {
            if open.sub.is_some() {
                open.sub = None;
                open.sub_hot = None;
            } else if open.dropped && open.keys {
                open.dropped = false;
                open.hot = None;
            } else {
                app.menu_bar = None;
                cx.notify();
                return true;
            }
        }
        "f10" => {
            app.menu_bar = None;
            cx.notify();
            return true;
        }
        "left" => {
            if open.sub.is_some() {
                open.sub = None;
                open.sub_hot = None;
            } else {
                switch(&mut open, -1, &all);
            }
        }
        "right" => {
            let into = open.dropped && open.sub.is_none() && open.hot.is_some_and(|h| matches!(rows[h].act, Act::Sub(_)) && rows[h].enabled);
            if into {
                open.sub = open.hot;
                open.sub_hot = sub_rows_of(rows, open.hot).and_then(|r| next_row(r, None, 1));
            } else {
                switch(&mut open, 1, &all);
            }
        }
        "down" | "up" => {
            let step = if key == "down" { 1 } else { -1 };
            if !open.dropped {
                open.dropped = true;
                open.hot = next_row(rows, None, step);
            } else if let Some(sub) = sub_rows {
                open.sub_hot = next_row(sub, open.sub_hot, step);
            } else {
                open.hot = next_row(rows, open.hot, step);
            }
        }
        "enter" | "space" => {
            if !open.dropped {
                open.dropped = true;
                open.hot = next_row(rows, None, 1);
            } else {
                let (list, at) = match sub_rows {
                    Some(sub) => (sub, open.sub_hot),
                    None => (rows, open.hot),
                };
                if let Some(i) = at.filter(|&i| list[i].enabled) {
                    if open.sub.is_none() && matches!(list[i].act, Act::Sub(_)) {
                        open.sub = Some(i);
                        open.sub_hot = sub_rows_of(rows, Some(i)).and_then(|r| next_row(r, None, 1));
                    } else {
                        let act = take_act(&mut all, open.menu, open.sub, i);
                        run(app, act, window, cx);
                        return true;
                    }
                }
            }
        }
        k if k.chars().count() == 1 && !m.control && !m.platform => {
            let letter = k.chars().next().unwrap().to_ascii_lowercase();
            if !open.dropped {
                if let Some(i) = all.iter().position(|(name, _)| access_key(name) == Some(letter)) {
                    open.menu = i;
                    open.dropped = true;
                    open.hot = next_row(&all[i].1, None, 1);
                }
            } else {
                let list = sub_rows.unwrap_or(rows);
                let hit = list.iter().position(|r| r.selectable() && r.enabled && access_key(&r.label) == Some(letter));
                if let Some(i) = hit {
                    if open.sub.is_none() && matches!(list[i].act, Act::Sub(_)) {
                        open.hot = Some(i);
                        open.sub = Some(i);
                        open.sub_hot = sub_rows_of(rows, Some(i)).and_then(|r| next_row(r, None, 1));
                    } else {
                        let act = take_act(&mut all, open.menu, open.sub, i);
                        run(app, act, window, cx);
                        return true;
                    }
                }
            }
        }
        // Tab and the rest: nothing else happens while the bar has focus.
        _ => {}
    }
    app.menu_bar = Some(open);
    cx.notify();
    true
}

fn sub_rows_of(rows: &[Row], at: Option<usize>) -> Option<&Vec<Row>> {
    match &rows.get(at?)?.act {
        Act::Sub(r) => Some(r),
        _ => None,
    }
}

fn take_act(all: &mut [(String, Vec<Row>)], menu: usize, sub: Option<usize>, i: usize) -> Act {
    let rows = &mut all[menu].1;
    let list = match sub {
        Some(s) => match &mut rows[s].act {
            Act::Sub(r) => r,
            _ => return Act::Rule,
        },
        None => rows,
    };
    std::mem::replace(&mut list[i].act, Act::Rule)
}

/// Words with the access key underlined when `cues`.
fn words(label: &str, cues: bool) -> StyledText {
    let (text, range) = plain(label);
    let styled = StyledText::new(text);
    match range.filter(|_| cues) {
        Some(r) => styled.with_highlights([(r, HighlightStyle { underline: Some(UnderlineStyle { thickness: px(1.0), color: None, wavy: false }), ..HighlightStyle::default() })]),
        None => styled,
    }
}

/// The menus' words, for the title bar; nothing on the Mac.
pub fn render_words(app: &NegsApp, window: &Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let all = menus(app, window, cx);
    if all.is_empty() {
        return None;
    }
    let theme = &app.theme;
    let cues = app.alt.down || app.menu_bar.is_some_and(|o| o.keys);
    let mut row = div().flex().flex_none().items_center().gap(px(2.0)).ml(px(6.0));
    for (i, (name, rows)) in all.into_iter().enumerate() {
        let open = app.menu_bar.filter(|o| o.menu == i);
        let lit = open.is_some();
        let mut word = div()
            .id(("menu-bar", i))
            .relative()
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(4.0))
            .text_size(px(12.5))
            .text_color(if lit { theme.paper } else { theme.label })
            .when(lit, |d| d.bg(theme.subtle_active))
            .hover(|s| s.bg(theme.subtle_hover))
            .cursor_pointer()
            .child(words(&name, cues))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _w, cx| {
                this.menu_bar = if this.menu_bar.is_some_and(|o| o.menu == i && o.dropped) { None } else { Some(Open::mouse(i)) };
                cx.stop_propagation();
                cx.notify();
            }))
            // With a menu open, the pointer moving onto another word opens it.
            .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                if *hovered && this.menu_bar.is_some_and(|o| o.menu != i && o.dropped) {
                    let keys = this.menu_bar.is_some_and(|o| o.keys);
                    this.menu_bar = Some(Open { keys, ..Open::mouse(i) });
                    cx.notify();
                }
            }));
        if let Some(open) = open.filter(|o| o.dropped) {
            word = word.child(deferred(render_rows(app, rows, open, None, cues, cx)).with_priority(2));
        }
        row = row.child(word);
    }
    Some(row.into_any_element())
}

/// A menu's rows under its word (`parent` None), or a submenu beside its row.
fn render_rows(app: &NegsApp, rows: Vec<Row>, open: Open, parent: Option<usize>, cues: bool, cx: &mut Context<NegsApp>) -> AnyElement {
    let theme = &app.theme;
    let top = parent.is_none();
    let hot = if top { open.hot } else { open.sub_hot };
    let mut list = div()
        .id(if top { "menu-bar-items" } else { "menu-bar-sub" })
        .absolute()
        .when(top, |d| d.top(px(26.0)).left(px(0.0)))
        .when(!top, |d| d.top(px(-5.0)).left(px(244.0)))
        .w(px(250.0))
        .py(px(4.0))
        .flex()
        .flex_col()
        .bg(theme.panel)
        .border_1()
        .border_color(theme.line)
        .rounded(px(6.0))
        .shadow_lg()
        .occlude()
        .text_size(px(12.5));
    for (j, row) in rows.into_iter().enumerate() {
        let lit = hot == Some(j);
        let Row { label, hint, enabled, checked, act } = row;
        let el: AnyElement = match act {
            Act::Rule => div().my(px(4.0)).h(px(1.0)).bg(theme.line).into_any_element(),
            Act::Sub(children) => {
                let shown = top && open.sub == Some(j);
                div()
                    .id(("menu-bar-submenu", j))
                    .relative()
                    .flex()
                    .items_center()
                    .pl(px(26.0))
                    .pr(px(12.0))
                    .py(px(4.0))
                    .text_color(if enabled { theme.label } else { theme.dim })
                    .cursor_pointer()
                    .when(shown || lit, |d| d.bg(theme.subtle_hover))
                    .hover(|s| s.bg(theme.subtle_hover))
                    .child(div().flex_1().child(words(&label, cues)))
                    .child(div().text_color(theme.dim).child("›"))
                    .when(enabled, |d| {
                        d.on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                            if *hovered && let Some(o) = this.menu_bar.as_mut() {
                                o.hot = Some(j);
                                o.sub = Some(j);
                                o.sub_hot = None;
                                cx.notify();
                            }
                        }))
                    })
                    .when(shown, |d| d.child(render_rows(app, children, open, Some(j), cues, cx)))
                    .into_any_element()
            }
            act => {
                let act = std::rc::Rc::new(std::cell::RefCell::new(Some(act)));
                div()
                    .id(("menu-bar-item", j))
                    .flex()
                    .items_center()
                    .pl(px(8.0))
                    .pr(px(12.0))
                    .py(px(4.0))
                    .text_color(if enabled { theme.label } else { theme.dim })
                    .when(lit, |d| d.bg(theme.subtle_hover))
                    .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme.subtle_hover)))
                    // The tick, in its own column so the words line up.
                    .child(div().w(px(18.0)).flex_none().font_weight(FontWeight::BOLD).when(checked, |d| d.child("✓")))
                    .child(div().flex_1().child(words(&label, cues)))
                    .when_some(hint, |d, k| d.child(div().text_color(theme.dim).child(k)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                        if *hovered && let Some(o) = this.menu_bar.as_mut() {
                            if top {
                                o.hot = Some(j);
                                o.sub = None;
                                o.sub_hot = None;
                            } else {
                                o.sub_hot = Some(j);
                            }
                            cx.notify();
                        }
                    }))
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |this, _, window, cx| {
                            if let Some(act) = act.borrow_mut().take() {
                                run(this, act, window, cx);
                            }
                        }))
                    })
                    .into_any_element()
            }
        };
        list = list.child(el);
    }
    list.into_any_element()
}

/// Behind an open menu: a click anywhere under the title bar closes it.
pub fn render_scrim(app: &NegsApp, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    app.menu_bar.filter(|o| o.dropped)?;
    Some(
        div()
            .id("menu-bar-scrim")
            .absolute()
            // Below the title bar: there a click on a word opens or closes it.
            .top(px(crate::titlebar::TITLEBAR_HEIGHT))
            .left_0()
            .size_full()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _w, cx| {
                close(this);
                cx.notify();
            }))
            .on_mouse_down(gpui::MouseButton::Right, cx.listener(|this, _, _w, cx| {
                close(this);
                cx.notify();
            }))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_read_as_windows_writes_them() {
        assert_eq!(hint("ctrl-shift-e"), "Ctrl+Shift+E");
        assert_eq!(hint("ctrl--"), "Ctrl+-");
        assert_eq!(hint("ctrl-="), "Ctrl+=");
        assert_eq!(hint("ctrl-,"), "Ctrl+,");
        assert_eq!(hint("f7"), "F7");
        assert_eq!(hint("]"), "]");
        assert_eq!(hint("p"), "P");
        assert_eq!(hint("shift-delete"), "Shift+Del");
    }

    #[test]
    fn access_keys_follow_the_ampersand_or_the_first_letter() {
        assert_eq!(access_key("&File"), Some('f'));
        assert_eq!(access_key("Fi&t Photograph"), Some('t'));
        assert_eq!(access_key("Rock && Roll"), Some('r'));
        assert_eq!(access_key("★★★"), None);
        assert_eq!(plain("Fi&t Photograph"), ("Fit Photograph".to_string(), Some(2..3)));
        assert_eq!(plain("Rock && Roll"), ("Rock & Roll".to_string(), None));
        assert_eq!(plain("Show A&ll Controls").0, "Show All Controls");
    }

    #[test]
    fn arrows_skip_rules_and_wrap() {
        let row = |a: Act| Row { label: "x".into(), hint: None, enabled: true, checked: false, act: a };
        let rows = vec![row(Act::Command(Command::Pick)), Row::rule(), row(Act::Command(Command::Reject))];
        assert_eq!(next_row(&rows, None, 1), Some(0));
        assert_eq!(next_row(&rows, Some(0), 1), Some(2));
        assert_eq!(next_row(&rows, Some(2), 1), Some(0));
        assert_eq!(next_row(&rows, None, -1), Some(2));
        assert_eq!(next_row(&[Row::rule()], None, 1), None);
    }
}
