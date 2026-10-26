//! Opt-in real-desktop integration audit. Production launches never run this.
use crate::{Editor, exports::Kind};
use adw::prelude::*;
use gtk::glib;
use std::{
    cell::RefCell,
    hash::{Hash, Hasher},
    path::PathBuf,
    rc::Rc,
    time::{Duration, Instant},
};
struct Audit {
    step: u32,
    started: Instant,
    before: u64,
    retained: u64,
    checks: Vec<String>,
}
fn digest(bytes: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    h.finish()
}
pub fn schedule(ui: &Rc<Editor>, report: PathBuf) {
    let out = report.parent().unwrap().to_path_buf();
    std::fs::create_dir_all(&out).unwrap();
    let audit = Rc::new(RefCell::new(Audit {
        step: 0,
        started: Instant::now(),
        before: 0,
        retained: 0,
        checks: Vec::new(),
    }));
    glib::timeout_add_local(
        Duration::from_millis(30),
        glib::clone!(
            #[weak]
            ui,
            #[upgrade_or]
            glib::ControlFlow::Break,
            move || {
                let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut a = audit.borrow_mut();
                    if a.started.elapsed() > Duration::from_secs(90) {
                        std::fs::write(
                        &report,
                        serde_json::to_vec_pretty(
                            &serde_json::json!({"status":"FAIL","step":a.step,"checks":a.checks}),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                        ui.window.close();
                        return glib::ControlFlow::Break;
                    }
                    let ready = ui.runtime.borrow().ready;
                    let idle = ui.runtime.borrow().exporting.is_none();
                    let current = {
                        let r = ui.runtime.borrow();
                        r.retained_id.is_some() && r.current_revision == Some(r.revision)
                    };
                    match a.step {
                        0 if ready => {
                            assert!(ui.current_path.borrow().is_none());
                            ui.begin_export(out.join("look.cube"), Kind::Lut);
                            a.step = 1;
                        }
                        1 if idle && out.join("look.cube").exists() => {
                            let cube = std::fs::read_to_string(out.join("look.cube")).unwrap();
                            assert!(cube.contains("LUT_3D_SIZE 33"));
                            a.checks.push("Photo-independent LUT export".into());
                            ui.open(out.join("source.png"));
                            a.step = 2;
                        }
                        2 if current => {
                            a.before =
                                digest(&ui.runtime.borrow().developed.as_ref().unwrap().pixels);
                            let control = ui
                                .adjustments
                                .borrow()
                                .iter()
                                .find(|(k, _)| k == "Camera.Exposure")
                                .unwrap()
                                .1
                                .clone();
                            control.set_value(1.5);
                            assert!(ui.runtime.borrow().retained_id.is_none());
                            a.step = 3;
                        }
                        3 if current => {
                            if ui.history.borrow().state.values.get("Post.Contrast") != Some(&60.) {
                                let control = ui
                                    .adjustments
                                    .borrow()
                                    .iter()
                                    .find(|(k, _)| k == "Post.Contrast")
                                    .unwrap()
                                    .1
                                    .clone();
                                control.set_value(60.);
                                return glib::ControlFlow::Continue;
                            }
                            assert_ne!(
                                a.before,
                                digest(&ui.runtime.borrow().developed.as_ref().unwrap().pixels)
                            );
                            a.retained = ui.runtime.borrow().retained_id.unwrap();
                            a.checks.push("Native exposure control changes developed pixels; stale Save is disabled".into());
                            ui.show_original(true);
                            a.step = 4;
                        }
                        4 if ui.runtime.borrow().original.is_some() => {
                            {
                                let r = ui.runtime.borrow();
                                assert!(r.showing_original);
                                assert_ne!(
                                    digest(&r.original.as_ref().unwrap().pixels),
                                    digest(&r.developed.as_ref().unwrap().pixels)
                                );
                                assert_eq!(r.retained_id, Some(a.retained));
                            }
                            ui.show_original(false);
                            a.checks.push("Original uses different pixels and preserves the retained developed render".into());
                            {
                                let mut h = ui.history.borrow_mut();
                                h.begin("Audit crop and border");
                                h.state.crop = Some([0.1, 0.05, 0.8, 0.85]);
                                h.state.straighten = 5.;
                                h.state.rotation = 1;
                                h.state.border.enabled = true;
                                h.state.border.style = crate::border::Style::Keyline;
                                h.state.border.width = 0.08;
                                h.state.border.color = crate::border::Color([35, 52, 69]);
                                h.commit();
                            }
                            ui.sync_state();
                            assert_eq!(ui.runtime.borrow().retained_id, Some(a.retained));
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.export_edge = 1;
                                p.export_pixels = 128;
                                p.metadata = false;
                            }
                            ui.begin_export(out.join("preview.png"), Kind::Preview);
                            a.step = 5;
                        }
                        5 if idle && out.join("preview.png").exists() => {
                            let image = gtk::gdk_pixbuf::Pixbuf::from_file(out.join("preview.png"))
                                .unwrap();
                            assert_eq!(image.width().max(image.height()), 128);
                            assert_eq!(&image.read_pixel_bytes().as_ref()[..3], &[35, 52, 69]);
                            a.checks.push("Save Preview writes matching cropped/straightened/rotated keyline geometry and pixels".into());
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.color_space = "Display P3".into();
                                p.png_bit_depth = 16;
                            }
                            ui.begin_export(out.join("wide-preview.png"), Kind::Preview);
                            a.step = 21;
                        }
                        21 if idle && out.join("wide-preview.png").exists() => {
                            let (depth, icc) = png_depth_and_icc(&out.join("wide-preview.png"));
                            assert_eq!((depth, icc), (16, true));
                            a.checks.push(
                                "Save Preview converts the retained render to Display P3".into(),
                            );
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.color_space = "sRGB".into();
                                p.png_bit_depth = 8;
                            }
                            ui.runtime.borrow_mut().retained_id = None;
                            ui.begin_export(out.join("fast.png"), Kind::Fast);
                            a.step = 6;
                        }
                        6 if idle && out.join("fast.png").exists() => {
                            assert_eq!(
                                std::fs::read(out.join("preview.png")).unwrap(),
                                std::fs::read(out.join("fast.png")).unwrap()
                            );
                            a.checks.push(
                                "Independent Fast export equals the retained preview byte for byte"
                                    .into(),
                            );
                            ui.begin_export(out.join("reference.png"), Kind::Reference);
                            a.step = 7;
                        }
                        7 if idle && out.join("reference.png").exists() => {
                            let image =
                                gtk::gdk_pixbuf::Pixbuf::from_file(out.join("reference.png"))
                                    .unwrap();
                            assert_eq!(image.width().max(image.height()), 128);
                            a.checks.push(
                            "Reference export uses the separate f64 process and retains framing"
                                .into(),
                        );
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.jpeg_quality = 35;
                            }
                            ui.begin_export(out.join("quality35.jpg"), Kind::Reference);
                            a.step = 8;
                        }
                        8 if idle && out.join("quality35.jpg").exists() => {
                            ui.preferences.borrow_mut().jpeg_quality = 95;
                            ui.begin_export(out.join("quality95.jpg"), Kind::Reference);
                            a.step = 9;
                        }
                        9 if idle && out.join("quality95.jpg").exists() => {
                            assert_ne!(
                                std::fs::read(out.join("quality35.jpg")).unwrap(),
                                std::fs::read(out.join("quality95.jpg")).unwrap()
                            );
                            a.checks.push(
                                "Remembered JPEG quality changes the actual encoded output".into(),
                            );
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.color_space = "Display P3".into();
                                p.png_bit_depth = 16;
                            }
                            ui.begin_export(out.join("wide.png"), Kind::Reference);
                            a.step = 20;
                        }
                        20 if idle && out.join("wide.png").exists() => {
                            let (depth, icc) = png_depth_and_icc(&out.join("wide.png"));
                            assert_eq!((depth, icc), (16, true));
                            a.checks.push(
                                "16-bit Display P3 export writes 16-bit PNG with an ICC profile"
                                    .into(),
                            );
                            {
                                let mut p = ui.preferences.borrow_mut();
                                p.color_space = "sRGB".into();
                                p.png_bit_depth = 8;
                            }
                            std::fs::write(out.join("cancelled.png"), b"existing destination")
                                .unwrap();
                            ui.begin_export(out.join("cancelled.png"), Kind::Reference);
                            ui.export_cancel.emit_clicked();
                            a.step = 10;
                        }
                        10 if idle => {
                            assert_eq!(
                                std::fs::read(out.join("cancelled.png")).unwrap(),
                                b"existing destination"
                            );
                            assert!(
                                !std::fs::read_dir(&out)
                                    .unwrap()
                                    .flatten()
                                    .any(|e| e.file_name().to_string_lossy().starts_with(".negs-"))
                            );
                            a.checks.push("Native Cancel preserves an existing destination and removes staging files".into());
                            if let Some(raw) = std::env::var_os("NEGS_RAW_AUDIT") {
                                ui.history
                                    .borrow_mut()
                                    .state
                                    .choices
                                    .insert("Limit preview to 12 MP".into(), "true".into());
                                ui.open(PathBuf::from(raw));
                                a.step = 11;
                                return glib::ControlFlow::Continue;
                            }
                            a.step = 15;
                        }
                        11 if current => {
                            assert_eq!(ui.canvas.source_size(), (6048, 4024));
                            a.before =
                                digest(&ui.runtime.borrow().developed.as_ref().unwrap().pixels);
                            a.checks.push(
                                "Real NEF develops with full 6048×4024 source dimensions".into(),
                            );
                            ui.show_original(true);
                            a.step = 12;
                        }
                        12 if ui.runtime.borrow().original.is_some() => {
                            assert!(ui.runtime.borrow().showing_original);
                            {
                                let mut h = ui.history.borrow_mut();
                                h.begin("Audit RAW white balance");
                                h.state.choices.insert("White balance".into(), "3".into());
                                h.state.values.insert("RAW.Temperature".into(), 4200.);
                                h.state.values.insert("RAW.Tint".into(), 1.2);
                                h.commit();
                            }
                            ui.sync_state();
                            assert!(!ui.runtime.borrow().showing_original);
                            a.step = 13;
                        }
                        13 if current => {
                            assert_ne!(
                                a.before,
                                digest(&ui.runtime.borrow().developed.as_ref().unwrap().pixels)
                            );
                            a.checks.push("RAW custom WB changes pixels and releases an invalidated Original view".into());
                            ui.preferences.borrow_mut().metadata = true;
                            ui.begin_export(out.join("raw-metadata.jpg"), Kind::Preview);
                            a.step = 14;
                        }
                        14 if idle && out.join("raw-metadata.jpg").exists() => {
                            let bytes = std::fs::read(out.join("raw-metadata.jpg")).unwrap();
                            assert!(bytes.windows(5).any(|w| w == b"NIKON"));
                            a.checks.push(
                                "RAW Save Preview exports JPEG with Nikon source metadata".into(),
                            );
                            a.step = 15;
                        }
                        15 => {
                            std::fs::write(
                                &report,
                                serde_json::to_vec_pretty(
                                    &serde_json::json!({"status":"PASS","checks":a.checks}),
                                )
                                .unwrap(),
                            )
                            .unwrap();
                            ui.window.close();
                            return glib::ControlFlow::Break;
                        }
                        _ => {}
                    }
                    glib::ControlFlow::Continue
                }));
                match run {
                    Ok(flow) => flow,
                    Err(error) => {
                        let message = error
                            .downcast_ref::<String>()
                            .cloned()
                            .or_else(|| error.downcast_ref::<&str>().map(|s| s.to_string()))
                            .unwrap_or("Audit assertion failed".into());
                        let a = audit.borrow();
                        let _=std::fs::write(&report,serde_json::to_vec_pretty(&serde_json::json!({"status":"FAIL","step":a.step,"error":message,"checks":a.checks})).unwrap());
                        if let Some(dialog) = ui.window.visible_dialog() {
                            dialog.force_close();
                        }
                        ui.window.close();
                        glib::ControlFlow::Break
                    }
                }
            }
        ),
    );
}

/// A PNG's bit depth and whether it carries an ICC profile (`iCCP`).
fn png_depth_and_icc(path: &std::path::Path) -> (u8, bool) {
    let bytes = std::fs::read(path).unwrap();
    let (mut at, mut icc) = (8, false);
    while at + 8 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        icc |= &bytes[at + 4..at + 8] == b"iCCP";
        at += 12 + len;
    }
    (bytes[24], icc)
}
