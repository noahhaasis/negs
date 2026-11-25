//! Settings (⌘, / Ctrl+,): a window of its own, as on the Mac and Windows,
//! with the same four pages (`windows/src/ui/settings.rs`, `Settings.swift`):
//! Photographs, Speed, Upkeep and Look. It reads and changes the main
//! window's `NegsApp` through its entity, and redraws when that changes.

use gpui::{AnyElement, App, AppContext, Bounds, Context, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled, Window, WindowBounds, WindowHandle, WindowOptions, div, prelude::FluentBuilder, px, size};
use negs_app::lightroom::Kind;
use negs_app::state::FotufilmFilms;
use negs_app::theme::ThemeName;

use crate::NegsApp;
use crate::sheet::ViewerSheet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Photographs,
    Speed,
    Upkeep,
    Look,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Photographs, Tab::Speed, Tab::Upkeep, Tab::Look];
    pub fn title(self) -> &'static str {
        match self {
            Tab::Photographs => "Photographs",
            Tab::Speed => "Speed",
            Tab::Upkeep => "Upkeep",
            Tab::Look => "Look",
        }
    }
    /// As the Mac stores it (`settingsTab`); anything else is Photographs.
    pub fn parse(text: &str) -> Tab {
        Tab::ALL.into_iter().find(|t| t.title() == text).unwrap_or(Tab::Photographs)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    MatchNew,
    NegsBuilt,
    SeparateJpegs,
    Sidecars,
    Previews,
    LimitPreview,
    LowMemory,
}

