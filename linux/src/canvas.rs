use crate::border::{Layout, Settings, Style};
use gtk::{gdk, glib, graphene, prelude::*, subclass::prelude::*};
use std::cell::{Cell, RefCell};

pub fn srgb_texture(
    width: i32,
    height: i32,
    format: gdk::MemoryFormat,
    bytes: &glib::Bytes,
    stride: usize,
) -> gdk::Texture {
    texture(width, height, format, bytes, stride, &gdk::ColorState::srgb())
}

/// The helper's name for its wide proxy: Rec. 2020 primaries, sRGB curve.
pub const WIDE_DISPLAY_SPACE: &str = "Rec. 2020 (sRGB transfer)";

/// The colour state of a helper frame (`display_color_space`): sRGB, or
/// BT.2020 primaries with the sRGB transfer (CICP 9/13/0, full range), so
/// GTK converts a wide-gamut preview for the display.
pub fn display_color_state(display_space: &str) -> gdk::ColorState {
    if display_space != WIDE_DISPLAY_SPACE {
        return gdk::ColorState::srgb();
    }
    let cicp = gdk::CicpParams::new();
    cicp.set_color_primaries(9);
    cicp.set_transfer_function(13);
    cicp.set_matrix_coefficients(0);
    cicp.set_range(gdk::CicpRange::Full);
    cicp.build_color_state().unwrap_or_else(|_| gdk::ColorState::srgb())
}

pub fn texture(
    width: i32,
    height: i32,
    format: gdk::MemoryFormat,
    bytes: &glib::Bytes,
    stride: usize,
    color_state: &gdk::ColorState,
) -> gdk::Texture {
    gdk::MemoryTextureBuilder::new()
        .set_width(width)
        .set_height(height)
        .set_format(format)
        .set_color_state(color_state)
        .set_bytes(Some(bytes))
        .set_stride(stride)
        .build()
}

