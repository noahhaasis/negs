//! The developed photograph's tones: 256 bins per channel of the cropped
//! photograph. The border is drawn on the canvas, never in the preview, so it
//! never counts. With it, the RGB waveform: signal level against horizontal
//! position in the shown crop. Mirrors `Histogram.swift`.

#[derive(Clone, Debug, PartialEq)]
pub struct Histogram {
    pub red: [u32; 256],
    pub green: [u32; 256],
    pub blue: [u32; 256],
}

use crate::Editor;
use adw::prelude::*;
use gtk::{gdk, gio, glib};
use std::{cell::RefCell, rc::Rc};

/// GTK owns only the small counted result; source buffers are scanned on a worker.
#[derive(Default)]
pub struct Panel {
    data: RefCell<Option<Scopes>>,
    area: RefCell<Option<gtk::DrawingArea>>,
}

pub fn clear(ui: &Editor) {
    ui.runtime.borrow_mut().hist_generation += 1;
    ui.scopes.data.take();
    ui.hist.take();
    if let Some(area) = ui.scopes.area.borrow().as_ref() {
        area.queue_draw();
    }
}

pub fn update(ui: &Rc<Editor>) {
    let state = ui.history.borrow().state.clone();
    let mut runtime = ui.runtime.borrow_mut();
    let Some(frame) = runtime.developed.clone() else {
        drop(runtime);
        clear(ui);
        return;
    };
    runtime.hist_generation += 1;
    let generation = runtime.hist_generation;
    drop(runtime);
    let ui = ui.clone();
    glib::MainContext::default().spawn_local(async move {
        let counted = gio::spawn_blocking(move || {
            Scopes::of(
                &frame.pixels,
                frame.width as usize,
                frame.height as usize,
                state.crop,
                state.straighten,
                state.rotation,
                1 << 20,
            )
        })
        .await;
        if ui.runtime.borrow().closing || ui.runtime.borrow().hist_generation != generation {
            return;
        }
        if let Ok(counted) = counted {
            ui.hist.replace(Some([
                counted.histogram.red.to_vec(),
                counted.histogram.green.to_vec(),
                counted.histogram.blue.to_vec(),
            ]));
            ui.scopes.data.replace(Some(counted));
            if let Some(area) = ui.scopes.area.borrow().as_ref() {
                area.queue_draw();
            }
        }
    });
}

fn height(value: f64) -> i32 {
    if value.is_finite() {
        value.round().clamp(88., 420.) as i32
    } else {
        88
    }
}

fn resize(ui: &Editor, value: f64) {
    let value = height(value);
    ui.preferences.borrow_mut().scope_height = value as f64;
    if let Some(area) = ui.scopes.area.borrow().as_ref() {
        area.set_content_height(value);
    }
}

pub fn build(ui: &Rc<Editor>) -> gtk::Box {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 2);
    let mode = gtk::DropDown::from_strings(&["Histogram", "Waveform"]);
    mode.set_selected(u32::from(ui.preferences.borrow().scope_mode == "Waveform"));
    mode.set_tooltip_text(Some(crate::help::SCOPE));
    mode.update_property(&[gtk::accessible::Property::Label("Scope mode")]);
    panel.append(&mode);
    let area = gtk::DrawingArea::new();
    area.set_content_height(height(ui.preferences.borrow().scope_height));
    area.set_hexpand(true);
    area.add_css_class("histogram");
    area.update_property(&[
        gtk::accessible::Property::Label(if mode.selected() == 1 {
            "RGB waveform"
        } else {
            "Histogram"
        }),
        gtk::accessible::Property::Description(
            "Red, green and blue tones of the developed photograph, excluding the border.",
        ),
    ]);
    ui.scopes.area.replace(Some(area.clone()));
    // Keep the existing histogram redraw hook working for initial camera placeholders.
    ui.histogram.replace(Some(area.clone()));
    area.set_draw_func(glib::clone!(
        #[weak]
        ui,
        move |_, cr, width, height| draw(&ui, cr, width, height)
    ));
    mode.connect_selected_notify(glib::clone!(
        #[weak]
        ui,
        #[weak]
        area,
        move |mode| {
            ui.preferences.borrow_mut().scope_mode = if mode.selected() == 1 {
                "Waveform"
            } else {
                "Histogram"
            }
            .into();
            area.update_property(&[gtk::accessible::Property::Label(if mode.selected() == 1 {
                "RGB waveform"
            } else {
                "Histogram"
            })]);
            area.queue_draw();
        }
    ));
    panel.append(&area);
    let grip = gtk::Button::with_label("↕");
    grip.add_css_class("flat");
    grip.set_cursor_from_name(Some("ns-resize"));
    grip.set_tooltip_text(Some(crate::help::SCOPE_RESIZE));
    grip.update_property(&[gtk::accessible::Property::Label("Resize scope")]);
    let origin = Rc::new(std::cell::Cell::new(88.));
    let drag = gtk::GestureDrag::new();
    drag.connect_drag_begin(glib::clone!(
        #[weak]
        ui,
        #[strong]
        origin,
        move |_, _, _| {
            origin.set(ui.preferences.borrow().scope_height);
        }
    ));
    drag.connect_drag_update(glib::clone!(
        #[weak]
        ui,
        move |_, _, dy| resize(&ui, origin.get() + dy)
    ));
    grip.add_controller(drag);
    let double = gtk::GestureClick::new();
    double.connect_pressed(glib::clone!(
        #[weak]
        ui,
        move |_, count, _, _| if count == 2 {
            resize(&ui, 88.);
        }
    ));
    grip.add_controller(double);
    let keys = gtk::EventControllerKey::new();
    keys.connect_key_pressed(glib::clone!(
        #[weak]
        ui,
        #[upgrade_or]
        glib::Propagation::Proceed,
        move |_, key, _, _| {
            let current = ui.preferences.borrow().scope_height;
            let next = match key {
                gdk::Key::Up => current - 8.,
                gdk::Key::Down => current + 8.,
                gdk::Key::Home => 88.,
                _ => return glib::Propagation::Proceed,
            };
            resize(&ui, next);
            glib::Propagation::Stop
        }
    ));
    grip.add_controller(keys);
    panel.append(&grip);
    panel
}

