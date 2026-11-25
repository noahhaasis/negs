//! Film & Paper Stock Preview Tray (`TrayState` + `render_tray`).
//!
//! Driven directly by `negs_app::tray_rules` and `Model::tray_*`:
//! - `186 px` total height (`ROW` 158 px on `theme.film` + `FOOT` 28 px on `theme.panel`)
//! - Exact horizontal layout using `tray_rules::{starts, tile_left, content_width, reveal, jump, glide, current}`
//! - Color-managed stock tiles via `TileImageCache` with automatic VRAM cleanup when the tray closes
//! - Favourite star overlay (`20 × 20` at top-right corner, 5 px inset) drawn as crisp GPUI vector paths
//! - Smooth 250 ms glide when clicking a group in the foot index
//! - Live film search input with `.key_context("TextInput")` and paper-lock toggle for papers
//! - Keyboard navigation (`Left`, `Right`, `Return`, `Escape`) via `tray_rules::tray_key`.

use std::time::Instant;

use gpui::{
    AnyElement, Bounds, ClipboardItem, ContentMask, Context, Corners, FocusHandle, FontWeight, Hsla,
    KeyDownEvent, MouseButton, MouseDownEvent, PathBuilder, Pixels, ScrollDelta, ScrollWheelEvent,
    SharedString, Window, canvas, div, fill, point, prelude::*, px,
};
use negs_app::model::Model;
use negs_app::previews::Target;
use negs_app::tray_rules::{self as rules, Key, Section, TrayKey};

use crate::NegsApp;
use crate::theme_gpui::{GpuiTheme, hex};
use crate::tile_cache::{TileImageCache, cover_bounds};

/// Maps a GPUI keystroke key name to `negs_app::tray_rules::Key`.
pub fn map_key_name(key: &str) -> Key {
    match key {
        "escape" => Key::Escape,
        "enter" => Key::Return,
        "left" => Key::Left,
        "right" => Key::Right,
        _ => Key::Other,
    }
}

/// Resolves a key press while the tray is open using `tray_rules::tray_key`.
pub fn resolve_tray_key(key: &str, command: bool, typing: bool) -> TrayKey {
    rules::tray_key(map_key_name(key), command, typing)
}

/// Computes the next horizontal scroll offset for a wheel/trackpad scroll event.
/// Both vertical wheel (`dy`) and horizontal trackpad/wheel (`dx`) move the row sideways.
pub fn wheel_scroll_offset(offset: f64, dx: f64, dy: f64, visible: f64, content: f64) -> f64 {
    let delta = if dx.abs() > dy.abs() { -dx } else { -dy };
    rules::clamp(offset + delta, visible, content)
}

/// Returns the active section index underlined in the foot bar for `scroll_x`.
pub fn active_group_index(scroll_x: f64, sections: &[Section]) -> usize {
    let starts = rules::starts(sections);
    rules::current(scroll_x, &starts)
}

