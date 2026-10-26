//! Native options followed by a portal-compatible save picker.
use crate::Editor;
use adw::prelude::*;
use gtk::{gio, glib};
use std::{path::PathBuf, rc::Rc};

pub fn present(ui: &Rc<Editor>, kind: &str) {
    let lut = kind == "lut";
    let preview = kind == "preview";
    let title = if lut {
        "Export LUT"
    } else if preview {
        "Save Preview"
    } else {
        "Export Photograph"
    };
    let dialog = adw::Dialog::builder()
        .title(title)
        .content_width(480)
        .content_height(660)
        .build();
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    let body = gtk::Box::new(gtk::Orientation::Vertical, 16);
    body.set_margin_top(16);
    body.set_margin_bottom(24);
    body.set_margin_start(24);
    body.set_margin_end(24);
    let group = adw::PreferencesGroup::new();
    let p = ui.preferences.borrow();
    let engine = adw::ComboRow::builder()
        .title("Engine")
        .model(&gtk::StringList::new(&[
            "Fast · GPU / f32",
            "Reference · CPU / f64",
        ]))
        .selected(p.export_engine.min(1))
        .visible(!lut && !preview)
        .build();
    group.add(&engine);
    let format = adw::ComboRow::builder()
        .title("Format")
        .model(&gtk::StringList::new(if lut {
            &["Cube (.cube)"][..]
        } else {
            &["PNG · lossless", "JPEG · smaller", "TIFF"][..]
        }))
        .selected(if lut { 0 } else { p.output_format.min(2) })
        .build();
    group.add(&format);
    let depth = adw::ComboRow::builder()
        .title("Bit depth")
        .model(&gtk::StringList::new(&["8-bit", "16-bit"]))
        .visible(!lut)
        .build();
    group.add(&depth);
    let color_space = adw::ComboRow::builder()
        .title("Colour space")
        .visible(!lut)
        .build();
    group.add(&color_space);
    // Set while refresh rewrites the rows, so their handlers don't store it.
    let updating = Rc::new(std::cell::Cell::new(false));
    if lut {
        group.add(
            &adw::ActionRow::builder()
                .title("Grid size")
                .subtitle("33 × 33 × 33 · sRGB encoded input and output")
                .build(),
        );
    }
    let quality_row = adw::ActionRow::builder().title("JPEG quality").build();
    let quality = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1., 100., 1.);
    quality.set_value(p.jpeg_quality.clamp(1, 100) as f64);
    quality.set_digits(0);
    quality.set_draw_value(true);
    quality.set_value_pos(gtk::PositionType::Left);
    quality.set_width_request(220);
    quality.set_valign(gtk::Align::Center);
    quality.update_property(&[gtk::accessible::Property::Label("JPEG quality")]);
    quality_row.add_suffix(&quality);
    group.add(&quality_row);
    let edge = adw::ComboRow::builder()
        .title("Size")
        .model(&gtk::StringList::new(&[
            "Full Size",
            "Long Edge",
            "Short Edge",
        ]))
        .selected(p.export_edge.min(2))
        .visible(!lut)
        .build();
    group.add(&edge);
    let pixels_row = adw::ActionRow::builder().title("Edge pixels").build();
    let pixels = gtk::SpinButton::with_range(16., 65535., 1.);
    pixels.set_value(p.export_pixels.clamp(16, 65535) as f64);
    pixels.set_width_chars(7);
    pixels.set_valign(gtk::Align::Center);
    pixels.update_property(&[gtk::accessible::Property::Label("Export edge pixels")]);
    pixels_row.add_suffix(&pixels);
    group.add(&pixels_row);
    let border = adw::SwitchRow::builder()
        .title("Include border")
        .active(ui.history.borrow().state.border.export)
        .visible(!lut)
        .build();
    group.add(&border);
    let metadata = adw::SwitchRow::builder()
        .title("Copy photograph metadata")
        .active(p.metadata)
        .visible(!lut)
        .build();
    group.add(&metadata);
    drop(p);
    body.append(&group);
    let filename = gtk::Entry::new();
    filename.update_property(&[gtk::accessible::Property::Label("Output filename")]);
    let stem = ui
        .current_path
        .borrow()
        .as_ref()
        .and_then(|p| p.file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or("negs".into());
    let film = crate::catalog::selected(&ui.history.borrow().state, false)
        .id
        .clone();
    filename.set_text(&format!(
        "{stem}_{film}.{}",
        if lut {
            "cube"
        } else {
            extension(format.selected())
        }
    ));
    body.append(&filename);
    let destination = gtk::Label::new(Some(
        &ui.preferences
            .borrow()
            .last_save_folder
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or("Choose a destination folder".into()),
    ));
    destination.set_wrap(true);
    destination.set_xalign(0.);
    destination.add_css_class("caption");
    body.append(&destination);
    let choose = gtk::Button::with_label("Choose Destination…");
    choose.set_tooltip_text(Some("Choose the output folder and suggested filename"));
    body.append(&choose);
    let hint = gtk::Label::new(None);
    hint.set_wrap(true);
    hint.set_xalign(0.);
    hint.add_css_class("dim-label");
    body.append(&hint);
    let refresh = Rc::new(glib::clone!(
        #[weak]
        ui,
        #[weak]
        engine,
        #[weak]
        format,
        #[weak]
        edge,
        #[weak]
        pixels,
        #[weak]
        pixels_row,
        #[weak]
        border,
        #[weak]
        depth,
        #[weak]
        color_space,
        #[strong]
        updating,
        #[weak]
        quality_row,
        #[weak]
        hint,
        #[weak]
        filename,
        move || {
            pixels_row.set_visible(!lut && edge.selected() != 0);
            quality_row.set_visible(!lut && format.selected() == 1);
            let p = ui.preferences.borrow();
            let bits = p.bit_depth(format.selected());
            let space = p.color_space(format.selected()).to_string();
            drop(p);
            // ProPhoto RGB is offered at 16 bits only.
            let spaces = &crate::preferences::COLOR_SPACES[..if bits == 16 { 4 } else { 3 }];
            updating.set(true);
            depth.set_selected(if bits == 16 { 1 } else { 0 });
            depth.set_sensitive(format.selected() != 1);
            depth.set_subtitle(if format.selected() == 1 { "JPEG is 8-bit only" } else { "" });
            if color_space.model().is_none_or(|m| m.n_items() as usize != spaces.len()) {
                color_space.set_model(Some(&gtk::StringList::new(spaces)));
            }
            color_space.set_selected(spaces.iter().position(|s| *s == space).unwrap_or(0) as u32);
            color_space.set_subtitle(if bits == 16 { "" } else { "ProPhoto RGB needs 16-bit" });
            updating.set(false);
            let current = filename.text();
            let stem = std::path::Path::new(current.as_str())
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy();
            let expected = if lut {
                "cube"
            } else {
                extension(format.selected())
            };
            let name = format!("{stem}.{expected}");
            if current.as_str() != name {
                filename.set_text(&name);
            }
            let mut message: String = if lut {
                "33³ Cube includes film, paper, manual exposure, development, print filters and color. Excludes auto exposure, grain, halation/scatter, highlight boost, glare, diffusion, blur/sharpening, RAW development, resizing and Post edits. DIR inhibition stays active without spatial diffusion.".into()
            } else if preview {
                "Saves the current developed preview as shown.".into()
            } else if engine.selected() == 0 {
                "Fast uses the f32 GPU path. Looks the same as Reference, in a fraction of the time.".into()
            } else {
                "Reference uses the f64 CPU path. The most precise result; large photographs can take about half a minute.".into()
            };
            if !lut {
                if ui.history.borrow().state.crop.is_some() {
                    message.push_str(" Crops the photograph.");
                }
                if border.is_active() && ui.history.borrow().state.border.enabled {
                    message.push_str(" Adds the border.");
                }
                if space != "sRGB" {
                    message.push_str(&if preview {
                        format!(" Converts to {space}; the preview's colours stay within sRGB.")
                    } else {
                        format!(" Develops in {space} and embeds its profile.")
                    });
                }
                if edge.selected() != 0 {
                    message.push_str(&format!(
                        " Fits {:.0} px on the {} edge, without enlargement.",
                        pixels.value(),
                        if edge.selected() == 1 {
                            "long"
                        } else {
                            "short"
                        }
                    ));
                }
            }
            if crate::runtime::ui_only() {
                message.push_str(
                    "\nEngine processing and file writing are not connected in this UI preview.",
                );
            }
            hint.set_text(&message);
        }
    ));
    refresh();
    engine.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        move |r| {
            ui.preferences.borrow_mut().export_engine = r.selected();
            refresh();
        }
    ));
    format.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        move |r| {
            if !lut {
                ui.preferences.borrow_mut().output_format = r.selected();
            }
            refresh();
        }
    ));
    edge.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        move |r| {
            ui.preferences.borrow_mut().export_edge = r.selected();
            refresh();
        }
    ));
    pixels.connect_value_changed(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        move |v| {
            ui.preferences.borrow_mut().export_pixels = v.value() as u32;
            refresh();
        }
    ));
    depth.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[weak]
        format,
        #[strong]
        refresh,
        #[strong]
        updating,
        move |r| {
            if !updating.get() {
                let bits = if r.selected() == 1 { 16 } else { 8 };
                ui.preferences.borrow_mut().set_bit_depth(format.selected(), bits);
                refresh();
            }
        }
    ));
    color_space.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        #[strong]
        updating,
        move |r| {
            if !updating.get() {
                if let Some(name) = crate::preferences::COLOR_SPACES.get(r.selected() as usize) {
                    ui.preferences.borrow_mut().color_space = (*name).into();
                }
                refresh();
            }
        }
    ));
    quality.connect_value_changed(glib::clone!(
        #[weak]
        ui,
        move |v| {
            ui.preferences.borrow_mut().jpeg_quality = v.value() as u32;
        }
    ));
    metadata.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        move |r| {
            ui.preferences.borrow_mut().metadata = r.is_active();
        }
    ));
    border.connect_active_notify(glib::clone!(
        #[weak]
        ui,
        #[strong]
        refresh,
        move |r| {
            let mut h = ui.history.borrow_mut();
            h.begin("Change Border");
            h.state.border.export = r.is_active();
            h.commit();
            drop(h);
            ui.sync_state();
            refresh();
        }
    ));
    choose.connect_clicked(glib::clone!(#[weak] ui,#[weak] filename,#[weak] destination,move |_| {
        let picker=gtk::FileDialog::builder().title("Choose Output Destination").initial_name(filename.text()).build();
        if let Some(folder)=ui.preferences.borrow().last_save_folder.as_ref().filter(|p|p.is_dir()) {picker.set_initial_folder(Some(&gio::File::for_path(folder)));}
        picker.save(Some(&ui.window),None::<&gio::Cancellable>,glib::clone!(#[weak] ui,#[weak] filename,#[weak] destination,move |result| {
            if let Ok(file)=result && let Some(path)=file.path() {
                if ui.current_path.borrow().as_ref().is_some_and(|source|same_file(source,&path)) {ui.toast.add_toast(adw::Toast::new("Choose a different filename to preserve the original photograph."));return;}
                ui.preferences.borrow_mut().last_save_folder=path.parent().map(std::path::Path::to_path_buf);
                if let Some(name)=path.file_name() {filename.set_text(&name.to_string_lossy());}destination.set_text(&path.to_string_lossy());
            }
        }));
    }));
    let save = gtk::Button::with_label(if lut {
        "Export LUT"
    } else if preview {
        "Save Preview"
    } else {
        "Export Photograph"
    });
    save.add_css_class("suggested-action");
    save.set_sensitive(
        !crate::runtime::ui_only()
            && ui.runtime.borrow().ready
            && ui.runtime.borrow().exporting.is_none(),
    );
    save.set_tooltip_text(Some(if crate::runtime::ui_only() {
        "Requires the film engine connection"
    } else {
        "Choose a filename and save atomically"
    }));
    save.connect_clicked(glib::clone!(#[weak] ui,#[weak] dialog,#[weak] filename,#[weak] format,#[weak] engine,move |_|{
        if preview && ui.runtime.borrow().retained_id.is_none() {ui.toast.add_toast(adw::Toast::new("Wait for the current preview to finish developing."));return;}
        let picker=gtk::FileDialog::builder().title(title).initial_name(filename.text()).build();
        if let Some(folder)=ui.preferences.borrow().last_save_folder.as_ref().filter(|p|p.is_dir()){picker.set_initial_folder(Some(&gio::File::for_path(folder)));}
        let kind=if lut {crate::exports::Kind::Lut}else if preview {crate::exports::Kind::Preview}else if engine.selected()==1 {crate::exports::Kind::Reference}else{crate::exports::Kind::Fast};
        let expected=if lut {"cube"}else{extension(format.selected())};
        picker.save(Some(&ui.window),None::<&gio::Cancellable>,glib::clone!(#[weak] ui,#[weak] dialog,move |result|{
            if let Ok(file)=result && let Some(path)=file.path(){
                let ext=path.extension().and_then(|s|s.to_str()).unwrap_or("").to_ascii_lowercase();
                if ext!=expected && !(expected=="jpg" && ext=="jpeg") && !(expected=="tif" && ext=="tiff") {ui.engine_error(&format!("Use a .{expected} filename for the selected output format."));return;}
                if ui.current_path.borrow().as_ref().is_some_and(|source|same_file(source,&path)){ui.engine_error("Choose a different filename to preserve the original photograph.");return;}
                ui.preferences.borrow_mut().last_save_folder=path.parent().map(std::path::Path::to_path_buf);
                dialog.close();ui.begin_export(path,kind);
            }
        }));
    }));
    body.append(&save);
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(&body)
        .build();
    toolbar.set_content(Some(&scroll));
    dialog.set_child(Some(&toolbar));
    dialog.present(Some(&ui.window));
}
fn extension(format: u32) -> &'static str {
    match format {
        1 => "jpg",
        2 => "tif",
        _ => "png",
    }
}
fn same_file(a: &PathBuf, b: &PathBuf) -> bool {
    use std::os::unix::fs::MetadataExt;
    if a == b {
        return true;
    }
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(a), Ok(b)) => a.dev() == b.dev() && a.ino() == b.ino(),
        _ => false,
    }
}
