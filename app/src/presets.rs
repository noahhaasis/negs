//! Presets Sidebar (`PresetsState` + `render_presets`).
//!
//! Two-column darkroom editorial grid of presets with color-managed live
//! thumbnails (`TileImageCache`), Newsreader typography, active grease-pencil
//! accent ring, inline rename with live `negs_app::presets::name::problem`
//! validation, pointer drag-to-reorder, external `.json` file drag-and-drop,
//! right-click context menu, and empty state.

use std::path::{Path, PathBuf};
use std::process::Command;

use gpui::{
    Bounds, ClipboardItem, ContentMask, Context, Corners, ExternalPaths, FocusHandle, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    PathPromptOptions, Pixels, Point, SharedString, Window, canvas, div, fill, prelude::*, px,
};
use negs_app::model::Model;
use negs_app::presets::{Library, PresetFile, Status as PresetStatus, name as preset_name};
use negs_app::previews::Target;
use negs_app::state::Stock;

use crate::NegsApp;
use crate::theme_gpui::GpuiTheme;
use crate::tile_cache::{TileImageCache, cover_bounds};

/// Recommended width for the right-hand Presets sidebar in DIPs.
pub const PRESETS_SIDEBAR_WIDTH: f32 = 332.0;

const COLUMN_GAP: f32 = 14.0;
const ROW_GAP: f32 = 18.0;
const SIDE_GUTTER: f32 = 16.0;
const FOOT_HEIGHT: f32 = 34.0;
const DRAG_THRESHOLD_PX: f32 = 6.0;
const CONTEXT_MENU_WIDTH: f32 = 184.0;
const CONTEXT_MENU_HEIGHT: f32 = 176.0;

/// Formatted detail lines shown beneath a preset's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetDetail {
    Valid {
        film: String,
        paper_lead: &'static str,
        paper_name: String,
    },
    Invalid(String),
    Checking,
}

/// Formats a preset's film/paper or invalid status into its display lines.
pub fn format_preset_detail(status: Option<&PresetStatus>, stocks: &[Stock]) -> PresetDetail {
    let stock_name = |id: &str, paper: bool| -> String {
        stocks
            .iter()
            .find(|s| s.paper == paper && s.id == id)
            .map_or_else(|| id.to_owned(), |s| s.name.clone())
    };
    match status {
        Some(PresetStatus::Valid {
            film, paper, scan, ..
        }) => {
            let film_display = stock_name(film, false);
            if *scan {
                PresetDetail::Valid {
                    film: film_display,
                    paper_lead: "not printed · film scan",
                    paper_name: String::new(),
                }
            } else {
                PresetDetail::Valid {
                    film: film_display,
                    paper_lead: "printed on",
                    paper_name: stock_name(paper, true),
                }
            }
        }
        Some(PresetStatus::Invalid(reason)) => PresetDetail::Invalid(reason.clone()),
        Some(PresetStatus::Checking) | None => PresetDetail::Checking,
    }
}

/// Reorders a slice by moving the item at `from` to index `to` (clamped to bounds).
pub fn reorder_presets<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    if items.is_empty() || from >= items.len() {
        return items.to_vec();
    }
    let target = to.min(items.len() - 1);
    if from == target {
        return items.to_vec();
    }
    let mut out = items.to_vec();
    let moved = out.remove(from);
    out.insert(target, moved);
    out
}

/// Filters external dropped paths down to `.json` preset files (case-insensitive).
pub fn filter_json_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths
        .iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("json"))
        })
        .cloned()
        .collect()
}

/// Validates a draft preset name using `negs_app::presets::name::problem`.
pub fn validate_preset_name(draft: &str) -> Option<String> {
    preset_name::problem(draft.trim())
}

/// Footer bar copy: normal hint vs drop-target highlight.
pub fn footer_hint(drop_active: bool) -> &'static str {
    if drop_active {
        "Drop .json files to import"
    } else {
        "Drag to reorder · Right-click for options"
    }
}

/// Self-contained UTF-8-safe single-line text editor state used for inline rename.
#[derive(Clone, Debug)]
struct InlineRename {
    file: PresetFile,
    draft: String,
    /// Cursor position in Unicode scalar (`char`) indices.
    cursor: usize,
    select_all: bool,
    error: Option<String>,
    needs_focus: bool,
}

impl InlineRename {
    fn new(file: PresetFile) -> Self {
        let draft = file.name();
        let cursor = draft.chars().count();
        Self {
            file,
            draft,
            cursor,
            select_all: true,
            error: None,
            needs_focus: true,
        }
    }

    fn char_len(&self) -> usize {
        self.draft.chars().count()
    }

    fn byte_index(&self, char_idx: usize) -> usize {
        self.draft
            .char_indices()
            .nth(char_idx)
            .map_or(self.draft.len(), |(b, _)| b)
    }