/// Returns `(section_idx, stock_idx, screen_x)` for all tiles visible within
/// `[-margin, viewport_w + margin]` so wide trays only paint on-screen tiles.
pub fn visible_tile_indices(
    sections: &[Section],
    scroll_x: f64,
    viewport_w: f64,
) -> Vec<(usize, usize, f64)> {
    let starts = rules::starts(sections);
    let margin = rules::TILE;
    let mut out = Vec::new();
    for (si, (section, start)) in sections.iter().zip(&starts).enumerate() {
        for i in 0..section.stocks.len() {
            let x = rules::tile_left(*start, i) - scroll_x;
            if x + rules::TILE + margin < 0.0 || x - margin > viewport_w {
                continue;
            }
            out.push((si, i, x));
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Glide {
    from: f64,
    to: f64,
    started: Instant,
}

/// Interactive state for the Film & Paper Stock Preview Tray.
pub struct TrayState {
    pub tile_cache: TileImageCache,
    scroll_x: f64,
    viewport_width: f64,
    glide: Option<Glide>,
    /// Last synced tray mode (`Some(paper)` when open, `None` when closed).
    was_open: Option<bool>,
    last_current: String,
    last_in_favourites: bool,
    last_query: String,
    shown_scan: bool,
    search_focus: FocusHandle,
    search_cursor: usize,
    search_select_all: bool,
    hovered_tile: Option<(usize, usize)>,
}

impl TrayState {
    pub fn new(cx: &mut Context<NegsApp>) -> Self {
        Self {
            tile_cache: TileImageCache::new(),
            scroll_x: 0.0,
            viewport_width: 960.0,
            glide: None,
            was_open: None,
            last_current: String::new(),
            last_in_favourites: true,
            last_query: String::new(),
            shown_scan: false,
            search_focus: cx.focus_handle(),
            search_cursor: 0,
            search_select_all: false,
            hovered_tile: None,
        }
    }

    pub fn scroll_offset(&self) -> f64 {
        self.scroll_x
    }

    pub fn is_search_focused(&self, window: &Window) -> bool {
        self.search_focus.is_focused(window)
    }

    pub fn clear_cache(&mut self, window: &mut Window) {
        self.tile_cache.clear(window);
    }

    /// Steps an active smooth glide animation. Returns `true` if still animating.
    pub fn step_glide(&mut self, now: Instant) -> bool {
        let Some(glide) = self.glide else {
            return false;
        };
        let elapsed_ms = now.saturating_duration_since(glide.started).as_secs_f64() * 1000.0;
        self.scroll_x = rules::glide(glide.from, glide.to, elapsed_ms);
        if elapsed_ms >= rules::GLIDE_MS {
            self.scroll_x = glide.to;
            self.glide = None;
            false
        } else {
            true
        }
    }

    /// Starts a 250 ms smooth glide to `target` (clamped to valid scroll bounds).
    pub fn start_glide_to(&mut self, target: f64, sections: &[Section]) {
        let content = rules::content_width(sections);
        let clamped = rules::clamp(target, self.viewport_width, content);
        if (clamped - self.scroll_x).abs() < 0.5 {
            self.scroll_x = clamped;
            self.glide = None;
            return;
        }
        self.glide = Some(Glide {
            from: self.scroll_x,
            to: clamped,
            started: Instant::now(),
        });
    }

    /// Scrolls smoothly to group `section_idx` using `tray_rules::jump`.
    pub fn jump_to_section(&mut self, section_idx: usize, sections: &[Section]) {
        let starts = rules::starts(sections);
        if let Some(&start) = starts.get(section_idx) {
            let content = rules::content_width(sections);
            let target = rules::jump(start, self.viewport_width, content);
            self.start_glide_to(target, sections);
        }
    }

    /// Applies mouse wheel / trackpad horizontal scrolling, canceling any active glide.
    pub fn scroll_by_wheel(&mut self, dx: f64, dy: f64, sections: &[Section]) {
        self.glide = None;
        let content = rules::content_width(sections);
        self.scroll_x = wheel_scroll_offset(self.scroll_x, dx, dy, self.viewport_width, content);
    }

    /// Synchronizes tray scroll position, search cursor, and VRAM cache lifecycle
    /// with `Model`. Releases all cached tray `RenderImage`s immediately on close.
    pub fn sync_from_model(&mut self, model: &Model, window: &mut Window) {
        let Some(paper) = model.tray_shown() else {
            if self.was_open.take().is_some() {
                self.tile_cache.clear(window);
                self.glide = None;
                self.hovered_tile = None;
                self.last_query.clear();
                self.search_cursor = 0;
                self.search_select_all = false;
                if self.search_focus.is_focused(window) {
                    window.blur();
                }
            }
            return;
        };

        let Some(visit) = model.tray() else {
            return;
        };

        let opened = self.was_open.replace(paper) != Some(paper);
        let sections = model.tray_sections();
        let current = model.tray_current(paper, visit.read_as);
        let content = rules::content_width(&sections);

        if opened {
            self.glide = None;
            self.hovered_tile = None;
            self.search_cursor = visit.query.chars().count();
            self.search_select_all = false;
            if visit.opens_on_scan {
                self.shown_scan = true;
                self.scroll_x = 0.0;
            } else {
                self.shown_scan = false;
                self.scroll_x = rules::reveal(
                    0.0,
                    &current,
                    visit.in_favourites,
                    true,
                    &sections,
                    self.viewport_width,
                );
            }
        } else if visit.opens_on_scan
            && !self.shown_scan
            && current == visit.before_id
            && visit.query.is_empty()
        {
            self.shown_scan = true;
            self.start_glide_to(0.0, &sections);
        } else if self.last_query != visit.query {
            self.glide = None;
            self.scroll_x = if visit.query.is_empty() {
                rules::reveal(
                    self.scroll_x,
                    &current,
                    visit.in_favourites,
                    true,
                    &sections,
                    self.viewport_width,
                )
            } else {
                0.0
            };
        } else if self.last_current != current || self.last_in_favourites != visit.in_favourites {
            self.glide = None;
            self.scroll_x = rules::reveal(
                self.scroll_x,
                &current,
                visit.in_favourites,
                false,
                &sections,
                self.viewport_width,
            );
        }

        self.scroll_x = rules::clamp(self.scroll_x, self.viewport_width, content);
        self.last_current = current;
        self.last_in_favourites = visit.in_favourites;
        self.last_query = visit.query.clone();
        self.search_cursor = self.search_cursor.min(self.last_query.chars().count());
    }

    /// Handles keyboard navigation (`Left`, `Right`, `Return`, `Escape`) while the
    /// tray is open and not typing. Returns `true` if handled.
    pub fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        model: &mut Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) -> bool {
        if model.tray_shown().is_none() {
            return false;
        }
        let typing = self.is_search_focused(window);
        let command = event.keystroke.modifiers.control
            || event.keystroke.modifiers.platform
            || event.keystroke.modifiers.alt;

        match resolve_tray_key(event.keystroke.key.as_str(), command, typing) {
            TrayKey::GoBack => {
                model.end_tray(false);
                self.sync_from_model(model, window);
                cx.notify();
                true
            }
            TrayKey::Keep => {
                model.end_tray(true);
                self.sync_from_model(model, window);
                cx.notify();
                true
            }
            TrayKey::Step(offset) => {
                model.step_tray(offset);
                self.sync_from_model(model, window);
                let _ = model.pump_previews();
                cx.notify();
                true
            }
            TrayKey::Pass => false,
        }
    }

    fn handle_search_key(
        &mut self,
        event: &KeyDownEvent,
        model: &mut Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        let key = event.keystroke.key.as_str();
        let cmd = if cfg!(target_os = "macos") {
            event.keystroke.modifiers.platform
        } else {
            event.keystroke.modifiers.control
        };

        if key == "enter" {
            model.tray_submit();
            self.sync_from_model(model, window);
            let _ = model.pump_previews();
            cx.notify();
            return;
        }
        if key == "escape" {
            let was_empty = model.tray().is_none_or(|t| t.query.is_empty());
            model.tray_cancel_search();
            self.search_cursor = 0;
            self.search_select_all = false;
            if was_empty {
                window.blur();
            }
            self.sync_from_model(model, window);
            let _ = model.pump_previews();
            cx.notify();
            return;
        }

        let mut query = model.tray().map(|t| t.query.clone()).unwrap_or_default();
        let char_len = query.chars().count();
        self.search_cursor = self.search_cursor.min(char_len);

        let byte_idx = |s: &str, ci: usize| -> usize {
            s.char_indices().nth(ci).map_or(s.len(), |(b, _)| b)
        };

        if cmd {
            match key {
                "a" => {
                    self.search_select_all = true;
                    self.search_cursor = char_len;
                    cx.notify();
                }
                "c" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(query));
                }
                "x" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(query));
                    self.search_cursor = 0;
                    self.search_select_all = false;
                    model.set_tray_query("");
                    self.sync_from_model(model, window);
                    let _ = model.pump_previews();
                    cx.notify();
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                            if self.search_select_all {
                                query.clear();
                                self.search_cursor = 0;
                                self.search_select_all = false;
                            }
                            let b = byte_idx(&query, self.search_cursor);
                            query.insert_str(b, &clean);
                            self.search_cursor += clean.chars().count();
                            model.set_tray_query(&query);
                            self.sync_from_model(model, window);
                            let _ = model.pump_previews();
                            cx.notify();
                        }
                    }
                }
                _ => {}
            }
            return;
        }

        let mut text_changed = false;
        match key {
            "backspace" => {
                if self.search_select_all {
                    query.clear();
                    self.search_cursor = 0;
                    self.search_select_all = false;
                    text_changed = true;
                } else if self.search_cursor > 0 {
                    let s = byte_idx(&query, self.search_cursor - 1);
                    let e = byte_idx(&query, self.search_cursor);
                    query.replace_range(s..e, "");
                    self.search_cursor -= 1;
                    text_changed = true;
                }
            }
            "delete" => {
                if self.search_select_all {
                    query.clear();
                    self.search_cursor = 0;
                    self.search_select_all = false;
                    text_changed = true;
                } else if self.search_cursor < char_len {
                    let s = byte_idx(&query, self.search_cursor);
                    let e = byte_idx(&query, self.search_cursor + 1);
                    query.replace_range(s..e, "");
                    text_changed = true;
                }
            }
            "left" => {
                if self.search_select_all {
                    self.search_cursor = 0;
                    self.search_select_all = false;
                } else {
                    self.search_cursor = self.search_cursor.saturating_sub(1);
                }
            }
            "right" => {
                if self.search_select_all {
                    self.search_cursor = char_len;
                    self.search_select_all = false;
                } else {
                    self.search_cursor = (self.search_cursor + 1).min(char_len);
                }
            }
            "home" => {
                self.search_cursor = 0;
                self.search_select_all = false;
            }
            "end" => {
                self.search_cursor = char_len;
                self.search_select_all = false;
            }
            _ => {
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                    && !event.keystroke.modifiers.alt
                {
                    let inserted = event
                        .keystroke
                        .key_char
                        .as_deref()
                        .or_else(|| (key.len() == 1).then_some(key))
                        .or_else(|| (key == "space").then_some(" "));
                    if let Some(text) = inserted {
                        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
                        if !clean.is_empty() {
                            if self.search_select_all {
                                query.clear();
                                self.search_cursor = 0;
                                self.search_select_all = false;
                            }
                            let b = byte_idx(&query, self.search_cursor);
                            query.insert_str(b, &clean);
                            self.search_cursor += clean.chars().count();
                            text_changed = true;
                        }
                    }
                }
            }
        }

        if text_changed {
            model.set_tray_query(&query);
            self.sync_from_model(model, window);
            let _ = model.pump_previews();
        }
        cx.notify();
    }
}

