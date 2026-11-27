//! The batch export's popover (the Mac's `BatchProgressView`; the Windows
//! app's `ui/batch.rs`): over the status line, how far the batch has come
//! under a rule, what failed and why, and Cancel, Retry Failed and Show in
//! Finder/Explorer. A finished batch is forgotten once it closes. Also the
//! status line's words for an export or a batch.

use gpui::{AnyElement, Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px, relative};

use crate::NegsApp;

/// Failures listed before "and N more".
const LISTED: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Cancel,
    Retry,
    Show,
}

impl Button {
    pub fn title(self) -> &'static str {
        match self {
            Button::Cancel => "Cancel",
            Button::Retry => "Retry Failed",
            Button::Show if cfg!(target_os = "macos") => "Show in Finder",
            Button::Show if cfg!(target_os = "windows") => "Show in Explorer",
            Button::Show => "Show in Files",
        }
    }
}

/// What the popover shows, from the model's batch.
#[derive(Clone, Debug, PartialEq)]
pub struct View {
    pub title: String,
    pub rule: (f64, f64),
    pub line: String,
    pub failures: Vec<(String, String)>,
    pub more: usize,
    pub skipped: usize,
    pub running: bool,
}

impl View {
    pub fn of(run: &negs_app::batch::Run) -> View {
        let current = run.current.filter(|_| run.running).and_then(|i| run.journal.items.get(i));
        let folder = run.journal.destination.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let all: Vec<(String, String)> = run.failures().iter().map(|i| (i.title.clone(), i.reason.clone().unwrap_or_else(|| "Unknown error".into()))).collect();
        let more = all.len().saturating_sub(LISTED);
        View {
            title: run.title(),
            rule: run.rule_parts(),
            line: current.map_or_else(|| format!("To “{folder}”"), |i| i.title.clone()),
            failures: all.into_iter().take(LISTED).collect(),
            more,
            skipped: run.skipped(),
            running: run.running,
        }
    }

    pub fn buttons(&self) -> Vec<Button> {
        let mut buttons = vec![];
        if self.running {
            buttons.push(Button::Cancel);
        } else if !self.failures.is_empty() {
            buttons.push(Button::Retry);
        }
        buttons.push(Button::Show);
        buttons
    }
}

/// The status line's words for an export or a batch, the rule's share
/// while a batch runs, and whether a Show link and a Cancel belong there.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Words {
    pub text: String,
    pub strong: Option<String>,
    pub dim: Option<String>,
    pub progress: Option<f64>,
    pub show: bool,
    pub cancel_export: bool,
}

pub fn words(model: &negs_app::model::Model) -> Option<Words> {
    let batch = model.batch.as_ref();
    if let Some(run) = batch.filter(|b| b.running) {
        return Some(Words { text: run.progress_words(), progress: Some(run.fraction()), show: true, ..Default::default() });
    }
    if let Some(words) = &model.lightroom.words {
        return Some(Words { text: words.clone(), ..Default::default() });
    }
    if let Some(run) = batch.filter(|b| b.needs_attention()) {
        let (text, strong) = run.outcome_words();
        return Some(Words { text, strong, show: true, ..Default::default() });
    }
    if model.exporting {
        let name = model.source.as_deref().map(negs_app::model::file_name).unwrap_or_default();
        return Some(Words { text: format!("Exporting {name}"), dim: Some(format!("· {} s", model.export_started.elapsed().as_secs())), cancel_export: true, ..Default::default() });
    }
    None
}

/// The status line's part: words, the rule, Show and Cancel.
pub fn render_words(app: &NegsApp, words: Words, cx: &mut Context<NegsApp>) -> AnyElement {
    let theme = &app.theme;
    let (paper, dim, accent) = (theme.paper, theme.dim, theme.accent);
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .min_w_0()
        .child(div().text_color(theme.label).truncate().child(words.text))
        .when_some(words.strong, |el, s| el.child(div().text_color(theme.label).child("·")).child(div().font_weight(gpui::FontWeight(600.0)).text_color(paper).child(s)))
        .when_some(words.dim, |el, d| el.child(div().text_color(dim).child(d)))
        .when_some(words.progress, |el, f| {
            el.child(div().w(px(90.0)).h(px(3.0)).rounded(px(2.0)).bg(theme.track).child(div().h_full().rounded(px(2.0)).bg(accent).w(relative(f.clamp(0.0, 1.0) as f32))))
        })
        .when(words.show, |el| {
            let track = theme.track;
            let label = theme.label;
            el.child(
                div()
                    .id("batch-show")
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(track)
                    .text_color(dim)
                    .hover(move |s| s.text_color(label))
                    .child("Show")
                    .on_click(cx.listener(|this, _, _w, cx| {
                        toggle(this);
                        cx.notify();
                    })),
            )
        })
        .when(words.cancel_export, |el| {
            el.child(div().id("export-cancel-status").cursor_pointer().px(px(6.0)).rounded(px(3.0)).border_1().border_color(theme.track).text_color(theme.label).hover(move |s| s.text_color(paper)).child("Cancel").on_click(cx.listener(|this, _, _w, cx| {
                this.model.cancel_export();
                this.after_model_update(cx);
                cx.notify();
            })))
        })
        .into_any_element()
}

