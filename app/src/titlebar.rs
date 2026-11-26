//! Quiet darkroom title band (`L6`–`L11`): `38 px` tall in `bg`, showing the
//! inline menu bar off the Mac, the middle-truncated filename in Newsreader
//! 15 pt 600, bare italic Newsreader 17 pt `Grid · Photo · Compare` view
//! switch with grease-pencil underline or Light Table highlighter, and custom
//! vector window caption buttons off the Mac.

use gpui::{
    AnyElement, Bounds, Context, Font, FontWeight, InteractiveElement, IntoElement, MouseButton,
    PathBuilder, Pixels, SharedString, Styled, TextRun, Window, WindowControlArea, canvas, div,
    fill, point, prelude::*, px, size,
};
use negs_app::theme::DISABLED_OPACITY;

use crate::NegsApp;
use crate::marks_gpui::{paint_highlighter, paint_pencil};
use crate::theme_gpui::{GpuiTheme, hex, scale};

/// Height of the title band (`L7`, matching SwiftUI `Theme.Size.titleBar` and
/// Win32 `titlebar::HEIGHT`).
pub const TITLEBAR_HEIGHT: f32 = 38.0;

/// Width of each Windows/Linux caption button (`46 × 38 DIP`, Win32 `titlebar.rs:43`).
pub const CAPTION_BUTTON_WIDTH: f32 = 46.0;

/// Left inset before the inline menu bar off the Mac (`Win32 titlebar.rs:45`).
const MENU_INSET: f32 = 6.0;

/// Gap between the menu bar (or macOS traffic lights) and the title (`14 pt`).
const NAME_GAP: f32 = 14.0;

/// Left inset on macOS in windowed mode (matches SwiftUI's `88 pt` = traffic
/// lights ending at `74 pt` + `14 pt` gap).
const MAC_TRAFFIC_LIGHT_CLEARANCE: f32 = 88.0;

/// Left inset on macOS in fullscreen mode (`Theme.Size.gutter`).
const MAC_FULLSCREEN_INSET: f32 = 22.0;

/// Resolves the window's title text (`L8`), matching SwiftUI
/// `EditorWindowController.swift:395-404` and Win32 `mod.rs:693-712`.
pub fn title_text(app: &NegsApp) -> String {
    if let Some(compare) = app.model.compare_title() {
        return compare;
    }
    if app.model.shelf.in_grid {
        if let Some(item) = app.model.shelf.selection.current() {
            return app
                .model
                .shelf
                .library
                .as_ref()
                .map_or_else(|| item.file_name(), |lib| lib.name(item));
        }
    }
    if let Some(name) = app
        .model
        .source
        .as_deref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
    {
        return name;
    }
    "Negs".to_string()
}

