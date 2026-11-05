//! Opt-in checks against the real serial helper and a mapped GTK canvas.
use super::*;
use std::time::Instant;

#[test]
fn quick_policy_preserves_first_open_small_photos_zoom_and_post_reuse() {
    assert_eq!(
        render_step(false, 0.2, 6e6, true, false, false),
        RenderStep::Quick(0.2)
    );
    for (scale, pixels, has_frame, done, reuse) in [
        (0.2, 6e6, false, false, false),
        (0.2, 4e6, true, false, false),
        (0.8, 6e6, true, false, false),
        (1., 6e6, true, false, false),
        (0., 6e6, true, false, false),
        (f32::NAN, 6e6, true, false, false),
        (0.2, 6e6, true, true, false),
        (0.2, 6e6, true, false, true),
    ] {
        assert_eq!(
            render_step(false, scale, pixels, has_frame, done, reuse),
            RenderStep::Settled
        );
    }
    assert_eq!(
        render_step(true, 0.2, 0., false, false, true),
        RenderStep::Drag(0.2)
    );
    let command = json!({"command":"render","input":"photo","source_generation":1,"film":"gold",
        "params":{"prep":{"exposure_ev":0},"post":{"gamma":1}},"preview_path":"a"});
    let mut changed = command.clone();
    changed["preview_path"] = json!("b");
    changed["params"]["post"]["gamma"] = json!(1.2);
    assert_eq!(reuse_key(command.clone()), reuse_key(changed.clone()));
    for field in [
        "film",
        "input",
        "source_generation",
        "crop",
        "limit_preview",
    ] {
        changed = command.clone();
        changed[field] = json!("changed");
        assert_ne!(reuse_key(command.clone()), reuse_key(changed));
    }
    changed = command.clone();
    changed["params"]["prep"]["exposure_ev"] = json!(0.2);
    assert_ne!(reuse_key(command), reuse_key(changed));
}

