//! Prep's live camera/original brightness match. The helper supplies a
//! colour-managed reference and solves it; this module owns only UI/state,
//! request lifetime and the exposure offset above that solution.
use crate::{
    Editor,
    fixtures::Knob,
    model::{BrightnessMatch, State},
    parameters,
};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const EXPOSURE: &str = "Prep.Exposure";

#[derive(Default)]
pub struct Panel {
    token: Arc<AtomicU64>,
    running: Cell<bool>,
    failure: RefCell<Option<Failure>>,
    toggle: RefCell<Option<adw::SwitchRow>>,
    exposure: RefCell<Option<gtk::Adjustment>>,
    note: RefCell<Option<gtk::Label>>,
    output_token: Cell<u64>,
}

struct Failure {
    generation: u64,
    /// None: a property of the photograph; Some: this look failed to solve.
    inputs: Option<Value>,
    message: String,
}

/// The same dependency mask as macOS and Windows. The runtime projection
/// includes the edited values/choices, rather than just saved raw params.
pub fn match_inputs(state: &State, defaults: &Value) -> Value {
    let mut params = parameters::runtime(state, defaults);
    for path in [
        "prep",
        "enlarger.print_exposure",
        "post",
        "film_render.grain",
        "io.rotate_cw",
        "io.crop",
        "io.crop_center",
        "io.crop_size",
        "io.output_color_space",
        "io.output_cctf_encoding",
        "settings.serial_grain",
    ] {
        parameters::set(&mut params, path, Value::Null);
    }
    json!({"film":crate::catalog::selected(state,false).id,
        "paper":crate::catalog::selected(state,true).id,"params":params})
}

fn unavailable(ui: &Editor, state: &State) -> Option<String> {
    let runtime = ui.runtime.borrow();
    let generation = runtime.source_generation;
    if runtime.ready && !runtime.supports_brightness_reference {
        return Some("The installed image engine does not support brightness matching. Reinstall the matching Negs package.".into());
    }
    drop(runtime);
    if let Some(failure) = ui.brightness.failure.borrow().as_ref()
        && failure.generation == generation
        && failure.inputs.is_none()
    {
        return Some(failure.message.clone());
    }
    if ui
        .current_path
        .borrow()
        .as_deref()
        .is_some_and(|path| !crate::photo::is_raw(path))
    {
        let params = parameters::runtime(state, &ui.runtime.borrow().defaults);
        if params["io"]["input_interpretation"]["mode"]
            .as_str()
            .unwrap_or("auto")
            != "auto"
        {
            return Some("Matching reads the file’s own colour tags. Set Input Colour to Automatic to use it.".into());
        }
    }
    None
}

fn failed_here(ui: &Editor, state: &State) -> bool {
    let runtime = ui.runtime.borrow();
    let failure = ui.brightness.failure.borrow();
    failure.as_ref().is_some_and(|f| {
        f.generation == runtime.source_generation
            && f.inputs
                .as_ref()
                .is_some_and(|inputs| *inputs == match_inputs(state, &runtime.defaults))
    })
}

pub fn active(ui: &Editor, state: &State) -> bool {
    state.brightness_match.is_some() && unavailable(ui, state).is_none() && !failed_here(ui, state)
}

fn unsolved(ui: &Editor, state: &State) -> bool {
    active(ui, state)
        && state.brightness_match.as_ref().is_some_and(|matched| {
            matched.ev.is_none()
                || matched.inputs.as_ref()
                    != Some(&match_inputs(state, &ui.runtime.borrow().defaults))
        })
}

pub fn running(ui: &Editor) -> bool {
    ui.brightness.running.get()
}

/// Unlike `pending`, this excludes the final settled render after a solve.
pub fn needs_solution(ui: &Editor) -> bool {
    unsolved(ui, &ui.history.borrow().state)
}

