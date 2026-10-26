use crate::{
    Editor,
    fixtures::{self, Knob},
};
use adw::prelude::*;
use gtk::glib;
use std::{cell::Cell, rc::Rc};

pub fn knob(ui: &Rc<Editor>, group: &str, knob: &Knob) -> gtk::ListBoxRow {
    let key = format!("{group}.{}", knob.0);
    let engine_default = crate::parameters::path(&key)
        .and_then(|p| crate::parameters::get(&ui.runtime.borrow().defaults, &p).as_f64())
        .unwrap_or(knob.4);
    let knob = &Knob(knob.0, knob.1, knob.2, knob.3, engine_default, knob.5);
    let value = ui
        .history
        .borrow()
        .state
        .values
        .get(&key)
        .copied()
        .unwrap_or(knob.4);
    let (row, adjustment) = number(ui, knob, value, false, move |state, value| {
        state.values.insert(key.clone(), value);
    });
    ui.adjustments
        .borrow_mut()
        .push((format!("{group}.{}", knob.0), adjustment));
    row
}

pub fn number(
    ui: &Rc<Editor>,
    knob: &Knob,
    value: f64,
    presentation: bool,
    change: impl Fn(&mut crate::model::State, f64) + 'static,
) -> (gtk::ListBoxRow, gtk::Adjustment) {
    let adjustment = gtk::Adjustment::new(value, knob.1, knob.2, knob.3, knob.3 * 10., 0.);
    let row = gtk::ListBoxRow::new();
    row.set_activatable(false);
    row.set_focusable(false);
    let outer = gtk::Box::new(gtk::Orientation::Vertical, 0);
    outer.add_css_class("adjustment");
    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let label = gtk::Label::new(Some(knob.0));
    label.set_hexpand(true);
    label.set_xalign(0.);
    line.append(&label);
    let digits = if knob.3 >= 1. {
        0
    } else if knob.3 >= 0.1 {
        1
    } else if knob.3 >= 0.01 {
        2
    } else {
        3
    };
    let spin = gtk::SpinButton::new(Some(&adjustment), knob.3, digits);
    wheel_only_when_focused(&spin);
    spin.set_width_chars(5);
    spin.set_numeric(true);
    spin.update_property(&[gtk::accessible::Property::Label(&format!(
        "{} value",
        knob.0
    ))]);
    let knob_default = knob.4;
    let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
    reset.add_css_class("flat");
    reset.set_tooltip_text(Some(&format!("Reset {} to {}{}", knob.0, knob.4, knob.5)));
    reset.update_property(&[gtk::accessible::Property::Label(&format!(
        "Reset {}",
        knob.0
    ))]);
    reset.connect_clicked(glib::clone!(
        #[weak]
        adjustment,
        move |_| adjustment.set_value(knob_default)
    ));
    let reset_slot = reset_slot(&reset);
    line.append(&reset_slot);
    let double = gtk::GestureClick::new();
    double.connect_pressed(glib::clone!(
        #[weak]
        reset,
        move |_, count, _, _| if count == 2 {
            reset.emit_clicked();
        }
    ));
    label.add_controller(double);
    line.append(&spin);
    if !knob.5.is_empty() {
        let unit = gtk::Label::new(Some(knob.5));
        unit.add_css_class("dim-label");
        line.append(&unit);
    }
    outer.append(&line);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    wheel_only_when_focused(&scale);
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.update_property(&[gtk::accessible::Property::Label(knob.0)]);
    outer.append(&scale);
    row.set_child(Some(&outer));
    reset_menu(&row, &reset);
    let update_modified = glib::clone!(
        #[weak]
        reset_slot,
        #[weak]
        label,
        move |a: &gtk::Adjustment| {
            let modified = (a.value() - knob_default).abs() > 1e-6 * (1. + knob_default.abs());
            reset_slot.set_visible_child_name(if modified { "reset" } else { "empty" });
            if modified {
                label.add_css_class("modified");
            } else {
                label.remove_css_class("modified");
            }
        }
    );
    update_modified(&adjustment);
    adjustment.connect_value_changed(update_modified);
    // Escape discards a typed value; Return/focus loss use GtkSpinButton's native commit.
    let typing_value = Rc::new(Cell::new(value));
    let focus = gtk::EventControllerFocus::new();
    focus.connect_enter(glib::clone!(
        #[weak]
        adjustment,
        #[strong]
        typing_value,
        move |_| typing_value.set(adjustment.value())
    ));
    spin.add_controller(focus);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        spin,
        #[weak]
        adjustment,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                spin.set_value(typing_value.get());
                spin.set_text(&format!("{:.1$}", adjustment.value(), digits as usize));
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));
    spin.add_controller(keys);
    let dragging = Rc::new(Cell::new(false));
    // Observe raw events without competing with GtkRange's own drag gesture.
    let gesture = gtk::EventControllerLegacy::new();
    gesture.set_propagation_phase(gtk::PropagationPhase::Capture);
    let title = format!("Change {}", knob.0);
    let step = knob.3;
    gesture.connect_event(glib::clone!(
        #[weak]
        ui,
        #[strong]
        dragging,
        #[strong]
        title,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, event| {
            match event.event_type() {
                gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin => {
                    dragging.set(true);
                    ui.runtime.borrow_mut().interacting = true;
                    ui.history.borrow_mut().begin(&title);
                }
                gtk::gdk::EventType::ButtonRelease
                | gtk::gdk::EventType::TouchEnd
                | gtk::gdk::EventType::TouchCancel => {
                    let dragging = dragging.clone();
                    let ui = ui.clone();
                    glib::idle_add_local_once(move || {
                        dragging.set(false);
                        ui.runtime.borrow_mut().interacting = false;
                        ui.history.borrow_mut().commit();
                        ui.sync_history();
                    });
                }
                _ => {}
            }
            glib::Propagation::Proceed
        }
    ));
    scale.add_controller(gesture);
    adjustment.connect_value_changed(glib::clone!(
        #[weak]
        ui,
        move |a| {
            if ui.syncing.get() {
                return;
            }
            let started = glib::monotonic_time();
            let decimal_scale = 10f64.powi(digits as i32);
            let value =
                (((a.value() / step).round() * step) * decimal_scale).round() / decimal_scale;
            let mut h = ui.history.borrow_mut();
            h.begin(&title);
            change(&mut h.state, value);
            if !dragging.get() {
                h.commit();
            }
            drop(h);
            if presentation {
                ui.canvas.apply(&ui.history.borrow().state);
            }
            ui.sync_history();
            ui.perf
                .slider_sample((glib::monotonic_time() - started) as f64 / 1000.);
        }
    ));
    (row, adjustment)
}

