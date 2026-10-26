//! Immutable export snapshots and atomic file publication.
use crate::{
    Editor,
    engine::{self, Engine, Job},
    model::State,
    parameters,
};
use adw::prelude::*;
use gtk::{gio, glib};
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::Command,
    rc::Rc,
    sync::{Arc, atomic::Ordering},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Preview,
    Fast,
    Reference,
    Lut,
}
struct Snapshot {
    source: Option<PathBuf>,
    output: PathBuf,
    state: State,
    params: Value,
    film: String,
    paper: String,
    fit: Value,
    quality: u32,
    metadata: bool,
    retained: Option<u64>,
    kind: Kind,
    backend: String,
    source_stamp: Option<crate::filmstrip::Stamp>,
}
struct Staging {
    path: PathBuf,
    record: PathBuf,
}
impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_file(&self.record);
    }
}
impl Editor {
    pub fn begin_export(self: &Rc<Self>, output: PathBuf, kind: Kind) {
        let mut r = self.runtime.borrow_mut();
        if !r.ready || r.exporting.is_some() || (kind == Kind::Preview && r.retained_id.is_none()) {
            return;
        }
        let Some(client) = r.client.clone() else {
            return;
        };
        let source = self.current_path.borrow().clone();
        if kind != Kind::Lut && source.is_none() {
            return;
        }
        if source
            .as_ref()
            .is_some_and(|p| engine::same_file(p, &output))
        {
            drop(r);
            self.engine_error("Choose a different filename to preserve the original photograph.");
            return;
        }
        let state = self.history.borrow().state.clone();
        let p = self.preferences.borrow();
        let fit = match p.export_edge {
            1 => json!({"long_edge":p.export_pixels}),
            2 => json!({"short_edge":p.export_pixels}),
            _ => Value::Null,
        };
        let retained = if kind == Kind::Preview
            || (kind == Kind::Fast
                && r.current_revision == Some(r.revision)
                && r.developed
                    .as_ref()
                    .is_some_and(|f| f.result["export_resolution"] == true))
        {
            r.retained_id
        } else {
            None
        };
        let source_stamp = source.as_ref().map(|p| crate::filmstrip::stamp(p));
        let snapshot = Snapshot {
            source,
            output: output.clone(),
            params: parameters::runtime(&state, &r.defaults),
            film: crate::catalog::selected(&state, false).id.clone(),
            paper: crate::catalog::selected(&state, true).id.clone(),
            state,
            fit,
            quality: p.jpeg_quality,
            metadata: p.metadata,
            retained,
            kind,
            backend: r
                .developed
                .as_ref()
                .and_then(|f| f.result["backend"].as_str())
                .unwrap_or("")
                .into(),
            source_stamp,
        };
        drop(p);
        let job = Arc::new(Job::new());
        r.exporting = Some(job.clone());
        r.preview_saving = kind == Kind::Preview;
        r.block_helper_render = retained.is_some();
        drop(r);
        self.sync_history();
        self.export_progress.set_visible(true);
        self.export_cancel.set_visible(kind != Kind::Preview);
        self.export_elapsed.set_label(if kind == Kind::Lut {
            "Exporting LUT…"
        } else if kind == Kind::Preview {
            "Saving preview…"
        } else {
            "Exporting…"
        });
        let started = Instant::now();
        let ui = self.clone();
        glib::timeout_add_local(
            Duration::from_millis(250),
            glib::clone!(
                #[weak]
                ui,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    if ui.runtime.borrow().closing || ui.runtime.borrow().exporting.is_none() {
                        return glib::ControlFlow::Break;
                    }
                    ui.export_elapsed.set_label(&format!(
                        "{} · {} s",
                        if kind == Kind::Preview {
                            "Saving"
                        } else {
                            "Exporting"
                        },
                        started.elapsed().as_secs()
                    ));
                    glib::ControlFlow::Continue
                }
            ),
        );
        let ui = self.clone();
        glib::MainContext::default().spawn_local(async move {
            let result = gio::spawn_blocking(move || write_export(&client, &job, snapshot)).await;
            {
                let mut r = ui.runtime.borrow_mut();
                r.exporting = None;
                r.preview_saving = false;
                r.block_helper_render = false;
                if r.closing {
                    return;
                }
            }
            ui.export_progress.set_visible(false);
            match result {
                Ok(Ok(backend)) => {
                    let name = output.file_name().unwrap_or_default().to_string_lossy();
                    ui.status.set_label(&format!(
                        "{} {name}{}",
                        if kind == Kind::Preview {
                            "Saved"
                        } else {
                            "Exported"
                        },
                        if backend.starts_with("CPU") && kind == Kind::Fast {
                            " on the CPU (32-bit): no GPU was available for Fast export"
                        } else {
                            ""
                        }
                    ));
                }
                Ok(Err(error)) if error == "Export cancelled" => {
                    ui.status.set_label("Export cancelled")
                }
                Ok(Err(error)) => ui.engine_error(&if kind == Kind::Fast {
                    format!("{error}\nTry Reference for a CPU export.")
                } else {
                    error
                }),
                Err(_) => ui.engine_error("The export worker stopped unexpectedly."),
            }
            ui.sync_history();
            ui.render_next();
        });
    }
    pub fn cancel_export(self: &Rc<Self>) {
        let mut r = self.runtime.borrow_mut();
        if r.preview_saving {
            return;
        }
        let Some(job) = r.exporting.clone() else {
            return;
        };
        job.cancel();
        if job.using_helper.load(Ordering::Acquire) {
            r.retained_id = None;
            r.current_revision = None;
            r.revision += 1;
        }
        self.export_elapsed.set_label("Cancelling…");
    }
}

