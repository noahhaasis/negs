//! Reusable single-line text input widget for GPUI (`TextInput` key context).
//!
//! Designed for inline slider numeric editing, Image Size `LongestSide`, border
//! hex color editing, and Phase 3b's Presets rename & Stock Tray search.

use std::ops::Range;

use gpui::{
    App, ClipboardItem, Context, FocusHandle, IntoElement, KeyDownEvent, MouseButton,
    SharedString, Window, div, prelude::*, px,
};

use crate::theme_gpui::GpuiTheme;

/// Pure editing buffer for a single-line text field (UTF-8 safe, zero GPUI dependency).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBuffer {
    pub text: String,
    pub original: String,
    /// Byte index in `text` (always on a valid UTF-8 char boundary).
    pub cursor: usize,
    /// Selection anchor byte index, when a selection is active.
    pub anchor: Option<usize>,
}

/// Logical outcome of processing a key event on a `TextBuffer`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputAction {
    None,
    Changed,
    Commit(String),
    Cancel,
    Copy(String),
    Cut(String),
}

/// Outcome returned to views after `InlineTextState::handle_key_event` processes
/// a `KeyDownEvent` (including clipboard interactions).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextInputOutcome {
    Ignored,
    Handled,
    Changed,
    Commit(String),
    Cancel,
}

impl TextBuffer {
    pub fn new(initial: impl Into<String>) -> Self {
        let text = initial.into();
        let len = text.len();
        Self {
            original: text.clone(),
            text,
            cursor: len,
            anchor: None,
        }
    }

    /// Replaces the text and remembers it as the Escape restore baseline.
    pub fn begin(&mut self, value: impl Into<String>, select_all: bool) {
        let text = value.into();
        let len = text.len();
        self.original = text.clone();
        self.text = text;
        self.cursor = len;
        self.anchor = if select_all && len > 0 { Some(0) } else { None };
    }

    /// Updates displayed text externally without treating it as a user edit.
    pub fn sync_external(&mut self, value: impl Into<String>) {
        let text = value.into();
        let len = text.len();
        self.original = text.clone();
        self.text = text;
        self.cursor = self.cursor.min(len);
        self.cursor = floor_char_boundary(&self.text, self.cursor);
        self.anchor = None;
    }

    /// Active non-empty byte range selection, if any.
    pub fn selection(&self) -> Option<Range<usize>> {
        let anchor = self.anchor?;
        if anchor == self.cursor {
            None
        } else {
            Some(anchor.min(self.cursor)..anchor.max(self.cursor))
        }
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|r| &self.text[r])
    }

    pub fn select_all(&mut self) {
        if self.text.is_empty() {
            self.cursor = 0;
            self.anchor = None;
        } else {
            self.anchor = Some(0);
            self.cursor = self.text.len();
        }
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// Deletes the active selection and places the cursor at its start.
    pub fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection() else {
            self.anchor = None;
            return false;
        };
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = None;
        true
    }

    /// Inserts a string at the cursor (replacing any active selection).
    /// Newlines and control characters are stripped for single-line safety.
    pub fn insert_str(&mut self, s: &str) -> bool {
        let cleaned: String = s.chars().filter(|c| !c.is_control()).collect();
        if cleaned.is_empty() {
            return false;
        }
        self.delete_selection();
        self.text.insert_str(self.cursor, &cleaned);
        self.cursor += cleaned.len();
        self.anchor = None;
        true
    }

    pub fn backspace(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let prev = prev_char_boundary(&self.text, self.cursor);
        self.text.replace_range(prev..self.cursor, "");
        self.cursor = prev;
        true
    }

    pub fn delete_forward(&mut self) -> bool {
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let next = next_char_boundary(&self.text, self.cursor);
        self.text.replace_range(self.cursor..next, "");
        true
    }

    pub fn move_left(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
            self.cursor = prev_char_boundary(&self.text, self.cursor);
        } else if let Some(range) = self.selection() {
            self.cursor = range.start;
            self.anchor = None;
        } else {
            self.cursor = prev_char_boundary(&self.text, self.cursor);
            self.anchor = None;
        }
    }

    pub fn move_right(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
            self.cursor = next_char_boundary(&self.text, self.cursor);
        } else if let Some(range) = self.selection() {
            self.cursor = range.end;
            self.anchor = None;
        } else {
            self.cursor = next_char_boundary(&self.text, self.cursor);
            self.anchor = None;
        }
    }

    pub fn move_home(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = 0;
    }

    pub fn move_end(&mut self, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = self.text.len();
    }

    /// Cancels editing and restores the original baseline text.
    pub fn cancel(&mut self) -> String {
        self.text = self.original.clone();
        self.cursor = self.text.len();
        self.anchor = None;
        self.text.clone()
    }

    /// Processes a normalized keystroke against the buffer.
    pub fn handle_key(
        &mut self,
        key: &str,
        key_char: Option<&str>,
        shift: bool,
        cmd_or_ctrl: bool,
        clipboard_text: Option<&str>,
    ) -> TextInputAction {
        let lower = key.to_ascii_lowercase();
        if cmd_or_ctrl {
            match lower.as_str() {
                "a" => {
                    self.select_all();
                    return TextInputAction::None;
                }
                "c" => {
                    let copied = self
                        .selected_text()
                        .unwrap_or(self.text.as_str())
                        .to_string();
                    return TextInputAction::Copy(copied);
                }
                "x" => {
                    let cut = if let Some(sel) = self.selected_text() {
                        let s = sel.to_string();
                        self.delete_selection();
                        s
                    } else {
                        let s = std::mem::take(&mut self.text);
                        self.cursor = 0;
                        self.anchor = None;
                        s
                    };
                    return TextInputAction::Cut(cut);
                }
                "v" => {
                    if let Some(clip) = clipboard_text {
                        if self.insert_str(clip) {
                            return TextInputAction::Changed;
                        }
                    }
                    return TextInputAction::None;
                }
                _ => return TextInputAction::None,
            }
        }

        match lower.as_str() {
            "enter" | "return" => TextInputAction::Commit(self.text.clone()),
            "escape" => {
                self.cancel();
                TextInputAction::Cancel
            }
            "backspace" => {
                if self.backspace() {
                    TextInputAction::Changed
                } else {
                    TextInputAction::None
                }
            }
            "delete" => {
                if self.delete_forward() {
                    TextInputAction::Changed
                } else {
                    TextInputAction::None
                }
            }
            "left" => {
                self.move_left(shift);
                TextInputAction::None
            }
            "right" => {
                self.move_right(shift);
                TextInputAction::None
            }
            "home" => {
                self.move_home(shift);
                TextInputAction::None
            }
            "end" => {
                self.move_end(shift);
                TextInputAction::None
            }
            _ => {
                if let Some(ch) = key_char {
                    if self.insert_str(ch) {
                        return TextInputAction::Changed;
                    }
                }
                TextInputAction::None
            }
        }
    }
}

