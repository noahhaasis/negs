//! Negs — cross-platform GPUI desktop application.
//!
//! Phase 2 + 3a: GPUI shell, Newsreader typography, 6 themes, `Host` bridge to
//! `negs_app::model::Model`, exact-fit Lanczos + monitor ICC / macOS Display P3
//! photo canvas, vector `negs-marks` overlay, native Open/Save dialogs,
//! drag-and-drop, keyboard shortcuts, `NEGS_STARTUP_REPORT` instrumentation,
//! custom darkroom widgets, and the complete Prep / Lab / Output Inspector.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bench;
mod canvas;
mod color_display;
mod batch_view;
mod compare_view;
mod export_card;
mod crop_view;
mod facts;
mod grid;
mod host_gpui;
mod inspector;
mod library_menu;
mod marks_gpui;
mod marks_raster;
mod motion;
mod place;
mod presets;
mod print;
mod menu_bar;
mod menu_state;
mod settings_window;
mod sheet;
mod status;
mod strip;
mod theme_gpui;
mod trace_ui;
mod updates;
mod win;
mod thumbs;
mod tile_cache;
mod titlebar;
mod tooltip;
mod tray;
mod widgets;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, App, AppContext, Application, Context, FocusHandle, Focusable, IntoElement, KeyBinding,
    KeyDownEvent, Menu, MenuItem, MouseButton, MouseMoveEvent, MouseUpEvent, PathPromptOptions,
    Pixels, Point, SharedString, TitlebarOptions, Window, WindowBounds, WindowOptions, actions, div,
    prelude::*, px, size,
};
use negs_app::host;
use negs_app::model::{LibraryKey, Model, Pane};
use negs_app::platform;
use negs_app::session::{Preferences, SessionStore};
use negs_app::theme::ThemeName;
use negs_library::Flag;

use crate::canvas::{CanvasState, render_canvas};
use crate::color_display::active_display_profile;
use crate::host_gpui::{GpuiHost, HostChannels};
use crate::inspector::{
    InspectorState, build_inspector_view, handle_root_mouse_move, handle_root_mouse_up,
    render_inspector_column,
};
use crate::grid::GridState;
use crate::library_menu::MenuState;
use crate::marks_raster::MarkSprites;
use crate::presets::PresetsState;
use crate::strip::StripState;
use crate::thumbs::Thumbs;
use crate::status::render_status_bar;
use crate::theme_gpui::{GpuiTheme, initial_theme, next_theme, register_fonts};
use crate::titlebar::{render_theme_menu, render_titlebar};
use crate::tray::{TrayState, render_tray};

actions!(
    negs,
    [
        OpenPhoto,
        SavePreview,
        ExportPhoto,
        ExportLut,
        OpenSettings,
        AboutNegs,
        CopyEdits,
        PasteEdits,
        PastePrevious,
        DuplicatePhoto,
        ResetEdits,
        TrashPhoto,
        TrashRejected,
        ToggleAdvance,
        ClearFilter,
        RaiseRating,
        LowerRating,
        OpenInLightroom,
        OpenInLightroomClassic,
        NewPreset,
        ImportPresets,
        ShowPresetsFolder,
        ToggleFullScreen,
        Recent0,
        Recent1,
        Recent2,
        Recent3,
        Recent4,
        Recent5,
        Recent6,
        Recent7,
        Recent8,
        Recent9,
        UndoAction,
        RedoAction,
        ToggleOriginal,
        ToggleSplit,
        ToggleZoomFit,
        ZoomIn,
        ZoomOut,
        ZoomFit,
        Rate0,
        Rate1,
        Rate2,
        Rate3,
        Rate4,
        Rate5,
        FlagPick,
        FlagReject,
        FlagUnflag,
        CycleTheme,
        ToggleInspector,
        ShowAdjustments,
        ShowPresets,
        ToggleGrid,
        ToggleFilmstrip,
        ToggleCrop,
        RotatePhoto,
        FlipPhoto,
        ToggleImageOnly,
        ToggleCompare,
        ZoomActual,
        SheetTheme,
        SheetDarkroom,
        SheetBlack,
        SheetCharcoal,
        SheetGrey22,
        SheetMiddleGrey,
        SheetLightGrey,
        SheetPaper,
        ThemeDarkroom,
        ThemeContactSheet,
        ThemeLightTable,
        ThemeDaylight,
        ThemePlain,
        ThemePlainDark,
        QuitApp,
        CheckForUpdates,
        ToggleAllControls,
    ]
);

/// Monotonic tick logger for `NEGS_STARTUP_REPORT` (matching `windows/src/ui/mod.rs`).
pub struct StartupReport {
    file: Option<std::fs::File>,
    seen: Vec<&'static str>,
}

impl StartupReport {
    fn new() -> Self {
        // Prime the monotonic clock epoch at process startup so all subsequent
        // timestamps measure elapsed time since main() entry.
        let _ = platform::monotonic_ticks();
        let file = std::env::var_os("NEGS_STARTUP_REPORT").and_then(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        });
        Self {
            file,
            seen: Vec::new(),
        }
    }

    pub fn stamp(&mut self, name: &'static str) {
        if self.seen.contains(&name) {
            return;
        }
        self.seen.push(name);
        if let Some(f) = self.file.as_mut() {
            let _ = writeln!(f, "{name} {}", platform::monotonic_ticks());
            let _ = f.flush();
        }
    }
}

/// Root GPUI view owning the pure-Rust `Model`, canvas state, inspector state,
/// active theme, and host handle.
pub struct NegsApp {
    pub model: Model,
    pub host: Arc<GpuiHost>,
    pub theme: GpuiTheme,
    pub theme_menu_open: bool,
    pub canvas: CanvasState,
    pub inspector: InspectorState,
    pub presets: PresetsState,
    pub tray: TrayState,
    /// The strip's and grid's thumbnails, their frames, and the marks' sprites.
    pub thumbs: Thumbs,
    pub strip: StripState,
    pub grid: GridState,
    pub sprites: MarkSprites,
    /// The camera's facts, for the caption.
    pub facts: facts::Facts,
    /// The crop panel's fields, and the angle slider's track.
    pub crop_panel: crop_view::CropPanel,
    pub crop_panel_track: (f32, f32),
    /// Compare's pictures, frames and words.
    pub compare: compare_view::CompareView,
    pub export_card: Option<export_card::ExportCard>,
    /// A one-second tick runs while an export or a batch does (its seconds, its rule).
    ticking: bool,
    pub menu_bar: Option<menu_bar::Open>,
    /// Alt held on its own (the menu bar's access keys show, off the Mac).
    pub alt: menu_bar::AltState,
    /// The window's place, written to the preferences on quitting.
    window_place: Option<(gpui::WindowBounds, f32)>,
    /// An export or batch was running at the last frame (to flash the taskbar when it ends).
    was_exporting: bool,
    updates_scheduled: bool,
    /// Offer to resume an unfinished batch once the window can ask.
    resume_offer: bool,
    /// The Lightroom kinds File lists.
    menu_lightroom: Vec<negs_app::lightroom::Kind>,
    menu_recent: Vec<PathBuf>,
    pub settings_window: Option<gpui::WindowHandle<settings_window::SettingsView>>,
    /// The library's context or filter menu, when open.
    pub menu: Option<MenuState>,
    /// `NEGS_FRAME_BENCH`: the app driving itself and timing its frames.
    bench: Option<bench::Bench>,
    pub startup: StartupReport,
    focus_handle: FocusHandle,
    last_zoom_command: u64,
    last_source: Option<PathBuf>,
    shut_down: bool,
}