// Disable only the native wheel controllers while unfocused, letting events
// bubble to the scrolled panel. Consuming them would leave dead spots in it.
fn wheel_only_when_focused(widget: &impl IsA<gtk::Widget>) {
    let controllers = widget.observe_controllers();
    let scrolls: Vec<_> = (0..controllers.n_items())
        .filter_map(|i| {
            controllers
                .item(i)?
                .downcast::<gtk::EventControllerScroll>()
                .ok()
        })
        .map(|scroll| {
            let phase = scroll.propagation_phase();
            scroll.set_propagation_phase(gtk::PropagationPhase::None);
            (scroll, phase)
        })
        .collect();
    let focus = gtk::EventControllerFocus::new();
    // SpinButton delegates focus to its text child, so track the whole subtree.
    focus.connect_contains_focus_notify(move |focus| {
        for (scroll, phase) in &scrolls {
            scroll.set_propagation_phase(if focus.contains_focus() {
                *phase
            } else {
                gtk::PropagationPhase::None
            });
        }
    });
    widget.add_controller(focus);
}

fn list() -> gtk::ListBox {
    let list = gtk::ListBox::new();
    list.set_selection_mode(gtk::SelectionMode::None);
    list.add_css_class("boxed-list");
    list
}
fn section(body: &gtk::Box, name: &str) {
    let label = gtk::Label::new(Some(name));
    label.set_xalign(0.);
    label.add_css_class("section-label");
    body.append(&label);
}
pub fn combo(ui: &Rc<Editor>, list: &gtk::ListBox, title: &str, options: &[&str]) -> adw::ComboRow {
    let model = gtk::StringList::new(options);
    let row = adw::ComboRow::builder().title(title).model(&model).build();
    row.set_selected(
        ui.history
            .borrow()
            .state
            .choices
            .get(title)
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
    );
    let key = title.to_owned();
    row.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |row| {
            if ui.syncing.get() {
                return;
            }
            let mut h = ui.history.borrow_mut();
            h.begin(&format!("Change {key}"));
            h.state
                .choices
                .insert(key.clone(), row.selected().to_string());
            h.commit();
            drop(h);
            if key == "Film" {
                ui.canvas.apply(&ui.history.borrow().state);
            }
            ui.sync_history();
        }
    ));
    let default = choice_default(&ui.runtime.borrow().defaults, title, "0");
    choice_reset(ui, &row, title, &default);
    ui.choices.borrow_mut().push((title.into(), row.clone()));
    list.append(&row);
    row
}
pub fn toggle(ui: &Rc<Editor>, list: &gtk::ListBox, title: &str, active: bool) -> adw::SwitchRow {
    let active = choice_default(
        &ui.runtime.borrow().defaults,
        title,
        if active { "true" } else { "false" },
    ) == "true";
    let row = adw::SwitchRow::builder()
        .title(title)
        .active(active)
        .build();
    let key = title.to_owned();
    if let Some(v) = ui.history.borrow().state.choices.get(&key) {
        row.set_active(v == "true");
    }
    row.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| {
            if ui.syncing.get() {
                return;
            }
            let mut h = ui.history.borrow_mut();
            h.begin(&format!("Change {key}"));
            h.state
                .choices
                .insert(key.clone(), row.is_active().to_string());
            h.commit();
            drop(h);
            ui.sync_history();
        }
    ));
    choice_reset(ui, &row, title, if active { "true" } else { "false" });
    ui.toggles
        .borrow_mut()
        .push((title.into(), active, row.clone()));
    list.append(&row);
    row
}