/// Outputs also wait for the render caused by the applied match. Optional
/// graph/thumbnail work uses this to yield to matching and the main preview.
pub fn pending(ui: &Editor) -> bool {
    let state = ui.history.borrow().state.clone();
    if ui.current_path.borrow().is_none() {
        return false;
    }
    if ui.brightness.running.get() || unsolved(ui, &state) {
        return true;
    }
    let runtime = ui.runtime.borrow();
    active(ui, &state) && runtime.current_revision != Some(runtime.revision)
}

pub fn shows_meter(ui: &Editor, state: &State) -> bool {
    if !active(ui, state) {
        return true;
    }
    let runtime = ui.runtime.borrow();
    let params = parameters::runtime(state, &runtime.defaults);
    ["camera.auto_exposure", "camera.auto_exposure_method"]
        .into_iter()
        .any(|path| parameters::get(&params, path) != parameters::get(&runtime.defaults, path))
}

fn matched_ev(ui: &Editor, state: &State) -> f64 {
    if active(ui, state) {
        state
            .brightness_match
            .as_ref()
            .and_then(|m| m.ev)
            .unwrap_or(0.)
    } else {
        0.
    }
}

fn total_ev(state: &State) -> f64 {
    state
        .values
        .get(EXPOSURE)
        .copied()
        .or_else(|| state.params["prep"]["exposure_ev"].as_f64())
        .unwrap_or(0.)
}

pub fn build(ui: &Rc<Editor>) -> gtk::Box {
    let body = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    let toggle = adw::SwitchRow::builder()
        .title("Match camera brightness")
        .build();
    toggle.set_tooltip_text(Some(crate::help::MATCH));
    toggle.update_property(&[gtk::accessible::Property::Description(crate::help::MATCH)]);
    toggle.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| {
            if ui.syncing.get() {
                return;
            }
            let on = row.is_active();
            let mut history = ui.history.borrow_mut();
            if on == history.state.brightness_match.is_some() {
                return;
            }
            history.commit();
            history.begin(if on {
                "Match Camera Brightness"
            } else {
                "Stop Matching Camera Brightness"
            });
            history.state.brightness_match = on.then(BrightnessMatch::default);
            if on {
                history.state.values.insert(EXPOSURE.into(), 0.);
            }
            history.commit();
            drop(history);
            ui.sync_state();
        }
    ));
    list.append(&toggle);
    let initial = total_ev(&ui.history.borrow().state);
    let owner = Rc::downgrade(ui);
    let (exposure, adjustment) = crate::inspector::number(
        ui,
        &Knob("Exposure", -3., 3., 0.05, 0., "EV"),
        initial,
        false,
        move |state, offset| {
            let matched = owner
                .upgrade()
                .map(|ui| matched_ev(&ui, state))
                .unwrap_or(0.);
            state
                .values
                .insert(EXPOSURE.into(), (matched + offset).clamp(-3., 3.));
        },
    );
    crate::inspector::control_help(&exposure, crate::help::text("prep.exposure_ev"));
    list.append(&exposure);
    body.append(&list);
    let note = gtk::Label::new(None);
    note.set_wrap(true);
    note.set_xalign(0.);
    note.add_css_class("caption");
    note.add_css_class("dim-label");
    note.set_tooltip_text(Some(crate::help::MATCH));
    body.append(&note);
    ui.brightness.toggle.replace(Some(toggle));
    ui.brightness.exposure.replace(Some(adjustment));
    ui.brightness.note.replace(Some(note));
    let owner = Rc::downgrade(ui);
    ui.rules.borrow_mut().push(Box::new(move |state| {
        if let Some(ui) = owner.upgrade() {
            refresh(&ui, state);
        }
    }));
    body
}