/// Truncates `text` in the middle to keep `keep` characters split evenly
/// between head and tail, joined by `…`.
pub fn middle_cut(text: &str, keep: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if keep >= chars.len() {
        return text.to_string();
    }
    if keep == 0 {
        return String::new();
    }
    let head = keep.div_ceil(2);
    let tail = keep / 2;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// Fits `title` into `room` DIPs using middle truncation, matching Win32
/// `titlebar::fitted` and macOS `byTruncatingMiddle` (`L8`).
fn fitted_middle(title: &str, font: &Font, font_size: f32, room: f32, window: &Window) -> String {
    if room <= 0.0 || title.is_empty() {
        return String::new();
    }
    let measure = |text: &str| -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let run = TextRun {
            len: text.len(),
            font: font.clone(),
            color: gpui::black(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped = window.text_system().shape_line(
            SharedString::from(text.to_string()),
            px(font_size),
            &[run],
            None,
        );
        f32::from(shaped.width)
    };
    if measure(title) <= room {
        return title.to_string();
    }
    let total = title.chars().count();
    let (mut low, mut high) = (0usize, total);
    while low < high {
        let mid = (low + high).div_ceil(2);
        if measure(&middle_cut(title, mid)) <= room {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    middle_cut(title, low)
}

/// Computes available horizontal DIP room for the title string.
pub fn title_room(
    viewport_w: f32,
    is_mac: bool,
    fullscreen: bool,
    has_photo: bool,
    menu_bar_width: f32,
) -> f32 {
    let left_used = if is_mac {
        if fullscreen {
            MAC_FULLSCREEN_INSET
        } else {
            MAC_TRAFFIC_LIGHT_CLEARANCE
        }
    } else {
        MENU_INSET + menu_bar_width + NAME_GAP
    };
    let view_switch_w = if has_photo { 196.0 + 18.0 } else { 0.0 };
    let captions_w = if !is_mac && !fullscreen {
        3.0 * CAPTION_BUTTON_WIDTH
    } else {
        0.0
    };
    let right_gap = 16.0;
    (viewport_w - left_used - view_switch_w - captions_w - right_gap).max(0.0)
}

fn view_switch_word(
    id: &'static str,
    label: &'static str,
    selected: bool,
    disabled: bool,
    tooltip: String,
    theme: &GpuiTheme,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let (vs_font, vs_size) = theme.serif_spec(scale::VIEW_SWITCH, 400.0, true);
    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let accent = theme.accent;
    let highlighter_ink = theme.highlighter_ink;
    let label_color = theme.label;
    let text_color = if selected { theme.paper } else { theme.dim };

    div()
        .id(id)
        .relative()
        .flex()
        .items_center()
        .justify_center()
        .h(px(28.0))
        .pb(px(5.0))
        .text_color(text_color)
        .when(disabled, |d| d.opacity(DISABLED_OPACITY))
        .when(!disabled && !selected, |d| {
            d.hover(move |s| s.text_color(label_color))
        })
        .when(!disabled, |d| d.active(|s| s.opacity(0.62)))
        .when(selected && highlighter, |d| {
            d.child(
                canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                        let mark_rect = Bounds {
                            origin: point(bounds.origin.x, bounds.origin.y + bounds.size.height - px(6.0)),
                            size: size(bounds.size.width, px(6.0)),
                        };
                        paint_highlighter(window, mark_rect, plain, 5.0, highlighter_ink);
                    },
                )
                .absolute()
                .inset_0(),
            )
        })
        .child(
            div()
                .relative()
                .font(vs_font)
                .text_size(px(vs_size))
                .line_height(px(22.0))
                .child(label),
        )
        .when(selected && !highlighter, |d| {
            d.child(
                canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                        paint_pencil(
                            window,
                            bounds,
                            plain,
                            negs_marks::PENCIL_WIDTH as f32,
                            accent,
                        );
                    },
                )
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(0.5))
                .h(px(5.0)),
            )
        })
        .when(!disabled, |d| d.cursor_pointer().on_click(on_click))
        .tooltip(crate::tooltip::tip(tooltip, theme))
        .into_any_element()
}