    fn clear_if_selected_all(&mut self) {
        if self.select_all {
            self.draft.clear();
            self.cursor = 0;
            self.select_all = false;
        }
    }

    fn insert_str(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| !c.is_control()).collect();
        if clean.is_empty() {
            return;
        }
        self.clear_if_selected_all();
        let byte_pos = self.byte_index(self.cursor);
        self.draft.insert_str(byte_pos, &clean);
        self.cursor += clean.chars().count();
        self.revalidate();
    }

    fn backspace(&mut self) {
        if self.select_all {
            self.draft.clear();
            self.cursor = 0;
            self.select_all = false;
        } else if self.cursor > 0 {
            let start = self.byte_index(self.cursor - 1);
            let end = self.byte_index(self.cursor);
            self.draft.replace_range(start..end, "");
            self.cursor -= 1;
        }
        self.revalidate();
    }

    fn delete(&mut self) {
        if self.select_all {
            self.draft.clear();
            self.cursor = 0;
            self.select_all = false;
        } else if self.cursor < self.char_len() {
            let start = self.byte_index(self.cursor);
            let end = self.byte_index(self.cursor + 1);
            self.draft.replace_range(start..end, "");
        }
        self.revalidate();
    }

    fn move_left(&mut self) {
        if self.select_all {
            self.cursor = 0;
            self.select_all = false;
        } else {
            self.cursor = self.cursor.saturating_sub(1);
        }
    }

    fn move_right(&mut self) {
        if self.select_all {
            self.cursor = self.char_len();
            self.select_all = false;
        } else {
            self.cursor = (self.cursor + 1).min(self.char_len());
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
        self.select_all = false;
    }

    fn move_end(&mut self) {
        self.cursor = self.char_len();
        self.select_all = false;
    }

    fn revalidate(&mut self) {
        self.error = validate_preset_name(&self.draft);
    }

    fn before_and_after_cursor(&self) -> (&str, &str) {
        let split = self.byte_index(self.cursor);
        (&self.draft[..split], &self.draft[split..])
    }
}

#[derive(Clone, Debug)]
struct DragReorder {
    from: usize,
    over: usize,
}

#[derive(Clone, Debug)]
struct ContextMenuState {
    file: PresetFile,
    /// Position in window coordinates, converted to sidebar-local on render.
    window_pos: Point<Pixels>,
}

/// Interactive state for the Presets sidebar.
pub struct PresetsState {
    pub tile_cache: TileImageCache,
    rename_focus: FocusHandle,
    renaming: Option<InlineRename>,
    press: Option<(usize, Point<Pixels>)>,
    drag: Option<DragReorder>,
    context_menu: Option<ContextMenuState>,
    selected_index: Option<usize>,
    drop_active: bool,
    last_pane_bounds: Bounds<Pixels>,
}

impl PresetsState {
    pub fn new(cx: &mut Context<NegsApp>) -> Self {
        Self {
            tile_cache: TileImageCache::new(),
            rename_focus: cx.focus_handle(),
            renaming: None,
            press: None,
            drag: None,
            context_menu: None,
            selected_index: None,
            drop_active: false,
            last_pane_bounds: Bounds::default(),
        }
    }

    pub fn is_renaming(&self) -> bool {
        self.renaming.is_some()
    }

    pub fn is_context_menu_open(&self) -> bool {
        self.context_menu.is_some()
    }

    pub fn close_context_menu(&mut self) {
        self.context_menu = None;
    }

    /// Releases all cached preset thumbnail `RenderImage`s from GPUI's atlas.
    pub fn clear_cache(&mut self, window: &mut Window) {
        self.tile_cache.clear(window);
    }

    /// Begins inline renaming of `file` on its tile name line.
    pub fn begin_rename(
        &mut self,
        file: &PresetFile,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        self.context_menu = None;
        self.drag = None;
        self.press = None;
        self.renaming = Some(InlineRename::new(file.clone()));
        window.focus(&self.rename_focus);
        cx.notify();
    }

    /// Commits the current inline rename if valid; otherwise keeps editing with inline error.
    pub fn commit_rename(
        &mut self,
        model: &mut Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        let Some(mut draft) = self.renaming.take() else {
            return;
        };
        draft.revalidate();
        if draft.error.is_some() {
            // Keep editing so the user sees the live validation warning.
            self.renaming = Some(draft);
            cx.notify();
            return;
        }
        let trimmed = draft.draft.trim().to_owned();
        model.rename_preset(&draft.file, &trimmed);
        window.blur();
        cx.notify();
    }

    /// Cancels the current inline rename, calling `model.cancel_rename(&file)`.
    pub fn cancel_rename(
        &mut self,
        model: &mut Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        let Some(draft) = self.renaming.take() else {
            return;
        };
        model.cancel_rename(&draft.file);
        window.blur();
        cx.notify();
    }