pub fn build(ui: &Rc<Editor>) {
    let head = gtk::Box::new(gtk::Orientation::Vertical, 10);
    head.add_css_class("inspector-head");
    let title = gtk::Label::new(Some("ADJUSTMENTS"));
    title.set_xalign(0.);
    title.add_css_class("caption");
    title.add_css_class("inspector-title");
    head.append(&title);
    let histogram = gtk::DrawingArea::new();
    histogram.set_content_height(82);
    histogram.add_css_class("histogram");
    histogram.update_property(&[gtk::accessible::Property::Label(
        "RGB histogram of the camera preview",
    )]);
    histogram.set_draw_func(glib::clone!(
        #[weak]
        ui,
        move |_, cr, w, h| {
            cr.set_source_rgb(0.095, 0.098, 0.106);
            let _ = cr.paint();
            let hist = ui.hist.borrow();
            let Some(hist) = hist.as_ref() else {
                return;
            };
            let max = hist
                .iter()
                .flat_map(|h| h[1..255].iter())
                .copied()
                .max()
                .unwrap_or(1)
                .max(1) as f64;
            cr.set_operator(gtk::cairo::Operator::Add);
            for (n, color) in [
                (0, (0.92, 0.31, 0.25)),
                (1, (0.36, 0.78, 0.49)),
                (2, (0.35, 0.57, 0.94)),
            ] {
                cr.set_source_rgba(color.0, color.1, color.2, 0.48);
                cr.move_to(0., h as f64);
                for (i, v) in hist[n].iter().enumerate() {
                    cr.line_to(
                        i as f64 / 255. * w as f64,
                        h as f64 - (*v as f64 / max).min(1.) * (h as f64 - 8.),
                    );
                }
                cr.line_to(w as f64, h as f64);
                cr.close_path();
                let _ = cr.fill();
            }
        }
    ));
    ui.histogram.replace(Some(histogram.clone()));
    head.append(&histogram);
    let switch = gtk::StackSwitcher::new();
    switch.set_halign(gtk::Align::Fill);
    switch.set_hexpand(true);
    head.append(&switch);
    ui.inspector.append(&head);
    let stack = gtk::Stack::new();
    stack.set_vexpand(true);
    stack.set_transition_type(gtk::StackTransitionType::Crossfade);
    switch.set_stack(Some(&stack));
    let lab = gtk::Box::new(gtk::Orientation::Vertical, 0);
    lab.add_css_class("inspector-body");
    section(&lab, "FILM & PAPER");
    let profiles = list();
    stock(ui, &profiles, false);
    development(ui, &profiles, false);
    toggle(ui, &profiles, "Scan film directly", false);
    let paper = stock(ui, &profiles, true);
    visible(ui, &paper, |s| !flag(s, "Scan film directly", false));
    development(ui, &profiles, true);
    lab.append(&profiles);
    section(&lab, "IMAGE SIZE");
    image_size(ui, &lab);
    section(&lab, "CAMERA");
    let camera = list();
    toggle(ui, &camera, "Auto exposure", true);
    let meter = combo(
        ui,
        &camera,
        "Metering",
        &[
            "Average",
            "Median",
            "Center-Weighted",
            "Partial",
            "Matrix",
            "Multi-Zone",
            "Highlight-Weighted",
        ],
    );
    visible(ui, &meter, |s| flag(s, "Auto exposure", true));
    for k in fixtures::GROUPS[0].knobs {
        camera.append(&knob(ui, "Camera", k));
    }
    lab.append(&camera);
    let raw = list();
    combo(
        ui,
        &raw,
        "White balance",
        &["As Shot", "Daylight", "Tungsten", "Custom"],
    );
    for k in &[
        Knob("Temperature", 1667., 25000., 1., 6500., "K"),
        Knob("Tint", 0.01, 4., 0.01, 1., ""),
    ] {
        let row = knob(ui, "RAW", k);
        visible(ui, &row, |s| choice(s, "White balance") == 3);
        raw.append(&row);
    }
    let raw_caption = gtk::Label::new(Some("RAW white balance · changes re-decode the photograph"));
    raw_caption.set_wrap(true);
    raw_caption.add_css_class("caption");
    lab.append(&raw_caption);
    lab.append(&raw);
    for widget in [
        raw.upcast::<gtk::Widget>(),
        raw_caption.upcast::<gtk::Widget>(),
    ] {
        // Updated after every successful open, without rebuilding the inspector.
        let owner = Rc::downgrade(ui);
        ui.rules.borrow_mut().push(Box::new(move |_| {
            if let Some(ui) = owner.upgrade() {
                widget.set_visible(
                    ui.current_path
                        .borrow()
                        .as_deref()
                        .is_some_and(crate::photo::is_raw),
                );
            }
        }));
    }
    section(&lab, "DEVELOPMENT");
    let groups = fixtures::GROUPS
        .iter()
        .filter(|g| g.name == "Enlarger")
        .chain(
            fixtures::GROUPS
                .iter()
                .filter(|g| g.name != "Enlarger" && g.name != "Camera"),
        );
    for group in groups {
        let rows = list();
        let expander = adw::ExpanderRow::builder().title(group.name).build();
        if matches!(
            group.name,
            "Film Curves"
                | "Grain"
                | "Halation"
                | "DIR Couplers"
                | "Lens Diffusion"
                | "Print Curves"
                | "Glare"
        ) {
            let switch = gtk::Switch::new();
            switch.set_valign(gtk::Align::Center);
            let key = format!("{} enabled", group.name);
            let default = effect_default(ui, group.name);
            switch.set_active(flag(&ui.history.borrow().state, &key, default));
            switch.update_property(&[gtk::accessible::Property::Label(&format!(
                "Enable {}",
                group.name
            ))]);
            expander.add_suffix(&switch);
            let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
            reset.set_valign(gtk::Align::Center);
            reset.add_css_class("flat");
            reset.set_tooltip_text(Some("Reset effect switch to Default"));
            reset.update_property(&[gtk::accessible::Property::Label(&format!(
                "Reset {} switch",
                group.name
            ))]);
            let reset_slot = reset_slot(&reset);
            expander.add_suffix(&reset_slot);
            reset.connect_clicked(glib::clone!(
                #[weak]
                ui,
                #[strong]
                key,
                move |_| {
                    let mut h = ui.history.borrow_mut();
                    h.begin(&format!("Reset {key}"));
                    reset_choice(&mut h.state, &key, if default { "true" } else { "false" });
                    h.commit();
                    drop(h);
                    ui.sync_state();
                }
            ));
            reset_menu(&expander, &reset);
            ui.rules.borrow_mut().push(Box::new(glib::clone!(
                #[strong]
                key,
                move |s| reset_slot.set_visible_child_name(if flag(s, &key, default) != default {
                    "reset"
                } else {
                    "empty"
                })
            )));

            switch.connect_active_notify(glib::clone!(
                #[weak]
                ui,
                #[strong]
                key,
                move |w| {
                    if ui.syncing.get() {
                        return;
                    }
                    let mut h = ui.history.borrow_mut();
                    h.begin(&format!("Change {key}"));
                    h.state
                        .choices
                        .insert(key.clone(), w.is_active().to_string());
                    h.commit();
                    drop(h);
                    ui.sync_state();
                }
            ));
            ui.rules
                .borrow_mut()
                .push(Box::new(move |s| switch.set_active(flag(s, &key, default))));
        }
        let built = Rc::new(Cell::new(false));
        if group.name == "Enlarger"
            || std::env::var("NEGS_INSPECTOR_MODE").as_deref() == Ok("eager")
        {
            group_controls(ui, &expander, group);
            built.set(true);
        }
        let name = group.name.to_string();
        expander.set_expanded(ui.preferences.borrow().expanded.iter().any(|v| v == &name));
        expander.connect_expanded_notify(glib::clone!(
            #[weak]
            ui,
            #[strong]
            built,
            move |e| {
                if e.is_expanded() && !built.replace(true) {
                    group_controls(&ui, e, group);
                    ui.sync_state();
                }
                let mut prefs = ui.preferences.borrow_mut();
                prefs.expanded.retain(|n| n != &name);
                if e.is_expanded() {
                    prefs.expanded.push(name.clone());
                }
            }
        ));
        if expander.is_expanded() && !built.replace(true) {
            group_controls(ui, &expander, group);
        }
        if matches!(group.name, "Enlarger" | "Print Curves" | "Glare") {
            sensitive(ui, &expander, |s| !flag(s, "Scan film directly", false));
        }
        rows.append(&expander);
        lab.append(&rows);
    }
    section(&lab, "SCAN & COLOR");
    let color = list();
    combo(
        ui,
        &color,
        "Output gamut",
        &["OKLCh", "Off", "OKLrab", "CAM16-UCS", "ACES RGC"],
    );
    combo(ui, &color, "Input gamut", &["Off", "xy Chromaticity"]);
    toggle(ui, &color, "CAT16 adaptation", false);
    combo(
        ui,
        &color,
        "Spectral upsampling",
        &["Hanatos 2025", "Arctic 2026 (Alpha)", "Mallett 2019"],
    );
    toggle(ui, &color, "Limit preview to 12 MP", true);
    toggle(ui, &color, "Encode output as sRGB", true);
    toggle(ui, &color, "Use less memory for export", false);
    lab.append(&color);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&lab)
        .build();
    stack.add_titled(&scroll, Some("lab"), "Lab");
    let post = gtk::Box::new(gtk::Orientation::Vertical, 0);
    post.add_css_class("inspector-body");
    section(&post, "TONE & COLOR");
    let tone = list();
    for k in fixtures::POST {
        tone.append(&knob(ui, "Post", k));
    }
    post.append(&tone);
    section(&post, "FRAME");
    post.append(&crate::border_ui::build(ui));
    let postscroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&post)
        .build();
    stack.add_titled(&postscroll, Some("post"), "Post");
    ui.inspector.append(&stack);
    let footer = gtk::Box::new(gtk::Orientation::Vertical, 8);
    footer.add_css_class("inspector-footer");
    let reset = gtk::Button::with_label("Reset Adjustments");
    reset.set_action_name(Some("win.reset"));
    footer.append(&reset);
    let hint = gtk::Label::new(Some(if crate::runtime::ui_only() {
        "UI preview · film processing is not connected"
    } else {
        "Film processing · Spektrafilm"
    }));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    footer.append(&hint);
    ui.inspector.append(&footer);
    stack.set_visible_child_name(&ui.preferences.borrow().tab);
    stack.connect_visible_child_name_notify(glib::clone!(
        #[weak]
        ui,
        move |s| {
            if let Some(name) = s.visible_child_name() {
                ui.preferences.borrow_mut().tab = name.to_string();
            }
        }
    ));
    ui.tabs.replace(Some(stack));
    ui.perf
        .detail("initial_slider_count", ui.adjustments.borrow().len() + 3);
    ui.perf.mark("inspector_ready_ms");
    ui.perf.write();
}

