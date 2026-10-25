use gtk::{gdk, glib, graphene, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Canvas {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub source_size: Cell<(i32, i32)>,
        pub zoom: Cell<f64>,
        pub fitting: Cell<bool>,
        pub pan: Cell<(f64, f64)>,
        pub rotation: Cell<u8>,
        pub border: Cell<u8>,
        pub crop: Cell<Option<[f64; 4]>>,
        pub cropping: Cell<bool>,
        pub draft: Cell<[f64; 4]>,
        pub drag_origin: Cell<(f64, f64)>,
        pub crop_origin: Cell<[f64; 4]>,
        pub crop_handle: Cell<u8>,
        pub drag_point: Cell<(f64, f64)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for Canvas {
        const NAME: &'static str = "NegsPhotoCanvas";
        type Type = super::Canvas;
        type ParentType = gtk::Widget;
        fn class_init(klass: &mut Self::Class) {
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }
    impl ObjectImpl for Canvas {}
    impl WidgetImpl for Canvas {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let obj = self.obj();
            let Some(texture) = self.texture.borrow().clone() else {
                return;
            };
            let (iw, ih) = obj.image_size();
            let crop = if self.cropping.get() {
                [0., 0., 1., 1.]
            } else {
                self.crop.get().unwrap_or([0., 0., 1., 1.])
            };
            let cw = iw * crop[2];
            let ch = ih * crop[3];
            let (ow, oh) = if self.rotation.get() % 2 == 1 {
                (ch, cw)
            } else {
                (cw, ch)
            };
            let border = if self.cropping.get() {
                0
            } else {
                self.border.get()
            };
            let mat = if border > 0 { ow.max(oh) * 0.045 } else { 0. };
            let zoom = obj.display_scale();
            let (px, py) = self.pan.get();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (obj.width() as f64 / 2. + px) as f32,
                (obj.height() as f64 / 2. + py) as f32,
            ));
            snapshot.scale(zoom as f32, zoom as f32);
            if border > 0 {
                let color = if border == 3 {
                    gdk::RGBA::BLACK
                } else {
                    gdk::RGBA::WHITE
                };
                rect(
                    snapshot,
                    &color,
                    -ow / 2. - mat,
                    -oh / 2. - mat,
                    ow + mat * 2.,
                    oh + mat * 2.,
                );
                if border == 2 {
                    rect(
                        snapshot,
                        &gdk::RGBA::BLACK,
                        -ow / 2. - 1.5 / zoom,
                        -oh / 2. - 1.5 / zoom,
                        ow + 3. / zoom,
                        oh + 3. / zoom,
                    );
                }
            }
            snapshot.rotate(self.rotation.get() as f32 * 90.);
            snapshot.translate(&graphene::Point::new((-cw / 2.) as f32, (-ch / 2.) as f32));
            snapshot.push_clip(&graphene::Rect::new(0., 0., cw as f32, ch as f32));
            snapshot.append_texture(
                &texture,
                &graphene::Rect::new(
                    (-crop[0] * iw) as f32,
                    (-crop[1] * ih) as f32,
                    iw as f32,
                    ih as f32,
                ),
            );
            snapshot.pop();
            if self.cropping.get() {
                let [x, y, w, h] = self.draft.get();
                let (x, y, w, h) = (x * iw, y * ih, w * iw, h * ih);
                let dim = gdk::RGBA::new(0., 0., 0., 0.60);
                rect(snapshot, &dim, 0., 0., iw, y);
                rect(snapshot, &dim, 0., y + h, iw, ih - y - h);
                rect(snapshot, &dim, 0., y, x, h);
                rect(snapshot, &dim, x + w, y, iw - x - w, h);
                let thin = 1. / zoom;
                for n in 0..=3 {
                    let color = if n == 0 || n == 3 {
                        gdk::RGBA::WHITE
                    } else {
                        gdk::RGBA::new(1., 1., 1., 0.35)
                    };
                    rect(snapshot, &color, x + w * n as f64 / 3., y, thin, h);
                    rect(snapshot, &color, x, y + h * n as f64 / 3., w, thin);
                }
                let line = 3. / zoom;
                let len = 16. / zoom;
                for (hx, hy, sx, sy) in [
                    (x, y, 1., 1.),
                    (x + w, y, -1., 1.),
                    (x, y + h, 1., -1.),
                    (x + w, y + h, -1., -1.),
                ] {
                    rect(
                        snapshot,
                        &gdk::RGBA::WHITE,
                        hx + if sx < 0. { -len } else { 0. },
                        hy - line / 2.,
                        len,
                        line,
                    );
                    rect(
                        snapshot,
                        &gdk::RGBA::WHITE,
                        hx - line / 2.,
                        hy + if sy < 0. { -len } else { 0. },
                        line,
                        len,
                    );
                }
            }
            snapshot.restore();
        }
    }
}

