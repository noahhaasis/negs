//! Save Preview, Export and Batch Export: an options card in the window,
//! then the platform's own save or folder dialog (Noah, 2026-11-25: GPUI's
//! dialogs can't hold controls, so the options come first, Lightroom-style;
//! `docs/windows-ui-features.md` §11). The choices and their words are pure
//! (`Choices`); the card is drawn in the darkroom's ink.

use std::path::{Path, PathBuf};

use gpui::{AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled, Window, div, prelude::FluentBuilder, px};
use negs_app::batch::{self, Existing, Naming};
use negs_app::engine::ExportEngine;
use negs_app::inspector_view::output::{COLOR_SPACES, OutputFormat, PROPHOTO, QUALITY_RANGE};
use negs_app::model::{Edge, ExportSize, Model, PIXEL_RANGE, SaveSettings};
use negs_app::session::Preferences;

use crate::NegsApp;
use crate::widgets::text_input::{InlineTextState, TextInputOutcome, TextInputStyle, render_text_input};

/// What the card is for.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// The preview as shown, no re-render.
    Preview,
    /// The open photograph at full resolution.
    Export,
    /// The grid's selection, each with its own edits, into a folder. `first`
    /// is the first photograph's `{name}` and `{film}`, for the example.
    Batch { targets: Vec<negs_library::Item>, first: (String, String) },
}

impl Job {
    pub fn title(&self) -> String {
        match self {
            Job::Preview => "Save Preview".into(),
            Job::Export => "Export Photograph".into(),
            Job::Batch { targets, .. } if targets.len() == 1 => "Export Photograph".into(),
            Job::Batch { targets, .. } => format!("Export {} Photographs", targets.len()),
        }
    }
    /// The card's button: it opens the save dialog, or the folder chooser.
    pub fn button(&self) -> &'static str {
        match self {
            Job::Preview => "Save…",
            Job::Export => "Export…",
            Job::Batch { .. } => "Choose Folder…",
        }
    }
    fn exports(&self) -> bool {
        !matches!(self, Job::Preview)
    }
}

/// The card's choices, from and back to the preferences (the same keys the
/// Mac and Windows apps keep).
#[derive(Clone, Debug, PartialEq)]
pub struct Choices {
    pub format: OutputFormat,
    pub engine: ExportEngine,
    pub edge: Edge,
    pub pixels: u32,
    /// The colour space picked; ProPhoto RGB at 8 bits saves as Adobe RGB.
    pub space: usize,
    pub png_depth: u32,
    pub tiff_depth: u32,
    pub quality: u32,
    pub naming: Naming,
    pub custom: String,
    pub existing: Existing,
}

impl Choices {
    pub fn from_prefs(p: &Preferences) -> Choices {
        let depth = |d: u32| if d == 16 { 16 } else { 8 };
        Choices {
            format: OutputFormat::from_key(&p.output_format),
            engine: ExportEngine::from_key(&p.export_engine),
            edge: Edge::from_key(&p.export_edge),
            pixels: p.export_edge_pixels.clamp(PIXEL_RANGE.0, PIXEL_RANGE.1),
            space: COLOR_SPACES.iter().position(|s| *s == p.color_space).unwrap_or(0),
            png_depth: depth(p.png_bit_depth),
            tiff_depth: depth(p.tiff_bit_depth),
            quality: p.jpeg_quality.clamp(QUALITY_RANGE.0, QUALITY_RANGE.1),
            naming: Naming::from_key(&p.batch_naming),
            custom: if p.batch_custom_name.is_empty() { "{name}_{film}".into() } else { p.batch_custom_name.clone() },
            existing: Existing::from_key(&p.batch_existing),
        }
    }

