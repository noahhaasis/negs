//! Complete Prep / Lab / Output Inspector driven directly from
//! `negs_app::inspector_view::InspectorView`.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, PathPromptOptions, Pixels, ScrollHandle, SharedString,
    TextRun, Window, canvas, div, point, prelude::*, px, size,
};
use negs_app::border::BorderColor;
use negs_app::edits::{self, MoreAction, Tab};
use negs_app::inspector_view::{
    self, EditChoice, Effect, Expanded, InspectorUi, InspectorView, Kind, NegativeAction,
    RowAction, RowId, RowView, SliderValue, Track, When, schema,
};
use negs_app::model::{Model, Pane, dirty};
use negs_app::negative::Pick;
use negs_app::session::SCOPE_HEIGHTS;
use negs_app::state;
use negs_app::theme::DISABLED_OPACITY;

use crate::NegsApp;
use crate::marks_gpui::{paint_highlighter, paint_pencil};
use crate::theme_gpui::{GpuiTheme, scale, tab_tracking};
use crate::widgets::color_filter::render_color_filter;
use crate::widgets::controls::{
    BORDER_COLOR_PRESETS, GUTTER, border_color_hsla, render_actions_row,
    render_balance_print_row, render_border_styles_row, render_choice_row, render_color_row,
    render_edit_dot, render_flat_field_row, render_group_header, render_longest_side_row,
    render_more_row, render_note_row, render_photograph_row, render_section_header,
    render_toggle_row, render_word_button,
};
use crate::widgets::scopes::{
    ScopeHeightDrag, ScopesCache, render_scopes, scopes_total_height,
};
use crate::widgets::slider::{SliderCallbacks, SliderDrag, render_ink_slider};
use crate::widgets::text_input::{
    InlineTextState, TextInputOutcome, TextInputStyle, parse_number, parse_pixels,
    render_text_input,
};
use crate::widgets::tone_graph::{ToneGraphState, render_tone_graph};

pub const INSPECTOR_MIN_WIDTH: f32 = 340.0;
pub const INSPECTOR_MAX_WIDTH: f32 = 440.0;
pub const CANVAS_MIN_WIDTH: f32 = 640.0;
pub const PANE_HEADER_HEIGHT: f32 = 42.0;
pub const TAB_ROW_HEIGHT: f32 = 50.0;

/// Y coordinate (column-local DIPs) where the Tab Edits popover anchors directly
/// beneath the `Prep · Lab · Output` tab bar (`L17`).
pub fn tab_edits_anchor_y(scope_height: f64) -> f32 {
    PANE_HEADER_HEIGHT + scopes_total_height(scope_height) + TAB_ROW_HEIGHT + 4.0
}

/// Returns true when `row_id` belongs inside an indented `RevealedRows` group
/// with a `1 px` `track` vertical rule (`L21`, matching `Inspector.swift:244-254`).
pub fn is_revealed_row(row_id: RowId) -> bool {
    schema::rows().get(row_id).is_some_and(|r| {
        matches!(
            r.when,
            When::CustomWhiteBalance
                | When::MeterAuto
                | When::CustomFormat
                | When::LongestSide
                | When::AssumeInput
                | When::ManualInput
        )
    })
}

/// Returns true when a `Kind::Group` header draws a `1 px` `line` divider above it (`L19`).
pub fn group_is_ruled(prev_visible_kind: Option<Kind>) -> bool {
    match prev_visible_kind {
        None | Some(Kind::Section(_)) => false,
        Some(Kind::Footer(text)) if text.starts_with("Your negative is read") => false,
        _ => true,
    }
}

/// Clamps the resizable inspector column width (`340..=440 DIP`, `L6`),
/// leaving at least `CANVAS_MIN_WIDTH` for the canvas whenever the window can.
pub fn clamp_inspector_width(desired: f32, viewport_w: f32) -> f32 {
    let max_for_window = (viewport_w - CANVAS_MIN_WIDTH).max(INSPECTOR_MIN_WIDTH);
    desired
        .clamp(INSPECTOR_MIN_WIDTH, INSPECTOR_MAX_WIDTH)
        .min(max_for_window)
}

#[derive(Clone, Debug)]
pub struct OpenChoice {
    pub row_id: RowId,
    pub anchor_y: f32,
}

pub struct OpenColorPicker {
    pub line: bool,
    pub current: BorderColor,
    pub hex_input: InlineTextState,
    pub hex_error: bool,
}

/// All interactive UI state for the right-hand Inspector column.
pub struct InspectorState {
    pub scroll_handle: ScrollHandle,
    pub scope_height: f64,
    pub scopes_cache: ScopesCache,
    pub tone_graph: ToneGraphState,
    pub active_slider_drag: Option<SliderDrag>,
    pub hovered_slider: Option<RowId>,
    pub active_scope_drag: Option<ScopeHeightDrag>,
    pub slider_bounds: HashMap<RowId, Bounds<Pixels>>,
    pub editing_slider: Option<(RowId, InlineTextState)>,
    pub longest_side_input: InlineTextState,
    pub longest_side_synced: String,
    pub open_choice: Option<OpenChoice>,
    pub open_tab_edits: Option<Tab>,
    pub open_color_picker: Option<OpenColorPicker>,
}

impl InspectorState {
    pub fn new(initial_scope_height: f64, cx: &mut Context<NegsApp>) -> Self {
        let clamped = initial_scope_height.clamp(SCOPE_HEIGHTS.0, SCOPE_HEIGHTS.1);
        Self {
            scroll_handle: ScrollHandle::new(),
            scope_height: clamped,
            scopes_cache: ScopesCache::default(),
            tone_graph: ToneGraphState::default(),
            active_slider_drag: None,
            hovered_slider: None,
            active_scope_drag: None,
            slider_bounds: HashMap::new(),
            editing_slider: None,
            longest_side_input: InlineTextState::new(cx.focus_handle(), ""),
            longest_side_synced: String::new(),
            open_choice: None,
            open_tab_edits: None,
            open_color_picker: None,
        }
    }

    pub fn has_active_drag(&self) -> bool {
        self.active_slider_drag.is_some()
            || self.active_scope_drag.is_some()
            || self.tone_graph.is_dragging()
    }

    pub fn close_popovers(&mut self) {
        self.open_choice = None;
        self.open_tab_edits = None;
        self.open_color_picker = None;
    }

    pub fn set_track_bounds(&mut self, row_id: RowId, bounds: Bounds<Pixels>) {
        self.slider_bounds.insert(row_id, bounds);
    }
}

/// Constructs the current `InspectorUi` snapshot from `model.prefs`.
pub fn inspector_ui(model: &Model) -> InspectorUi<'_> {
    InspectorUi {
        tab: Tab::from_title(model.prefs.tab()),
        expanded: Expanded::Titles(&model.prefs.expanded_adjustments),
        decimal: '.',
    }
}

/// Builds a fresh `InspectorView` directly from `model`.
pub fn build_inspector_view(model: &Model) -> InspectorView {
    let facts = model.inspector_facts();
    let ui = inspector_ui(model);
    inspector_view::build(&facts, &ui)
}

/// Applies one `Effect` to `model`. Returns `Some(effect)` when the effect
/// requires a UI dialog/popover (`ChooseLightFrame` or `ChooseBorderColor`).
pub fn apply_model_effect(model: &mut Model, effect: Effect) -> Option<Effect> {
    match effect {
        Effect::Set(path, value) => model.set(path, value),
        Effect::SetFilterShift(path, value) => model.set_filter_shift(path, value),
        Effect::ResetValue(path) => model.reset_value(path),
        Effect::ResetFilterShift(path) => model.reset_filter_shift(path),
        Effect::ResetFilterShiftAfterClick(path) => model.reset_filter_shift_after_click(path),
        Effect::SetExposureOffset(value) => model.set_exposure_offset(value),
        Effect::ResetExposureOffset => model.reset_exposure_offset(),
        Effect::SetBorderPercent(field, value) => model.set_border_percent(field, value),
        Effect::Border(change, undo) => model.set_border(|b| change.apply(b), undo),
        Effect::BeginInteraction => model.begin_interaction(),
        Effect::EndInteraction(undo) => model.end_interaction(undo),
        Effect::SetMatchesBrightness(on) => model.set_matches_brightness(on),
        Effect::SwitchColorFilter(on) => model.switch_color_filter(on),
        Effect::ChooseFilmFormat(format) => model.choose_film_format(format),
        Effect::ChoosePreviewSpace(space) => model.choose_preview_space(space),
        Effect::ChooseImageSize(mode) => model.choose_image_size(mode),
        Effect::SetFilterLink(link) => model.set_filter_link(link),
        Effect::BalancePrint => model.balance_print(),
        Effect::ClearFlatField => model.clear_flat_field(),
        Effect::Negative(action) => match action {
            NegativeAction::FindAgain => model.redetect_negative_base(),
            NegativeAction::Pick(Pick::Base) => model.begin_negative_pick(Pick::Base),
            NegativeAction::CropToPicture => model.crop_to_picture(),
            NegativeAction::AutoPrint => model.auto_print_negative(),
            NegativeAction::Pick(Pick::Neutral) => model.begin_negative_pick(Pick::Neutral),
            NegativeAction::UseForRoll => model.use_negative_for_roll(),
            NegativeAction::RollFiltration => model.use_roll_filtration(),
        },
        Effect::ToneGraphChannel(channel) => {
            model.prefs.tone_graph_channel = channel.into();
            model.prefs.save();
            model.dirty |= dirty::INSPECTOR;
        }
        Effect::SetExpanded(title, open) => {
            model.prefs.set_expanded(title, open);
            model.prefs.save();
            model.dirty |= dirty::STRUCTURE;
        }
        Effect::More(action) => {
            let prefs = &mut model.prefs;
            match action {
                MoreAction::Reveal(key) => prefs.set_revealed(&key, true),
                MoreAction::Conceal(key) => prefs.set_revealed(&key, false),
                MoreAction::ShowFewer => prefs.lab_all_controls = false,
            }
            prefs.save();
            model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR;
        }
        Effect::ShowBacklight => {
            model.negative.backlight_shown = true;
            model.dirty |= dirty::STRUCTURE | dirty::INSPECTOR;
        }
        Effect::ResetEdits(targets, undo) => model.reset_edits(&targets, undo),
        dialog @ (Effect::ChooseLightFrame | Effect::ChooseBorderColor { .. }) => {
            return Some(dialog);
        }
    }
    None
}