/// Stateful GPUI text input combining a `TextBuffer` with a GPUI `FocusHandle`.
#[derive(Clone)]
pub struct InlineTextState {
    pub buffer: TextBuffer,
    pub focus_handle: FocusHandle,
}

impl InlineTextState {
    pub fn new(focus_handle: FocusHandle, initial: impl Into<String>) -> Self {
        Self {
            buffer: TextBuffer::new(initial),
            focus_handle,
        }
    }

    pub fn text(&self) -> &str {
        &self.buffer.text
    }

    pub fn begin(&mut self, value: impl Into<String>, select_all: bool) {
        self.buffer.begin(value, select_all);
    }

    pub fn sync_external(&mut self, value: impl Into<String>) {
        self.buffer.sync_external(value);
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus_handle.is_focused(window)
    }

    /// Handles a GPUI `KeyDownEvent`, integrating clipboard Copy/Cut/Paste.
    pub fn handle_key_event(&mut self, event: &KeyDownEvent, cx: &mut App) -> TextInputOutcome {
        let ks = &event.keystroke;
        let cmd_or_ctrl = ks.modifiers.secondary() || ks.modifiers.control;
        let paste_text = if cmd_or_ctrl && ks.key.eq_ignore_ascii_case("v") {
            cx.read_from_clipboard().and_then(|item| item.text())
        } else {
            None
        };

        let action = self.buffer.handle_key(
            &ks.key,
            ks.key_char.as_deref(),
            ks.modifiers.shift,
            cmd_or_ctrl,
            paste_text.as_deref(),
        );

        match action {
            TextInputAction::None => {
                if matches!(
                    ks.key.to_ascii_lowercase().as_str(),
                    "left" | "right" | "home" | "end" | "backspace" | "delete"
                ) || (cmd_or_ctrl && matches!(ks.key.to_ascii_lowercase().as_str(), "a" | "v"))
                {
                    TextInputOutcome::Handled
                } else {
                    TextInputOutcome::Ignored
                }
            }
            TextInputAction::Changed => TextInputOutcome::Changed,
            TextInputAction::Commit(text) => TextInputOutcome::Commit(text),
            TextInputAction::Cancel => TextInputOutcome::Cancel,
            TextInputAction::Copy(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                TextInputOutcome::Handled
            }
            TextInputAction::Cut(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                TextInputOutcome::Changed
            }
        }
    }
}

