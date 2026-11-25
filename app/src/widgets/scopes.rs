//! Scopes widget (`ScopesView`): Histogram & RGB Waveform with mode switcher,
//! cached BGRA8 `RenderImage` waveform rasterization (VRAM-safe via `drop_image`),
//! and bottom drag-to-resize handle constrained to `negs_app::session::SCOPE_HEIGHTS`.

use std::sync::Arc;

use gpui::{
    Bounds, Context, Corners, Hsla, IntoElement, MouseButton, MouseDownEvent, PathBuilder, Pixels,
    RenderImage, Window, canvas, div, fill, point, prelude::*, px, size,
};
use negs_app::histogram::{Histogram, Waveform};
use negs_app::session::SCOPE_HEIGHTS;

use crate::color_display::bgra_to_render_image;
use crate::theme_gpui::{GpuiTheme, hex_alpha};

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
    // Fast sampled FNV-1a across all 3 channels to catch tone changes even when
    // picture_id/width/samples are unchanged.
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
    let mut stroke_path = PathBuilder::stroke(px(1.15));

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

fn paint_histogram_luma(
    window: &mut Window,
    hist: &Histogram,
    peak: f32,
    bounds: Bounds<Pixels>,
    color: Hsla,
) {
    let x0 = f32::from(bounds.origin.x);
    let y0 = f32::from(bounds.origin.y);
    let w = f32::from(bounds.size.width).max(1.0);
    let h = f32::from(bounds.size.height).max(1.0);
    let bottom = y0 + h;

    let mut stroke = PathBuilder::stroke(px(1.0));
    for i in 0..256 {
        let luma = 0.2126 * hist.red[i] as f32
            + 0.7152 * hist.green[i] as f32
            + 0.0722 * hist.blue[i] as f32;
        let fx = i as f32 / 255.0;
        let fy = (luma / peak).min(1.0);
        let pt = point(px(x0 + fx * w), px(bottom - fy * h));
        if i == 0 {
            stroke.move_to(pt);
        } else {
            stroke.line_to(pt);
        }
    }
    if let Ok(path) = stroke.build() {
        window.paint_path(path, color);
    }
}

/// Renders the Scopes header (`Histogram` · `Waveform`), plot well, and height resize grip.
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
    let luma_color = theme.paper.opacity(0.45);
    let on_hist = on_select_mode.clone();
    let on_wave = on_select_mode;

    let plot_element = if is_waveform {
        let wave_canvas = canvas(
            |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, (), window, _cx| {
                // Horizontal 25% / 50% / 75% reference lines.
                for frac in [0.25f32, 0.5, 0.75] {
                    let y = bounds.origin.y + bounds.size.height * frac;
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(bounds.origin.x, y),
                            size: size(bounds.size.width, px(1.0)),
                        },
                        grid_line.opacity(0.55),
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
            .px(px(4.0))
            .py(px(4.0))
            .gap(px(4.0))
            .child(
                div()
                    .w(px(22.0))
                    .h_full()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .items_end()
                    .text_size(px(9.0))
                    .text_color(theme.dim)
                    .child("100")
                    .child("50")
                    .child("0"),
            )
            .child(wave_canvas)
            .into_any_element()
    } else {
        let hist_canvas = canvas(
            |_bounds, _window, _cx| {},
            move |bounds: Bounds<Pixels>, (), window, _cx| {
                for frac in [0.25f32, 0.5, 0.75] {
                    let y = bounds.origin.y + bounds.size.height * frac;
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(bounds.origin.x, y),
                            size: size(bounds.size.width, px(1.0)),
                        },
                        grid_line.opacity(0.55),
                    ));
                }
                let Some(hist) = &histogram else { return };
                let peak = hist.peak() as f32;
                if peak <= 0.0 {
                    return;
                }
                let inset = Bounds {
                    origin: point(bounds.origin.x + px(3.0), bounds.origin.y + px(5.0)),
                    size: size(
                        (bounds.size.width - px(6.0)).max(px(1.0)),
                        (bounds.size.height - px(5.0)).max(px(1.0)),
                    ),
                };
                paint_histogram_channel(
                    window,
                    &hist.red,
                    peak,
                    inset,
                    hex_alpha(0xff6b6b, 0.34),
                    hex_alpha(0xff6b6b, 0.85),
                );
                paint_histogram_channel(
                    window,
                    &hist.green,
                    peak,
                    inset,
                    hex_alpha(0x6bcc78, 0.34),
                    hex_alpha(0x6bcc78, 0.85),
                );
                paint_histogram_channel(
                    window,
                    &hist.blue,
                    peak,
                    inset,
                    hex_alpha(0x4d96ff, 0.34),
                    hex_alpha(0x4d96ff, 0.85),
                );
                paint_histogram_luma(window, hist, peak, inset, luma_color);
            },
        )
        .size_full();

        hist_canvas.into_any_element()
    };

    let grip_color = if dragging_height {
        theme.accent
    } else {
        theme.track
    };
    let grip_hover = theme.dim;

    div()
        .w_full()
        .px(px(16.0))
        .pt(px(8.0))
        .pb(px(2.0))
        .flex()
        .flex_col()
        .gap(px(5.0))
        // Header switcher: Histogram · Waveform
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.0))
                .child(
                    div()
                        .id("scope-mode-histogram")
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(if !is_waveform { theme.paper } else { theme.dim })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_hist(this, "Histogram", window, cx);
                        }))
                        .child("Histogram"),
                )
                .child(
                    div()
                        .id("scope-mode-waveform")
                        .cursor_pointer()
                        .text_size(px(11.0))
                        .text_color(if is_waveform { theme.paper } else { theme.dim })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            on_wave(this, "Waveform", window, cx);
                        }))
                        .child("Waveform"),
                ),
        )
        // Plot frame
        .child(
            div()
                .relative()
                .w_full()
                .h(px(scope_height as f32))
                .rounded(px(4.0))
                .bg(theme.canvas)
                .border_1()
                .border_color(theme.line)
                .overflow_hidden()
                .child(plot_element)
                .when(!has_photo, |el| {
                    el.child(
                        div()
                            .absolute()
                            .inset_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(theme.serif_family.clone())
                            .italic()
                            .text_size(px(11.5))
                            .text_color(theme.dim)
                            .child("No photograph"),
                    )
                }),
        )
        // Bottom resize grip
        .child(
            div()
                .id("scope-resize-grip")
                .w_full()
                .h(px(10.0))
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