pub fn flag(s: &crate::model::State, key: &str, default: bool) -> bool {
    s.choices.get(key).map(|v| v == "true").unwrap_or(default)
}
pub fn choice(s: &crate::model::State, key: &str) -> u32 {
    s.choices.get(key).and_then(|v| v.parse().ok()).unwrap_or(0)
}
fn visible(
    ui: &Rc<Editor>,
    widget: &impl IsA<gtk::Widget>,
    test: impl Fn(&crate::model::State) -> bool + 'static,
) {
    let w = widget.clone().upcast::<gtk::Widget>();
    ui.rules
        .borrow_mut()
        .push(Box::new(move |s| w.set_visible(test(s))));
}
fn sensitive(
    ui: &Rc<Editor>,
    widget: &impl IsA<gtk::Widget>,
    test: impl Fn(&crate::model::State) -> bool + 'static,
) {
    let w = widget.clone().upcast::<gtk::Widget>();
    ui.rules
        .borrow_mut()
        .push(Box::new(move |s| w.set_sensitive(test(s))));
}
fn group_controls(ui: &Rc<Editor>, expander: &adw::ExpanderRow, group: &'static fixtures::Group) {
    let content = list();
    let key = format!("{} enabled", group.name);
    let default = effect_default(ui, group.name);
    sensitive(ui, &content, move |s| flag(s, &key, default));
    match group.name {
        "Lens Diffusion" => {
            combo(
                ui,
                &content,
                "Filter",
                &["Black Pro-Mist", "Glimmerglass", "Pro-Mist", "CineBloom"],
            );
        }
        "Grain" => {
            toggle(ui, &content, "Sublayers", true);
        }
        "Enlarger" => {
            toggle(ui, &content, "Exposure compensation", true);
        }
        "Scanner" => {
            toggle(ui, &content, "White correction", true);
            toggle(ui, &content, "Black correction", true);
        }
        _ => {}
    }
    for k in group.knobs {
        let row = knob(ui, group.name, k);
        if k.0 == "Sublayer count" {
            sensitive(ui, &row, |s| flag(s, "Sublayers", true));
        }
        if k.0 == "White level" {
            sensitive(ui, &row, |s| flag(s, "White correction", true));
        }
        if k.0 == "Black level" {
            sensitive(ui, &row, |s| flag(s, "Black correction", true));
        }
        if matches!(
            k.0,
            "Red gamma"
                | "Green gamma"
                | "Blue gamma"
                | "Magenta shift"
                | "Yellow shift"
                | "Preflash magenta"
                | "Preflash yellow"
        ) {
            visible(ui, &row, |s| !crate::catalog::selected(s, false).bw);
        }
        content.append(&row);
    }
    expander.add_row(&content);
}
fn stock(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool) -> adw::ComboRow {
    let stocks = crate::catalog::stocks(paper);
    let labels: Vec<_> = stocks
        .iter()
        .map(|s| format!("{} · {}", s.group, s.name))
        .collect();
    let names: Vec<_> = labels.iter().map(String::as_str).collect();
    let model = gtk::SortListModel::new(Some(gtk::StringList::new(&names)), None::<gtk::Sorter>);
    model.set_section_sorter(Some(&gtk::CustomSorter::new(|a, b| {
        let rank = |item: &glib::Object| {
            let text = item.downcast_ref::<gtk::StringObject>().unwrap().string();
            crate::catalog::group_order(text.split_once(" · ").unwrap().0)
        };
        rank(a).cmp(&rank(b)).into()
    })));
    let row = adw::ComboRow::builder()
        .title(if paper { "Paper" } else { "Film" })
        .model(&model)
        .expression(gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            None::<gtk::Expression>,
            "string",
        ))
        .enable_search(true)
        .search_match_mode(gtk::StringFilterMatchMode::Substring)
        .build();
    row.set_selected(
        stocks
            .iter()
            .position(|s| s.id == crate::catalog::selected(&ui.history.borrow().state, paper).id)
            .unwrap_or(0) as u32,
    );
    let selected_factory = gtk::SignalListItemFactory::new();
    selected_factory.connect_setup(|_, o| {
        let item = o.downcast_ref::<gtk::ListItem>().unwrap();
        let label = gtk::Label::new(None);
        label.set_ellipsize(gtk::pango::EllipsizeMode::End);
        label.set_max_width_chars(23);
        item.set_child(Some(&label));
    });
    selected_factory.connect_bind(|_, o| {
        let item = o.downcast_ref::<gtk::ListItem>().unwrap();
        let text = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        item.child()
            .and_downcast::<gtk::Label>()
            .unwrap()
            .set_text(text.split_once(" · ").unwrap().1);
    });
    row.set_factory(Some(&selected_factory));
    let popup_factory = gtk::SignalListItemFactory::new();
    popup_factory.connect_setup(|_, o| {
        let item = o.downcast_ref::<gtk::ListItem>().unwrap();
        let name = gtk::Label::new(None);
        name.set_xalign(0.);
        name.set_wrap(true);
        item.set_child(Some(&name));
    });
    popup_factory.connect_bind(|_, o| {
        let item = o.downcast_ref::<gtk::ListItem>().unwrap();
        let text = item
            .item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        item.child()
            .and_downcast::<gtk::Label>()
            .unwrap()
            .set_text(text.split_once(" · ").unwrap().1);
    });
    row.set_list_factory(Some(&popup_factory));
    // Real section headers stay outside selectable rows, including during search.
    let header_factory = gtk::SignalListItemFactory::new();
    header_factory.connect_setup(|_, o| {
        let header = o.downcast_ref::<gtk::ListHeader>().unwrap();
        let label = gtk::Label::new(None);
        label.set_xalign(0.);
        label.add_css_class("heading");
        label.add_css_class("dim-label");
        label.set_margin_start(12);
        label.set_margin_end(12);
        label.set_margin_top(8);
        label.set_margin_bottom(4);
        header.set_child(Some(&label));
    });
    header_factory.connect_bind(|_, o| {
        let header = o.downcast_ref::<gtk::ListHeader>().unwrap();
        let text = header
            .item()
            .and_downcast::<gtk::StringObject>()
            .unwrap()
            .string();
        header
            .child()
            .and_downcast::<gtk::Label>()
            .unwrap()
            .set_text(text.split_once(" · ").unwrap().0);
    });
    row.set_header_factory(Some(&header_factory));
    row.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |r| {
            if ui.syncing.get() {
                return;
            }
            let Some(stock) = stocks.get(r.selected() as usize) else {
                return;
            };
            let mut h = ui.history.borrow_mut();
            h.begin(if paper { "Change Paper" } else { "Change Film" });
            h.state.choices.insert(
                if paper { "Paper ID" } else { "Film ID" }.into(),
                stock.id.clone(),
            );
            h.state.choices.remove(if paper {
                "Print development"
            } else {
                "Development"
            });
            if !paper {
                h.state
                    .choices
                    .insert("Scan film directly".into(), stock.positive.to_string());
                if !stock.target.is_empty() {
                    h.state
                        .choices
                        .insert("Paper ID".into(), stock.target.clone());
                    h.state.choices.remove("Print development");
                }
            }
            h.commit();
            drop(h);
            ui.sync_state();
        }
    ));
    let stocks = crate::catalog::stocks(paper);
    ui.rules.borrow_mut().push(Box::new(glib::clone!(
        #[weak]
        row,
        move |s| {
            row.set_selected(
                stocks
                    .iter()
                    .position(|stock| stock.id == crate::catalog::selected(s, paper).id)
                    .unwrap_or(0) as u32,
            );
        }
    )));
    list.append(&row);
    row
}
fn development(ui: &Rc<Editor>, list: &gtk::ListBox, paper: bool) {
    let title = if paper {
        "Print development"
    } else {
        "Development"
    };
    let row = combo(ui, list, title, &["Profile default"]);
    let last = Rc::new(std::cell::RefCell::new(String::new()));
    ui.rules.borrow_mut().push(Box::new(move |s| {
        let stock = crate::catalog::selected(s, paper);
        if *last.borrow() != stock.id {
            *last.borrow_mut() = stock.id.clone();
            let options: Vec<_> = std::iter::once("Profile default".to_string())
                .chain(stock.times.iter().map(|v| format!("{v} min")))
                .collect();
            row.set_model(Some(&gtk::StringList::new(
                &options.iter().map(String::as_str).collect::<Vec<_>>(),
            )));
        }
        row.set_visible(stock.times.len() > 1 && (!paper || !flag(s, "Scan film directly", false)));
        row.set_selected(choice(s, title));
    }));
}
fn image_size(ui: &Rc<Editor>, body: &gtk::Box) {
    let sizing = list();
    combo(
        ui,
        &sizing,
        "Develop at",
        &["Original size", "Longest side…"],
    );
    let row = adw::ActionRow::builder().title("Longest side").build();
    let entry = gtk::Entry::builder()
        .width_chars(7)
        .max_width_chars(9)
        .valign(gtk::Align::Center)
        .input_purpose(gtk::InputPurpose::Digits)
        .text("2048")
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(
        "Image Size longest side in pixels",
    )]);
    row.add_suffix(&entry);
    row.add_suffix(&gtk::Label::new(Some("px")));
    sizing.append(&row);
    let error = gtk::Label::new(None);
    error.set_wrap(true);
    error.set_xalign(0.);
    error.add_css_class("error");
    let readout = gtk::Label::new(None);
    readout.set_wrap(true);
    readout.set_xalign(0.);
    readout.add_css_class("caption");
    body.append(&sizing);
    body.append(&error);
    body.append(&readout);
    let commit = Rc::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        error,
        #[weak]
        entry,
        move || {
            let source = ui.canvas.source_size();
            let crop = ui.history.borrow().state.crop;
            match crate::image_size::validate(&entry.text(), source, crop) {
                Ok(value) => {
                    entry.remove_css_class("error");
                    error.set_text("");
                    let mut h = ui.history.borrow_mut();
                    h.begin("Change Image Size");
                    h.state
                        .values
                        .insert("Image Size.Longest side".into(), value as f64);
                    h.commit();
                    drop(h);
                    ui.sync_state();
                }
                Err(message) => {
                    error.set_text(&message);
                    entry.add_css_class("error");
                }
            }
        }
    ));
    entry.connect_activate(glib::clone!(
        #[strong]
        commit,
        move |_| commit()
    ));
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave(move |_| commit());
    entry.add_controller(focus);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        ui,
        #[weak]
        entry,
        #[weak]
        error,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape {
                entry.set_text(&format!(
                    "{:.0}",
                    ui.history
                        .borrow()
                        .state
                        .values
                        .get("Image Size.Longest side")
                        .copied()
                        .unwrap_or(2048.)
                ));
                entry.remove_css_class("error");
                error.set_text("");
                return glib::Propagation::Stop;
            }
            glib::Propagation::Proceed
        }
    ));
    entry.add_controller(keys);
    let owner = Rc::downgrade(ui);
    ui.rules.borrow_mut().push(Box::new(move |s| {
        let Some(ui) = owner.upgrade() else {
            return;
        };
        row.set_visible(choice(s, "Develop at") == 1);
        let target = s
            .values
            .get("Image Size.Longest side")
            .copied()
            .unwrap_or(2048.) as u32;
        if !entry.has_focus() && !entry.has_css_class("error") {
            entry.set_text(&target.to_string());
        }
        if ui.current_path.borrow().is_none() {
            readout.set_text("Resizes before film simulation, including halation and grain.");
            return;
        }
        let (working, photo) = crate::image_size::project(
            ui.canvas.source_size(),
            s.crop,
            (choice(s, "Develop at") == 1).then_some(target),
        );
        let source = ui.canvas.source_size();
        let r=ui.runtime.borrow();
        let note=if crate::runtime::ui_only(){"This preview still displays the camera image.".to_string()}
            else if let Some(frame)=r.developed.as_ref().filter(|_|r.current_revision==Some(r.revision)){
                let full=frame.result["working_width"].as_f64().unwrap_or(frame.width as f64);
                let percent=(frame.width as f64/full*100.).min(100.);
                if percent<99.9 {format!("Preview shown at {percent:.0}%. Exports develop all pixels; inspect grain in an export.")}else{String::new()}
            }else{"Developing the current adjustments…".into()};
        readout.set_text(&format!(
            "Source {} × {} px
Working {} × {} px · Photograph {} × {} px
Resizes before film simulation. {note}",
            source.0, source.1, working.0, working.1, photo.0, photo.1
        ));
    }));
}