    /// Remembers the choices; the engine only when the job exported, the
    /// naming only for a batch, as the Windows dialogs do.
    pub fn store(&self, p: &mut Preferences, job: &Job, folder: Option<PathBuf>) {
        p.output_format = self.format.key().into();
        p.color_space = COLOR_SPACES[self.space.min(COLOR_SPACES.len() - 1)].into();
        p.png_bit_depth = self.png_depth;
        p.tiff_bit_depth = self.tiff_depth;
        p.jpeg_quality = self.quality;
        p.export_edge = self.edge.key().into();
        p.export_edge_pixels = self.pixels;
        if folder.is_some() {
            p.last_save_folder = folder;
        }
        if job.exports() {
            p.export_engine = self.engine.key().into();
        }
        if matches!(job, Job::Batch { .. }) {
            p.batch_naming = self.naming.key().into();
            p.batch_custom_name = self.custom.clone();
            p.batch_existing = self.existing.key().into();
        }
    }

    /// Bits per channel: JPEG's are 8, PNG and TIFF remember their own.
    pub fn depth(&self) -> u32 {
        match self.format {
            OutputFormat::Jpeg => 8,
            OutputFormat::Png => self.png_depth,
            OutputFormat::Tiff => self.tiff_depth,
        }
    }

    pub fn set_depth(&mut self, depth: u32) {
        let depth = if depth == 16 { 16 } else { 8 };
        match self.format {
            OutputFormat::Png => self.png_depth = depth,
            OutputFormat::Tiff => self.tiff_depth = depth,
            OutputFormat::Jpeg => {}
        }
    }

    /// Whether `space` can be chosen now: ProPhoto RGB needs 16 bits.
    pub fn space_enabled(&self, space: usize) -> bool {
        space != PROPHOTO || self.depth() == 16
    }

    pub fn settings(&self) -> SaveSettings {
        let bit_depth = self.depth();
        SaveSettings { bit_depth, color_space: SaveSettings::effective_space(self.space, bit_depth), jpeg_quality: self.quality }
    }

    pub fn size(&self) -> ExportSize {
        ExportSize { edge: self.edge, pixels: self.pixels.clamp(PIXEL_RANGE.0, PIXEL_RANGE.1) }
    }

    /// The words under the choices: what the engine does, what the save
    /// adds (`extras`, from `Model::save_extras`), then the size and colour.
    pub fn message(&self, job: &Job, extras: &str, preview_space: &str) -> String {
        if let Job::Batch { targets, .. } = job {
            return batch::message(targets.len(), self.engine, self.size(), self.settings(), preview_space);
        }
        let lead = Model::save_lead(job.exports().then_some(self.engine));
        let size = self.size().summary().map(|s| format!(" {s}.")).unwrap_or_default();
        let space = self.settings().summary(job.exports(), preview_space).map(|s| format!(" {s}.")).unwrap_or_default();
        format!("{lead}{extras}{size}{space}")
    }

    /// "For example: DSC_0412_kodak_portra_400.jpg".
    pub fn example(&self, first: &(String, String)) -> String {
        format!("For example: {}", self.batch_options(false).file_name(&first.0, &first.1))
    }

    pub fn batch_options(&self, low_memory: bool) -> batch::Options {
        let save = self.settings();
        batch::Options {
            format: self.format.key().into(),
            engine: self.engine.key().into(),
            edge: self.edge.key().into(),
            pixels: self.size().pixels,
            bit_depth: save.bit_depth,
            color_space: save.color_space,
            jpeg_quality: save.jpeg_quality,
            naming: self.naming,
            custom: self.custom.clone(),
            existing: self.existing,
            low_memory,
            lightroom: None,
        }
    }
}

/// The file the save dialog gave back and its format: a known extension
/// typed there wins (the engine encodes by extension); otherwise the
/// format's own is added.
pub fn destination(chosen: PathBuf, format: OutputFormat) -> (PathBuf, OutputFormat) {
    let typed = chosen.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase());
    let known = match typed.as_deref() {
        Some("png") => Some(OutputFormat::Png),
        Some("jpg" | "jpeg") => Some(OutputFormat::Jpeg),
        Some("tif" | "tiff") => Some(OutputFormat::Tiff),
        _ => None,
    };
    match known {
        Some(format) => (chosen, format),
        None => {
            let mut name = chosen.into_os_string();
            name.push(".");
            name.push(format.extension());
            (PathBuf::from(name), format)
        }
    }
}

/// The open card.
pub struct ExportCard {
    pub job: Job,
    pub choices: Choices,
    extras: String,
    preview_space: String,
    pixels: InlineTextState,
    quality: InlineTextState,
    custom: InlineTextState,
}

