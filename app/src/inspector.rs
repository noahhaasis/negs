//! Complete Prep / Lab / Output Inspector driven directly from
//! `negs_app::inspector_view::InspectorView`.

use std::collections::HashMap;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, Focusable, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, PathPromptOptions, Pixels, ScrollHandle, SharedString, Window, div, point,
    prelude::*, px,
};
use negs_app::border::BorderColor;
use negs_app::edits::{self, MoreAction, Tab};
use negs_app::inspector_view::{
    self, EditChoice, Effect, Expanded, InspectorUi, InspectorView, Kind, NegativeAction,
    RowAction, RowId, RowView, SliderValue, Track,
};
use negs_app::json;
use negs_app::model::{Model, Pane, dirty};
use negs_app::negative::Pick;
use negs_app::session::SCOPE_HEIGHTS;
use negs_app::state;

use crate::NegsApp;
use crate::theme_gpui::GpuiTheme;
use crate::widgets::color_filter::render_color_filter;
use crate::widgets::controls::{
    BORDER_COLOR_PRESETS, border_color_hsla, render_actions_row, render_balance_print_row,
    render_border_styles_row, render_choice_row, render_color_row, render_flat_field_row,
    render_group_header, render_longest_side_row, render_more_row, render_note_row,
    render_photograph_row, render_section_header, render_toggle_row,
};
use crate::widgets::scopes::{ScopeHeightDrag, ScopesCache, render_scopes};
use crate::widgets::slider::{SliderCallbacks, SliderDrag, render_ink_slider};
use crate::widgets::text_input::{
    InlineTextState, TextInputOutcome, TextInputStyle, parse_number, parse_pixels,
    render_text_input,
};
use crate::widgets::tone_graph::{ToneGraphState, render_tone_graph};

pub const INSPECTOR_WIDTH: f32 = 340.0;

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
fn render_row(
    app: &NegsApp,
    row: &RowView,
    is_first_visible: bool,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let Some(kind) = row.kind else {
        return div().into_any_element();
    };
    let row_id = row.id;

    match kind {
        Kind::Section(_) => render_section_header(row, is_first_visible, theme).into_any_element(),

        Kind::Footer(_) | Kind::Note(_) => render_note_row(row, theme).into_any_element(),

        Kind::Group { enable, .. } => render_group_header(
            row,
            enable.is_some(),
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
            render_border_styles_row(row, &options, theme, cx, move |this, idx, window, cx| {
                dispatch_row(this, row_id, RowAction::Press(idx), window, cx);
            })
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

/// Top header bar of the Inspector column: `Adjustments` · `Presets` + right actions.
fn render_pane_header(
    app: &NegsApp,
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let pane = app.model.pane();
    let is_presets = pane == Pane::Presets;
    let can_reset = app.model.source.is_some() && view.total > 0;
    let can_presets = app.model.can_change_presets();
    let hover_paper = theme.paper;

    let count_label = if view.total == 0 {
        None
    } else if view.total == 1 {
        Some("1 edit".to_string())
    } else {
        Some(format!("{} edits", view.total))
    };

    div()
        .flex()
        .items_center()
        .justify_between()
        .w_full()
        .h(px(40.0))
        .px(px(16.0))
        .border_b_1()
        .border_color(theme.line)
        // Left: Adjustments · Presets
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.0))
                .child(
                    div()
                        .id("pane-tab-adjustments")
                        .cursor_pointer()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(14.0))
                        .text_color(if !is_presets { theme.paper } else { theme.dim })
                        .hover(move |s| s.text_color(hover_paper))
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
                        .cursor_pointer()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(14.0))
                        .text_color(if is_presets { theme.paper } else { theme.dim })
                        .hover(move |s| s.text_color(hover_paper))
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.inspector.close_popovers();
                            this.model.show_pane(Pane::Presets);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child("Presets"),
                ),
        )
        // Right: edit count + Reset, or New + Import…
        .child(if !is_presets {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .when_some(count_label, |el, label| {
                    el.child(div().text_size(px(11.0)).text_color(theme.dim).child(label))
                })
                .child(
                    div()
                        .id("pane-reset-all")
                        .text_size(px(11.5))
                        .text_color(if can_reset {
                            theme.label
                        } else {
                            theme.dim.opacity(0.45)
                        })
                        .when(can_reset, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(hover_paper))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.inspector.close_popovers();
                                    this.model.reset();
                                    this.after_model_update(cx);
                                    cx.notify();
                                }))
                        })
                        .child("Reset"),
                )
                .into_any_element()
        } else {
            div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(
                    div()
                        .id("pane-presets-new")
                        .text_size(px(11.5))
                        .text_color(if can_presets {
                            theme.label
                        } else {
                            theme.dim.opacity(0.45)
                        })
                        .when(can_presets, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(hover_paper))
                                .on_click(cx.listener(|this, _, _window, cx| {
                                    this.model.new_preset();
                                    this.after_model_update(cx);
                                    cx.notify();
                                }))
                        })
                        .child("New"),
                )
                .child(
                    div()
                        .id("pane-presets-import")
                        .text_size(px(11.5))
                        .text_color(if can_presets {
                            theme.label
                        } else {
                            theme.dim.opacity(0.45)
                        })
                        .when(can_presets, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.text_color(hover_paper))
                                .on_click(cx.listener(|_this, _, _window, cx| {
                                    let receiver = cx.prompt_for_paths(PathPromptOptions {
                                        files: true,
                                        directories: false,
                                        multiple: true,
                                        prompt: Some(SharedString::from("Import Presets")),
                                    });
                                    cx.spawn(async move |this, cx| {
                                        if let Ok(Ok(Some(paths))) = receiver.await {
                                            if !paths.is_empty() {
                                                let _ = this.update(cx, |app, cx| {
                                                    app.model.import_presets(paths, None);
                                                    app.after_model_update(cx);
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    })
                                    .detach();
                                }))
                        })
                        .child("Import…"),
                )
                .into_any_element()
        })
        .into_any_element()
}