/// Visual options for rendering an `InlineTextState`.
#[derive(Clone, Debug)]
pub struct TextInputStyle {
    pub width: f32,
    pub height: f32,
    pub font_size: f32,
    pub placeholder: SharedString,
    pub align_right: bool,
    pub error: bool,
    pub disabled: bool,
}

impl Default for TextInputStyle {
    fn default() -> Self {
        Self {
            width: 120.0,
            height: 24.0,
            font_size: 12.0,
            placeholder: SharedString::default(),
            align_right: false,
            error: false,
            disabled: false,
        }
    }
}

/// Renders a single-line GPUI text input with `.key_context("TextInput")`,
/// selection highlight, crisp caret, and Enter/Esc/clipboard handling.
pub fn render_text_input<V: 'static>(
    id: impl Into<SharedString>,
    state: &InlineTextState,
    style: TextInputStyle,
    theme: &GpuiTheme,
    window: &Window,
    cx: &mut Context<V>,
    on_key: impl Fn(&mut V, &KeyDownEvent, &mut Window, &mut Context<V>) + 'static,
    on_focus_click: impl Fn(&mut V, &mut Window, &mut Context<V>) + 'static,
) -> impl IntoElement {
    let focused = state.is_focused(window);
    let border_color = if style.error {
        theme.warn
    } else if focused {
        theme.accent
    } else {
        theme.track
    };

    let bg = if style.disabled {
        theme.panel
    } else {
        theme.canvas
    };

    let text_color = if style.disabled {
        theme.dim.opacity(0.55)
    } else {
        theme.paper
    };

    let text = &state.buffer.text;
    let cursor = floor_char_boundary(text, state.buffer.cursor.min(text.len()));
    let sel = state.buffer.selection();

    let content = if text.is_empty() && !focused {
        div()
            .flex()
            .items_center()
            .text_color(theme.dim)
            .child(style.placeholder.clone())
    } else if !focused {
        div()
            .flex()
            .items_center()
            .text_color(text_color)
            .child(text.clone())
    } else if let Some(range) = sel {
        let before = text[..range.start].to_string();
        let selected = text[range.clone()].to_string();
        let after = text[range.end..].to_string();
        div()
            .flex()
            .items_center()
            .when(!before.is_empty(), |el| {
                el.child(div().text_color(text_color).child(before))
            })
            .child(
                div()
                    .bg(theme.accent.opacity(0.32))
                    .rounded(px(2.0))
                    .text_color(theme.paper)
                    .child(selected),
            )
            .when(!after.is_empty(), |el| {
                el.child(div().text_color(text_color).child(after))
            })
    } else {
        let before = text[..cursor].to_string();
        let after = text[cursor..].to_string();
        div()
            .flex()
            .items_center()
            .when(!before.is_empty(), |el| {
                el.child(div().text_color(text_color).child(before))
            })
            .child(div().w(px(1.5)).h(px(style.font_size + 2.0)).bg(theme.accent))
            .when(!after.is_empty(), |el| {
                el.child(div().text_color(text_color).child(after))
            })
    };

    div()
        .id(id.into())
        .track_focus(&state.focus_handle)
        .key_context("TextInput")
        .flex()
        .items_center()
        .when(style.align_right, |el| el.justify_end())
        .w(px(style.width))
        .h(px(style.height))
        .px(px(6.0))
        .rounded(px(4.0))
        .bg(bg)
        .border_1()
        .border_color(border_color)
        .overflow_hidden()
        .font_family(theme.ui_family.clone())
        .text_size(px(style.font_size))
        .cursor_text()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _event, window, cx| {
                cx.stop_propagation();
                on_focus_click(this, window, cx);
            }),
        )
        .on_key_down(cx.listener(move |this, event: &KeyDownEvent, window, cx| {
            on_key(this, event, window, cx);
        }))
        .child(content)
}