    /// Synchronizes local UI state with `Model` (e.g. auto-starting inline rename
    /// when `model.new_preset()` sets `model.renaming_preset`).
    pub fn sync_from_model(
        &mut self,
        model: &Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        if let Some(target_path) = model.renaming_preset.as_ref() {
            let already_renaming = self
                .renaming
                .as_ref()
                .is_some_and(|r| &r.file.path == target_path);
            if !already_renaming {
                if let Some(file) = model.presets.iter().find(|p| &p.path == target_path) {
                    self.renaming = Some(InlineRename::new(file.clone()));
                }
            }
        }
        if let Some(rename) = self.renaming.as_mut() {
            if rename.needs_focus {
                rename.needs_focus = false;
                window.focus(&self.rename_focus);
            }
        }
        if let Some(menu) = self.context_menu.as_ref() {
            if !model.presets.contains(&menu.file) {
                self.context_menu = None;
            }
        }
        if let Some(sel) = self.selected_index {
            if sel >= model.presets.len() {
                self.selected_index = None;
            }
        }
        let _ = cx;
    }

    /// Handles keyboard shortcuts relevant to the Presets sidebar (`F2` to rename,
    /// `Escape` to dismiss context menu). Returns `true` if the key was handled.
    pub fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        model: &mut Model,
        window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) -> bool {
        if self.renaming.is_some() {
            return false;
        }
        if event.keystroke.key == "escape" && self.context_menu.is_some() {
            self.context_menu = None;
            cx.notify();
            return true;
        }
        if event.keystroke.key == "f2" && model.can_change_presets() {
            let target = self
                .selected_index
                .and_then(|i| model.presets.get(i))
                .or_else(|| {
                    model
                        .applied_preset()
                        .and_then(|p| model.presets.iter().find(|f| &f.path == p))
                })
                .or_else(|| model.presets.first())
                .cloned();
            if let Some(file) = target {
                self.begin_rename(&file, window, cx);
                return true;
            }
        }
        false
    }

    /// Prompts the user to pick one or more `.json` preset files and imports them.
    /// Designed so `PaneHeader` ("Import…") and the empty state can share one entry point.
    pub fn prompt_import_presets(_window: &mut Window, cx: &mut Context<NegsApp>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(SharedString::from("Import Presets")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                let json_paths = filter_json_paths(&paths);
                if !json_paths.is_empty() {
                    let _ = this.update(cx, |app, cx| {
                        app.model.import_presets(json_paths, None);
                        app.model.pump_previews();
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// Prompts the user for a destination `.json` path and exports `file` byte-for-byte.
    pub fn prompt_export_preset(
        file: PresetFile,
        _window: &mut Window,
        cx: &mut Context<NegsApp>,
    ) {
        let suggested = format!("{}.json", file.name());
        let dir = negs_app::platform::pictures_dir();
        let receiver = cx.prompt_for_new_path(&dir, Some(&suggested));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(dest))) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    app.model.export_preset(&file, &dest);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Opens the Presets folder in the OS file manager (or reveals `target` if provided).
    pub fn show_presets_folder(target: Option<&Path>) {
        let folder = Library::default().folder;
        let _ = std::fs::create_dir_all(&folder);
        #[cfg(target_os = "macos")]
        {
            if let Some(file) = target.filter(|p| p.exists()) {
                let _ = Command::new("open").arg("-R").arg(file).spawn();
            } else {
                let _ = Command::new("open").arg(&folder).spawn();
            }
        }
        #[cfg(target_os = "windows")]
        {
            if let Some(file) = target.filter(|p| p.exists()) {
                let _ = Command::new("explorer.exe")
                    .arg(format!("/select,{}", file.display()))
                    .spawn();
            } else {
                let _ = Command::new("explorer.exe").arg(&folder).spawn();
            }
        }
        #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
        {
            let _ = target;
            let _ = Command::new("xdg-open").arg(&folder).spawn();
        }
    }

    fn handle_rename_key(
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
            self.commit_rename(model, window, cx);
            return;
        }
        if key == "escape" {
            self.cancel_rename(model, window, cx);
            return;
        }

        let Some(rename) = self.renaming.as_mut() else {
            return;
        };

        if cmd {
            match key {
                "a" => {
                    rename.select_all = true;
                    rename.cursor = rename.char_len();
                }
                "c" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(rename.draft.clone()));
                }
                "x" => {
                    cx.write_to_clipboard(ClipboardItem::new_string(rename.draft.clone()));
                    rename.draft.clear();
                    rename.cursor = 0;
                    rename.select_all = false;
                    rename.revalidate();
                }
                "v" => {
                    if let Some(item) = cx.read_from_clipboard() {
                        if let Some(text) = item.text() {
                            rename.insert_str(&text);
                        }
                    }
                }
                _ => return,
            }
            cx.notify();
            return;
        }

        match key {
            "backspace" => rename.backspace(),
            "delete" => rename.delete(),
            "left" => rename.move_left(),
            "right" => rename.move_right(),
            "home" => rename.move_home(),
            "end" => rename.move_end(),
            _ => {
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                    && !event.keystroke.modifiers.alt
                {
                    if let Some(ch) = event.keystroke.key_char.as_deref() {
                        rename.insert_str(ch);
                    } else if key.len() == 1 {
                        rename.insert_str(key);
                    } else if key == "space" {
                        rename.insert_str(" ");
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        }
        cx.notify();
    }
}

/// Renders the Presets sidebar contents (tiles grid or empty state, footer bar,
/// and floating right-click context menu).
pub fn render_presets(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    // Sync rename requests from Model and report visible range so `pump_previews` develops tiles.
    app.presets.sync_from_model(&app.model, window, cx);
    let count = app.model.presets.len();
    app.model.set_visible_presets(0..count);
    let _ = app.model.pump_previews();

    let theme = app.theme.clone();
    let enabled = app.model.can_change_presets();
    let show_previews = app.model.prefs.previews;
    let applied_path = app.model.applied_preset().cloned();
    let files = app.model.presets.clone();
    let drag_state = app.presets.drag.clone();
    let drop_active = app.presets.drop_active;
    let context_menu = app.presets.context_menu.clone();
    let pane_bounds = app.presets.last_pane_bounds;

    // Resolve thumbnails through the shared VRAM-safe TileImageCache.
    app.presets.tile_cache.begin_frame();
    let mut tile_images: Vec<Option<(std::sync::Arc<gpui::RenderImage>, u32, u32, bool)>> =
        Vec::with_capacity(files.len());
    for file in &files {
        let is_invalid = matches!(
            app.model.preset_status.get(file),
            Some(PresetStatus::Invalid(_))
        );
        let entry = if show_previews && !is_invalid {
            app.model
                .previews
                .thumb(&Target::Preset(file.path.clone()))
                .and_then(|thumb| {
                    thumb.image.as_ref().map(|bmp| {
                        let img = app.presets.tile_cache.get_or_insert(bmp);
                        (img, bmp.width, bmp.height, thumb.dimmed)
                    })
                })
        } else {
            None
        };
        tile_images.push(entry);
    }
    app.presets.tile_cache.retain_used(window);

    let details: Vec<PresetDetail> = files
        .iter()
        .map(|f| format_preset_detail(app.model.preset_status.get(f), &app.model.stocks))
        .collect();

    let entity = cx.entity().clone();
    let bounds_tracker = canvas(
        {
            let entity = entity.clone();
            move |bounds, _window, cx| {
                entity.update(cx, |this, _cx| {
                    this.presets.last_pane_bounds = bounds;
                });
            }
        },
        |_bounds, (), _window, _cx| {},
    )
    .absolute()
    .inset_0();

    let body = if files.is_empty() {
        render_empty_state(&theme, enabled, cx).into_any_element()
    } else {
        let mut rows = Vec::new();
        for row_idx in (0..files.len()).step_by(2) {
            let left_tile = render_preset_tile(
                app,
                row_idx,
                &files[row_idx],
                &details[row_idx],
                tile_images[row_idx].clone(),
                applied_path.as_ref() == Some(&files[row_idx].path),
                show_previews,
                enabled,
                drag_state.as_ref(),
                &theme,
                cx,
            );
            let right_tile = if row_idx + 1 < files.len() {
                render_preset_tile(
                    app,
                    row_idx + 1,
                    &files[row_idx + 1],
                    &details[row_idx + 1],
                    tile_images[row_idx + 1].clone(),
                    applied_path.as_ref() == Some(&files[row_idx + 1].path),
                    show_previews,
                    enabled,
                    drag_state.as_ref(),
                    &theme,
                    cx,
                )
                .into_any_element()
            } else {
                div().flex_1().min_w_0().into_any_element()
            };

            rows.push(
                div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .gap(px(COLUMN_GAP))
                    .child(left_tile)
                    .child(right_tile),
            );
        }

        div()
            .id("presets-scroll-grid")
            .flex_1()
            .w_full()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(SIDE_GUTTER))
            .pt(px(16.0))
            .pb(px(20.0))
            .flex()
            .flex_col()
            .gap(px(ROW_GAP))
            .children(rows)
            .into_any_element()
    };

    let footer = div()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .h(px(FOOT_HEIGHT))
        .px(px(12.0))
        .border_t_1()
        .border_color(if drop_active {
            theme.accent
        } else {
            theme.line
        })
        .bg(theme.panel)
        .font_family(theme.serif_family.clone())
        .italic()
        .text_size(px(12.5))
        .text_color(if drop_active { theme.paper } else { theme.dim })
        .child(footer_hint(drop_active));

    div()
        .id("presets-sidebar-root")
        .relative()
        .flex()
        .flex_col()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(theme.panel)
        .when(drop_active, |el| {
            el.border_2().border_color(theme.accent)
        })
        .drag_over::<ExternalPaths>({
            let accent = theme.accent;
            move |style, _, _, _| style.border_2().border_color(accent)
        })
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            this.presets.drop_active = false;
            let json_paths = filter_json_paths(paths.paths());
            if !json_paths.is_empty() {
                this.model.import_presets(json_paths, None);
                let _ = this.model.pump_previews();
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseDownEvent, window, cx| {
                if this.presets.context_menu.is_some() {
                    this.presets.context_menu = None;
                    cx.notify();
                }
                if this.presets.is_renaming() {
                    this.presets.commit_rename(&mut this.model, window, cx);
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                this.presets.press = None;
                if let Some(drag) = this.presets.drag.take() {
                    if drag.from != drag.over {
                        let reordered = reorder_presets(&this.model.presets, drag.from, drag.over);
                        this.model.move_presets(reordered);
                    }
                    cx.notify();
                }
            }),
        )
        .child(bounds_tracker)
        .child(body)
        .when(!files.is_empty(), |el| el.child(footer))
        .when_some(context_menu, |el, menu| {
            el.child(render_context_menu(
                &menu,
                pane_bounds,
                &app.model,
                &theme,
                cx,
            ))
        })
}

#[allow(clippy::too_many_arguments)]
fn render_preset_tile(
    app: &NegsApp,
    index: usize,
    file: &PresetFile,
    detail: &PresetDetail,
    thumb: Option<(std::sync::Arc<gpui::RenderImage>, u32, u32, bool)>,
    applied: bool,
    show_previews: bool,
    enabled: bool,
    drag_state: Option<&DragReorder>,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement + use<> {
    let is_invalid = matches!(detail, PresetDetail::Invalid(_));
    let is_valid = matches!(detail, PresetDetail::Valid { .. });
    let is_renaming = app
        .presets
        .renaming
        .as_ref()
        .is_some_and(|r| r.file.path == file.path);
    let is_drag_source = drag_state.is_some_and(|d| d.from == index);
    let is_drag_target = drag_state.is_some_and(|d| d.from != index && d.over == index);

    let blank = theme.blank;
    let accent = theme.accent;
    let dim = theme.dim;
    let paper = theme.paper;
    let label = theme.label;
    let warn = theme.warn;
    let hover_bg = theme.subtle_hover;
    let serif = theme.serif_family.clone();

    let file_click = file.clone();
    let file_ctx = file.clone();

    let thumb_well = show_previews.then(|| {
        let thumb_canvas = thumb.map(|(render_img, img_w, img_h, dimmed)| {
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
                            window.paint_quad(fill(bounds, blank.opacity(0.45)));
                        }
                    });
                },
            )
            .size_full()
        });

        div()
            .relative()
            .w_full()
            .h(px(92.0))
            .rounded(px(4.0))
            .overflow_hidden()
            .bg(blank)
            .when(applied, |el| el.border_2().border_color(accent))
            .when(!applied, |el| {
                el.border_1().border_color(theme.line.opacity(0.75))
            })
            .when_some(thumb_canvas, |el, c| el.child(c))
            .when(is_invalid, |el| {
                el.flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .font_family(serif.clone())
                            .italic()
                            .text_size(px(13.0))
                            .text_color(dim)
                            .child("Unreadable"),
                    )
            })
    });

    let name_block = if is_renaming {
        let rename = app.presets.renaming.as_ref().unwrap();
        let (before, after) = rename.before_and_after_cursor();
        let before = before.to_string();
        let after = after.to_string();
        let select_all = rename.select_all;
        let error = rename.error.clone();
        let focus = app.presets.rename_focus.clone();

        div()
            .id("preset-rename-input")
            .track_focus(&focus)
            .key_context("TextInput")
            .w_full()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event: &MouseDownEvent, _window, cx| {
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                cx.stop_propagation();
                this.presets
                    .handle_rename_key(event, &mut this.model, window, cx);
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .w_full()
                    .min_h(px(20.0))
                    .px(px(3.0))
                    .py(px(1.0))
                    .bg(theme.canvas.opacity(0.6))
                    .border_b_2()
                    .border_color(if error.is_some() { warn } else { accent })
                    .font_family(serif.clone())
                    .text_size(px(15.0))
                    .text_color(paper)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .when(select_all, |el| {
                                el.bg(accent.opacity(0.32)).rounded(px(2.0))
                            })
                            .child(before)
                            .when(!select_all, |el| {
                                el.child(div().w(px(1.5)).h(px(15.0)).bg(accent))
                            })
                            .child(after),
                    ),
            )
            .when_some(error, |el, msg| {
                el.child(
                    div()
                        .font_family(theme.ui_family.clone())
                        .text_size(px(11.0))
                        .text_color(warn)
                        .child(msg),
                )
            })
            .into_any_element()
    } else {
        let name_color = if is_invalid {
            dim
        } else if applied {
            paper
        } else {
            label
        };
        div()
            .w_full()
            .font_family(serif.clone())
            .text_size(px(15.0))
            .when(applied, |el| el.font_weight(FontWeight::SEMIBOLD))
            .text_color(name_color)
            .truncate()
            .child(file.name())
            .into_any_element()
    };

    let detail_block = match detail {
        PresetDetail::Valid {
            film,
            paper_lead,
            paper_name,
        } => div()
            .flex()
            .flex_col()
            .w_full()
            .font_family(serif.clone())
            .text_size(px(12.5))
            .text_color(dim)
            .child(div().truncate().child(film.clone()))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(4.0))
                    .overflow_hidden()
                    .child(div().italic().child(*paper_lead))
                    .when(!paper_name.is_empty(), |el| {
                        el.child(div().truncate().child(paper_name.clone()))
                    }),
            )
            .into_any_element(),
        PresetDetail::Invalid(reason) => div()
            .w_full()
            .font_family(serif.clone())
            .italic()
            .text_size(px(12.5))
            .text_color(dim)
            .overflow_hidden()
            .max_h(px(34.0))
            .child(reason.clone())
            .into_any_element(),
        PresetDetail::Checking => div()
            .w_full()
            .h(px(32.0))
            .font_family(serif.clone())
            .italic()
            .text_size(px(12.0))
            .text_color(dim.opacity(0.65))
            .child("Checking…")
            .into_any_element(),
    };

    div()
        .id(SharedString::from(format!("preset-tile-{index}")))
        .relative()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(5.0))
        .p(px(4.0))
        .rounded(px(6.0))
        .when(is_drag_source, |el| el.opacity(0.45))
        .when(!enabled, |el| el.opacity(0.38))
        .when(is_drag_target, |el| {
            el.border_l_2().border_color(accent)
        })
        .when(enabled && !is_renaming, |el| {
            el.cursor_pointer().hover(move |s| s.bg(hover_bg))
        })
        .on_drop(cx.listener(move |this, paths: &ExternalPaths, _window, cx| {
            cx.stop_propagation();
            this.presets.drop_active = false;
            let json_paths = filter_json_paths(paths.paths());
            if !json_paths.is_empty() {
                this.model.import_presets(json_paths, Some(index));
                let _ = this.model.pump_previews();
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                if this.presets.context_menu.is_some() {
                    this.presets.context_menu = None;
                }
                if this.presets.is_renaming() {
                    this.presets.commit_rename(&mut this.model, window, cx);
                    return;
                }
                this.presets.selected_index = Some(index);
                if this.model.can_change_presets() {
                    this.presets.press = Some((index, event.position));
                }
                cx.notify();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
            if let Some(drag) = this.presets.drag.as_mut() {
                if drag.over != index {
                    drag.over = index;
                    cx.notify();
                }
                return;
            }
            if let Some((from, start)) = this.presets.press {
                let dx = f32::from(event.position.x - start.x).abs();
                let dy = f32::from(event.position.y - start.y).abs();
                if dx >= DRAG_THRESHOLD_PX || dy >= DRAG_THRESHOLD_PX {
                    this.presets.drag = Some(DragReorder { from, over: index });
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, _event: &MouseUpEvent, _window, cx| {
                cx.stop_propagation();
                this.presets.press = None;
                if let Some(drag) = this.presets.drag.take() {
                    if drag.from != drag.over {
                        let reordered = reorder_presets(&this.model.presets, drag.from, drag.over);
                        this.model.move_presets(reordered);
                    }
                    cx.notify();
                    return;
                }
                if is_valid && this.model.can_change_presets() {
                    this.model.apply_preset(&file_click);
                    cx.notify();
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
                this.presets.selected_index = Some(index);
                this.presets.context_menu = Some(ContextMenuState {
                    file: file_ctx.clone(),
                    window_pos: event.position,
                });
                cx.notify();
            }),
        )
        .when_some(thumb_well, |el, well| el.child(well))
        .child(name_block)
        .child(detail_block)
}

fn render_empty_state(
    theme: &GpuiTheme,
    enabled: bool,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let hover = theme.subtle_hover;
    div()
        .flex_1()
        .w_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .px(px(32.0))
        .pb(px(36.0))
        .gap(px(12.0))
        .child(
            div()
                .font_family(theme.serif_family.clone())
                .font_weight(FontWeight::MEDIUM)
                .text_size(px(21.0))
                .text_color(theme.paper)
                .child("No presets yet"),
        )
        .child(
            div()
                .font_family(theme.serif_family.clone())
                .italic()
                .text_size(px(14.0))
                .line_height(px(20.0))
                .text_color(theme.dim)
                .text_center()
                .child(
                    "Save this photograph’s film, paper and Lab settings with New, or import presets made in Negs or spektrafilm. You can also drop them here.",
                ),
        )
        .child(
            div()
                .id("presets-empty-import")
                .tooltip(crate::tooltip::tip("Import presets (Ctrl+Shift+O)", theme))
                .mt(px(6.0))
                .px(px(14.0))
                .py(px(6.0))
                .rounded(px(5.0))
                .border_1()
                .border_color(theme.track)
                .font_family(theme.ui_family.clone())
                .text_size(px(12.5))
                .text_color(if enabled { theme.paper } else { theme.dim })
                .when(enabled, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|_this, _, window, cx| {
                            PresetsState::prompt_import_presets(window, cx);
                        }))
                })
                .when(!enabled, |el| el.opacity(0.45))
                .child("Import Presets…"),
        )
}

fn render_context_menu(
    menu: &ContextMenuState,
    pane_bounds: Bounds<Pixels>,
    model: &Model,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let pane_w = f32::from(pane_bounds.size.width).max(PRESETS_SIDEBAR_WIDTH);
    let pane_h = f32::from(pane_bounds.size.height).max(320.0);
    let rel_x = (f32::from(menu.window_pos.x) - f32::from(pane_bounds.origin.x))
        .clamp(8.0, (pane_w - CONTEXT_MENU_WIDTH - 8.0).max(8.0));
    let rel_y = (f32::from(menu.window_pos.y) - f32::from(pane_bounds.origin.y))
        .clamp(8.0, (pane_h - CONTEXT_MENU_HEIGHT - 8.0).max(8.0));

    let valid = matches!(
        model.preset_status.get(&menu.file),
        Some(PresetStatus::Valid { .. })
    ) && model.can_change_presets();

    let hover = theme.subtle_hover;
    let file_apply = menu.file.clone();
    let file_rename = menu.file.clone();
    let file_export = menu.file.clone();
    let file_folder = menu.file.clone();
    let file_delete = menu.file.clone();

    let item = |id: &'static str, label: &'static str, shortcut: Option<&'static str>, enabled: bool, warn_ink: bool| {
        let color = if !enabled {
            theme.dim.opacity(0.45)
        } else if warn_ink {
            theme.warn
        } else {
            theme.paper
        };
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .w_full()
            .px(px(12.0))
            .py(px(6.0))
            .font_family(theme.ui_family.clone())
            .text_size(px(12.0))
            .text_color(color)
            .when(enabled, |el| el.cursor_pointer().hover(move |s| s.bg(hover)))
            .child(label)
            .when_some(shortcut, |el, sc| {
                el.child(div().text_size(px(11.0)).text_color(theme.dim).child(sc))
            })
    };

    div()
        .id("preset-context-menu")
        .absolute()
        .left(px(rel_x))
        .top(px(rel_y))
        .w(px(CONTEXT_MENU_WIDTH))
        .py(px(4.0))
        .rounded(px(6.0))
        .bg(theme.panel)
        .border_1()
        .border_color(theme.track)
        .shadow_md()
        .flex()
        .flex_col()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|_this, _event: &MouseDownEvent, _window, cx| {
                cx.stop_propagation();
            }),
        )
        .child(
            item("preset-ctx-apply", "Apply", None, valid, false).when(valid, |el| {
                el.on_click(cx.listener(move |this, _, _window, cx| {
                    this.presets.context_menu = None;
                    this.model.apply_preset(&file_apply);
                    cx.notify();
                }))
            }),
        )
        .child(
            item("preset-ctx-rename", "Rename", Some("F2"), true, false).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.presets.begin_rename(&file_rename, window, cx);
                },
            )),
        )
        .child(
            item("preset-ctx-export", "Export…", None, true, false).on_click(cx.listener(
                move |this, _, window, cx| {
                    this.presets.context_menu = None;
                    PresetsState::prompt_export_preset(file_export.clone(), window, cx);
                    cx.notify();
                },
            )),
        )
        .child(
            item("preset-ctx-folder", "Show Presets Folder", None, true, false).on_click(
                cx.listener(move |this, _, _window, cx| {
                    this.presets.context_menu = None;
                    PresetsState::show_presets_folder(Some(&file_folder.path));
                    cx.notify();
                }),
            ),
        )
        .child(div().my(px(4.0)).w_full().h(px(1.0)).bg(theme.line))
        .child(
            item("preset-ctx-delete", "Delete", None, true, true).on_click(cx.listener(
                move |this, _, _window, cx| {
                    this.presets.context_menu = None;
                    this.model.delete_preset(&file_delete);
                    cx.notify();
                },
            )),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn stock(id: &str, name: &str, paper: bool) -> Stock {
        Stock {
            id: id.into(),
            name: name.into(),
            paper,
            positive: false,
            bw: false,
            target: String::new(),
            times: vec![],
            grain: Value::Null,
            fotufilm: false,
            manufacturer_data: true,
            negs_built: false,
            cine_print: false,
        }
    }

    #[test]
    fn reorder_presets_moves_forward_backward_and_handles_edges() {
        let list = vec!["a", "b", "c", "d"];
        assert_eq!(reorder_presets(&list, 0, 2), vec!["b", "c", "a", "d"]);
        assert_eq!(reorder_presets(&list, 3, 1), vec!["a", "d", "b", "c"]);
        assert_eq!(reorder_presets(&list, 2, 2), vec!["a", "b", "c", "d"]);
        assert_eq!(reorder_presets(&list, 1, 99), vec!["a", "c", "d", "b"]);
        assert_eq!(reorder_presets(&list, 10, 0), vec!["a", "b", "c", "d"]);
        let empty: Vec<&str> = vec![];
        assert!(reorder_presets(&empty, 0, 0).is_empty());
    }

    #[test]
    fn filter_json_paths_keeps_only_json_case_insensitively() {
        let paths = vec![
            PathBuf::from("/tmp/portra.json"),
            PathBuf::from("/tmp/WARM.JSON"),
            PathBuf::from("/tmp/photo.nef"),
            PathBuf::from("/tmp/notes.txt"),
            PathBuf::from("/tmp/no_ext"),
        ];
        assert_eq!(
            filter_json_paths(&paths),
            vec![
                PathBuf::from("/tmp/portra.json"),
                PathBuf::from("/tmp/WARM.JSON"),
            ]
        );
    }

    #[test]
    fn validate_preset_name_matches_negs_app_rules() {
        assert_eq!(validate_preset_name("Portra 400 Warm"), None);
        assert_eq!(validate_preset_name("  Portra 400  "), None);
        assert!(validate_preset_name("").is_some());
        assert!(validate_preset_name("   ").is_some());
        assert!(validate_preset_name("bad/name").is_some());
        assert!(validate_preset_name("CON").is_some());
        assert!(validate_preset_name(".hidden").is_some());
        assert!(validate_preset_name("trailing.").is_some());
    }

    #[test]
    fn format_preset_detail_covers_printed_scan_invalid_and_checking() {
        let stocks = vec![
            stock("portra400", "Kodak Portra 400", false),
            stock("endura", "Kodak Endura", true),
            stock("velvia50", "Fujichrome Velvia 50", false),
        ];

        let printed = PresetStatus::Valid {
            film: "portra400".into(),
            paper: "endura".into(),
            scan: false,
            text: "{}".into(),
            white_balance: Value::Null,
        };
        assert_eq!(
            format_preset_detail(Some(&printed), &stocks),
            PresetDetail::Valid {
                film: "Kodak Portra 400".into(),
                paper_lead: "printed on",
                paper_name: "Kodak Endura".into(),
            }
        );

        let scan = PresetStatus::Valid {
            film: "velvia50".into(),
            paper: "endura".into(),
            scan: true,
            text: "{}".into(),
            white_balance: Value::Null,
        };
        assert_eq!(
            format_preset_detail(Some(&scan), &stocks),
            PresetDetail::Valid {
                film: "Fujichrome Velvia 50".into(),
                paper_lead: "not printed · film scan",
                paper_name: String::new(),
            }
        );

        let invalid = PresetStatus::Invalid("Corrupt JSON".into());
        assert_eq!(
            format_preset_detail(Some(&invalid), &stocks),
            PresetDetail::Invalid("Corrupt JSON".into())
        );

        assert_eq!(
            format_preset_detail(Some(&PresetStatus::Checking), &stocks),
            PresetDetail::Checking
        );
        assert_eq!(format_preset_detail(None, &stocks), PresetDetail::Checking);
    }

    #[test]
    fn inline_rename_editing_is_utf8_safe() {
        let file = PresetFile {
            path: PathBuf::from("/tmp/Café.json"),
            size: 10,
            modified: std::time::SystemTime::UNIX_EPOCH,
        };
        let mut edit = InlineRename::new(file);
        assert!(edit.select_all);
        assert_eq!(edit.draft, "Café");

        // Typing while select_all replaces entire draft.
        edit.insert_str("Gold");
        assert!(!edit.select_all);
        assert_eq!(edit.draft, "Gold");
        assert_eq!(edit.cursor, 4);

        edit.move_left();
        edit.insert_str(" 200 ");
        assert_eq!(edit.draft, "Gol 200 d");

        edit.move_end();
        edit.backspace();
        assert_eq!(edit.draft, "Gol 200 ");
        assert!(edit.error.is_none()); // trimmed "Gol 200" is valid!

        edit.insert_str("/bad");
        assert!(edit.error.is_some());
    }

    #[test]
    fn footer_hint_changes_on_drop_active() {
        assert_ne!(footer_hint(false), footer_hint(true));
        assert!(footer_hint(true).contains("Drop"));
    }
}