fn refresh(ui: &Editor, state: &State) {
    let syncing = ui.syncing.replace(true);
    let unavailable = unavailable(ui, state);
    let active = active(ui, state);
    if let Some(row) = ui.brightness.toggle.borrow().as_ref() {
        let raw = ui
            .current_path
            .borrow()
            .as_deref()
            .is_none_or(crate::photo::is_raw);
        row.set_title(if raw {
            "Match camera brightness"
        } else {
            "Match original brightness"
        });
        row.set_active(active);
        row.set_sensitive(
            ui.current_path.borrow().is_some() && unavailable.is_none() && !failed_here(ui, state),
        );
    }
    let matched = matched_ev(ui, state);
    if let Some(adjustment) = ui.brightness.exposure.borrow().as_ref() {
        adjustment.set_lower(-3. - matched);
        adjustment.set_upper(3. - matched);
        adjustment.set_value(total_ev(state) - matched);
    }
    if let Some(note) = ui.brightness.note.borrow().as_ref() {
        let failure = ui.brightness.failure.borrow();
        let text = if state.brightness_match.is_none() {
            None
        } else if let Some(reason) = unavailable {
            Some(reason)
        } else if failed_here(ui, state) {
            failure.as_ref().map(|f| f.message.clone())
        } else if ui.brightness.running.get() || unsolved(ui, state) {
            Some("Matching…".into())
        } else {
            state.brightness_match.as_ref().and_then(|m| {
                m.ev.map(|ev| {
                    let reference = if ui
                        .current_path
                        .borrow()
                        .as_deref()
                        .is_some_and(crate::photo::is_raw)
                    {
                        "the camera"
                    } else {
                        "the original"
                    };
                    format!(
                        "Starts at {}{:.2} EV to match {reference}{}.",
                        if ev >= 0. { "+" } else { "−" },
                        ev.abs(),
                        if m.highlight_limited {
                            ", held back to protect highlights"
                        } else {
                            ""
                        }
                    )
                })
            })
        };
        note.set_text(text.as_deref().unwrap_or(""));
        note.set_visible(text.is_some());
    }
    ui.syncing.set(syncing);
}

/// Invalidate in-flight work on source replacement or shutdown. Worker results
/// and delayed outputs still complete, but cannot apply to a later photograph.
pub fn clear(ui: &Editor) {
    ui.brightness.token.fetch_add(1, Ordering::Relaxed);
    ui.brightness.running.set(false);
    ui.brightness.failure.take();
    ui.brightness
        .output_token
        .set(ui.brightness.output_token.get().wrapping_add(1));
}

struct SolveFailure {
    message: String,
    for_photo: bool,
}

fn failure_reason(reason: &str) -> SolveFailure {
    let (message, for_photo) = if reason.contains("midtone quantiles") {
        (
            "The camera preview is too dark or too bright to match.",
            true,
        )
    } else if reason.contains("±3 EV") {
        ("Matching would take more than ±3 EV.", false)
    } else {
        ("No reliable brightness match with these settings.", false)
    };
    SolveFailure {
        message: message.into(),
        for_photo,
    }
}