/// A list that opens under its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum List {
    NewFilm,
    NewPaper,
    Sheet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    Section(&'static str),
    Tick(&'static str, Tick),
    List(&'static str, List),
    Fotufilm,
    Hint(Hint),
    Updates,
    Lightroom(Kind),
    Themes,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hint {
    Text(&'static str),
    NewPhotos,
    Pairing,
    Sidecars,
    Theme,
    Lightroom(Kind),
}

const NEW_PHOTOS: &str = "A photo you haven’t edited opens with this film and paper, and its thumbnail shows them. Photos you’ve edited keep their own.";
const NEW_PHOTOS_SLIDE: &str = "Not printed: scanning the film. A photo you haven’t edited opens with this film, and its thumbnail shows it. Photos you’ve edited keep their own.";
const SEPARATE_JPEGS: &str = "A RAW and the camera’s JPEG of the same name, such as IMG_0412.CR3 and IMG_0412.JPG, are two photos.";
const PAIRED_JPEGS: &str = "A RAW and the camera’s JPEG of the same name, such as IMG_0412.CR3 and IMG_0412.JPG, are one photo: the RAW develops, the JPEG goes to the Trash with it, and Original can show it. A JPEG you’ve already rated or edited stays a photo of its own.";
const SIDECARS_ON: &str = "Each photo remembers its own edits in a small file beside it, such as IMG_0412.CR3.negs, which travels with the folder. A photo you haven’t edited starts with the film and paper above, default Prep and Lab settings and no border; the Image Size and output colour stay.";
const SIDECARS_OFF: &str = "Ratings and edits won’t be saved. Reopening a photo starts from the defaults.";
const LIGHTROOM_CLOUD: &str = "Open in Lightroom sends full-size 16-bit TIFFs. Each is deleted once Lightroom has copied it into its library, or after a week.";
const LIGHTROOM_CLASSIC: &str = "Lightroom Classic adds the TIFFs in “Negs for Lightroom” in Pictures where they are, so Negs never deletes them. Delete them only once Classic no longer needs them.";

/// Each page's rows, as the Windows app's (`settings::items`). Updates
/// aren't configured in this app yet (the updater is Phase 6's choice).
pub fn rows(tab: Tab) -> Vec<Row> {
    match tab {
        Tab::Photographs => vec![
            Row::Section("New photographs"),
            Row::List("Film", List::NewFilm),
            Row::List("Paper", List::NewPaper),
            Row::Hint(Hint::NewPhotos),
            Row::Tick("Match to camera brightness", Tick::MatchNew),
            Row::Hint(Hint::Text("New photographs start with Prep › Match camera brightness on: a RAW develops about as bright as the camera's own preview, a JPEG or TIFF as the file itself.")),
            Row::Section("Films"),
            Row::Fotufilm,
            Row::Hint(Hint::Text("Films ported from Fotufilm. From datasheets hides the eight it models from sample photographs rather than the manufacturer’s data. Photos already using a hidden film keep it.")),
            Row::Tick("Show Negs-built films", Tick::NegsBuilt),
            Row::Hint(Hint::Text(negs_app::state::NEGS_BUILT_HINT)),
            Row::Section("RAW + JPEG"),
            Row::Tick("Show camera JPEGs separately", Tick::SeparateJpegs),
            Row::Hint(Hint::Pairing),
            Row::Section("Edits"),
            Row::Tick("Save ratings and edits next to photos", Tick::Sidecars),
            Row::Hint(Hint::Sidecars),
        ],
        Tab::Speed => vec![
            Row::Section("Previews"),
            Row::Tick("Film, paper and preset previews", Tick::Previews),
            Row::Hint(Hint::Text("Show each film, paper and preset on your photograph. Turn off to use simple menus and lists on slower computers.")),
            Row::Tick("Limit preview to 12 MP", Tick::LimitPreview),
            Row::Hint(Hint::Text("Develops the preview faster on large photographs. Export keeps the full Image Size.")),
            Row::Section("Export"),
            Row::Tick("Use less memory for export", Tick::LowMemory),
            Row::Hint(Hint::Text("Develops the grain one colour at a time. Same result, less memory, longer export.")),
        ],
        Tab::Upkeep => {
            let mut rows = vec![Row::Section("Updates"), Row::Updates, Row::Hint(Hint::Text("Install a release build to receive updates."))];
            rows.push(Row::Section("Lightroom"));
            for kind in Kind::ALL {
                rows.push(Row::Lightroom(kind));
                rows.push(Row::Hint(Hint::Lightroom(kind)));
            }
            rows
        }
        Tab::Look => vec![
            Row::Section("Theme"),
            Row::Themes,
            Row::Hint(Hint::Theme),
            Row::Section("Viewer"),
            Row::List("Sheet", List::Sheet),
            Row::Hint(Hint::Text("The colour behind the photograph. Theme’s own follows the theme; a grey is fairer for judging a photograph. The caption and the label under it change their ink to suit.")),
        ],
    }
}

pub fn ticked(model: &negs_app::model::Model, tick: Tick) -> bool {
    let prefs = &model.prefs;
    match tick {
        Tick::MatchNew => prefs.match_new_photos,
        Tick::NegsBuilt => prefs.show_negs_built_films,
        Tick::SeparateJpegs => prefs.library.separate_camera_jpegs,
        Tick::Sidecars => prefs.library.sidecars(),
        Tick::Previews => prefs.previews,
        Tick::LimitPreview => prefs.limits_preview(),
        Tick::LowMemory => prefs.low_memory_export,
    }
}

pub fn set_tick(model: &mut negs_app::model::Model, tick: Tick, on: bool) {
    match tick {
        Tick::MatchNew => model.set_match_new_photos(on),
        Tick::NegsBuilt => model.set_show_negs_built_films(on),
        Tick::SeparateJpegs => model.set_separate_camera_jpegs(on),
        Tick::Sidecars => model.set_sidecars(on),
        Tick::Previews => model.set_previews(on),
        Tick::LimitPreview => model.set_limit_preview(on),
        Tick::LowMemory => model.set_low_memory_export(on),
    }
}

pub fn hint_text(model: &negs_app::model::Model, theme: ThemeName, hint: Hint) -> &'static str {
    match hint {
        Hint::Text(text) => text,
        Hint::NewPhotos if model.new_photo_film_is_slide() => NEW_PHOTOS_SLIDE,
        Hint::NewPhotos => NEW_PHOTOS,
        Hint::Pairing if model.prefs.library.separate_camera_jpegs => SEPARATE_JPEGS,
        Hint::Pairing => PAIRED_JPEGS,
        Hint::Sidecars if model.prefs.library.sidecars() => SIDECARS_ON,
        Hint::Sidecars => SIDECARS_OFF,
        Hint::Theme => theme.hint(),
        Hint::Lightroom(Kind::Cloud) => LIGHTROOM_CLOUD,
        Hint::Lightroom(Kind::Classic) => LIGHTROOM_CLASSIC,
    }
}

/// "4.9 MB", as the Mac's `ByteCountFormatter` in its file style.
pub fn byte_count(bytes: u64) -> String {
    let units = ["bytes", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    match unit {
        0 if bytes == 0 => "Zero KB".into(),
        0 => format!("{bytes} bytes"),
        1 => format!("{} KB", value.round()),
        _ => format!("{value:.1} {}", units[unit]),
    }
}

pub struct SettingsView {
    app: Entity<NegsApp>,
    focus: FocusHandle,
    open: Option<List>,
    /// The bytes each Lightroom's sent copies take, once measured.
    sent: [Option<u64>; 2],
    deleting: bool,
}

impl SettingsView {
    fn new(app: Entity<NegsApp>, cx: &mut Context<Self>) -> Self {
        cx.observe(&app, |_, _, cx| cx.notify()).detach();
        let mut view = SettingsView { app, focus: cx.focus_handle(), open: None, sent: [None, None], deleting: false };
        view.measure_sent(cx);
        view
    }

    fn measure_sent(&mut self, cx: &mut Context<Self>) {
        let task = cx.background_executor().spawn(async { Kind::ALL.map(|k| negs_app::lightroom::size_of(&k.folder())) });
        cx.spawn(async move |this, cx| {
            let sizes = task.await;
            let _ = this.update(cx, |view, cx| {
                view.sent = sizes;
                cx.notify();
            });
        })
        .detach();
    }

    /// Settings › Lightroom shows a kind that's installed, or has copies left.
    fn lightroom_shown(&self, kinds: &[Kind], kind: Kind) -> bool {
        kinds.contains(&kind) || self.sent[kind as usize].is_some_and(|b| b > 0)
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut NegsApp, &mut Context<NegsApp>)) {
        self.app.update(cx, |app, cx| {
            f(app, cx);
            app.after_model_update(cx);
            cx.notify();
        });
        cx.notify();
    }

    fn delete_sent(&mut self, kind: Kind, window: &mut Window, cx: &mut Context<Self>) {
        if self.deleting || self.app.read(cx).model.batch_running() {
            return;
        }
        let go = move |view: &mut SettingsView, cx: &mut Context<SettingsView>| {
            view.deleting = true;
            cx.notify();
            let task = cx.background_executor().spawn(async move { negs_app::lightroom::delete_sent(&kind.folder()) });
            cx.spawn(async move |this, cx| {
                task.await;
                let _ = this.update(cx, |view, cx| {
                    view.deleting = false;
                    view.measure_sent(cx);
                });
            })
            .detach();
        };
        if kind == Kind::Classic {
            let answer = window.prompt(
                gpui::PromptLevel::Warning,
                "Delete the TIFFs sent to Lightroom Classic?",
                Some("Lightroom Classic may still use them: photos it added from “Negs for Lightroom” go missing there."),
                &["Delete", "Cancel"],
                cx,
            );
            cx.spawn(async move |this, cx| {
                if answer.await == Ok(0) {
                    let _ = this.update(cx, |view, cx| go(view, cx));
                }
            })
            .detach();
        } else {
            go(self, cx);
        }
    }
}

impl Focusable for SettingsView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let app = self.app.read(cx);
        let theme = app.theme.clone();
        let tab = Tab::parse(&app.model.prefs.settings_tab);
        let model = &app.model;
        let kinds = model.lightroom_kinds();
        let serif = theme.serif_family.clone();
        let (paper, label, dim, accent, track, line) = (theme.paper, theme.label, theme.dim, theme.accent, theme.track, theme.line);

        // The tabs, as words with the pencil under the chosen one.
        let mut tabs = div().flex().gap(px(22.0)).px(px(28.0)).pb(px(8.0)).border_b_1().border_color(line);
        // Off the Mac: a strip to drag the window by, with Close (GPUI's
        // own title bar is hidden there).
        let top = div()
            .flex()
            .h(px(36.0))
            .flex_none()
            .child(div().flex_1().h_full().when(!cfg!(target_os = "macos"), |d| d.window_control_area(gpui::WindowControlArea::Drag)))
            .children(crate::titlebar::caption_buttons(&theme, window, true));
        for t in Tab::ALL {
            let chosen = t == tab;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("settings-tab-{}", t.title())))
                    .flex()
                    .flex_col()
                    .cursor_pointer()
                    .child(div().font_family(serif.clone()).text_size(px(16.0)).text_color(if chosen { paper } else { dim }).child(t.title()))
                    .child(div().h(px(2.0)).w_full().when(chosen, |d| d.bg(accent)))
                    .on_click(cx.listener(move |view, _, _w, cx| {
                        view.open = None;
                        view.change(cx, |app, _cx| {
                            app.model.prefs.settings_tab = t.title().into();
                            app.model.prefs.save();
                        });
                    })),
            );
        }

        let mut body = div().id("settings-body").flex().flex_col().px(px(28.0)).pb(px(24.0)).overflow_y_scroll().flex_1().min_h_0();
        for (i, row) in rows(tab).into_iter().enumerate() {
            let shown = match row {
                Row::Section("Lightroom") => Kind::ALL.into_iter().any(|k| self.lightroom_shown(&kinds, k)),
                Row::Lightroom(k) | Row::Hint(Hint::Lightroom(k)) => self.lightroom_shown(&kinds, k),
                _ => true,
            };
            if !shown {
                continue;
            }
            let el: AnyElement = match row {
                Row::Section(title) => div().pt(px(20.0)).pb(px(6.0)).font_family(theme.ui_family.clone()).text_size(px(10.5)).font_weight(gpui::FontWeight(600.0)).text_color(dim).child(title.to_uppercase()).into_any_element(),
                Row::Hint(hint) => div().w_full().min_w_0().pb(px(6.0)).text_size(px(11.5)).text_color(dim).child(hint_text(model, theme.name, hint)).into_any_element(),
                Row::Tick(title, tick) => {
                    let on = ticked(model, tick);
                    div()
                        .id(("settings-tick", i))
                        .flex()
                        .items_center()
                        .gap(px(10.0))
                        .py(px(6.0))
                        .cursor_pointer()
                        .child(div().size(px(14.0)).rounded(px(3.0)).border_1().border_color(if on { accent } else { track }).when(on, |d| d.bg(accent)).flex().items_center().justify_center().when(on, |d| d.child(div().font_family(theme.ui_family.clone()).text_size(px(10.0)).text_color(theme.on_fill).child("✓"))))
                        .child(div().text_size(px(13.0)).text_color(label).child(title))
                        .on_click(cx.listener(move |view, _, _w, cx| view.change(cx, move |app, _| set_tick(&mut app.model, tick, !on))))
                        .into_any_element()
                }
                Row::Fotufilm => {
                    let chosen = FotufilmFilms::from_key(&model.prefs.fotufilm_films);
                    let mut words = div().flex().gap(px(14.0));
                    for (j, films) in FotufilmFilms::ALL.into_iter().enumerate() {
                        let on = films == chosen;
                        words = words.child(
                            div()
                                .id(("settings-fotufilm", j))
                                .flex()
                                .flex_col()
                                .cursor_pointer()
                                .child(div().font_family(serif.clone()).text_size(px(14.5)).text_color(if on { paper } else { dim }).child(films.title()))
                                .child(div().h(px(2.0)).w_full().when(on, |d| d.bg(accent)))
                                .on_click(cx.listener(move |view, _, _w, cx| view.change(cx, move |app, _| app.model.set_fotufilm_films(films)))),
                        );
                    }
                    div().flex().items_center().gap(px(12.0)).py(px(6.0)).child(div().w(px(110.0)).text_size(px(13.0)).text_color(label).child("Fotufilm films")).child(words).into_any_element()
                }
                Row::List(title, list) => {
                    let (choices, chosen, enabled): (Vec<(String, String)>, String, bool) = match list {
                        List::NewFilm => (model.new_photo_film_choices(), model.new_photo_stocks().0, true),
                        List::NewPaper => (model.new_photo_paper_choices(), model.new_photo_stocks().1, !model.new_photo_film_is_slide()),
                        List::Sheet => (ViewerSheet::ALL.iter().map(|s| (s.key().to_string(), s.title().to_string())).collect(), crate::sheet::from_prefs(&model.prefs).key().to_string(), true),
                    };
                    let shown_name = choices.iter().find(|(id, _)| *id == chosen).map(|(_, n)| n.clone()).unwrap_or_else(|| "—".into());
                    let open = self.open == Some(list);
                    let mut el = div().flex().flex_col().py(px(4.0)).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.0))
                            .child(div().w(px(110.0)).text_size(px(13.0)).text_color(label).child(title))
                            .child(
                                div()
                                    .id(("settings-list", i))
                                    .flex_1()
                                    .flex()
                                    .justify_between()
                                    .items_center()
                                    .px(px(10.0))
                                    .py(px(4.0))
                                    .rounded(px(4.0))
                                    .border_1()
                                    .border_color(if open { accent } else { track })
                                    .when(enabled, |d| d.cursor_pointer())
                                    .when(!enabled, |d| d.opacity(negs_app::theme::DISABLED_OPACITY))
                                    .child(div().font_family(serif.clone()).text_size(px(14.0)).text_color(paper).when(!enabled && list == List::NewPaper, |d| d.line_through()).child(shown_name))
                                    .child(div().font_family(theme.ui_family.clone()).text_size(px(10.0)).text_color(dim).child(if open { "▴" } else { "▾" }))
                                    .when(enabled, |d| {
                                        d.on_click(cx.listener(move |view, _, _w, cx| {
                                            view.open = if view.open == Some(list) { None } else { Some(list) };
                                            cx.notify();
                                        }))
                                    }),
                            ),
                    );
                    if open {
                        let mut options = div().id(("settings-options", i)).ml(px(122.0)).mt(px(4.0)).max_h(px(240.0)).overflow_y_scroll().flex().flex_col().rounded(px(4.0)).border_1().border_color(line).bg(theme.film);
                        for (j, (id, name)) in choices.into_iter().enumerate() {
                            let on = id == chosen;
                            options = options.child(
                                div()
                                    .id(("settings-option", j))
                                    .px(px(10.0))
                                    .py(px(4.0))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(theme.subtle_hover))
                                    .font_family(serif.clone())
                                    .text_size(px(14.0))
                                    .text_color(if on { paper } else { label })
                                    .child(name)
                                    .on_click(cx.listener(move |view, _, _w, cx| {
                                        view.open = None;
                                        let id = id.clone();
                                        view.change(cx, move |app, cx| match list {
                                            List::NewFilm => app.model.set_new_photo_film(&id),
                                            List::NewPaper => app.model.set_new_photo_paper(&id),
                                            List::Sheet => {
                                                if let Some(sheet) = ViewerSheet::ALL.into_iter().find(|s| s.key() == id) {
                                                    app.choose_sheet(sheet, cx);
                                                }
                                            }
                                        });
                                    })),
                            );
                        }
                        el = el.child(options);
                    }
                    el.into_any_element()
                }
                Row::Updates => div().py(px(6.0)).text_size(px(13.0)).text_color(label).child("Updates are not configured for this build.").into_any_element(),
                Row::Lightroom(kind) => {
                    let words = match self.sent[kind as usize] {
                        Some(bytes) => format!("{} sent", byte_count(bytes)),
                        None => "Nothing sent".into(),
                    };
                    let can = !self.deleting && self.sent[kind as usize].is_some_and(|b| b > 0);
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .py(px(6.0))
                        .child(div().flex().gap(px(8.0)).child(div().text_size(px(13.0)).text_color(label).child(kind.title())).child(div().font_family(serif.clone()).italic().text_size(px(13.5)).text_color(dim).child(words)))
                        .child(
                            div()
                                .id(("settings-delete-sent", i))
                                .px(px(10.0))
                                .py(px(3.0))
                                .rounded(px(4.0))
                                .border_1()
                                .border_color(track)
                                .text_size(px(12.0))
                                .text_color(label)
                                .when(!can, |d| d.opacity(negs_app::theme::DISABLED_OPACITY))
                                .when(can, |d| d.cursor_pointer().on_click(cx.listener(move |view, _, window, cx| view.delete_sent(kind, window, cx))))
                                .child(if self.deleting { "Deleting…" } else { "Delete Sent Copies" }),
                        )
                        .into_any_element()
                }
                Row::Themes => {
                    let mut grid = div().flex().flex_wrap().gap(px(12.0)).py(px(6.0));
                    for (j, name) in ThemeName::ALL.into_iter().enumerate() {
                        let t = crate::theme_gpui::GpuiTheme::from_name(name);
                        let on = name == theme.name;
                        grid = grid.child(
                            div()
                                .id(("settings-theme", j))
                                .w(px(118.0))
                                .flex()
                                .flex_col()
                                .gap(px(6.0))
                                .cursor_pointer()
                                .child(
                                    div()
                                        .h(px(62.0))
                                        .rounded(px(6.0))
                                        .border_2()
                                        .border_color(if on { accent } else { line })
                                        .bg(t.canvas)
                                        .flex()
                                        .overflow_hidden()
                                        .child(div().flex_1().m(px(10.0)).bg(t.film).rounded(px(2.0)).child(div().m(px(6.0)).h(px(4.0)).w(px(30.0)).bg(t.accent)))
                                        .child(div().w(px(30.0)).h_full().bg(t.panel).border_l_1().border_color(t.line)),
                                )
                                .child(div().font_family(serif.clone()).text_size(px(14.0)).text_color(if on { paper } else { label }).when(on, |d| d.font_weight(gpui::FontWeight(700.0))).child(name.title()))
                                .on_click(cx.listener(move |view, _, _w, cx| view.change(cx, move |app, cx| app.set_theme(name, cx)))),
                        );
                    }
                    grid.into_any_element()
                }
            };
            body = body.child(el);
        }

        div()
            .id("settings-root")
            .track_focus(&self.focus)
            .key_context("Settings")
            .on_key_down(cx.listener(|view, e: &KeyDownEvent, window, cx| {
                if e.keystroke.key == "escape" {
                    if view.open.take().is_none() {
                        window.remove_window();
                    }
                    cx.notify();
                }
            }))
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.panel)
            .text_color(paper)
            .font_family(theme.ui_family.clone())
            .child(top)
            .child(tabs)
            .child(body)
    }
}