fn rect(s: &gtk::Snapshot, c: &gdk::RGBA, x: f64, y: f64, w: f64, h: f64) {
    if w > 0. && h > 0. {
        s.append_color(
            c,
            &graphene::Rect::new(x as f32, y as f32, w as f32, h as f32),
        );
    }
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl Canvas {
    fn image_size(&self) -> (f64, f64) {
        let (w, h) = self.imp().source_size.get();
        (w.max(1) as f64, h.max(1) as f64)
    }
    pub fn new() -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_hexpand(true);
        obj.set_vexpand(true);
        obj.set_focusable(true);
        obj.set_cursor_from_name(Some("grab"));
        obj.update_property(&[gtk::accessible::Property::Label("Photograph preview"),
            gtk::accessible::Property::Description("Drag to pan. Control and scroll to zoom. Double click to fit. Crop corners can be dragged.")]);
        obj.imp().fitting.set(true);
        obj.imp().zoom.set(1.);
        let click = gtk::GestureClick::new();
        click.connect_pressed(glib::clone!(
            #[weak]
            obj,
            move |_, n, _, _| {
                obj.grab_focus();
                if n == 2 {
                    obj.fit();
                }
            }
        ));
        obj.add_controller(click);
        let drag = gtk::GestureDrag::new();
        drag.connect_drag_begin(glib::clone!(
            #[weak]
            obj,
            move |_, x, y| {
                let imp = obj.imp();
                imp.drag_origin.set(imp.pan.get());
                imp.crop_origin.set(imp.draft.get());
                imp.drag_point.set(obj.image_point(x, y));
                let (nx, ny) = imp.drag_point.get();
                let [cx, cy, cw, ch] = imp.draft.get();
                let handle = [(cx, cy), (cx + cw, cy), (cx, cy + ch), (cx + cw, cy + ch)]
                    .iter()
                    .position(|(hx, hy)| (nx - hx).abs() < 0.05 && (ny - hy).abs() < 0.05)
                    .map(|n| n as u8 + 1)
                    .unwrap_or(0);
                imp.crop_handle.set(handle);
                obj.set_cursor_from_name(Some("grabbing"));
            }
        ));
        drag.connect_drag_update(glib::clone!(
            #[weak]
            obj,
            move |gesture, dx, dy| {
                let imp = obj.imp();
                if imp.cropping.get() {
                    let Some((sx, sy)) = gesture.start_point() else {
                        return;
                    };
                    let (nx, ny) = obj.image_point(sx + dx, sy + dy);
                    let (ox, oy) = imp.drag_point.get();
                    let [x, y, w, h] = imp.crop_origin.get();
                    let (dx, dy) = (nx - ox, ny - oy);
                    let draft = match imp.crop_handle.get() {
                        1 => {
                            let xx = (x + dx).clamp(0., x + w - 0.1);
                            let yy = (y + dy).clamp(0., y + h - 0.1);
                            [xx, yy, x + w - xx, y + h - yy]
                        }
                        2 => {
                            let yy = (y + dy).clamp(0., y + h - 0.1);
                            [x, yy, (w + dx).clamp(0.1, 1. - x), y + h - yy]
                        }
                        3 => {
                            let xx = (x + dx).clamp(0., x + w - 0.1);
                            [xx, y, x + w - xx, (h + dy).clamp(0.1, 1. - y)]
                        }
                        4 => [
                            x,
                            y,
                            (w + dx).clamp(0.1, 1. - x),
                            (h + dy).clamp(0.1, 1. - y),
                        ],
                        _ => [(x + dx).clamp(0., 1. - w), (y + dy).clamp(0., 1. - h), w, h],
                    };
                    imp.draft.set(draft);
                } else {
                    let (x, y) = imp.drag_origin.get();
                    imp.pan.set((x + dx, y + dy));
                }
                obj.queue_draw();
            }
        ));
        drag.connect_drag_end(glib::clone!(
            #[weak]
            obj,
            move |_, _, _| {
                obj.set_cursor_from_name(Some("grab"));
            }
        ));
        obj.add_controller(drag);
        let scroll = gtk::EventControllerScroll::new(gtk::EventControllerScrollFlags::BOTH_AXES);
        scroll.connect_scroll(glib::clone!(
            #[weak]
            obj,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |event, dx, dy| {
                if event
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK)
                {
                    let point = event
                        .current_event()
                        .and_then(|e| e.position())
                        .unwrap_or((obj.width() as f64 / 2., obj.height() as f64 / 2.));
                    obj.zoom_at((-dy * 0.12).exp(), point);
                } else {
                    let (x, y) = obj.imp().pan.get();
                    obj.imp().pan.set((x - dx * 30., y - dy * 30.));
                    obj.queue_draw();
                }
                glib::Propagation::Stop
            }
        ));
        obj.add_controller(scroll);
        obj
    }

    pub fn display_scale(&self) -> f64 {
        let imp = self.imp();
        if !imp.fitting.get() {
            return imp.zoom.get();
        }
        let Some(_) = imp.texture.borrow().clone() else {
            return 1.;
        };
        let crop = if imp.cropping.get() {
            [0., 0., 1., 1.]
        } else {
            imp.crop.get().unwrap_or([0., 0., 1., 1.])
        };
        let (w, h) = self.image_size();
        let (w, h) = (w * crop[2], h * crop[3]);
        let (w, h) = if imp.rotation.get() % 2 == 1 {
            (h, w)
        } else {
            (w, h)
        };
        let mat = if imp.border.get() > 0 && !imp.cropping.get() {
            w.max(h) * 0.09
        } else {
            0.
        };
        (((self.width() - 72).max(1) as f64 / (w + mat))
            .min((self.height() - 72).max(1) as f64 / (h + mat)))
        .max(0.005)
    }

    fn image_point(&self, x: f64, y: f64) -> (f64, f64) {
        let Some(_) = self.imp().texture.borrow().clone() else {
            return (0., 0.);
        };
        let (px, py) = self.imp().pan.get();
        let z = self.display_scale();
        let x = (x - self.width() as f64 / 2. - px) / z;
        let y = (y - self.height() as f64 / 2. - py) / z;
        let (x, y) = match self.imp().rotation.get() % 4 {
            1 => (y, -x),
            2 => (-x, -y),
            3 => (-y, x),
            _ => (x, y),
        };
        let (w, h) = self.image_size();
        (x / w + 0.5, y / h + 0.5)
    }

    pub fn set_texture(&self, t: &gdk::Texture, source: (i32, i32)) {
        self.imp().texture.replace(Some(t.clone()));
        self.imp().source_size.set(source);
        self.fit();
    }
    pub fn fit(&self) {
        self.imp().fitting.set(true);
        self.imp().pan.set((0., 0.));
        self.queue_draw();
    }
    pub fn actual_size(&self) {
        self.imp().fitting.set(false);
        let scale = self
            .native()
            .and_then(|n| n.surface())
            .map(|s| s.scale())
            .unwrap_or(1.);
        self.imp().zoom.set(1. / scale);
        self.imp().pan.set((0., 0.));
        self.queue_draw();
    }
    pub fn zoom_at(&self, factor: f64, point: (f64, f64)) {
        let old = self.display_scale();
        let new = (old * factor).clamp(0.005, 16.);
        let (px, py) = self.imp().pan.get();
        let (cx, cy) = (self.width() as f64 / 2., self.height() as f64 / 2.);
        self.imp().pan.set((
            point.0 - cx - (point.0 - cx - px) * new / old,
            point.1 - cy - (point.1 - cy - py) * new / old,
        ));
        self.imp().zoom.set(new);
        self.imp().fitting.set(false);
        self.queue_draw();
    }
    pub fn apply(&self, state: &crate::model::State) {
        self.imp().rotation.set(state.rotation);
        self.imp().border.set(state.border);
        self.imp().crop.set(state.crop);
        self.queue_draw();
    }
    pub fn begin_crop(&self) {
        self.imp()
            .draft
            .set(self.imp().crop.get().unwrap_or([0.08, 0.08, 0.84, 0.84]));
        self.imp().cropping.set(true);
        self.fit();
    }
    pub fn end_crop(&self) -> [f64; 4] {
        self.imp().cropping.set(false);
        self.fit();
        self.imp().draft.get()
    }
    pub fn cropping(&self) -> bool {
        self.imp().cropping.get()
    }
    pub fn crop_ratio(&self, ratio: f64) {
        if let Some(t) = self.imp().texture.borrow().as_ref() {
            let w = 0.84f64.min(0.84 * t.height() as f64 * ratio / t.width() as f64);
            let h = w * t.width() as f64 / ratio / t.height() as f64;
            self.imp().draft.set([(1. - w) / 2., (1. - h) / 2., w, h]);
            self.queue_draw();
        }
    }
}
