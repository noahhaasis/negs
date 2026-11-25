//! Negs — cross-platform GPUI desktop application.
//!
//! Phase 2: GPUI shell, Newsreader typography, 6 themes, `Host` bridge to
//! `negs_app::model::Model`, exact-fit Lanczos + monitor ICC / macOS Display P3
//! photo canvas, vector `negs-marks` overlay, native Open/Save dialogs,
//! drag-and-drop, keyboard shortcuts, and `NEGS_STARTUP_REPORT` instrumentation.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod canvas;
mod color_display;
mod host_gpui;
mod marks_gpui;
mod status;
mod theme_gpui;
mod titlebar;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, AppContext, Application, Context, FocusHandle, Focusable, IntoElement, KeyBinding, Menu,
    MenuItem, PathPromptOptions, Point, SharedString, TitlebarOptions, Window, WindowBounds,
    WindowOptions, actions, div, prelude::*, px, size,
};
use negs_app::host;
use negs_app::model::{Edge, ExportSize, LibraryKey, Model, SaveSettings};
use negs_app::platform;
use negs_app::session::{Preferences, SessionStore};
use negs_app::theme::ThemeName;
use negs_library::Flag;

use crate::canvas::{CanvasState, render_canvas};
use crate::color_display::active_display_profile;
use crate::host_gpui::{GpuiHost, HostChannels};
use crate::status::render_status_bar;
use crate::theme_gpui::{GpuiTheme, initial_theme, next_theme, register_fonts};
use crate::titlebar::{render_theme_menu, render_titlebar};