/// Opens Settings, or brings it to the front. The window opens after this
/// update: `open_window` draws it at once, and its view reads the app.
pub fn open(app: &mut NegsApp, cx: &mut Context<NegsApp>) {
    if let Some(handle) = app.settings_window
        && handle.update(cx, |_, window, _| window.activate_window()).is_ok()
    {
        return;
    }
    let entity = cx.entity();
    cx.defer(move |cx| {
        let bounds = Bounds::centered(None, size(px(520.0), px(640.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_min_size: Some(size(px(440.0), px(420.0))),
            titlebar: Some(gpui::TitlebarOptions { title: Some(SharedString::from("Settings")), appears_transparent: true, traffic_light_position: Some(gpui::Point::new(px(14.0), px(13.0))) }),
            app_id: Some("negs".to_string()),
            ..Default::default()
        };
        let app_entity = entity.clone();
        let handle: Option<WindowHandle<SettingsView>> = cx
            .open_window(options, move |window, cx| {
                let view = cx.new(|cx| SettingsView::new(app_entity, cx));
                window.focus(&view.read(cx).focus.clone());
                view
            })
            .ok();
        entity.update(cx, |app, _| app.settings_window = handle);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pages_hold_the_windows_apps_sections() {
        let sections = |tab| rows(tab).into_iter().filter_map(|r| if let Row::Section(s) = r { Some(s) } else { None }).collect::<Vec<_>>();
        assert_eq!(sections(Tab::Photographs), ["New photographs", "Films", "RAW + JPEG", "Edits"]);
        assert_eq!(sections(Tab::Speed), ["Previews", "Export"]);
        assert_eq!(sections(Tab::Upkeep), ["Updates", "Lightroom"]);
        assert_eq!(sections(Tab::Look), ["Theme", "Viewer"]);
        assert_eq!(Tab::parse("Look"), Tab::Look);
        assert_eq!(Tab::parse("nonsense"), Tab::Photographs);
    }

    /// Reads only: the setters save the preferences, and `negs-app`'s
    /// test-profile guard isn't compiled into this crate's tests.
    #[test]
    fn ticks_read_the_preferences() {
        let mut prefs = negs_app::session::Preferences::default();
        prefs.match_new_photos = true;
        prefs.low_memory_export = false;
        prefs.show_negs_built_films = true;
        let model = negs_app::model::Model::new(prefs);
        assert!(ticked(&model, Tick::MatchNew));
        assert!(!ticked(&model, Tick::LowMemory));
        assert!(ticked(&model, Tick::NegsBuilt));
    }

    #[test]
    fn sizes_read_as_the_mac_writes_them() {
        assert_eq!(byte_count(0), "Zero KB");
        assert_eq!(byte_count(512), "512 bytes");
        assert_eq!(byte_count(4_900_000), "4.9 MB");
        assert_eq!(byte_count(1_200_000_000), "1.2 GB");
    }
}