fn wait(ui: &Rc<Editor>, label: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(180);
    while !condition() {
        assert!(Instant::now() < deadline, "{label}: {}", ui.status.text());
        while glib::MainContext::default().iteration(false) {}
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn editor() -> Rc<Editor> {
    assert!(
        std::env::var_os("NEGS_PROFILE_DIR").is_some(),
        "Use an isolated profile"
    );
    assert!(!ui_only());
    adw::init().unwrap();
    let app = adw::Application::builder()
        .application_id("org.negs.RenderTest")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.register(None::<&gio::Cancellable>).unwrap();
    let ui = Editor::new(&app, crate::perf::Perf::new());
    ui.preferences.borrow_mut().previews = false;
    ui.window.set_default_size(1200, 850);
    ui.shell_painted.set(true);
    ui.window.present();
    ui.start_engine();
    wait(&ui, "engine", || ui.runtime.borrow().ready);
    ui
}

fn settled(ui: &Rc<Editor>) -> bool {
    let r = ui.runtime.borrow();
    !r.rendering && r.current_revision == Some(r.revision) && r.retained_id.is_some()
}

fn close(ui: &Rc<Editor>) {
    ui.runtime.borrow_mut().closing = true;
    crate::brightness::clear(ui);
    ui.runtime.borrow().client.as_ref().unwrap().stop();
    ui.window.destroy();
    while glib::MainContext::default().iteration(false) {}
}

#[test]
#[ignore = "Requires desktop, rebuilt helper, isolated NEGS_PROFILE_DIR and NEGS_RENDER_TEST_SOURCE"]
fn film_change_timings() {
    let ui = editor();
    let source = PathBuf::from(std::env::var_os("NEGS_RENDER_TEST_SOURCE").unwrap());
    ui.open(source);
    wait(&ui, "open", || {
        settled(&ui) && !crate::brightness::pending(&ui)
    });
    let mut measurements = Vec::new();
    for matching in [false, true] {
        ui.history.borrow_mut().state.brightness_match =
            matching.then(crate::model::BrightnessMatch::default);
        ui.sync_state();
        wait(&ui, "match toggle", || {
            settled(&ui) && !crate::brightness::pending(&ui)
        });
        for film in [
            "kodak_portra_400",
            "kodak_gold_200",
            "kodak_portra_400",
            "kodak_gold_200",
        ] {
            let before = ui.runtime.borrow().developed.clone().unwrap();
            let started = Instant::now();
            let start_us = glib::monotonic_time();
            let clock = ui.window.frame_clock().unwrap();
            let painted = Rc::new(std::cell::RefCell::new((None, None)));
            let observed = painted.clone();
            let weak = Rc::downgrade(&ui);
            let previous = before.clone();
            let handler = clock.connect_after_paint(move |clock| {
                let Some(ui) = weak.upgrade() else {
                    return;
                };
                let changed = ui
                    .runtime
                    .borrow()
                    .developed
                    .as_ref()
                    .is_some_and(|f| !Arc::ptr_eq(f, &previous));
                let mut observed = observed.borrow_mut();
                if changed {
                    observed.0.get_or_insert(clock.frame_counter());
                    if settled(&ui) && !crate::brightness::pending(&ui) {
                        observed.1.get_or_insert(clock.frame_counter());
                    }
                }
            });
            let stock = crate::catalog::stocks(false)
                .into_iter()
                .find(|s| s.id == film)
                .unwrap();
            crate::catalog::choose(&mut ui.history.borrow_mut().state, stock, false);
            ui.sync_state();
            wait(&ui, "first changed frame", || {
                ui.runtime
                    .borrow()
                    .developed
                    .as_ref()
                    .is_some_and(|f| !Arc::ptr_eq(f, &before))
            });
            let first_ms = started.elapsed().as_secs_f64() * 1000.;
            wait(&ui, "settled", || {
                settled(&ui) && !crate::brightness::pending(&ui)
            });
            let settled_ms = started.elapsed().as_secs_f64() * 1000.;
            wait(&ui, "settled paint", || painted.borrow().1.is_some());
            clock.disconnect(handler);
            let presentation = |counter: i64| {
                let deadline = Instant::now() + Duration::from_secs(2);
                loop {
                    if let Some(timing) = clock.timings(counter)
                        && timing.is_complete()
                        && timing.presentation_time() > 0
                    {
                        break Some((timing.presentation_time() - start_us) as f64 / 1000.);
                    }
                    if Instant::now() > deadline {
                        break None;
                    }
                    glib::MainContext::default().iteration(false);
                    std::thread::sleep(Duration::from_millis(2));
                }
            };
            let (first, final_paint) = *painted.borrow();
            let first_presented_ms = presentation(first.unwrap());
            let settled_presented_ms = presentation(final_paint.unwrap());
            let r = ui.runtime.borrow();
            measurements.push(json!({"matching":matching,"film":film,"first_ms":first_ms,
                "settled_ms":settled_ms,
                "first_presented_ms":first_presented_ms,"settled_presented_ms":settled_presented_ms,
                "helper_memory_kib":r.client.as_ref().unwrap().memory_kib(),
                "backend":r.developed.as_ref().unwrap().result["backend"],
                "canvas_scale":ui.canvas.interactive_scale()}));
        }
    }
    let report = json!({"measurements":measurements});
    println!("{report}");
    if let Some(path) = std::env::var_os("NEGS_RENDER_TEST_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    close(&ui);
}

fn choose(ui: &Rc<Editor>, id: &str) {
    let stock = crate::catalog::stocks(false)
        .into_iter()
        .find(|s| s.id == id)
        .unwrap();
    let mut h = ui.history.borrow_mut();
    h.begin("Change Film");
    crate::catalog::choose(&mut h.state, stock, false);
    h.commit();
    drop(h);
    ui.sync_state();
}

fn steps(ui: &Editor) -> Vec<RenderStep> {
    ui.runtime
        .borrow()
        .render_log
        .iter()
        .map(|(_, step)| *step)
        .collect()
}

#[test]
#[ignore = "Requires desktop, rebuilt helper and isolated NEGS_PROFILE_DIR"]
fn quick_frames_hold_match_and_preserve_final_output() {
    let ui = editor();
    let directory = PathBuf::from(std::env::var_os("NEGS_PROFILE_DIR").unwrap());
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("six-megapixels.png");
    let pixels: Vec<_> = (0..2000)
        .flat_map(|y| {
            (0..3000).flat_map(move |x| {
                [
                    (24 + x * 200 / 3000) as u8,
                    (32 + y * 180 / 2000) as u8,
                    (48 + (x + y) * 160 / 5000) as u8,
                ]
            })
        })
        .collect();
    let mut encoder = png::Encoder::new(std::fs::File::create(&source).unwrap(), 3000, 2000);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&pixels)
        .unwrap();
    ui.open(source.clone());
    wait(&ui, "initial match", || {
        settled(&ui) && !crate::brightness::pending(&ui)
    });
    assert!(
        steps(&ui).iter().all(|step| *step == RenderStep::Settled),
        "First open never renders quick"
    );
    ui.history.borrow_mut().state.brightness_match = None;
    ui.sync_state();
    wait(&ui, "matching off", || settled(&ui));
    {
        let mut r = ui.runtime.borrow_mut();
        r.test_scale = Some(0.2);
        r.render_log.clear();
    }
    let preview_scale = ui.runtime.borrow().preview_scale;
    choose(&ui, "kodak_portra_400");
    wait(&ui, "quick frame", || {
        let r = ui.runtime.borrow();
        r.quick_frame == Some(r.identity())
    });
    assert!(ui.runtime.borrow().retained_id.is_none());
    assert_ne!(
        ui.runtime.borrow().current_revision,
        Some(ui.runtime.borrow().revision)
    );
    assert_eq!(ui.runtime.borrow().preview_scale, preview_scale);
    assert!(ui.status.text().starts_with("Preview ·"));
    ui.cache.flush();
    assert!(
        !ui.cache.file.exists(),
        "Quick frames must not enter the launch cache"
    );
    let premature_save = directory.join("quick-must-not-save.png");
    ui.begin_export(premature_save.clone(), crate::exports::Kind::Preview);
    assert!(!premature_save.exists());
    wait(&ui, "quick then settled", || settled(&ui));
    assert_eq!(
        steps(&ui),
        vec![RenderStep::Quick(0.2), RenderStep::Settled]
    );
    let final_frame = ui.runtime.borrow().developed.clone().unwrap();
    assert_eq!((final_frame.width, final_frame.height), (3000, 2000));

    // Independent helper, same inputs, without a quick step: final bytes agree.
    let command = {
        let r = ui.runtime.borrow();
        json!({"command":"render","input":source,"source_generation":r.source_generation,
            "film":r.desired["film"],"paper":r.desired["paper"],"params":r.desired["params"],
            "limit_preview":r.desired["limit_preview"],"preview_path":directory.join("independent.rgba")})
    };
    let independent = Engine::new().unwrap();
    let (_, result) = independent
        .request(command, Duration::from_secs(180))
        .unwrap();
    let expected = Frame::read(&directory.join("independent.rgba"), result).unwrap();
    assert_eq!(final_frame.pixels, expected.pixels);
    independent.stop();

    // Post edits reuse the helper's settled float output, without quick work.
    ui.runtime.borrow_mut().render_log.clear();
    ui.history
        .borrow_mut()
        .state
        .values
        .insert("Post.Saturation".into(), 0.8);
    ui.sync_state();
    wait(&ui, "post edit", || settled(&ui));
    assert_eq!(steps(&ui), vec![RenderStep::Settled]);

    // Release a slider's already-small frame directly to settled.
    ui.runtime.borrow_mut().render_log.clear();
    ui.runtime.borrow_mut().interacting = true;
    ui.history
        .borrow_mut()
        .state
        .values
        .insert("Prep.Contrast".into(), 20.);
    ui.sync_state();
    wait(&ui, "drag", || !ui.runtime.borrow().rendering);
    ui.runtime.borrow_mut().interacting = false;
    ui.sync_state();
    wait(&ui, "release", || settled(&ui));
    assert_eq!(steps(&ui), vec![RenderStep::Drag(0.2), RenderStep::Settled]);

    // A provisional frame stays hidden during matching. Its solve can start
    // from the held decode, and only one final full-size render follows it.
    ui.history.borrow_mut().state.brightness_match = Some(crate::model::BrightnessMatch::default());
    ui.sync_state();
    wait(&ui, "enable matching", || {
        settled(&ui) && !crate::brightness::pending(&ui)
    });
    ui.runtime.borrow_mut().render_log.clear();
    let previous = ui.runtime.borrow().developed.clone().unwrap();
    choose(&ui, "kodak_gold_200");
    wait(&ui, "held quick", || ui.runtime.borrow().held.is_some());
    assert!(Arc::ptr_eq(
        ui.runtime.borrow().developed.as_ref().unwrap(),
        &previous
    ));
    assert!(ui.runtime.borrow().retained_id.is_none());
    assert!(crate::brightness::running(&ui));
    let matched_save = directory.join("matched-save.png");
    ui.begin_export(matched_save.clone(), crate::exports::Kind::Preview);
    assert!(!matched_save.exists());
    wait(&ui, "match/save", || {
        matched_save.exists() && settled(&ui) && !crate::brightness::pending(&ui)
    });
    assert_eq!(
        steps(&ui)
            .iter()
            .filter(|step| **step == RenderStep::Settled)
            .count(),
        1
    );

    // Undo and superseding edits cannot make an earlier quick frame saveable.
    ui.runtime.borrow_mut().render_log.clear();
    ui.history.borrow_mut().state.brightness_match = None;
    choose(&ui, "kodak_portra_400");
    wait(&ui, "undo quick", || {
        let r = ui.runtime.borrow();
        r.quick_frame == Some(r.identity())
    });
    ui.history.borrow_mut().undo();
    ui.sync_state();
    wait(&ui, "undo settled", || {
        settled(&ui) && !crate::brightness::pending(&ui)
    });
    assert_eq!(
        crate::catalog::selected(&ui.history.borrow().state, false).id,
        "kodak_gold_200"
    );
    assert!(ui.runtime.borrow().held.is_none());

    // A failed quick request silently falls through to one settled attempt.
    ui.history.borrow_mut().state.brightness_match = None;
    ui.runtime.borrow_mut().render_log.clear();
    ui.runtime.borrow_mut().fail_quick = true;
    choose(&ui, "kodak_portra_400");
    wait(&ui, "failed quick fallback", || settled(&ui));
    assert_eq!(
        steps(&ui),
        vec![RenderStep::Quick(0.2), RenderStep::Settled]
    );
    assert!(!ui.status.text().contains("Could not"));

    // A post-only edit made while a film's quick frame is in flight must
    // develop the film at full size; the earlier retained look is gone.
    ui.runtime.borrow_mut().render_log.clear();
    choose(&ui, "kodak_gold_200");
    ui.history
        .borrow_mut()
        .state
        .values
        .insert("Post.Contrast".into(), 10.);
    ui.sync_state();
    wait(&ui, "coalesced post", || settled(&ui));
    assert!(steps(&ui).iter().any(|step| step.quick()));
    assert_eq!(steps(&ui).last(), Some(&RenderStep::Settled));

    // The launch cache is not a developed frame, even when an edit clears
    // its activity flag before the helper's first request finishes.
    ui.runtime.borrow_mut().live_frame = false;
    ui.runtime.borrow_mut().render_log.clear();
    choose(&ui, "kodak_portra_400");
    wait(&ui, "cached pixels", || settled(&ui));
    assert_eq!(steps(&ui), vec![RenderStep::Settled]);

    ui.show_original(true);
    wait(&ui, "original", || ui.runtime.borrow().original.is_some());
    choose(&ui, "kodak_gold_200");
    wait(&ui, "develop behind Original", || settled(&ui));
    assert!(ui.runtime.borrow().showing_original);
    ui.show_original(false);
    assert!(!ui.runtime.borrow().showing_original);

    choose(&ui, "kodak_portra_400");
    let second = directory.join("second.png");
    std::fs::copy(&source, &second).unwrap();
    let generation = ui.runtime.borrow().source_generation;
    ui.open(second.clone());
    wait(&ui, "source replacement", || {
        *ui.current_path.borrow() == Some(second.clone())
            && settled(&ui)
            && !crate::brightness::pending(&ui)
    });
    assert!(ui.runtime.borrow().source_generation > generation);
    assert!(ui.runtime.borrow().held.is_none());
    let new_generation = ui.runtime.borrow().source_generation;
    assert!(
        ui.runtime
            .borrow()
            .render_log
            .iter()
            .filter(|(id, _)| id.generation == new_generation)
            .all(|(_, step)| *step == RenderStep::Settled)
    );
    ui.history.borrow_mut().state.brightness_match = None;
    ui.sync_state();
    // An open stock grid may already have one request in flight. Once the
    // quick main request starts, optional work cannot slip ahead of settling.
    fn film_row(widget: &gtk::Widget) -> Option<adw::ExpanderRow> {
        if let Some(row) = widget.downcast_ref::<adw::ExpanderRow>()
            && row.title() == "Film"
        {
            return Some(row.clone());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            if let Some(row) = film_row(&widget) {
                return Some(row);
            }
            child = widget.next_sibling();
        }
        None
    }
    ui.preferences.borrow_mut().previews = true;
    crate::stock_browser::apply(&ui);
    film_row(ui.inspector.upcast_ref())
        .unwrap()
        .set_expanded(true);
    let engine = ui.runtime.borrow().client.clone().unwrap();
    engine.request_log.lock().unwrap().clear();
    ui.runtime.borrow_mut().render_log.clear();
    let next = if crate::catalog::selected(&ui.history.borrow().state, false).id == "kodak_gold_200"
    {
        "kodak_portra_400"
    } else {
        "kodak_gold_200"
    };
    choose(&ui, next);
    wait(&ui, "main before optional work", || settled(&ui));
    assert_eq!(
        steps(&ui),
        vec![RenderStep::Quick(0.2), RenderStep::Settled]
    );
    {
        let log = engine.request_log.lock().unwrap();
        let main: Vec<_> = log
            .iter()
            .enumerate()
            .filter_map(|(i, command)| (command == "render").then_some(i))
            .collect();
        assert_eq!(main.len(), 2);
        assert_eq!(
            main[1],
            main[0] + 1,
            "Optional work interrupted quick → settled: {log:?}"
        );
    }
    ui.preferences.borrow_mut().previews = false;
    crate::stock_browser::apply(&ui);
    ui.runtime.borrow_mut().render_log.clear();
    {
        let mut r = ui.runtime.borrow_mut();
        r.fail_quick = true;
        r.fail_settled = true;
    }
    choose(
        &ui,
        if next == "kodak_gold_200" {
            "kodak_portra_400"
        } else {
            "kodak_gold_200"
        },
    );
    wait(&ui, "settled failure", || {
        !ui.runtime.borrow().errors.is_empty()
    });
    assert_eq!(
        steps(&ui),
        vec![RenderStep::Quick(0.2), RenderStep::Settled]
    );
    assert_eq!(ui.runtime.borrow().errors.len(), 1);
    ui.render_next();
    ui.sync_history();
    assert_eq!(steps(&ui).len(), 2, "Failed settled frames must not loop");
    assert!(ui.runtime.borrow().retained_id.is_none());
    close(&ui);
}