impl Focusable for NegsApp {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl NegsApp {
    fn new(
        host: Arc<GpuiHost>,
        channels: HostChannels,
        cli_path: Option<PathBuf>,
        startup: StartupReport,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let prefs = Preferences::load();
        let initial_scope_height = prefs.scope_height;
        let theme_name =
            initial_theme((!prefs.app_theme.is_empty()).then_some(prefs.app_theme.as_str()));
        let theme = GpuiTheme::from_name(theme_name);
        let focus_handle = cx.focus_handle();

        // Record native HWND on Windows so `negs_app::icc` & `platform` know the owner window.
        #[cfg(windows)]
        {
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = HasWindowHandle::window_handle(window) {
                if let RawWindowHandle::Win32(win) = handle.as_raw() {
                    host.set_owner_hwnd(win.hwnd.get());
                }
            }
        }

        let display_profile = active_display_profile(window, cx);
        if std::env::var_os("NEGS_LOG_ICC").is_some() {
            eprintln!(
                "negs-icc: display_key={} label={:?} active_transform={}",
                display_profile.display_key,
                display_profile.label,
                display_profile.transform.is_some()
            );
        }
        let mut thumbs = Thumbs::default();
        thumbs.set_display(display_profile.transform.clone());
        let mut model = Model::new(prefs);
        model.set_preview_color(display_profile.transform.clone());
        model.launch();

        if let Some(path) = cli_path {
            if path.is_dir() {
                model.open_folder(&path);
            } else {
                model.open(&path);
            }
        } else {
            model.expect_restoration();
            std::thread::spawn(move || {
                let loaded = SessionStore::default().load();
                host::post(move |m| m.restoration_loaded(loaded));
            });
        }

        // Spawn foreground task draining `Host::post` model callbacks.
        let task_rx = channels.tasks;
        cx.spawn(async move |this, cx| {
            while let Ok(first) = task_rx.recv().await {
                let mut batch = vec![first];
                while let Ok(more) = task_rx.try_recv() {
                    batch.push(more);
                }
                let updated = this.update(cx, |app, cx| {
                    for task in batch {
                        task(&mut app.model);
                    }
                    app.after_model_update(cx);
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        })
        .detach();

        // Spawn foreground task driving `Host::after` UI-thread timers.
        let timer_rx = channels.timers;
        cx.spawn(async move |this, cx| {
            while let Ok(due) = timer_rx.recv().await {
                let this_weak = this.clone();
                let wait = due.saturating_duration_since(Instant::now());
                let timer = cx.background_executor().timer(wait);
                cx.spawn(async move |cx| {
                    timer.await;
                    let _ = this_weak.update(cx, |app, cx| {
                        if GpuiHost::run_due_timers(&mut app.model) > 0 {
                            app.after_model_update(cx);
                            cx.notify();
                        }
                    });
                })
                .detach();
            }
        })
        .detach();

        // Observe window moves/resizes to refresh active monitor ICC profile automatically.
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_place = Some((window.window_bounds(), window.scale_factor()));
            this.refresh_display_profile(window, cx);
            cx.notify();
        })
        .detach();

        // Back from another app: the folder as it left it, and Lightroom
        // installed meanwhile or photos added there (`WM_ACTIVATEAPP`).
        cx.observe_window_activation(window, |this, window, cx| {
            // A profile chosen in the system's settings meanwhile.
            if window.is_window_active() {
                this.recheck_display_profile(window, cx);
            }
            if window.is_window_active() && this.model.first_render_shown {
                this.model.library_refresh();
                if this.model.lightroom_detected() {
                    this.model.refresh_lightroom(Duration::ZERO);
                }
                this.after_model_update(cx);
            }
        })
        .detach();

        let inspector = InspectorState::new(initial_scope_height, cx);
        let presets = PresetsState::new(cx);
        let tray = TrayState::new(cx);
        let mut app = Self {
            model,
            host,
            theme,
            theme_menu_open: false,
            canvas: CanvasState::with_display_profile(display_profile),
            inspector,
            presets,
            tray,
            thumbs,
            strip: StripState::default(),
            grid: GridState::default(),
            sprites: MarkSprites::default(),
            facts: facts::Facts::default(),
            crop_panel: crop_view::CropPanel::new(cx),
            crop_panel_track: (0.0, 1.0),
            compare: compare_view::CompareView::default(),
            export_card: None,
            ticking: false,
            settings_window: None,
            menu_bar: None,
            alt: menu_bar::AltState::default(),
            window_place: None,
            was_exporting: false,
            updates_scheduled: false,
            resume_offer: false,
            menu_lightroom: Vec::new(),
            menu_recent: Vec::new(),
            menu: None,
            bench: bench::Bench::from_env(),
            startup,
            focus_handle,
            last_zoom_command: 0,
            last_source: None,
            shut_down: false,
        };
        app.startup.stamp("gpu_ready");
        app.window_place = Some((window.window_bounds(), window.scale_factor()));
        // Logging off or shutting down: the edits and the session saved, nothing asked.
        let weak = cx.entity().downgrade();
        let mut async_cx = cx.to_async();
        let handed_over = win::watch_main_window(window, Box::new(move || {
            let _ = weak.update(&mut async_cx, |app, _| app.shutdown_cleanly());
        }));
        // A photograph opened from Explorer while Negs runs (win.rs).
        if let Some(paths) = handed_over {
            cx.spawn(async move |this, cx| {
                while let Ok(path) = paths.recv().await {
                    let _ = this.update(cx, |app, cx| {
                        if path.exists() {
                            app.open_dropped_paths(&[path], cx);
                        }
                    });
                }
            })
            .detach();
        }
        // WinSparkle asks to quit to install an update.
        if let Some(requests) = updates::quit_requests() {
            cx.spawn(async move |this, cx| {
                while requests.recv().await.is_ok() {
                    let _ = this.update(cx, |app, cx| {
                        app.shutdown_cleanly();
                        cx.quit();
                    });
                }
            })
            .detach();
        }
        app.after_model_update(cx);
        app
    }

    pub fn stamp_startup(&mut self, name: &'static str) {
        self.startup.stamp(name);
    }

    /// The canvas asks on every paint: only a move to another display reads
    /// the profile again (reading and building its transform took most of
    /// each frame).
    pub fn refresh_display_profile(&mut self, window: &Window, cx: &App) {
        let key = window.display(cx).map(|d| u64::from(u32::from(d.id()))).unwrap_or(0);
        if key != self.canvas.display_profile.display_key {
            self.recheck_display_profile(window, cx);
        }
    }

    /// Re-queries the window's active display ICC profile and invalidates cached textures on change.
    pub fn recheck_display_profile(&mut self, window: &Window, cx: &App) {
        let next = active_display_profile(window, cx);
        let changed = next.display_key != self.canvas.display_profile.display_key
            || next.label != self.canvas.display_profile.label;
        if changed {
            self.canvas.display_profile = next.clone();
            self.canvas.invalidate_images();
            self.presets.tile_cache.invalidate();
            self.tray.tile_cache.invalidate();
            self.thumbs.set_display(next.transform.clone());
            self.model.set_preview_color(next.transform);
        }
    }

    pub fn after_model_update(&mut self, cx: &mut Context<Self>) {
        self.keep_ticking(cx);
        // File's Lightroom items follow what's installed.
        // File's Lightroom items and Open Recent follow the model.
        let kinds = self.model.lightroom_kinds();
        if kinds != self.menu_lightroom || self.model.prefs.recent_files != self.menu_recent {
            cx.set_menus(menus(&kinds, &self.model.prefs.recent_files));
            self.menu_lightroom = kinds;
            self.menu_recent = self.model.prefs.recent_files.clone();
        }
        if self.model.zoom_command != self.last_zoom_command {
            self.last_zoom_command = self.model.zoom_command;
            if self.model.fit_image {
                self.canvas.reset_fit();
            } else {
                self.canvas.set_actual_size();
            }
        }
        if self.model.source != self.last_source {
            self.last_source = self.model.source.clone();
            self.canvas.before_after = false;
            self.canvas.split_mode = false;
            self.canvas.reset_fit();
            self.canvas.invalidate_images();
            self.inspector.close_popovers();
            self.inspector.editing_slider = None;
        }
        // Cropping and closing the photograph end Image Only and upside down.
        if self.model.cropping() || self.model.source.is_none() {
            self.canvas.image_only = false;
            self.canvas.upside_down = false;
        }
        if !self.model.cropping() {
            self.canvas.crop_drag = None;
        }
        // Keep Original loaded whenever Before/After or Split mode is active.
        if (self.canvas.before_after || self.canvas.split_mode)
            && self.model.can_show_original()
            && !self.model.holding_original
        {
            self.model.hold_original(true);
        }

        // Tell the engine whether Print Curves' tone graph is currently visible.
        let tone_graph_visible = self.model.inspector_visible
            && self.model.pane() == Pane::Adjustments
            && build_inspector_view(&self.model).tone_graph;
        self.model.show_tone_graph(tone_graph_visible);
        let _ = self.model.pump_previews();

        // The first developed picture is up: the strip's thumbnails (which
        // waited, so as not to slow it) may develop now.
        if self.model.image.is_some() && !self.model.first_render_shown {
            self.model.first_render_shown = true;
            self.model.dirty |= negs_app::model::dirty::STRIP;
            self.startup.stamp("first_image_scene");
            // Which Lightroom is installed, off the launch path (ticket 48).
            self.model.refresh_lightroom(Duration::from_secs(2));
            // A batch the last session didn't finish: offered at the next frame.
            self.resume_offer = true;
            // The updater, 3 s later, never during startup.
            if !self.updates_scheduled && updates::configured() {
                self.updates_scheduled = true;
                let automatic = self.model.prefs.check_for_updates;
                cx.spawn(async move |_, cx| {
                    cx.background_executor().timer(Duration::from_secs(3)).await;
                    updates::start(automatic);
                })
                .detach();
            }
        }
        // A downloaded update waits for an export or a save.
        updates::set_busy(self.model.exporting || self.model.saving || self.model.batch_running());

        // Thumbnails that arrived; the strip and grid follow the model.
        let folder = self.model.source.as_deref().and_then(Path::parent).map(Path::to_path_buf);
        self.thumbs.set_folder(folder);
        let arrived = self.thumbs.drain();
        if self.model.dirty != 0 || arrived {
            strip::sync(self);
            grid::sync(self);
        }

        self.model.dirty = 0;
    }

    /// Returns `(photo_filename, rating 0..=5, flag)` for the open photograph.
    pub fn current_marks(&self) -> (String, u8, Option<Flag>) {
        let name = self
            .model
            .source
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(item) = self.model.library_item() else {
            return (name, 0, None);
        };
        let Some(library) = self.model.shelf.library.as_ref() else {
            return (name, 0, None);
        };
        let summary = library.summary(&item);
        (name, summary.rating, summary.flag)
    }

    /// View › Viewer Sheet: the sheet behind the photograph, remembered.
    pub fn choose_sheet(&mut self, sheet: sheet::ViewerSheet, cx: &mut Context<Self>) {
        self.model.prefs.viewer_sheet = sheet.key().into();
        self.model.prefs.save();
        self.model.dirty |= negs_app::model::dirty::CANVAS;
        cx.notify();
    }

    pub fn set_theme(&mut self, name: ThemeName, cx: &mut Context<Self>) {
        self.theme = GpuiTheme::from_name(name);
        self.model.prefs.app_theme = name.key().to_string();
        self.model.prefs.save();
        cx.notify();
    }

    pub fn cycle_theme(&mut self, cx: &mut Context<Self>) {
        let next = next_theme(self.theme.name);
        self.set_theme(next, cx);
    }

    pub fn toggle_inspector(&mut self, cx: &mut Context<Self>) {
        self.inspector.close_popovers();
        self.presets.close_context_menu();
        self.model.toggle_inspector();
        self.after_model_update(cx);
        cx.notify();
    }

    pub fn toggle_pane(&mut self, pane: Pane, cx: &mut Context<Self>) {
        self.inspector.close_popovers();
        self.presets.close_context_menu();
        self.model.toggle_pane(pane);
        self.after_model_update(cx);
        cx.notify();
    }

    pub fn toggle_before_after(&mut self, cx: &mut Context<Self>) {
        if !self.model.can_show_original() {
            return;
        }
        self.canvas.split_mode = false;
        self.canvas.before_after = !self.canvas.before_after;
        self.model.hold_original(self.canvas.before_after);
        cx.notify();
    }

    pub fn toggle_split_mode(&mut self, cx: &mut Context<Self>) {
        if !self.model.can_show_original() {
            return;
        }
        self.canvas.before_after = false;
        self.canvas.split_mode = !self.canvas.split_mode;
        self.model.hold_original(self.canvas.split_mode);
        cx.notify();
    }

    pub fn toggle_zoom_fit(&mut self, cx: &mut Context<Self>) {
        if self.model.source.is_none() || self.model.cropping() {
            return;
        }
        if self.model.comparison().is_some() {
            // Both photographs.
            let fit = !self.model.fit_image;
            self.model.zoom(fit);
            self.after_model_update(cx);
            cx.notify();
            return;
        }
        if self.canvas.fit {
            self.canvas.set_actual_size();
            self.model.fit_image = false;
        } else {
            self.canvas.reset_fit();
            self.model.fit_image = true;
        }
        cx.notify();
    }

    pub fn open_dropped_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Some(path) = paths
            .iter()
            .find(|p| p.is_dir() || negs_app::model::is_photo(p))
            .or_else(|| paths.first())
        else {
            return;
        };
        if path.is_dir() {
            self.model.open_folder(path);
        } else {
            self.model.open(path);
        }
        self.after_model_update(cx);
        cx.notify();
    }

