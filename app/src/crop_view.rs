//! Crop and straighten (`Crop.swift`; ported from `windows/src/ui/canvas.rs`
//! crop mode and the Windows crop panel in `inspector.rs`).
//!
//! While cropping the canvas shows the whole developed photograph, turned
//! by the angle being drawn, fitted between the caption's and label's bands
//! (no zoom); outside the crop it is dimmed (`CROP_SHADE` at 0.6, blended
//! encoded as Core Animation does), with a white outline, thick corner
//! brackets, a rule-of-thirds grid while a handle is dragged and a fine 9×9
//! grid while straightening. The straightening dial takes the label's
//! place. The crop panel replaces the inspector. Return applies, Esc
//! cancels (unless a ratio field has focus).
//!
//! The turned picture is made on the CPU once per angle and size: the
//! photograph exact-fit to the frame (Lanczos, colour-managed, as the canvas
//! draws it), then turned about its centre with bilinear sampling; a shaded
//! copy is drawn whole and the plain one clipped to the crop rectangle, so
//! dragging a handle costs nothing.

use std::sync::Arc;

use gpui::{
    AnyElement, Bounds, ContentMask, Context, Corners, CursorStyle, IntoElement, KeyDownEvent, MouseButton, Pixels, RenderImage, Window, div,
    prelude::*, px,
};
use negs_app::crop::{self, CropAspect, CropSession, CropSettings, Handle, Orientation, Point, Rect, Size};
use negs_app::picture::Picture;

use crate::NegsApp;
use crate::color_display::{DisplayColorProfile, bgra_to_render_image, color_manage_srgb_to_bgra, resample_and_color_manage_bgra};
use crate::print;
use crate::sheet::SheetInk;
use crate::theme_gpui::{GpuiTheme, hex, hex_alpha};
use crate::widgets::text_input::{InlineTextState, TextInputOutcome, TextInputStyle, render_text_input};

/// What a press while cropping takes hold of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropTarget {
    Handle(Handle),
    Dial,
}

/// A drag in progress: the dial from an angle and an x, or a handle from
/// the shown rectangle and the press point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CropDrag {
    Dial { angle: f64, x: f64 },
    Handle { handle: Handle, start: Rect, at: Point },
}

/// The frame the photograph is drawn in and DIPs per photo pixel.
pub fn layout(canvas: Rect, session: &CropSession) -> (Rect, f64) {
    print::crop_place(canvas, session.photo)
}

/// The crop rectangle on screen.
pub fn crop_rect(frame: Rect, scale: f64, shown: Rect) -> Rect {
    Rect::new(frame.x + shown.x * scale, frame.y + shown.y * scale, shown.width * scale, shown.height * scale)
}

/// What's under `p` (window DIPs) while cropping.
pub fn target_at(canvas: Rect, session: &CropSession, p: Point) -> Option<CropTarget> {
    let (frame, scale) = layout(canvas, session);
    if print::dial_rect(frame).contains(p) {
        return Some(CropTarget::Dial);
    }
    crop::handle_at(p, crop_rect(frame, scale, session.shown()), 1.0).map(CropTarget::Handle)
}

pub fn cursor_for(target: Option<CropTarget>) -> CursorStyle {
    match target {
        Some(CropTarget::Dial) => CursorStyle::ResizeLeftRight,
        Some(CropTarget::Handle(h)) => match (h.x, h.y) {
            (0, 0) => CursorStyle::OpenHand,
            (-1, -1) | (1, 1) => CursorStyle::ResizeUpLeftDownRight,
            (1, -1) | (-1, 1) => CursorStyle::ResizeUpRightDownLeft,
            (_, 0) => CursorStyle::ResizeLeftRight,
            _ => CursorStyle::ResizeUpDown,
        },
        None => CursorStyle::Arrow,
    }
}

/// A press at `p`: starts a drag, or (double click on the dial) levels the
/// photograph. None when it hit nothing.
pub fn press(canvas: Rect, session: &mut CropSession, p: Point, double: bool) -> Option<Option<CropDrag>> {
    match target_at(canvas, session, p)? {
        CropTarget::Dial if double => {
            session.begin_straightening();
            session.straighten(0.0);
            session.end_straightening();
            Some(None)
        }
        CropTarget::Dial => {
            session.begin_straightening();
            Some(Some(CropDrag::Dial { angle: session.angle, x: p.x }))
        }
        CropTarget::Handle(handle) => {
            session.adjusting = true;
            Some(Some(CropDrag::Handle { handle, start: session.shown(), at: p }))
        }
    }
}