fn write_export(client: &Engine, job: &Job, mut snapshot: Snapshot) -> engine::Result<String> {
    let parent = snapshot
        .output
        .parent()
        .filter(|p| p.is_dir())
        .ok_or("Choose an existing destination folder.")?;
    if snapshot
        .source
        .as_ref()
        .is_some_and(|p| engine::same_file(p, &snapshot.output))
    {
        return Err("Choose a different filename to preserve the original photograph.".into());
    }
    let ext = snapshot
        .output
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("png");
    let id = engine::unique();
    let stage = parent.join(format!(".negs-{id}.{ext}"));
    let record = client.scratch.directory.join("exports.json");
    std::fs::write(
        &record,
        serde_json::to_vec(&vec![&stage]).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let staging = Staging {
        path: stage,
        record,
    };
    // Exclusive creation refuses an accidental collision before any encoder opens it.
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&staging.path)
        .map_err(|e| e.to_string())?;
    if job.cancelled() {
        return Err("Export cancelled".into());
    }
    let mut backend = snapshot.backend.clone();
    let mut saved = false;
    if snapshot.kind == Kind::Lut {
        job.using_helper.store(true, Ordering::Release);
        let result=client.request_for_job(json!({"command":"export_lut","film":snapshot.film,"paper":snapshot.paper,"params":snapshot.params,"size":33,"output":staging.path}),Duration::from_secs(180), job);
        job.using_helper.store(false, Ordering::Release);
        if job.cancelled() {
            return Err("Export cancelled".into());
        }
        result?;
        saved = true;
    }
    if let Some(render_id) = snapshot.retained {
        let border = &snapshot.state.border;
        let mut command = json!({"command":"save","render_id":render_id,"output":staging.path,"rotate_cw":snapshot.state.rotation,
            "crop":parameters::crop(&snapshot.state),"fit":snapshot.fit,
            "save_options":{"jpeg_quality":snapshot.quality,"copy_metadata":snapshot.metadata},"crop_after_post":true});
        if border.enabled && border.export {
            let (style, width) = match border.style {
                crate::border::Style::Solid => ("white", border.width),
                crate::border::Style::Keyline => ("keyline", border.width),
                crate::border::Style::Film => ("film", border.film_width),
                crate::border::Style::Film35 => ("35mm", 0.),
            };
            command["border"] = json!(style);
            command["border_options"] = json!({"width":width,"line":border.line,"color":border.color.hex(),"line_color":border.line_color.hex()});
            command["instagram"] = json!(border.instagram);
        }
        job.using_helper.store(true, Ordering::Release);
        let result = client.request_for_job(command, Duration::from_secs(180), job);
        job.using_helper.store(false, Ordering::Release);
        if job.cancelled() {
            return Err("Export cancelled".into());
        }
        match result {
            Ok(_) => saved = true,
            Err(error) if snapshot.kind == Kind::Preview => return Err(error),
            Err(_) => {}
        }
    }
    if !saved {
        let source = snapshot.source.as_ref().ok_or("No photograph is open")?;
        parameters::set(
            &mut snapshot.params,
            "io.rotate_cw",
            json!(snapshot.state.rotation),
        );
        let params = client.scratch.directory.join(format!("export-{id}.json"));
        std::fs::write(
            &params,
            serde_json::to_vec(&snapshot.params).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let log = client.scratch.directory.join(format!("export-{id}.log"));
        let executable = if snapshot.kind == Kind::Reference {
            &client.paths.reference
        } else {
            &client.paths.fast
        };
        let mut command = Command::new(executable);
        command
            .arg("process")
            .arg(source)
            .arg("-o")
            .arg(&staging.path)
            .arg("--film")
            .arg(&snapshot.film)
            .arg("--paper")
            .arg(&snapshot.paper)
            .arg("--params")
            .arg(&params)
            .arg("--data-dir")
            .arg(&client.paths.data)
            .arg("--jpeg-quality")
            .arg(snapshot.quality.to_string());
        command.env(
            "SPEKTRAFILM_BACKEND",
            if snapshot.kind == Kind::Reference {
                "cpu"
            } else {
                "wgpu"
            },
        );
        if !snapshot.metadata {
            command.arg("--no-metadata");
        }
        if crate::inspector::flag(&snapshot.state, "Scan film directly", false) {
            command.arg("--scan-film");
        }
        let crop = parameters::crop(&snapshot.state);
        command.arg("--crop").arg(
            ["x", "y", "width", "height", "angle"]
                .iter()
                .map(|k| crop[*k].to_string())
                .collect::<Vec<_>>()
                .join(","),
        );
        add_border_args(&mut command, &snapshot.state);
        if let Some(px) = snapshot.fit["long_edge"].as_u64() {
            command.arg("--long-edge").arg(px.to_string());
        }
        if let Some(px) = snapshot.fit["short_edge"].as_u64() {
            command.arg("--short-edge").arg(px.to_string());
        }
        let result = job.run(&mut command, &log, Duration::from_secs(1800));
        let _ = std::fs::remove_file(params);
        result?;
        backend = engine::tail(&log)
            .lines()
            .find_map(|line| line.strip_prefix("Backend: "))
            .unwrap_or("unknown backend")
            .to_string();
    }
    if job.cancelled() {
        return Err("Export cancelled".into());
    }
    if snapshot.source.as_ref().map(|p| crate::filmstrip::stamp(p)) != snapshot.source_stamp {
        return Err(
            "The source photograph changed during export. Try again after it finishes updating."
                .into(),
        );
    }
    job.publish(&staging.path, &snapshot.output, snapshot.source.as_deref())?;
    Ok(backend)
}
fn add_border_args(command: &mut Command, state: &State) {
    let border = &state.border;
    if !border.enabled || !border.export {
        return;
    }
    let (style, width) = match border.style {
        crate::border::Style::Solid => ("solid", border.width),
        crate::border::Style::Keyline => ("keyline", border.width),
        crate::border::Style::Film => ("film", border.film_width),
        crate::border::Style::Film35 => ("35mm", 0.),
    };
    command
        .arg("--border")
        .arg(style)
        .arg("--border-width")
        .arg(width.to_string())
        .arg("--border-color")
        .arg(border.color.hex())
        .arg("--keyline-width")
        .arg(border.line.to_string())
        .arg("--keyline-color")
        .arg(border.line_color.hex());
    if border.instagram {
        command.arg("--instagram");
    }
}
