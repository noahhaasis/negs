//! What the in-window menu bar shows for each item as it opens: enabled,
//! ticked, and the titles that follow the model ("Undo Change Film", "Done
//! Cropping", "Move 3 Photographs to Recycle Bin…"), as `windows/`'s
//! `update_menu` and `library::update_menu` set them. GPUI 0.2.2's own menus
//! (the Mac's menu bar) can't carry these; ours can.

use gpui::{Action, App, Window};
use negs_app::model::Pane;
use negs_library::Flag;

use crate::*;

/// An item's state; `title` replaces the menu's own words when set.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub enabled: bool,
    pub checked: bool,
    pub title: Option<String>,
}

fn on(enabled: bool) -> Option<State> {
    Some(State { enabled, ..State::default() })
}

fn tick(enabled: bool, checked: bool) -> Option<State> {
    Some(State { enabled, checked, title: None })
}

/// The state the model gives `action`, or None for "enabled if some view
/// handles it" (GPUI's `is_action_available`).
pub fn state(app: &NegsApp, action: &dyn Action, window: &Window, _cx: &App) -> Option<State> {
    let model = &app.model;
    let any = action.as_any();
    let photo = model.source.is_some();
    let cropping = model.cropping();
    let library = model.shelf.library.as_ref();
    let can = model.can_change_library();
    let summary = library.zip(model.rating_targets().first().cloned()).map(|(l, i)| l.summary(&i));
    let rating = summary.as_ref().map(|s| s.rating as i64);
    let flag = summary.as_ref().and_then(|s| s.flag);
    let windows = cfg!(target_os = "windows");
    macro_rules! is {
        ($t:ty) => {
            any.is::<$t>()
        };
    }
    if is!(UndoAction) {
        let title = model.undo_title().map_or_else(|| "&Undo".to_string(), |t| format!("&Undo {t}"));
        return Some(State { enabled: model.undo_title().is_some() && !cropping, checked: false, title: Some(title) });
    }
    if is!(RedoAction) {
        let title = model.redo_title().map_or_else(|| "&Redo".to_string(), |t| format!("&Redo {t}"));
        return Some(State { enabled: model.redo_title().is_some() && !cropping, checked: false, title: Some(title) });
    }
    if is!(SavePreview) {
        return on(model.can_save());
    }
    if is!(ExportPhoto) {
        return Some(State { enabled: model.can_export_any(), checked: false, title: Some(format!("&{}", model.export_title())) });
    }
    if is!(ExportLut) {
        return on(model.can_export_lut());
    }
    if is!(OpenInLightroom) {
        return on(model.can_open_in_lightroom(negs_app::lightroom::Kind::Cloud));
    }
    if is!(OpenInLightroomClassic) {
        return on(model.can_open_in_lightroom(negs_app::lightroom::Kind::Classic));
    }
    if is!(NewPreset) || is!(ImportPresets) {
        return on(model.can_change_presets());
    }
    if is!(CheckForUpdates) {
        return on(crate::updates::configured());
    }
    if is!(ToggleOriginal) {
        return tick(model.can_show_original() && !cropping, model.holding_original);
    }
    for (stars, hit) in [is!(Rate0), is!(Rate1), is!(Rate2), is!(Rate3), is!(Rate4), is!(Rate5)].into_iter().enumerate() {
        if hit {
            return tick(can, rating == Some(stars as i64));
        }
    }
    if is!(FlagPick) {
        return tick(can, flag == Some(Flag::Pick));
    }
    if is!(FlagReject) {
        return tick(can, flag == Some(Flag::Reject));
    }
    if is!(FlagUnflag) {
        return tick(can, summary.is_some() && flag.is_none());
    }
    if is!(RaiseRating) || is!(LowerRating) || is!(CopyEdits) || is!(DuplicatePhoto) {
        return on(can);
    }
    if is!(ResetEdits) {
        return on(if library.is_some() { can } else { model.has_catalog() && !cropping });
    }
    if is!(PasteEdits) {
        return on(model.can_paste_edits());
    }
    if is!(PastePrevious) {
        return on(model.can_paste_from_previous());
    }
    if is!(TrashPhoto) {
        let count = model.delete_targets().len();
        let bin = if windows { "Recycle Bin" } else { "Trash" };
        let title = if count > 1 { format!("Move {count} Photographs to {bin}…") } else { format!("&Move to {bin}…") };
        return Some(State { enabled: can && count > 0, checked: false, title: Some(title) });
    }
    if is!(TrashRejected) {
        return on(can && !model.rejected().is_empty());
    }
    if is!(ToggleAdvance) {
        return tick(library.is_some(), model.prefs.library.advance_after_rating);
    }
    if is!(ClearFilter) {
        return on(model.library_filter().active());
    }
    if is!(ToggleFilmstrip) {
        return tick(true, model.prefs.filmstrip_visible);
    }
    if is!(ShowAdjustments) {
        return tick(true, model.inspector_visible && model.pane() == Pane::Adjustments);
    }
    if is!(ShowPresets) {
        return tick(true, model.inspector_visible && model.pane() == Pane::Presets);
    }
    if is!(ToggleInspector) {
        return tick(true, model.inspector_visible);
    }
    if is!(ToggleAllControls) {
        return tick(true, model.prefs.lab_all_controls);
    }
    if is!(ToggleImageOnly) {
        return tick(photo && !cropping, app.canvas.image_only);
    }
    if is!(ToggleFullScreen) {
        return tick(true, window.is_fullscreen());
    }
    if is!(ToggleGrid) {
        return tick(library.is_some() && !cropping, app.grid.shown);
    }
    if is!(ToggleCompare) {
        let title = negs_app::compare::menu_title(model.comparison().is_some()).split('\t').next().unwrap_or_default().to_string();
        return Some(State { enabled: model.can_toggle_compare(), checked: false, title: Some(title) });
    }
    if is!(ToggleSplit) {
        return tick(photo && !cropping, app.canvas.split_mode);
    }
    if is!(ZoomIn) || is!(ZoomOut) {
        return on(app.grid.covers() || (photo && !cropping));
    }
    if is!(ZoomFit) || is!(ZoomActual) || is!(ToggleZoomFit) {
        return on(photo && !cropping);
    }
    if is!(RotatePhoto) || is!(FlipPhoto) {
        return on(model.image.is_some());
    }
    if is!(ToggleCrop) {
        let title = if cropping { "Done &Cropping" } else { "&Crop" };
        return Some(State { enabled: cropping || model.image.is_some(), checked: false, title: Some(title.into()) });
    }
    let theme = app.theme.name;
    for (name, hit) in [
        (ThemeName::Darkroom, is!(ThemeDarkroom)),
        (ThemeName::ContactSheet, is!(ThemeContactSheet)),
        (ThemeName::LightTable, is!(ThemeLightTable)),
        (ThemeName::Daylight, is!(ThemeDaylight)),
        (ThemeName::Plain, is!(ThemePlain)),
        (ThemeName::PlainDark, is!(ThemePlainDark)),
    ] {
        if hit {
            return tick(true, theme == name);
        }
    }
    let chosen = sheet::from_prefs(&model.prefs);
    for (s, hit) in [
        (sheet::ViewerSheet::Theme, is!(SheetTheme)),
        (sheet::ViewerSheet::Darkroom, is!(SheetDarkroom)),
        (sheet::ViewerSheet::Black, is!(SheetBlack)),
        (sheet::ViewerSheet::Charcoal, is!(SheetCharcoal)),
        (sheet::ViewerSheet::Grey22, is!(SheetGrey22)),
        (sheet::ViewerSheet::MiddleGrey, is!(SheetMiddleGrey)),
        (sheet::ViewerSheet::LightGrey, is!(SheetLightGrey)),
        (sheet::ViewerSheet::Paper, is!(SheetPaper)),
    ] {
        if hit {
            return tick(true, chosen == s);
        }
    }
    None
}
