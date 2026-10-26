use crate::Editor;
use adw::prelude::*;
use gtk::glib;
use std::{cell::Cell, rc::Rc};

fn source_portrait(state: &crate::model::State, source: (i32, i32)) -> bool {
    if state.rotation % 2 == 1 {
        source.0 > source.1
    } else {
        source.1 > source.0
    }
}

fn aspect_ratio(index: u32, source: (i32, i32), portrait: bool, custom: (f64, f64)) -> f64 {
    let ratio = match index {
        0 => source.0.max(source.1).max(1) as f64 / source.0.min(source.1).max(1) as f64,
        1 => 0.,
        2 => 1.,
        3 => 16. / 9.,
        4 => 1.5,
        5 => 4. / 3.,
        6 => 1.25,
        7 => 1.4,
        _ => return custom.0 / custom.1,
    };
    if portrait && ratio > 0. {
        1. / ratio
    } else {
        ratio
    }
}

pub fn build(ui: &Rc<Editor>) {
    ui.crop_bar.set_orientation(gtk::Orientation::Vertical);
    ui.crop_bar.set_halign(gtk::Align::Fill);
    let line = gtk::FlowBox::new();
    line.set_selection_mode(gtk::SelectionMode::None);
    line.set_homogeneous(false);
    line.set_column_spacing(6);
    line.set_row_spacing(6);
    line.set_min_children_per_line(1);
    line.set_max_children_per_line(6);
    let ratios = gtk::DropDown::from_strings(&[
        "Original",
        "Freeform",
        "Square · 6×6",
        "16:9",
        "3:2 · 35mm",
        "4:3 · 645",
        "5:4 · 4×5",
        "7:5 · 5×7",
        "Custom",
    ]);
    ratios.set_hexpand(true);
    ratios.update_property(&[gtk::accessible::Property::Label("Crop aspect ratio")]);
    let portrait = gtk::ToggleButton::with_label("Portrait");
    portrait.set_tooltip_text(Some("Swap between landscape and portrait aspect ratios"));
    line.insert(&ratios, -1);
    line.insert(&portrait, -1);
    let custom = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let width = gtk::SpinButton::with_range(0.01, 1000., 0.01);
    width.set_value(5.);
    width.set_width_chars(4);
    let height = gtk::SpinButton::with_range(0.01, 1000., 0.01);
    height.set_value(4.);
    height.set_width_chars(4);
    width.update_property(&[gtk::accessible::Property::Label("Custom crop ratio width")]);
    height.update_property(&[gtk::accessible::Property::Label("Custom crop ratio height")]);
    custom.append(&width);
    custom.append(&gtk::Label::new(Some(":")));
    custom.append(&height);
    custom.set_visible(false);
    line.insert(&custom, -1);
    let reset = gtk::Button::with_label("Reset");
    reset.set_tooltip_text(Some("Reset crop and straighten"));
    let a = gtk::Adjustment::new(0., -45., 45., 0.1, 0.5, 0.);
    line.insert(&reset, -1);
    for (title, action) in [("Cancel", "win.crop-cancel"), ("Done", "win.crop-done")] {
        let button = gtk::Button::with_label(title);
        button.set_action_name(Some(action));
        if title == "Done" {
            button.add_css_class("suggested-action");
        }
        line.insert(&button, -1);
    }
    ui.crop_bar.append(&line);
    let angle_line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    angle_line.append(&gtk::Label::new(Some("Straighten")));

    let dial = gtk::Scale::new(gtk::Orientation::Horizontal, Some(&a));
    dial.set_draw_value(false);
    dial.set_hexpand(true);
    dial.update_property(&[gtk::accessible::Property::Label("Straighten angle")]);
    let spin = gtk::SpinButton::new(Some(&a), 0.1, 1);
    spin.set_width_chars(4);
    spin.update_property(&[gtk::accessible::Property::Label("Straighten degrees")]);
    angle_line.append(&dial);
    angle_line.append(&spin);
    angle_line.append(&gtk::Label::new(Some("°")));
    let size = gtk::Label::new(None);
    size.add_css_class("caption");
    size.set_width_chars(17);
    angle_line.append(&size);
    ui.crop_bar.append(&angle_line);
    let help = gtk::Label::new(Some(
        "Drag edges or corners; drag inside to move. Arrow keys nudge. Enter applies · Escape cancels.",
    ));
    help.set_wrap(true);
    help.add_css_class("caption");
    ui.crop_bar.append(&help);
    let apply = Rc::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        ratios,
        #[weak]
        portrait,
        #[weak]
        width,
        #[weak]
        height,
        #[weak]
        custom,
        move || {
            if ui.syncing.get() || !ui.canvas.cropping() {
                return;
            }
            let index = ratios.selected();
            custom.set_visible(index == 8);
            ui.canvas.crop_ratio(aspect_ratio(
                index,
                ui.canvas.source_size(),
                portrait.is_active(),
                (width.value(), height.value()),
            ));
            let mut h = ui.history.borrow_mut();
            h.state
                .choices
                .insert("Crop aspect".into(), index.to_string());
            h.state
                .choices
                .insert("Crop portrait".into(), portrait.is_active().to_string());
            h.state.values.insert("Crop width".into(), width.value());
            h.state.values.insert("Crop height".into(), height.value());
        }
    ));
    reset.connect_clicked(glib::clone!(
        #[weak]
        ui,
        #[weak]
        ratios,
        #[weak]
        portrait,
        #[weak]
        a,
        #[strong]
        apply,
        move |_| {
            ui.syncing.set(true);
            a.set_value(0.);
            ratios.set_selected(0);
            portrait.set_active(source_portrait(
                &ui.history.borrow().state,
                ui.canvas.source_size(),
            ));
            ui.syncing.set(false);
            apply();
            // Keep Reset's full frame exact, without aspect-fit rounding.
            ui.canvas.reset_crop();
            let source = ui.canvas.source_size();
            ui.canvas
                .lock_ratio(source.0.max(1) as f64 / source.1.max(1) as f64);
        }
    ));
    ratios.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[weak]
        portrait,
        #[strong]
        apply,
        move |ratios| {
            if !ui.syncing.get() && ratios.selected() == 0 {
                ui.syncing.set(true);
                portrait.set_active(source_portrait(
                    &ui.history.borrow().state,
                    ui.canvas.source_size(),
                ));
                ui.syncing.set(false);
            }
            apply();
        }
    ));
    portrait.connect_toggled(glib::clone!(
        #[weak]
        ui,
        #[weak]
        ratios,
        #[weak]
        width,
        #[weak]
        height,
        #[strong]
        apply,
        move |_| {
            if !ui.syncing.get() && ratios.selected() == 8 {
                ui.syncing.set(true);
                let value = width.value();
                width.set_value(height.value());
                height.set_value(value);
                ui.syncing.set(false);
            }
            apply();
        }
    ));
    width.connect_value_changed(glib::clone!(
        #[strong]
        apply,
        move |_| apply()
    ));
    height.connect_value_changed(move |_| apply());
    a.connect_value_changed(glib::clone!(
        #[weak]
        ui,
        move |a| {
            if !ui.syncing.get() && ui.canvas.cropping() {
                ui.canvas.set_angle(a.value());
            }
        }
    ));
    ui.rules.borrow_mut().push(Box::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        portrait,
        #[weak]
        width,
        #[weak]
        height,
        move |s| {
            portrait.set_active(crate::inspector::flag(
                s,
                "Crop portrait",
                source_portrait(s, ui.canvas.source_size()),
            ));
            width.set_value(s.values.get("Crop width").copied().unwrap_or(5.));
            height.set_value(s.values.get("Crop height").copied().unwrap_or(4.));
        }
    )));
    // The crop readout follows pointer drags without waking the main loop while idle.
    let was = Rc::new(Cell::new(false));
    ui.crop_bar.connect_map(glib::clone!(
        #[weak]
        ui,
        #[weak]
        a,
        #[weak]
        ratios,
        #[weak]
        portrait,
        #[weak]
        width,
        #[weak]
        height,
        #[weak]
        custom,
        #[weak]
        reset,
        move |_| {
            let state = ui.history.borrow().state.clone();
            ui.syncing.set(true);
            a.set_value(state.straighten);
            ratios.set_selected(crate::inspector::choice(&state, "Crop aspect"));
            portrait.set_active(crate::inspector::flag(
                &state,
                "Crop portrait",
                source_portrait(&state, ui.canvas.source_size()),
            ));
            width.set_value(state.values.get("Crop width").copied().unwrap_or(5.));
            height.set_value(state.values.get("Crop height").copied().unwrap_or(4.));
            custom.set_visible(ratios.selected() == 8);
            let source = ui.canvas.source_size();
            let shown = aspect_ratio(
                ratios.selected(),
                source,
                portrait.is_active(),
                (width.value(), height.value()),
            );
            let source_ratio = if state.rotation % 2 == 1 && shown > 0. {
                1. / shown
            } else {
                shown
            };
            ui.canvas.lock_ratio(source_ratio);
            ui.syncing.set(false);
            if was.replace(true) {
                return;
            }
            ui.crop_bar.add_tick_callback(glib::clone!(
                #[weak]
                ui,
                #[weak]
                size,
                #[weak]
                reset,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move |_, _| {
                    let source = ui.canvas.source_size();
                    let r = ui.canvas.crop_draft();
                    reset.set_sensitive(r != [0., 0., 1., 1.] || ui.canvas.draft_angle() != 0.);
                    let state = ui.history.borrow();
                    let target =
                        (crate::inspector::choice(&state.state, "Develop at") == 1).then(|| {
                            state
                                .state
                                .values
                                .get("Image Size.Longest side")
                                .copied()
                                .unwrap_or(2048.) as u32
                        });
                    let (_, photo) = crate::image_size::project(source, Some(r), target);
                    let photo = if state.state.rotation % 2 == 1 {
                        (photo.1, photo.0)
                    } else {
                        photo
                    };
                    size.set_text(&format!("{} × {} px", photo.0, photo.1));
                    glib::ControlFlow::Continue
                }
            ));
        }
    ));
}