/// Applies all `Effect`s produced by `inspector_view::dispatch` or `edit_list`,
/// opening native path pickers or inline popovers where needed.
pub fn apply_effects(
    app: &mut NegsApp,
    effects: Vec<Effect>,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) {
    for effect in effects {
        let Some(dialog) = apply_model_effect(&mut app.model, effect) else {
            continue;
        };
        match dialog {
            Effect::ChooseLightFrame => {
                let receiver = cx.prompt_for_paths(PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(SharedString::from("Choose Light Frame")),
                });
                cx.spawn(async move |this, cx| {
                    if let Ok(Ok(Some(paths))) = receiver.await {
                        if let Some(light) = paths.into_iter().next() {
                            let _ = this.update(cx, |app, cx| {
                                app.model.choose_flat_field(&light);
                                app.after_model_update(cx);
                                cx.notify();
                            });
                        }
                    }
                })
                .detach();
            }
            Effect::ChooseBorderColor { line, current } => {
                app.inspector.close_popovers();
                let mut hex_input = InlineTextState::new(cx.focus_handle(), current.hex());
                hex_input.begin(current.hex(), false);
                let focus = hex_input.focus_handle.clone();
                app.inspector.open_color_picker = Some(OpenColorPicker {
                    line,
                    current,
                    hex_input,
                    hex_error: false,
                });
                window.focus(&focus);
            }
            _ => {}
        }
    }
    app.after_model_update(cx);
    cx.notify();
}

/// Dispatches a `RowAction` through `negs_app::inspector_view::dispatch` and applies its effects.
pub fn dispatch_row(
    app: &mut NegsApp,
    row_id: RowId,
    action: RowAction,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) {
    let effects = {
        let facts = app.model.inspector_facts();
        let ui = inspector_ui(&app.model);
        inspector_view::dispatch(&facts, &ui, row_id, action)
    };
    apply_effects(app, effects, window, cx);
}