/// Renders the Film or Paper Stock Preview Tray (`186 px` tall) when open, or `None` when closed.
pub fn render_tray(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> Option<AnyElement> {
    app.tray.sync_from_model(&app.model, window);
    let paper = app.model.tray_shown()?;
    let visit = app.model.tray()?.clone();

    // Pump previews whenever the tray is visible so tiles develop immediately.
    let _ = app.model.pump_previews();

    // Advance any active smooth group-index glide and request next frame while animating.
    if app.tray.step_glide(Instant::now()) {
        window.request_animation_frame();
    }

    let sections = app.model.tray_sections();
    let starts = rules::starts(&sections);
    let current = app.model.tray_current(paper, visit.read_as);
    let before_name = app.model.tray_before_name();
    let scroll_x = app.tray.scroll_x;
    let viewport_w = app.tray.viewport_width;
    let hovered = app.tray.hovered_tile;
    let keep_paper = app.model.prefs.keep_paper;
    let theme = app.theme.clone();

    // Film-surface tokens (stay high-contrast dark on Contact Sheet's black film ground).
    let f = theme.tokens.film;
    let film_bg = theme.film;
    let film_blank = hex(f.blank);
    let film_line = hex(f.line);
    let film_paper = hex(f.paper);
    let film_label = hex(f.label);
    let film_dim = hex(f.dim);
    let film_accent = hex(f.accent);

    // Resolve visible tile RenderImages through the VRAM-safe TileImageCache.
    app.tray.tile_cache.begin_frame();
    let visible = visible_tile_indices(&sections, scroll_x, viewport_w);

    let mut tile_elements: Vec<AnyElement> = Vec::with_capacity(visible.len() + sections.len());

    // Section headers & subtle vertical dividers aligned with `tray_rules::starts`.
    for (si, (section, &start)) in sections.iter().zip(&starts).enumerate() {
        let sec_x = start - scroll_x;
        let sec_w = rules::width(section);
        if sec_x + sec_w < -rules::TILE || sec_x > viewport_w + rules::TILE {
            continue;
        }
        let title_x = rules::tile_left(start, 0) - scroll_x;
        // Vertical group separator hairline at the start of each group (except the first).
        if si > 0 {
            tile_elements.push(
                div()
                    .absolute()
                    .left(px((sec_x + 4.0) as f32))
                    .top(px(18.0))
                    .w(px(1.0))
                    .h(px(rules::PICTURE as f32))
                    .bg(film_line.opacity(0.7))
                    .into_any_element(),
            );
        }
        // Editorial section title above the group's first tile.
        tile_elements.push(
            div()
                .absolute()
                .left(px(title_x as f32))
                .top(px(2.0))
                .h(px(15.0))
                .font_family(theme.serif_family.clone())
                .italic()
                .text_size(px(11.5))
                .text_color(film_label.opacity(0.85))
                .child(section.title.clone())
                .into_any_element(),
        );
    }

    // Visible tiles (exact `TILE = 212` × `PICTURE = 118` geometry).
    for (si, i, screen_x) in visible {
        let section = &sections[si];
        let entry = &section.stocks[i];
        let is_your_negative = entry.id == rules::YOUR_NEGATIVE;
        let is_selected = entry.id == current;
        let is_before = visit.shows_before(&entry.id, &current);
        let is_fav = !is_your_negative && app.model.prefs.is_favorite(&entry.id, paper);
        let is_hot = hovered == Some((si, i));
        let in_favourites = section.favourites;

        let thumb_info = if is_your_negative {
            None
        } else {
            app.model
                .previews
                .thumb(&Target::Stock {
                    id: entry.id.clone(),
                    paper,
                })
                .and_then(|thumb| {
                    thumb.image.as_ref().map(|bmp| {
                        let img = app.tray.tile_cache.get_or_insert(bmp);
                        (img, bmp.width, bmp.height, thumb.dimmed)
                    })
                })
        };

        let stock_id_click = entry.id.clone();
        let stock_id_star = entry.id.clone();
        let on_photo = theme.on_photo;

        let picture_box = div()
            .relative()
            .w(px(rules::TILE as f32))
            .h(px(rules::PICTURE as f32))
            .rounded(px(3.0))
            .overflow_hidden()
            .bg(film_blank)
            .when(is_selected, |el| el.border_2().border_color(film_accent))
            .when(!is_selected, |el| {
                el.border_1().border_color(film_line.opacity(0.55))
            })
            .when(is_your_negative, |el| {
                el.flex()
                    .items_center()
                    .justify_center()
                    .px(px(16.0))
                    .child(
                        div()
                            .font_family(theme.serif_family.clone())
                            .italic()
                            .text_size(px(14.0))
                            .line_height(px(18.0))
                            .text_center()
                            .text_color(film_label)
                            .child(negs_app::negative::TILE_TEXT),
                    )
            })
            .when_some(thumb_info, |el, (render_img, img_w, img_h, dimmed)| {
                el.child(
                    canvas(
                        |_bounds, _window, _cx| {},
                        move |bounds: Bounds<Pixels>, (), window, _cx| {
                            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                                let draw_bounds = cover_bounds(bounds, img_w, img_h);
                                let _ = window.paint_image(
                                    draw_bounds,
                                    Corners::default(),
                                    render_img.clone(),
                                    0,
                                    false,
                                );
                                if dimmed {
                                    window.paint_quad(fill(bounds, film_blank.opacity(0.45)));
                                }
                            });
                        },
                    )
                    .size_full(),
                )
            })
            .when(!is_your_negative && (is_fav || is_hot), |el| {
                el.child(render_star_button(
                    si,
                    i,
                    is_fav,
                    film_accent,
                    on_photo,
                    stock_id_star,
                    paper,
                    cx,
                ))
            });

        let name_color = if is_selected {
            film_paper
        } else if is_before || is_hot {
            film_label
        } else {
            film_dim
        };

        let name_row = div()
            .flex()
            .items_baseline()
            .gap(px(6.0))
            .w(px(rules::TILE as f32))
            .h(px(18.0))
            .mt(px(2.0))
            .overflow_hidden()
            .child(
                div()
                    .font_family(theme.serif_family.clone())
                    .text_size(px(13.5))
                    .when(is_selected, |el| el.font_weight(FontWeight::SEMIBOLD))
                    .text_color(name_color)
                    .truncate()
                    .child(entry.name.clone()),
            )
            .when(is_before, |el| {
                el.child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .italic()
                        .text_size(px(12.0))
                        .text_color(film_dim)
                        .flex_shrink_0()
                        .child("before"),
                )
            });

        tile_elements.push(
            div()
                .id(SharedString::from(format!("tray-tile-{si}-{i}")))
                .absolute()
                .left(px(screen_x as f32))
                .top(px(18.0))
                .w(px(rules::TILE as f32))
                .flex()
                .flex_col()
                .cursor_pointer()
                .on_hover(cx.listener(move |this, hovering: &bool, _window, cx| {
                    if *hovering {
                        this.tray.hovered_tile = Some((si, i));
                        cx.notify();
                    } else if this.tray.hovered_tile == Some((si, i)) {
                        this.tray.hovered_tile = None;
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        this.model.choose_tile(&stock_id_click, in_favourites);
                        this.tray.sync_from_model(&this.model, window);
                        let _ = this.model.pump_previews();
                        cx.notify();
                    }),
                )
                .child(picture_box)
                .child(name_row)
                .into_any_element(),
        );
    }

    app.tray.tile_cache.retain_used(window);

    let entity = cx.entity().clone();
    let width_tracker = canvas(
        {
            let entity = entity.clone();
            move |bounds: Bounds<Pixels>, _window, cx| {
                let w = f32::from(bounds.size.width) as f64;
                if w > 1.0 {
                    entity.update(cx, |this, _cx| {
                        if (this.tray.viewport_width - w).abs() > 0.5 {
                            this.tray.viewport_width = w;
                            let sections = this.model.tray_sections();
                            let content = rules::content_width(&sections);
                            this.tray.scroll_x = rules::clamp(this.tray.scroll_x, w, content);
                        }
                    });
                }
            }
        },
        |_bounds, (), _window, _cx| {},
    )
    .absolute()
    .inset_0();

    let tile_row = div()
        .id("tray-tile-row")
        .relative()
        .w_full()
        .h(px(rules::ROW as f32))
        .bg(film_bg)
        .border_t_1()
        .border_color(theme.line)
        .overflow_hidden()
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
            let (dx, dy) = match event.delta {
                ScrollDelta::Pixels(p) => (f32::from(p.x) as f64, f32::from(p.y) as f64),
                ScrollDelta::Lines(l) => (
                    l.x as f64 * rules::WHEEL_NOTCH * 0.5,
                    l.y as f64 * rules::WHEEL_NOTCH * 0.5,
                ),
            };
            let sections = this.model.tray_sections();
            this.tray.scroll_by_wheel(dx, dy, &sections);
            cx.notify();
        }))
        .child(width_tracker)
        .when(sections.is_empty(), |el| {
            el.flex().items_center().justify_center().child(
                div()
                    .font_family(theme.serif_family.clone())
                    .italic()
                    .text_size(px(15.0))
                    .text_color(film_dim)
                    .child(rules::no_match(&visit.query, paper)),
            )
        })
        .when(!sections.is_empty(), |el| el.children(tile_elements));

    let foot_bar = render_foot_bar(
        app,
        paper,
        visit.read_as,
        &visit.query,
        &sections,
        active_group_index(scroll_x, &sections),
        before_name.as_deref(),
        keep_paper,
        &theme,
        window,
        cx,
    );

    Some(
        div()
            .id("stock-preview-tray")
            .flex()
            .flex_col()
            .w_full()
            .h(px(rules::HEIGHT as f32))
            .flex_shrink_0()
            .child(tile_row)
            .child(foot_bar)
            .into_any_element(),
    )
}