// A homogeneous stack measures both children, reserving the reset button's space
// even at the default value. The empty page has no focus or pointer target.
fn reset_slot(reset: &gtk::Button) -> gtk::Stack {
    let slot = gtk::Stack::new();
    slot.set_valign(gtk::Align::Center);
    slot.add_named(reset, Some("reset"));
    slot.add_named(&gtk::Box::new(gtk::Orientation::Horizontal, 0), Some("empty"));
    slot.set_visible_child_name("empty");
    slot
}

pub(crate) fn reset_menu(widget: &impl IsA<gtk::Widget>, reset: &gtk::Button) {
    let widget = widget.as_ref().clone();
    let click = gtk::GestureClick::new();
    click.set_button(3);
    click.connect_pressed(glib::clone!(
        #[weak]
        reset,
        #[weak(rename_to=widget)]
        widget,
        move |_, _, x, y| {
            let popover = gtk::Popover::new();
            popover.set_parent(&widget);
            popover.set_pointing_to(Some(&gtk::gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
            let button = gtk::Button::with_label("Reset to Default");
            button.set_sensitive(reset.is_visible() && reset.is_child_visible());
            button.connect_clicked(glib::clone!(
                #[weak]
                reset,
                #[weak]
                popover,
                move |_| {
                    reset.emit_clicked();
                    popover.popdown();
                }
            ));
            popover.set_child(Some(&button));
            popover.connect_closed(|p| p.unparent());
            popover.popup();
        }
    ));
    widget.add_controller(click);
}
fn choice_reset(ui: &Rc<Editor>, row: &impl IsA<adw::ActionRow>, key: &str, default: &str) {
    let reset = gtk::Button::from_icon_name("edit-undo-symbolic");
    reset.set_valign(gtk::Align::Center);
    reset.add_css_class("flat");
    reset.set_tooltip_text(Some("Reset to Default"));
    reset.update_property(&[gtk::accessible::Property::Label(&format!("Reset {key}"))]);
    let reset_slot = reset_slot(&reset);
    row.add_suffix(&reset_slot);
    let key = key.to_string();
    let default = default.to_string();
    reset.connect_clicked(glib::clone!(
        #[weak]
        ui,
        #[strong]
        key,
        #[strong]
        default,
        move |_| {
            let mut h = ui.history.borrow_mut();
            h.begin(&format!("Reset {key}"));
            reset_choice(&mut h.state, &key, &default);
            h.commit();
            drop(h);
            ui.sync_state();
        }
    ));
    let widget = row.clone().upcast::<adw::ActionRow>();
    reset_menu(&widget, &reset);
    ui.rules.borrow_mut().push(Box::new(move |s| {
        let modified = s.choices.get(&key).is_some_and(|v| v != &default);
        reset_slot.set_visible_child_name(if modified { "reset" } else { "empty" });
        if modified {
            widget.add_css_class("modified");
        } else {
            widget.remove_css_class("modified");
        }
    }));
}

/// Resolve the same value for reset, display and runtime translation. Keeping an
/// explicit choice matters when a restored full RuntimeParams has another value.
fn choice_default(defaults: &serde_json::Value, key: &str, fallback: &str) -> String {
    if key == "Limit preview to 12 MP" && defaults.is_object() {
        return "false".into();
    }
    if let Some(value) = crate::parameters::TOGGLES
        .iter()
        .find(|(name, _)| *name == key)
        .and_then(|(_, path)| crate::parameters::get(defaults, path).as_bool())
    {
        return value.to_string();
    }
    crate::parameters::CHOICES
        .iter()
        .find(|(name, _, _)| *name == key)
        .and_then(|(_, path, options)| {
            options
                .iter()
                .position(|&option| crate::parameters::get(defaults, path).as_str() == Some(option))
        })
        .map(|index| index.to_string())
        .unwrap_or_else(|| fallback.into())
}
fn reset_choice(state: &mut crate::model::State, key: &str, value: &str) {
    state.choices.insert(key.into(), value.into());
}
fn effect_default(ui: &Editor, group: &str) -> bool {
    let fallback = if matches!(group, "Lens Diffusion" | "Film Curves" | "Print Curves") {
        "false"
    } else {
        "true"
    };
    choice_default(
        &ui.runtime.borrow().defaults,
        &format!("{group} enabled"),
        fallback,
    ) == "true"
}

#[cfg(test)]
mod reset_tests {
    use super::{choice_default, reset_choice};
    use serde_json::json;

    #[test]
    fn reset_choice_overrides_restored_params_and_keeps_display_in_sync() {
        let defaults = json!({"camera":{"auto_exposure_method":"center_weighted"},"scanner":{"white_correction":false},"settings":{"use_cat16":true}});
        let mut state = crate::model::State {
            params: json!({"camera":{"auto_exposure_method":"median"},"scanner":{"white_correction":true},"settings":{"use_cat16":false}}),
            ..Default::default()
        };
        for (key, fallback) in [
            ("Metering", "0"),
            ("White correction", "true"),
            ("CAT16 adaptation", "false"),
        ] {
            let value = choice_default(&defaults, key, fallback);
            reset_choice(&mut state, key, &value);
        }
        let params = crate::parameters::runtime(&state, &defaults);
        assert_eq!(state.choices["Metering"], "2");
        assert_eq!(params["camera"]["auto_exposure_method"], "center_weighted");
        assert_eq!(state.choices["White correction"], "false");
        assert_eq!(params["scanner"]["white_correction"], false);
        assert_eq!(state.choices["CAT16 adaptation"], "true");
        assert_eq!(params["settings"]["use_cat16"], true);
    }

    #[test]
    fn preview_limit_and_effect_reset_follow_connected_defaults() {
        let defaults = json!({"film_render":{"grain":{"active":false}}});
        assert_eq!(
            choice_default(&defaults, "Limit preview to 12 MP", "true"),
            "false"
        );
        assert_eq!(choice_default(&defaults, "Grain enabled", "true"), "false");
        assert_eq!(
            choice_default(&serde_json::Value::Null, "Limit preview to 12 MP", "true"),
            "true"
        );
    }
}