/// Pinned Film & Paper stock selector strip at the top of Adjustments.
/// Clicking Film calls `model.toggle_tray(false)` and clicking Paper calls
/// `model.toggle_tray(true)` so Phase 3b's Stock Tray opens seamlessly.
fn render_stock_strip(
    app: &NegsApp,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let film_name = app.model.film_name();
    let paper_name = app.model.paper_name();
    let scanning = json::boolean(app.model.value("io.scan_film"));
    let tray_shown = app.model.tray_shown();
    let film_open = tray_shown == Some(false);
    let paper_open = tray_shown == Some(true);
    let hover = theme.subtle_hover;

    div()
        .w_full()
        .px(px(16.0))
        .pt(px(10.0))
        .pb(px(6.0))
        .flex()
        .flex_col()
        .gap(px(5.0))
        .border_b_1()
        .border_color(theme.line)
        // Film row
        .child(
            div()
                .id("stock-strip-film")
                .flex()
                .items_center()
                .justify_between()
                .px(px(9.0))
                .py(px(6.0))
                .rounded(px(4.0))
                .bg(if film_open {
                    theme.accent.opacity(0.14)
                } else {
                    theme.canvas
                })
                .border_1()
                .border_color(if film_open { theme.accent } else { theme.line })
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(|this, _, _window, cx| {
                    this.inspector.close_popovers();
                    this.model.toggle_tray(false);
                    this.after_model_update(cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.0))
                        .min_w_0()
                        .child(div().text_size(px(10.5)).text_color(theme.dim).child("Film"))
                        .child(
                            div()
                                .font_family(theme.serif_family.clone())
                                .text_size(px(13.5))
                                .text_color(theme.paper)
                                .truncate()
                                .child(if film_name.is_empty() {
                                    "Loading catalog…".to_string()
                                } else {
                                    film_name
                                }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(if film_open { theme.accent } else { theme.dim })
                        .child(if film_open { "▴" } else { "▾" }),
                ),
        )
        // Paper row
        .child(
            div()
                .id("stock-strip-paper")
                .flex()
                .items_center()
                .justify_between()
                .px(px(9.0))
                .py(px(6.0))
                .rounded(px(4.0))
                .bg(if paper_open {
                    theme.accent.opacity(0.14)
                } else {
                    theme.canvas
                })
                .border_1()
                .border_color(if paper_open { theme.accent } else { theme.line })
                .when(!scanning, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(|this, _, _window, cx| {
                            this.inspector.close_popovers();
                            this.model.toggle_tray(true);
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                })
                .when(scanning, |el| el.opacity(0.5))
                .child(
                    div()
                        .flex()
                        .items_baseline()
                        .gap(px(8.0))
                        .min_w_0()
                        .child(div().text_size(px(10.5)).text_color(theme.dim).child("Paper"))
                        .child(
                            div()
                                .font_family(theme.serif_family.clone())
                                .text_size(px(13.5))
                                .text_color(if scanning { theme.dim } else { theme.paper })
                                .when(scanning, |el| el.line_through())
                                .truncate()
                                .child(if paper_name.is_empty() {
                                    "—".to_string()
                                } else {
                                    paper_name
                                }),
                        ),
                )
                .child(
                    div()
                        .text_size(px(10.5))
                        .text_color(if paper_open { theme.accent } else { theme.dim })
                        .child(if paper_open { "▴" } else { "▾" }),
                ),
        )
        .into_any_element()
}

/// Tab bar (`Prep` · `Lab` · `Output`) with per-tab edit count badges.
fn render_tab_bar(
    app: &NegsApp,
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let hover = theme.subtle_hover;
    let active_tab = view.tab;

    div()
        .flex()
        .items_center()
        .w_full()
        .h(px(34.0))
        .px(px(12.0))
        .gap(px(4.0))
        .border_t_1()
        .border_b_1()
        .border_color(theme.line)
        .children(Tab::ALL.into_iter().enumerate().map(|(i, tab)| {
            let active = tab == active_tab;
            let count = view.counts[i];
            let popover_open = app.inspector.open_tab_edits == Some(tab);
            div()
                .id(SharedString::from(format!("insp-tab-{}", tab.title())))
                .relative()
                .flex_1()
                .h_full()
                .flex()
                .items_center()
                .justify_center()
                .gap(px(5.0))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
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
                .child(
                    div()
                        .text_size(px(12.0))
                        .text_color(if active { theme.paper } else { theme.dim })
                        .child(tab.title()),
                )
                .when(count > 0, |el| {
                    el.child(
                        div()
                            .id(SharedString::from(format!(
                                "insp-tab-count-{}",
                                tab.title()
                            )))
                            .px(px(5.0))
                            .py(px(1.0))
                            .rounded_full()
                            .bg(if popover_open {
                                theme.accent
                            } else {
                                theme.accent.opacity(0.22)
                            })
                            .text_size(px(10.0))
                            .text_color(if popover_open {
                                theme.on_fill
                            } else {
                                theme.paper
                            })
                            .cursor_pointer()
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
                            .child(count.to_string()),
                    )
                })
                .when(active, |el| {
                    el.child(
                        div()
                            .absolute()
                            .bottom_0()
                            .left(px(8.0))
                            .right(px(8.0))
                            .h(px(2.0))
                            .bg(theme.accent),
                    )
                })
        }))
        .into_any_element()
}

/// Output tab footer with `view.export_summary` and primary Export button.
fn render_output_foot(
    view: &InspectorView,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> AnyElement {
    let can_export = view.can_export;
    div()
        .w_full()
        .px(px(16.0))
        .py(px(12.0))
        .border_t_1()
        .border_color(theme.line)
        .flex()
        .flex_col()
        .gap(px(8.0))
        .when(!view.export_summary.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .text_color(theme.dim)
                    .child(view.export_summary.clone()),
            )
        })
        .child(
            div()
                .id("inspector-export-btn")
                .w_full()
                .py(px(7.0))
                .rounded(px(4.0))
                .flex()
                .items_center()
                .justify_center()
                .bg(if can_export { theme.fill } else { theme.track })
                .text_size(px(12.5))
                .text_color(if can_export {
                    theme.on_fill
                } else {
                    theme.dim
                })
                .when(can_export, |el| {
                    el.cursor_pointer().on_click(cx.listener(|this, _, window, cx| {
                        this.prompt_save_preview(window, cx);
                    }))
                })
                .child("Export…"),
        )
        .into_any_element()
}

/// Floating Tab Edits popover showing every modified edit in `tab` with
/// per-edit reset and `"Reset <Tab>"`.
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
    let hover = theme.subtle_hover;

    Some(
        div()
            .id("tab-edits-popover")
            .absolute()
            .top(px(120.0))
            .left(px(12.0))
            .right(px(12.0))
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
            // Header
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(12.0))
                    .py(px(8.0))
                    .border_b_1()
                    .border_color(theme.line)
                    .child(
                        div()
                            .font_family(theme.serif_family.clone())
                            .text_size(px(13.0))
                            .text_color(theme.paper)
                            .child(head),
                    )
                    .child(
                        div()
                            .id("tab-edits-close")
                            .cursor_pointer()
                            .px(px(4.0))
                            .text_size(px(12.0))
                            .text_color(theme.dim)
                            .on_click(cx.listener(|this, _, _window, cx| {
                                this.inspector.open_tab_edits = None;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            // Individual edits list
            .child(
                div()
                    .flex()
                    .flex_col()
                    .max_h(px(260.0))
                    .py(px(4.0))
                    .children(edits_rc.iter().enumerate().map(|(i, edit)| {
                        let edits_for_click = edits_rc.clone();
                        div()
                            .id(SharedString::from(format!("tab-edit-item-{i}")))
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap_2()
                            .px(px(12.0))
                            .py(px(5.0))
                            .hover(move |s| s.bg(hover))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.label)
                                    .truncate()
                                    .child(edit.title.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(8.0))
                                    .child(
                                        div()
                                            .text_size(px(11.0))
                                            .text_color(theme.dim)
                                            .child(edit.value.clone()),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("tab-edit-reset-{i}")))
                                            .cursor_pointer()
                                            .px(px(4.0))
                                            .py(px(1.0))
                                            .rounded(px(3.0))
                                            .text_size(px(11.0))
                                            .text_color(theme.accent)
                                            .on_click(cx.listener(move |this, _, window, cx| {
                                                if let Some(effect) = inspector_view::edit_list(
                                                    tab,
                                                    &edits_for_click,
                                                    EditChoice::One(i),
                                                ) {
                                                    apply_effects(this, vec![effect], window, cx);
                                                }
                                            }))
                                            .child("↺"),
                                    ),
                            )
                    })),
            )
            // Footer: Reset <Tab>
            .child({
                let edits_for_all = edits_rc.clone();
                div()
                    .border_t_1()
                    .border_color(theme.line)
                    .px(px(12.0))
                    .py(px(7.0))
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("tab-edits-reset-all")
                            .cursor_pointer()
                            .px(px(8.0))
                            .py(px(3.0))
                            .rounded(px(4.0))
                            .text_size(px(11.5))
                            .text_color(theme.warn)
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.inspector.open_tab_edits = None;
                                if let Some(effect) =
                                    inspector_view::edit_list(tab, &edits_for_all, EditChoice::All)
                                {
                                    apply_effects(this, vec![effect], window, cx);
                                }
                            }))
                            .child(format!("Reset {}", tab.title())),
                    )
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
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .font_family(theme.serif_family.clone())
                        .text_size(px(13.0))
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
                ),
        )
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