fn draw(ui: &Editor, cr: &gtk::cairo::Context, width: i32, height: i32) {
    let (w, h) = (width as f64, height as f64);
    cr.set_source_rgb(0.095, 0.098, 0.106);
    let _ = cr.paint();
    cr.set_source_rgba(1., 1., 1., 0.08);
    cr.set_line_width(0.5);
    for f in [0.25, 0.5, 0.75] {
        cr.move_to(0., h * f);
        cr.line_to(w, h * f);
    }
    let _ = cr.stroke();
    let data = ui.scopes.data.borrow();
    if ui.preferences.borrow().scope_mode == "Waveform" {
        let Some(scopes) = data.as_ref() else { return };
        let wave = &scopes.waveform;
        if wave.samples == 0 {
            return;
        }
        let bytes: Vec<u8> = wave
            .raster()
            .into_iter()
            .flat_map(|[r, g, b, a]| {
                if cfg!(target_endian = "little") {
                    [b, g, r, a]
                } else {
                    [a, r, g, b]
                }
            })
            .collect();
        let Ok(surface) = gtk::cairo::ImageSurface::create_for_data(
            bytes,
            gtk::cairo::Format::ARgb32,
            wave.width as i32,
            256,
            wave.width as i32 * 4,
        ) else {
            return;
        };
        let _ = cr.save();
        cr.scale(w / wave.width as f64, h / 256.);
        cr.set_operator(gtk::cairo::Operator::Add);
        let _ = cr.set_source_surface(&surface, 0., 0.);
        cr.source().set_filter(gtk::cairo::Filter::Nearest);
        let _ = cr.paint();
        let _ = cr.restore();
        return;
    }
    let legacy = ui.hist.borrow();
    let channels: [&[u32]; 3] = if let Some(data) = data.as_ref() {
        [
            &data.histogram.red,
            &data.histogram.green,
            &data.histogram.blue,
        ]
    } else if let Some(data) = legacy.as_ref() {
        [&data[0], &data[1], &data[2]]
    } else {
        return;
    };
    let inner = channels
        .iter()
        .flat_map(|c| c[1..255].iter())
        .copied()
        .max()
        .unwrap_or(0);
    let peak = data
        .as_ref()
        .map(|data| data.histogram.peak())
        .unwrap_or_else(|| {
            if inner > 0 {
                inner
            } else {
                channels
                    .iter()
                    .flat_map(|c| c.iter())
                    .copied()
                    .max()
                    .unwrap_or(0)
            }
        })
        .max(1) as f64;
    cr.set_operator(gtk::cairo::Operator::Add);
    for (bins, (r, g, b)) in
        channels
            .into_iter()
            .zip([(0.92, 0.31, 0.25), (0.36, 0.78, 0.49), (0.35, 0.57, 0.94)])
    {
        cr.set_source_rgba(r, g, b, 0.48);
        cr.move_to(0., h);
        for (i, count) in bins.iter().enumerate() {
            cr.line_to(
                i as f64 / 255. * w,
                h * (1. - (*count as f64 / peak).min(1.)),
            );
        }
        cr.line_to(w, h);
        cr.close_path();
        let _ = cr.fill();
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self {
            red: [0; 256],
            green: [0; 256],
            blue: [0; 256],
        }
    }
}