/// The pointer at `p` during `drag`: 7 px a degree on the dial, catching at
/// zero; a handle follows the dragged edge (40 DIPs at least), and inside
/// moves one axis at a time so the crop slides along the turned photo's edges.
pub fn drag(canvas: Rect, session: &mut CropSession, drag: CropDrag, p: Point) {
    match drag {
        CropDrag::Dial { angle, x } => {
            let mut next = angle - (p.x - x) / print::DIAL_SPACING;
            if next.abs() < 0.4 {
                next = 0.0;
            }
            session.straighten(next);
        }
        CropDrag::Handle { handle, start, at } => {
            let (_, scale) = layout(canvas, session);
            let offset = ((p.x - at.x) / scale, (p.y - at.y) / scale);
            let (photo, angle, bounds) = (session.photo, session.angle, session.bounds());
            if handle == Handle::MOVE {
                let across = crop::constrain(start, crop::move_rect(start, offset.0, 0.0, bounds), angle, photo);
                let shown = crop::constrain(across, crop::move_rect(across, 0.0, offset.1, bounds), angle, photo);
                session.set_shown(shown);
            } else {
                let edge = Point { x: if handle.x < 0 { start.x } else { start.max_x() }, y: if handle.y < 0 { start.y } else { start.max_y() } };
                let resized = crop::resize(start, handle, Point { x: edge.x + offset.0, y: edge.y + offset.1 }, session.ratio(), bounds, 40.0 / scale);
                session.set_shown(crop::constrain(start, resized, angle, photo));
            }
        }
    }
}

pub fn release(session: &mut CropSession, drag: CropDrag) {
    match drag {
        CropDrag::Dial { .. } => session.end_straightening(),
        CropDrag::Handle { .. } => session.adjusting = false,
    }
}

/// Return and Esc while cropping; None if the key isn't theirs.
pub fn key(key: &str, typing: bool) -> Option<bool> {
    if typing {
        return None;
    }
    match key {
        "enter" => Some(true),
        "escape" => Some(false),
        _ => None,
    }
}

// MARK: The turned picture

/// `base` (BGRA, `bw × bh`, the whole shown photograph exact-fit) turned
/// `angle` degrees clockwise about its centre into a `w × h` picture of its
/// bounding box (`crop::bounds`), `scale` picture pixels per photo pixel;
/// outside the photograph is transparent. Straight alpha.
pub fn turned(base: &[u8], bw: u32, bh: u32, photo: Size, angle: f64, w: u32, h: u32) -> Vec<u8> {
    let b = crop::bounds(photo, angle);
    let mut out = vec![0u8; w as usize * h as usize * 4];
    if bw == 0 || bh == 0 || photo.width <= 0.0 {
        return out;
    }
    let (s, k) = angle.to_radians().sin_cos();
    let (cx, cy) = (photo.width / 2.0, photo.height / 2.0);
    // Photo pixels per output pixel, and base pixels per photo pixel.
    let (ox, oy) = (b.width / w as f64, b.height / h as f64);
    let (fx, fy) = (bw as f64 / photo.width, bh as f64 / photo.height);
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(1, 16);
    let rows_each = (h as usize).div_ceil(threads).max(1);
    std::thread::scope(|scope| {
        for (chunk, rows) in out.chunks_mut(rows_each * w as usize * 4).enumerate() {
            scope.spawn(move || {
                for (r, row) in rows.chunks_mut(w as usize * 4).enumerate() {
                    let y = (chunk * rows_each + r) as f64;
                    for x in 0..w as usize {
                        let (px_, py) = (b.x + (x as f64 + 0.5) * ox, b.y + (y + 0.5) * oy);
                        let (dx, dy) = (px_ - cx, py - cy);
                        let (gx, gy) = (cx + dx * k + dy * s, cy - dx * s + dy * k);
                        let sample = bilinear(base, bw, bh, gx * fx - 0.5, gy * fy - 0.5);
                        row[x * 4..x * 4 + 4].copy_from_slice(&sample);
                    }
                }
            });
        }
    });
    out
}

/// Bilinear BGRA at (x, y) in pixel centres; outside the picture is clear,
/// so the turned photograph's edge is anti-aliased.
fn bilinear(src: &[u8], w: u32, h: u32, x: f64, y: f64) -> [u8; 4] {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (x - x0, y - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let mut acc = [0.0f64; 4];
    let mut weight_alpha = 0.0;
    for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
        for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
            let (sx, sy) = (x0 + dx, y0 + dy);
            let wgt = wx * wy;
            if wgt <= 0.0 || sx < 0 || sy < 0 || sx >= w as i64 || sy >= h as i64 {
                continue;
            }
            let i = (sy as usize * w as usize + sx as usize) * 4;
            for c in 0..3 {
                acc[c] += src[i + c] as f64 * wgt;
            }
            weight_alpha += wgt;
        }
    }
    if weight_alpha <= 0.0 {
        return [0, 0, 0, 0];
    }
    let c = |v: f64| (v / weight_alpha).round().clamp(0.0, 255.0) as u8;
    [c(acc[0]), c(acc[1]), c(acc[2]), (weight_alpha * 255.0).round().clamp(0.0, 255.0) as u8]
}

/// `pixels` blended toward `shade` (BGRA) at `opacity`, in encoded values.
pub fn shaded(pixels: &[u8], shade: [u8; 4], opacity: f32) -> Vec<u8> {
    let mut out = pixels.to_vec();
    for p in out.chunks_exact_mut(4) {
        for c in 0..3 {
            p[c] = (p[c] as f32 + (shade[c] as f32 - p[c] as f32) * opacity).round() as u8;
        }
    }
    out
}