pub fn render_titlebar(
    app: &NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let theme = &app.theme;
    let active = window.is_window_active();
    let is_mac = cfg!(target_os = "macos");
    let fullscreen = window.is_fullscreen();
    let has_photo = app.model.source.is_some();

    let in_grid = app.model.shelf.in_grid;
    let can_grid = app.model.shelf.library.is_some() && has_photo && !app.model.cropping();
    let comparing = app.model.comparison().is_some();
    let can_compare = comparing || app.model.compare_word().0;
    let in_grid = in_grid && !comparing;

    let left_pad = if is_mac {
        if fullscreen {
            MAC_FULLSCREEN_INSET
        } else {
            MAC_TRAFFIC_LIGHT_CLEARANCE
        }
    } else {
        MENU_INSET
    };

    let (title_font, title_size) = theme.serif_spec(scale::TITLE_NAME, 600.0, false);
    let raw_title = title_text(app);
    // Approximate menu bar width off-Mac: 5 words (`File Edit Photo View Help`).
    let menu_bar_approx = if is_mac { 0.0 } else { 232.0 };
    let room = title_room(
        f32::from(window.viewport_size().width),
        is_mac,
        fullscreen,
        has_photo,
        menu_bar_approx,
    );
    let shown_title = fitted_middle(&raw_title, &title_font, title_size, room, window);
    let title_color = if active { theme.paper } else { theme.dim };

    div()
        .id("titlebar")
        .relative()
        .flex()
        .items_center()
        .w_full()
        .h(px(TITLEBAR_HEIGHT))
        .pl(px(left_pad))
        .bg(theme.canvas)
        .font(theme.ui_font(FontWeight::NORMAL))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _w, cx| {
                if this.menu_bar.is_some() {
                    crate::menu_bar::close(this);
                    cx.notify();
                }
            }),
        )
        // Left: off-Mac inline menu bar (directly at x=6) + filename
        .children(crate::menu_bar::render_words(app, window, cx))
        .child(
            div()
                .flex()
                .items_center()
                .h_full()
                .when(!is_mac, |d| d.pl(px(NAME_GAP)))
                .window_control_area(WindowControlArea::Drag)
                .child(
                    div()
                        .font(title_font)
                        .text_size(px(title_size))
                        .text_color(title_color)
                        .whitespace_nowrap()
                        .child(shown_title),
                ),
        )
        // Flexible drag region between title and ViewSwitch.
        .child(
            div()
                .flex_1()
                .min_w(px(16.0))
                .h_full()
                .window_control_area(WindowControlArea::Drag),
        )
        // Right: bare italic Newsreader `Grid · Photo · Compare` (only when a photo is open, `L10`).
        .when(has_photo, |d| {
            let grid_tip = if comparing {
                "Stop comparing and show all photographs (G)".to_string()
            } else {
                "All Photographs (G)".to_string()
            };
            let photo_tip = if comparing {
                "Stop comparing and edit the photograph".to_string()
            } else {
                "Edit Photograph (E)".to_string()
            };
            let compare_tip = if comparing {
                "Comparing two photographs (C to leave)".to_string()
            } else {
                app.model.compare_word().1
            };
            d.child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(16.0))
                    .pr(px(if is_mac { 22.0 } else { 18.0 }))
                    .h_full()
                    .child(view_switch_word(
                        "tb-grid",
                        "Grid",
                        in_grid,
                        !can_grid,
                        grid_tip,
                        theme,
                        cx.listener(|this, _, window, cx| {
                            if this.model.comparison().is_some() {
                                this.model.leave_compare();
                                crate::grid::open(this);
                                this.after_model_update(cx);
                                cx.notify();
                            } else if !this.model.shelf.in_grid {
                                this.toggle_grid(window, cx);
                            }
                        }),
                    ))
                    .child(view_switch_word(
                        "tb-photo",
                        "Photo",
                        !in_grid && !comparing,
                        false,
                        photo_tip,
                        theme,
                        cx.listener(|this, _, window, cx| {
                            if this.model.comparison().is_some() {
                                this.model.leave_compare();
                                this.after_model_update(cx);
                                cx.notify();
                            } else if this.grid.shown && this.model.shelf.in_grid {
                                this.toggle_grid(window, cx);
                            }
                        }),
                    ))
                    .child(view_switch_word(
                        "tb-compare",
                        "Compare",
                        comparing,
                        !can_compare,
                        compare_tip,
                        theme,
                        cx.listener(|this, _, _window, cx| {
                            if this.model.comparison().is_none() {
                                crate::compare_view::toggle(this);
                                this.after_model_update(cx);
                                cx.notify();
                            }
                        }),
                    )),
            )
        })
        .children(caption_buttons(theme, window, false))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptionKind {
    Min,
    Max,
    Close,
}

fn paint_caption_glyph(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    kind: CaptionKind,
    maximized: bool,
    color: gpui::Hsla,
) {
    let s = window.scale_factor();
    let snap = |v: f32| ((v * s).round() + 0.5) / s;
    let cx = f32::from(bounds.origin.x) + f32::from(bounds.size.width) * 0.5;
    let cy = f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5;
    let stroke = (s.round().max(1.0)) / s;

    let stroke_line = |window: &mut Window, x0: f32, y0: f32, x1: f32, y1: f32| {
        let mut b = PathBuilder::stroke(px(stroke));
        b.move_to(point(px(x0), px(y0)));
        b.line_to(point(px(x1), px(y1)));
        if let Ok(path) = b.build() {
            window.paint_path(path, color);
        }
    };

    let stroke_rect = |window: &mut Window, x: f32, y: f32, w: f32, h: f32| {
        let mut b = PathBuilder::stroke(px(stroke));
        b.move_to(point(px(x), px(y)));
        b.line_to(point(px(x + w), px(y)));
        b.line_to(point(px(x + w), px(y + h)));
        b.line_to(point(px(x), px(y + h)));
        b.close();
        if let Ok(path) = b.build() {
            window.paint_path(path, color);
        }
    };

    match kind {
        CaptionKind::Min => {
            let y = snap(cy) - stroke * 0.5;
            window.paint_quad(fill(
                Bounds {
                    origin: point(px(cx - 5.0), px(y)),
                    size: size(px(10.0), px(stroke)),
                },
                color,
            ));
        }
        CaptionKind::Max if maximized => {
            let (x, y) = (snap(cx - 5.0), snap(cy - 3.0));
            stroke_rect(window, x, y, 8.0, 8.0);
            stroke_line(window, x + 2.0, y - 2.0, x + 10.0, y - 2.0);
            stroke_line(window, x + 10.0, y - 2.0, x + 10.0, y + 6.0);
        }
        CaptionKind::Max => {
            stroke_rect(window, snap(cx - 5.0), snap(cy - 5.0), 10.0, 10.0);
        }
        CaptionKind::Close => {
            stroke_line(window, cx - 5.0, cy - 5.0, cx + 5.0, cy + 5.0);
            stroke_line(window, cx + 5.0, cy - 5.0, cx - 5.0, cy + 5.0);
        }
    }
}

