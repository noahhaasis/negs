use crate::{
    Editor,
    border::{Color, Settings, Style},
    fixtures::Knob,
    inspector,
};
use adw::prelude::*;
use gtk::glib;
use std::rc::Rc;

pub struct Controls {
    enabled: adw::SwitchRow,
    style: adw::ComboRow,
    options: gtk::ListBox,
    width: (gtk::ListBoxRow, gtk::Adjustment),
    line: (gtk::ListBoxRow, gtk::Adjustment),
    film_width: (gtk::ListBoxRow, gtk::Adjustment),
    color: (adw::ActionRow, gtk::Entry, gtk::ColorDialogButton),
    line_color: (adw::ActionRow, gtk::Entry, gtk::ColorDialogButton),
    instagram: adw::SwitchRow,
    export: adw::SwitchRow,
}
impl Controls {
    pub fn sync(&self, s: &Settings) {
        self.enabled.set_active(s.enabled);
        self.style.set_selected(s.style.index());
        self.options.set_sensitive(s.enabled);
        let mat = matches!(s.style, Style::Solid | Style::Keyline);
        let line = s.style == Style::Keyline;
        self.width.0.set_visible(mat);
        self.width.1.set_value(s.width * 100.);
        self.line.0.set_visible(line);
        self.line.1.set_value(s.line * 100.);
        self.film_width.0.set_visible(s.style == Style::Film);
        self.film_width.1.set_value(s.film_width * 100.);
        for ((row, entry, picker), color, visible) in [
            (&self.color, s.color, mat),
            (&self.line_color, s.line_color, line),
        ] {
            row.set_visible(visible);
            entry.set_text(&color.hex());
            entry.remove_css_class("error");
            picker.set_rgba(&color.rgba());
        }
        self.instagram.set_active(s.instagram);
        self.export.set_active(s.export);
    }
}