/// Clean placeholder for `Pane::Presets` that Phase 3b replaces with the full
/// Presets browser & live-preview cards.
pub fn render_presets_placeholder(app: &NegsApp, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let theme = &app.theme;
    let count = app.model.presets.len();
    let subtitle = if count == 0 {
        "No saved presets yet. Use New or Import… above.".to_string()
    } else if count == 1 {
        "1 preset available in your library.".to_string()
    } else {
        format!("{count} presets available in your library.")
    };

    let _ = cx;
    div()
        .id("presets-pane-placeholder")
        .flex_1()
        .w_full()
        .min_h_0()
        .px(px(20.0))
        .py(px(28.0))
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .child(
            div()
                .font_family(theme.serif_family.clone())
                .text_size(px(16.0))
                .text_color(theme.paper)
                .child("Presets"),
        )
        .child(
            div()
                .text_size(px(12.0))
                .text_color(theme.dim)
                .child(subtitle),
        )
}

/// Compact fallback stock selector shown when `model.tray_shown()` is `Some(paper)`.
/// Phase 3b replaces this single hook with the full searchable Stock Tray.
pub fn render_stock_tray_fallback(
    app: &NegsApp,
    theme: &GpuiTheme,
    cx: &mut Context<NegsApp>,
) -> Option<AnyElement> {
    let paper = app.model.tray_shown()?;
    let current_id = if paper {
        app.model.state.paper.clone()
    } else {
        app.model.state.film.clone()
    };
    let stocks: Vec<(String, String)> = if paper {
        app.model
            .listed_papers()
            .into_iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    } else {
        app.model
            .listed_films()
            .into_iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    };
    let heading = if paper { "Paper stocks" } else { "Film stocks" };
    let hover = theme.subtle_hover;

    Some(
        div()
            .id("stock-tray-fallback")
            .w_full()
            .max_h(px(220.0))
            .bg(theme.film)
            .border_b_1()
            .border_color(theme.line)
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(16.0))
                    .py(px(6.0))
                    .border_b_1()
                    .border_color(theme.line)
                    .child(
                        div()
                            .text_size(px(10.5))
                            .text_color(theme.dim)
                            .child(format!("{heading} ({})", stocks.len())),
                    )
                    .child(
                        div()
                            .id("stock-tray-fallback-close")
                            .cursor_pointer()
                            .text_size(px(11.0))
                            .text_color(theme.dim)
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                this.model.toggle_tray(paper);
                                cx.notify();
                            }))
                            .child("Close"),
                    ),
            )
            .child(
                div()
                    .id("stock-tray-fallback-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py(px(3.0))
                    .flex()
                    .flex_col()
                    .children(stocks.into_iter().enumerate().map(|(i, (id, name))| {
                        let selected = id == current_id;
                        div()
                            .id(SharedString::from(format!("tray-fb-item-{i}")))
                            .flex()
                            .items_center()
                            .justify_between()
                            .px(px(16.0))
                            .py(px(4.5))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(move |this, _, _window, cx| {
                                if paper {
                                    this.model.choose_paper(&id);
                                } else {
                                    this.model.choose_film(&id);
                                }
                                this.model.toggle_tray(paper);
                                this.after_model_update(cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .font_family(theme.serif_family.clone())
                                    .text_size(px(12.5))
                                    .text_color(if selected { theme.paper } else { theme.label })
                                    .truncate()
                                    .child(name),
                            )
                            .when(selected, |el| {
                                el.child(
                                    div()
                                        .w(px(5.0))
                                        .h(px(5.0))
                                        .rounded_full()
                                        .bg(theme.accent),
                                )
                            })
                    })),
            )
            .into_any_element(),
    )
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
        render_presets_placeholder(app, cx).into_any_element()
    } else {
        let stock_strip = render_stock_strip(app, &theme, cx);
        let tray_fallback = render_stock_tray_fallback(app, &theme, cx);
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
        let tab_bar = render_tab_bar(app, &view, &theme, cx);

        let visible_rows: Vec<AnyElement> = view
            .rows
            .iter()
            .filter(|r| r.visible)
            .enumerate()
            .map(|(idx, row)| render_row(app, row, idx == 0, &theme, window, cx))
            .collect();

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
            .children(visible_rows);

        let output_foot = (view.tab == Tab::Output).then(|| render_output_foot(&view, &theme, cx));

        div()
            .flex_1()
            .w_full()
            .min_h_0()
            .flex()
            .flex_col()
            .child(stock_strip)
            .when_some(tray_fallback, |el, fb| el.child(fb))
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
            .w(px(INSPECTOR_WIDTH))
            .h_full()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .border_l_1()
            .border_color(theme.line)
            .font_family(theme.ui_family.clone())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _event, _window, cx| {
                    if this.theme_menu_open {
                        this.theme_menu_open = false;
                        cx.notify();
                    }
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
    use negs_app::border::{BorderSettings, BorderStyle};
    use negs_app::inspector_view::output::BorderWidth;
    use negs_app::inspector_view::schema::rows;
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