impl Histogram {
    /// The tallest bin, leaving out pure black and white so clipping doesn't
    /// flatten the rest; their spikes run off the top instead.
    pub fn peak(&self) -> u32 {
        let channels = [&self.red, &self.green, &self.blue];
        let inner = channels
            .iter()
            .map(|c| c[1..255].iter().copied().max().unwrap_or(0))
            .max()
            .unwrap_or(0);
        if inner > 0 {
            inner
        } else {
            channels
                .iter()
                .map(|c| c.iter().copied().max().unwrap_or(0))
                .max()
                .unwrap_or(0)
        }
    }
}

/// RGB signal level against horizontal position in the shown crop. Bins are
/// row-major, with level 0 in row 0 and 255 in row 255.
#[derive(Clone, Debug, PartialEq)]
pub struct Waveform {
    pub width: usize,
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    pub samples: usize,
}

impl Default for Waveform {
    fn default() -> Self {
        Self {
            width: 1,
            red: vec![0; 256],
            green: vec![0; 256],
            blue: vec![0; 256],
            samples: 0,
        }
    }
}

impl Waveform {
    /// A small raster, `width` x 256 with level 255 in the top row, as
    /// premultiplied red, green, blue and alpha: a log scale of the counts
    /// so faint levels show. Drawing then doesn't depend on the photo's size.
    pub fn raster(&self) -> Vec<[u8; 4]> {
        let peak = self
            .red
            .iter()
            .chain(&self.green)
            .chain(&self.blue)
            .copied()
            .max()
            .unwrap_or(0);
        let mut pixels = vec![[0u8; 4]; self.width * 256];
        if peak == 0 {
            return pixels;
        }
        let normalizer = (peak as f64).ln_1p();
        let intensity = |count: u32| {
            if count == 0 {
                0
            } else {
                (255.0 * (0.18 + 0.82 * (count as f64).ln_1p() / normalizer)).round() as u8
            }
        };
        for level in 0..256 {
            for x in 0..self.width {
                let bin = level * self.width + x;
                let (r, g, b) = (
                    intensity(self.red[bin]),
                    intensity(self.green[bin]),
                    intensity(self.blue[bin]),
                );
                pixels[(255 - level) * self.width + x] = [r, g, b, r.max(g).max(b)];
            }
        }
        pixels
    }
}

/// Both scopes from one bounded grid over the crop.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scopes {
    pub histogram: Histogram,
    pub waveform: Waveform,
}