#[derive(Clone)]
struct Asset {
    texture: gdk::Texture,
    source: (f64, f64),
    window: [f64; 4],
}

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct Canvas {
        pub texture: RefCell<Option<gdk::Texture>>,
        pub source_size: Cell<(i32, i32)>,
        pub photo_size: Cell<(i32, i32)>,
        pub zoom: Cell<f64>,
        pub fitting: Cell<bool>,
        pub pan: Cell<(f64, f64)>,
        pub rotation: Cell<u8>,
        pub border: RefCell<Settings>,
        pub(super) asset: RefCell<Option<Asset>>,
        pub asset_key: RefCell<Option<String>>,
        pub(super) asset_cache: RefCell<crate::border::asset::Cache<Asset>>,
        pub asset_loading: Cell<bool>,
        pub crop: Cell<Option<[f64; 4]>>,
        pub angle: Cell<f64>,
        pub draft_angle: Cell<f64>,
        pub angle_base: Cell<[f64; 4]>,
        pub ratio: Cell<f64>,
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
            // Keep the display canvas opaque independently of its container/theme,
            // including when an input image contains transparent pixels.
            rect(
                snapshot,
                &gdk::RGBA::new(24. / 255., 25. / 255., 27. / 255., 1.),
                0.,
                0.,
                obj.width() as f64,
                obj.height() as f64,
            );
            let Some(texture) = self.texture.borrow().clone() else {
                return;
            };
            // Clip before applying image transforms, including borders and crop
            // overlays, so zooming or panning cannot paint over adjacent UI.
            snapshot.push_clip(&graphene::Rect::new(
                0.,
                0.,
                obj.width() as f32,
                obj.height() as f32,
            ));
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
            let border = self.border.borrow().clone();
            let shown = !self.cropping.get() && border.visible(ow, oh);
            let asset = self.asset.borrow().clone();
            let layout = obj.border_layout(ow, oh);
            let zoom = obj.display_scale();
            let (px, py) = self.pan.get();
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (obj.width() as f64 / 2. + px) as f32,
                (obj.height() as f64 / 2. + py) as f32,
            ));
            snapshot.scale(zoom as f32, zoom as f32);
            if shown {
                let color = if matches!(border.style, Style::Solid | Style::Keyline) {
                    border.color.rgba()
                } else if border.instagram {
                    gdk::RGBA::WHITE
                } else {
                    gdk::RGBA::BLACK
                };
                rect(
                    snapshot,
                    &color,
                    -layout.width / 2.,
                    -layout.height / 2.,
                    layout.width,
                    layout.height,
                );
                if matches!(border.style, Style::Film | Style::Film35) {
                    rect(
                        snapshot,
                        &gdk::RGBA::BLACK,
                        -layout.frame_width / 2.,
                        -layout.frame_height / 2.,
                        layout.frame_width,
                        layout.frame_height,
                    );
                }
                if border.style == Style::Keyline {
                    rect(
                        snapshot,
                        &border.line_color.rgba(),
                        -ow / 2. - layout.line,
                        -oh / 2. - layout.line,
                        ow + 2. * layout.line,
                        oh + 2. * layout.line,
                    );
                }
            }
            snapshot.save();
            snapshot.rotate(self.rotation.get() as f32 * 90.);
            snapshot.translate(&graphene::Point::new((-cw / 2.) as f32, (-ch / 2.) as f32));
            snapshot.push_clip(&graphene::Rect::new(0., 0., cw as f32, ch as f32));
            snapshot.save();
            snapshot.translate(&graphene::Point::new(
                (iw / 2. - crop[0] * iw) as f32,
                (ih / 2. - crop[1] * ih) as f32,
            ));
            snapshot.rotate(if self.cropping.get() {
                self.draft_angle.get()
            } else {
                self.angle.get()
            } as f32);
            snapshot.translate(&graphene::Point::new(
                (-iw / 2. + crop[0] * iw) as f32,
                (-ih / 2. + crop[1] * ih) as f32,
            ));
            snapshot.append_texture(
                &texture,
                &graphene::Rect::new(
                    (-crop[0] * iw) as f32,
                    (-crop[1] * ih) as f32,
                    iw as f32,
                    ih as f32,
                ),
            );
            snapshot.restore();
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
                for (hx, hy, horizontal) in [
                    (x + w / 2., y, true),
                    (x + w / 2., y + h, true),
                    (x, y + h / 2., false),
                    (x + w, y + h / 2., false),
                ] {
                    rect(
                        snapshot,
                        &gdk::RGBA::WHITE,
                        hx - if horizontal { 8. / zoom } else { 1.5 / zoom },
                        hy - if horizontal { 1.5 / zoom } else { 8. / zoom },
                        if horizontal { 16. / zoom } else { 3. / zoom },
                        if horizontal { 3. / zoom } else { 16. / zoom },
                    );
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
            if shown && let Some(asset) = asset {
                match border.style {
                    Style::Film => {
                        let inset = (ow.min(oh) * 0.003).round().max(1.);
                        nine_patch(
                            snapshot,
                            &asset.texture,
                            ow + 2. * inset,
                            oh + 2. * inset,
                            (ow.min(oh) * 0.025).round().max(2.),
                        );
                    }
                    Style::Film35 => {
                        let turned = (asset.window[2] * asset.source.0
                            > asset.window[3] * asset.source.1)
                            != (ow > oh);
                        snapshot.save();
                        if turned {
                            snapshot.rotate(-90.);
                        }
                        let (w, h) = if turned {
                            (layout.frame_height, layout.frame_width)
                        } else {
                            (layout.frame_width, layout.frame_height)
                        };
                        snapshot.append_texture(
                            &asset.texture,
                            &graphene::Rect::new(
                                (-w / 2.) as f32,
                                (-h / 2.) as f32,
                                w as f32,
                                h as f32,
                            ),
                        );
                        snapshot.restore();
                    }
                    _ => {}
                }
            }
            snapshot.restore();
            snapshot.pop();
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

fn nine_patch(s: &gtk::Snapshot, texture: &gdk::Texture, w: f64, h: f64, pad: f64) {
    s.save();
    let (w, h) = if h > w {
        s.rotate(-90.);
        (h, w)
    } else {
        (w, h)
    };
    let (tw, th) = (texture.width() as f64, texture.height() as f64);
    let scale = w.min(h) / tw.min(th);
    let sx = (pad / scale).min(tw / 2.);
    let sy = (pad / scale).min(th / 2.);
    let xs = [0., sx, tw - sx, tw];
    let ys = [0., sy, th - sy, th];
    let xd = [-w / 2., -w / 2. + pad, w / 2. - pad, w / 2.];
    let yd = [-h / 2., -h / 2. + pad, h / 2. - pad, h / 2.];
    for y in 0..3 {
        for x in 0..3 {
            let (dw, dh) = (xd[x + 1] - xd[x], yd[y + 1] - yd[y]);
            let (kx, ky) = (dw / (xs[x + 1] - xs[x]), dh / (ys[y + 1] - ys[y]));
            if dw <= 0. || dh <= 0. {
                continue;
            }
            s.push_clip(&graphene::Rect::new(
                xd[x] as f32,
                yd[y] as f32,
                dw as f32,
                dh as f32,
            ));
            s.append_texture(
                texture,
                &graphene::Rect::new(
                    (xd[x] - xs[x] * kx) as f32,
                    (yd[y] - ys[y] * ky) as f32,
                    (tw * kx) as f32,
                    (th * ky) as f32,
                ),
            );
            s.pop();
        }
    }
    s.restore();
}

glib::wrapper! {
    pub struct Canvas(ObjectSubclass<imp::Canvas>) @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

#[cfg(test)]
mod border_tests {
    use super::*;

    #[test]
    #[ignore = "Requires the real desktop and system PNG loader; run explicitly with --ignored"]
    fn border_texture_reuse_and_latest_selection() {
        gtk::init().unwrap();
        let data = crate::data_root();
        for key in ["film", "full/kodak_portra_400", "half/generic"] {
            let decoded = crate::border::asset::load(&data, key).unwrap();
            let path = if key == "film" {
                data.join("borders/film.png")
            } else {
                data.join("borders/35mm").join(format!("{key}.png"))
            };
            let reference = gtk::gdk_pixbuf::Pixbuf::from_file(path).unwrap();
            assert_eq!(
                decoded.bytes.as_slice(),
                reference.read_pixel_bytes().as_ref(),
                "PNG pixel parity: {key}"
            );
        }
        let canvas = Canvas::new();
        let texture = srgb_texture(
            3,
            2,
            gdk::MemoryFormat::R8g8b8,
            &glib::Bytes::from_owned(vec![128; 18]),
            9,
        );
        canvas.set_texture(&texture, (1200, 800));
        let mut state = crate::fixtures::state();
        state.border.enabled = true;
        state.border.style = Style::Film;
        let wait = || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while canvas.imp().asset_loading.get() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "Border load timed out"
                );
                glib::MainContext::default().iteration(false);
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        };
        canvas.apply(&state);
        wait();
        let film = canvas.imp().asset.borrow().clone().unwrap().texture;
        state.border.enabled = false;
        canvas.apply(&state);
        assert!(canvas.imp().asset.borrow().is_none());
        state.border.enabled = true;
        canvas.apply(&state);
        assert!(!canvas.imp().asset_loading.get());
        assert_eq!(canvas.imp().asset.borrow().as_ref().unwrap().texture, film);

        state.border.style = Style::Film35;
        canvas.apply(&state);
        assert!(canvas.imp().asset_loading.get());
        for id in ["kodak_ektar_100", "kodak_gold_200", "kodak_portra_400"] {
            state.choices.insert("Film ID".into(), id.into());
            canvas.apply(&state);
        }
        wait();
        assert_eq!(
            canvas.imp().asset_key.borrow().as_deref(),
            Some("full/kodak_portra_400")
        );
        let full = canvas.imp().asset.borrow().clone().unwrap();
        assert_eq!(full.source, (3800., 3500.));
        canvas.apply(&state);
        assert_eq!(
            canvas.imp().asset.borrow().as_ref().unwrap().texture,
            full.texture
        );

        // A stale completion must start the latest uncached request, skipping
        // intermediate stock selections rather than publishing the old frame.
        for id in ["kodak_gold_200", "kodak_portra_800", "kodak_ektar_100"] {
            state.choices.insert("Film ID".into(), id.into());
            canvas.apply(&state);
        }
        wait();
        assert_eq!(
            canvas.imp().asset_key.borrow().as_deref(),
            Some("full/kodak_ektar_100")
        );
        let latest = canvas.imp().asset.borrow().clone().unwrap();
        let expected = crate::border::asset::load(&data, "full/kodak_ektar_100").unwrap();
        let reference = srgb_texture(
            expected.width,
            expected.height,
            gdk::MemoryFormat::R8g8b8a8,
            &glib::Bytes::from_owned(expected.bytes),
            expected.stride,
        );
        let mut actual_bytes =
            vec![0; latest.texture.width() as usize * latest.texture.height() as usize * 4];
        let mut expected_bytes = actual_bytes.clone();
        latest
            .texture
            .download(&mut actual_bytes, latest.texture.width() as usize * 4);
        reference.download(&mut expected_bytes, reference.width() as usize * 4);
        assert_eq!(
            actual_bytes, expected_bytes,
            "Stale stock texture was published"
        );

        canvas.set_texture(&texture, (800, 600));
        state.rotation = 1;
        canvas.apply(&state);
        wait();
        assert_eq!(
            canvas.imp().asset.borrow().as_ref().unwrap().source,
            (1900., 3500.)
        );
        canvas.set_texture(&texture, (1200, 800));
        state
            .choices
            .insert("Film ID".into(), "kodak_portra_800".into());
        canvas.apply(&state);
        state.border.enabled = false;
        canvas.apply(&state);
        wait();
        assert!(
            canvas.imp().asset.borrow().is_none(),
            "Stale border appeared after disabling"
        );
        canvas.set_texture(&texture, (1000, 1000));
        state.border.enabled = true;
        canvas.apply(&state);
        assert!(
            canvas.imp().asset.borrow().is_none(),
            "35mm appeared on an unsupported crop"
        );
    }
}

