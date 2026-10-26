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
                if std::env::var_os("NEGS_BORDER_BENCHMARK").is_some() {
                    border_benchmark(&ui);
                } else if std::env::var_os("NEGS_UI_BENCHMARK").is_some() {
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

// Alternate borders without the engine so PNG loading and texture reuse can be
// measured independently of film simulation. Also exercise disabling/re-enabling.
fn border_benchmark(ui: &Rc<Editor>) {
    let initial = ui.history.borrow().state.clone();
    let ui = Rc::downgrade(ui);
    glib::MainContext::default().spawn_local(async move {
        let Some(ui) = ui.upgrade() else { return };
        let mut samples = Vec::new();
        for style in [1, 3, 1, 3, 0, 1, 3, 1, 3, 1, 3] {
            let mut state = initial.clone();
            state.border.enabled = style != 0;
            state.border.style = crate::border::Style::from_index(style);
            let start = glib::monotonic_time();
            ui.canvas.apply(&state);
            while !ui.canvas.border_ready() {
                assert!(glib::monotonic_time() - start < 5_000_000, "Border load timed out");
                glib::timeout_future(Duration::from_millis(1)).await;
            }
            samples.push(serde_json::json!({"style":style,"ready_ms":(glib::monotonic_time()-start) as f64/1000.}));
            // Allow GPU upload/presentation before changing the border again.
            glib::timeout_future(Duration::from_millis(100)).await;
        }
        ui.canvas.apply(&initial);
        ui.perf.detail("border_selection_samples", serde_json::json!(samples));
        ui.perf.detail("border_benchmark_scope", "UI-only border selection to texture ready; includes worker scheduling, excludes compositor presentation");
        ui.perf.mark("border_benchmark_finished_ms");
        ui.perf.write();
    });
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
    ui.runtime.borrow_mut().interacting = true;
    ui.history.borrow_mut().begin("Benchmark slider drag");
    let frames = Rc::new(Cell::new(0u32));
    let origin = adjustment.value();
    ui.canvas.add_tick_callback(glib::clone!(#[weak] ui, #[upgrade_or] glib::ControlFlow::Break, move |_,_| {
        let n=frames.get();frames.set(n+1);
        adjustment.set_value(1.+0.8*(n as f64*0.06).sin());
        let center=(ui.canvas.width() as f64/2.,ui.canvas.height() as f64/2.);
        ui.canvas.zoom_at(if n.is_multiple_of(2){1.003}else{1./1.003},center);
        if n>=239 {
            ui.runtime.borrow_mut().interacting=false;adjustment.set_value(origin);ui.canvas.fit();ui.history.borrow_mut().commit();ui.sync_history();
            ui.perf.mark("ui_benchmark_finished_ms");ui.perf.detail("ui_benchmark_frames",240);
            ui.perf.detail("ui_benchmark_scope",if crate::runtime::ui_only(){"240 frame-clock-driven GTK value updates plus texture zoom; no engine or physical pointer"}else{"240 frame-clock-driven GTK value updates plus texture zoom with live interactive film rendering; no physical pointer"});
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
    // Film Base, Print Base and Balance Print, as on macOS and Windows.
    let expander = |title: &str| {
        widgets(&ui.inspector)
            .into_iter()
            .filter_map(|w| w.downcast::<adw::ExpanderRow>().ok())
            .find(|e| e.title() == title)
            .unwrap_or_else(|| panic!("missing {title}"))
    };
    assert!(expander("Film Base").is_sensitive());
    assert!(!expander("Print Base").is_sensitive());
    let balance = widgets(&ui.inspector)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk::Button>().ok())
        .find(|b| b.label().as_deref() == Some("Balance Print"))
        .unwrap();
    assert!(
        !balance.is_sensitive(),
        "No helper solves Balance Print here"
    );
    let adjustment = |key: &str| {
        ui.adjustments
            .borrow()
            .iter()
            .find(|(k, _)| k == key)
            .unwrap_or_else(|| panic!("missing {key}"))
            .1
            .clone()
    };
    let value = |key: &str| ui.history.borrow().state.values.get(key).copied();
    adjustment("Film Base.Tilt").set_value(-0.3);
    adjustment("Print Base.Density").set_value(1.5);
    assert_eq!(value("Film Base.Tilt"), Some(-0.3));
    activate(ui, "undo");
    assert_eq!(value("Print Base.Density"), Some(1.));
    activate(ui, "redo");
    assert_eq!(value("Print Base.Density"), Some(1.5));
    checks.push("Film Base and Print Base sliders edit with undo/redo; Print Base and Balance Print follow direct scanning and the helper");
    // Grain preset lanes show the film's preset and stay unset until edited.
    let row = |text: &str| {
        widgets(&ui.inspector)
            .into_iter()
            .filter_map(|w| w.downcast::<gtk::Label>().ok())
            .find(|l| l.text() == text)
            .and_then(|l| l.ancestor(gtk::ListBoxRow::static_type()))
            .unwrap_or_else(|| panic!("missing {text}"))
    };
    let lanes = || {
        ui.history
            .borrow()
            .state
            .values
            .keys()
            .filter(|k| crate::parameters::grain_lane(k).is_some())
            .count()
    };
    let position = |id: &str| films.iter().position(|s| s.id == id).unwrap() as u32;
    film.set_selected(position("kodak_vision3_500t"));
    assert_eq!(adjustment("Grain.Blue granularity").value(), 30.);
    assert_eq!(adjustment("Grain.Red uniformity").value(), 0.93);
    assert_eq!(lanes(), 0);
    adjustment("Grain.Green granularity").set_value(20.);
    assert_eq!(value("Grain.Green granularity"), Some(20.));
    activate(ui, "undo");
    assert_eq!(lanes(), 0);
    assert_eq!(adjustment("Grain.Green granularity").value(), 13.);
    activate(ui, "redo");
    assert_eq!(adjustment("Grain.Green granularity").value(), 20.);
    adjustment("Grain.Green granularity").set_value(13.);
    assert_eq!(
        lanes(),
        0,
        "A lane back at the preset leaves the field unset"
    );
    adjustment("Grain.Fine grain size").set_value(0.5);
    film.set_selected(position("kodak_doublex"));
    assert_eq!(lanes(), 0, "A new film brings its own preset");
    assert_eq!(adjustment("Grain.Red granularity").value(), 14.);
    assert_eq!(adjustment("Grain.Fine grain size").value(), 0.25);
    assert!(row("Granularity").is_visible() && row("Uniformity").is_visible());
    assert!(!row("Green granularity").is_visible() && !row("Blue uniformity").is_visible());
    assert!(row("Medium grain size").is_visible());
    film.set_selected(position("kodak_vision3_500t"));
    assert!(row("Red granularity").is_visible() && row("Blue uniformity").is_visible());
    adjustment("Grain.Red granularity").set_value(25.);
    activate(ui, "reset");
    assert_eq!(lanes(), 0);
    assert_eq!(value("Film Base.Tilt"), Some(0.));
    assert_eq!(value("Print Base.Density"), Some(1.));
    assert_eq!(adjustment("Grain.Red granularity").value(), 9.);
    checks.push("Grain preset lanes show and reset to the film's preset, return to unset, clear on a film change, show one emulsion for B&W, and clear on Reset Adjustments");
    // Collapsed groups have no accessible rows; check_controls.py uses these.
    expander("Grain").set_expanded(true);
    expander("Film Base").set_expanded(true);
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
    let crop_widgets = widgets(&ui.crop_bar);
    let aspect = crop_widgets
        .iter()
        .find_map(|w| w.clone().downcast::<gtk::DropDown>().ok())
        .unwrap();
    let portrait = crop_widgets
        .iter()
        .find_map(|w| w.clone().downcast::<gtk::ToggleButton>().ok())
        .unwrap();
    let crop_reset = crop_widgets
        .iter()
        .filter_map(|w| w.clone().downcast::<gtk::Button>().ok())
        .find(|b| b.label().as_deref() == Some("Reset"))
        .unwrap();
    let source = ui.canvas.source_size();
    let original = source.0.max(source.1) as f64 / source.0.min(source.1) as f64;
    assert!(
        original > 1.,
        "Use a non-square source to audit crop orientation"
    );
    aspect.set_selected(0);
    for _ in 0..4 {
        for tall in [true, false] {
            portrait.set_active(tall);
            let r = ui.canvas.crop_draft();
            let ratio = r[2] * source.0 as f64 / (r[3] * source.1 as f64);
            let shown = if ui.history.borrow().state.rotation % 2 == 1 {
                1. / ratio
            } else {
                ratio
            };
            let expected = if tall { 1. / original } else { original };
            assert!(
                (shown - expected).abs() < 1e-8,
                "Original crop must respect the Portrait toggle after rotation"
            );
        }
        activate(ui, "rotate");
    }
    crop_reset.emit_clicked();
    assert_eq!(ui.canvas.crop_draft(), [0., 0., 1., 1.]);
    let expected_portrait = if ui.history.borrow().state.rotation % 2 == 1 {
        source.0 > source.1
    } else {
        source.1 > source.0
    };
    assert_eq!(portrait.is_active(), expected_portrait);
    portrait.set_active(!expected_portrait);
    let oriented = ui.canvas.crop_draft();
    activate(ui, "crop-done");
    activate(ui, "crop");
    assert_eq!(aspect.selected(), 0);
    assert_eq!(portrait.is_active(), !expected_portrait);
    assert_eq!(ui.canvas.crop_draft(), oriented);
    activate(ui, "crop-cancel");
    activate(ui, "undo");
    assert_eq!(ui.history.borrow().state, retained);
    checks.push("Original crop orientation toggles at every quarter turn, resets to the source orientation, and survives reopening");
    activate(ui, "crop");
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
