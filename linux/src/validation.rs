//! Opt-in visual/performance instrumentation. Never used for rendering or engine claims.
use crate::Editor;
use adw::prelude::*;
use gtk::glib;
use std::{cell::Cell, rc::Rc, time::Duration};

pub fn schedule(ui: &Rc<Editor>) {
    glib::timeout_add_local_once(
        Duration::from_millis(1100),
        glib::clone!(
            #[weak]
            ui,
            move || {
                if let Some(renderer) = ui.window.renderer() {
                    ui.perf.detail("renderer", renderer.type_().name());
                }
                ui.perf.detail("window_width", ui.window.width());
                ui.perf.detail("window_height", ui.window.height());
                ui.perf.detail(
                    "data_directory",
                    crate::data_root().to_string_lossy().to_string(),
                );
                ui.perf.detail(
                    "surface_scale",
                    ui.window.surface().map(|s| s.scale()).unwrap_or(1.),
                );
                if let Ok(scene) = std::env::var("NEGS_UI_SCENE") {
                    match scene.as_str() {
                        "post" => {
                            if let Some(tabs) = ui.tabs.borrow().as_ref() {
                                tabs.set_visible_child_name("post");
                            }
                            ui.history.borrow_mut().state.border.enabled = true;
                            ui.history.borrow_mut().state.border.style =
                                crate::border::Style::from_index(
                                    std::env::var("NEGS_BORDER_STYLE")
                                        .ok()
                                        .and_then(|s| s.parse().ok())
                                        .unwrap_or(2),
                                );
                            ui.sync_state();
                        }
                        "crop" => {
                            let _ = gtk::prelude::WidgetExt::activate_action(
                                &ui.window, "win.crop", None,
                            );
                        }
                        "export" => {
                            let _ = gtk::prelude::WidgetExt::activate_action(
                                &ui.window,
                                "win.export",
                                None,
                            );
                        }
                        _ => {}
                    }
                }
                if let Some(path) = std::env::var_os("NEGS_UI_AUDIT") {
                    control_audit(&ui, path.into());
                }
                if std::env::var_os("NEGS_UI_BENCHMARK").is_some() {
                    benchmark(&ui);
                }
                ui.perf.write();
            }
        ),
    );
    if let Some(path) = std::env::var_os("NEGS_WIDGET_CAPTURE") {
        glib::timeout_add_local_once(
            Duration::from_millis(2000),
            glib::clone!(
                #[weak]
                ui,
                move || {
                    let paintable = gtk::WidgetPaintable::new(Some(&ui.window));
                    let snapshot = gtk::Snapshot::new();
                    paintable.snapshot(
                        &snapshot,
                        ui.window.width() as f64,
                        ui.window.height() as f64,
                    );
                    if let Some(node) = snapshot.to_node()
                        && let Some(renderer) = ui.window.renderer()
                    {
                        let texture = renderer.render_texture(&node, None);
                        match texture.save_to_png(&path) {
                            Ok(()) => eprintln!("Widget capture: {}", path.to_string_lossy()),
                            Err(e) => eprintln!("Capture failed: {e}"),
                        }
                    }
                }
            ),
        );
    }
}

fn benchmark(ui: &Rc<Editor>) {
    let Some((_, adjustment)) = ui
        .adjustments
        .borrow()
        .iter()
        .find(|(key, _)| key == "Enlarger.Print exposure")
        .cloned()
    else {
        return;
    };
    ui.perf.mark("ui_benchmark_started_ms");
    ui.history.borrow_mut().begin("Benchmark slider drag");
    let frames = Rc::new(Cell::new(0u32));
    let origin = adjustment.value();
    ui.canvas.add_tick_callback(glib::clone!(#[weak] ui, #[upgrade_or] glib::ControlFlow::Break, move |_,_| {
        let n=frames.get();frames.set(n+1);
        adjustment.set_value(1.+0.8*(n as f64*0.06).sin());
        let center=(ui.canvas.width() as f64/2.,ui.canvas.height() as f64/2.);
        ui.canvas.zoom_at(if n.is_multiple_of(2){1.003}else{1./1.003},center);
        if n>=239 {
            adjustment.set_value(origin);ui.canvas.fit();ui.history.borrow_mut().commit();
            ui.perf.mark("ui_benchmark_finished_ms");ui.perf.detail("ui_benchmark_frames",240);
            ui.perf.detail("ui_benchmark_scope","240 frame-clock-driven GTK value updates plus texture zoom; no engine or physical pointer");
            ui.perf.write();return glib::ControlFlow::Break;
        }
        glib::ControlFlow::Continue
    }));
    ui.canvas.queue_draw();
}