/// Window-level mouse-move handler supporting active slider, scope height, and tone graph drags.
pub fn handle_root_mouse_move(
    app: &mut NegsApp,
    event: &MouseMoveEvent,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> bool {
    if let Some(drag) = app.inspector.active_slider_drag.as_mut() {
        let pos = drag.position_for_x(f32::from(event.position.x));
        if drag.last_position != Some(pos) {
            drag.last_position = Some(pos);
            let row_id = drag.row_id;
            dispatch_row(app, row_id, RowAction::Track(pos, Track::Drag), window, cx);
        }
        return true;
    }

    if let Some(drag) = app.inspector.active_scope_drag {
        let next = drag.height_for_y(f32::from(event.position.y));
        if (next - app.inspector.scope_height).abs() > 0.5 {
            app.inspector.scope_height = next;
            cx.notify();
        }
        return true;
    }

    if app.inspector.tone_graph.is_dragging() {
        let changed = app
            .inspector
            .tone_graph
            .handle_mouse_move(&mut app.model, event.position, true);
        if changed {
            app.after_model_update(cx);
            cx.notify();
        }
        return true;
    }

    false
}

/// Window-level mouse-up handler finalizing active slider, scope height, or tone graph drags.
pub fn handle_root_mouse_up(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> bool {
    if let Some(drag) = app.inspector.active_slider_drag.take() {
        let pos = drag.last_position.unwrap_or(0);
        dispatch_row(app, drag.row_id, RowAction::Track(pos, Track::End), window, cx);
        return true;
    }

    if app.inspector.active_scope_drag.take().is_some() {
        let h = app.inspector.scope_height;
        if (app.model.prefs.scope_height - h).abs() > 0.5 {
            app.model.prefs.scope_height = h;
            app.model.prefs.save();
        }
        cx.notify();
        return true;
    }

    if app.inspector.tone_graph.handle_mouse_up(&mut app.model) {
        app.after_model_update(cx);
        cx.notify();
        return true;
    }

    false
}

fn commit_longest_side(app: &mut NegsApp, cx: &mut Context<NegsApp>) {
    let text = app.inspector.longest_side_input.text().to_string();
    let pixels = parse_pixels(&text);
    let current = state::size_target(&app.model.state.params);
    if current == Some(pixels) && app.model.size_error.is_none() {
        return;
    }
    let _ = app.model.set_longest_side(pixels);
    app.after_model_update(cx);
    cx.notify();
}

fn begin_slider_edit(
    app: &mut NegsApp,
    row_id: RowId,
    initial: String,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) {
    app.inspector.close_popovers();
    let mut state = InlineTextState::new(cx.focus_handle(), initial.clone());
    state.begin(initial, true);
    let focus = state.focus_handle.clone();
    app.inspector.editing_slider = Some((row_id, state));
    window.focus(&focus);
    cx.notify();
}

fn handle_slider_edit_key(
    app: &mut NegsApp,
    row_id: RowId,
    event: &KeyDownEvent,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) {
    let Some((editing_id, ref mut state)) = app.inspector.editing_slider else {
        return;
    };
    if editing_id != row_id {
        return;
    }
    let outcome = state.handle_key_event(event, cx);
    match outcome {
        TextInputOutcome::Commit(text) => {
            app.inspector.editing_slider = None;
            window.focus(&app.focus_handle(cx));
            if let Some(value) = parse_number(&text) {
                dispatch_row(app, row_id, RowAction::Set(value), window, cx);
            } else {
                cx.notify();
            }
        }
        TextInputOutcome::Cancel => {
            app.inspector.editing_slider = None;
            window.focus(&app.focus_handle(cx));
            cx.notify();
        }
        TextInputOutcome::Changed | TextInputOutcome::Handled => {
            cx.notify();
        }
        TextInputOutcome::Ignored => {}
    }
}

fn handle_slider_track_down(
    app: &mut NegsApp,
    row_id: RowId,
    slider: SliderValue,
    event: &MouseDownEvent,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) {
    app.inspector.close_popovers();
    app.inspector.editing_slider = None;

    if event.click_count >= 2 {
        dispatch_row(app, row_id, RowAction::Reset { double_click: true }, window, cx);
        return;
    }

    let bounds = app
        .inspector
        .slider_bounds
        .get(&row_id)
        .copied()
        .unwrap_or_default();
    let track_left = f32::from(bounds.origin.x);
    let track_width = f32::from(bounds.size.width).max(1.0);
    let mut drag = SliderDrag {
        row_id,
        low: slider.low,
        high: slider.high,
        step: slider.step,
        track_left,
        track_width,
        last_position: None,
    };
    let pos = drag.position_for_x(f32::from(event.position.x));
    drag.last_position = Some(pos);
    app.inspector.active_slider_drag = Some(drag);
    dispatch_row(app, row_id, RowAction::Track(pos, Track::Drag), window, cx);
}

/// Resolves the dropdown options for any choice-like `RowView`.
fn options_for_row(row: &RowView) -> Vec<String> {
    if !row.options.is_empty() {
        return row.options.clone();
    }
    row.kind.map_or_else(Vec::new, |k| k.options())
}

/// Renders a single visible `RowView`, matching every `Kind` variant exhaustively.
#[allow(clippy::too_many_arguments)]
fn render_row(
    app: &NegsApp,
    row: &RowView,
    is_first_visible: bool,
    prev_visible_kind: Option<Kind>,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let Some(kind) = row.kind else {
        return div().into_any_element();
    };
    let row_id = row.id;

    match kind {
        Kind::Section(_) => render_section_header(row, is_first_visible, theme, window),

        Kind::Footer(_) | Kind::Note(_) => render_note_row(row, theme).into_any_element(),

        Kind::Group { enable, .. } => render_group_header(
            row,
            enable.is_some(),
            group_is_ruled(prev_visible_kind),
            theme,
            cx,
            move |this, window, cx| dispatch_row(this, row_id, RowAction::Press(0), window, cx),
            move |this, on, window, cx| {
                dispatch_row(this, row_id, RowAction::Check(on), window, cx)
            },
        )
        .into_any_element(),

        Kind::Slider(_) | Kind::BorderSlider(..) | Kind::ExposureOffset => {
            let Some(slider) = row.slider else {
                return div().into_any_element();
            };
            let dragging = app
                .inspector
                .active_slider_drag
                .as_ref()
                .is_some_and(|d| d.row_id == row_id);
            let hovered = app.inspector.hovered_slider == Some(row_id);
            let editing_state = app
                .inspector
                .editing_slider
                .as_ref()
                .filter(|(id, _)| *id == row_id)
                .map(|(_, s)| s);

            render_ink_slider(
                row,
                slider,
                dragging,
                hovered,
                editing_state,
                theme,
                window,
                cx,
                SliderCallbacks {
                    on_bounds: Box::new(|this, id, bounds| {
                        this.inspector.set_track_bounds(id, bounds);
                    }),
                    on_track_down: Box::new(|this, id, sv, ev, win, cx| {
                        handle_slider_track_down(this, id, sv, ev, win, cx);
                    }),
                    on_action: Box::new(|this, id, action, win, cx| {
                        dispatch_row(this, id, action, win, cx);
                    }),
                    on_begin_edit: Box::new(|this, id, initial, win, cx| {
                        begin_slider_edit(this, id, initial, win, cx);
                    }),
                    on_edit_key: Box::new(|this, id, ev, win, cx| {
                        handle_slider_edit_key(this, id, ev, win, cx);
                    }),
                    on_edit_focus: Box::new(|this, win, _cx| {
                        if let Some((_, st)) = &this.inspector.editing_slider {
                            win.focus(&st.focus_handle);
                        }
                    }),
                    on_hover: Box::new(|this, id, inside, cx| {
                        let next = if inside {
                            Some(id)
                        } else if this.inspector.hovered_slider == Some(id) {
                            None
                        } else {
                            return;
                        };
                        if this.inspector.hovered_slider != next {
                            this.inspector.hovered_slider = next;
                            cx.notify();
                        }
                    }),
                },
            )
            .into_any_element()
        }

        Kind::Toggle(..) | Kind::MatchBrightness | Kind::BorderToggle(..) => render_toggle_row(
            row,
            theme,
            cx,
            move |this, on, window, cx| {
                dispatch_row(this, row_id, RowAction::Check(on), window, cx)
            },
            move |this, window, cx| {
                if inspector_view::has_reset(row_id) {
                    dispatch_row(
                        this,
                        row_id,
                        RowAction::Reset { double_click: false },
                        window,
                        cx,
                    );
                }
            },
        )
        .into_any_element(),

        Kind::Choice(..)
        | Kind::FilmFormat
        | Kind::PreviewSpace
        | Kind::SizeMode
        | Kind::FilterLink(_)
        | Kind::DevTime(_) => {
            let options = options_for_row(row);
            let menu_open = app
                .inspector
                .open_choice
                .as_ref()
                .is_some_and(|c| c.row_id == row_id);
            render_choice_row(
                row,
                &options,
                menu_open,
                theme,
                cx,
                move |this, anchor_y, _window, cx| {
                    let already = this
                        .inspector
                        .open_choice
                        .as_ref()
                        .is_some_and(|c| c.row_id == row_id);
                    this.inspector.close_popovers();
                    if !already {
                        this.inspector.open_choice = Some(OpenChoice { row_id, anchor_y });
                    }
                    cx.notify();
                },
                move |this, window, cx| {
                    if inspector_view::has_reset(row_id) {
                        dispatch_row(
                            this,
                            row_id,
                            RowAction::Reset { double_click: false },
                            window,
                            cx,
                        );
                    }
                },
            )
            .into_any_element()
        }

        Kind::More(_) => render_more_row(row, theme, cx, move |this, window, cx| {
            dispatch_row(this, row_id, RowAction::Press(0), window, cx);
        })
        .into_any_element(),

        Kind::ColorFilter => render_color_filter(
            row,
            &app.model.color_filters,
            app.model.color_filter(),
            theme,
            cx,
            |this, key, _window, cx| {
                if this.model.color_filter() != key {
                    this.model.set_color_filter(&key);
                    this.after_model_update(cx);
                    cx.notify();
                }
            },
        )
        .into_any_element(),

        Kind::ToneGraph => render_tone_graph(
            row,
            &app.model,
            &app.inspector.tone_graph,
            theme,
            cx,
            |this, bounds| {
                this.inspector.tone_graph.bounds = bounds;
            },
            |this, id, action, window, cx| {
                dispatch_row(this, id, action, window, cx);
            },
            |this, event, enabled, _window, cx| {
                this.inspector.close_popovers();
                if this.inspector.tone_graph.handle_mouse_down(
                    &mut this.model,
                    event.position,
                    event.click_count,
                    enabled,
                ) {
                    this.after_model_update(cx);
                    cx.notify();
                }
            },
            |this, event, enabled, _window, cx| {
                if !this.inspector.tone_graph.is_dragging()
                    && this.inspector.tone_graph.handle_mouse_move(
                        &mut this.model,
                        event.position,
                        enabled,
                    )
                {
                    cx.notify();
                }
            },
        )
        .into_any_element(),

        Kind::Actions(buttons) => render_actions_row(
            row,
            buttons,
            theme,
            cx,
            move |this, idx, window, cx| {
                dispatch_row(this, row_id, RowAction::Press(idx), window, cx);
            },
        )
        .into_any_element(),

        Kind::FlatField => render_flat_field_row(
            row,
            theme,
            cx,
            move |this, window, cx| {
                dispatch_row(this, row_id, RowAction::Press(0), window, cx);
            },
            move |this, window, cx| {
                dispatch_row(this, row_id, RowAction::Press(1), window, cx);
            },
        )
        .into_any_element(),

        Kind::BalancePrint(..) => render_balance_print_row(row, theme, cx, move |this, win, cx| {
            dispatch_row(this, row_id, RowAction::Press(0), win, cx);
        })
        .into_any_element(),

        Kind::BorderStyles => {
            let options = options_for_row(row);
            let border = &app.model.state.border;
            render_border_styles_row(
                row,
                &options,
                border.color,
                border.line_color,
                theme,
                cx,
                move |this, idx, window, cx| {
                    dispatch_row(this, row_id, RowAction::Press(idx), window, cx);
                },
            )
            .into_any_element()
        }

        Kind::Color(_) => render_color_row(row, theme, cx, move |this, window, cx| {
            dispatch_row(this, row_id, RowAction::Press(0), window, cx);
        })
        .into_any_element(),

        Kind::LongestSide => render_longest_side_row(
            row,
            &app.inspector.longest_side_input,
            app.model.size_error.is_some(),
            theme,
            window,
            cx,
            |this, event, window, cx| {
                let outcome = this.inspector.longest_side_input.handle_key_event(event, cx);
                match outcome {
                    TextInputOutcome::Commit(_) => {
                        commit_longest_side(this, cx);
                        window.focus(&this.focus_handle(cx));
                    }
                    TextInputOutcome::Cancel => {
                        this.model.size_error = None;
                        let current = state::size_target(&this.model.state.params)
                            .map_or_else(String::new, |v| v.to_string());
                        this.inspector.longest_side_input.sync_external(current.clone());
                        this.inspector.longest_side_synced = current;
                        window.focus(&this.focus_handle(cx));
                        cx.notify();
                    }
                    TextInputOutcome::Changed | TextInputOutcome::Handled => cx.notify(),
                    TextInputOutcome::Ignored => {}
                }
            },
            |this, window, cx| {
                this.inspector.close_popovers();
                window.focus(&this.inspector.longest_side_input.focus_handle);
                cx.notify();
            },
        )
        .into_any_element(),

        Kind::Photograph => render_photograph_row(row, theme).into_any_element(),
    }
}

/// Top header bar of the Inspector column (`PaneHeader`, `L14`):
/// `42 px` tall, `pt: 13`, `px: 22`, NO bottom border; `Adjustments` · `Presets`
/// in UI `12 pt` w500 (`gap: 16`), right side italic Newsreader `14 pt` edit
/// count + `WordButton`s (`Reset`, or `New` + `Import…`).
fn render_pane_header(
    app: &NegsApp,
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let pane = app.model.pane();
    let is_presets = pane == Pane::Presets;
    let has_photo = app.model.source.is_some();
    let can_reset = has_photo && !app.model.stocks.is_empty() && !app.model.cropping();
    let can_presets = app.model.can_change_presets();
    let hover_paper = theme.paper;
    let word_font = theme.ui_font(FontWeight::MEDIUM);
    let (count_num_font, count_size) = theme.serif_spec(scale::EDIT_COUNT, 700.0, true);
    let (count_word_font, _) = theme.serif_spec(scale::EDIT_COUNT, 400.0, true);

    div()
        .flex()
        .items_baseline()
        .justify_between()
        .w_full()
        .h(px(PANE_HEADER_HEIGHT))
        .pt(px(13.0))
        .px(px(GUTTER))
        // Left: Adjustments · Presets
        .child(
            div()
                .flex()
                .items_baseline()
                .gap(px(16.0))
                .child(
                    div()
                        .id("pane-tab-adjustments")
                        .tooltip(crate::tooltip::tip("Show the adjustments (F7)", theme))
                        .cursor_pointer()
                        .font(word_font.clone())
                        .text_size(px(scale::PANE_HEADER))
                        .text_color(if !is_presets { theme.paper } else { theme.dim })
                        .hover(move |s| s.text_color(hover_paper))
                        .active(|s| s.opacity(0.62))
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.inspector.close_popovers();
                            this.model.show_pane(Pane::Adjustments);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child("Adjustments"),
                )
                .child(
                    div()
                        .id("pane-tab-presets")
                        .tooltip(crate::tooltip::tip("Show the presets (F8)", theme))
                        .cursor_pointer()
                        .font(word_font)
                        .text_size(px(scale::PANE_HEADER))
                        .text_color(if is_presets { theme.paper } else { theme.dim })
                        .hover(move |s| s.text_color(hover_paper))
                        .active(|s| s.opacity(0.62))
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.inspector.close_popovers();
                            this.model.show_pane(Pane::Presets);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child("Presets"),
                ),
        )
        // Right: edit count + Reset (only when a photo is open), or New + Import…
        .child(if is_presets {
            div()
                .flex()
                .items_baseline()
                .gap(px(16.0))
                .child(render_word_button(
                    "pane-presets-new",
                    "New",
                    scale::WORD_BUTTON,
                    can_presets,
                    Some(SharedString::from(
                        "Save the film, paper and Lab settings as a preset (Ctrl+Shift+S)",
                    )),
                    theme,
                    cx,
                    |this, _window, cx| {
                        this.model.new_preset();
                        this.after_model_update(cx);
                        cx.notify();
                    },
                ))
                .child(render_word_button(
                    "pane-presets-import",
                    "Import…",
                    scale::WORD_BUTTON,
                    can_presets,
                    Some(SharedString::from("Import presets (Ctrl+Shift+O)")),
                    theme,
                    cx,
                    |_this, window, cx| {
                        crate::presets::PresetsState::prompt_import_presets(window, cx);
                    },
                ))
                .into_any_element()
        } else if has_photo {
            div()
                .flex()
                .items_baseline()
                .gap(px(14.0))
                .when(view.total > 0, |el| {
                    el.child(
                        div()
                            .flex()
                            .items_baseline()
                            .child(
                                div()
                                    .font(count_num_font)
                                    .text_size(px(count_size))
                                    .text_color(theme.paper)
                                    .child(view.total.to_string()),
                            )
                            .child(
                                div()
                                    .font(count_word_font)
                                    .text_size(px(count_size))
                                    .text_color(theme.dim)
                                    .child(if view.total == 1 { " edit" } else { " edits" }),
                            ),
                    )
                })
                .child(render_word_button(
                    "pane-reset-all",
                    "Reset",
                    scale::WORD_BUTTON,
                    can_reset,
                    Some(SharedString::from(
                        "Restore defaults on all tabs while keeping the film, paper, rotation, crop, border, and Image Size. You can undo this.",
                    )),
                    theme,
                    cx,
                    |this, _window, cx| {
                        this.inspector.close_popovers();
                        this.model.reset();
                        this.after_model_update(cx);
                        cx.notify();
                    },
                ))
                .into_any_element()
        } else {
            div().into_any_element()
        })
        .into_any_element()
}

/// Renders one editorial tab word (`Prep`, `Lab`, `Output`) at Newsreader
/// `21 pt` (`700 paper` chosen / `500 dim` inactive) with `tab_tracking` so
/// switching tabs never shifts neighbors, and `6 px` `PencilStroke` / Light
/// Table highlighter painted behind/under the chosen word (`L17`).
fn render_tab_word(
    title: &'static str,
    chosen: bool,
    theme: &GpuiTheme,
    window: &Window,
) -> AnyElement {
    let (bold_font, sz) = theme.serif_spec(scale::TAB, 700.0, false);
    let (med_font, _) = theme.serif_spec(scale::TAB, 500.0, false);
    let color = if chosen { theme.paper } else { theme.dim };
    let plain = theme.tokens.plain;
    let highlighter = theme.tokens.highlighter;
    let accent = theme.accent;
    let highlighter_ink = theme.highlighter_ink;

    let measure = |font: &gpui::Font| -> f32 {
        let run = TextRun {
            len: title.len(),
            font: font.clone(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let shaped =
            window
                .text_system()
                .shape_line(SharedString::from(title), px(sz), &[run], None);
        f32::from(shaped.width)
    };
    let bold_w = measure(&bold_font);
    let med_w = measure(&med_font);
    let word_w = bold_w.max(med_w);
    let chars: Vec<String> = title.chars().map(|c| c.to_string()).collect();
    let track = if chosen {
        0.0
    } else {
        tab_tracking(bold_w, med_w, chars.len())
    };
    let active_font = if chosen { bold_font } else { med_font };
    let char_widths: Vec<f32> = if track > 0.01 {
        chars
            .iter()
            .map(|ch| {
                let run = TextRun {
                    len: ch.len(),
                    font: active_font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                f32::from(
                    window
                        .text_system()
                        .shape_line(SharedString::from(ch.clone()), px(sz), &[run], None)
                        .width,
                )
            })
            .collect()
    } else {
        Vec::new()
    };

    let text_h = (sz + 6.0).ceil();
    let total_h = text_h + 6.0;

    canvas(
        |_b, _w, _cx| {},
        move |bounds: Bounds<Pixels>, (), window, cx| {
            // Paint Light Table highlighter BEFORE text so dark ink sits on top (`L3`).
            let mark_rect = Bounds {
                origin: point(bounds.origin.x, bounds.origin.y + px(text_h - 1.0)),
                size: size(bounds.size.width, px(negs_marks::PENCIL_HEIGHT as f32)),
            };
            if chosen && highlighter {
                paint_highlighter(window, mark_rect, plain, 6.0, highlighter_ink);
            }

            if track > 0.01 {
                let mut x = f32::from(bounds.origin.x);
                for (i, ch) in chars.iter().enumerate() {
                    let run = TextRun {
                        len: ch.len(),
                        font: active_font.clone(),
                        color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    let shaped = window.text_system().shape_line(
                        SharedString::from(ch.clone()),
                        px(sz),
                        &[run],
                        None,
                    );
                    let _ = shaped.paint(point(px(x), bounds.origin.y), px(text_h), window, cx);
                    x += char_widths[i] + track;
                }
            } else {
                let run = TextRun {
                    len: title.len(),
                    font: active_font.clone(),
                    color,
                    background_color: None,
                    underline: None,
                    strikethrough: None,
                };
                let shaped = window.text_system().shape_line(
                    SharedString::from(title),
                    px(sz),
                    &[run],
                    None,
                );
                let _ = shaped.paint(bounds.origin, px(text_h), window, cx);
            }

            if chosen && !highlighter {
                paint_pencil(
                    window,
                    mark_rect,
                    plain,
                    negs_marks::PENCIL_WIDTH as f32,
                    accent,
                );
            }
        },
    )
    .w(px(word_w.ceil().max(1.0)))
    .h(px(total_h))
    .into_any_element()
}

/// Editorial tab bar (`Prep` · `Lab` · `Output`, `L17`):
/// `50 px` tall, top & bottom `1 px` `line` rules, left-aligned from `22 px`
/// (`24 px` gap), Newsreader `21 pt` words with grease-pencil underline / Light
/// Table highlighter, and bare `5 px` yellow dot + `10.5 pt` semibold `label`
/// edit count.
fn render_tab_bar(
    _app: &NegsApp,
    view: &InspectorView,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let active_tab = view.tab;
    let count_font = theme.ui_font(FontWeight::SEMIBOLD);

    div()
        .flex()
        .items_start()
        .w_full()
        .h(px(TAB_ROW_HEIGHT))
        .px(px(GUTTER))
        .pt(px(12.0))
        .gap(px(24.0))
        .border_t_1()
        .border_b_1()
        .border_color(theme.line)
        .children(Tab::ALL.into_iter().enumerate().map(|(i, tab)| {
            let active = tab == active_tab;
            let count = view.counts[i];
            div()
                .flex()
                .items_start()
                .child(
                    div()
                        .id(SharedString::from(format!("insp-tab-{}", tab.title())))
                        .cursor_pointer()
                        .active(|s| s.opacity(0.62))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.inspector.close_popovers();
                            if this.model.prefs.tab() != tab.title()
                                || !this.model.prefs.inspector_tab_chosen
                            {
                                this.model.prefs.choose_tab(tab.title());
                                this.model.prefs.save();
                                this.inspector
                                    .scroll_handle
                                    .set_offset(point(px(0.0), px(0.0)));
                                this.model.dirty |= dirty::STRUCTURE;
                            }
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child(render_tab_word(tab.title(), active, theme, window)),
                )
                .when(count > 0, |el| {
                    el.child(
                        div()
                            .id(SharedString::from(format!(
                                "insp-tab-count-{}",
                                tab.title()
                            )))
                            .tooltip(crate::tooltip::tip(
                                format!(
                                    "Show the {} on {}",
                                    if count == 1 { "edit" } else { "edits" },
                                    tab.title()
                                ),
                                theme,
                            ))
                            .flex()
                            .items_start()
                            .cursor_pointer()
                            .active(|s| s.opacity(0.62))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _event, _window, cx| {
                                    cx.stop_propagation();
                                    let next = if this.inspector.open_tab_edits == Some(tab) {
                                        None
                                    } else {
                                        Some(tab)
                                    };
                                    this.inspector.close_popovers();
                                    this.inspector.open_tab_edits = next;
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .pl(px(5.0))
                                    .pr(px(3.0))
                                    .pt(px(6.0))
                                    .child(render_edit_dot(theme)),
                            )
                            .child(
                                div()
                                    .pt(px(1.0))
                                    .font(count_font.clone())
                                    .text_size(px(scale::TAB_COUNT))
                                    .text_color(theme.label)
                                    .child(count.to_string()),
                            ),
                    )
                })
        }))
        .into_any_element()
}

/// Output tab footer (`ExportFoot`, `L24`): top `1 px` `line` rule, centered
/// Newsreader `14 pt` italic summary, `36 px` `r = 3` `fill` button
/// `"Export Photograph…"` in Newsreader `18 pt` `700` `on_fill` with `1 px`
/// press sink, plus centered italic Lightroom `WordButton`s when installed.
fn render_output_foot(
    app: &NegsApp,
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let can_export = view.can_export && !app.model.cropping();
    let export_title = app.model.export_title();
    let export_tip = if export_title == "Export Photograph…" {
        "Export photograph (Ctrl+Shift+E)"
    } else {
        "Export the selected photographs (Ctrl+Shift+E)"
    };
    let (sum_font, sum_size) = theme.serif_spec(scale::EXPORT_SUMMARY, 400.0, true);
    let (btn_font, btn_size) = theme.serif_spec(scale::EXPORT_BUTTON, 700.0, false);
    let lightroom_kinds = app.model.lightroom_kinds();
    let targets = app.model.export_targets().len();

    div()
        .w_full()
        .px(px(GUTTER))
        .pt(px(11.0))
        .pb(px(16.0))
        .border_t_1()
        .border_color(theme.line)
        .flex()
        .flex_col()
        .items_center()
        .gap(px(8.0))
        .when(!view.export_summary.is_empty(), |el| {
            el.child(
                div()
                    .font(sum_font)
                    .text_size(px(sum_size))
                    .text_color(theme.label)
                    .truncate()
                    .child(view.export_summary.clone()),
            )
        })
        .child(
            div()
                .id("inspector-export-btn")
                .tooltip(crate::tooltip::tip(export_tip, theme))
                .w_full()
                .h(px(36.0))
                .pb(px(2.0))
                .rounded(px(3.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(theme.fill)
                .font(btn_font)
                .text_size(px(btn_size))
                .text_color(theme.on_fill)
                .when(!can_export, |el| el.opacity(DISABLED_OPACITY as f32))
                .when(can_export, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.opacity(0.94))
                        .active(|s| s.opacity(0.84).mt(px(1.0)))
                        .on_click(cx.listener(|this, _, _window, cx| {
                            crate::export_card::open(this, true, cx);
                        }))
                })
                .child(export_title),
        )
        .when(!lightroom_kinds.is_empty(), |el| {
            el.child(
                div()
                    .pt(px(2.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap(px(14.0))
                    .children(lightroom_kinds.into_iter().map(|kind| {
                        let enabled = app.model.can_open_in_lightroom(kind);
                        let what = if targets > 1 {
                            format!("the {targets} selected photographs")
                        } else {
                            "the photograph".to_string()
                        };
                        let shortcut = if kind == negs_app::lightroom::Kind::Cloud {
                            " (Alt+Ctrl+E)"
                        } else {
                            ""
                        };
                        let tip = format!(
                            "Open {what} in {} as full-size 16-bit TIFFs{shortcut}",
                            kind.title()
                        );
                        render_word_button(
                            SharedString::from(format!("insp-lr-{}", kind.title())),
                            format!("Open in {}", kind.title()),
                            13.5,
                            enabled,
                            Some(SharedString::from(tip)),
                            theme,
                            cx,
                            move |this, _window, cx| {
                                this.model.open_in_lightroom(kind);
                                this.after_model_update(cx);
                                cx.notify();
                            },
                        )
                    })),
            )
        })
        .into_any_element()
}

/// Floating Tab Edits popover (`TabEditsList`, `L17`): `300 px` wide, anchored
/// dynamically directly below the tab bar, Newsreader `15 pt` `600` italic
/// header, `1 px` `line` rules, `label` `↺` per edit (never `accent` text),
/// and left-aligned `WordButton("Reset <Tab>")`.
fn render_tab_edits_popover(
    app: &NegsApp,
    tab: Tab,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> Option<AnyElement> {
    let facts = app.model.inspector_facts();
    let edits_list = edits::edits(tab, &facts, '.');
    if edits_list.is_empty() {
        return None;
    }
    let head = inspector_view::edits_head(tab, edits_list.len());
    let edits_rc = Rc::new(edits_list);
    let anchor_y = tab_edits_anchor_y(app.inspector.scope_height);
    let (head_font, head_size) = theme.serif_spec(15.0, 600.0, true);
    let (reset_font, reset_size) = theme.serif_spec(15.0, 400.0, false);
    let ui_font = theme.ui_font(FontWeight::NORMAL);
    let paper_hover = theme.paper;

    Some(
        div()
            .id("tab-edits-popover")
            .absolute()
            .top(px(anchor_y))
            .left(px(GUTTER - 6.0))
            .w(px(300.0))
            .p(px(14.0))
            .rounded(px(6.0))
            .bg(theme.panel)
            .border_1()
            .border_color(theme.track)
            .shadow_md()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            // Header
            .child(
                div()
                    .pb(px(8.0))
                    .font(head_font)
                    .text_size(px(head_size))
                    .text_color(theme.paper)
                    .child(head),
            )
            // Individual edits list
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_h(px(320.0))
                    .children(edits_rc.iter().enumerate().map(|(i, edit)| {
                        let edits_for_click = edits_rc.clone();
                        let reset_tip = SharedString::from(format!("Reset {}", edit.title));
                        div()
                            .id(SharedString::from(format!("tab-edit-item-{i}")))
                            .flex()
                            .flex_col()
                            .child(div().w_full().h(px(1.0)).bg(theme.line))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(8.0))
                                    .py(px(5.0))
                                    .child(
                                        div()
                                            .font(ui_font.clone())
                                            .text_size(px(scale::ROW_LABEL))
                                            .text_color(theme.label)
                                            .truncate()
                                            .min_w_0()
                                            .child(edit.title.clone()),
                                    )
                                    .child(
                                        div()
                                            .flex()
                                            .items_center()
                                            .gap(px(8.0))
                                            .flex_shrink_0()
                                            .child(
                                                div()
                                                    .font(ui_font.clone())
                                                    .text_size(px(scale::ROW_LABEL))
                                                    .text_color(theme.dim)
                                                    .child(edit.value.clone()),
                                            )
                                            .child(
                                                div()
                                                    .id(SharedString::from(format!(
                                                        "tab-edit-reset-{i}"
                                                    )))
                                                    .tooltip(crate::tooltip::tip(reset_tip, theme))
                                                    .cursor_pointer()
                                                    .w(px(18.0))
                                                    .h(px(18.0))
                                                    .flex()
                                                    .items_center()
                                                    .justify_center()
                                                    .font(reset_font.clone())
                                                    .text_size(px(reset_size))
                                                    .text_color(theme.label)
                                                    .hover(move |s| s.text_color(paper_hover))
                                                    .active(|s| s.opacity(0.62))
                                                    .on_click(cx.listener(
                                                        move |this, _, window, cx| {
                                                            if let Some(effect) =
                                                                inspector_view::edit_list(
                                                                    tab,
                                                                    &edits_for_click,
                                                                    EditChoice::One(i),
                                                                )
                                                            {
                                                                apply_effects(
                                                                    this,
                                                                    vec![effect],
                                                                    window,
                                                                    cx,
                                                                );
                                                            }
                                                        },
                                                    ))
                                                    .child("↺"),
                                            ),
                                    ),
                            )
                    })),
            )
            // Bottom rule + left-aligned WordButton("Reset <Tab>")
            .child(div().w_full().h(px(1.0)).bg(theme.line))
            .child({
                let edits_for_all = edits_rc.clone();
                div()
                    .pt(px(8.0))
                    .flex()
                    .items_center()
                    .child(render_word_button(
                        "tab-edits-reset-all",
                        format!("Reset {}", tab.title()),
                        scale::WORD_BUTTON,
                        true,
                        Some(SharedString::from(format!(
                            "Put every control on {} back to its default. You can undo this.",
                            tab.title()
                        ))),
                        theme,
                        cx,
                        move |this, window, cx| {
                            this.inspector.open_tab_edits = None;
                            if let Some(effect) =
                                inspector_view::edit_list(tab, &edits_for_all, EditChoice::All)
                            {
                                apply_effects(this, vec![effect], window, cx);
                            }
                        },
                    ))
            })
            .into_any_element(),
    )
}

/// Floating dropdown popover for choice controls (`Kind::Choice`, `FilmFormat`, etc.).
fn render_choice_popover(
    open: &OpenChoice,
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> Option<AnyElement> {
    let row = view.rows.get(open.row_id)?;
    if !row.visible {
        return None;
    }
    let options = options_for_row(row);
    if options.is_empty() {
        return None;
    }
    let row_id = open.row_id;
    let selected = row.selected;
    let hover = theme.subtle_hover;
    // Anchor cleanly below the clicked button in column-local coordinates (offset by titlebar).
    let top_y = (open.anchor_y - crate::titlebar::TITLEBAR_HEIGHT + 14.0).clamp(48.0, 460.0);

    Some(
        div()
            .id("choice-popover-menu")
            .absolute()
            .top(px(top_y))
            .right(px(16.0))
            .w(px(208.0))
            .max_h(px(280.0))
            .py(px(4.0))
            .rounded(px(6.0))
            .bg(theme.panel)
            .border_1()
            .border_color(theme.track)
            .shadow_md()
            .flex()
            .flex_col()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .children(options.into_iter().enumerate().map(|(i, label)| {
                let is_sel = selected == Some(i);
                div()
                    .id(SharedString::from(format!("choice-opt-{row_id}-{i}")))
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(10.0))
                    .py(px(6.0))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.inspector.open_choice = None;
                        dispatch_row(this, row_id, RowAction::Select(i as isize), window, cx);
                    }))
                    .child(
                        div()
                            .text_size(px(12.0))
                            .text_color(if is_sel { theme.paper } else { theme.label })
                            .truncate()
                            .child(label),
                    )
                    .when(is_sel, |el| {
                        el.child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded_full()
                                .bg(theme.accent),
                        )
                    })
            }))
            .into_any_element(),
    )
}

/// Inline popover for picking border mat or keyline color (4 presets + custom hex).
fn render_color_picker_popover(
    picker: &OpenColorPicker,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let line = picker.line;
    let current = picker.current;
    let title = if line { "Line color" } else { "Border color" };
    let hover = theme.subtle_hover;

    div()
        .id("border-color-popover")
        .absolute()
        .bottom(px(86.0))
        .left(px(16.0))
        .right(px(16.0))
        .p(px(12.0))
        .rounded(px(6.0))
        .bg(theme.panel)
        .border_1()
        .border_color(theme.track)
        .shadow_md()
        .flex()
        .flex_col()
        .gap(px(10.0))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            cx.stop_propagation();
        })
        .child({
            let (title_font, title_size) = theme.serif_spec(14.0, 600.0, false);
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font(title_font)
                        .text_size(px(title_size))
                        .text_color(theme.paper)
                        .child(title),
                )
                .child(
                    div()
                        .id("color-picker-close")
                        .cursor_pointer()
                        .text_size(px(12.0))
                        .text_color(theme.dim)
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.inspector.open_color_picker = None;
                            cx.notify();
                        }))
                        .child("✕"),
                )
        })
        // Preset swatches: White, Black, Warm, Cream
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .children(BORDER_COLOR_PRESETS.into_iter().map(|(name, color)| {
                    let active = color == current;
                    div()
                        .id(SharedString::from(format!("bcol-preset-{name}")))
                        .flex_1()
                        .flex()
                        .flex_col()
                        .items_center()
                        .gap(px(4.0))
                        .p(px(5.0))
                        .rounded(px(4.0))
                        .bg(if active {
                            theme.accent.opacity(0.18)
                        } else {
                            theme.canvas
                        })
                        .border_1()
                        .border_color(if active { theme.accent } else { theme.line })
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _, _window, cx| {
                            this.model.set_border_color(line, color);
                            this.inspector.open_color_picker = None;
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child(
                            div()
                                .w(px(20.0))
                                .h(px(20.0))
                                .rounded(px(3.0))
                                .bg(border_color_hsla(color))
                                .border_1()
                                .border_color(theme.track),
                        )
                        .child(div().text_size(px(10.0)).text_color(theme.label).child(name))
                })),
        )
        // Custom hex input + Apply button
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(8.0))
                .child(render_text_input(
                    "border-color-hex-input",
                    &picker.hex_input,
                    TextInputStyle {
                        width: 140.0,
                        height: 26.0,
                        font_size: 12.0,
                        placeholder: SharedString::from("#FFFFFF"),
                        align_right: false,
                        error: picker.hex_error,
                        disabled: false,
                        ruled: false,
                        serif: false,
                    },
                    theme,
                    window,
                    cx,
                    |this, event, window, cx| {
                        let Some(ref mut p) = this.inspector.open_color_picker else {
                            return;
                        };
                        let outcome = p.hex_input.handle_key_event(event, cx);
                        match outcome {
                            TextInputOutcome::Commit(text) => {
                                if let Some(color) = BorderColor::from_hex(text.trim()) {
                                    let l = p.line;
                                    this.model.set_border_color(l, color);
                                    this.inspector.open_color_picker = None;
                                    window.focus(&this.focus_handle(cx));
                                    this.after_model_update(cx);
                                } else {
                                    p.hex_error = true;
                                }
                                cx.notify();
                            }
                            TextInputOutcome::Cancel => {
                                this.inspector.open_color_picker = None;
                                window.focus(&this.focus_handle(cx));
                                cx.notify();
                            }
                            TextInputOutcome::Changed => {
                                p.hex_error = false;
                                cx.notify();
                            }
                            TextInputOutcome::Handled => cx.notify(),
                            TextInputOutcome::Ignored => {}
                        }
                    },
                    |this, window, _cx| {
                        if let Some(p) = &this.inspector.open_color_picker {
                            window.focus(&p.hex_input.focus_handle);
                        }
                    },
                ))
                .child(
                    div()
                        .id("border-color-apply")
                        .px(px(10.0))
                        .py(px(4.5))
                        .rounded(px(4.0))
                        .bg(theme.fill)
                        .text_size(px(11.5))
                        .text_color(theme.on_fill)
                        .cursor_pointer()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if let Some(ref mut p) = this.inspector.open_color_picker {
                                let raw = p.hex_input.text().trim().to_string();
                                if let Some(color) = BorderColor::from_hex(&raw) {
                                    let l = p.line;
                                    this.model.set_border_color(l, color);
                                    this.inspector.open_color_picker = None;
                                    window.focus(&this.focus_handle(cx));
                                    this.after_model_update(cx);
                                } else {
                                    p.hex_error = true;
                                }
                                cx.notify();
                            }
                        }))
                        .child("Apply"),
                ),
        )
        .into_any_element()
}