/// Custom-drawn vector Minimise, Maximise, and Close caption buttons off the
/// Mac (`46 × 38 DIP`, `L11`, matching Win32 `windows/src/ui/titlebar.rs:440-479`).
/// Hidden in fullscreen. Settings passes `close_only: true`.
pub fn caption_buttons(
    theme: &GpuiTheme,
    window: &Window,
    close_only: bool,
) -> Option<AnyElement> {
    if cfg!(target_os = "macos") || window.is_fullscreen() {
        return None;
    }
    let active = window.is_window_active();
    let maximised = window.is_maximized();
    let idle_ink = if active { theme.label } else { theme.dim };
    let paper = theme.paper;
    let hover_bg = paper.opacity(0.08);
    let press_bg = paper.opacity(0.14);

    let button = |id: &'static str, area: WindowControlArea, kind: CaptionKind| {
        let close = kind == CaptionKind::Close;
        div()
            .id(id)
            .w(px(CAPTION_BUTTON_WIDTH))
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .window_control_area(area)
            .when(close, |d| {
                d.hover(|s| s.bg(hex(0xc42b1c)))
                    .active(|s| s.bg(hex(0xc83c30)))
            })
            .when(!close, |d| {
                d.hover(move |s| s.bg(hover_bg))
                    .active(move |s| s.bg(press_bg))
            })
            .child(
                canvas(
                    move |_bounds, _window, _cx| {},
                    move |bounds: Bounds<Pixels>, (), window, _cx| {
                        let hot = bounds.contains(&window.mouse_position());
                        let glyph = if hot {
                            if close { gpui::white() } else { paper }
                        } else {
                            idle_ink
                        };
                        paint_caption_glyph(window, bounds, kind, maximised, glyph);
                    },
                )
                .size_full(),
            )
    };

    let mut row = div().flex().flex_none().h_full();
    if !close_only {
        row = row
            .child(button("caption-min", WindowControlArea::Min, CaptionKind::Min))
            .child(button("caption-max", WindowControlArea::Max, CaptionKind::Max));
    }
    Some(
        row.child(button("caption-close", WindowControlArea::Close, CaptionKind::Close))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn middle_cut_keeps_head_and_tail_around_ellipsis() {
        assert_eq!(middle_cut("DSC_0262.NEF", 12), "DSC_0262.NEF");
        assert_eq!(middle_cut("DSC_0262.NEF", 8), "DSC_….NEF");
        assert_eq!(middle_cut("DSC_0262.NEF", 7), "DSC_…NEF");
        assert_eq!(middle_cut("DSC_0262.NEF", 1), "D…");
        assert_eq!(middle_cut("DSC_0262.NEF", 0), "");
        // UTF-8 safety
        assert_eq!(middle_cut("Ångström_2026.NEF", 6), "Ång…NEF");
    }

    #[test]
    fn title_room_accounts_for_platform_chrome_and_view_switch() {
        let mac_room = title_room(1280.0, true, false, true, 0.0);
        let mac_empty = title_room(1280.0, true, false, false, 0.0);
        assert!(mac_empty > mac_room + 180.0);

        let win_room = title_room(1280.0, false, false, true, 232.0);
        let win_full = title_room(1280.0, false, true, true, 232.0);
        assert!((win_full - win_room - 3.0 * CAPTION_BUTTON_WIDTH).abs() < 1e-3);
    }
}