/// Opens the card for Save Preview (`export` false) or File › Export: the
/// grid's selection exports as a batch.
pub fn open(app: &mut NegsApp, export: bool, cx: &mut Context<NegsApp>) {
    if app.export_card.is_some() {
        return;
    }
    let model = &app.model;
    let job = if !export {
        if model.source.is_none() || model.cropping() || !model.can_save() {
            return;
        }
        Job::Preview
    } else if model.exports_single() {
        if model.source.is_none() || model.cropping() || !model.can_export() {
            return;
        }
        Job::Export
    } else {
        let targets = model.export_targets();
        if !model.can_start_batch() || targets.is_empty() {
            return;
        }
        let first = model.batch_items(&targets[..1], &batch::Options::default()).into_iter().next();
        let first = first.map(|i| (batch::batch_name(&i.photo, &i.title), i.state.film)).unwrap_or_default();
        Job::Batch { targets, first }
    };
    let choices = Choices::from_prefs(&model.prefs);
    let extras = if matches!(job, Job::Batch { .. }) { String::new() } else { model.save_extras() };
    app.export_card = Some(ExportCard {
        pixels: InlineTextState::new(cx.focus_handle(), choices.pixels.to_string()),
        quality: InlineTextState::new(cx.focus_handle(), choices.quality.to_string()),
        custom: InlineTextState::new(cx.focus_handle(), choices.custom.clone()),
        preview_space: model.preview_space().to_string(),
        extras,
        job,
        choices,
    });
    cx.notify();
}

/// Esc closes, Return goes on (fields take their own keys first).
pub fn key(app: &mut NegsApp, key: &str, window: &mut Window, cx: &mut Context<NegsApp>) -> bool {
    if app.export_card.is_none() {
        return false;
    }
    match key {
        "escape" => close(app, cx),
        "enter" => confirm(app, window, cx),
        _ => {}
    }
    true
}

pub fn close(app: &mut NegsApp, cx: &mut Context<NegsApp>) {
    app.export_card = None;
    cx.notify();
}

/// Reads what's typed in the fields into the choices.
fn take_fields(card: &mut ExportCard) {
    let digits = |t: &str| t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<u32>().ok();
    if let Some(p) = digits(card.pixels.text()) {
        card.choices.pixels = p.clamp(PIXEL_RANGE.0, PIXEL_RANGE.1);
    }
    if let Some(q) = digits(card.quality.text()) {
        card.choices.quality = q.clamp(QUALITY_RANGE.0, QUALITY_RANGE.1);
    }
    let custom = card.custom.text().trim();
    if !custom.is_empty() {
        card.choices.custom = custom.to_string();
    }
}

/// The card's button: the save dialog with the name and extension chosen,
/// or the folder chooser for a batch; then the model saves or exports.
pub fn confirm(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) {
    let Some(mut card) = app.export_card.take() else { return };
    take_fields(&mut card);
    window.focus(&app.focus_handle.clone());
    let folder = app.model.prefs.save_folder().map(Path::to_path_buf).or_else(|| app.model.source.as_deref().and_then(Path::parent).map(Path::to_path_buf)).unwrap_or_else(negs_app::platform::pictures_dir);
    let ExportCard { job, choices, .. } = card;
    cx.notify();
    if let Job::Batch { targets, .. } = job {
        let receiver = cx.prompt_for_paths(gpui::PathPromptOptions { files: false, directories: true, multiple: false, prompt: Some(SharedString::from("Export")) });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = receiver.await else { return };
            let Some(folder) = paths.into_iter().next() else { return };
            let _ = this.update(cx, |app, cx| {
                let job = Job::Batch { targets: targets.clone(), first: Default::default() };
                choices.store(&mut app.model.prefs, &job, Some(folder.clone()));
                app.model.prefs.save();
                let options = choices.batch_options(app.model.prefs.low_memory_export);
                app.model.export_batch(targets, folder, options, None);
                app.after_model_update(cx);
                cx.notify();
            });
        })
        .detach();
        return;
    }
    let name = app.model.default_name(choices.format);
    let receiver = cx.prompt_for_new_path(&folder, Some(&name));
    cx.spawn(async move |this, cx| {
        let Ok(Ok(Some(chosen))) = receiver.await else { return };
        let _ = this.update(cx, |app, cx| {
            let (path, format) = destination(chosen, choices.format);
            let mut choices = choices.clone();
            choices.format = format;
            choices.store(&mut app.model.prefs, &job, path.parent().map(Path::to_path_buf));
            app.model.prefs.save();
            if job == Job::Export {
                app.model.export(path, choices.engine, choices.size(), choices.settings());
            } else {
                app.model.save_preview(path, choices.size(), choices.settings());
            }
            app.after_model_update(cx);
            cx.notify();
        });
    })
    .detach();
}

