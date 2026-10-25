//! Virtualized folder strip with bounded thumbnail work and native file notifications.
use crate::Editor;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, SystemTime},
};

pub type Stamp = (u64, Option<SystemTime>);
pub struct Job {
    path: PathBuf,
    item: glib::WeakRef<gtk::ListItem>,
    picture: gtk::Picture,
    stamp: Stamp,
}
pub struct Cached {
    pub path: PathBuf,
    pub stamp: Stamp,
    pub texture: gdk::Texture,
    pub source: (i32, i32),
}
pub fn stamp(path: &Path) -> Stamp {
    std::fs::metadata(path)
        .map(|m| (m.len(), m.modified().ok()))
        .unwrap_or((0, None))
}
fn item_path(item: &gtk::ListItem) -> Option<PathBuf> {
    item.item()
        .and_downcast::<gtk::StringObject>()
        .map(|s| PathBuf::from(s.string().as_str()))
}
pub fn build(ui: &Rc<Editor>) {
    let factory = gtk::SignalListItemFactory::new();
    factory.connect_setup(glib::clone!(
        #[weak]
        ui,
        move |_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let button = gtk::Button::new();
            button.add_css_class("strip-photo");
            let cell = gtk::Box::new(gtk::Orientation::Vertical, 5);
            let picture = gtk::Picture::new();
            picture.set_size_request(104, 68);
            picture.set_content_fit(gtk::ContentFit::Contain);
            cell.append(&picture);
            let label = gtk::Label::new(None);
            label.set_max_width_chars(16);
            label.set_width_chars(16);
            label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
            cell.append(&label);
            button.set_child(Some(&cell));
            button.connect_clicked(glib::clone!(
                #[weak]
                ui,
                #[weak]
                item,
                move |_| {
                    if let Some(path) = item_path(&item) {
                        ui.open(path);
                    }
                }
            ));
            item.set_child(Some(&button));
        }
    ));
    factory.connect_bind(glib::clone!(
        #[weak]
        ui,
        move |_, object| {
            let Some(item) = object.downcast_ref::<gtk::ListItem>() else {
                return;
            };
            let Some(path) = item_path(item) else {
                return;
            };
            let Some(button) = item.child().and_downcast::<gtk::Button>() else {
                return;
            };
            let cell = button.child().and_downcast::<gtk::Box>().unwrap();
            let picture = cell.first_child().and_downcast::<gtk::Picture>().unwrap();
            let label = cell.last_child().and_downcast::<gtk::Label>().unwrap();
            label.set_text(&path.file_name().unwrap_or_default().to_string_lossy());
            button.set_tooltip_text(Some(&path.to_string_lossy()));
            button.update_property(&[gtk::accessible::Property::Label(
                &path.file_name().unwrap_or_default().to_string_lossy(),
            )]);
            picture.set_paintable(None::<&gdk::Texture>);
            let stamp = stamp(&path);
            let cached = ui
                .thumbnail_cache
                .borrow()
                .iter()
                .find(|c| c.path == path && c.stamp == stamp)
                .map(|c| (c.texture.clone(), c.source));
            if let Some((texture, source)) = cached {
                picture.set_paintable(Some(&texture));
                describe(&picture, source);
            } else {
                ui.thumbnail_jobs.borrow_mut().push_back(Job {
                    path,
                    item: item.downgrade(),
                    picture,
                    stamp,
                });
                start_workers(&ui);
            }
        }
    ));
    let model = gtk::StringList::new(&[]);
    let selection = gtk::SingleSelection::new(Some(model.clone()));
    selection.set_autoselect(false);
    selection.set_can_unselect(true);
    selection.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        move |s| {
            if ui.syncing.get() || ui.canvas.cropping() {
                return;
            }
            let path = ui.strip_paths.borrow().get(s.selected() as usize).cloned();
            if let Some(path) = path {
                ui.open(path);
            }
        }
    ));
    let view = gtk::ListView::new(Some(selection.clone()), Some(factory));
    view.set_orientation(gtk::Orientation::Horizontal);
    view.set_single_click_activate(false);
    view.add_css_class("filmstrip");
    view.update_property(&[gtk::accessible::Property::Label(
        "Photographs in this folder",
    )]);
    ui.strip_scroll.set_child(Some(&view));
    ui.strip_model.replace(Some(model));
    ui.strip_selection.replace(Some(selection));
    ui.strip_view.replace(Some(view));
}
fn start_workers(ui: &Rc<Editor>) {
    while ui.thumbnail_workers.get() < 2 && !ui.thumbnail_jobs.borrow().is_empty() {
        ui.thumbnail_workers.set(ui.thumbnail_workers.get() + 1);
        let ui = ui.clone();
        glib::MainContext::default().spawn_local(async move {
            loop {
                let Some(job) = ui.thumbnail_jobs.borrow_mut().pop_front() else {
                    break;
                };
                let valid = || {
                    job.item
                        .upgrade()
                        .is_some_and(|item| item_path(&item).as_ref() == Some(&job.path))
                };
                if !valid() {
                    continue;
                }
                let path = job.path.clone();
                let result = gio::spawn_blocking(move || crate::photo::decode(&path, 192)).await;
                if !valid() || stamp(&job.path) != job.stamp {
                    continue;
                }
                if let Ok(Ok(photo)) = result {
                    let format = if photo.alpha {
                        gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gdk::MemoryFormat::R8g8b8
                    };
                    let texture: gdk::Texture = gdk::MemoryTexture::new(
                        photo.width,
                        photo.height,
                        format,
                        &glib::Bytes::from_owned(photo.bytes),
                        photo.stride,
                    )
                    .upcast();
                    job.picture.set_paintable(Some(&texture));
                    describe(&job.picture, photo.source);
                    let mut cache = ui.thumbnail_cache.borrow_mut();
                    cache.retain(|c| c.path != job.path);
                    cache.push_back(Cached {
                        path: job.path,
                        stamp: job.stamp,
                        texture,
                        source: photo.source,
                    });
                    while cache.len() > 192 {
                        cache.pop_front();
                    }
                }
            }
            ui.thumbnail_workers.set(ui.thumbnail_workers.get() - 1);
        });
    }
}
pub fn select(ui: &Editor, path: &Path) {
    if let Some(index) = ui.strip_paths.borrow().iter().position(|p| p == path) {
        if let Some(selection) = ui.strip_selection.borrow().as_ref() {
            let syncing = ui.syncing.replace(true);
            selection.set_selected(index as u32);
            ui.syncing.set(syncing);
        }
        if let Some(view) = ui.strip_view.borrow().as_ref() {
            view.scroll_to(index as u32, gtk::ListScrollFlags::NONE, None);
        }
    }
}
pub fn load(ui: &Rc<Editor>, path: PathBuf, force: bool) {
    select(ui, &path);
    let folder = path.parent().map(Path::to_path_buf);
    if !force && *ui.strip_folder.borrow() == folder {
        return;
    }
    if *ui.strip_folder.borrow() != folder {
        ui.strip_folder.replace(folder.clone());
        if let Some(monitor) = ui.folder_monitor.take() {
            monitor.cancel();
        }
        if let Some(folder) = folder
            && let Ok(monitor) = gio::File::for_path(&folder).monitor_directory(
                gio::FileMonitorFlags::WATCH_MOVES,
                None::<&gio::Cancellable>,
            )
        {
            monitor.connect_changed(glib::clone!(
                #[weak]
                ui,
                move |_, file, other, _| {
                    if let Some(path) = file.path() {
                        ui.changed_paths.borrow_mut().insert(path);
                    }
                    if let Some(path) = other.and_then(|f| f.path()) {
                        ui.changed_paths.borrow_mut().insert(path);
                    }
                    if let Some(timer) = ui.folder_debounce.take() {
                        timer.remove();
                    }
                    let timer = glib::timeout_add_local_once(
                        Duration::from_millis(180),
                        glib::clone!(
                            #[weak]
                            ui,
                            move || {
                                ui.folder_debounce.take();
                                let changes = ui.changed_paths.take();
                                let mut cache = ui.thumbnail_cache.borrow_mut();
                                cache.retain(|c| {
                                    !changes.contains(&c.path) || c.stamp == stamp(&c.path)
                                });
                                drop(cache);
                                for path in &changes {
                                    let index =
                                        ui.strip_paths.borrow().iter().position(|p| p == path);
                                    if let Some(index) = index
                                        && path.is_file()
                                        && let Some(model) = ui.strip_model.borrow().as_ref()
                                    {
                                        let syncing = ui.syncing.replace(true);
                                        model.splice(index as u32, 1, &[&path.to_string_lossy()]);
                                        ui.syncing.set(syncing);
                                    }
                                }
                                let current = ui.current_path.borrow().clone();
                                if let Some(current) = current
                                    && changes.contains(&current)
                                    && current.is_file()
                                    && ui.current_stamp.get() != Some(stamp(&current))
                                {
                                    ui.open(current);
                                }
                                let path = ui.current_path.borrow().clone();
                                if let Some(path) = path {
                                    load(&ui, path, true);
                                }
                            }
                        ),
                    );
                    ui.folder_debounce.replace(Some(timer));
                }
            ));
            ui.folder_monitor.replace(Some(monitor));
        }
    }
    let generation = ui.strip_generation.get() + 1;
    ui.strip_generation.set(generation);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let paths = gio::spawn_blocking(move || crate::photo::folder(&path))
            .await
            .unwrap_or_default();
        if ui.strip_generation.get() != generation {
            return;
        }
        if paths == *ui.strip_paths.borrow() {
            return;
        }
        let old = ui.strip_paths.borrow();
        // Splice only the changed middle, preserving widgets on unaffected paths.
        let prefix = old.iter().zip(&paths).take_while(|(a, b)| a == b).count();
        let suffix = old[prefix..]
            .iter()
            .rev()
            .zip(paths[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        let removed = old.len() - prefix - suffix;
        drop(old);
        let strings: Vec<_> = paths[prefix..paths.len() - suffix]
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        if let Some(model) = ui.strip_model.borrow().as_ref() {
            let syncing = ui.syncing.replace(true);
            model.splice(
                prefix as u32,
                removed as u32,
                &strings.iter().map(String::as_str).collect::<Vec<_>>(),
            );
            ui.syncing.set(syncing);
        }
        ui.strip_paths.replace(paths);
        let path = ui
            .pending_path
            .borrow()
            .clone()
            .or_else(|| ui.current_path.borrow().clone());
        if let Some(path) = path {
            select(&ui, &path);
        }
    });
}

fn describe(picture: &gtk::Picture, source: (i32, i32)) {
    picture.update_property(&[
        gtk::accessible::Property::Label("Photograph thumbnail"),
        gtk::accessible::Property::Description(&format!(
            "Source {} × {} pixels",
            source.0, source.1
        )),
    ]);
}