    pub fn prompt_open_photo(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some(SharedString::from("Open Photograph")),
        });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(paths))) = receiver.await {
                if let Some(path) = paths.into_iter().next() {
                    let _ = this.update(cx, |app, cx| {
                        if path.is_dir() {
                            app.model.open_folder(&path);
                        } else {
                            app.model.open(&path);
                        }
                        app.after_model_update(cx);
                        cx.notify();
                    });
                }
            }
        })
        .detach();
    }

    /// File › Open Recent: a file that's gone is dropped from the list,
    /// with the model's error (as the empty state's list does).
    fn open_recent(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(path) = self.model.prefs.recent_files.get(index).cloned() {
            self.model.open(&path);
            self.after_model_update(cx);
            cx.notify();
        }
    }

    /// Help › About Negs: the version, the credits and the licence.
    pub fn about(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let detail = format!(
            "Version {}\n\nNegs is built on Andrea Volpato's spektrafilm, the spectral film simulation behind every stock, and turbasvin's spektrafilm-rs, the Rust port that runs it on the GPU.\n\nLicensed under the GNU General Public License, version 3.",
            env!("CARGO_PKG_VERSION")
        );
        let _ = window.prompt(gpui::PromptLevel::Info, "Negs", Some(&detail), &["OK"], cx);
    }

    /// Quitting cancels an export or a batch in flight, so it asks first
    /// (`applicationShouldTerminate`); a Save Preview being written holds
    /// the quit until it's done.
    pub fn quit_needs_asking(&self) -> bool {
        self.model.exporting || self.model.batch_running() || self.model.saving
    }

    pub fn request_quit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.saving && !self.model.exporting {
            self.model.status = "Negs quits once the preview is saved.".into();
            cx.notify();
            return;
        }
        if !self.quit_needs_asking() {
            self.shutdown_cleanly();
            cx.quit();
            return;
        }
        let (message, detail) = if self.model.batch_running() {
            ("A batch export is still in progress.", "Quitting stops it. Negs offers to finish it when you open it again.")
        } else {
            ("An export is still in progress.", "Quitting cancels it. Any existing file at the destination is left unchanged.")
        };
        let answer = window.prompt(gpui::PromptLevel::Warning, message, Some(detail), &["Continue Exporting", "Quit"], cx);
        cx.spawn(async move |this, cx| {
            if answer.await == Ok(1) {
                let _ = this.update(cx, |app, cx| {
                    app.shutdown_cleanly();
                    cx.quit();
                });
            }
        })
        .detach();
    }

    /// While an export or a batch runs, a redraw each second: the status
    /// line's seconds and the batch's rule.
    fn keep_ticking(&mut self, cx: &mut Context<Self>) {
        if self.ticking || !(self.model.exporting || self.model.batch_running()) {
            return;
        }
        self.ticking = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_secs(1)).await;
                let going = this.update(cx, |app, cx| {
                    cx.notify();
                    let going = app.model.exporting || app.model.batch_running();
                    if !going {
                        app.ticking = false;
                    }
                    going
                });
                if !matches!(going, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Errors and notices as dialogs, and the taskbar's progress (Windows).
    fn windows_chrome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // Errors in a dialog as the Windows app's; elsewhere the status bar's
        // banner shows them. The bench drives itself: nothing may wait for a
        // button.
        if self.bench.is_none() {
            if cfg!(target_os = "windows") {
                if let Some(error) = self.model.error.take() {
                    win::show_error(window, cx, &error, true);
                }
            }
            // A notice (presets imported) has nowhere else to show.
            if let Some((message, explanation)) = self.model.notice.take() {
                win::show_message(window, cx, &message, &explanation, None, false);
            }
        }
        let progress = match self.model.batch.as_ref().filter(|_| self.model.batch_running()) {
            Some(run) => {
                let (done, failed) = batch_view::View::of(run).rule;
                win::Progress::Count(((done + failed) * 1000.0).round() as u64, 1000)
            }
            None if self.model.exporting => win::Progress::Busy,
            None => win::Progress::None,
        };
        win::set_progress(window, progress);
        let exporting = progress != win::Progress::None;
        if self.was_exporting && !exporting {
            win::flash_if_background(window);
        }
        self.was_exporting = exporting;
    }

    pub fn shutdown_cleanly(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        // The tray's trial put back, and the edits written before the session.
        self.model.end_tray(true);
        if let Err(error) = self.model.library_save_now() {
            eprintln!("{error}");
        }
        if let Some(session) = self.model.session() {
            let _ = SessionStore::default().save(&session);
        }
        if let Some((bounds, scale)) = self.window_place {
            let (window, pixels) = place::to_prefs(bounds, scale);
            self.model.prefs.window = Some(window);
            self.model.prefs.window_pixels = Some(pixels);
            self.model.prefs.save();
        }
        updates::stop();
        self.thumbs.shutdown();
        self.model.shutdown();
    }

    /// F6: the strip shows or hides (remembered).
    pub fn toggle_filmstrip(&mut self, cx: &mut Context<Self>) {
        self.model.prefs.filmstrip_visible = !self.model.prefs.filmstrip_visible;
        self.model.prefs.save();
        self.model.dirty |= negs_app::model::dirty::STRIP;
        self.after_model_update(cx);
        cx.notify();
    }

    pub fn toggle_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        grid::toggle(self, window);
        window.focus(&self.focus_handle.clone());
        self.after_model_update(cx);
        cx.notify();
    }

    /// The button came up: a crop drag, the angle slider, a press on the
    /// print's words that wandered off.
    fn end_crop_drags(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        canvas::crop_drag_end(self, cx);
        let p = negs_app::crop::Point { x: f32::from(position.x) as f64, y: f32::from(position.y) as f64 };
        if compare_view::mouse_up(self, p) {
            self.after_model_update(cx);
            cx.notify();
        }
        if self.crop_panel.slider.take().is_some() {
            if let Some(s) = self.model.crop_session.as_mut() {
                s.end_straightening();
            }
            self.model.crop_changed();
            cx.notify();
        }
        if self.canvas.pressed.take().is_some() {
            cx.notify();
        }
    }

    /// A rating or flag key: the photograph, or the grid's selection.
    fn library_key(&mut self, key: LibraryKey, cx: &mut Context<Self>) {
        self.model.library_key(key);
        self.after_model_update(cx);
        cx.notify();
    }

    /// Keys no binding took: the menu's Esc, the grid's, the strip's ← and
    /// →, Delete. Not while typing in a field.
    fn root_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let typing = window.context_stack().iter().any(|c| c.contains("TextInput"));
        if typing {
            return false;
        }
        // The export card is modal: Esc and Return are its, the rest wait.
        if export_card::key(self, event.keystroke.key.as_str(), window, cx) {
            return true;
        }
        let key = event.keystroke.key.as_str();
        let m = &event.keystroke.modifiers;
        if key == "escape" && menu_bar::close(self) {
            cx.notify();
            return true;
        }
        if self.menu.is_some() && key == "escape" {
            self.close_menu();
            cx.notify();
            return true;
        }
        let plain = !m.platform && !m.control && !m.alt && !m.shift;
        // Comparing: Esc, ← →, S.
        if plain && compare_view::key(self, key) {
            self.after_model_update(cx);
            cx.notify();
            return true;
        }
        // Cropping: Return applies, Esc cancels.
        if self.model.cropping() {
            if let Some(apply) = crop_view::key(key, false).filter(|_| plain) {
                self.model.end_crop(apply);
                self.after_model_update(cx);
                cx.notify();
                return true;
            }
        }
        if key == "escape" && plain {
            if self.model.cancel_negative_pick() {
            } else if self.canvas.image_only {
                self.canvas.image_only = false;
            } else {
                return false;
            }
            self.after_model_update(cx);
            cx.notify();
            return true;
        }
        // Hold T: the photograph upside down, to judge its balance.
        if key == "t" && plain && !self.grid.covers() {
            if !event.is_held && self.model.image.is_some() && !self.model.cropping() && !self.canvas.upside_down {
                self.canvas.upside_down = true;
                cx.notify();
            }
            return true;
        }
        if grid::key_down(self, event) {
            self.after_model_update(cx);
            cx.notify();
            return true;
        }
        if self.grid.shown && self.grid.flying() {
            return false;
        }
        let command = if cfg!(target_os = "macos") { m.platform } else { m.control };
        match key {
            "left" | "right" if !self.grid.shown && !command && !m.alt => {
                strip::open_neighbour(self, if key == "left" { -1 } else { 1 });
            }
            "backspace" | "delete" if self.model.shelf.library.is_some() => {
                // ⌘⌫ on the Mac, Shift+Delete on Windows: without asking
                // (unless it can't go to the Trash).
                let now = if cfg!(target_os = "macos") { m.platform } else { m.shift };
                self.request_delete(None, !now, window, cx);
            }
            _ => return false,
        }
        self.after_model_update(cx);
        cx.notify();
        true
    }
}