/// File › Export LUT…: a 33³ `.cube` of the film and paper as set.
pub fn export_lut(app: &mut NegsApp, cx: &mut Context<NegsApp>) {
    if !app.model.can_export_lut() {
        return;
    }
    let folder = app.model.prefs.save_folder().map(Path::to_path_buf).unwrap_or_else(negs_app::platform::pictures_dir);
    let name = format!("{}_{}.cube", app.model.state.film, app.model.state.paper);
    let receiver = cx.prompt_for_new_path(&folder, Some(&name));
    cx.spawn(async move |this, cx| {
        let Ok(Ok(Some(chosen))) = receiver.await else { return };
        let _ = this.update(cx, |app, cx| {
            let path = if chosen.extension().is_some_and(|e| e.eq_ignore_ascii_case("cube")) { chosen } else { chosen.with_extension("cube") };
            app.model.prefs.last_save_folder = path.parent().map(Path::to_path_buf);
            app.model.prefs.save();
            app.model.export_lut(path);
            app.after_model_update(cx);
            cx.notify();
        });
    })
    .detach();
}

fn field_key(app: &mut NegsApp, event: &KeyDownEvent, which: u8, window: &mut Window, cx: &mut Context<NegsApp>) {
    let Some(card) = app.export_card.as_mut() else { return };
    let field = match which {
        0 => &mut card.pixels,
        1 => &mut card.quality,
        _ => &mut card.custom,
    };
    let outcome = field.handle_key_event(event, cx);
    match outcome {
        TextInputOutcome::Commit(_) => {
            take_fields(card);
            card.pixels.sync_external(card.choices.pixels.to_string());
            card.quality.sync_external(card.choices.quality.to_string());
            window.focus(&app.focus_handle.clone());
        }
        TextInputOutcome::Changed => take_fields(card),
        TextInputOutcome::Cancel => {
            card.pixels.sync_external(card.choices.pixels.to_string());
            card.quality.sync_external(card.choices.quality.to_string());
            card.custom.sync_external(card.choices.custom.clone());
            window.focus(&app.focus_handle.clone());
        }
        _ => {}
    }
    cx.stop_propagation();
    cx.notify();
}