actions!(
    negs,
    [
        OpenPhoto,
        SavePreview,
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
        QuitApp,
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

/// Root GPUI view owning the pure-Rust `Model`, canvas state, active theme, and host handle.
pub struct NegsApp {
    pub model: Model,
    pub host: Arc<GpuiHost>,
    pub theme: GpuiTheme,
    pub theme_menu_open: bool,
    pub canvas: CanvasState,
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
        let theme_name = initial_theme((!prefs.app_theme.is_empty()).then_some(prefs.app_theme.as_str()));
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
            this.refresh_display_profile(window, cx);
            cx.notify();
        })
        .detach();

        let mut app = Self {
            model,
            host,
            theme,
            theme_menu_open: false,
            canvas: CanvasState::with_display_profile(display_profile),
            startup,
            focus_handle,
            last_zoom_command: 0,
            last_source: None,
            shut_down: false,
        };
        app.startup.stamp("gpu_ready");
        app.startup.stamp("inspector_shown");
        app
    }

    pub fn stamp_startup(&mut self, name: &'static str) {
        self.startup.stamp(name);
    }

    /// Re-queries the window's active display ICC profile and invalidates cached textures on change.
    pub fn refresh_display_profile(&mut self, window: &Window, cx: &App) {
        let next = active_display_profile(window, cx);
        let changed = next.display_key != self.canvas.display_profile.display_key
            || next.label != self.canvas.display_profile.label;
        if changed {
            self.canvas.display_profile = next.clone();
            self.canvas.invalidate_images();
            self.model.set_preview_color(next.transform);
        }
    }

    fn after_model_update(&mut self, _cx: &mut Context<Self>) {
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
        }
        // Keep Original loaded whenever Before/After or Split mode is active.
        if (self.canvas.before_after || self.canvas.split_mode)
            && self.model.can_show_original()
            && !self.model.holding_original
        {
            self.model.hold_original(true);
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
        if self.model.source.is_none() {
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

    pub fn prompt_save_preview(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.model.can_save() {
            return;
        }
        let default_dir = self
            .model
            .source
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .unwrap_or_else(platform::pictures_dir);
        let suggested = self
            .model
            .source
            .as_deref()
            .and_then(Path::file_stem)
            .map(|s| format!("{}-negs.jpg", s.to_string_lossy()))
            .unwrap_or_else(|| "preview.jpg".to_string());

        let receiver = cx.prompt_for_new_path(&default_dir, Some(&suggested));
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(Some(destination))) = receiver.await {
                let _ = this.update(cx, |app, cx| {
                    let save = SaveSettings {
                        bit_depth: 8,
                        color_space: 0,
                        jpeg_quality: app.model.prefs.jpeg_quality.clamp(1, 100),
                    };
                    app.model.save_preview(
                        destination,
                        ExportSize {
                            edge: Edge::Full,
                            pixels: 2048,
                        },
                        save,
                    );
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn shutdown_cleanly(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        if let Some(session) = self.model.session() {
            let _ = SessionStore::default().save(&session);
        }
        self.model.shutdown();
    }
}

impl gpui::Render for NegsApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Update native window title when photo changes.
        let win_title = match self.model.source.as_deref().and_then(Path::file_name) {
            Some(name) => format!("{} — Negs", name.to_string_lossy()),
            None => "Negs".to_string(),
        };
        window.set_window_title(&win_title);

        div()
            .id("negs-app-root")
            .track_focus(&self.focus_handle)
            .key_context("NegsApp")
            .on_action(cx.listener(|this, _: &OpenPhoto, window, cx| {
                this.prompt_open_photo(window, cx);
            }))
            .on_action(cx.listener(|this, _: &SavePreview, window, cx| {
                this.prompt_save_preview(window, cx);
            }))
            .on_action(cx.listener(|this, _: &UndoAction, _window, cx| {
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
                if this.model.source.is_some() {
                    this.canvas.zoom_by(1.25);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomOut, _window, cx| {
                if this.model.source.is_some() {
                    this.canvas.zoom_by(0.8);
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &ZoomFit, _window, cx| {
                if this.model.source.is_some() {
                    this.canvas.reset_fit();
                    this.model.fit_image = true;
                    cx.notify();
                }
            }))
            .on_action(cx.listener(|this, _: &Rate0, _window, cx| {
                this.model.library_key(LibraryKey::Rate(0));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Rate1, _window, cx| {
                this.model.library_key(LibraryKey::Rate(1));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Rate2, _window, cx| {
                this.model.library_key(LibraryKey::Rate(2));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Rate3, _window, cx| {
                this.model.library_key(LibraryKey::Rate(3));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Rate4, _window, cx| {
                this.model.library_key(LibraryKey::Rate(4));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &Rate5, _window, cx| {
                this.model.library_key(LibraryKey::Rate(5));
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FlagPick, _window, cx| {
                this.model.library_key(LibraryKey::Pick);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FlagReject, _window, cx| {
                this.model.library_key(LibraryKey::Reject);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &FlagUnflag, _window, cx| {
                this.model.library_key(LibraryKey::Unflag);
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &CycleTheme, _window, cx| {
                this.cycle_theme(cx);
            }))
            .on_action(cx.listener(|this, _: &QuitApp, _window, cx| {
                this.shutdown_cleanly();
                cx.quit();
            }))
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .bg(self.theme.canvas)
            .text_color(self.theme.paper)
            .child(render_titlebar(self, window, cx))
            .child(render_canvas(self, window, cx))
            .child(render_status_bar(self, cx))
            .when_some(render_theme_menu(self, cx), |el, menu| el.child(menu))
    }
}

fn bind_shortcuts(cx: &mut App) {
    let modifier = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
    cx.bind_keys([
        KeyBinding::new(&format!("{modifier}-o"), OpenPhoto, None),
        KeyBinding::new(&format!("{modifier}-s"), SavePreview, None),
        KeyBinding::new(&format!("{modifier}-z"), UndoAction, None),
        KeyBinding::new(&format!("{modifier}-shift-z"), RedoAction, None),
        KeyBinding::new("\\", ToggleOriginal, None),
        KeyBinding::new("shift-\\", ToggleSplit, None),
        KeyBinding::new("|", ToggleSplit, None),
        KeyBinding::new("z", ToggleZoomFit, None),
        KeyBinding::new(&format!("{modifier}-="), ZoomIn, None),
        KeyBinding::new(&format!("{modifier}-+"), ZoomIn, None),
        KeyBinding::new(&format!("{modifier}--"), ZoomOut, None),
        KeyBinding::new(&format!("{modifier}-0"), ZoomFit, None),
        KeyBinding::new("0", Rate0, None),
        KeyBinding::new("1", Rate1, None),
        KeyBinding::new("2", Rate2, None),
        KeyBinding::new("3", Rate3, None),
        KeyBinding::new("4", Rate4, None),
        KeyBinding::new("5", Rate5, None),
        KeyBinding::new("p", FlagPick, None),
        KeyBinding::new("x", FlagReject, None),
        KeyBinding::new("u", FlagUnflag, None),
        KeyBinding::new("t", CycleTheme, None),
        KeyBinding::new(&format!("{modifier}-q"), QuitApp, None),
    ]);
}

fn install_menus(cx: &mut App) {
    cx.set_menus(vec![
        Menu {
            name: "Negs".into(),
            items: vec![MenuItem::action("Quit Negs", QuitApp)],
        },
        Menu {
            name: "File".into(),
            items: vec![
                MenuItem::action("Open…", OpenPhoto),
                MenuItem::action("Save Preview…", SavePreview),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::action("Undo", UndoAction),
                MenuItem::action("Redo", RedoAction),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Original", ToggleOriginal),
                MenuItem::action("Split Before/After", ToggleSplit),
                MenuItem::separator(),
                MenuItem::action("Toggle Fit / 100%", ToggleZoomFit),
                MenuItem::action("Zoom In", ZoomIn),
                MenuItem::action("Zoom Out", ZoomOut),
                MenuItem::action("Zoom to Fit", ZoomFit),
                MenuItem::separator(),
                MenuItem::action("Cycle Theme", CycleTheme),
            ],
        },
        Menu {
            name: "Photo".into(),
            items: vec![
                MenuItem::action("Pick", FlagPick),
                MenuItem::action("Reject", FlagReject),
                MenuItem::action("Unflagged", FlagUnflag),
                MenuItem::separator(),
                MenuItem::action("Rate 5 Stars", Rate5),
                MenuItem::action("Rate 4 Stars", Rate4),
                MenuItem::action("Rate 3 Stars", Rate3),
                MenuItem::action("Rate 2 Stars", Rate2),
                MenuItem::action("Rate 1 Star", Rate1),
                MenuItem::action("Clear Rating", Rate0),
            ],
        },
    ]);
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
    let (gpui_host, channels) = GpuiHost::new();
    host::register(gpui_host.clone());

    Application::new().run(move |cx: &mut App| {
        if let Err(err) = register_fonts(cx) {
            eprintln!("warning: could not register embedded Newsreader fonts: {err:#}");
        }
        bind_shortcuts(cx);
        install_menus(cx);
        cx.activate(true);

        let bounds = WindowBounds::centered(size(px(1280.0), px(840.0)), cx);
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
                window.on_window_should_close(cx, move |_window, cx| {
                    view_for_close.update(cx, |app, _cx| {
                        app.shutdown_cleanly();
                    });
                    true
                });

                view
            })
            .expect("failed to open Negs GPUI window");

        let _ = window_handle;
    });
}