#[allow(clippy::too_many_arguments)]
fn render_star_button(
    si: usize,
    i: usize,
    favourite: bool,
    accent: Hsla,
    on_photo: Hsla,
    stock_id: String,
    paper: bool,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let star_canvas = canvas(
        |_bounds, _window, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, _cx| {
            paint_favourite_star(window, bounds, favourite, accent, on_photo);
        },
    )
    .size_full();

    div()
        .id(SharedString::from(format!("tray-star-{si}-{i}")))
        .absolute()
        .top(px(rules::STAR_INSET as f32))
        .right(px(rules::STAR_INSET as f32))
        .w(px(rules::STAR as f32))
        .h(px(rules::STAR as f32))
        .cursor_pointer()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                this.model.toggle_favorite(&stock_id, paper);
                this.tray.sync_from_model(&this.model, window);
                cx.notify();
            }),
        )
        .child(star_canvas)
}

/// Paints the 10-point favourite star vector inside `bounds` (20×20 box),
/// matching `windows/src/ui/tray.rs`.
fn paint_favourite_star(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    favourite: bool,
    accent: Hsla,
    on_photo: Hsla,
) {
    const POINTS: [(f32, f32); 10] = [
        (10.0, 2.6),
        (12.2, 7.5),
        (17.4, 8.0),
        (13.5, 11.5),
        (14.6, 16.7),
        (10.0, 14.0),
        (5.4, 16.7),
        (6.5, 11.5),
        (2.6, 8.0),
        (7.8, 7.5),
    ];

    let build_star = |stroke_width: Option<f32>| {
        let mut builder = match stroke_width {
            Some(w) => PathBuilder::stroke(px(w)),
            None => PathBuilder::fill(),
        };
        for (idx, &(sx, sy)) in POINTS.iter().enumerate() {
            let p = point(bounds.origin.x + px(sx), bounds.origin.y + px(sy));
            if idx == 0 {
                builder.move_to(p);
            } else {
                builder.line_to(p);
            }
        }
        builder.close();
        builder.build().ok()
    };

    // Soft dark halo so the star stays legible over bright highlight skies.
    if let Some(shadow) = build_star(Some(3.0)) {
        window.paint_path(shadow, Hsla::black().opacity(0.5));
    }
    if favourite {
        if let Some(filled) = build_star(None) {
            window.paint_path(filled, accent);
        }
        if let Some(outline) = build_star(Some(1.0)) {
            window.paint_path(outline, accent);
        }
    } else if let Some(outline) = build_star(Some(1.4)) {
        window.paint_path(outline, on_photo.opacity(0.92));
    }
}