#[derive(Clone, PartialEq)]
struct TurnedKey {
    picture: u64,
    turns: i32,
    flip: bool,
    angle_bits: u64,
    size: (u32, u32),
    display: (u64, String),
}

/// The base exact-fit picture and the turned plain + shaded ones.
#[derive(Default)]
pub struct CropImages {
    base: Option<((u64, i32, bool, u32, u32, u64, String), Arc<Vec<u8>>)>,
    turned: Option<(TurnedKey, Arc<RenderImage>, Arc<RenderImage>)>,
    pub drops: Vec<Arc<RenderImage>>,
}

impl CropImages {
    pub fn clear(&mut self) {
        self.base = None;
        if let Some((_, a, b)) = self.turned.take() {
            self.drops.push(a);
            self.drops.push(b);
        }
    }

    /// The plain and shaded turned pictures for this frame, made if needed.
    #[allow(clippy::too_many_arguments)]
    fn get(&mut self, picture: &Picture, session: &CropSession, frame: Rect, scale: f64, sf: f32, profile: &DisplayColorProfile) -> Option<(Arc<RenderImage>, Arc<RenderImage>)> {
        let orientation = Orientation::new(session.rotation, session.flip);
        let sf = sf.max(1.0) as f64;
        let (bw, bh) = (((frame.width * sf).round() as u32).max(1), ((frame.height * sf).round() as u32).max(1));
        let display = (profile.display_key, profile.label.clone());
        let base_key = (picture.id, orientation.turns, orientation.flip, bw, bh, display.0, display.1.clone());
        if self.base.as_ref().is_none_or(|(k, _)| *k != base_key) {
            let cm = profile.transform.as_deref();
            let bgra = picture.with_pixels(|rgba| {
                resample_and_color_manage_bgra(rgba, picture.width, picture.height, picture.wide, orientation, &CropSettings::default(), bw, bh, cm)
            })?;
            self.base = Some((base_key, Arc::new(bgra)));
        }
        let b = crop::bounds(session.photo, session.angle);
        let (w, h) = (((b.width * scale * sf).round() as u32).max(1), ((b.height * scale * sf).round() as u32).max(1));
        let key = TurnedKey { picture: picture.id, turns: orientation.turns, flip: orientation.flip, angle_bits: session.angle.to_bits(), size: (w, h), display };
        if let Some((k, plain, shade)) = &self.turned
            && *k == key
        {
            return Some((plain.clone(), shade.clone()));
        }
        let base = self.base.as_ref()?.1.clone();
        let plain = if session.angle == 0.0 && (w, h) == (bw, bh) { base.to_vec() } else { turned(&base, bw, bh, session.photo, session.angle, w, h) };
        let shade = color_manage_srgb_to_bgra(hex_rgb(negs_app::theme::CROP_SHADE), profile.transform.as_deref());
        let dimmed = shaded(&plain, shade, negs_app::theme::CROP_SHADE_OPACITY);
        let (plain, dimmed) = (bgra_to_render_image(plain, w, h), bgra_to_render_image(dimmed, w, h));
        if let Some((_, a, b)) = self.turned.replace((key, plain.clone(), dimmed.clone())) {
            self.drops.push(a);
            self.drops.push(b);
        }
        Some((plain, dimmed))
    }
}

