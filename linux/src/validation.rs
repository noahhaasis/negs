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
                    "surface_scale",
                    ui.window.surface().map(|s| s.scale()).unwrap_or(1.),
                );
                if let Ok(scene) = std::env::var("NEGS_UI_SCENE") {
                    match scene.as_str() {
                        "post" => {
                            if let Some(tabs) = ui.tabs.borrow().as_ref() {
                                tabs.set_visible_child_name("post");
                            }
                            ui.history.borrow_mut().state.border = 2;
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