impl Canvas {
    pub fn border_ready(&self) -> bool {
        self.imp().asset_key.borrow().is_none() || self.imp().asset.borrow().is_some()
    }

    fn border_layout(&self, w: f64, h: f64) -> Layout {
        let s = self.imp().border.borrow();
        if self.imp().cropping.get() || !s.visible(w, h) {
            return Layout {
                width: w,
                height: h,
                frame_width: w,
                frame_height: h,
                inset: 0.,
                line: 0.,
            };
        }
        let asset = self.imp().asset.borrow();
        s.layout(
            w,
            h,
            asset.as_ref().map(|a| (a.source.0, a.source.1, a.window)),
        )
    }
    fn image_size(&self) -> (f64, f64) {
        let (w, h) = self.imp().photo_size.get();
        (w.max(1) as f64, h.max(1) as f64)
    }
    pub fn new() -> Self {
        let obj: Self = glib::Object::builder().build();
        obj.set_hexpand(true);
        obj.set_vexpand(true);
        obj.set_overflow(gtk::Overflow::Hidden);
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
                let handle = [
                    (cx, cy),
                    (cx + cw, cy),
                    (cx, cy + ch),
                    (cx + cw, cy + ch),
                    (cx, cy + ch / 2.),
                    (cx + cw, cy + ch / 2.),
                    (cx + cw / 2., cy),
                    (cx + cw / 2., cy + ch),
                ]
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
                            let xx = (x + dx).clamp(0., x + w - 0.01);
                            let yy = (y + dy).clamp(0., y + h - 0.01);
                            [xx, yy, x + w - xx, y + h - yy]
                        }
                        2 => {
                            let yy = (y + dy).clamp(0., y + h - 0.01);
                            [x, yy, (w + dx).clamp(0.01, 1. - x), y + h - yy]
                        }
                        3 => {
                            let xx = (x + dx).clamp(0., x + w - 0.01);
                            [xx, y, x + w - xx, (h + dy).clamp(0.01, 1. - y)]
                        }
                        4 => [
                            x,
                            y,
                            (w + dx).clamp(0.01, 1. - x),
                            (h + dy).clamp(0.01, 1. - y),
                        ],
                        5 => {
                            let xx = (x + dx).clamp(0., x + w - 0.01);
                            [xx, y, x + w - xx, h]
                        }
                        6 => [x, y, (w + dx).clamp(0.01, 1. - x), h],
                        7 => {
                            let yy = (y + dy).clamp(0., y + h - 0.01);
                            [x, yy, w, y + h - yy]
                        }
                        8 => [x, y, w, (h + dy).clamp(0.01, 1. - y)],
                        _ => [
                            (x + dx).clamp(0., (1. - w).max(0.)),
                            (y + dy).clamp(0., (1. - h).max(0.)),
                            w,
                            h,
                        ],
                    };
                    let mut draft = draft;
                    if imp.ratio.get() > 0. && imp.crop_handle.get() != 0 {
                        let (iw, ih) = obj.image_size();
                        let ratio = imp.ratio.get() * ih / iw;
                        let width = match imp.crop_handle.get() {
                            5 | 6 => draft[2],
                            7 | 8 => draft[3] * ratio,
                            _ => draft[2].max(draft[3] * ratio),
                        }
                        .min(1.)
                        .min(ratio);
                        let height = width / ratio;
                        let left = matches!(imp.crop_handle.get(), 1 | 3 | 5);
                        let top = matches!(imp.crop_handle.get(), 1 | 2 | 7);
                        draft = [
                            if left {
                                (x + w - width).max(0.)
                            } else {
                                draft[0].min(1. - width)
                            },
                            if top {
                                (y + h - height).max(0.)
                            } else {
                                draft[1].min(1. - height)
                            },
                            width,
                            height,
                        ];
                    }
                    imp.draft.set(crate::crop_geometry::constrain(
                        imp.crop_origin.get(),
                        draft,
                        imp.draft_angle.get(),
                        obj.image_size(),
                    ));
                    imp.angle_base.set(imp.draft.get());
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
        let layout = self.border_layout(w, h);
        (((self.width() - 72).max(1) as f64 / layout.width)
            .min((self.height() - 72).max(1) as f64 / layout.height))
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
        self.imp().photo_size.set(source);
        self.fit();
    }
    pub fn update_texture(&self, t: &gdk::Texture, source: (i32, i32), photo: (i32, i32)) {
        let old = self.image_size();
        self.imp().texture.replace(Some(t.clone()));
        self.imp().source_size.set(source);
        self.imp().photo_size.set(photo);
        if !self.imp().fitting.get() && photo.0 > 0 {
            self.imp()
                .zoom
                .set(self.imp().zoom.get() * old.0 / photo.0 as f64);
        }
        self.queue_draw();
    }
    pub fn interactive_scale(&self) -> f64 {
        let scale = self
            .native()
            .and_then(|n| n.surface())
            .map(|s| s.scale())
            .unwrap_or(1.);
        self.display_scale() * scale
    }
    pub fn fit(&self) {
        self.imp().fitting.set(true);
        self.imp().pan.set((0., 0.));
        self.queue_draw();
    }
    pub fn view(&self) -> serde_json::Value {
        serde_json::json!({"fit":self.imp().fitting.get(),"zoom":self.imp().zoom.get(),"pan":self.imp().pan.get()})
    }
    pub fn restore_view(&self, value: &serde_json::Value) {
        self.imp()
            .fitting
            .set(value["fit"].as_bool().unwrap_or(true));
        self.imp().zoom.set(
            value["zoom"]
                .as_f64()
                .filter(|v| v.is_finite())
                .unwrap_or(1.)
                .clamp(0.005, 16.),
        );
        let side = |i: usize| {
            value["pan"][i]
                .as_f64()
                .filter(|v| v.is_finite() && v.abs() < 1e6)
                .unwrap_or(0.)
        };
        self.imp().pan.set((side(0), side(1)));
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
        if self.imp().cropping.get() {
            return;
        }
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
        self.imp().angle.set(state.straighten);
        self.imp().border.replace(state.border.clone());
        self.imp().crop.set(state.crop);
        self.load_border_asset(state);
        self.queue_draw();
    }
    fn load_border_asset(&self, state: &crate::model::State) {
        let (w, h) = self.image_size();
        let crop = state.crop.unwrap_or([0., 0., 1., 1.]);
        let format = crate::border::format(w * crop[2], h * crop[3]);
        let film = &crate::catalog::selected(state, false).id;
        let key = if !state.border.enabled {
            None
        } else {
            match state.border.style {
                Style::Film => Some("film".to_string()),
                Style::Film35 => format.map(|f| format!("{f}/{film}")),
                _ => None,
            }
        };
        if *self.imp().asset_key.borrow() == key {
            return;
        }
        self.imp().asset_key.replace(key);
        self.imp().asset.take();
        self.ensure_border_asset();
    }
    fn ensure_border_asset(&self) {
        let Some(key) = self.imp().asset_key.borrow().clone() else {
            return;
        };
        if self.imp().asset.borrow().is_some() {
            return;
        }
        if let Some(asset) = self.imp().asset_cache.borrow_mut().get(&key) {
            self.imp().asset.replace(Some(asset));
            self.queue_draw();
            return;
        }
        // Only one decode at a time. Its completion picks up the latest desired
        // key, so scrubbing through stocks cannot flood the worker pool.
        if self.imp().asset_loading.replace(true) {
            return;
        }
        let canvas = self.downgrade();
        let data = crate::data_root();
        let worker_key = key.clone();
        gtk::glib::MainContext::default().spawn_local(async move {
            let decoded =
                gtk::gio::spawn_blocking(move || crate::border::asset::load(&data, &worker_key))
                    .await;
            let Some(canvas) = canvas.upgrade() else {
                return;
            };
            canvas.imp().asset_loading.set(false);
            let current = canvas.imp().asset_key.borrow().as_ref() == Some(&key);
            match decoded {
                Ok(Ok(photo)) => {
                    let format = if photo.alpha {
                        gdk::MemoryFormat::R8g8b8a8
                    } else {
                        gdk::MemoryFormat::R8g8b8
                    };
                    let bytes = photo.bytes.len();
                    let texture = srgb_texture(
                        photo.width,
                        photo.height,
                        format,
                        &glib::Bytes::from_owned(photo.bytes),
                        photo.stride,
                    );
                    let asset = Asset {
                        texture,
                        source: (photo.width as f64, photo.height as f64),
                        window: photo.window,
                    };
                    canvas
                        .imp()
                        .asset_cache
                        .borrow_mut()
                        .insert(key, bytes, asset.clone());
                    if current {
                        canvas.imp().asset.replace(Some(asset));
                        canvas.queue_draw();
                    }
                }
                Ok(Err(e)) => eprintln!("Border asset could not load: {e}"),
                Err(_) => eprintln!("Border loader stopped before completing"),
            }
            if !current {
                canvas.ensure_border_asset();
            }
        });
    }
    pub fn begin_crop(&self) {
        self.imp()
            .draft
            .set(self.imp().crop.get().unwrap_or([0., 0., 1., 1.]));
        self.imp().angle_base.set(self.imp().draft.get());
        self.imp().draft_angle.set(self.imp().angle.get());
        self.imp().ratio.set(0.);
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
    pub fn source_size(&self) -> (i32, i32) {
        self.imp().source_size.get()
    }
    pub fn crop_draft(&self) -> [f64; 4] {
        self.imp().draft.get()
    }
    pub fn draft_angle(&self) -> f64 {
        self.imp().draft_angle.get()
    }
    pub fn set_angle(&self, degrees: f64) {
        self.imp().draft_angle.set(degrees.clamp(-45., 45.));
        self.imp().draft.set(crate::crop_geometry::inscribe(
            self.imp().angle_base.get(),
            degrees,
            self.image_size(),
        ));
        self.queue_draw();
    }
    pub fn reset_crop(&self) {
        self.imp().draft.set([0., 0., 1., 1.]);
        self.imp().angle_base.set([0., 0., 1., 1.]);
        self.imp().draft_angle.set(0.);
        self.imp().ratio.set(0.);
        self.queue_draw();
    }
    pub fn nudge_crop(&self, dx: f64, dy: f64) {
        let old = self.imp().draft.get();
        let [x, y, w, h] = old;
        let target = [
            (x + dx).clamp(0., (1. - w).max(0.)),
            (y + dy).clamp(0., (1. - h).max(0.)),
            w,
            h,
        ];
        self.imp().draft.set(crate::crop_geometry::constrain(
            old,
            target,
            self.draft_angle(),
            self.image_size(),
        ));
        self.imp().angle_base.set(self.imp().draft.get());
        self.queue_draw();
    }
    pub fn lock_ratio(&self, ratio: f64) {
        self.imp().ratio.set(ratio);
    }
    pub fn crop_ratio(&self, ratio: f64) {
        self.imp().ratio.set(ratio);
        if ratio <= 0. {
            return;
        }
        let ratio = if self.imp().rotation.get() % 2 == 1 {
            1. / ratio
        } else {
            ratio
        };
        self.imp().ratio.set(ratio);
        let (iw, ih) = self.image_size();
        let r = self.imp().draft.get();
        let w = 1f64.min(ih * ratio / iw);
        let h = w * iw / ratio / ih;
        let draft = [
            (r[0] + r[2] / 2. - w / 2.).clamp(0., (1. - w).max(0.)),
            (r[1] + r[3] / 2. - h / 2.).clamp(0., (1. - h).max(0.)),
            w,
            h,
        ];
        let draft = crate::crop_geometry::inscribe(draft, self.draft_angle(), self.image_size());
        self.imp().draft.set(draft);
        self.imp().angle_base.set(draft);
        self.queue_draw();
    }
}