/// Offers to resume a batch the last session didn't finish, once launch
/// has settled (the Mac's `offerBatchResume`, `ui/batch.rs`).
pub fn offer_resume(app: &mut NegsApp, window: &mut gpui::Window, cx: &mut Context<NegsApp>) {
    let Some(journal) = app.model.unfinished_batch() else { return };
    let title = resume_title(&journal);
    let answer = window.prompt(
        gpui::PromptLevel::Info,
        &title,
        Some("Negs quit before the export finished. Each photograph exports with its edits as they were when you chose Export."),
        &["Resume", "Discard"],
        cx,
    );
    cx.spawn(async move |this, cx| {
        let resume = answer.await == Ok(0);
        let _ = this.update(cx, |app, cx| {
            app.model.resume_batch(journal, resume);
            app.after_model_update(cx);
            cx.notify();
        });
    })
    .detach();
}

pub fn resume_title(journal: &negs_app::batch::Journal) -> String {
    let count = journal.remaining();
    let photographs = if count == 1 { "1 photograph".to_string() } else { format!("{count} photographs") };
    let folder = journal.destination.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    match journal.options.lightroom.as_deref() {
        Some(kind) => format!("Resume sending {photographs} to {}?", negs_app::batch::lightroom_title(kind)),
        None => format!("Resume exporting {photographs} to “{folder}”?"),
    }
}

pub fn toggle(app: &mut NegsApp) {
    if app.model.batch_popover_shown {
        close(app);
    } else if app.model.batch.is_some() {
        app.model.batch_popover_shown = true;
    }
}

/// Closes it; a finished batch is then forgotten.
pub fn close(app: &mut NegsApp) {
    app.model.batch_popover_shown = false;
    app.model.clear_batch();
}

fn press(app: &mut NegsApp, button: Button, cx: &mut Context<NegsApp>) {
    match button {
        Button::Cancel => app.model.cancel_batch(),
        Button::Retry => app.model.retry_batch(),
        Button::Show => {
            let (folder, written) = app.model.batch_written();
            match written.first() {
                Some(file) => cx.reveal_path(file),
                None => cx.open_with_system(&folder),
            }
        }
    }
    app.after_model_update(cx);
}