fn widgets(root: &impl IsA<gtk::Widget>) -> Vec<gtk::Widget> {
    let mut result = Vec::new();
    let mut pending = vec![root.as_ref().clone()];
    while let Some(w) = pending.pop() {
        let mut child = w.first_child();
        while let Some(c) = child {
            child = c.next_sibling();
            pending.push(c);
        }
        result.push(w);
    }
    result
}
fn combo(ui: &Editor, title: &str) -> adw::ComboRow {
    widgets(&ui.inspector)
        .into_iter()
        .filter_map(|w| w.downcast::<adw::ComboRow>().ok())
        .find(|r| r.title() == title)
        .unwrap_or_else(|| panic!("missing {title}"))
}
fn activate(ui: &Editor, name: &str) {
    gtk::prelude::WidgetExt::activate_action(&ui.window, &format!("win.{name}"), None).unwrap();
}
fn control_audit(ui: &Rc<Editor>, path: std::path::PathBuf) {
    let initial = ui.history.borrow().state.clone();
    let mut checks = Vec::new();
    let films = crate::catalog::stocks(false);
    let film = combo(ui, "Film");
    let paper = combo(ui, "Paper");
    film.set_selected(films.iter().position(|s| s.id == "kodak_doublex").unwrap() as u32);
    assert_eq!(combo(ui, "Development").model().unwrap().n_items(), 6);
    assert!(combo(ui, "Development").is_visible());
    film.set_selected(
        films
            .iter()
            .position(|s| s.id == "kodak_trix400_d76_8min")
            .unwrap() as u32,
    );
    assert_eq!(
        crate::catalog::selected(&ui.history.borrow().state, true).id,
        "ilford_mgrc_deluxe_grade2"
    );
    film.set_selected(
        films
            .iter()
            .position(|s| s.id == "fujifilm_velvia_100")
            .unwrap() as u32,
    );
    assert!(crate::inspector::flag(
        &ui.history.borrow().state,
        "Scan film directly",
        false
    ));
    assert!(!paper.is_visible());
    checks.push("Complete profile menus pair stocks, expose real development times, and apply slide direct-scan visibility");
    combo(ui, "Develop at").set_selected(1);
    let entry = widgets(&ui.inspector)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Entry>().ok())
        .find(|e| e.input_purpose() == gtk::InputPurpose::Digits)
        .unwrap();
    entry.set_text("999999");
    entry.emit_activate();
    assert!(entry.has_css_class("error"));
    assert!(
        !ui.history
            .borrow()
            .state
            .values
            .contains_key("Image Size.Longest side")
    );
    entry.set_text("3000");
    entry.emit_activate();
    assert!(!entry.has_css_class("error"));
    assert_eq!(
        ui.history.borrow().state.values["Image Size.Longest side"],
        3000.
    );
    checks.push("Image Size rejects invalid/oversized input and commits a valid pixel target");
    let retained = ui.history.borrow().state.clone();
    activate(ui, "crop");
    assert!(ui.canvas.cropping());
    assert!(!ui.window.lookup_action("export").unwrap().is_enabled());
    assert!(!ui.window.lookup_action("fit").unwrap().is_enabled());
    ui.canvas.set_angle(17.5);
    ui.canvas.crop_ratio(1.);
    activate(ui, "rotate");
    activate(ui, "crop-cancel");
    assert_eq!(ui.history.borrow().state, retained);
    activate(ui, "crop");
    ui.canvas.set_angle(12.3);
    ui.canvas.crop_ratio(1.);
    activate(ui, "rotate");
    activate(ui, "crop-done");
    assert_eq!(ui.history.borrow().state.straighten, 12.3);
    assert!(ui.history.borrow().state.crop.is_some());
    activate(ui, "undo");
    assert_eq!(ui.history.borrow().state, retained);
    activate(ui, "redo");
    assert_eq!(ui.history.borrow().state.straighten, 12.3);
    checks.push("Crop/straighten/rotation form one undo step; Cancel restores everything and crop commands disable conflicting actions");
    let kept = ui.history.borrow().state.clone();
    activate(ui, "reset");
    let reset = ui.history.borrow().state.clone();
    assert_eq!(reset.crop, kept.crop);
    assert_eq!(reset.border, kept.border);
    assert_eq!(reset.rotation, kept.rotation);
    assert_eq!(reset.choices.get("Film ID"), kept.choices.get("Film ID"));
    assert_eq!(reset.values.get("Image Size.Longest side"), Some(&3000.));
    checks.push("Reset Adjustments preserves film, crop, rotation, border and Image Size");
    let before_color = ui.history.borrow().state.border.clone();
    let picker = widgets(&ui.inspector)
        .into_iter()
        .find_map(|w| w.downcast::<gtk::ColorDialogButton>().ok())
        .unwrap();
    picker.set_rgba(&gtk::gdk::RGBA::new(0.1, 0.2, 0.3, 1.));
    picker.set_rgba(&gtk::gdk::RGBA::new(0.3, 0.4, 0.5, 1.));
    activate(ui, "undo");
    assert_eq!(ui.history.borrow().state.border, before_color);
    checks.push("A continuous run of border color changes is one undo step");
    activate(ui, "export");
    let dialog = ui.window.visible_dialog().unwrap();
    let rows: Vec<_> = widgets(&dialog)
        .into_iter()
        .filter_map(|w| w.downcast::<adw::ComboRow>().ok())
        .collect();
    rows.iter()
        .find(|r| r.title() == "Format")
        .unwrap()
        .set_selected(1);
    rows.iter()
        .find(|r| r.title() == "Size")
        .unwrap()
        .set_selected(2);
    let switches: Vec<_> = widgets(&dialog)
        .into_iter()
        .filter_map(|w| w.downcast::<adw::SwitchRow>().ok())
        .collect();
    switches
        .iter()
        .find(|r| r.title() == "Copy photograph metadata")
        .unwrap()
        .set_active(false);
    assert_eq!(ui.preferences.borrow().output_format, 1);
    assert_eq!(ui.preferences.borrow().export_edge, 2);
    assert!(!ui.preferences.borrow().metadata);
    dialog.force_close();
    checks.push("Export options expose conditional native controls and remember format, edge and metadata without writing a file");
    ui.history.borrow_mut().state = initial;
    ui.sync_state();
    let report = serde_json::json!({"status":"PASS","checks":checks,"scope":"Native GTK widget signals and editor commands in the real Wayland session. No engine/render/export claim."});
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