fn update(ui: &Rc<Editor>, name: &str, edit: impl FnOnce(&mut Settings)) {
    if ui.syncing.get() {
        return;
    }
    let mut h = ui.history.borrow_mut();
    h.begin(name);
    edit(&mut h.state.border);
    h.commit();
    drop(h);
    ui.sync_state();
}
fn color_row(
    ui: &Rc<Editor>,
    title: &str,
    line: bool,
    color: Color,
) -> (adw::ActionRow, gtk::Entry, gtk::ColorDialogButton) {
    let row = adw::ActionRow::builder().title(title).build();
    let entry = gtk::Entry::builder()
        .text(color.hex())
        .max_length(7)
        .width_chars(8)
        .max_width_chars(8)
        .valign(gtk::Align::Center)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&format!("{title} hex"))]);
    entry.set_tooltip_text(Some("8-bit sRGB color, #rrggbb. Press Enter to apply."));
    let dialog = gtk::ColorDialog::builder()
        .title(title)
        .with_alpha(false)
        .build();
    let picker = gtk::ColorDialogButton::new(Some(dialog));
    picker.set_valign(gtk::Align::Center);
    picker.set_rgba(&color.rgba());
    picker.update_property(&[gtk::accessible::Property::Label(title)]);
    let commit = Rc::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        entry,
        move || {
            match Color::try_from(entry.text().to_string()) {
                Ok(color) => apply_color(&ui, line, color),
                Err(_) => entry.add_css_class("error"),
            }
        }
    ));
    entry.connect_activate({
        let commit = commit.clone();
        move |_| commit()
    });
    let focus = gtk::EventControllerFocus::new();
    focus.connect_leave({
        let commit = commit.clone();
        move |_| commit()
    });
    entry.add_controller(focus);
    let apply = gtk::Button::from_icon_name("object-select-symbolic");
    apply.add_css_class("flat");
    apply.set_valign(gtk::Align::Center);
    apply.set_tooltip_text(Some(&format!("Apply {title}")));
    apply.update_property(&[gtk::accessible::Property::Label(&format!("Apply {title}"))]);
    apply.connect_clicked(move |_| commit());
    picker.connect_rgba_notify(glib::clone!(
        #[weak]
        ui,
        move |picker| {
            apply_color(&ui, line, Color::from_rgba(picker.rgba()));
        }
    ));
    row.add_suffix(&entry);
    row.add_suffix(&apply);
    row.add_suffix(&picker);
    reset_control(
        ui,
        &row,
        title,
        move |s| {
            if line {
                s.line_color != Settings::default().line_color
            } else {
                s.color != Settings::default().color
            }
        },
        move |s| {
            if line {
                s.line_color = Settings::default().line_color
            } else {
                s.color = Settings::default().color
            }
        },
    );
    (row, entry, picker)
}
pub fn build(ui: &Rc<Editor>) -> gtk::Box {
    let state = ui.history.borrow().state.border.clone();
    let body = gtk::Box::new(gtk::Orientation::Vertical, 8);
    let enabled = adw::SwitchRow::builder()
        .title("Add border")
        .active(state.enabled)
        .build();
    let heading = gtk::ListBox::new();
    heading.set_selection_mode(gtk::SelectionMode::None);
    heading.add_css_class("boxed-list");
    reset_control(
        ui,
        &enabled,
        "Add border",
        |s| s.enabled,
        |s| s.enabled = false,
    );
    heading.append(&enabled);
    body.append(&heading);
    let options = gtk::ListBox::new();
    options.set_selection_mode(gtk::SelectionMode::None);
    options.add_css_class("boxed-list");
    let style = adw::ComboRow::builder()
        .title("Border style")
        .model(&gtk::StringList::new(&["Solid", "Film", "Keyline", "35mm"]))
        .selected(state.style.index())
        .build();
    enabled.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| update(&ui, "Change Border", |s| s.enabled = row.is_active())
    ));
    style.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |row| {
            if !ui.syncing.get() && row.selected() == 3 && !fits_35mm(&ui) {
                ui.toast.add_toast(adw::Toast::new(
                    "35mm needs a 3:2 or 3:4 photograph. Crop to 3:2 or 4:3 first.",
                ));
                ui.sync_state();
                return;
            }
            update(&ui, "Change Border Style", |s| {
                s.style = Style::from_index(row.selected())
            });
        }
    ));
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(|_, o| {
        let item = o.downcast_ref::<gtk::ListItem>().unwrap();
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        let swatch = gtk::DrawingArea::new();
        swatch.set_size_request(56, 40);
        b.append(&swatch);
        b.append(&gtk::Label::new(None));
        item.set_child(Some(&b));
    });
    factory.connect_bind(glib::clone!(
        #[weak]
        ui,
        move |_, o| {
            let item = o.downcast_ref::<gtk::ListItem>().unwrap();
            let index = item.position();
            let b = item.child().and_downcast::<gtk::Box>().unwrap();
            b.last_child()
                .and_downcast::<gtk::Label>()
                .unwrap()
                .set_text(
                    ["Solid", "Film", "Keyline", "35mm"]
                        .get(index as usize)
                        .copied()
                        .unwrap_or(""),
                );
            let compatible = index != 3 || fits_35mm(&ui);
            item.set_selectable(compatible);
            item.set_activatable(compatible);
            b.set_sensitive(compatible);
            b.first_child()
                .and_downcast::<gtk::DrawingArea>()
                .unwrap()
                .set_draw_func(move |_, cr, w, h| {
                    cr.set_source_rgb(
                        if index == 0 || index == 2 { 0.94 } else { 0.04 },
                        if index == 0 || index == 2 { 0.92 } else { 0.04 },
                        if index == 0 || index == 2 { 0.87 } else { 0.04 },
                    );
                    let _ = cr.paint();
                    if index == 2 {
                        cr.set_source_rgb(0.05, 0.05, 0.05);
                        cr.rectangle(6., 6., w as f64 - 12., h as f64 - 12.);
                        let _ = cr.fill();
                    }
                    cr.set_source_rgb(0.28, 0.42, 0.48);
                    cr.rectangle(8., 8., w as f64 - 16., h as f64 - 16.);
                    let _ = cr.fill();
                    if index == 3 {
                        for x in (4..w).step_by(9) {
                            cr.set_source_rgb(0.75, 0.7, 0.6);
                            cr.rectangle(x as f64, 2., 5., 3.);
                            cr.rectangle(x as f64, h as f64 - 5., 5., 3.);
                            let _ = cr.fill();
                        }
                    }
                });
        }
    ));
    style.set_list_factory(Some(&factory));
    reset_control(
        ui,
        &style,
        "Border style",
        |s| s.style != Settings::default().style,
        |s| s.style = Settings::default().style,
    );
    options.append(&style);
    let width = inspector::number(
        ui,
        &Knob("Mat thickness", 0., 15., 0.1, 3., "%"),
        state.width * 100.,
        true,
        |s, v| s.border.width = v / 100.,
    );
    let line = inspector::number(
        ui,
        &Knob("Line thickness", 0.05, 2., 0.05, 0.4, "%"),
        state.line * 100.,
        true,
        |s, v| s.border.line = v / 100.,
    );
    let film_width = inspector::number(
        ui,
        &Knob("Outer black thickness", 0., 15., 0.1, 0., "%"),
        state.film_width * 100.,
        true,
        |s, v| s.border.film_width = v / 100.,
    );
    for row in [&width.0, &line.0, &film_width.0] {
        options.append(row);
    }
    let color = color_row(ui, "Mat color", false, state.color);
    let line_color = color_row(ui, "Line color", true, state.line_color);
    options.append(&color.0);
    options.append(&line_color.0);
    let instagram = adw::SwitchRow::builder()
        .title("Extend to 3:4 for Instagram")
        .active(state.instagram)
        .build();
    instagram.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| update(&ui, "Change Instagram Frame", |s| s.instagram =
            row.is_active())
    ));
    reset_control(
        ui,
        &instagram,
        "Instagram frame",
        |s| s.instagram,
        |s| s.instagram = false,
    );
    options.append(&instagram);
    let export = adw::SwitchRow::builder()
        .title("Include border in export")
        .active(state.export)
        .build();
    export.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |row| update(&ui, "Change Border Export", |s| s.export = row.is_active())
    ));
    export.set_subtitle("Saved for when export is connected");
    reset_control(
        ui,
        &export,
        "Border export",
        |s| !s.export,
        |s| s.export = true,
    );
    options.append(&export);
    body.append(&options);
    let warning = gtk::Label::new(Some(
        "35mm needs a 3:2 or 3:4 photograph. Crop to 3:2 or 4:3 to use it.",
    ));
    warning.set_wrap(true);
    warning.set_xalign(0.);
    warning.add_css_class("caption");
    body.append(&warning);
    let owner = Rc::downgrade(ui);
    ui.rules.borrow_mut().push(Box::new(move |s| {
        if let Some(ui) = owner.upgrade() {
            warning.set_visible(s.border.enabled && !fits_35mm(&ui));
        }
    }));
    let help = gtk::Label::new(Some(
        "Thickness is a percentage of the photograph’s long edge. 35mm needs a 3:2 or 4:3 crop; other ratios show the photograph without that border.",
    ));
    help.set_wrap(true);
    help.set_xalign(0.);
    help.add_css_class("caption");
    help.add_css_class("dim-label");
    body.append(&help);
    let controls = Controls {
        enabled,
        style,
        options,
        width,
        line,
        film_width,
        color,
        line_color,
        instagram,
        export,
    };
    controls.sync(&state);
    ui.border.replace(Some(controls));
    body
}