/// The popover over the status line's left end.
pub fn render_popover(app: &NegsApp, cx: &mut Context<NegsApp>) -> Option<AnyElement> {
    if !app.model.batch_popover_shown {
        return None;
    }
    let view = View::of(app.model.batch.as_ref()?);
    let theme = &app.theme;
    let serif = theme.serif_family.clone();
    let (paper, label, dim, accent, track) = (theme.paper, theme.label, theme.dim, theme.accent, theme.track);
    let (done, failed) = view.rule;
    let rule = div()
        .flex()
        .w_full()
        .h(px(3.0))
        .my(px(8.0))
        .rounded(px(2.0))
        .bg(track)
        .child(div().h_full().bg(accent).w(relative(done as f32)))
        .child(div().h_full().bg(dim).w(relative(failed as f32)));
    let mut card = div()
        .id("batch-popover")
        .absolute()
        .left(px(12.0))
        .bottom(px(crate::status::STATUS_HEIGHT + 6.0))
        .w(px(300.0))
        .flex()
        .flex_col()
        .px(px(16.0))
        .py(px(14.0))
        .bg(theme.panel)
        .border_1()
        .border_color(theme.line)
        .rounded(px(8.0))
        .shadow_lg()
        .on_mouse_down_out(cx.listener(|this, _, _w, cx| {
            if this.model.batch_popover_shown {
                close(this);
                cx.notify();
            }
        }))
        .child(div().font_family(serif.clone()).font_weight(gpui::FontWeight(600.0)).text_size(px(17.0)).text_color(paper).child(view.title.clone()))
        .child(rule)
        .child(div().font_family(serif.clone()).italic().text_size(px(14.0)).text_color(label).truncate().child(view.line.clone()));
    if !view.failures.is_empty() {
        card = card.child(div().pt(px(10.0)).font_family(serif.clone()).font_weight(gpui::FontWeight(600.0)).text_size(px(15.0)).text_color(paper).child(format!("{} failed", view.failures.len() + view.more)));
        for (name, reason) in &view.failures {
            card = card
                .child(div().pt(px(4.0)).text_size(px(12.5)).text_color(label).truncate().child(name.clone()))
                .child(div().font_family(serif.clone()).italic().text_size(px(13.5)).text_color(dim).child(reason.clone()));
        }
        if view.more > 0 {
            card = card.child(div().pt(px(4.0)).font_family(serif.clone()).italic().text_size(px(13.5)).text_color(dim).child(format!("and {} more", view.more)));
        }
    }
    if view.skipped > 0 {
        card = card.child(div().pt(px(6.0)).font_family(serif.clone()).italic().text_size(px(13.5)).text_color(dim).child(format!("{} skipped: a file of that name was there", view.skipped)));
    }
    let mut buttons = div().flex().justify_end().gap(px(8.0)).pt(px(12.0));
    for (i, button) in view.buttons().into_iter().enumerate() {
        buttons = buttons.child(
            div()
                .id(("batch-button", i))
                .px(px(10.0))
                .py(px(3.0))
                .rounded(px(4.0))
                .border_1()
                .border_color(track)
                .cursor_pointer()
                .font_family(serif.clone())
                .text_size(px(13.5))
                .text_color(label)
                .hover(move |s| s.text_color(paper))
                .child(button.title())
                .on_click(cx.listener(move |this, _, _w, cx| {
                    press(this, button, cx);
                    cx.notify();
                })),
        );
    }
    Some(card.child(buttons).into_any_element())
}

#[cfg(test)]
mod tests {
    use super::*;
    use negs_app::batch::{Item, Journal, Options, Run, Status};
    use std::path::PathBuf;

    fn run(statuses: &[Status], running: bool) -> Run {
        let items = statuses
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Item {
                    photo: PathBuf::from(format!("/p/DSC_{i}.NEF")),
                    variant: None,
                    title: format!("DSC_{i}"),
                    state: Default::default(),
                    needs_match: false,
                    stores_match: false,
                    rating: None,
                    file_name: format!("DSC_{i}.jpg"),
                    size: None,
                    look: None,
                    status: *s,
                    reason: (*s == Status::Failed).then(|| "Out of memory".into()),
                    written: None,
                }
            })
            .collect();
        Run { journal: Journal::new(PathBuf::from("/out/Roll"), Options::default(), items), current: running.then_some(1), running, cancelled: false }
    }

    #[test]
    fn a_running_batch_offers_cancel_and_names_the_photograph() {
        let view = View::of(&run(&[Status::Done, Status::Waiting, Status::Waiting], true));
        assert_eq!(view.title, "Exporting 2 of 3");
        assert_eq!(view.line, "DSC_1");
        assert_eq!(view.buttons(), vec![Button::Cancel, Button::Show]);
    }

    #[test]
    fn the_resume_offer_counts_what_is_left() {
        let r = run(&[Status::Done, Status::Waiting, Status::Waiting], false);
        assert_eq!(resume_title(&r.journal), "Resume exporting 2 photographs to “Roll”?");
        let mut one = run(&[Status::Done, Status::Waiting], false);
        one.journal.options.lightroom = Some("classic".into());
        assert_eq!(resume_title(&one.journal), "Resume sending 1 photograph to Lightroom Classic?");
    }

    #[test]
    fn failures_are_listed_then_counted_and_offer_a_retry() {
        let mut statuses = vec![Status::Done];
        statuses.extend(std::iter::repeat_n(Status::Failed, 8));
        let view = View::of(&run(&statuses, false));
        assert_eq!(view.failures.len(), LISTED);
        assert_eq!(view.more, 2);
        assert_eq!(view.line, "To “Roll”");
        assert_eq!(view.buttons(), vec![Button::Retry, Button::Show]);
        assert_eq!(view.failures[0].1, "Out of memory");
    }
}