fn hex_rgb(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

/// Paints crop mode inside `canvas` (window DIPs): the turned photograph,
/// shaded outside the crop, the outline, grid and corners, the caption and
/// the dial. Returns the dial's turn-arrow targets.
#[allow(clippy::too_many_arguments)]
pub fn paint(
    images: &mut CropImages,
    p: &mut print::Painter,
    canvas: Rect,
    picture: &Picture,
    session: &CropSession,
    words: &print::PrintWords,
    ink: &SheetInk,
    theme_paper: u32,
    profile: &DisplayColorProfile,
    hot: Option<print::Target>,
) -> Vec<(print::Target, Rect)> {
    let sf = p.window.scale_factor();
    let (frame, scale) = layout(canvas, session);
    let shown = session.shown();
    let c = crop_rect(frame, scale, shown);
    if let Some((plain, dimmed)) = images.get(picture, session, frame, scale, sf, profile) {
        let b = crop::bounds(session.photo, session.angle);
        let at = Rect::new(frame.x + b.x * scale, frame.y + b.y * scale, b.width * scale, b.height * scale);
        let bounds = phys_bounds(at, sf);
        let _ = p.window.paint_image(bounds, Corners::default(), dimmed, 0, false);
        p.window.with_content_mask(Some(ContentMask { bounds: print::bounds(c) }), |window| {
            let _ = window.paint_image(bounds, Corners::default(), plain, 0, false);
        });
    }
    let paper = theme_paper;
    let lines = if session.straightening { 9 } else if session.adjusting { 3 } else { 0 };
    for i in 1..lines {
        let x = c.x + c.width * i as f64 / lines as f64;
        let y = c.y + c.height * i as f64 / lines as f64;
        p.fill(Rect::new(x, c.y, 1.0, c.height), hex_alpha(paper, 0.3));
        p.fill(Rect::new(c.x, y, c.width, 1.0), hex_alpha(paper, 0.3));
    }
    // The outline, a DIP outside the crop.
    p.window.paint_quad(gpui::quad(
        print::bounds(Rect::new(c.x - 1.0, c.y - 1.0, c.width + 2.0, c.height + 2.0)),
        Corners::default(),
        gpui::transparent_black(),
        gpui::Edges::all(px(1.0)),
        hex_alpha(paper, 0.9),
        gpui::BorderStyle::Solid,
    ));
    // The corners, 20 DIPs along each edge, just outside the line.
    let length = 20.0f64.min(c.width / 2.0).min(c.height / 2.0);
    let width = 3.0;
    let outer = c.inset(-width / 2.0, -width / 2.0);
    let mut corners = vec![];
    for (x, y, dx, dy) in [(outer.x, outer.y, 1.0, 1.0), (outer.max_x(), outer.y, -1.0, 1.0), (outer.x, outer.max_y(), 1.0, -1.0), (outer.max_x(), outer.max_y(), -1.0, -1.0)] {
        corners.push(Rect::new(if dx > 0.0 { x - width / 2.0 } else { x - length }, y - width / 2.0, length + width / 2.0, width));
        corners.push(Rect::new(x - width / 2.0, if dy > 0.0 { y - width / 2.0 } else { y - length }, width, length + width / 2.0));
    }
    // Uncropped, the corners lie on the sheet: on a light one, edged in its ink.
    if !ink.light {
        for b in &corners {
            p.fill(Rect::new(b.x - 1.0, b.y - 1.0, b.width + 2.0, b.height + 2.0), hex(ink.paper));
        }
    }
    for b in &corners {
        p.fill(*b, hex(paper));
    }
    let portrait = frame.height > frame.width;
    print::paint_caption(p, canvas, frame, portrait, words, ink);
    print::paint_dial(p, frame, session.angle, ink, hot, negs_app::host::decimal_separator())
}

/// `r` (DIPs) on whole physical pixels.
fn phys_bounds(r: Rect, sf: f32) -> Bounds<Pixels> {
    let sf = sf.max(1.0) as f64;
    let (x, y) = ((r.x * sf).round(), (r.y * sf).round());
    let (w, h) = ((r.width * sf).round().max(1.0), (r.height * sf).round().max(1.0));
    let eps = 1e-4;
    print::bounds(Rect::new((x + eps) / sf, (y + eps) / sf, (w - eps) / sf, (h - eps) / sf))
}

// MARK: The crop panel

/// The crop panel's own state: the custom ratio's two fields.
pub struct CropPanel {
    pub width: InlineTextState,
    pub height: InlineTextState,
    synced: (String, String),
    /// The angle slider being dragged: its track's left and width.
    pub slider: Option<(f32, f32)>,
}

impl CropPanel {
    pub fn new(cx: &mut Context<NegsApp>) -> Self {
        Self { width: InlineTextState::new(cx.focus_handle(), ""), height: InlineTextState::new(cx.focus_handle(), ""), synced: Default::default(), slider: None }
    }
}

/// A number typed into a ratio field, in either decimal mark.
pub fn parse_ratio(text: &str) -> Option<f64> {
    let t = text.trim().replace(',', ".");
    t.parse::<f64>().ok().filter(|v| v.is_finite() && *v > 0.0)
}

/// The value shown in a ratio field: up to two decimals, no trailing zeros.
pub fn ratio_text(v: f64, decimal: char) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    s.replace('.', &decimal.to_string())
}

/// The angle slider: −45…45° in tenths.
pub fn slider_angle(x: f32, left: f32, width: f32) -> f64 {
    let f = ((x - left) / width.max(1.0)).clamp(0.0, 1.0) as f64;
    ((-crop::MAX_ANGLE + f * 2.0 * crop::MAX_ANGLE) * 10.0).round() / 10.0
}

/// The size row's words: the draft's pixels, and a note (a warning when the
/// crop can't take the Longest side).
pub fn size_words(model: &negs_app::model::Model, session: &CropSession) -> (String, String, bool) {
    use negs_app::state::SizeMode;
    let (size, note, warn) = match model.crop_draft_size() {
        Some(Ok(size)) => {
            let source = model.source_size.0 as f64;
            let scale = size.working.0 as f64 / source.max(1.0);
            let note = if model.size_mode() != SizeMode::LongestSide {
                String::new()
            } else if scale >= 1.0 {
                format!("Image Size develops the whole frame {scale:.1}× the source.")
            } else {
                format!("Image Size develops the whole frame at {:.0}% of the source.", scale * 100.0)
            };
            ((size.photo.0 as f64, size.photo.1 as f64), note, false)
        }
        Some(Err(error)) => {
            let shown = session.shown();
            ((shown.width, shown.height), error.to_string(), true)
        }
        None => {
            let shown = session.shown();
            ((shown.width, shown.height), String::new(), false)
        }
    };
    (format!("{} × {} px", size.0.round() as i64, size.1.round() as i64), note, warn)
}