/// Parses a typed decimal number, accepting both `.` and `,` decimal separators
/// and an optional trailing `%` (matching `windows/src/ui/inspector.rs::parse_number`).
pub fn parse_number(text: &str) -> Option<f64> {
    let cleaned = text.trim().trim_end_matches('%').trim().replace(',', ".");
    cleaned.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses a pixel dimension string, ignoring thousand separators (`,`, `.`, spaces, `'`).
pub fn parse_pixels(text: &str) -> u32 {
    let digits: String = text
        .chars()
        .filter(|c| !matches!(c, ',' | '.' | ' ' | '\u{a0}' | '\u{202f}' | '\''))
        .collect();
    digits.parse::<u32>().unwrap_or(0)
}

fn floor_char_boundary(s: &str, idx: usize) -> usize {
    let mut i = idx.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

fn prev_char_boundary(s: &str, idx: usize) -> usize {
    let idx = floor_char_boundary(s, idx);
    s[..idx].char_indices().next_back().map_or(0, |(i, _)| i)
}

fn next_char_boundary(s: &str, idx: usize) -> usize {
    let idx = floor_char_boundary(s, idx);
    if idx >= s.len() {
        return s.len();
    }
    let mut iter = s[idx..].char_indices();
    iter.next();
    iter.next().map_or(s.len(), |(off, _)| idx + off)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_buffer_insert_backspace_delete_and_cursor_movement() {
        let mut buf = TextBuffer::new("12.5");
        assert_eq!(buf.cursor, 4);

        buf.move_left(false);
        assert_eq!(buf.cursor, 3);
        assert!(buf.insert_str("0"));
        assert_eq!(buf.text, "12.05");
        assert_eq!(buf.cursor, 4);

        assert!(buf.backspace());
        assert_eq!(buf.text, "12.5");
        assert_eq!(buf.cursor, 3);

        buf.move_home(false);
        assert_eq!(buf.cursor, 0);
        assert!(buf.delete_forward());
        assert_eq!(buf.text, "2.5");

        buf.move_end(false);
        assert_eq!(buf.cursor, 3);
        assert!(buf.insert_str("0"));
        assert_eq!(buf.text, "2.50");
    }

    #[test]
    fn text_buffer_selection_copy_cut_paste_and_cancel() {
        let mut buf = TextBuffer::default();
        buf.begin("Portra 400", true);
        assert_eq!(buf.selected_text(), Some("Portra 400"));

        // Typing replaces selection.
        assert_eq!(
            buf.handle_key("g", Some("Gold"), false, false, None),
            TextInputAction::Changed
        );
        assert_eq!(buf.text, "Gold");
        assert_eq!(buf.selection(), None);

        // Shift-left extends selection.
        buf.handle_key("left", None, true, false, None);
        buf.handle_key("left", None, true, false, None);
        assert_eq!(buf.selected_text(), Some("ld"));

        // Copy does not mutate.
        assert_eq!(
            buf.handle_key("c", None, false, true, None),
            TextInputAction::Copy("ld".to_string())
        );
        assert_eq!(buf.text, "Gold");

        // Cut removes selection.
        assert_eq!(
            buf.handle_key("x", None, false, true, None),
            TextInputAction::Cut("ld".to_string())
        );
        assert_eq!(buf.text, "Go");

        // Paste inserts at cursor (stripping newlines).
        assert_eq!(
            buf.handle_key("v", None, false, true, Some("ld\n200")),
            TextInputAction::Changed
        );
        assert_eq!(buf.text, "Gold200");

        // Select all + Enter commits.
        buf.handle_key("a", None, false, true, None);
        assert_eq!(buf.selected_text(), Some("Gold200"));
        assert_eq!(
            buf.handle_key("enter", None, false, false, None),
            TextInputAction::Commit("Gold200".to_string())
        );

        // Escape restores original baseline.
        assert_eq!(
            buf.handle_key("escape", None, false, false, None),
            TextInputAction::Cancel
        );
        assert_eq!(buf.text, "Portra 400");
    }

    #[test]
    fn text_buffer_handles_utf8_characters_safely() {
        let mut buf = TextBuffer::new("50 µm");
        buf.move_left(false); // before 'm'
        buf.move_left(false); // before 'µ'
        assert!(buf.backspace()); // removes ' '
        assert_eq!(buf.text, "50µm");
        assert!(buf.delete_forward()); // removes 'µ'
        assert_eq!(buf.text, "50m");
    }

    #[test]
    fn numeric_parsing_accepts_dot_comma_and_percent_and_rejects_garbage() {
        assert_eq!(parse_number("1.25"), Some(1.25));
        assert_eq!(parse_number("1,25"), Some(1.25));
        assert_eq!(parse_number(" -0,50 "), Some(-0.5));
        assert_eq!(parse_number("3.5%"), Some(3.5));
        assert_eq!(parse_number("3,5 %"), Some(3.5));
        assert_eq!(parse_number("1x2"), None);
        assert_eq!(parse_number(""), None);
        assert_eq!(parse_number("NaN"), None);
        assert_eq!(parse_number("inf"), None);

        assert_eq!(parse_pixels("4,096"), 4096);
        assert_eq!(parse_pixels("4 096"), 4096);
        assert_eq!(parse_pixels("2048"), 2048);
        assert_eq!(parse_pixels("abc"), 0);
    }
}