/// The card over a dimmed window.
pub fn render(app: &mut NegsApp, window: &mut Window, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    let theme = app.theme.clone();
    let card = app.export_card.as_ref()?;
    let choices = card.choices.clone();
    let job = card.job.clone();
    let message = choices.message(&job, &card.extras, &card.preview_space);
    let serif = theme.serif_family.clone();

    // A row: its label, then words to choose from (the chosen one in paper,
    // underlined), each `(title, chosen, enabled, on_click)`.
    type Pick = Box<dyn Fn(&mut Choices)>;
    let row = |id: &'static str, label: &'static str, words: Vec<(String, bool, bool, Pick)>, extra: Option<AnyElement>, cx: &mut Context<NegsApp>| {
        let mut line = div().flex().flex_wrap().items_center().gap_x(px(14.0)).gap_y(px(4.0)).flex_1().min_w_0();
        for (i, (title, chosen, enabled, pick)) in words.into_iter().enumerate() {
            let pick = std::rc::Rc::new(pick);
            line = line.child(
                div()
                    .id((id, i))
                    .flex()
                    .flex_col()
                    .when(enabled, |d| d.cursor_pointer())
                    .when(!enabled, |d| d.opacity(negs_app::theme::DISABLED_OPACITY))
                    .when(enabled, |d| {
                        let pick = pick.clone();
                        d.on_click(cx.listener(move |this, _, _w, cx| {
                            if let Some(card) = this.export_card.as_mut() {
                                take_fields(card);
                                pick(&mut card.choices);
                            }
                            cx.notify();
                        }))
                    })
                    .child(div().font_family(serif.clone()).text_size(px(14.5)).text_color(if chosen { theme.paper } else { theme.dim }).child(title))
                    .child(div().h(px(2.0)).w_full().when(chosen, |d| d.bg(theme.accent))),
            );
        }
        if let Some(extra) = extra {
            line = line.child(extra);
        }
        div()
            .flex()
            .items_start()
            .gap(px(12.0))
            .py(px(5.0))
            .child(div().w(px(96.0)).flex_shrink_0().pt(px(2.0)).text_size(px(12.0)).text_color(theme.label).child(label))
            .child(line)
            .into_any_element()
    };

    let mut body = div().flex().flex_col().w_full();
    let formats = OutputFormat::ALL
        .into_iter()
        .map(|f| {
            let pick: Pick = Box::new(move |c: &mut Choices| c.format = f);
            (f.title().to_string(), f == choices.format, true, pick)
        })
        .collect();
    body = body.child(row("export-format", "Format", formats, None, cx));
    if job.exports() {
        let engines = [ExportEngine::Fast, ExportEngine::Reference]
            .into_iter()
            .map(|e| {
                let pick: Pick = Box::new(move |c: &mut Choices| c.engine = e);
                (e.title().to_string(), e == choices.engine, true, pick)
            })
            .collect();
        body = body.child(row("export-engine", "Engine", engines, None, cx));
    }
    let style = TextInputStyle { width: 64.0, height: 22.0, font_size: 12.5, align_right: true, ..Default::default() };
    let card = app.export_card.as_ref()?;
    let pixels = (choices.edge != Edge::Full).then(|| {
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(render_text_input("export-pixels", &card.pixels, style.clone(), &theme, window, cx, |this, e: &KeyDownEvent, w, cx| field_key(this, e, 0, w, cx), |this, w, _cx| {
                if let Some(c) = &this.export_card {
                    c.pixels.focus_handle.focus(w)
                }
            }))
            .child(div().text_size(px(12.0)).text_color(theme.dim).child("px"))
            .into_any_element()
    });
    let edges = Edge::ALL
        .into_iter()
        .map(|e| {
            let pick: Pick = Box::new(move |c: &mut Choices| c.edge = e);
            (e.title().to_string(), e == choices.edge, true, pick)
        })
        .collect();
    body = body.child(row("export-size", "Size", edges, pixels, cx));
    let jpeg = choices.format == OutputFormat::Jpeg;
    let depths = [8, 16]
        .into_iter()
        .map(|d| {
            let pick: Pick = Box::new(move |c: &mut Choices| {
                c.set_depth(d);
                if !c.space_enabled(c.space) {
                    c.space = negs_app::inspector_view::output::PROPHOTO_FALLBACK;
                }
            });
            (format!("{d}-bit"), choices.depth() == d, !jpeg, pick)
        })
        .collect();
    body = body.child(row("export-depth", "Bit depth", depths, None, cx));
    let effective = choices.settings().color_space;
    let spaces = (0..COLOR_SPACES.len())
        .map(|i| {
            let pick: Pick = Box::new(move |c: &mut Choices| c.space = i);
            let title = if i == PROPHOTO { "ProPhoto RGB (16-bit)".to_string() } else { COLOR_SPACES[i].to_string() };
            (title, i == effective, choices.space_enabled(i), pick)
        })
        .collect();
    body = body.child(row("export-space", "Colour space", spaces, None, cx));
    if jpeg {
        let card = app.export_card.as_ref()?;
        let field = div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(render_text_input("export-quality", &card.quality, style.clone(), &theme, window, cx, |this, e: &KeyDownEvent, w, cx| field_key(this, e, 1, w, cx), |this, w, _cx| {
                if let Some(c) = &this.export_card {
                    c.quality.focus_handle.focus(w)
                }
            }))
            .child(div().text_size(px(12.0)).text_color(theme.dim).child(format!("({}–{})", QUALITY_RANGE.0, QUALITY_RANGE.1)))
            .into_any_element();
        body = body.child(row("export-quality-row", "JPEG quality", vec![], Some(field), cx));
    }
    if let Job::Batch { first, .. } = &job {
        let card = app.export_card.as_ref()?;
        let custom = (choices.naming == Naming::Custom).then(|| {
            render_text_input("export-custom", &card.custom, TextInputStyle { width: 150.0, height: 22.0, font_size: 12.5, ..Default::default() }, &theme, window, cx, |this, e: &KeyDownEvent, w, cx| field_key(this, e, 2, w, cx), |this, w, _cx| {
                if let Some(c) = &this.export_card {
                    c.custom.focus_handle.focus(w)
                }
            })
            .into_any_element()
        });
        let namings = Naming::ALL
            .into_iter()
            .map(|n| {
                let pick: Pick = Box::new(move |c: &mut Choices| c.naming = n);
                (n.title().to_string(), n == choices.naming, true, pick)
            })
            .collect();
        body = body.child(row("export-naming", "File name", namings, custom, cx));
        body = body.child(div().pl(px(108.0)).pb(px(4.0)).font_family(theme.serif_family.clone()).italic().text_size(px(13.0)).text_color(theme.dim).child(choices.example(first)));
        let existing = Existing::ALL
            .into_iter()
            .map(|e| {
                let pick: Pick = Box::new(move |c: &mut Choices| c.existing = e);
                (e.title().to_string(), e == choices.existing, true, pick)
            })
            .collect();
        body = body.child(row("export-existing", "If a file exists", existing, None, cx));
    }

    let button = |id: &'static str, title: &'static str, primary: bool| {
        div()
            .id(id)
            .px(px(16.0))
            .py(px(6.0))
            .rounded(px(5.0))
            .cursor_pointer()
            .text_size(px(13.0))
            .when(primary, |d| d.bg(theme.fill).text_color(theme.on_fill))
            .when(!primary, |d| d.border_1().border_color(theme.track).text_color(theme.label).hover(|s| s.bg(theme.subtle_hover)))
            .child(title)
    };
    let card_el = div()
        .id("export-card")
        .w(px(520.0))
        .max_w_full()
        .flex()
        .flex_col()
        .bg(theme.panel)
        .border_1()
        .border_color(theme.line)
        .rounded(px(10.0))
        .shadow_lg()
        .px(px(24.0))
        .pt(px(18.0))
        .pb(px(18.0))
        .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .child(div().font_family(theme.serif_family.clone()).text_size(px(21.0)).text_color(theme.paper).pb(px(10.0)).child(job.title()))
        .child(body)
        .child(div().w_full().min_w_0().pt(px(12.0)).font_family(theme.serif_family.clone()).italic().text_size(px(13.5)).text_color(theme.label).child(message))
        .child(
            div()
                .flex()
                .justify_end()
                .gap(px(10.0))
                .pt(px(18.0))
                .child(button("export-cancel", "Cancel", false).on_click(cx.listener(|this, _, _w, cx| close(this, cx))))
                .child(button("export-go", job.button(), true).on_click(cx.listener(|this, _, w, cx| confirm(this, w, cx)))),
        );
    Some(
        div()
            .id("export-scrim")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::hsla(0.0, 0.0, 0.0, 0.35))
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|this, _, _w, cx| close(this, cx)))
            .child(card_el)
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs() -> Preferences {
        Preferences::default()
    }

    #[test]
    fn choices_round_trip_through_the_preferences() {
        let mut p = prefs();
        let mut c = Choices::from_prefs(&p);
        c.format = OutputFormat::Tiff;
        c.set_depth(16);
        c.space = PROPHOTO;
        c.edge = Edge::Long;
        c.pixels = 3000;
        c.engine = ExportEngine::Reference;
        c.quality = 80;
        c.store(&mut p, &Job::Export, Some(PathBuf::from("/tmp")));
        assert_eq!(p.output_format, "tiff");
        assert_eq!(p.tiff_bit_depth, 16);
        assert_eq!(p.color_space, "ProPhoto RGB");
        assert_eq!((p.export_edge.as_str(), p.export_edge_pixels), ("long", 3000));
        assert_eq!(p.export_engine, "reference");
        assert_eq!(p.last_save_folder, Some(PathBuf::from("/tmp")));
        assert_eq!(Choices::from_prefs(&p), c);
    }

    #[test]
    fn save_preview_keeps_the_engine_and_the_batch_its_naming() {
        let mut p = prefs();
        let before = p.export_engine.clone();
        let mut c = Choices::from_prefs(&p);
        c.engine = if before == "fast" { ExportEngine::Reference } else { ExportEngine::Fast };
        c.naming = Naming::Custom;
        c.store(&mut p, &Job::Preview, None);
        assert_eq!(p.export_engine, before);
        assert_ne!(p.batch_naming, "custom");
        c.store(&mut p, &Job::Batch { targets: vec![], first: Default::default() }, None);
        assert_eq!(p.batch_naming, "custom");
    }

    #[test]
    fn jpeg_is_eight_bits_and_prophoto_needs_sixteen() {
        let mut c = Choices::from_prefs(&prefs());
        c.format = OutputFormat::Png;
        c.set_depth(8);
        c.space = PROPHOTO;
        assert!(!c.space_enabled(PROPHOTO));
        assert_eq!(c.settings().color_space, negs_app::inspector_view::output::PROPHOTO_FALLBACK);
        c.set_depth(16);
        assert_eq!(c.settings(), SaveSettings { bit_depth: 16, color_space: PROPHOTO, jpeg_quality: c.quality });
        c.format = OutputFormat::Jpeg;
        assert_eq!(c.depth(), 8);
        c.format = OutputFormat::Png;
        assert_eq!(c.depth(), 16, "PNG remembers its own depth");
    }

    #[test]
    fn the_typed_extension_picks_the_format() {
        assert_eq!(destination(PathBuf::from("/a/b.JPEG"), OutputFormat::Png), (PathBuf::from("/a/b.JPEG"), OutputFormat::Jpeg));
        assert_eq!(destination(PathBuf::from("/a/b.tif"), OutputFormat::Png).1, OutputFormat::Tiff);
        assert_eq!(destination(PathBuf::from("/a/b"), OutputFormat::Tiff), (PathBuf::from("/a/b.tif"), OutputFormat::Tiff));
        assert_eq!(destination(PathBuf::from("/a/b.v2"), OutputFormat::Png), (PathBuf::from("/a/b.v2.png"), OutputFormat::Png));
    }

    #[test]
    fn messages_follow_the_choices() {
        let mut c = Choices::from_prefs(&prefs());
        c.space = 0;
        c.edge = Edge::Full;
        c.engine = ExportEngine::Fast;
        assert_eq!(c.message(&Job::Export, " Crops the photograph.", "sRGB"), "Looks the same as Reference, in a fraction of the time. Crops the photograph.");
        assert_eq!(c.message(&Job::Preview, "", "sRGB"), "Saves the preview as shown.");
        c.edge = Edge::Long;
        c.pixels = 2048;
        assert!(c.message(&Job::Export, "", "sRGB").ends_with(" Fits 2048 px on the long edge."));
        let batch = Job::Batch { targets: vec![], first: ("DSC_1".into(), "kodak_gold_200".into()) };
        assert!(c.message(&batch, "", "sRGB").starts_with("Each of the 0 photographs"));
        assert_eq!(batch.title(), "Export 0 Photographs");
    }

    #[test]
    fn the_example_shows_the_naming() {
        let mut c = Choices::from_prefs(&prefs());
        c.format = OutputFormat::Jpeg;
        let first = ("DSC_1".to_string(), "kodak_gold_200".to_string());
        c.naming = Naming::NameFilm;
        assert_eq!(c.example(&first), "For example: DSC_1_kodak_gold_200.jpg");
        c.naming = Naming::Name;
        assert_eq!(c.example(&first), "For example: DSC_1.jpg");
        c.naming = Naming::Custom;
        c.custom = "{film}-{name}".into();
        assert_eq!(c.example(&first), "For example: kodak_gold_200-DSC_1.jpg");
    }
}
