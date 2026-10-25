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
                Ok(color) => update(&ui, "Change Border Color", |s| {
                    if line {
                        s.line_color = color;
                    } else {
                        s.color = color;
                    }
                }),
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
            update(&ui, "Change Border Color", |s| {
                let color = Color::from_rgba(picker.rgba());
                if line {
                    s.line_color = color;
                } else {
                    s.color = color;
                }
            });
        }
    ));
    row.add_suffix(&entry);
    row.add_suffix(&apply);
    row.add_suffix(&picker);
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
        move |row| update(&ui, "Change Border Style", |s| s.style =
            Style::from_index(row.selected()))
    ));
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
    options.append(&export);
    body.append(&options);
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