/// Renders the entire collapsible right-hand Inspector column.
pub fn render_inspector_column(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> Option<AnyElement> {
    if !app.model.inspector_visible {
        return None;
    }

    app.inspector.scopes_cache.flush_drops(window);

    let view = build_inspector_view(&app.model);

    // Sync LongestSide text field when not actively focused and not holding a rejected input.
    if let Some(long_row) = view
        .rows
        .iter()
        .find(|r| r.visible && r.kind == Some(Kind::LongestSide))
    {
        if !app.inspector.longest_side_input.is_focused(window)
            && !long_row.keep_typed
            && app.inspector.longest_side_synced != long_row.value
        {
            app.inspector
                .longest_side_input
                .sync_external(long_row.value.clone());
            app.inspector.longest_side_synced = long_row.value.clone();
        }
    }

    let theme = app.theme.clone();
    let is_presets = app.model.pane() == Pane::Presets;

    // Resolve cached waveform RenderImage before immutable borrows below.
    let is_waveform = app.model.prefs.scope_mode == "Waveform";
    let picture_id = app.model.image.as_ref().map_or(0, |p| p.id);
    let waveform_img = if is_waveform {
        app.model
            .waveform
            .as_ref()
            .and_then(|wf| app.inspector.scopes_cache.resolve_waveform(wf, picture_id))
    } else {
        None
    };

    let pane_header = render_pane_header(app, &view, &theme, cx);

    let body = if is_presets {
        crate::presets::render_presets(app, window, cx).into_any_element()
    } else if app.model.source.is_none() {
        // Empty inspector (`InspectorEmptyLine`, `L13`): top rule + italic serif sentence.
        let (empty_font, empty_size) = theme.serif_spec(14.0, 400.0, true);
        div()
            .flex_1()
            .w_full()
            .flex()
            .flex_col()
            .child(div().w_full().h(px(1.0)).bg(theme.line))
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .px(px(GUTTER))
                    .pt(px(18.0))
                    .font(empty_font)
                    .text_size(px(empty_size))
                    .line_height(px(20.0))
                    .text_color(theme.dim)
                    .child("The film, the paper and every setting appear here once a photograph is open."),
            )
            .into_any_element()
    } else {
        let scopes = render_scopes(
            is_waveform,
            app.inspector.scope_height,
            app.inspector.active_scope_drag.is_some(),
            app.model.source.is_some(),
            app.model.histogram.clone(),
            waveform_img,
            &theme,
            cx,
            |this, mode, _window, cx| {
                if this.model.prefs.scope_mode != mode {
                    this.model.prefs.scope_mode = mode.into();
                    this.model.prefs.save();
                    this.model.dirty |= dirty::HISTOGRAM | dirty::INSPECTOR;
                    cx.notify();
                }
            },
            |this, event, _window, cx| {
                if event.click_count >= 2 {
                    this.inspector.scope_height = SCOPE_HEIGHTS.0;
                    this.model.prefs.scope_height = SCOPE_HEIGHTS.0;
                    this.model.prefs.save();
                    cx.notify();
                } else {
                    this.inspector.active_scope_drag = Some(ScopeHeightDrag {
                        start_y: f32::from(event.position.y),
                        start_height: this.inspector.scope_height,
                    });
                    cx.notify();
                }
            },
        );
        let tab_bar = render_tab_bar(app, &view, &theme, window, cx);

        // Render visible rows, grouping consecutive revealed sub-rows into `RevealedRows` (`L21`).
        let mut visible_elements: Vec<AnyElement> = Vec::new();
        let mut revealed_batch: Vec<AnyElement> = Vec::new();
        let mut prev_visible_kind: Option<Kind> = None;
        let flush_revealed = |batch: &mut Vec<AnyElement>, out: &mut Vec<AnyElement>, track: gpui::Hsla| {
            if batch.is_empty() {
                return;
            }
            let items = std::mem::take(batch);
            out.push(
                div()
                    .relative()
                    .w_full()
                    .pl(px(12.0))
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .absolute()
                            .left(px(GUTTER))
                            .top(px(4.0))
                            .bottom(px(4.0))
                            .w(px(1.0))
                            .bg(track),
                    )
                    .children(items)
                    .into_any_element(),
            );
        };

        for (idx, row) in view.rows.iter().filter(|r| r.visible).enumerate() {
            let el = render_row(app, row, idx == 0, prev_visible_kind, &theme, window, cx);
            prev_visible_kind = row.kind;
            let help = match row.kind {
                Some(Kind::MatchBrightness) => row.help.clone().map(SharedString::from),
                Some(kind) => kind.help().map(SharedString::from),
                None => None,
            };
            let wrapped = match help {
                Some(help) if !help.is_empty() => div()
                    .id(("row-tip", row.id))
                    .tooltip(crate::tooltip::tip(help, &theme))
                    .child(el)
                    .into_any_element(),
                _ => el,
            };
            if is_revealed_row(row.id) {
                revealed_batch.push(wrapped);
            } else {
                flush_revealed(&mut revealed_batch, &mut visible_elements, theme.track);
                visible_elements.push(wrapped);
            }
        }
        flush_revealed(&mut revealed_batch, &mut visible_elements, theme.track);
        // Close the final section with `9 px` bottom padding + `1 px` `line` rule (`L19`).
        if !visible_elements.is_empty() {
            visible_elements.push(
                div()
                    .w_full()
                    .pt(px(9.0))
                    .child(div().w_full().h(px(1.0)).bg(theme.line))
                    .into_any_element(),
            );
        }

        let scroll_area = div()
            .id("inspector-rows-scroll")
            .flex_1()
            .w_full()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&app.inspector.scroll_handle)
            .pb(px(16.0))
            .flex()
            .flex_col()
            .children(visible_elements);

        let output_foot =
            (view.tab == Tab::Output).then(|| render_output_foot(app, &view, &theme, cx));

        div()
            .flex_1()
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(scopes)
            .child(tab_bar)
            .child(scroll_area)
            .when_some(output_foot, |el, foot| el.child(foot))
            .into_any_element()
    };

    let tab_edits_popover = app
        .inspector
        .open_tab_edits
        .and_then(|tab| render_tab_edits_popover(app, tab, &theme, cx));
    let choice_popover = app
        .inspector
        .open_choice
        .as_ref()
        .and_then(|open| render_choice_popover(open, &view, &theme, cx));
    let color_popover = app
        .inspector
        .open_color_picker
        .as_ref()
        .map(|picker| render_color_picker_popover(picker, &theme, window, cx));
    let any_popover =
        tab_edits_popover.is_some() || choice_popover.is_some() || color_popover.is_some();

    Some(
        div()
            .id("inspector-column")
            .relative()
            .w(px(app.inspector_width))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .border_t_1()
            .border_l_1()
            .border_color(theme.line)
            .font_family(theme.ui_family.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    if this.inspector.open_choice.is_some()
                        || this.inspector.open_tab_edits.is_some()
                        || this.inspector.open_color_picker.is_some()
                    {
                        this.inspector.close_popovers();
                        cx.notify();
                    }
                }),
            )
            .child(pane_header)
            .child(body)
            .when(any_popover, |el| {
                el.child(
                    div()
                        .id("inspector-popover-backdrop")
                        .absolute()
                        .inset_0()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _event, _window, cx| {
                                cx.stop_propagation();
                                this.inspector.close_popovers();
                                cx.notify();
                            }),
                        ),
                )
            })
            .when_some(tab_edits_popover, |el, pop| el.child(pop))
            .when_some(choice_popover, |el, pop| el.child(pop))
            .when_some(color_popover, |el, pop| el.child(pop))
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_width_clamps_to_range_and_leaves_canvas_room() {
        assert_eq!(clamp_inspector_width(300.0, 1320.0), INSPECTOR_MIN_WIDTH);
        assert_eq!(clamp_inspector_width(380.0, 1320.0), 380.0);
        assert_eq!(clamp_inspector_width(500.0, 1320.0), INSPECTOR_MAX_WIDTH);
        // Window of 1000 px: max inspector leaving 640 px canvas is 360 px.
        assert_eq!(clamp_inspector_width(420.0, 1000.0), 360.0);
        // Narrow window: never goes below minimum 340 px.
        assert_eq!(clamp_inspector_width(420.0, 900.0), INSPECTOR_MIN_WIDTH);
    }

    #[test]
    fn tab_edits_anchor_tracks_scope_height() {
        let base = tab_edits_anchor_y(88.0);
        assert_eq!(base, 42.0 + 134.0 + 50.0 + 4.0);
        assert_eq!(tab_edits_anchor_y(188.0), base + 100.0);
    }

    #[test]
    fn revealed_rows_and_group_rules_match_swiftui() {
        let all = schema::rows();
        let wb_temp = all
            .iter()
            .position(|r| r.when == When::CustomWhiteBalance)
            .expect("custom WB row");
        assert!(is_revealed_row(wb_temp));
        let metering = all
            .iter()
            .position(|r| r.when == When::MeterAuto)
            .expect("metering row");
        assert!(is_revealed_row(metering));
        let long_side = all
            .iter()
            .position(|r| r.when == When::LongestSide)
            .expect("longest side row");
        assert!(is_revealed_row(long_side));
        let brightness_sec = all
            .iter()
            .position(|r| r.kind == Kind::Section("Brightness"))
            .expect("brightness section");
        assert!(!is_revealed_row(brightness_sec));

        assert!(!group_is_ruled(None));
        assert!(!group_is_ruled(Some(Kind::Section("Film"))));
        assert!(!group_is_ruled(Some(Kind::Footer(
            "Your negative is read through the film's curves and base."
        ))));
        assert!(group_is_ruled(Some(Kind::Toggle("Scan film directly", "io.scan_film"))));
        assert!(group_is_ruled(Some(Kind::Group {
            title: "Grain",
            enable: Some("film_render.grain.active")
        })));
    }
    use negs_app::border::{BorderSettings, BorderStyle};
    use negs_app::inspector_view::output::BorderWidth;
    use negs_app::inspector_view::schema::rows;
    use negs_app::json;
    use negs_app::session::Preferences;
    use negs_app::state::{ColorFilter, EditState, FilterLink, SizeMode, Stock};
    use serde_json::{Value, json};

    fn test_model() -> Model {
        let mut model = Model::new(Preferences::default());
        model.stocks = vec![
            Stock {
                id: "kodak_gold_200".into(),
                name: "Kodak Gold 200".into(),
                paper: false,
                positive: false,
                bw: false,
                target: "kodak_portra_endura".into(),
                times: vec![3.25, 4.5],
                grain: json!({"rms_granularity": [12, 13, 12], "agx_particle_area_um2": [0.2, 0.2, 0.2]}),
                fotufilm: false,
                manufacturer_data: false,
                negs_built: false,
                cine_print: false,
            },
            Stock {
                id: "ilford_hp5".into(),
                name: "Ilford HP5 Plus".into(),
                paper: false,
                positive: false,
                bw: true,
                target: "ilford_multigrade".into(),
                times: vec![7.5, 11.0],
                grain: json!({"rms_granularity": [16, 16, 16], "agx_particle_area_um2": [0.3, 0.3, 0.3]}),
                fotufilm: false,
                manufacturer_data: false,
                negs_built: false,
                cine_print: false,
            },
            Stock {
                id: "kodak_portra_endura".into(),
                name: "Kodak Portra Endura".into(),
                paper: true,
                positive: false,
                bw: false,
                target: String::new(),
                times: vec![1.0, 2.0],
                grain: Value::Null,
                fotufilm: false,
                manufacturer_data: false,
                negs_built: false,
                cine_print: false,
            },
        ];
        model.color_filters = vec![
            ColorFilter {
                key: "yellow".into(),
                name: "Yellow".into(),
                glass: "GG495".into(),
                colour: 0xe6c832,
            },
            ColorFilter {
                key: "red".into(),
                name: "Red".into(),
                glass: "RG610".into(),
                colour: 0xd93829,
            },
        ];
        model.defaults = json!({
            "io": {
                "scan_film": false,
                "rotate_cw": 0,
                "output_cctf_encoding": true,
                "input_interpretation": {"mode": "auto", "color_space": "sRGB", "linear": false},
                "input_color_space": "sRGB",
                "input_cctf_decoding": true,
                "raw_white_balance": {"mode": "as_shot", "temperature": 5500, "tint": 0},
                "lens_correction": {"enabled": true},
                "photo_long_side_px": null
            },
            "prep": {
                "exposure_ev": 0.0,
                "contrast": 0.0,
                "highlights": 0.0,
                "shadows": 0.0,
                "whites": 0.0,
                "blacks": 0.0,
                "saturation": 0.0,
                "vibrance": 0.0,
                "warmth": 0.0,
                "tint": 0.0,
                "clarity": 0.0,
                "sharpen": 0.0
            },
            "camera": {
                "auto_exposure": true,
                "auto_exposure_method": "center_weighted",
                "exposure_compensation_ev": 0.0,
                "film_format_mm": 36.0,
                "lens_blur_um": 0.0,
                "color_filter": "none"
            },
            "enlarger": {
                "print_exposure": 1.0,
                "y_filter_shift": 0.0,
                "m_filter_shift": 0.0,
                "c_filter_shift": 0.0
            },
            "print_render": {
                "density_curves_morph": {
                    "active": false,
                    "gamma_factor": 1.0,
                    "gamma_factor_fast": 1.0,
                    "gamma_factor_slow": 1.0
                }
            }
        });
        model.state = EditState {
            params: model.defaults.clone(),
            film: "kodak_gold_200".into(),
            paper: "kodak_portra_endura".into(),
            ..Default::default()
        };
        model.source_size = (6000, 4000);
        model.image_size = negs_app::crop::Size::new(6000.0, 4000.0);
        model
    }

    fn kind_discriminant(k: Kind) -> &'static str {
        match k {
            Kind::Section(_) => "Section",
            Kind::Footer(_) => "Footer",
            Kind::Group { .. } => "Group",
            Kind::Slider(_) => "Slider",
            Kind::BorderSlider(..) => "BorderSlider",
            Kind::Toggle(..) => "Toggle",
            Kind::MatchBrightness => "MatchBrightness",
            Kind::ExposureOffset => "ExposureOffset",
            Kind::FilmFormat => "FilmFormat",
            Kind::More(_) => "More",
            Kind::PreviewSpace => "PreviewSpace",
            Kind::SizeMode => "SizeMode",
            Kind::LongestSide => "LongestSide",
            Kind::Photograph => "Photograph",
            Kind::Note(_) => "Note",
            Kind::BorderToggle(..) => "BorderToggle",
            Kind::Choice(..) => "Choice",
            Kind::DevTime(_) => "DevTime",
            Kind::BorderStyles => "BorderStyles",
            Kind::Color(_) => "Color",
            Kind::BalancePrint(..) => "BalancePrint",
            Kind::FilterLink(_) => "FilterLink",
            Kind::ToneGraph => "ToneGraph",
            Kind::ColorFilter => "ColorFilter",
            Kind::Actions(_) => "Actions",
            Kind::FlatField => "FlatField",
        }
    }

    #[test]
    fn inspector_view_covers_every_schema_kind_across_modes_and_tabs() {
        let mut seen = std::collections::HashSet::new();

        // Scenario 1: RAW, show all controls, expanded all, B&W film (ColorFilter + DevTime + ToneGraph + Note).
        let mut m = test_model();
        m.is_raw = true;
        m.lens_supported = true;
        m.prefs.lab_all_controls = true;
        m.state.film = "ilford_hp5".into();
        m.size_error = Some("Enter a whole number from 256 to 16000 px.".into());
        json::set(&mut m.state.params, "camera.film_format_mm", json!(50.0));
        json::set(&mut m.state.params, "io.photo_long_side_px", json!(3000));
        m.state.border = BorderSettings {
            enabled: true,
            style: BorderStyle::Keyline,
            ..Default::default()
        };
        for tab in Tab::ALL {
            let facts = m.inspector_facts();
            let ui = InspectorUi {
                tab,
                expanded: Expanded::All,
                decimal: '.',
            };
            let view = inspector_view::build(&facts, &ui);
            assert_eq!(view.rows.len(), rows().len());
            for row in view.rows.iter().filter(|r| r.visible) {
                let k = row.kind.expect("visible row has kind");
                seen.insert(kind_discriminant(k));
                // Verify options_for_row never panics on any visible row.
                let _ = options_for_row(row);
            }
        }

        // Scenario 2: Negative mode on Prep + Lab (Actions, FlatField, Note, Footer, Backlight More).
        let mut neg = test_model();
        neg.is_raw = true;
        json::set(
            &mut neg.state.params,
            negs_app::negative::ROUTE_PATH,
            json!(negs_app::negative::ROUTE),
        );
        neg.negative.backlight_shown = false;
        for tab in [Tab::Prep, Tab::Lab] {
            let facts = neg.inspector_facts();
            let ui = InspectorUi {
                tab,
                expanded: Expanded::All,
                decimal: '.',
            };
            let view = inspector_view::build(&facts, &ui);
            for row in view.rows.iter().filter(|r| r.visible) {
                seen.insert(kind_discriminant(row.kind.unwrap()));
            }
        }

        // Scenario 3: Scan film directly skips Print section & Print groups.
        let mut scan = test_model();
        json::set(&mut scan.state.params, "io.scan_film", json!(true));
        let facts = scan.inspector_facts();
        let ui = InspectorUi {
            tab: Tab::Lab,
            expanded: Expanded::All,
            decimal: '.',
        };
        let view = inspector_view::build(&facts, &ui);
        assert!(
            view.rows
                .iter()
                .any(|r| r.visible && r.kind == Some(Kind::Section("Print")) && r.skipped)
        );
        assert!(!view.tone_graph, "tone graph hides while scanning film");

        // Verify all 26 Kind variants appeared across the scenarios.
        for expected in [
            "Section",
            "Footer",
            "Group",
            "Slider",
            "BorderSlider",
            "Toggle",
            "MatchBrightness",
            "ExposureOffset",
            "FilmFormat",
            "More",
            "PreviewSpace",
            "SizeMode",
            "LongestSide",
            "Photograph",
            "Note",
            "BorderToggle",
            "Choice",
            "DevTime",
            "BorderStyles",
            "Color",
            "BalancePrint",
            "FilterLink",
            "ToneGraph",
            "ColorFilter",
            "Actions",
            "FlatField",
        ] {
            assert!(
                seen.contains(expected),
                "Kind::{expected} was not exercised by inspector scenarios"
            );
        }
    }

    #[test]
    fn apply_model_effect_updates_model_and_returns_dialog_effects() {
        let mut m = test_model();

        // Set / Reset
        assert_eq!(
            apply_model_effect(&mut m, Effect::Set("prep.contrast", json!(0.25))),
            None
        );
        assert!((json::number(m.value("prep.contrast")) - 0.25).abs() < 1e-9);
        assert_eq!(
            apply_model_effect(&mut m, Effect::ResetValue("prep.contrast")),
            None
        );
        assert!((json::number(m.value("prep.contrast")) - 0.0).abs() < 1e-9);

        // Tone graph channel & group expanded state
        assert_eq!(
            apply_model_effect(&mut m, Effect::ToneGraphChannel("r")),
            None
        );
        assert_eq!(m.prefs.tone_graph_channel, "r");

        assert_eq!(
            apply_model_effect(&mut m, Effect::SetExpanded("Print Curves", true)),
            None
        );
        assert!(m.prefs.expanded_adjustments.contains("Print Curves"));

        // More reveal / conceal / show fewer
        assert_eq!(
            apply_model_effect(&mut m, Effect::More(MoreAction::Reveal("Enlarger".into()))),
            None
        );
        assert!(m.prefs.more_adjustments.contains("Enlarger"));
        assert_eq!(
            apply_model_effect(&mut m, Effect::More(MoreAction::Conceal("Enlarger".into()))),
            None
        );
        assert!(!m.prefs.more_adjustments.contains("Enlarger"));
        m.prefs.lab_all_controls = true;
        assert_eq!(
            apply_model_effect(&mut m, Effect::More(MoreAction::ShowFewer)),
            None
        );
        assert!(!m.prefs.lab_all_controls);

        // Border, image size, filter link, show backlight
        assert_eq!(
            apply_model_effect(
                &mut m,
                Effect::Border(inspector_view::BorderChange::Enabled(true), "Change Border")
            ),
            None
        );
        assert!(m.state.border.enabled);

        assert_eq!(
            apply_model_effect(&mut m, Effect::SetBorderPercent(BorderWidth::Mat, 5.0)),
            None
        );
        assert!((m.state.border.width - 0.05).abs() < 1e-6);

        assert_eq!(
            apply_model_effect(&mut m, Effect::SetFilterLink(FilterLink::Together)),
            None
        );
        assert_eq!(m.filter_link(), FilterLink::Together);

        assert_eq!(
            apply_model_effect(&mut m, Effect::ChooseImageSize(SizeMode::LongestSide)),
            None
        );
        assert_eq!(m.size_mode(), SizeMode::LongestSide);

        assert_eq!(apply_model_effect(&mut m, Effect::ShowBacklight), None);
        assert!(m.negative.backlight_shown);

        // Dialog effects pass through for UI handling
        assert_eq!(
            apply_model_effect(&mut m, Effect::ChooseLightFrame),
            Some(Effect::ChooseLightFrame)
        );
        let col_eff = Effect::ChooseBorderColor {
            line: true,
            current: BorderColor::BLACK,
        };
        assert_eq!(apply_model_effect(&mut m, col_eff.clone()), Some(col_eff));
    }

    #[test]
    fn tab_edits_list_resets_single_edit_and_whole_tab() {
        let mut m = test_model();
        m.source = Some(std::path::PathBuf::from("/tmp/test.nef"));
        apply_model_effect(&mut m, Effect::Set("prep.contrast", json!(0.3)));
        apply_model_effect(&mut m, Effect::Set("prep.warmth", json!(0.2)));

        let facts = m.inspector_facts();
        let list = edits::edits(Tab::Prep, &facts, '.');
        assert_eq!(list.len(), 2);

        // Reset one edit
        let eff_one = inspector_view::edit_list(Tab::Prep, &list, EditChoice::One(0)).unwrap();
        assert_eq!(apply_model_effect(&mut m, eff_one), None);
        assert_eq!(edits::edits(Tab::Prep, &m.inspector_facts(), '.').len(), 1);

        // Reset whole tab
        let remaining = edits::edits(Tab::Prep, &m.inspector_facts(), '.');
        let eff_all = inspector_view::edit_list(Tab::Prep, &remaining, EditChoice::All).unwrap();
        assert_eq!(apply_model_effect(&mut m, eff_all), None);
        assert_eq!(edits::edits(Tab::Prep, &m.inspector_facts(), '.').len(), 0);
    }
}