#[allow(clippy::too_many_arguments)]
fn render_foot_bar(
    app: &NegsApp,
    paper: bool,
    read_as: bool,
    query: &str,
    sections: &[Section],
    active_group: usize,
    before_name: Option<&str>,
    keep_paper: bool,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let searching = rules::searching(query);
    let hover = theme.subtle_hover;

    // Left: Group index (or match count while searching).
    let left = if searching {
        let count: usize = sections.iter().map(|s| s.stocks.len()).sum();
        div()
            .font_family(theme.serif_family.clone())
            .italic()
            .text_size(px(12.5))
            .text_color(theme.label)
            .child(rules::matches(count))
            .into_any_element()
    } else {
        div()
            .flex()
            .items_center()
            .gap(px(14.0))
            .overflow_hidden()
            .children(sections.iter().enumerate().map(|(i, sec)| {
                let is_active = i == active_group;
                let color = if is_active { theme.paper } else { theme.dim };
                div()
                    .id(SharedString::from(format!("tray-group-idx-{i}")))
                    .h(px(rules::FOOT as f32))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .when(is_active, |el| {
                        el.border_b_2().border_color(theme.accent)
                    })
                    .font_family(theme.serif_family.clone())
                    .italic()
                    .text_size(px(12.5))
                    .text_color(color)
                    .hover({
                        let paper_col = theme.paper;
                        move |s| s.text_color(paper_col)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _event: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                            let sections = this.model.tray_sections();
                            this.tray.jump_to_section(i, &sections);
                            cx.notify();
                        }),
                    )
                    .child(sec.title.clone())
            }))
            .into_any_element()
    };

    // Center: Hint text.
    let center = div()
        .font_family(theme.serif_family.clone())
        .italic()
        .text_size(px(12.5))
        .text_color(theme.dim)
        .truncate()
        .child(rules::hint(before_name));

    // Right: Film Search box or Paper Lock toggle, plus Done button.
    let mut right = div().flex().items_center().gap(px(12.0)).flex_shrink_0();

    if paper {
        let lock_words = rules::keep_paper_words(keep_paper);
        let lock_color = if keep_paper { theme.paper } else { theme.dim };
        let dot_color = if keep_paper {
            theme.accent
        } else {
            theme.track
        };
        right = right.child(
            div()
                .id("tray-keep-paper")
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _event: &MouseDownEvent, _window, cx| {
                        cx.stop_propagation();
                        let next = !this.model.prefs.keep_paper;
                        this.model.set_keep_paper(next);
                        let _ = this.model.pump_previews();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .w(px(7.0))
                        .h(px(7.0))
                        .rounded(px(4.0))
                        .bg(dot_color),
                )
                .child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .italic()
                        .text_size(px(12.5))
                        .text_color(lock_color)
                        .child(lock_words),
                ),
        );
    } else if !read_as {
        right = right.child(render_search_box(app, query, theme, window, cx));
    }

    right = right.child(
        div()
            .id("tray-done-btn")
            .px(px(8.0))
            .py(px(2.0))
            .rounded(px(4.0))
            .cursor_pointer()
            .hover(move |s| s.bg(hover))
            .font_family(theme.serif_family.clone())
            .font_weight(FontWeight::SEMIBOLD)
            .text_size(px(13.5))
            .text_color(theme.paper)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    this.model.end_tray(true);
                    this.tray.sync_from_model(&this.model, window);
                    cx.notify();
                }),
            )
            .child("Done"),
    );

    div()
        .flex()
        .items_center()
        .justify_between()
        .gap(px(12.0))
        .w_full()
        .h(px(rules::FOOT as f32))
        .px(px(16.0))
        .bg(theme.panel)
        .border_t_1()
        .border_color(theme.line)
        .child(left)
        .child(center)
        .child(right)
}

