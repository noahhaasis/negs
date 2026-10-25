use crate::{
    Editor,
    fixtures::{self, Knob},
};
use adw::prelude::*;
use gtk::glib;
use std::{cell::Cell, rc::Rc};

pub fn knob(ui: &Rc<Editor>, group: &str, knob: &Knob) -> gtk::ListBoxRow {
    let key = format!("{group}.{}", knob.0);
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
    spin.set_width_chars(5);
    spin.set_numeric(true);
    spin.update_property(&[gtk::accessible::Property::Label(&format!(
        "{} value",
        knob.0
    ))]);
    line.append(&spin);
    if !knob.5.is_empty() {
        let unit = gtk::Label::new(Some(knob.5));
        unit.add_css_class("dim-label");
        line.append(&unit);
    }
    outer.append(&line);
    let scale = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&adjustment));
    scale.set_draw_value(false);
    scale.set_hexpand(true);
    scale.update_property(&[gtk::accessible::Property::Label(knob.0)]);
    outer.append(&scale);
    row.set_child(Some(&outer));
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
                    ui.history.borrow_mut().begin(&title);
                }
                gtk::gdk::EventType::ButtonRelease
                | gtk::gdk::EventType::TouchEnd
                | gtk::gdk::EventType::TouchCancel => {
                    let dragging = dragging.clone();
                    let ui = ui.clone();
                    glib::idle_add_local_once(move || {
                        dragging.set(false);
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
            let value = (a.value() / step).round() * step;
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
fn combo(ui: &Rc<Editor>, list: &gtk::ListBox, title: &str, options: &[&str]) {
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
    ui.choices.borrow_mut().push((title.into(), row.clone()));
    list.append(&row);
}
fn toggle(ui: &Rc<Editor>, list: &gtk::ListBox, title: &str, active: bool) {
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
    ui.toggles
        .borrow_mut()
        .push((title.into(), active, row.clone()));
    list.append(&row);
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
    combo(
        ui,
        &profiles,
        "Film",
        &[
            "Kodak Portra 400",
            "Kodak Gold 200",
            "Kodak Ektar 100",
            "Fujifilm Velvia 100",
            "Kodak Tri-X 400",
            "Kodak Double-X",
        ],
    );
    toggle(ui, &profiles, "Scan film directly", false);
    combo(
        ui,
        &profiles,
        "Paper",
        &[
            "Kodak Portra Endura",
            "Kodak Ultra Endura",
            "Fujifilm Crystal Archive",
            "Ilford Multigrade RC",
        ],
    );
    lab.append(&profiles);
    section(&lab, "DEVELOPMENT");
    let groups = fixtures::GROUPS
        .iter()
        .filter(|g| g.name == "Enlarger")
        .chain(fixtures::GROUPS.iter().filter(|g| g.name != "Enlarger"));
    for group in groups {
        let rows = list();
        let expander = adw::ExpanderRow::builder().title(group.name).build();
        let built = Rc::new(Cell::new(false));
        if group.name == "Camera"
            || group.name == "Enlarger"
            || std::env::var("NEGS_INSPECTOR_MODE").as_deref() == Ok("eager")
        {
            if group.name == "Camera" {
                toggle(ui, &list_in_expander(&expander), "Auto exposure", true);
            }
            for k in group.knobs {
                expander.add_row(&knob(ui, group.name, k));
            }
            built.set(true);
        }
        expander.connect_expanded_notify(glib::clone!(
            #[weak]
            ui,
            move |e| {
                if e.is_expanded() && !built.replace(true) {
                    for k in group.knobs {
                        e.add_row(&knob(&ui, group.name, k));
                    }
                }
            }
        ));
        expander.set_expanded(group.name == "Enlarger");
        rows.append(&expander);
        lab.append(&rows);
    }
    section(&lab, "IMAGE SIZE");
    let sizing = list();
    combo(
        ui,
        &sizing,
        "Develop at",
        &["Original size", "Longest side…"],
    );
    lab.append(&sizing);
    section(&lab, "SCAN & COLOR");
    let color = list();
    combo(
        ui,
        &color,
        "Output gamut",
        &["OKLCh", "Off", "OKLrab", "CAM16-UCS", "ACES RGC"],
    );
    combo(
        ui,
        &color,
        "Spectral upsampling",
        &["Hanatos 2025", "Arctic 2026 (Alpha)", "Mallett 2019"],
    );
    toggle(ui, &color, "Limit preview to 12 MP", true);
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
    let hint = gtk::Label::new(Some("UI preview · film processing is not connected"));
    hint.add_css_class("dim-label");
    hint.add_css_class("caption");
    footer.append(&hint);
    ui.inspector.append(&footer);
    ui.tabs.replace(Some(stack));
    ui.perf
        .detail("initial_slider_count", ui.adjustments.borrow().len() + 3);
    ui.perf.mark("inspector_ready_ms");
    ui.perf.write();
}

fn list_in_expander(expander: &adw::ExpanderRow) -> gtk::ListBox {
    // A separate boxed list keeps native switch rows available inside an expander.
    let list = list();
    expander.add_row(&list);
    list
}