fn section(title: &str, theme: &GpuiTheme) -> AnyElement {
    div()
        .pt(px(18.0))
        .pb(px(6.0))
        .font_family(theme.ui_family.clone())
        .text_size(px(10.5))
        .font_weight(gpui::FontWeight(600.0))
        .text_color(theme.dim)
        .child(title.to_string())
        .into_any_element()
}

/// Replaces the adjustments while cropping (`CropInspector`).
pub fn render_panel(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    let session = app.model.crop_session.clone()?;
    let theme = app.theme.clone();
    let decimal = negs_app::host::decimal_separator();
    let synced = (ratio_text(session.custom.width, decimal), ratio_text(session.custom.height, decimal));
    if app.crop_panel.synced != synced {
        if !app.crop_panel.width.is_focused(window) {
            app.crop_panel.width.sync_external(synced.0.clone());
        }
        if !app.crop_panel.height.is_focused(window) {
            app.crop_panel.height.sync_external(synced.1.clone());
        }
        app.crop_panel.synced = synced;
    }
    let typing = app.crop_panel.width.is_focused(window) || app.crop_panel.height.is_focused(window);
    if let Some(s) = app.model.crop_session.as_mut() {
        s.typing = typing;
    }
    let (size_text, note, warn) = size_words(&app.model, &session);
    let portrait = session.orientation_portrait();
    let turns = session.aspect.turns();

    let mut rows = div().flex().flex_col().w_full().px(px(22.0));
    rows = rows.child(section("ASPECT RATIO", &theme));
    for (i, aspect) in CropAspect::ALL.into_iter().enumerate() {
        let chosen = aspect == session.aspect;
        let mut row = div()
            .id(("crop-aspect", i))
            .flex()
            .items_center()
            .h(px(26.0))
            .gap(px(10.0))
            .cursor_pointer()
            .hover(|s| s.bg(theme.subtle_hover))
            .on_click(cx.listener(move |this, _, _w, cx| {
                if let Some(s) = this.model.crop_session.as_mut() {
                    s.choose(aspect);
                }
                this.model.crop_changed();
                this.after_model_update(cx);
                cx.notify();
            }))
            .child(div().size(px(6.0)).rounded(px(3.0)).when(chosen, |d| d.bg(theme.accent)).when(!chosen, |d| d.border_1().border_color(theme.track)))
            .child(
                div()
                    .flex_1()
                    .font_family(theme.serif_family.clone())
                    .text_size(px(15.0))
                    .text_color(if chosen { theme.paper } else { theme.label })
                    .child(aspect.title().to_string()),
            );
        if let Some(format) = aspect.format() {
            row = row.child(div().font_family(theme.serif_family.clone()).italic().text_size(px(13.5)).text_color(theme.dim).child(format.to_string()));
        }
        rows = rows.child(row);
    }
    if session.aspect == CropAspect::Custom {
        let style = TextInputStyle { width: 70.0, height: 24.0, font_size: 13.0, align_right: true, ..Default::default() };
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .pl(px(16.0))
                .py(px(4.0))
                .child(div().text_size(px(12.0)).text_color(theme.label).child("Ratio"))
                .child(render_text_input("crop-ratio-w", &app.crop_panel.width, style.clone(), &theme, window, cx, |this, e: &KeyDownEvent, _w, cx| ratio_key(this, e, true, cx), |this, w, _cx| this.crop_panel.width.focus_handle.focus(w)))
                .child(div().text_size(px(12.0)).text_color(theme.dim).child("×"))
                .child(render_text_input("crop-ratio-h", &app.crop_panel.height, style, &theme, window, cx, |this, e: &KeyDownEvent, _w, cx| ratio_key(this, e, false, cx), |this, w, _cx| this.crop_panel.height.focus_handle.focus(w))),
        );
    }
    // Orientation: two words, the chosen one in paper with the pencil under it.
    let orientation_word = |id: &'static str, title: &'static str, is_portrait: bool, cx: &mut Context<NegsApp>| {
        let chosen = portrait == is_portrait;
        div()
            .id(id)
            .flex()
            .flex_col()
            .when(turns, |d| d.cursor_pointer())
            .when(!turns, |d| d.opacity(negs_app::theme::DISABLED_OPACITY))
            .on_click(cx.listener(move |this, _, _w, cx| {
                if let Some(s) = this.model.crop_session.as_mut()
                    && s.aspect.turns()
                {
                    s.orient(is_portrait);
                }
                this.model.crop_changed();
                cx.notify();
            }))
            .child(div().font_family(theme.serif_family.clone()).text_size(px(14.5)).text_color(if chosen { theme.paper } else { theme.dim }).child(title))
            .child(div().h(px(2.0)).w_full().when(chosen && turns, |d| d.bg(theme.accent)))
    };
    rows = rows.child(
        div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .pt(px(10.0))
            .child(div().w(px(84.0)).text_size(px(12.0)).text_color(theme.label).when(!turns, |d| d.opacity(negs_app::theme::DISABLED_OPACITY)).child("Orientation"))
            .child(orientation_word("crop-landscape", "Landscape", false, cx))
            .child(orientation_word("crop-portrait", "Portrait", true, cx)),
    );
    // Straighten: the angle as a slider with its value and a reset.
    let fraction = ((session.angle + crop::MAX_ANGLE) / (2.0 * crop::MAX_ANGLE)).clamp(0.0, 1.0) as f32;
    let entity = cx.entity().clone();
    let track = gpui::canvas(
        move |bounds, _w, _cx| bounds,
        {
            let theme = theme.clone();
            move |bounds: Bounds<Pixels>, _: Bounds<Pixels>, window, cx| {
                let (l, w) = (f32::from(bounds.origin.x), f32::from(bounds.size.width));
                let mid = f32::from(bounds.origin.y) + f32::from(bounds.size.height) / 2.0;
                window.paint_quad(gpui::fill(print::bounds(Rect::new(l as f64, mid as f64 - 0.5, w as f64, 1.0)), theme.track));
                let zero = l + w / 2.0;
                window.paint_quad(gpui::fill(print::bounds(Rect::new(zero as f64 - 0.5, mid as f64 - 4.0, 1.0, 8.0)), theme.dim));
                let x = l + w * fraction;
                window.paint_quad(gpui::fill(print::bounds(Rect::new(x as f64 - 5.0, mid as f64 - 5.0, 10.0, 10.0)), theme.paper).corner_radii(px(5.0)));
                entity.update(cx, |this, _| {
                    if let Some(s) = this.crop_panel.slider.as_mut() {
                        *s = (l, w);
                    }
                    this.crop_panel_track = (l, w);
                });
            }
        },
    )
    .w_full()
    .h(px(20.0));
    rows = rows.child(section("STRAIGHTEN", &theme)).child(
        div()
            .flex()
            .items_center()
            .gap(px(10.0))
            .child(div().w(px(50.0)).text_size(px(12.0)).text_color(if session.angle != 0.0 { theme.paper } else { theme.label }).child("Angle"))
            .child(
                div()
                    .id("crop-angle")
                    .flex_1()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, e: &gpui::MouseDownEvent, _w, cx| {
                            let (l, w) = this.crop_panel_track;
                            this.crop_panel.slider = Some((l, w));
                            if let Some(s) = this.model.crop_session.as_mut() {
                                s.begin_straightening();
                                s.straighten(slider_angle(f32::from(e.position.x), l, w));
                            }
                            this.model.crop_changed();
                            cx.notify();
                        }),
                    )
                    .child(track),
            )
            .child(div().w(px(44.0)).text_size(px(12.0)).text_color(theme.paper).child(format!("{}°", ratio_text_tenths(session.angle, decimal))))
            .child(
                div()
                    .id("crop-angle-reset")
                    .w(px(16.0))
                    .font_family(theme.ui_family.clone())
                    .text_size(px(13.0))
                    .text_color(theme.dim)
                    .when(session.angle != 0.0, |d| {
                        d.cursor_pointer().child("↺").on_click(cx.listener(|this, _, _w, cx| {
                            if let Some(s) = this.model.crop_session.as_mut() {
                                s.begin_straightening();
                                s.straighten(0.0);
                                s.end_straightening();
                            }
                            this.model.crop_changed();
                            cx.notify();
                        }))
                    }),
            ),
    );
    // Flip left to right: the edit's, kept with the crop on Done.
    rows = rows.child(section("FLIP", &theme)).child(
        div()
            .id("crop-flip")
            .flex()
            .items_center()
            .gap(px(8.0))
            .cursor_pointer()
            .on_click(cx.listener(|this, _, _w, cx| {
                this.model.flip();
                this.after_model_update(cx);
                cx.notify();
            }))
            .child(
                div()
                    .size(px(13.0))
                    .rounded(px(2.0))
                    .border_1()
                    .border_color(if session.flip { theme.accent } else { theme.track })
                    .when(session.flip, |d| d.bg(theme.accent)),
            )
            .child(div().text_size(px(12.5)).text_color(if session.flip { theme.paper } else { theme.label }).child("Flip left to right")),
    );
    rows = rows
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .pt(px(18.0))
                .child(div().w(px(50.0)).text_size(px(12.0)).text_color(theme.label).child("Size"))
                .child(div().text_size(px(12.5)).text_color(theme.paper).child(size_text)),
        )
        .child(div().pt(px(2.0)).text_size(px(11.0)).text_color(theme.dim).w_full().min_w_0().child("The exported photograph’s size, before the border."))
        .when(!note.is_empty(), |d| d.child(div().pt(px(4.0)).text_size(px(11.0)).text_color(if warn { theme.warn } else { theme.dim }).w_full().min_w_0().child(note)))
        .child(
            div()
                .pt(px(16.0))
                .text_size(px(11.0))
                .text_color(theme.dim)
                .w_full()
                .min_w_0()
                .child("Drag the corners or edges to crop, or inside to move it. Drag the dial under the photograph to straighten it; the arrows at its ends turn it a quarter. The border is added after cropping."),
        );

    let modified = session.modified();
    let header = div()
        .flex()
        .items_center()
        .justify_between()
        .px(px(22.0))
        .h(px(52.0))
        .border_b_1()
        .border_color(theme.line)
        .child(div().font_family(theme.serif_family.clone()).text_size(px(20.0)).text_color(theme.paper).child("Crop"))
        .child(
            div()
                .id("crop-reset")
                .text_size(px(12.0))
                .text_color(if modified { theme.label } else { theme.dim })
                .when(!modified, |d| d.opacity(negs_app::theme::DISABLED_OPACITY))
                .when(modified, |d| d.cursor_pointer().hover(|s| s.text_color(theme.paper)))
                .on_click(cx.listener(|this, _, _w, cx| {
                    if let Some(s) = this.model.crop_session.as_mut() {
                        s.reset();
                    }
                    this.model.crop_changed();
                    cx.notify();
                }))
                .child("Reset Crop"),
        );
    let foot = div()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .px(px(22.0))
        .py(px(16.0))
        .border_t_1()
        .border_color(theme.line)
        .child(div().font_family(theme.serif_family.clone()).italic().text_size(px(14.0)).text_color(theme.dim).child("return applies · esc cancels"))
        .child(
            div()
                .flex()
                .gap(px(10.0))
                .child(
                    div()
                        .id("crop-cancel")
                        .flex_1()
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .border_1()
                        .border_color(theme.track)
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.subtle_hover))
                        .font_family(theme.serif_family.clone())
                        .text_size(px(15.0))
                        .text_color(theme.label)
                        .on_click(cx.listener(|this, _, _w, cx| {
                            this.model.end_crop(false);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child("Cancel"),
                )
                .child(
                    div()
                        .id("crop-done")
                        .flex_grow()
                        .flex_basis(px(180.0))
                        .h(px(34.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .bg(theme.fill)
                        .cursor_pointer()
                        .hover(|s| s.opacity(0.92))
                        .font_family(theme.serif_family.clone())
                        .font_weight(gpui::FontWeight(600.0))
                        .text_size(px(15.0))
                        .text_color(theme.on_fill)
                        .on_click(cx.listener(|this, _, _w, cx| {
                            this.model.end_crop(true);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child("Done"),
                ),
        );
    Some(
        div()
            .id("crop-panel")
            .w(px(crate::inspector::INSPECTOR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .border_l_1()
            .border_color(theme.line)
            .font_family(theme.ui_family.clone())
            .child(header)
            .child(div().id("crop-rows").flex_1().min_h_0().overflow_y_scroll().pb(px(12.0)).child(rows))
            .child(foot)
            .into_any_element(),
    )
}

fn ratio_text_tenths(v: f64, decimal: char) -> String {
    let v = if v == 0.0 { 0.0 } else { v };
    format!("{v:.1}").replace('.', &decimal.to_string())
}

/// A key in a ratio field: Enter commits the number, Esc puts it back.
fn ratio_key(app: &mut NegsApp, event: &KeyDownEvent, width: bool, cx: &mut Context<NegsApp>) {
    let field = if width { &mut app.crop_panel.width } else { &mut app.crop_panel.height };
    match field.handle_key_event(event, cx) {
        TextInputOutcome::Commit(text) => {
            if let (Some(v), Some(s)) = (parse_ratio(&text), app.model.crop_session.as_mut()) {
                if width {
                    s.set_custom(Some(v), None);
                } else {
                    s.set_custom(None, Some(v));
                }
            }
            app.crop_panel.synced = Default::default();
            app.model.crop_changed();
        }
        TextInputOutcome::Cancel => app.crop_panel.synced = Default::default(),
        _ => {}
    }
    cx.stop_propagation();
    cx.notify();
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::state::EditState;

    fn session() -> CropSession {
        CropSession::new(EditState::default(), Size::new(3000.0, 2000.0), 0, false)
    }

    const CANVAS: Rect = Rect::new(0.0, 0.0, 1000.0, 722.0);

    #[test]
    fn handles_and_the_dial_are_found_under_the_pointer() {
        let s = session();
        let (frame, scale) = layout(CANVAS, &s);
        let c = crop_rect(frame, scale, s.shown());
        assert_eq!(c, frame);
        assert_eq!(target_at(CANVAS, &s, Point { x: c.x, y: c.y }), Some(CropTarget::Handle(Handle { x: -1, y: -1 })));
        assert_eq!(target_at(CANVAS, &s, Point { x: c.max_x(), y: c.mid_y() }), Some(CropTarget::Handle(Handle { x: 1, y: 0 })));
        assert_eq!(target_at(CANVAS, &s, Point { x: c.mid_x(), y: c.mid_y() }), Some(CropTarget::Handle(Handle::MOVE)));
        let dial = print::dial_rect(frame);
        assert_eq!(target_at(CANVAS, &s, Point { x: dial.mid_x(), y: dial.mid_y() }), Some(CropTarget::Dial));
        assert_eq!(cursor_for(Some(CropTarget::Handle(Handle { x: 1, y: -1 }))), CursorStyle::ResizeUpRightDownLeft);
    }

    #[test]
    fn a_corner_drag_follows_the_edge_and_keeps_a_locked_ratio() {
        let mut s = session();
        s.choose(CropAspect::Square);
        let (frame, scale) = layout(CANVAS, &s);
        let c = crop_rect(frame, scale, s.shown());
        let at = Point { x: c.max_x(), y: c.max_y() };
        let Some(Some(d)) = press(CANVAS, &mut s, at, false) else { panic!("no handle") };
        assert!(s.adjusting);
        drag(CANVAS, &mut s, d, Point { x: at.x - 100.0, y: at.y - 40.0 });
        let r = s.shown();
        assert!((r.width - r.height).abs() < 1e-6, "{r:?}");
        assert!(r.width < 2000.0);
        release(&mut s, d);
        assert!(!s.adjusting);
    }

    #[test]
    fn moving_inside_stays_on_the_photograph() {
        let mut s = session();
        s.choose(CropAspect::Square);
        let (frame, scale) = layout(CANVAS, &s);
        let c = crop_rect(frame, scale, s.shown());
        let mid = Point { x: c.mid_x(), y: c.mid_y() };
        let Some(Some(d)) = press(CANVAS, &mut s, mid, false) else { panic!() };
        drag(CANVAS, &mut s, d, Point { x: mid.x + 5000.0, y: mid.y + 5000.0 });
        let r = s.shown();
        assert!((r.max_x() - 3000.0).abs() < 1e-6 && (r.max_y() - 2000.0).abs() < 1e-6, "{r:?}");
    }

    #[test]
    fn the_dial_turns_seven_pixels_a_degree_catches_at_zero_and_levels_on_double_click() {
        let mut s = session();
        let (frame, _) = layout(CANVAS, &s);
        let dial = print::dial_rect(frame);
        let at = Point { x: dial.mid_x(), y: dial.mid_y() };
        let Some(Some(d)) = press(CANVAS, &mut s, at, false) else { panic!() };
        assert!(s.straightening);
        drag(CANVAS, &mut s, d, Point { x: at.x - 3.0 * print::DIAL_SPACING, y: at.y });
        assert!((s.angle - 3.0).abs() < 1e-9);
        // The crop shrank to stay on the turned photograph.
        assert!(crop::fits(s.shown(), s.angle, s.photo));
        drag(CANVAS, &mut s, d, Point { x: at.x - 0.2 * print::DIAL_SPACING, y: at.y });
        assert_eq!(s.angle, 0.0);
        drag(CANVAS, &mut s, d, Point { x: at.x + 2.0 * print::DIAL_SPACING, y: at.y });
        release(&mut s, d);
        assert!(!s.straightening);
        assert!(press(CANVAS, &mut s, at, true).is_some());
        assert_eq!(s.angle, 0.0);
    }

    #[test]
    fn return_and_escape_belong_to_a_ratio_field_while_typing() {
        assert_eq!(key("enter", false), Some(true));
        assert_eq!(key("escape", false), Some(false));
        assert_eq!(key("enter", true), None);
        assert_eq!(key("a", false), None);
        assert_eq!(parse_ratio(" 4,5 "), Some(4.5));
        assert_eq!(parse_ratio("-1"), None);
        assert_eq!(ratio_text(1.5, ','), "1,5");
        assert_eq!(ratio_text(3.0, '.'), "3");
        assert_eq!(slider_angle(0.0, 0.0, 100.0), -45.0);
        assert_eq!(slider_angle(50.0, 0.0, 100.0), 0.0);
        assert_eq!(slider_angle(75.0, 0.0, 100.0), 22.5);
    }

    #[test]
    fn the_turned_picture_keeps_the_middle_and_clears_the_corners() {
        // A 4 × 2 picture of one grey, turned 30°.
        let base = vec![100u8, 110, 120, 255].repeat(8);
        let photo = Size::new(4.0, 2.0);
        let b = crop::bounds(photo, 30.0);
        let (w, h) = ((b.width * 10.0).round() as u32, (b.height * 10.0).round() as u32);
        let out = turned(&base, 4, 2, photo, 30.0, w, h);
        let at = |x: u32, y: u32| &out[((y * w + x) * 4) as usize..][..4];
        assert_eq!(at(w / 2, h / 2), &[100, 110, 120, 255]);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(w - 1, h - 1)[3], 0);
        // Unturned at its own size, it's the base.
        assert_eq!(turned(&base, 4, 2, photo, 0.0, 4, 2), base);
        let dim = shaded(&base, [0, 0, 0, 255], 0.5);
        assert_eq!(&dim[..4], &[50, 55, 60, 255]);
    }
}
