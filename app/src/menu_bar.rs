//! The menu bar in the title bar, off the Mac: GPUI keeps the menus it's
//! given on Windows and Linux but draws none, so their words sit after
//! "Negs" as the Windows app's do (`ui/titlebar.rs`), and each opens its
//! menu under it. The items are `cx.get_menus()`'s, the same as the Mac's
//! menu bar, and choosing one dispatches its action.

use gpui::{Action, AnyElement, Context, InteractiveElement, IntoElement, OwnedMenuItem, ParentElement, StatefulInteractiveElement, Styled, Window, deferred, div, prelude::FluentBuilder, px};

use crate::NegsApp;

/// Which menu is open, and which of its submenus.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Open {
    pub menu: usize,
    pub sub: Option<usize>,
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

pub fn close(app: &mut NegsApp) -> bool {
    app.menu_bar.take().is_some()
}

fn choose(app: &mut NegsApp, action: Box<dyn Action>, window: &mut Window, cx: &mut Context<NegsApp>) {
    app.menu_bar = None;
    cx.notify();
    window.focus(&app.focus_handle);
    window.dispatch_action(action, cx);
}

/// The menus' words, for the title bar; nothing on the Mac.
pub fn render_words(app: &NegsApp, window: &Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if cfg!(target_os = "macos") {
        return None;
    }
    let menus = cx.get_menus()?;
    let theme = &app.theme;
    let mut row = div().flex().flex_none().items_center().gap(px(2.0)).ml(px(6.0));
    for (i, menu) in menus.iter().enumerate() {
        let open = app.menu_bar.filter(|o| o.menu == i);
        let mut word = div()
            .id(("menu-bar", i))
            .relative()
            .px(px(8.0))
            .py(px(3.0))
            .rounded(px(4.0))
            .text_size(px(12.5))
            .text_color(if open.is_some() { theme.paper } else { theme.label })
            .when(open.is_some(), |d| d.bg(theme.subtle_active))
            .hover(|s| s.bg(theme.subtle_hover))
            .cursor_pointer()
            .child(menu.name.clone())
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(move |this, _, _w, cx| {
                this.menu_bar = if this.menu_bar.is_some_and(|o| o.menu == i) { None } else { Some(Open { menu: i, sub: None }) };
                cx.stop_propagation();
                cx.notify();
            }))
            // With a menu open, the pointer moving onto another word opens it.
            .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                if *hovered && this.menu_bar.is_some_and(|o| o.menu != i) {
                    this.menu_bar = Some(Open { menu: i, sub: None });
                    cx.notify();
                }
            }));
        if let Some(open) = open {
            word = word.child(deferred(render_items(app, &menu.items, open.sub, true, window, cx)).with_priority(2));
        }
        row = row.child(word);
    }
    Some(row.into_any_element())
}

/// A menu's items under its word, or a submenu beside its row.
fn render_items(app: &NegsApp, items: &[OwnedMenuItem], sub: Option<usize>, top: bool, window: &Window, cx: &mut Context<NegsApp>) -> AnyElement {
    let theme = &app.theme;
    let mut list = div()
        .id(if top { "menu-bar-items" } else { "menu-bar-sub" })
        .absolute()
        .when(top, |d| d.top(px(26.0)).left(px(0.0)))
        .when(!top, |d| d.top(px(-5.0)).left(px(224.0)))
        .w(px(230.0))
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
    for (j, item) in items.iter().enumerate() {
        let el: AnyElement = match item {
            OwnedMenuItem::Separator => div().my(px(4.0)).h(px(1.0)).bg(theme.line).into_any_element(),
            OwnedMenuItem::Action { name, action, .. } => {
                let enabled = window.is_action_available(action.as_ref(), cx);
                let key = key_hint(action.as_ref(), window);
                let action = action.boxed_clone();
                div()
                    .id(("menu-bar-item", j))
                    .flex()
                    .justify_between()
                    .px(px(12.0))
                    .py(px(4.0))
                    .text_color(if enabled { theme.label } else { theme.dim })
                    .when(enabled, |d| d.cursor_pointer().hover(|s| s.bg(theme.subtle_hover)))
                    .child(name.clone())
                    .when_some(key, |d, k| d.child(div().text_color(theme.dim).child(k)))
                    .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                        if *hovered && top && let Some(open) = this.menu_bar.as_mut().filter(|o| o.sub.is_some()) {
                            open.sub = None;
                            cx.notify();
                        }
                    }))
                    .when(enabled, |d| d.on_click(cx.listener(move |this, _, window, cx| choose(this, action.boxed_clone(), window, cx))))
                    .into_any_element()
            }
            OwnedMenuItem::Submenu(menu) => {
                let open = sub == Some(j);
                div()
                    .id(("menu-bar-submenu", j))
                    .relative()
                    .flex()
                    .justify_between()
                    .px(px(12.0))
                    .py(px(4.0))
                    .text_color(theme.label)
                    .cursor_pointer()
                    .when(open, |d| d.bg(theme.subtle_hover))
                    .hover(|s| s.bg(theme.subtle_hover))
                    .child(menu.name.clone())
                    .child(div().text_color(theme.dim).child("›"))
                    .on_hover(cx.listener(move |this, hovered: &bool, _w, cx| {
                        if *hovered && let Some(o) = this.menu_bar.as_mut() {
                            o.sub = Some(j);
                            cx.notify();
                        }
                    }))
                    .when(open, |d| d.child(render_items(app, &menu.items, None, false, window, cx)))
                    .into_any_element()
            }
            OwnedMenuItem::SystemMenu(_) => continue,
        };
        list = list.child(el);
    }
    list.into_any_element()
}

/// Behind an open menu: a click anywhere under the title bar closes it.
pub fn render_scrim(app: &NegsApp, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    app.menu_bar?;
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
    use super::hint;

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
}