fn fits_35mm(ui: &Editor) -> bool {
    let (w, h) = ui.canvas.source_size();
    let crop = ui.history.borrow().state.crop.unwrap_or([0., 0., 1., 1.]);
    w <= 0 || h <= 0 || crate::border::format(w as f64 * crop[2], h as f64 * crop[3]).is_some()
}

fn reset_control(
    ui: &Rc<Editor>,
    row: &impl IsA<adw::ActionRow>,
    name: &str,
    modified: impl Fn(&Settings) -> bool + 'static,
    change: impl Fn(&mut Settings) + 'static,
) {
    let button = gtk::Button::from_icon_name("edit-undo-symbolic");
    button.add_css_class("flat");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Reset to Default"));
    button.update_property(&[gtk::accessible::Property::Label(&format!("Reset {name}"))]);
    row.add_suffix(&button);
    let name = name.to_string();
    button.connect_clicked(glib::clone!(
        #[weak]
        ui,
        move |_| update(&ui, &format!("Reset {name}"), &change)
    ));
    inspector::reset_menu(&row.clone().upcast::<adw::ActionRow>(), &button);
    ui.rules
        .borrow_mut()
        .push(Box::new(move |s| button.set_visible(modified(&s.border))));
}
fn apply_color(ui: &Rc<Editor>, line: bool, color: Color) {
    if ui.syncing.get() {
        return;
    }
    let mut h = ui.history.borrow_mut();
    let current = if line {
        h.state.border.line_color
    } else {
        h.state.border.color
    };
    if current == color {
        return;
    }
    let coalesced = ui
        .last_color
        .borrow()
        .as_ref()
        .is_some_and(|(last, when, state)| {
            *last == line && when.elapsed() < std::time::Duration::from_secs(1) && *state == h.state
        });
    if !coalesced {
        h.begin("Change Border Color");
    }
    if line {
        h.state.border.line_color = color;
    } else {
        h.state.border.color = color;
    }
    h.commit();
    ui.last_color
        .replace(Some((line, std::time::Instant::now(), h.state.clone())));
    drop(h);
    ui.sync_state();
}