impl gpui::Render for NegsApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let render_started = Instant::now();
        // Update native window title when photo changes.
        let win_title = match self.model.compare_title().map(Into::into).or_else(|| self.model.source.as_deref().and_then(Path::file_name).map(|n| n.to_os_string())) {
            Some(name) => format!("{} — Negs", name.to_string_lossy()),
            None => "Negs".to_string(),
        };
        window.set_window_title(&win_title);

        if let Some(mut bench) = self.bench.take() {
            bench.frame_start();
            bench.tick(self, window, cx);
            self.bench = Some(bench);
        }
        thumbs::flush_drops(window);
        tooltip::set_quiet(self.inspector.active_slider_drag.is_some());
        self.windows_chrome(window, cx);
        if std::mem::take(&mut self.resume_offer) && self.bench.is_none() {
            batch_view::offer_resume(self, window, cx);
        }
        self.sprites.begin_frame();
        let titlebar_el = render_titlebar(self, window, cx).into_any_element();
        // The grid first: a flight that ends shows the photograph view again.
        let grid_el = grid::render_grid(self, window, cx);
        // Under the open grid the photograph view isn't drawn (its size is
        // kept, so the strip under it stays laid out for the fold).
        let covered = self.grid.covers();
        self.canvas.hide_photo = self.grid.flight.as_ref().is_some_and(|f| f.canvas_flyer.is_some());
        compare_view::sync(self);
        let canvas_el = if covered {
            div().flex_1().w_full().min_h_0().bg(self.theme.canvas).into_any_element()
        } else {
            render_canvas(self, window, cx).into_any_element()
        };
        let tray_el = render_tray(self, window, cx);
        let strip_el = strip::render_strip(self, window, cx);
        let image_only = self.canvas.image_only && !covered;
        let inspector_el = if covered && self.model.inspector_visible {
            Some(div().w(px(inspector::INSPECTOR_WIDTH)).h_full().flex_shrink_0().bg(self.theme.panel).into_any_element())
        } else if self.model.cropping() {
            crop_view::render_panel(self, window, cx)
        } else if image_only {
            None
        } else {
            render_inspector_column(self, window, cx)
        };
        let strip_el = if image_only { None } else { strip_el };
        // Comparing: "Editing ③ the left one" over the panel.
        let inspector_el = match (inspector_el, compare_view::note_words(self)) {
            (Some(panel), Some(words)) if !covered => Some(compare_note(self, words, panel, cx)),
            (panel, _) => panel,
        };
        let menu_el = library_menu::render_menu(self, window, cx);
        let card_el = export_card::render(self, window, cx);
        let menu_scrim = menu_bar::render_scrim(self, cx);
        let batch_el = batch_view::render_popover(self, cx);
        self.stamp_startup("inspector_shown");
        let show_status = self.model.tray_shown().is_none() && !image_only;
        let status_el = show_status.then(|| render_status_bar(self, cx).into_any_element());
        let theme_menu = render_theme_menu(self, cx);
        if let Some(bench) = self.bench.as_mut() {
            bench.frame_end();
        }
        if trace_ui::on() {
            trace_ui::record("render", render_started);
        }

        div()
            .id("negs-app-root")
            .track_focus(&self.focus_handle)
            .key_context(if self.export_card.is_some() {
                "NegsApp Modal"
            } else if self.menu_bar.is_some() {
                "NegsApp MenuBar"
            } else {
                "NegsApp"
            })
            // The menu bar's keys come first (off the Mac): Alt, F10, Alt+letter, and all keys while it has focus.
            .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if menu_bar::key(this, event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_modifiers_changed(cx.listener(|this, event: &gpui::ModifiersChangedEvent, window, cx| {
                menu_bar::modifiers_changed(this, &event.modifiers, window, cx);
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.tray.handle_key_down(event, &mut this.model, window, cx) {
                    this.after_model_update(cx);
                    return;
                }
                if this
                    .presets
                    .handle_key_down(event, &mut this.model, window, cx)
                {
                    this.after_model_update(cx);
                    return;
                }
                if this.root_key_down(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _window, cx| {
                if event.keystroke.key == "t" && this.canvas.upside_down {
                    this.canvas.upside_down = false;
                    cx.notify();
                }
            }))
            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                if this.inspector.has_active_drag() {
                    let moved = Instant::now();
                    if handle_root_mouse_move(this, event, window, cx) && trace_ui::on() {
                        trace_ui::record("move", moved);
                    }
                }
                if canvas::crop_drag_move(this, event.position, cx) {
                    return;
                }
                if event.pressed_button == Some(MouseButton::Left) {
                    let p = negs_app::crop::Point { x: f32::from(event.position.x) as f64, y: f32::from(event.position.y) as f64 };
                    if compare_view::mouse_drag(this, p) {
                        this.after_model_update(cx);
                        cx.notify();
                        return;
                    }
                }
                if let Some((l, w)) = this.crop_panel.slider
                    && event.pressed_button == Some(MouseButton::Left)
                {
                    if let Some(s) = this.model.crop_session.as_mut() {
                        s.straighten(crop_view::slider_angle(f32::from(event.position.x), l, w));
                    }
                    this.model.crop_changed();
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if this.inspector.has_active_drag() {
                        handle_root_mouse_up(this, window, cx);
                    }
                    this.end_crop_drags(event.position, cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event: &MouseUpEvent, window, cx| {
                    if this.inspector.has_active_drag() {
                        handle_root_mouse_up(this, window, cx);
                    }
                    this.end_crop_drags(event.position, cx);
                }),
            )
            .on_action(cx.listener(|this, _: &ToggleCrop, _window, cx| {
                if this.model.cropping() {
                    this.model.end_crop(true);
                } else {
                    this.model.begin_crop();
                }
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &RotatePhoto, _window, cx| {
                // As on the Mac: rotating starts cropping, where the turn is kept or undone.
                if this.model.image.is_some() {
                    if !this.model.cropping() {
                        this.model.begin_crop();
                    }
                    this.model.rotate();
                    this.after_model_update(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &FlipPhoto, _window, cx| {
                if this.model.image.is_some() {
                    this.model.flip();
                    this.after_model_update(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &SheetTheme, _window, cx| this.choose_sheet(sheet::ViewerSheet::Theme, cx)))
            .on_action(cx.listener(|this, _: &SheetDarkroom, _window, cx| this.choose_sheet(sheet::ViewerSheet::Darkroom, cx)))
            .on_action(cx.listener(|this, _: &SheetBlack, _window, cx| this.choose_sheet(sheet::ViewerSheet::Black, cx)))
            .on_action(cx.listener(|this, _: &SheetCharcoal, _window, cx| this.choose_sheet(sheet::ViewerSheet::Charcoal, cx)))
            .on_action(cx.listener(|this, _: &SheetGrey22, _window, cx| this.choose_sheet(sheet::ViewerSheet::Grey22, cx)))
            .on_action(cx.listener(|this, _: &SheetMiddleGrey, _window, cx| this.choose_sheet(sheet::ViewerSheet::MiddleGrey, cx)))
            .on_action(cx.listener(|this, _: &SheetLightGrey, _window, cx| this.choose_sheet(sheet::ViewerSheet::LightGrey, cx)))
            .on_action(cx.listener(|this, _: &SheetPaper, _window, cx| this.choose_sheet(sheet::ViewerSheet::Paper, cx)))
            .on_action(cx.listener(|this, _: &ThemeDarkroom, _window, cx| this.set_theme(ThemeName::Darkroom, cx)))
            .on_action(cx.listener(|this, _: &ThemeContactSheet, _window, cx| this.set_theme(ThemeName::ContactSheet, cx)))
            .on_action(cx.listener(|this, _: &ThemeLightTable, _window, cx| this.set_theme(ThemeName::LightTable, cx)))
            .on_action(cx.listener(|this, _: &ThemeDaylight, _window, cx| this.set_theme(ThemeName::Daylight, cx)))
            .on_action(cx.listener(|this, _: &ThemePlain, _window, cx| this.set_theme(ThemeName::Plain, cx)))
            .on_action(cx.listener(|this, _: &ThemePlainDark, _window, cx| this.set_theme(ThemeName::PlainDark, cx)))
            .on_action(cx.listener(|this, _: &ToggleCompare, _window, cx| {
                compare_view::toggle(this);
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ZoomActual, _window, cx| {
                if this.model.source.is_some() && !this.model.cropping() {
                    this.model.zoom(false);
                    this.after_model_update(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ToggleImageOnly, _window, cx| {
                this.canvas.image_only = !this.canvas.image_only && this.model.source.is_some() && !this.model.cropping();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &OpenPhoto, window, cx| {
                this.prompt_open_photo(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SavePreview, _window, cx| {
                export_card::open(this, false, cx);
            }))
            .on_action(cx.listener(|this, _: &ExportPhoto, _window, cx| {
                export_card::open(this, true, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenSettings, _window, cx| {
                settings_window::open(this, cx);
            }))
            .on_action(cx.listener(|this, _: &AboutNegs, window, cx| {
                this.about(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ExportLut, _window, cx| {
                export_card::export_lut(this, cx);
            }))
            .on_action(cx.listener(|this, _: &UndoAction, _window, cx| {
                this.close_menu();
                this.model.perform_undo();
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &RedoAction, _window, cx| {
                this.model.perform_redo();
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleOriginal, _window, cx| {
                this.toggle_before_after(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSplit, _window, cx| {
                this.toggle_split_mode(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleZoomFit, _window, cx| {
                this.toggle_zoom_fit(cx);
            }))
            .on_action(cx.listener(|this, _: &ZoomIn, _window, cx| {
                if this.grid.covers() {
                    grid::set_size(this, this.model.prefs.grid_size + negs_app::sheet_rules::GRID_SIZE_STEP);
                    cx.notify();
                } else if this.model.source.is_some() && !this.model.cropping() {
                    this.canvas.zoom_by(1.25);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _window, cx| {
                if this.grid.covers() {
                    grid::set_size(this, this.model.prefs.grid_size - negs_app::sheet_rules::GRID_SIZE_STEP);
                    cx.notify();
                } else if this.model.source.is_some() && !this.model.cropping() {
                    this.canvas.zoom_by(0.8);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomFit, _window, cx| {
                if this.model.source.is_some() && !this.model.cropping() {
                    this.model.zoom(true);
                    this.after_model_update(cx);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Rate0, _window, cx| {
                this.library_key(LibraryKey::Rate(0), cx);
            }))
            .on_action(cx.listener(|this, _: &Rate1, _window, cx| {
                this.library_key(LibraryKey::Rate(1), cx);
            }))
            .on_action(cx.listener(|this, _: &Rate2, _window, cx| {
                this.library_key(LibraryKey::Rate(2), cx);
            }))
            .on_action(cx.listener(|this, _: &Rate3, _window, cx| {
                this.library_key(LibraryKey::Rate(3), cx);
            }))
            .on_action(cx.listener(|this, _: &Rate4, _window, cx| {
                this.library_key(LibraryKey::Rate(4), cx);
            }))
            .on_action(cx.listener(|this, _: &Rate5, _window, cx| {
                this.library_key(LibraryKey::Rate(5), cx);
            }))
            .on_action(cx.listener(|this, _: &FlagPick, _window, cx| {
                this.library_key(LibraryKey::Pick, cx);
            }))
            .on_action(cx.listener(|this, _: &FlagReject, _window, cx| {
                this.library_key(LibraryKey::Reject, cx);
            }))
            .on_action(cx.listener(|this, _: &FlagUnflag, _window, cx| {
                this.library_key(LibraryKey::Unflag, cx);
            }))
            .on_action(cx.listener(|this, _: &CycleTheme, _window, cx| {
                this.cycle_theme(cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleInspector, _window, cx| {
                this.toggle_inspector(cx);
            }))
            .on_action(cx.listener(|this, _: &ShowAdjustments, _window, cx| {
                this.toggle_pane(Pane::Adjustments, cx);
            }))
            .on_action(cx.listener(|this, _: &ShowPresets, _window, cx| {
                this.toggle_pane(Pane::Presets, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleGrid, window, cx| {
                this.toggle_grid(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleFilmstrip, _window, cx| {
                this.toggle_filmstrip(cx);
            }))
            .on_action(cx.listener(|this, _: &CopyEdits, window, cx| {
                this.run_command(library_menu::Command::CopyEdits, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PasteEdits, window, cx| {
                this.run_command(library_menu::Command::PasteEdits, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &PastePrevious, window, cx| {
                this.run_command(library_menu::Command::PastePrevious, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &DuplicatePhoto, window, cx| {
                this.run_command(library_menu::Command::Duplicate, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ResetEdits, window, cx| {
                this.run_command(library_menu::Command::ResetEdits, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TrashPhoto, window, cx| {
                this.run_command(library_menu::Command::Trash, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &TrashRejected, window, cx| {
                this.run_command(library_menu::Command::TrashRejected, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleAdvance, window, cx| {
                this.run_command(library_menu::Command::Advance, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &ClearFilter, window, cx| {
                this.run_command(library_menu::Command::ClearFilter, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &RaiseRating, window, cx| {
                this.run_command(library_menu::Command::Raise, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &LowerRating, window, cx| {
                this.run_command(library_menu::Command::Lower, None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenInLightroom, window, cx| {
                this.run_command(library_menu::Command::Lightroom(negs_app::lightroom::Kind::Cloud), None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenInLightroomClassic, window, cx| {
                this.run_command(library_menu::Command::Lightroom(negs_app::lightroom::Kind::Classic), None, window, cx);
            }))
            .on_action(cx.listener(|this, _: &Recent0, _window, cx| this.open_recent(0, cx)))
            .on_action(cx.listener(|this, _: &Recent1, _window, cx| this.open_recent(1, cx)))
            .on_action(cx.listener(|this, _: &Recent2, _window, cx| this.open_recent(2, cx)))
            .on_action(cx.listener(|this, _: &Recent3, _window, cx| this.open_recent(3, cx)))
            .on_action(cx.listener(|this, _: &Recent4, _window, cx| this.open_recent(4, cx)))
            .on_action(cx.listener(|this, _: &Recent5, _window, cx| this.open_recent(5, cx)))
            .on_action(cx.listener(|this, _: &Recent6, _window, cx| this.open_recent(6, cx)))
            .on_action(cx.listener(|this, _: &Recent7, _window, cx| this.open_recent(7, cx)))
            .on_action(cx.listener(|this, _: &Recent8, _window, cx| this.open_recent(8, cx)))
            .on_action(cx.listener(|this, _: &Recent9, _window, cx| this.open_recent(9, cx)))
            .on_action(cx.listener(|this, _: &NewPreset, _window, cx| {
                if !this.model.can_change_presets() {
                    return;
                }
                if !(this.model.inspector_visible && this.model.pane() == Pane::Presets) {
                    this.toggle_pane(Pane::Presets, cx);
                }
                this.model.new_preset();
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|_this, _: &ImportPresets, window, cx| {
                presets::PresetsState::prompt_import_presets(window, cx);
            }))
            .on_action(cx.listener(|_this, _: &ShowPresetsFolder, _window, _cx| {
                presets::PresetsState::show_presets_folder(None);
            }))
            .on_action(cx.listener(|_this, _: &ToggleFullScreen, window, _cx| {
                window.toggle_fullscreen();
            }))
            .on_action(cx.listener(|_this, _: &CheckForUpdates, _window, _cx| {
                updates::check(true);
            }))
            .on_action(cx.listener(|this, _: &ToggleAllControls, _window, cx| {
                this.model.prefs.lab_all_controls = !this.model.prefs.lab_all_controls;
                this.model.prefs.save();
                this.model.dirty |= negs_app::model::dirty::STRUCTURE | negs_app::model::dirty::INSPECTOR | negs_app::model::dirty::CHROME;
                this.after_model_update(cx);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &QuitApp, window, cx| {
                this.request_quit(window, cx);
            }))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(self.theme.canvas)
            .text_color(self.theme.paper)
            .child(titlebar_el)
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_col()
                            .child(canvas_el)
                            .when_some(strip_el, |el, strip| el.child(strip))
                            .when_some(tray_el, |el, tray| el.child(tray)),
                    )
                    .when_some(inspector_el, |el, col| el.child(col)),
            )
            .when_some(status_el, |el, status| el.child(status))
            .when_some(grid_el, |el, grid| el.child(grid))
            .when_some(theme_menu, |el, menu| el.child(menu))
            .when_some(menu_el, |el, menu| el.child(menu))
            .when_some(menu_scrim, |el, scrim| el.child(scrim))
            .when_some(batch_el, |el, popover| el.child(popover))
            .when_some(card_el, |el, card| el.child(card))
    }
}

/// The panel under Compare's note (`CompareEditingNote`): "Editing ③ the
/// left one", or against the original, "The original can't be edited." and
/// Edit 3.
fn compare_note(app: &NegsApp, (original, number, place): (bool, String, String), panel: AnyElement, cx: &mut Context<NegsApp>) -> AnyElement {
    let theme = &app.theme;
    let note = div()
        .flex()
        .items_center()
        .justify_between()
        .h(px(36.0))
        .px(px(22.0))
        .bg(theme.panel)
        .border_b_1()
        .border_color(theme.line)
        .when(original, |d| {
            d.child(div().font_family(theme.serif_family.clone()).italic().text_size(px(14.0)).text_color(theme.label).child("The original can\u{2019}t be edited."))
                .child(
                    div()
                        .id("compare-edit")
                        .cursor_pointer()
                        .text_size(px(12.0))
                        .text_color(theme.label)
                        .hover(|s| s.text_color(theme.paper))
                        .on_click(cx.listener(|this, _, _w, cx| {
                            if let Some(first) = this.model.comparison().map(|c| !c.active_first) {
                                this.model.activate_compared(first);
                            }
                            this.after_model_update(cx);
                            cx.notify();
                        }))
                        .child(format!("Edit {number}")),
                )
        })
        .when(!original, |d| {
            d.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(div().text_size(px(12.0)).text_color(theme.dim).child("Editing"))
                    .child(
                        div()
                            .min_w(px(28.0))
                            .h(px(24.0))
                            .px(px(6.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(12.0))
                            .border_2()
                            .border_color(theme.accent)
                            .font_family(theme.serif_family.clone())
                            .font_weight(gpui::FontWeight(600.0))
                            .text_size(px(14.0))
                            .text_color(theme.paper)
                            .child(number),
                    )
                    .child(div().font_family(theme.serif_family.clone()).italic().text_size(px(14.0)).text_color(theme.label).child(place)),
            )
        });
    div().flex().flex_col().h_full().flex_shrink_0().child(note).child(div().flex_1().min_h_0().child(panel)).into_any_element()
}

fn bind_shortcuts(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    let non_text = Some("NegsApp && !TextInput && !Modal && !MenuBar");
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-o"), OpenPhoto, None),
        KeyBinding::new(&format!("{modifier}-s"), SavePreview, None),
        KeyBinding::new(&format!("{modifier}-shift-e"), ExportPhoto, None),
        KeyBinding::new(&format!("{modifier}-shift-l"), ExportLut, None),
        KeyBinding::new(&format!("{modifier}-,"), OpenSettings, None),
        KeyBinding::new(&format!("{modifier}-shift-c"), CopyEdits, non_text),
        KeyBinding::new(&format!("{modifier}-shift-v"), PasteEdits, non_text),
        KeyBinding::new(&format!("{modifier}-d"), DuplicatePhoto, non_text),
        KeyBinding::new(&format!("{modifier}-shift-r"), ResetEdits, non_text),
        KeyBinding::new("]", RaiseRating, non_text),
        KeyBinding::new(&format!("{modifier}-shift-o"), ImportPresets, non_text),
        KeyBinding::new(if cfg!(target_os = "macos") { "ctrl-cmd-f" } else { "f11" }, ToggleFullScreen, None),
        KeyBinding::new("[", LowerRating, non_text),
        KeyBinding::new(&format!("{modifier}-z"), UndoAction, None),
        KeyBinding::new(&format!("{modifier}-shift-z"), RedoAction, None),
        // Windows' own Redo key.
        KeyBinding::new("ctrl-y", RedoAction, if cfg!(target_os = "macos") { Some("Never") } else { None }),
        KeyBinding::new("\\", ToggleOriginal, non_text),
        KeyBinding::new("shift-\\", ToggleSplit, non_text),
        KeyBinding::new("|", ToggleSplit, non_text),
        KeyBinding::new("z", ToggleZoomFit, non_text),
        KeyBinding::new(&format!("{modifier}-="), ZoomIn, None),
        KeyBinding::new(&format!("{modifier}-+"), ZoomIn, None),
        KeyBinding::new(&format!("{modifier}--"), ZoomOut, None),
        KeyBinding::new(&format!("{modifier}-0"), ZoomFit, None),
        KeyBinding::new("0", Rate0, non_text),
        KeyBinding::new("1", Rate1, non_text),
        KeyBinding::new("2", Rate2, non_text),
        KeyBinding::new("3", Rate3, non_text),
        KeyBinding::new("4", Rate4, non_text),
        KeyBinding::new("5", Rate5, non_text),
        KeyBinding::new("p", FlagPick, non_text),
        KeyBinding::new("x", FlagReject, non_text),
        KeyBinding::new("u", FlagUnflag, non_text),
        KeyBinding::new(&format!("{modifier}-k"), ToggleCrop, None),
        KeyBinding::new(&format!("{modifier}-r"), RotatePhoto, None),
        KeyBinding::new("f", ToggleImageOnly, non_text),
        KeyBinding::new("c", ToggleCompare, non_text),
        KeyBinding::new(&format!("{modifier}-1"), ZoomActual, None),
        KeyBinding::new("i", ToggleInspector, non_text),
        KeyBinding::new("g", ToggleGrid, non_text),
        KeyBinding::new("f6", ToggleFilmstrip, None),
        KeyBinding::new("f7", ShowAdjustments, None),
        KeyBinding::new("f8", ShowPresets, None),
        KeyBinding::new(&format!("{modifier}-q"), QuitApp, None),
    ]);
}

fn install_menus(cx: &mut App) {
    cx.set_menus(menus(&[], &[]));
}

/// The menus: the Mac's menu bar, and the in-window menu bar elsewhere
/// (`menu_bar.rs`), in the Windows app's order (`build_menu`).
fn menus(lightroom: &[negs_app::lightroom::Kind], recent: &[PathBuf]) -> Vec<Menu> {
    let mac = cfg!(target_os = "macos");
    let trash = if mac { "Move to Trash…" } else if cfg!(target_os = "windows") { "&Move to Recycle Bin…" } else { "&Move to Trash…" };
    let recent_actions: [Box<dyn gpui::Action>; 10] = [Box::new(Recent0), Box::new(Recent1), Box::new(Recent2), Box::new(Recent3), Box::new(Recent4), Box::new(Recent5), Box::new(Recent6), Box::new(Recent7), Box::new(Recent8), Box::new(Recent9)];
    let mut file = vec![MenuItem::action("&Open…", OpenPhoto)];
    if !recent.is_empty() {
        let items = recent
            .iter()
            .zip(recent_actions)
            .map(|(path, action)| MenuItem::Action { name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().into(), action, os_action: None })
            .collect();
        file.push(MenuItem::submenu(Menu { name: "Open &Recent".into(), items }));
    }
    file.push(MenuItem::separator());
    file.extend([
        MenuItem::action("&Save Preview…", SavePreview),
        MenuItem::action("&Export Photograph…", ExportPhoto),
        MenuItem::action("Export &LUT…", ExportLut),
    ]);
    // Open in Lightroom, Open in Lightroom Classic: only those installed.
    for kind in lightroom {
        let title = format!("Open in {}", kind.title());
        file.push(match kind {
            negs_app::lightroom::Kind::Cloud => MenuItem::action(title, OpenInLightroom),
            negs_app::lightroom::Kind::Classic => MenuItem::action(title, OpenInLightroomClassic),
        });
    }
    file.extend([MenuItem::separator(), MenuItem::action("&New Preset", NewPreset), MenuItem::action("&Import Presets…", ImportPresets), MenuItem::action("Show Presets &Folder", ShowPresetsFolder)]);
    if !mac {
        file.extend([MenuItem::separator(), MenuItem::action("Se&ttings…", OpenSettings), MenuItem::separator(), MenuItem::action("E&xit", QuitApp)]);
    }
    let mut out = Vec::new();
    if mac {
        out.push(Menu {
            name: "Negs".into(),
            items: vec![
                MenuItem::action("About Negs", AboutNegs),
                MenuItem::separator(),
                MenuItem::action("Settings…", OpenSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Negs", QuitApp),
            ],
        });
    }
    out.push(Menu { name: "&File".into(), items: file });
    out.push(Menu { name: "&Edit".into(), items: vec![MenuItem::action("&Undo", UndoAction), MenuItem::action("&Redo", RedoAction)] });
    out.push(Menu {
        name: "&Photo".into(),
        items: vec![
            MenuItem::action("Show &Original", ToggleOriginal),
            MenuItem::separator(),
            MenuItem::submenu(Menu {
                name: "R&ating".into(),
                items: vec![
                    MenuItem::action("&None", Rate0),
                    MenuItem::separator(),
                    MenuItem::action("★", Rate1),
                    MenuItem::action("★★", Rate2),
                    MenuItem::action("★★★", Rate3),
                    MenuItem::action("★★★★", Rate4),
                    MenuItem::action("★★★★★", Rate5),
                    MenuItem::separator(),
                    MenuItem::action("&Raise Rating", RaiseRating),
                    MenuItem::action("&Lower Rating", LowerRating),
                ],
            }),
            MenuItem::submenu(Menu {
                name: "&Flag".into(),
                items: vec![MenuItem::action("&Pick", FlagPick), MenuItem::action("&Reject", FlagReject), MenuItem::action("&Unflagged", FlagUnflag)],
            }),
            MenuItem::separator(),
            MenuItem::action("&Copy Edits", CopyEdits),
            MenuItem::action("&Paste Edits", PasteEdits),
            MenuItem::action("Paste from Pre&vious", PastePrevious),
            MenuItem::separator(),
            MenuItem::action("&Duplicate", DuplicatePhoto),
            MenuItem::action("&Reset Edits", ResetEdits),
            MenuItem::separator(),
            MenuItem::action(trash, TrashPhoto),
            MenuItem::action("Delete Re&jected Photographs…", TrashRejected),
            MenuItem::separator(),
            MenuItem::action("Advance After Ra&ting", ToggleAdvance),
            MenuItem::separator(),
            MenuItem::submenu(Menu { name: menu_bar::FILTER.into(), items: vec![] }),
            MenuItem::action("C&lear Filter", ClearFilter),
        ],
    });
    out.push(Menu {
        name: "&View".into(),
        items: vec![
            MenuItem::action("&Film Strip", ToggleFilmstrip),
            MenuItem::action("&Adjustments", ShowAdjustments),
            MenuItem::action("&Presets", ShowPresets),
            MenuItem::action("I&nspector", ToggleInspector),
            MenuItem::action("&Image Only", ToggleImageOnly),
            MenuItem::action("F&ull Screen", ToggleFullScreen),
            MenuItem::action("&Grid", ToggleGrid),
            MenuItem::action("C&ompare", ToggleCompare),
            MenuItem::action("S&plit Before/After", ToggleSplit),
            MenuItem::action("Show A&ll Controls", ToggleAllControls),
            MenuItem::separator(),
            MenuItem::action("Fi&t Photograph", ZoomFit),
            MenuItem::action("Actual &Size", ZoomActual),
            MenuItem::action("Toggle Fit / 100%", ToggleZoomFit),
            MenuItem::action("Zoom &In", ZoomIn),
            MenuItem::action("Zoom &Out", ZoomOut),
            MenuItem::separator(),
            MenuItem::action("&Rotate Clockwise", RotatePhoto),
            MenuItem::action("Flip &Horizontal", FlipPhoto),
            MenuItem::action("&Crop", ToggleCrop),
            MenuItem::separator(),
            MenuItem::submenu(Menu {
                name: "T&heme".into(),
                items: vec![
                    MenuItem::action("&Darkroom", ThemeDarkroom),
                    MenuItem::action("&Contact Sheet", ThemeContactSheet),
                    MenuItem::action("&Light Table", ThemeLightTable),
                    MenuItem::action("Da&ylight", ThemeDaylight),
                    MenuItem::action("&Plain", ThemePlain),
                    MenuItem::action("Plain Dar&k", ThemePlainDark),
                ],
            }),
            MenuItem::submenu(Menu {
                name: "&Viewer Sheet".into(),
                items: vec![
                    MenuItem::action("&Theme’s Own", SheetTheme),
                    MenuItem::action("&Darkroom", SheetDarkroom),
                    MenuItem::action("&Black", SheetBlack),
                    MenuItem::action("C&harcoal", SheetCharcoal),
                    MenuItem::action("&22% Grey", SheetGrey22),
                    MenuItem::action("&Middle Grey", SheetMiddleGrey),
                    MenuItem::action("&Light Grey", SheetLightGrey),
                    MenuItem::action("&Paper", SheetPaper),
                ],
            }),
        ],
    });
    if !mac {
        out.push(Menu { name: "&Help".into(), items: vec![MenuItem::action("Check for &Updates…", CheckForUpdates), MenuItem::separator(), MenuItem::action("&About Negs", AboutNegs)] });
    }
    if mac {
        // The Mac's menus have no access keys.
        for menu in &mut out {
            strip_access_keys(menu);
        }
    }
    out
}

fn strip_access_keys(menu: &mut Menu) {
    menu.name = menu_bar::plain(&menu.name).0.into();
    for item in &mut menu.items {
        match item {
            MenuItem::Action { name, .. } => *name = menu_bar::plain(name).0.into(),
            MenuItem::Submenu(sub) => strip_access_keys(sub),
            _ => {}
        }
    }
}

fn cli_photo_arg() -> Option<PathBuf> {
    std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .find(|p| !p.to_string_lossy().starts_with('-'))
}

fn main() {
    let startup = StartupReport::new();
    let cli_path = cli_photo_arg();
    win::hold_lifetime_mutex();
    // One Negs: a second hands its photograph to the first and goes.
    if std::env::var_os("NEGS_ALLOW_MULTIPLE").is_none() {
        let absolute = cli_path.as_ref().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.clone()));
        if win::forward_to_running(absolute.as_deref()) {
            return;
        }
    }
    win::set_app_id();
    let (gpui_host, channels) = GpuiHost::new();
    host::register(gpui_host.clone());

    Application::new().run(move |cx: &mut App| {
        if let Err(err) = register_fonts(cx) {
            eprintln!("warning: could not register embedded Newsreader fonts: {err:#}");
        }
        bind_shortcuts(cx);
        install_menus(cx);
        cx.activate(true);

        // Where it was last time, if that's still on a display.
        let saved = Preferences::load();
        let displays: Vec<_> = cx.displays().iter().map(|d| d.bounds()).collect();
        let bounds = place::from_prefs(saved.window, saved.window_pixels, &displays)
            .unwrap_or_else(|| WindowBounds::centered(size(px(1280.0), px(840.0)), cx));
        let options = WindowOptions {
            window_bounds: Some(bounds),
            window_min_size: Some(size(px(800.0), px(560.0))),
            titlebar: Some(TitlebarOptions {
                title: Some(SharedString::from("Negs")),
                appears_transparent: true,
                traffic_light_position: Some(Point::new(px(14.0), px(13.0))),
            }),
            app_id: Some("negs".to_string()),
            ..Default::default()
        };

        let host_for_win = gpui_host.clone();
        let window_handle = cx
            .open_window(options, move |window, cx| {
                let view = cx.new(|cx| {
                    NegsApp::new(host_for_win, channels, cli_path, startup, window, cx)
                });
                let view_for_focus = view.clone();
                window.focus(&view_for_focus.read(cx).focus_handle(cx));

                let view_for_close = view.clone();
                window.on_window_should_close(cx, move |window, cx| {
                    view_for_close.update(cx, |app, cx| {
                        if app.quit_needs_asking() {
                            app.request_quit(window, cx);
                            false
                        } else {
                            app.shutdown_cleanly();
                            true
                        }
                    })
                });

                view
            })
            .expect("failed to open Negs GPUI window");

        let _ = window_handle;
    });
}
