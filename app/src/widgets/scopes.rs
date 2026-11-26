//! Scopes widget (`ScopesView`, `L16`): Histogram & RGB Waveform with mode
//! switcher, cached BGRA8 `RenderImage` waveform rasterization (VRAM-safe via
//! `drop_image`), and bottom drag-to-resize handle constrained to
//! `negs_app::session::SCOPE_HEIGHTS`.

use std::sync::Arc;

use gpui::{
    Bounds, Context, Corners, FontWeight, Hsla, IntoElement, MouseButton, MouseDownEvent,
    PathBuilder, Pixels, RenderImage, Window, canvas, div, fill, point, prelude::*, px, size,
};
use negs_app::histogram::{Histogram, Waveform};
use negs_app::session::SCOPE_HEIGHTS;

use crate::color_display::bgra_to_render_image;
use crate::theme_gpui::{GpuiTheme, hex_alpha, scale};
use crate::widgets::controls::GUTTER;

/// Fixed height of the `Histogram · Waveform ··· RGB` header row.
pub const SCOPE_HEADER_HEIGHT: f32 = 16.0;
/// Fixed height of the bottom resize grip hit area.
pub const SCOPE_GRIP_HEIGHT: f32 = 8.0;
/// Vertical gap between header, plot, and grip (`6 px` each).
const SCOPE_GAP: f32 = 6.0;
/// Bottom padding under the scopes panel before the tab bar (`10 px`).
const SCOPE_BOTTOM_PAD: f32 = 10.0;

/// Total height of the Scopes section (header + plot + grip + gaps + bottom pad),
/// used to anchor the Tab Edits popover cleanly under the tab bar (`L17`).
pub fn scopes_total_height(scope_height: f64) -> f32 {
    SCOPE_HEADER_HEIGHT + SCOPE_GAP + scope_height as f32 + SCOPE_GAP + SCOPE_GRIP_HEIGHT + SCOPE_BOTTOM_PAD
}

/// Active vertical drag of the bottom scope height handle.
#[derive(Clone, Copy, Debug)]
pub struct ScopeHeightDrag {
    pub start_y: f32,
    pub start_height: f64,
}

impl ScopeHeightDrag {
    pub fn height_for_y(&self, y: f32) -> f64 {
        (self.start_height + (y - self.start_y) as f64)
            .round()
            .clamp(SCOPE_HEIGHTS.0, SCOPE_HEIGHTS.1)
    }
}

/// Caches the GPU `RenderImage` for the RGB waveform and queues replaced images
/// for `window.drop_image(...)`.
#[derive(Default)]
pub struct ScopesCache {
    waveform_key: Option<(u64, usize, usize, u64)>,
    waveform_image: Option<Arc<RenderImage>>,
    pending_drops: Vec<Arc<RenderImage>>,
}

impl ScopesCache {
    pub fn flush_drops(&mut self, window: &mut Window) {
        for img in self.pending_drops.drain(..) {
            let _ = window.drop_image(img);
        }
    }

    /// Returns a cached BGRA8 `RenderImage` for `wf`, rebuilding only when its
    /// contents change.
    pub fn resolve_waveform(
        &mut self,
        wf: &Waveform,
        picture_id: u64,
    ) -> Option<Arc<RenderImage>> {
        if wf.samples == 0 || wf.width == 0 {
            return None;
        }
        let key = (picture_id, wf.width, wf.samples, waveform_fingerprint(wf));
        if self.waveform_key == Some(key) {
            return self.waveform_image.clone();
        }
        let img = waveform_to_render_image(wf);
        if let Some(old) = self.waveform_image.replace(img.clone()) {
            self.pending_drops.push(old);
        }
        self.waveform_key = Some(key);
        Some(img)
    }
}