impl Scopes {
    /// Samples the crop of an RGBA preview on an even grid of at most
    /// `max_samples` pixels, which keeps the shape and takes a few
    /// milliseconds at any size; the budget also bounds very thin panoramas.
    /// The waveform runs across the photograph as shown, after `rotation`
    /// quarter turns clockwise.
    pub fn of(
        rgba: &[u8],
        width: usize,
        height: usize,
        crop: Option<[f64; 4]>,
        angle: f64,
        rotation: u8,
        max_samples: usize,
    ) -> Scopes {
        let [x, y, w, h] = crop.unwrap_or([0., 0., 1., 1.]);
        let (awf, ahf) = (w * width as f64, h * height as f64);
        let origin = (x * width as f64, y * height as f64);
        if ![x, y, w, h, angle].iter().all(|v| v.is_finite())
            || awf < 1.0
            || ahf < 1.0
            || max_samples == 0
            || width
                .checked_mul(height)
                .and_then(|n| n.checked_mul(4))
                .is_none_or(|n| rgba.len() < n)
        {
            return Scopes::default();
        }
        let (aw, ah) = (awf as usize, ahf as usize);
        let columns = aw
            .min(max_samples)
            .min(((max_samples as f64 * aw as f64 / ah as f64).sqrt() as usize).max(1));
        let rows = ah.min((max_samples / columns).max(1));
        let turn = rotation % 4;
        let shown_columns = if turn % 2 == 0 { columns } else { rows };
        let wave_width = shown_columns.min(256);
        let mut histogram = Histogram::default();
        let (mut red, mut green, mut blue) = (
            vec![0u32; wave_width * 256],
            vec![0u32; wave_width * 256],
            vec![0u32; wave_width * 256],
        );
        let mut samples = 0;
        // A straightened crop is in the turned frame: turn each sample back
        // to the photograph.
        let (sin, cos) = (-angle * std::f64::consts::PI / 180.0).sin_cos();
        let (cx, cy) = (width as f64 / 2.0, height as f64 / 2.0);
        for row in 0..rows {
            for column in 0..columns {
                let dx = origin.0 + (column as f64 + 0.5) * awf / columns as f64 - cx;
                let dy = origin.1 + (row as f64 + 0.5) * ahf / rows as f64 - cy;
                let (x, y) = (
                    (cx + dx * cos - dy * sin).floor(),
                    (cy + dx * sin + dy * cos).floor(),
                );
                if x < 0.0 || y < 0.0 || x as usize >= width || y as usize >= height {
                    continue;
                }
                let p = (y as usize * width + x as usize) * 4;
                let (r, g, b) = (rgba[p] as usize, rgba[p + 1] as usize, rgba[p + 2] as usize);
                histogram.red[r] += 1;
                histogram.green[g] += 1;
                histogram.blue[b] += 1;
                let shown = match turn {
                    1 => rows - 1 - row,
                    2 => columns - 1 - column,
                    3 => row,
                    _ => column,
                };
                let wx = shown * wave_width / shown_columns;
                red[r * wave_width + wx] += 1;
                green[g * wave_width + wx] += 1;
                blue[b * wave_width + wx] += 1;
                samples += 1;
            }
        }
        Scopes {
            histogram,
            waveform: Waveform {
                width: wave_width,
                red,
                green,
                blue,
                samples,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn halves(width: usize, height: usize) -> Vec<u8> {
        let mut pixels = vec![0; width * height * 4];
        for y in 0..height {
            for x in 0..width {
                let color = if x == width / 2 {
                    [200, 0, 0, 255]
                } else if x < width / 2 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                };
                pixels[(y * width + x) * 4..(y * width + x + 1) * 4].copy_from_slice(&color);
            }
        }
        pixels
    }

    #[test]
    fn histogram_and_waveform_share_crop_samples_and_positions() {
        let image = halves(100, 60);
        let scopes = Scopes::of(&image, 100, 60, None, 0., 0, 1 << 20);
        assert_eq!(scopes.histogram.red.iter().sum::<u32>(), 6000);
        assert_eq!(
            (
                scopes.histogram.red[0],
                scopes.histogram.red[200],
                scopes.histogram.red[255]
            ),
            (3000, 60, 2940)
        );
        let wave = scopes.waveform;
        assert_eq!(
            (
                wave.width,
                wave.samples,
                wave.red[0],
                wave.red[200 * 100 + 50],
                wave.red[255 * 100 + 99]
            ),
            (100, 6000, 60, 60, 60)
        );
        let crop = Scopes::of(&image, 100, 60, Some([0., 0., 0.4, 1.]), 0., 0, 1 << 20);
        assert_eq!(crop.histogram.red[0], 2400);
        assert_eq!(crop.histogram.red[255], 0);
        assert!(wave.raster()[255 * 100][0] > 0);
        assert!(wave.raster()[99][0] > 0);
    }

    #[test]
    fn waveform_follows_rotation_and_straightening() {
        let image = halves(100, 60);
        for (angle, turn) in [(180., 0), (0., 2)] {
            let wave = Scopes::of(&image, 100, 60, None, angle, turn, 1 << 20).waveform;
            assert_eq!((wave.red[255 * 100], wave.red[99]), (60, 60));
        }
        for turn in [1, 3] {
            let wave = Scopes::of(&image, 100, 60, None, 0., turn, 1 << 20).waveform;
            assert_eq!(wave.width, 60);
            for x in 0..60 {
                assert_eq!(
                    (wave.red[x], wave.red[200 * 60 + x], wave.red[255 * 60 + x]),
                    (50, 1, 49)
                );
            }
        }
    }

    #[test]
    fn sample_budget_bounds_extreme_aspect_ratios() {
        for (width, height) in [(10000, 1), (1, 10000), (101, 61)] {
            let scopes = Scopes::of(&halves(width, height), width, height, None, 0., 0, 127);
            assert!(
                scopes.waveform.samples > 0
                    && scopes.waveform.samples <= 127
                    && scopes.waveform.width <= 256
            );
            assert_eq!(
                scopes.histogram.red.iter().sum::<u32>() as usize,
                scopes.waveform.samples
            );
        }
        assert_eq!(Scopes::of(&[], 0, 0, None, 0., 0, 100), Scopes::default());
        assert_eq!(
            Scopes::of(&[0; 4], 1, 1, None, f64::NAN, 0, 100),
            Scopes::default()
        );
        assert_eq!(
            Scopes::of(&[0; 4], usize::MAX, 2, None, 0., 0, 100),
            Scopes::default()
        );
    }

    #[test]
    fn clipping_spikes_do_not_flatten_the_rest_of_the_histogram() {
        let mut histogram = Histogram::default();
        histogram.red[0] = 1000;
        histogram.green[255] = 900;
        histogram.blue[17] = 30;
        assert_eq!(histogram.peak(), 30);
        assert_eq!((height(0.), height(f64::NAN), height(500.)), (88, 88, 420));
    }
}