pub fn pump(ui: &Rc<Editor>) {
    let state = ui.history.borrow().state.clone();
    refresh(ui, &state);
    if state.brightness_match.is_none() {
        if ui.brightness.running.replace(false) {
            ui.brightness.token.fetch_add(1, Ordering::Relaxed);
        }
        ui.release_held_frame();
        ui.render_next();
        return;
    }
    if !ui.brightness.running.get() && !unsolved(ui, &state) {
        ui.release_held_frame();
        ui.render_next();
        return;
    }
    if crate::runtime::ui_only()
        || ui.brightness.running.get()
        || !unsolved(ui, &state)
        || ui.canvas.cropping()
    {
        return;
    }
    let runtime = ui.runtime.borrow();
    if !runtime.ready
        || runtime.closing
        || runtime.interacting
        || runtime.rendering
        || runtime.block_helper_render
        || runtime.exporting.is_some()
    {
        return;
    }
    let (Some(client), Some((source, _)), Some(frame)) = (
        runtime.client.clone(),
        runtime.source.clone(),
        runtime.matching_frame(),
    ) else {
        return;
    };
    let generation = runtime.source_generation;
    let inputs = match_inputs(&state, &runtime.defaults);
    let mut params = parameters::runtime(&state, &runtime.defaults);
    parameters::set(&mut params, "prep", runtime.defaults["prep"].clone());
    parameters::set(
        &mut params,
        "enlarger.print_exposure",
        runtime.defaults["enlarger"]["print_exposure"].clone(),
    );
    let film = crate::catalog::selected(&state, false).id.clone();
    let paper = crate::catalog::selected(&state, true).id.clone();
    let expected_width = frame.result["source_width"]
        .as_u64()
        .unwrap_or(frame.width as u64);
    let expected_height = frame.result["source_height"]
        .as_u64()
        .unwrap_or(frame.height as u64);
    drop(runtime);
    let token = ui.brightness.token.clone();
    let id = token.fetch_add(1, Ordering::Relaxed) + 1;
    let reference = client
        .scratch
        .directory
        .join(format!("brightness-{id}.png"));
    ui.brightness.running.set(true);
    refresh(ui, &state);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let result=gio::spawn_blocking(move || {
            let solve=(|| {
                if token.load(Ordering::Relaxed)!=id {return Ok(None);}
                let (_,made)=client.request(json!({"command":"brightness_reference","input":source,"reference_path":reference,
                    "expected_width":expected_width,"expected_height":expected_height}),Duration::from_secs(60))
                    .map_err(|message|failure_reason(&message))?;
                if made["usable"]!=true {
                    return Err(SolveFailure {message:made["reason"].as_str().unwrap_or("The photograph couldn’t be read for matching.").into(),for_photo:true});
                }
                if token.load(Ordering::Relaxed)!=id {return Ok(None);}
                let (_,result)=client.request(json!({"command":"place_exposure","input":source,"source_generation":generation,
                    "film":film,"paper":paper,"params":params,"reference_path":reference}),Duration::from_secs(180))
                    .map_err(|message|failure_reason(&message))?;
                Ok(Some(result))
            })();
            let _=std::fs::remove_file(&reference);
            solve
        }).await;
        if ui.brightness.token.load(Ordering::Relaxed)!=id {return;}
        ui.brightness.running.set(false);
        if ui.runtime.borrow().closing || ui.runtime.borrow().source_generation!=generation {return;}
        let current=ui.history.borrow().state.clone();
        if current.brightness_match.is_none() || match_inputs(&current,&ui.runtime.borrow().defaults)!=inputs {
            pump(&ui); ui.render_next(); return;
        }
        let result=match result {
            Ok(Ok(Some(result)))=>match result["exposure_ev"].as_f64().filter(|ev|ev.is_finite() && (-3.0..=3.0).contains(ev)) {
                Some(ev)=>Ok((ev,result["highlight_limited"]==true)),
                None=>Err(failure_reason("Invalid solution")),
            },
            Ok(Ok(None))=>{pump(&ui);return;},
            Ok(Err(failure))=>Err(failure),
            Err(_)=>Err(failure_reason("The image worker stopped")),
        };
        match result {
            Ok((ev,limited))=>{
                let mut history=ui.history.borrow_mut();
                apply_match(&mut history.state,ev,limited,inputs);
                drop(history);
                ui.brightness.failure.take();
            }
            Err(failure)=>{
                ui.brightness.failure.replace(Some(Failure {generation,
                    inputs:(!failure.for_photo).then_some(inputs),message:failure.message}));
            }
        }
        ui.sync_state();
        pump(&ui);
        ui.render_next();
        crate::tone_graph::pump(&ui);
        crate::stock_browser::pump(&ui);
    });
}

/// A solve is part of the edit that initiated it, not another Undo step.
fn apply_match(state: &mut State, ev: f64, limited: bool, inputs: Value) {
    let Some(mut matched) = state.brightness_match.clone() else {
        return;
    };
    let offset = total_ev(state) - matched.ev.unwrap_or(0.);
    if matched.ev.is_none_or(|old| (ev - old).abs() >= 0.05) {
        matched.ev = Some(ev);
        state
            .values
            .insert(EXPOSURE.into(), (ev + offset).clamp(-3., 3.));
    }
    matched.inputs = Some(inputs);
    matched.highlight_limited = limited;
    state.brightness_match = Some(matched);
}