fn waveform_fingerprint(wf: &Waveform) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let step = (wf.red.len() / 512).max(1);
    for channel in [&wf.red, &wf.green, &wf.blue] {
        for &v in channel.iter().step_by(step) {
            h ^= v as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Converts `Waveform::raster()` (premultiplied RGBA) into a GPUI BGRA8 `RenderImage`.
pub fn waveform_to_render_image(wf: &Waveform) -> Arc<RenderImage> {
    let raster = wf.raster();
    let mut bgra = Vec::with_capacity(raster.len() * 4);
    for [r, g, b, a] in raster {
        bgra.push(b);
        bgra.push(g);
        bgra.push(r);
        bgra.push(a);
    }
    bgra_to_render_image(bgra, wf.width as u32, 256)
}

fn paint_histogram_channel(
    window: &mut Window,
    bins: &[u32; 256],
    peak: f32,
    bounds: Bounds<Pixels>,
    fill_color: Hsla,
    stroke_color: Hsla,
) {
    let x0 = f32::from(bounds.origin.x);
    let y0 = f32::from(bounds.origin.y);
    let w = f32::from(bounds.size.width).max(1.0);
    let h = f32::from(bounds.size.height).max(1.0);
    let bottom = y0 + h;

    let mut fill_path = PathBuilder::fill();
    let mut stroke_path = PathBuilder::stroke(px(1.0));

    fill_path.move_to(point(px(x0), px(bottom)));
    for (i, &count) in bins.iter().enumerate() {
        let fx = i as f32 / 255.0;
        let fy = (count as f32 / peak).min(1.0);
        let pt = point(px(x0 + fx * w), px(bottom - fy * h));
        fill_path.line_to(pt);
        if i == 0 {
            stroke_path.move_to(pt);
        } else {
            stroke_path.line_to(pt);
        }
    }
    fill_path.line_to(point(px(x0 + w), px(bottom)));
    fill_path.close();

    if let Ok(path) = fill_path.build() {
        window.paint_path(path, fill_color);
    }
    if let Ok(path) = stroke_path.build() {
        window.paint_path(path, stroke_color);
    }
}

/// Renders the Scopes header (`Histogram` · `Waveform` + right `RGB`),
/// borderless `bg` plot well, and bottom height resize grip (`L16`).
#[allow(clippy::too_many_arguments)]
pub fn render_scopes<V: 'static>(
    is_waveform: bool,
    scope_height: f64,
    dragging_height: bool,
    has_photo: bool,
    histogram: Option<Histogram>,
    waveform_img: Option<Arc<RenderImage>>,
    theme: &GpuiTheme,
    cx: &mut Context<V>,
    on_select_mode: impl Fn(&mut V, &'static str, &mut Window, &mut Context<V>) + Clone + 'static,
    on_grip_down: impl Fn(&mut V, &MouseDownEvent, &mut Window, &mut Context<V>) + 'static,
) -> gpui::AnyElement {
    let grid_line = theme.line;
    let on_hist = on_select_mode.clone();
    let on_wave = on_select_mode;
    let show_empty = !has_photo || histogram.is_none();

    let plot_element = if is_waveform {
        let wave_canvas = canvas(
            |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, (), window, _cx| {
                for frac in [0.25f32, 0.5, 0.75] {
                    let y = bounds.origin.y + px((f32::from(bounds.size.height) * frac).floor());
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(bounds.origin.x, y),
                            size: size(bounds.size.width, px(1.0)),
                        },
                        grid_line,
                    ));
                }
                if let Some(img) = &waveform_img {
                    let _ = window.paint_image(
                        bounds,
                        Corners::default(),
                        img.clone(),
                        0,
                        false,
                    );
                }
            },
        )
        .flex_1()
        .h_full();

        div()
            .flex()
            .size_full()
            .p(px(6.0))
            .gap(px(4.0))
            .child(
                div()
                    .w(px(22.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .items_end()
                    .font(theme.ui_font(FontWeight::NORMAL))
                    .text_size(px(9.0))
                    .line_height(px(10.0))
                    .text_color(theme.dim)
                    .child("100")
                    .child("75")
                    .child("50")
                    .child("25")
                    .child("0"),
            )
            .child(wave_canvas)
            .into_any_element()
    } else {
        let hist_canvas = canvas(
            |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, (), window, _cx| {
                let inset = Bounds {
                    origin: point(bounds.origin.x + px(4.0), bounds.origin.y + px(6.0)),
                    size: size(
                        (bounds.size.width - px(8.0)).max(px(1.0)),
                        (bounds.size.height - px(6.0)).max(px(1.0)),
                    ),
                };
                for frac in [0.25f32, 0.5, 0.75] {
                    let y = inset.origin.y + px((f32::from(inset.size.height) * frac).floor());
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(inset.origin.x, y),
                            size: size(inset.size.width, px(1.0)),
                        },
                        grid_line,
                    ));
                }
                let Some(hist) = &histogram else { return };
                let peak = hist.peak() as f32;
                if peak <= 0.0 {
                    return;
                }
                paint_histogram_channel(
                    window,
                    &hist.red,
                    peak,
                    inset,
                    hex_alpha(0xff6b6b, 0.40),
                    hex_alpha(0xff6b6b, 0.90),
                );
                paint_histogram_channel(
                    window,
                    &hist.green,
                    peak,
                    inset,
                    hex_alpha(0x6bcc78, 0.40),
                    hex_alpha(0x6bcc78, 0.90),
                );
                paint_histogram_channel(
                    window,
                    &hist.blue,
                    peak,
                    inset,
                    hex_alpha(0x4d96ff, 0.40),
                    hex_alpha(0x4d96ff, 0.90),
                );
            },
        )
        .size_full();

        hist_canvas.into_any_element()
    };

    let grip_color = if dragging_height {
        theme.dim
    } else {
        theme.track
    };
    let grip_hover = theme.dim;
    let word_font = theme.ui_font(FontWeight::MEDIUM);
    let note_font = theme.ui_font(FontWeight::NORMAL);
    let (empty_font, empty_size) = theme.serif_spec(scale::HINT, 400.0, true);

    div()
        .w_full()
        .px(px(GUTTER))
        .pb(px(SCOPE_BOTTOM_PAD))
        .flex()
        .flex_col()
        .gap(px(SCOPE_GAP))
        // Header switcher: Histogram · Waveform ... RGB
        .child(
            div()
                .h(px(SCOPE_HEADER_HEIGHT))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(14.0))
                        .child(
                            div()
                                .id("scope-mode-histogram")
                                .cursor_pointer()
                                .font(word_font.clone())
                                .text_size(px(scale::SCOPE_WORD))
                                .text_color(if !is_waveform { theme.paper } else { theme.dim })
                                .active(|s| s.opacity(0.62))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_hist(this, "Histogram", window, cx);
                                }))
                                .child("Histogram"),
                        )
                        .child(
                            div()
                                .id("scope-mode-waveform")
                                .cursor_pointer()
                                .font(word_font)
                                .text_size(px(scale::SCOPE_WORD))
                                .text_color(if is_waveform { theme.paper } else { theme.dim })
                                .active(|s| s.opacity(0.62))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    on_wave(this, "Waveform", window, cx);
                                }))
                                .child("Waveform"),
                        ),
                )
                .child(
                    div()
                        .font(note_font)
                        .text_size(px(scale::SCOPE_NOTE))
                        .text_color(theme.dim)
                        .child("RGB"),
                ),
        )
        // Borderless plot well in `bg`
        .child(
            div()
                .relative()
                .w_full()
                .h(px(scope_height as f32))
                .rounded(px(3.0))
                .bg(theme.canvas)
                .overflow_hidden()
                .child(plot_element)
                .when(show_empty, |el| {
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .font(empty_font)
                            .text_size(px(empty_size))
                            .text_color(theme.dim)
                            .child("No developed photograph"),
                    )
                }),
        )
        // Bottom resize grip
        .child(
            div()
                .id("scope-resize-grip")
                .w_full()
                .h(px(SCOPE_GRIP_HEIGHT))
                .flex()
                .items_center()
                .justify_center()
                .cursor_ns_resize()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        on_grip_down(this, event, window, cx);
                    }),
                )
                .child(
                    div()
                        .w(px(32.0))
                        .h(px(3.0))
                        .rounded_full()
                        .bg(grip_color)
                        .hover(move |s| s.bg(grip_hover)),
                ),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::crop::{CropSettings, Orientation};
    use negs_app::histogram::Scopes;

    #[test]
    fn scope_height_drag_clamps_to_session_bounds() {
        let drag = ScopeHeightDrag {
            start_y: 100.0,
            start_height: SCOPE_HEIGHTS.0,
        };
        assert_eq!(drag.height_for_y(100.0), SCOPE_HEIGHTS.0);
        assert_eq!(drag.height_for_y(20.0), SCOPE_HEIGHTS.0);
        assert_eq!(drag.height_for_y(200.0), SCOPE_HEIGHTS.0 + 100.0);
        assert_eq!(drag.height_for_y(900.0), SCOPE_HEIGHTS.1);
    }

    #[test]
    fn scopes_total_height_tracks_plot_height() {
        assert_eq!(scopes_total_height(88.0), 134.0);
        assert_eq!(scopes_total_height(188.0), 234.0);
    }

    #[test]
    fn waveform_converts_to_valid_render_image() {
        let mut rgba = vec![0u8; 16 * 8 * 4];
        for px_i in 0..(16 * 8) {
            rgba[px_i * 4] = 180;
            rgba[px_i * 4 + 1] = 120;
            rgba[px_i * 4 + 2] = 60;
            rgba[px_i * 4 + 3] = 255;
        }
        let scopes = Scopes::shown(&rgba, 16, 8, &CropSettings::default(), Orientation::new(0, false), 1024);
        assert!(scopes.waveform.samples > 0);
        let img = waveform_to_render_image(&scopes.waveform);
        assert_eq!(img.size(0).width.0, scopes.waveform.width as i32);
        assert_eq!(img.size(0).height.0, 256);
    }
}