fn render_search_box(
    app: &NegsApp,
    query: &str,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let focused = app.tray.is_search_focused(window);
    let focus_handle = app.tray.search_focus.clone();
    let cursor = app.tray.search_cursor.min(query.chars().count());
    let split = query
        .char_indices()
        .nth(cursor)
        .map_or(query.len(), |(b, _)| b);
    let before = query[..split].to_string();
    let after = query[split..].to_string();
    let select_all = app.tray.search_select_all && !query.is_empty();
    let accent = theme.accent;

    div()
        .id("tray-search-box")
        .track_focus(&focus_handle)
        .key_context("TextInput")
        .flex()
        .items_center()
        .justify_between()
        .w(px(148.0))
        .h(px(20.0))
        .px(px(4.0))
        .border_b_1()
        .border_color(if focused { accent } else { theme.track })
        .font_family(theme.ui_family.clone())
        .text_size(px(12.0))
        .cursor_text()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.tray.search_focus);
                cx.notify();
            }),
        )
        .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
            cx.stop_propagation();
            this.tray
                .handle_search_key(event, &mut this.model, window, cx);
        }))
        .child(if query.is_empty() && !focused {
            div()
                .text_color(theme.dim.opacity(0.8))
                .child("Search films")
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .text_color(theme.paper)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .when(select_all, |el| {
                            el.bg(accent.opacity(0.32)).rounded(px(2.0))
                        })
                        .child(before)
                        .when(focused && !select_all, |el| {
                            el.child(div().w(px(1.5)).h(px(13.0)).bg(accent))
                        })
                        .child(after),
                )
                .into_any_element()
        })
        .when(!query.is_empty(), |el| {
            el.child(
                div()
                    .id("tray-search-clear")
                    .pl(px(4.0))
                    .cursor_pointer()
                    .text_size(px(12.0))
                    .text_color(theme.dim)
                    .hover({
                        let p = theme.paper;
                        move |s| s.text_color(p)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            this.model.set_tray_query("");
                            this.tray.search_cursor = 0;
                            this.tray.search_select_all = false;
                            this.tray.sync_from_model(&this.model, window);
                            let _ = this.model.pump_previews();
                            cx.notify();
                        }),
                    )
                    .child("×"),
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::tray_rules::{Cancel, Entry, Submit};

    fn sample_sections() -> Vec<Section> {
        rules::sections(
            vec![
                Section::group(
                    "Color Negative",
                    vec![
                        Entry::new("gold", "Kodak Gold 200"),
                        Entry::new("portra400", "Kodak Portra 400"),
                        Entry::new("ektar", "Kodak Ektar 100"),
                    ],
                ),
                Section::group(
                    "Black & White",
                    vec![
                        Entry::new("hp5", "Ilford HP5 Plus"),
                        Entry::new("trix", "Kodak Tri-X 400"),
                    ],
                ),
            ],
            vec![Entry::new("portra400", "Kodak Portra 400")],
            "",
        )
    }

    #[test]
    fn wheel_scroll_moves_sideways_and_clamps() {
        let sections = sample_sections();
        let content = rules::content_width(&sections);
        let visible = 500.0;
        let max_scroll = (content - visible).max(0.0);

        // Downward vertical wheel (dy < 0) advances right; upward (dy > 0) moves left.
        let s1 = wheel_scroll_offset(0.0, 0.0, -120.0, visible, content);
        assert_eq!(s1, 120.0);
        let s2 = wheel_scroll_offset(s1, 0.0, 60.0, visible, content);
        assert_eq!(s2, 60.0);
        // Horizontal trackpad dominates when |dx| > |dy|.
        let s3 = wheel_scroll_offset(60.0, -200.0, 10.0, visible, content);
        assert_eq!(s3, 260.0);
        // Clamped at both ends.
        assert_eq!(wheel_scroll_offset(0.0, 0.0, 500.0, visible, content), 0.0);
        assert_eq!(
            wheel_scroll_offset(max_scroll, 0.0, -500.0, visible, content),
            max_scroll
        );
    }

    #[test]
    fn active_group_index_follows_scroll_position() {
        let sections = sample_sections();
        let starts = rules::starts(&sections);
        assert_eq!(active_group_index(0.0, &sections), 0);
        assert_eq!(active_group_index(starts[1], &sections), 1);
        assert_eq!(active_group_index(starts[2], &sections), 2);
    }

    #[test]
    fn glide_and_reveal_follow_tray_rules() {
        let sections = sample_sections();
        let visible = 500.0;
        let content = rules::content_width(&sections);

        // Opening centers current tile.
        let centered = rules::reveal(0.0, "ektar", false, true, &sections, visible);
        let expected_x = rules::tile_x("ektar", 1, &sections).unwrap();
        assert_eq!(centered, rules::centring(expected_x, visible, content));

        // Stepping leaves offset alone if tile is already visible.
        assert_eq!(
            rules::reveal(centered, "ektar", false, false, &sections, visible),
            centered
        );

        // Glide eases smoothly over 250 ms and lands exactly at target.
        assert_eq!(rules::glide(0.0, 400.0, 0.0), 0.0);
        assert_eq!(rules::glide(0.0, 400.0, rules::GLIDE_MS / 2.0), 200.0);
        assert_eq!(rules::glide(0.0, 400.0, rules::GLIDE_MS), 400.0);
    }

    #[test]
    fn keyboard_resolution_and_search_submit_cancel_match_tray_rules() {
        assert_eq!(resolve_tray_key("left", false, false), TrayKey::Step(-1));
        assert_eq!(resolve_tray_key("right", false, false), TrayKey::Step(1));
        assert_eq!(resolve_tray_key("enter", false, false), TrayKey::Keep);
        assert_eq!(resolve_tray_key("escape", false, false), TrayKey::GoBack);
        assert_eq!(resolve_tray_key("p", false, false), TrayKey::Pass);
        // Typing or command modifier passes through.
        assert_eq!(resolve_tray_key("left", false, true), TrayKey::Pass);
        assert_eq!(resolve_tray_key("escape", true, false), TrayKey::Pass);

        assert_eq!(rules::submit(""), Submit::Keep);
        assert_eq!(rules::submit("portra"), Submit::TryFirst);
        assert_eq!(rules::cancel(""), Cancel::GoBack);
        assert_eq!(rules::cancel("portra"), Cancel::Clear);
    }

    #[test]
    fn visible_tile_indices_culls_offscreen_tiles() {
        let sections = sample_sections();
        let all_count: usize = sections.iter().map(|s| s.stocks.len()).sum();
        let at_start = visible_tile_indices(&sections, 0.0, 260.0);
        assert!(!at_start.is_empty());
        assert!(at_start.len() < all_count, "far-right tiles should be culled");
        assert_eq!(at_start[0].0, 0);
        assert_eq!(at_start[0].1, 0);
    }
}
