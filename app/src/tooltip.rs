//! Tooltips, as the Windows app's (`dialogs::tooltip`) and the Mac app's
//! `.help`: a control's help after half a second's hover, in the theme's
//! panel and ink. None while a slider or the canvas is being dragged: GPUI
//! shows a tooltip wherever the pointer rests, button held or not.

use std::cell::Cell;

use gpui::{AnyView, App, AppContext, Context, Hsla, IntoElement, ParentElement, Render, SharedString, Styled, Window, div, prelude::FluentBuilder, px};

use crate::theme_gpui::GpuiTheme;

thread_local! {
    static QUIET: Cell<bool> = const { Cell::new(false) };
}

/// Set each render: whether a drag is under way.
pub fn set_quiet(quiet: bool) {
    QUIET.with(|q| q.set(quiet));
}

pub struct Tip {
    text: SharedString,
    ground: Hsla,
    line: Hsla,
    ink: Hsla,
    family: SharedString,
}

impl Render for Tip {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().when(!self.text.is_empty(), |d| {
            d.max_w(px(320.0))
                .px(px(8.0))
                .py(px(5.0))
                .rounded(px(4.0))
                .bg(self.ground)
                .border_1()
                .border_color(self.line)
                .shadow_sm()
                .font_family(self.family.clone())
                .text_size(px(12.0))
                .line_height(px(16.0))
                .text_color(self.ink)
                .child(self.text.clone())
        })
    }
}

/// A builder for `.tooltip(…)` showing `text` in `theme`'s colours.
pub fn tip(text: impl Into<SharedString>, theme: &GpuiTheme) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    let (ground, line, ink, family) = (theme.panel, theme.line, theme.label, theme.ui_family.clone());
    move |_window, cx| {
        let text = if QUIET.with(|q| q.get()) { SharedString::default() } else { text.clone() };
        cx.new(|_| Tip { text, ground, line, ink, family: family.clone() }).into()
    }
}