/// Called before begin_export snapshots the edit. A delayed action belongs
/// to the source it was requested for and cannot write a different photograph.
pub fn defer_output(ui: &Rc<Editor>, output: PathBuf, kind: crate::exports::Kind) -> bool {
    if kind == crate::exports::Kind::Lut || !pending(ui) {
        return false;
    }
    let generation = ui.runtime.borrow().source_generation;
    let source = ui.current_path.borrow().clone();
    let id = ui.brightness.output_token.get().wrapping_add(1);
    ui.brightness.output_token.set(id);
    ui.status.set_text("Matching camera brightness…");
    pump(ui);
    let started = Instant::now();
    let weak = Rc::downgrade(ui);
    glib::timeout_add_local(Duration::from_millis(25), move || {
        let Some(ui) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if ui.runtime.borrow().closing
            || ui.brightness.output_token.get() != id
            || ui.runtime.borrow().source_generation != generation
            || *ui.current_path.borrow() != source
        {
            return glib::ControlFlow::Break;
        }
        if pending(&ui) {
            // CPU fallback on a large RAW can spend over 30 seconds on the
            // final frame alone. Allow the bounded reference/solve/render
            // chain to finish instead of abandoning an otherwise valid Save.
            if started.elapsed() < Duration::from_secs(10 * 60) {
                return glib::ControlFlow::Continue;
            }
            ui.status.set_text(
                "Brightness matching did not finish. Wait for the preview and save again.",
            );
            return glib::ControlFlow::Break;
        }
        ui.begin_export(output.clone(), kind);
        glib::ControlFlow::Break
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solved_exposure_preserves_the_offset_and_suppresses_small_rematches() {
        let mut state = State::default();
        state.values.insert(EXPOSURE.into(), 0.3);
        state.brightness_match = Some(BrightnessMatch::default());
        apply_match(&mut state, 0.8, false, json!({"look":1}));
        assert!((total_ev(&state) - 1.1).abs() < 1e-9);
        assert_eq!(state.brightness_match.as_ref().unwrap().ev, Some(0.8));
        apply_match(&mut state, 0.82, true, json!({"look":2}));
        assert!((total_ev(&state) - 1.1).abs() < 1e-9);
        assert_eq!(state.brightness_match.as_ref().unwrap().ev, Some(0.8));
        assert!(state.brightness_match.as_ref().unwrap().highlight_limited);
        apply_match(&mut state, 1.2, false, json!({"look":3}));
        assert!((total_ev(&state) - 1.5).abs() < 1e-9);
        apply_match(&mut state, 3., false, json!({"look":4}));
        assert_eq!(total_ev(&state), 3.);
    }

    #[test]
    fn camera_brightness_dependencies_exclude_user_brightness_but_include_the_look() {
        let defaults = json!({"prep":{"exposure_ev":0.,"highlights":0.},"post":{},
            "camera":{"exposure_compensation_ev":0.,"auto_exposure":true},
            "enlarger":{"print_exposure":1.,"m_filter_shift":0.},"film_render":{"grain":{"active":true}},
            "io":{"upscale_factor":1,"photo_long_side_px":null,"rotate_cw":0,"output_color_space":"sRGB","output_cctf_encoding":true},
            "settings":{"serial_grain":false}});
        let mut state = State::default();
        let initial = match_inputs(&state, &defaults);
        state.values.insert(EXPOSURE.into(), 1.2);
        state.values.insert("Prep.Highlights".into(), -50.);
        state.values.insert("Enlarger.Print exposure".into(), 1.6);
        state.choices.insert("Grain enabled".into(), "false".into());
        state.crop = Some([0.1, 0.2, 0.6, 0.5]);
        state.rotation = 1;
        state.straighten = 2.;
        assert_eq!(initial, match_inputs(&state, &defaults));
        state.values.insert("Camera.Exposure".into(), 0.8);
        assert_ne!(initial, match_inputs(&state, &defaults));
        assert_eq!(
            failure_reason("Camera preview has too few usable midtone quantiles").for_photo,
            true
        );
        assert_eq!(failure_reason("outside ±3 EV range").for_photo, false);
    }

    #[test]
    #[ignore = "Requires a desktop, rebuilt helper and NEGS_PROFILE_DIR; run explicitly with --ignored --test-threads=1"]
    fn live_matching_cancels_stale_work_and_defers_saves() {
        assert!(
            std::env::var_os("NEGS_PROFILE_DIR").is_some(),
            "Use an isolated NEGS_PROFILE_DIR"
        );
        assert!(!crate::runtime::ui_only());
        adw::init().unwrap();
        let app = adw::Application::builder()
            .application_id("org.negs.BrightnessTest")
            .flags(gio::ApplicationFlags::NON_UNIQUE)
            .build();
        app.register(None::<&gio::Cancellable>).unwrap();
        let ui = Editor::new(&app, crate::perf::Perf::new());
        ui.shell_painted.set(true);
        ui.window.present();
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let source = root.join("docs/card.jpg");
        let out = PathBuf::from(std::env::var_os("NEGS_PROFILE_DIR").unwrap())
            .join(format!("brightness-{}", crate::engine::unique()));
        std::fs::create_dir_all(&out).unwrap();
        let wait = |label: &str, condition: &dyn Fn() -> bool| {
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                if condition() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "{label}: status {}, matching {}, failure {:?}",
                    ui.status.text(),
                    ui.brightness.running.get(),
                    ui.brightness.failure.borrow().as_ref().map(|f| &f.message)
                );
                glib::MainContext::default().iteration(false);
                std::thread::sleep(Duration::from_millis(1));
            }
        };
        ui.start_engine();
        ui.open(source.clone());
        wait("initial match", &|| {
            !pending(&ui)
                && ui
                    .history
                    .borrow()
                    .state
                    .brightness_match
                    .as_ref()
                    .is_some_and(|m| m.ev.is_some())
                && ui.runtime.borrow().retained_id.is_some()
        });
        assert!(
            ui.runtime.borrow().supports_brightness_reference,
            "Rebuild the helper before this test"
        );
        assert!(
            ui.brightness
                .toggle
                .borrow()
                .as_ref()
                .unwrap()
                .title()
                .contains("original")
        );

        // A sub-threshold rematch releases the held settled frame without
        // an exposure edit or another render, and enables Save again.
        let previous = ui.runtime.borrow().developed.clone().unwrap();
        let exposure = total_ev(&ui.history.borrow().state);
        ui.history
            .borrow_mut()
            .state
            .values
            .insert("Camera.Exposure".into(), 0.0001);
        ui.sync_state();
        let revision = ui.runtime.borrow().revision;
        wait("unchanged solve started", &|| running(&ui));
        assert!(Arc::ptr_eq(
            ui.runtime.borrow().developed.as_ref().unwrap(),
            &previous
        ));
        wait("unchanged solve released", &|| !pending(&ui));
        assert_eq!(ui.runtime.borrow().revision, revision);
        assert_eq!(total_ev(&ui.history.borrow().state), exposure);
        assert!(ui.runtime.borrow().retained_id.is_some());
        assert!(
            ui.window
                .lookup_action("save-preview")
                .unwrap()
                .is_enabled()
        );

        // Failure may belong to this look or to the photograph. Supersede
        // the worker with that verdict and verify that its held frame is
        // released, rather than leaving matching/output permanently pending.
        for (for_photo, value) in [(false, 0.1), (true, 0.2)] {
            ui.history
                .borrow_mut()
                .state
                .values
                .insert("Camera.Exposure".into(), value);
            ui.sync_state();
            wait("failing solve started", &|| running(&ui));
            ui.brightness.token.fetch_add(1, Ordering::Relaxed);
            ui.brightness.running.set(false);
            let inputs = match_inputs(&ui.history.borrow().state, &ui.runtime.borrow().defaults);
            ui.brightness.failure.replace(Some(Failure {
                generation: ui.runtime.borrow().source_generation,
                inputs: (!for_photo).then_some(inputs),
                message: "Test match unavailable".into(),
            }));
            ui.sync_state();
            wait("failed solve released", &|| !pending(&ui));
            assert!(ui.runtime.borrow().retained_id.is_some());
            assert!(
                ui.window
                    .lookup_action("save-preview")
                    .unwrap()
                    .is_enabled()
            );
            ui.brightness.failure.take();
        }

        // A new dependency set invalidates the solution; the result must
        // describe the latest look, even if an earlier solve was already sent.
        {
            let mut history = ui.history.borrow_mut();
            history.state.values.insert("Camera.Exposure".into(), 0.6);
        }
        ui.sync_state();
        wait("first re-match sent", &|| ui.brightness.running.get());
        ui.history
            .borrow_mut()
            .state
            .values
            .insert("Camera.Exposure".into(), -0.4);
        ui.sync_state();
        wait("latest look matched", &|| !pending(&ui));
        let state = ui.history.borrow().state.clone();
        assert_eq!(
            state.brightness_match.as_ref().unwrap().inputs.as_ref(),
            Some(&match_inputs(&state, &ui.runtime.borrow().defaults))
        );

        // Turning the real switch off during a solve supersedes its reply.
        ui.history
            .borrow_mut()
            .state
            .values
            .insert("Camera.Exposure".into(), 0.3);
        ui.sync_state();
        wait("cancellable match sent", &|| ui.brightness.running.get());
        ui.brightness
            .toggle
            .borrow()
            .as_ref()
            .unwrap()
            .set_active(false);
        let cancelled = ui.brightness.token.load(Ordering::Relaxed);
        wait("switch-off preview", &|| {
            !ui.runtime.borrow().rendering
                && ui.runtime.borrow().current_revision == Some(ui.runtime.borrow().revision)
        });
        assert!(ui.history.borrow().state.brightness_match.is_none());
        assert_eq!(cancelled, ui.brightness.token.load(Ordering::Relaxed));

        // Save is invoked before matching can settle. It must wait for the
        // final render and save it, not the preceding retained preview.
        ui.brightness
            .toggle
            .borrow()
            .as_ref()
            .unwrap()
            .set_active(true);
        let saved = out.join("matched-save.png");
        ui.begin_export(saved.clone(), crate::exports::Kind::Preview);
        assert!(!saved.exists());
        wait("deferred Save Preview", &|| {
            saved.exists() && ui.runtime.borrow().exporting.is_none()
        });
        assert!(!pending(&ui));
        let state = ui.history.borrow().state.clone();
        assert!(state.brightness_match.as_ref().unwrap().ev.is_some());
        let (_, width, height) = gtk::gdk_pixbuf::Pixbuf::file_info(&saved).unwrap();
        let frame = ui.runtime.borrow().developed.clone().unwrap();
        assert_eq!((width, height), (frame.width, frame.height));

        // A waiting save belongs to its source. Opening another photograph
        // cannot publish that second photograph to the first one's target.
        ui.history
            .borrow_mut()
            .state
            .values
            .insert("Camera.Exposure".into(), 0.9);
        ui.sync_state();
        let cancelled_save = out.join("superseded-save.png");
        ui.begin_export(cancelled_save.clone(), crate::exports::Kind::Preview);
        let second = out.join("second.jpg");
        std::fs::copy(&source, &second).unwrap();
        ui.open(second.clone());
        wait("second photograph", &|| {
            ui.current_path.borrow().as_ref() == Some(&second)
                && !pending(&ui)
                && ui.runtime.borrow().retained_id.is_some()
        });
        assert!(!cancelled_save.exists());
        ui.runtime.borrow_mut().closing = true;
        clear(&ui);
        if let Some(engine) = ui.runtime.borrow().client.as_ref() {
            engine.stop();
        }
        ui.window.close();
    }
}
