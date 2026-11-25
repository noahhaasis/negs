//! Color-managed exact-fit photo canvas with zoom/pan, Before/After toggle,
//! draggable Split-Screen divider, vector `negs-marks` overlay, drag-and-drop,
//! and VRAM-safe `RenderImage` caching.
//!
//! The photograph lies on the viewer sheet (`sheet.rs`) as a print
//! (`print.rs`): fitted between the caption's band above and the label's
//! below, with a light under it when its edge sinks into the sheet, trim
//! marks beside its corners that start cropping, and the label's names that
//! open the stock tray. Crop mode (`crop_view.rs`) takes the canvas over;
//! Prep's pickers take the next click; hold T to see it upside down.

use std::sync::Arc;

use gpui::{
    BoxShadow, Bounds, ContentMask, Context, Corners, CursorStyle, ExternalPaths, HitboxBehavior, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage, ScrollDelta, ScrollWheelEvent,
    Window, canvas, div, fill, point, prelude::*, px, size,
};
use negs_app::border::{BorderSettings, BorderStyle};
use negs_app::crop::{CropSession, CropSettings, Orientation, Rect};
use negs_app::picture::Picture;

use crate::NegsApp;
use crate::color_display::{
    DisplayColorProfile, PreparedImage, SourceRegion, color_manage_border_hsla,
    oriented_cropped_dimensions, prepare_exact_fit, prepare_zoomed_tile,
};
use crate::crop_view::{self, CropDrag, CropImages, CropTarget};
use crate::marks_gpui::marks_overlay_badge;
use crate::print::{self, PrintWords, Target};
use crate::sheet::{self, SheetInk, ViewerSheet};
use crate::theme_gpui::{hex, hex_alpha};
const MIN_ZOOM: f64 = 0.05;
const MAX_ZOOM: f64 = 16.0;
const SPLIT_HIT_HALF_WIDTH: f32 = 12.0;

#[derive(Clone, Debug, PartialEq, Eq)]
struct BorderKey {
    style: u8,
    instagram: bool,
    width_bits: u64,
    line_bits: u64,
    color: [u8; 3],
    line_color: [u8; 3],
}

impl BorderKey {
    fn from_settings(b: BorderSettings) -> Option<Self> {
        if !b.enabled || !matches!(b.style, BorderStyle::Solid | BorderStyle::Keyline) {
            return None;
        }
        let style = match b.style {
            BorderStyle::Solid => 1,
            BorderStyle::Keyline => 2,
            _ => return None,
        };
        Some(Self {
            style,
            instagram: b.instagram,
            width_bits: b.width.to_bits(),
            line_bits: b.line.to_bits(),
            color: b.color.0,
            line_color: b.line_color.0,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DisplayCacheKey {
    picture_id: u64,
    wide: bool,
    target_w: u32,
    target_h: u32,
    region: Option<SourceRegion>,
    turns: i32,
    flip: bool,
    crop_bits: [u64; 5],
    border: Option<BorderKey>,
    icc_label: String,
    display_key: u64,
}

impl DisplayCacheKey {
    #[allow(clippy::too_many_arguments)]
    fn new(
        picture: &Picture,
        target_w: u32,
        target_h: u32,
        region: Option<SourceRegion>,
        orientation: Orientation,
        crop: &CropSettings,
        border: Option<BorderSettings>,
        profile: &DisplayColorProfile,
    ) -> Self {
        Self {
            picture_id: picture.id,
            wide: picture.wide,
            target_w,
            target_h,
            region,
            turns: orientation.turns,
            flip: orientation.flip,
            crop_bits: [
                crop.rect.x.to_bits(),
                crop.rect.y.to_bits(),
                crop.rect.width.to_bits(),
                crop.rect.height.to_bits(),
                crop.angle.to_bits(),
            ],
            border: border.and_then(BorderKey::from_settings),
            icc_label: profile.label.clone(),
            display_key: profile.display_key,
        }
    }
}

/// Interactive viewport, zoom/pan, split-screen, and `RenderImage` cache state.
pub struct CanvasState {
    pub fit: bool,
    /// Physical screen pixels per oriented photo pixel (`1.0` = crisp 100% 1:1).
    pub zoom: f64,
    /// Offset from photo center in oriented photo pixels.
    pub pan: (f64, f64),
    /// `\` toggle: show Original instead of Developed.
    pub before_after: bool,
    /// `Shift+\` toggle: show split-screen Original | Developed with draggable bar.
    pub split_mode: bool,
    /// Fraction across the drawn photo where the split divider sits (`0.05..=0.95`).
    pub split_fraction: f32,
    /// Active drag state for panning when zoomed.
    pub dragging_pan: Option<(Point<Pixels>, (f64, f64))>,
    /// Active drag state for the split-screen divider.
    pub dragging_split: bool,
    /// Last measured canvas element bounds in window DIPs.
    pub last_canvas_bounds: Bounds<Pixels>,
    /// Last drawn photo rectangle in window DIPs (used for split hit-testing & cursor zoom).
    pub last_photo_bounds: Bounds<Pixels>,
    /// The photograph alone on screen (no border), for Prep's pickers.
    pub last_image_rect: Rect,
    /// Last window scale factor.
    pub scale_factor: f32,
    /// The photograph isn't drawn: it's flying to or from the grid.
    pub hide_photo: bool,
    /// Active monitor's color profile.
    pub display_profile: DisplayColorProfile,
    /// The sheet the print lies on (Settings › Viewer, View › Viewer Sheet).
    pub sheet: ViewerSheet,
    /// Where the label's names, the lock, the trim marks and crop's turn
    /// arrows were last drawn, and the print itself (window DIPs).
    pub targets: Vec<(Target, Rect)>,
    pub print_rect: Option<Rect>,
    /// The target under the pointer, and whether it's over the print.
    pub hot: Option<Target>,
    pub over_print: bool,
    /// A press on a target: it acts when the button comes up over it.
    pub pressed: Option<(Target, (f64, f64))>,
    /// Crop mode: the drag in progress and what's under the pointer.
    pub crop_drag: Option<CropDrag>,
    pub crop_hover: Option<CropTarget>,
    pub crop_images: CropImages,
    /// The pointer's cursor over Compare.
    pub compare_cursor: CursorStyle,
    /// T held: the photograph upside down.
    pub upside_down: bool,
    /// View › Image Only (F): no strip, status line or panel.
    pub image_only: bool,
    /// Zoomed in or out from fitting (not a zoom that happens to fit).
    pub zoomed: bool,
    /// Cached exact-fit or zoomed-tile `RenderImage` for the primary (developed/placeholder) image.
    cached_primary: Option<(DisplayCacheKey, PreparedImage)>,
    /// Cached exact-fit or zoomed-tile `RenderImage` for the Original ("Before") image.
    cached_original: Option<(DisplayCacheKey, PreparedImage)>,
    /// Old `RenderImage` handles queued for `window.drop_image(...)` to keep GPU VRAM clean.
    pending_drops: Vec<Arc<RenderImage>>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            fit: true,
            zoom: 1.0,
            pan: (0.0, 0.0),
            before_after: false,
            split_mode: false,
            split_fraction: 0.5,
            dragging_pan: None,
            dragging_split: false,
            last_canvas_bounds: Bounds::default(),
            last_photo_bounds: Bounds::default(),
            last_image_rect: Rect::new(0.0, 0.0, 0.0, 0.0),
            scale_factor: 1.0,
            hide_photo: false,
            display_profile: DisplayColorProfile::default(),
            sheet: ViewerSheet::Theme,
            targets: Vec::new(),
            print_rect: None,
            hot: None,
            over_print: false,
            pressed: None,
            crop_drag: None,
            crop_hover: None,
            crop_images: CropImages::default(),
            compare_cursor: CursorStyle::Arrow,
            upside_down: false,
            image_only: false,
            zoomed: false,
            cached_primary: None,
            cached_original: None,
            pending_drops: Vec::new(),
        }
    }
}

impl CanvasState {
    pub fn with_display_profile(display_profile: DisplayColorProfile) -> Self {
        Self {
            display_profile,
            ..Self::default()
        }
    }

    /// Queues any cached GPU images for VRAM cleanup (`window.drop_image`).
    pub fn invalidate_images(&mut self) {
        if let Some((_, old)) = self.cached_primary.take() {
            self.pending_drops.push(old.render_image);
        }
        if let Some((_, old)) = self.cached_original.take() {
            self.pending_drops.push(old.render_image);
        }
        self.crop_images.clear();
    }

    /// Releases any replaced `RenderImage`s from GPUI's sprite atlas.
    pub fn flush_drops(&mut self, window: &mut Window) {
        for img in self.pending_drops.drain(..).chain(self.crop_images.drops.drain(..)) {
            let _ = window.drop_image(img);
        }
    }

    /// The whole photograph as drawn now (fit, not a zoomed tile), for the
    /// grid's flight from it.
    pub fn whole_image(&self) -> Option<Arc<RenderImage>> {
        self.cached_primary.as_ref().filter(|(_, p)| p.region.is_none()).map(|(_, p)| p.render_image.clone())
    }

    /// Resets view to fit-to-canvas with centered pan.
    pub fn reset_fit(&mut self) {
        self.fit = true;
        self.pan = (0.0, 0.0);
    }

    /// Switches to crisp 100% (1:1 physical pixels) centered view.
    pub fn set_actual_size(&mut self) {
        self.fit = false;
        self.zoom = 1.0;
        self.pan = (0.0, 0.0);
    }

    /// Multiplies zoom by `factor`, clamping to `MIN_ZOOM..=MAX_ZOOM`.
    pub fn zoom_by(&mut self, factor: f64) {
        self.fit = false;
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
    }

    /// Computes fit zoom (`physical_pixels_per_doc_pixel`, capped at 1.0 so small images aren't blurred).
    pub fn compute_fit_zoom(&self, doc_w: f64, doc_h: f64) -> f64 {
        fit_zoom(rect_of(self.last_canvas_bounds), self.scale_factor, doc_w, doc_h)
    }

    /// Computes the live physical screen pixels per photo pixel to report via `Host::canvas_scale()`.
    pub fn live_canvas_scale(&self, photo_w: f64, photo_h: f64) -> Option<f64> {
        if photo_w <= 0.0 || photo_h <= 0.0 {
            return None;
        }
        let z = if self.fit {
            self.compute_fit_zoom(photo_w, photo_h)
        } else {
            self.zoom
        };
        (z.is_finite() && z > 0.0).then_some(z)
    }

    fn is_over_split_divider(&self, pos: Point<Pixels>) -> bool {
        if !self.split_mode {
            return false;
        }
        let pb = self.last_photo_bounds;
        if f32::from(pb.size.width) <= 0.0 || f32::from(pb.size.height) <= 0.0 {
            return false;
        }
        let div_x = f32::from(pb.origin.x) + f32::from(pb.size.width) * self.split_fraction;
        let px_x = f32::from(pos.x);
        let px_y = f32::from(pos.y);
        (px_x - div_x).abs() <= SPLIT_HIT_HALF_WIDTH
            && px_y >= f32::from(pb.origin.y) - 16.0
            && px_y <= f32::from(pb.origin.y) + f32::from(pb.size.height) + 16.0
    }

    fn update_split_from_x(&mut self, x: Pixels) {
        let pb = self.last_photo_bounds;
        let w = f32::from(pb.size.width);
        if w > 1.0 {
            let frac = (f32::from(x) - f32::from(pb.origin.x)) / w;
            self.split_fraction = frac.clamp(0.05, 0.95);
        }
    }
}

/// Ensures we have a cached `PreparedImage` (exact-fit Lanczos when downscaling, or
/// visible native-resolution color-managed tile when zoomed >= 100%).
#[allow(clippy::too_many_arguments)]
fn resolve_prepared(
    slot: &mut Option<(DisplayCacheKey, PreparedImage)>,
    pending_drops: &mut Vec<Arc<RenderImage>>,
    picture: &Picture,
    orientation: Orientation,
    crop: &CropSettings,
    border: Option<BorderSettings>,
    film: &str,
    zoom: f64,
    target_w: u32,
    target_h: u32,
    visible_region: Option<SourceRegion>,
    force_exact_fit: bool,
    profile: &DisplayColorProfile,
) -> Option<PreparedImage> {
    let cm = profile.transform.as_deref();
    let (oriented_w, oriented_h) =
        oriented_cropped_dimensions(picture.width, picture.height, orientation, crop);

    // Downscale / exact-fit path when zoom < 0.999 and image is larger than target,
    // or when enlarging a tried stock's stand-in tile (`force_exact_fit`).
    let use_zoomed_tile =
        !force_exact_fit && (zoom >= 0.999 || (oriented_w <= target_w && oriented_h <= target_h));

    if use_zoomed_tile {
        let region = visible_region.unwrap_or(SourceRegion {
            x: 0,
            y: 0,
            width: oriented_w,
            height: oriented_h,
        });
        // Reuse existing cached tile if it already fully covers the requested visible region
        // for the exact same picture/orientation/crop/ICC!
        if let Some((key, cached)) = slot.as_ref() {
            let same_image = key.picture_id == picture.id
                && key.wide == picture.wide
                && key.turns == orientation.turns
                && key.flip == orientation.flip
                && key.icc_label == profile.label
                && key.display_key == profile.display_key
                && key.crop_bits
                    == [
                        crop.rect.x.to_bits(),
                        crop.rect.y.to_bits(),
                        crop.rect.width.to_bits(),
                        crop.rect.height.to_bits(),
                        crop.angle.to_bits(),
                    ];
            if same_image {
                if let Some(cr) = cached.region {
                    if region.x >= cr.x
                        && region.y >= cr.y
                        && region.x + region.width <= cr.x + cr.width
                        && region.y + region.height <= cr.y + cr.height
                    {
                        return Some(cached.clone());
                    }
                }
            }
        }

        // Expand the tile with a 25% pan margin on each side so small pans hit the cache!
        let margin_x = (region.width / 4).max(64);
        let margin_y = (region.height / 4).max(64);
        let x0 = region.x.saturating_sub(margin_x);
        let y0 = region.y.saturating_sub(margin_y);
        let x1 = (region.x + region.width + margin_x).min(oriented_w);
        let y1 = (region.y + region.height + margin_y).min(oriented_h);
        let padded = SourceRegion {
            x: x0,
            y: y0,
            width: x1.saturating_sub(x0).max(1),
            height: y1.saturating_sub(y0).max(1),
        };

        let key = DisplayCacheKey::new(
            picture,
            padded.width,
            padded.height,
            Some(padded),
            orientation,
            crop,
            None,
            profile,
        );
        if let Some((cached_key, cached)) = slot.as_ref() {
            if *cached_key == key {
                return Some(cached.clone());
            }
        }
        let prepared = prepare_zoomed_tile(picture, orientation, crop, padded, cm)?;
        if let Some((_, old)) = slot.replace((key, prepared.clone())) {
            pending_drops.push(old.render_image);
        }
        Some(prepared)
    } else {
        let key = DisplayCacheKey::new(
            picture,
            target_w,
            target_h,
            None,
            orientation,
            crop,
            border,
            profile,
        );
        if let Some((cached_key, cached)) = slot.as_ref() {
            if *cached_key == key {
                return Some(cached.clone());
            }
        }
        let prepared =
            prepare_exact_fit(picture, orientation, crop, border, film, target_w, target_h, cm)?;
        if let Some((_, old)) = slot.replace((key, prepared.clone())) {
            pending_drops.push(old.render_image);
        }
        Some(prepared)
    }
}

/// Computes exact GPUI `Bounds<Pixels>` that map to whole physical pixels `(ox_phys, oy_phys, w_phys, h_phys)`
/// so GPUI's `paint_image` (`floor` origin, `ceil` size) renders 1:1 with zero fractional-DPI blur.
fn physical_to_dip_bounds(
    ox_phys: f32,
    oy_phys: f32,
    w_phys: f32,
    h_phys: f32,
    scale_factor: f32,
) -> Bounds<Pixels> {
    let sf = scale_factor.max(1.0);
    // Tiny -1e-4 guard prevents `ceil()` from adding +1px due to float rounding at fractional DPI (e.g. 1.25/1.5).
    let eps = 1e-4;
    Bounds {
        origin: point(px((ox_phys.round() + eps) / sf), px((oy_phys.round() + eps) / sf)),
        size: size(
            px(((w_phys.round() - eps).max(1.0)) / sf),
            px(((h_phys.round() - eps).max(1.0)) / sf),
        ),
    }
}

/// `b` as a `crop::Rect` (window DIPs).
pub fn rect_of(b: Bounds<Pixels>) -> Rect {
    Rect::new(f32::from(b.origin.x) as f64, f32::from(b.origin.y) as f64, f32::from(b.size.width) as f64, f32::from(b.size.height) as f64)
}

/// Physical pixels per document pixel that fit the print in the band
/// between the caption and the label, never past 1:1.
pub fn fit_zoom(canvas: Rect, sf: f32, doc_w: f64, doc_h: f64) -> f64 {
    if doc_w <= 0.0 || doc_h <= 0.0 {
        return 1.0;
    }
    let sf = sf.max(1.0) as f64;
    let (aw, ah) = print::fit_room(canvas);
    ((aw.max(64.0) * sf) / doc_w).min((ah.max(64.0) * sf) / doc_h).min(1.0).max(MIN_ZOOM)
}

/// Zoomed in or out from fitting, not a zoom that happens to match the fit.
pub fn is_zoomed(fit: bool, zoom: f64, fit_zoom: f64) -> bool {
    !fit && (zoom / fit_zoom.max(1e-9) - 1.0).abs() > 0.005
}

/// Where a window point falls on the photograph (`photo`, its border left
/// out) as shown, as fractions from its top left; outside, beyond 0…1.
/// Upside down, the fractions turn with it.
pub fn shown_fraction(photo: Rect, p: (f64, f64), upside_down: bool) -> Option<[f64; 2]> {
    if photo.width <= 0.0 || photo.height <= 0.0 {
        return None;
    }
    let f = [(p.0 - photo.x) / photo.width, (p.1 - photo.y) / photo.height];
    Some(if upside_down { [1.0 - f[0], 1.0 - f[1]] } else { f })
}

/// The caption's and the label's words for the open photograph.
pub fn print_words(app: &NegsApp) -> PrintWords {
    let model = &app.model;
    let Some(source) = model.source.clone() else { return PrintWords::default() };
    let decimal = negs_app::host::decimal_separator();
    PrintWords {
        facts: app.facts.get(&source).map(|f| f.items(decimal)).unwrap_or_default(),
        file: negs_app::model::file_name(&source),
        film: model.film_name(),
        paper: model.paper_name(),
        scanning: negs_app::json::boolean(model.value("io.scan_film")),
        original: model.holding_original && model.original.is_some(),
        keep_paper: model.prefs.keep_paper,
        tray: model.tray_shown().filter(|_| !model.tray().is_some_and(|t| t.read_as)),
        negative: model.is_negative(),
        read_as_open: model.tray_shown() == Some(false) && model.tray().is_some_and(|t| t.read_as),
        prompt: model.negative_pick().map(|p| p.prompt()),
    }
}

/// What the paint closure draws, gathered before layout.
#[derive(Clone)]
struct Scene {
    words: PrintWords,
    ink: SheetInk,
    serif: gpui::SharedString,
    plain: bool,
    theme_paper: u32,
    cropping: Option<CropSession>,
    /// The developed picture crop mode shows.
    developed: Option<Picture>,
    image_only: bool,
    upside_down: bool,
    croppable: bool,
    failed: bool,
    high_contrast: bool,
    hot: Option<Target>,
    over_print: bool,
    shown_crop: CropSettings,
    cursor: CursorStyle,
}

/// Renders either the darkroom empty state or the live color-managed photo canvas.
pub fn render_canvas(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let theme = app.theme.clone();
    app.canvas.flush_drops(window);
    app.canvas.sheet = sheet::from_prefs(&app.model.prefs);
    let ink = sheet::ink(app.canvas.sheet, &theme.tokens);

    if app.model.source.is_none() {
        return render_empty_state(app, &ink, cx).into_any_element();
    }

    if app.model.comparison().is_some() {
        return render_compare(app, ink, cx).into_any_element();
    }

    // Current rating & flag for the vector `negs-marks` overlay.
    let (photo_name, rating, flag) = app.current_marks();
    let marks_badge = marks_overlay_badge(photo_name, rating, flag, &theme);

    let entity = cx.entity().clone();
    let accent = theme.accent;
    let badge_bg = theme.badge_bg;
    let on_photo = theme.on_photo;
    let canvas_bg = hex(ink.sheet);
    let split_mode = app.canvas.split_mode;
    let before_after = app.canvas.before_after;
    let has_original = app.model.original.is_some();
    let loading_original = (split_mode || before_after) && !has_original;
    let develop_error = app.model.develop_error.clone();
    let hide_photo = app.canvas.hide_photo;
    let cropping = app.model.crop_session.clone();
    let badge = app.model.badge().filter(|_| cropping.is_none());
    let picking = app.model.negative_pick().is_some();
    let cursor = if let Some(_) = &cropping {
        crop_view::cursor_for(app.canvas.crop_hover)
    } else if picking {
        CursorStyle::Crosshair
    } else if app.canvas.hot.is_some() {
        CursorStyle::PointingHand
    } else if !app.canvas.fit {
        if app.canvas.dragging_pan.is_some() { CursorStyle::ClosedHand } else { CursorStyle::OpenHand }
    } else {
        CursorStyle::Arrow
    };
    let scene = Scene {
        words: print_words(app),
        ink,
        serif: theme.serif_family.clone(),
        plain: theme.tokens.plain,
        theme_paper: theme.tokens.hex.paper,
        cropping,
        developed: app.model.image.clone(),
        image_only: app.canvas.image_only,
        upside_down: app.canvas.upside_down,
        croppable: app.model.image.is_some() && !app.model.render_failed,
        failed: app.model.render_failed,
        high_contrast: false,
        hot: app.canvas.hot,
        over_print: app.canvas.over_print,
        shown_crop: app.model.shown_crop(),
        cursor,
    };
    let whole = app.model.negative_pick() == Some(negs_app::negative::Pick::Base);

    let photo_canvas = canvas(
        {
            let entity = entity.clone();
            move |bounds, window, cx| {
                let sf = window.scale_factor();
                entity.update(cx, |this, _cx| {
                    this.canvas.last_canvas_bounds = bounds;
                    this.canvas.scale_factor = sf;
                    let photo_size = this.model.photo_size();
                    let (pw, ph) = if this.model.rotation().rem_euclid(2) == 1 {
                        (photo_size.height, photo_size.width)
                    } else {
                        (photo_size.width, photo_size.height)
                    };
                    if pw > 0.0 && ph > 0.0 {
                        this.host
                            .set_canvas_scale(this.canvas.live_canvas_scale(pw, ph));
                    }
                });
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            }
        },
        {
            let entity = entity.clone();
            move |bounds: Bounds<Pixels>, hitbox: gpui::Hitbox, window, cx| {
                window.set_cursor_style(scene.cursor, &hitbox);
                let sf = window.scale_factor().max(1.0);
                let canvas_rect = rect_of(bounds);
                // Crop mode takes the canvas over.
                if let (Some(session), Some(developed)) = (&scene.cropping, &scene.developed) {
                    let profile = entity.read(cx).canvas.display_profile.clone();
                    let mut images = std::mem::take(&mut entity.update(cx, |this, _| std::mem::take(&mut this.canvas.crop_images)));
                    let targets = {
                        let mut p = print::Painter { window, cx, serif: scene.serif.clone(), plain: scene.plain };
                        crop_view::paint(&mut images, &mut p, canvas_rect, developed, session, &scene.words, &scene.ink, scene.theme_paper, &profile, scene.hot)
                    };
                    entity.update(cx, |this, _| {
                        this.canvas.crop_images = images;
                        this.canvas.targets = targets;
                        this.canvas.print_rect = None;
                    });
                    return;
                }
                let (
                    developed_pic,
                    original_pic,
                    is_developed_frame,
                    is_trial_stand_in,
                    trial_photo_dims,
                    orientation,
                    crop,
                    border,
                    film,
                    fit,
                    zoom,
                    pan,
                    before_after,
                    split_mode,
                    split_fraction,
                    profile,
                ) = entity.update(cx, |this, cx| {
                    this.refresh_display_profile(window, cx);
                    this.canvas.flush_drops(window);
                    this.stamp_startup("first_window_composited");

                    let mut orientation = this.model.state.orientation();
                    if scene.upside_down {
                        orientation = Orientation::new(orientation.turns + 2, orientation.flip);
                    }
                    let crop = scene.shown_crop;
                    let border = if whole { None } else { Some(this.model.state.border) };
                    let film = this.model.state.film.clone();

                    let (primary, is_dev, is_trial, photo_dims) =
                        if let Some(trial) = this.model.shown_trial() {
                            let ps = this.model.photo_size();
                            let dims = if ps.width > 0.0 && ps.height > 0.0 {
                                let (ow, oh) = oriented_cropped_dimensions(
                                    ps.width.round() as u32,
                                    ps.height.round() as u32,
                                    orientation,
                                    &crop,
                                );
                                (ow as f64, oh as f64)
                            } else {
                                (0.0, 0.0)
                            };
                            (Some(trial.picture.clone()), false, true, dims)
                        } else {
                            let is_dev = this.model.image.is_some();
                            let primary = this
                                .model
                                .image
                                .clone()
                                .or_else(|| this.model.placeholder.clone())
                                .or_else(|| this.model.stand_in.clone());
                            (primary, is_dev, false, (0.0, 0.0))
                        };
                    let original = this.model.original.clone();
                    (
                        primary,
                        original,
                        is_dev,
                        is_trial,
                        photo_dims,
                        orientation,
                        crop,
                        border,
                        film,
                        this.canvas.fit,
                        this.canvas.zoom,
                        this.canvas.pan,
                        this.canvas.before_after,
                        this.canvas.split_mode,
                        this.canvas.split_fraction,
                        this.canvas.display_profile.clone(),
                    )
                });

                let Some(primary_pic) = developed_pic else {
                    return;
                };

                // Stock trial stand-ins are already cropped by the engine (only unturned),
                // and scale to the photograph's oriented cropped size until full-res finishes.
                let primary_crop = if is_trial_stand_in {
                    CropSettings::default()
                } else {
                    crop
                };
                let (oriented_w, oriented_h) = if is_trial_stand_in {
                    let (tw, th) = oriented_cropped_dimensions(
                        primary_pic.width,
                        primary_pic.height,
                        orientation,
                        &primary_crop,
                    );
                    let (sw, sh) = negs_app::tray_rules::stand_in_size((tw, th), trial_photo_dims);
                    ((sw.round() as u32).max(1), (sh.round() as u32).max(1))
                } else {
                    oriented_cropped_dimensions(
                        primary_pic.width,
                        primary_pic.height,
                        orientation,
                        &crop,
                    )
                };
                let active_border = border.and_then(|b| {
                    b.shown(oriented_w, oriented_h)
                        .filter(|s| matches!(s.style, BorderStyle::Solid | BorderStyle::Keyline))
                });
                // The print's edge: its border's colour, or the photograph's own.
                let light_edge = sheet::light_table::edge(border.and_then(|b| b.shown(oriented_w, oriented_h)).as_ref());
                let (doc_w, doc_h) = match active_border {
                    Some(b) => b.document(oriented_w, oriented_h, &film),
                    None => (oriented_w, oriented_h),
                };

                // Effective zoom (physical pixels per document pixel): fitted
                // between the caption's and the label's bands.
                let fitted_zoom = fit_zoom(canvas_rect, sf, doc_w as f64, doc_h as f64);
                let effective_zoom = if fit { fitted_zoom } else { zoom };
                let zoomed = is_zoomed(fit, effective_zoom, fitted_zoom);

                let target_w = ((doc_w as f64 * effective_zoom).round() as u32).max(1);
                let target_h = ((doc_h as f64 * effective_zoom).round() as u32).max(1);

                // The print centres in its band.
                let band = print::band(canvas_rect);
                let canvas_cx_phys = band.mid_x() as f32 * sf;
                let canvas_cy_phys = band.mid_y() as f32 * sf;
                let pan_x_phys = (pan.0 * effective_zoom) as f32;
                let pan_y_phys = (pan.1 * effective_zoom) as f32;

                let doc_ox_phys = canvas_cx_phys - target_w as f32 * 0.5 + pan_x_phys;
                let doc_oy_phys = canvas_cy_phys - target_h as f32 * 0.5 + pan_y_phys;
                let photo_bounds = physical_to_dip_bounds(
                    doc_ox_phys,
                    doc_oy_phys,
                    target_w as f32,
                    target_h as f32,
                    sf,
                );
                // The photograph alone (no border), for Prep's pickers.
                let image_w = oriented_w as f64 * effective_zoom / sf as f64;
                let image_h = oriented_h as f64 * effective_zoom / sf as f64;
                let doc = rect_of(photo_bounds);
                let image_rect = Rect::new(doc.mid_x() - image_w / 2.0, doc.mid_y() - image_h / 2.0, image_w, image_h);

                // Compute visible source region for zoomed-in (>= 100%) native tile mode.
                let use_zoomed_tile =
                    effective_zoom >= 0.999 || (oriented_w <= target_w && oriented_h <= target_h);
                let visible_region = if use_zoomed_tile {
                    let vp_x0 = band.x as f32 * sf;
                    let vp_y0 = band.y as f32 * sf;
                    let vp_x1 = vp_x0 + band.width as f32 * sf;
                    let vp_y1 = vp_y0 + band.height as f32 * sf;

                    let photo_w_phys = (oriented_w as f64 * effective_zoom) as f32;
                    let photo_h_phys = (oriented_h as f64 * effective_zoom) as f32;
                    let photo_ox_phys = canvas_cx_phys - photo_w_phys * 0.5 + pan_x_phys;
                    let photo_oy_phys = canvas_cy_phys - photo_h_phys * 0.5 + pan_y_phys;

                    let sx0 = (((vp_x0 - photo_ox_phys) as f64 / effective_zoom).floor() as i64)
                        .clamp(0, oriented_w.saturating_sub(1) as i64)
                        as u32;
                    let sy0 = (((vp_y0 - photo_oy_phys) as f64 / effective_zoom).floor() as i64)
                        .clamp(0, oriented_h.saturating_sub(1) as i64)
                        as u32;
                    let sx1 = (((vp_x1 - photo_ox_phys) as f64 / effective_zoom).ceil() as i64)
                        .clamp(sx0 as i64 + 1, oriented_w as i64)
                        as u32;
                    let sy1 = (((vp_y1 - photo_oy_phys) as f64 / effective_zoom).ceil() as i64)
                        .clamp(sy0 as i64 + 1, oriented_h as i64)
                        as u32;
                    Some(SourceRegion {
                        x: sx0,
                        y: sy0,
                        width: (sx1 - sx0).max(1),
                        height: (sy1 - sy0).max(1),
                    })
                } else {
                    None
                };

                let (prepared_primary, prepared_original) = entity.update(cx, |this, _cx| {
                    this.canvas.zoom = effective_zoom;
                    this.canvas.zoomed = zoomed;
                    this.canvas.last_photo_bounds = photo_bounds;
                    this.canvas.last_image_rect = image_rect;

                    let need_primary = !before_after || split_mode || original_pic.is_none();
                    let need_original = (before_after || split_mode) && original_pic.is_some();

                    let prim = if need_primary {
                        resolve_prepared(
                            &mut this.canvas.cached_primary,
                            &mut this.canvas.pending_drops,
                            &primary_pic,
                            orientation,
                            &primary_crop,
                            active_border,
                            &film,
                            effective_zoom,
                            target_w,
                            target_h,
                            visible_region,
                            is_trial_stand_in,
                            &profile,
                        )
                    } else {
                        None
                    };

                    let orig = if need_original {
                        original_pic.as_ref().and_then(|orig_pic| {
                            resolve_prepared(
                                &mut this.canvas.cached_original,
                                &mut this.canvas.pending_drops,
                                orig_pic,
                                orientation,
                                &crop,
                                active_border,
                                &film,
                                effective_zoom,
                                target_w,
                                target_h,
                                visible_region,
                                false,
                                &profile,
                            )
                        })
                    } else {
                        None
                    };
                    (prim, orig)
                });

                // The print on screen, cut to its band.
                let print_rect = {
                    let r = doc.intersection(&band);
                    (r.width > 0.0 && r.height > 0.0).then_some(r)
                };
                // A light under the print when its edge sinks into the sheet;
                // fitted only: zoomed in, the print has no edges on screen.
                if let Some(print) = print_rect.filter(|_| !zoomed && !scene.failed && !scene.high_contrast && !hide_photo)
                    && let Some(light) = sheet::light_table::light(scene.ink.sheet, light_edge)
                {
                    use sheet::light_table::{BLUR, CORNER_RADIUS, SPREAD};
                    window.paint_shadows(
                        print::bounds(print),
                        Corners::all(px(CORNER_RADIUS as f32)),
                        &[BoxShadow { color: hex(light), offset: point(px(0.0), px(0.0)), blur_radius: px(BLUR as f32), spread_radius: px(SPREAD as f32) }],
                    );
                }

                // Clip the photograph to its band.
                window.with_content_mask(Some(ContentMask { bounds: print::bounds(band) }), |window| {
                    // When zoomed in with a Solid/Keyline border, paint the color-managed mat quad behind the native photo tile.
                    if use_zoomed_tile {
                        if let Some(b) = active_border {
                            let mat_color =
                                color_manage_border_hsla(b.color, profile.transform.as_deref());
                            window.paint_quad(fill(photo_bounds, mat_color));
                        }
                    }

                    let paint_prepared = |window: &mut Window, prep: &PreparedImage| {
                        if hide_photo {
                            return;
                        }
                        let draw_bounds = if let Some(reg) = prep.region {
                            let photo_w_phys = (oriented_w as f64 * effective_zoom) as f32;
                            let photo_h_phys = (oriented_h as f64 * effective_zoom) as f32;
                            let photo_ox_phys = canvas_cx_phys - photo_w_phys * 0.5 + pan_x_phys;
                            let photo_oy_phys = canvas_cy_phys - photo_h_phys * 0.5 + pan_y_phys;
                            let tile_ox = photo_ox_phys + (reg.x as f64 * effective_zoom) as f32;
                            let tile_oy = photo_oy_phys + (reg.y as f64 * effective_zoom) as f32;
                            let tile_w = (reg.width as f64 * effective_zoom) as f32;
                            let tile_h = (reg.height as f64 * effective_zoom) as f32;
                            physical_to_dip_bounds(tile_ox, tile_oy, tile_w, tile_h, sf)
                        } else {
                            photo_bounds
                        };
                        let _ = window.paint_image(
                            draw_bounds,
                            Corners::default(),
                            prep.render_image.clone(),
                            0,
                            false,
                        );
                    };

                    if split_mode {
                        if let Some(after) = &prepared_primary {
                            paint_prepared(window, after);
                        }
                        let div_x =
                            photo_bounds.origin.x + photo_bounds.size.width * split_fraction;
                        if let Some(before) = &prepared_original {
                            let left_mask = Bounds {
                                origin: bounds.origin,
                                size: size(
                                    (div_x - bounds.origin.x).max(px(0.0)),
                                    bounds.size.height,
                                ),
                            };
                            window.with_content_mask(
                                Some(ContentMask { bounds: left_mask }),
                                |window| {
                                    paint_prepared(window, before);
                                },
                            );
                        }
                        // Crisp vertical divider line + center grip handle.
                        let line_bounds = Bounds {
                            origin: point(div_x - px(1.0), photo_bounds.origin.y),
                            size: size(px(2.0), photo_bounds.size.height),
                        };
                        window.paint_quad(fill(line_bounds, accent));

                        let grip_h = px(34.0);
                        let grip_w = px(12.0);
                        let grip_y =
                            photo_bounds.origin.y + (photo_bounds.size.height - grip_h) * 0.5;
                        let grip_bounds = Bounds {
                            origin: point(div_x - grip_w * 0.5, grip_y),
                            size: size(grip_w, grip_h),
                        };
                        window.paint_quad(gpui::quad(
                            grip_bounds,
                            Corners::all(px(6.0)),
                            badge_bg,
                            gpui::Edges::all(px(1.5)),
                            accent,
                            gpui::BorderStyle::Solid,
                        ));
                    } else if before_after {
                        if let Some(orig) = &prepared_original {
                            paint_prepared(window, orig);
                        } else if let Some(prim) = &prepared_primary {
                            paint_prepared(window, prim);
                        }
                    } else if let Some(prim) = &prepared_primary {
                        paint_prepared(window, prim);
                    }
                });

                // The caption over the print, the label under it, the trim marks.
                let mut targets = vec![];
                if let Some(print) = print_rect.filter(|_| !hide_photo) {
                    let portrait = !zoomed && print.height > print.width;
                    let mut p = print::Painter { window, cx, serif: scene.serif.clone(), plain: scene.plain };
                    print::paint_caption(&mut p, canvas_rect, print, portrait, &scene.words, &scene.ink);
                    targets.extend(print::paint_label(&mut p, canvas_rect, print, &scene.words, &scene.ink, scene.hot));
                    if !zoomed && !scene.image_only && scene.croppable && !scene.failed && scene.words.prompt.is_none() {
                        targets.extend(print::paint_trim_marks(window, print, &scene.ink, scene.hot, scene.over_print));
                    }
                }
                entity.update(cx, |this, _| {
                    this.canvas.targets = targets;
                    this.canvas.print_rect = print_rect;
                });

                // Record `first_image_on_screen` once the first developed RenderImage paints!
                if is_developed_frame && prepared_primary.is_some() {
                    entity.update(cx, |this, _cx| {
                        this.stamp_startup("first_image_on_screen");
                    });
                }
            }
        },
    )
    .size_full();

    div()
        .id("photo-canvas-root")
        .relative()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(canvas_bg)
        .overflow_hidden()
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_2().border_color(accent))
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            this.open_dropped_paths(paths.paths(), cx);
        }))
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
            if this.model.cropping() {
                return;
            }
            let dy = match event.delta {
                ScrollDelta::Pixels(p) => f32::from(p.y) as f64,
                ScrollDelta::Lines(l) => l.y as f64 * 28.0,
            };
            if dy.abs() > 0.01 && this.model.source.is_some() {
                let factor = (1.0 + dy * 0.0025).clamp(0.75, 1.35);
                this.canvas.zoom_by(factor);
                this.model.fit_image = false;
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                if this.theme_menu_open {
                    this.theme_menu_open = false;
                }
                mouse_down(this, event, window, cx);
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
            hover(this, event, cx);
            if this.canvas.dragging_split {
                this.canvas.update_split_from_x(event.position.x);
                cx.notify();
            } else if let Some((start_pos, start_pan)) = this.canvas.dragging_pan {
                let sf = this.canvas.scale_factor.max(1.0) as f64;
                let z = this.canvas.zoom.max(MIN_ZOOM);
                let dx_photo =
                    (f32::from(event.position.x) - f32::from(start_pos.x)) as f64 * sf / z;
                let dy_photo =
                    (f32::from(event.position.y) - f32::from(start_pos.y)) as f64 * sf / z;
                this.canvas.pan = (start_pan.0 + dx_photo, start_pan.1 + dy_photo);
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                if let Some((target, _)) = this.canvas.pressed.take() {
                    let p = negs_app::crop::Point { x: f32::from(event.position.x) as f64, y: f32::from(event.position.y) as f64 };
                    if print::hit(&this.canvas.targets, p) == Some(target) {
                        act(this, target, cx);
                    }
                    cx.notify();
                }
                if this.canvas.dragging_split || this.canvas.dragging_pan.is_some() {
                    this.canvas.dragging_split = false;
                    this.canvas.dragging_pan = None;
                    cx.notify();
                }
            }),
        )
        .on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
            if !*hovered && (this.canvas.hot.is_some() || this.canvas.over_print) {
                this.canvas.hot = None;
                this.canvas.over_print = false;
                cx.notify();
            }
        }))
        .child(photo_canvas)
        // Developing, Updating or Loading original, at the top centre.
        .when_some(badge, |el, title| {
            el.child(
                div().absolute().top(px(print::CAPTION_BAND as f32 + 16.0)).left_0().right_0().flex().justify_center().child(
                    div()
                        .px(px(14.0))
                        .py(px(5.0))
                        .rounded(px(5.0))
                        .bg(hex_alpha(theme.tokens.hex.bg, 0.78))
                        .border_1()
                        .border_color(theme.paper.opacity(0.12))
                        .font_family(theme.serif_family.clone())
                        .italic()
                        .text_size(px(17.0))
                        .text_color(theme.paper)
                        .child(title),
                ),
            )
        })
        // Before / After or Split overlay badge
        .when(split_mode || before_after, |el| {
            let text = if loading_original {
                "Loading original…"
            } else if split_mode {
                "Original  |  Developed"
            } else {
                "Original"
            };
            el.child(
                div()
                    .absolute()
                    .top(px(14.0))
                    .left(px(16.0))
                    .px_3()
                    .py(px(5.0))
                    .rounded(px(5.0))
                    .bg(badge_bg)
                    .text_xs()
                    .text_color(on_photo)
                    .child(text),
            )
        })
        // Vector `negs-marks` overlay badge in top-right corner when rated/flagged
        .when_some(marks_badge, |el, badge| {
            el.child(
                div()
                    .absolute()
                    .top(px(14.0))
                    .right(px(16.0))
                    .child(badge),
            )
        })
        // Develop error card on canvas if rendering failed
        .when_some(develop_error, |el, err| {
            el.child(
                div()
                    .absolute()
                    .bottom(px(18.0))
                    .left(px(18.0))
                    .max_w(px(520.0))
                    .p_3()
                    .rounded(px(6.0))
                    .bg(badge_bg)
                    .border_1()
                    .border_color(theme.warn)
                    .text_xs()
                    .text_color(theme.warn)
                    .child(format!("Could not develop photograph: {err}")),
            )
        })
        .into_any_element()
}

fn xy(p: Point<Pixels>) -> (f64, f64) {
    (f32::from(p.x) as f64, f32::from(p.y) as f64)
}

/// A press on the canvas: Prep's pickers, crop's handles and dial, the
/// label's names and marks, else the split line, a pan or a double-click zoom.
fn mouse_down(this: &mut NegsApp, event: &MouseDownEvent, _window: &mut Window, cx: &mut Context<NegsApp>) {
    let (x, y) = xy(event.position);
    let p = negs_app::crop::Point { x, y };
    // Prep's pickers: the click reads the negative there.
    if this.model.negative_pick().is_some() {
        match shown_fraction(this.canvas.last_image_rect, (x, y), this.canvas.upside_down) {
            Some(shown) => this.model.picked_shown_point(shown),
            None => {
                this.model.cancel_negative_pick();
            }
        }
        this.after_model_update(cx);
        cx.notify();
        return;
    }
    if this.model.cropping() {
        // The dial's turn arrows act when the button comes up over them.
        if let Some(target) = print::hit(&this.canvas.targets, p) {
            this.canvas.pressed = Some((target, (x, y)));
            return;
        }
        let canvas = rect_of(this.canvas.last_canvas_bounds);
        if let Some(session) = this.model.crop_session.as_mut()
            && let Some(drag) = crop_view::press(canvas, session, p, event.click_count >= 2)
        {
            this.canvas.crop_drag = drag;
            this.model.crop_changed();
            this.after_model_update(cx);
            cx.notify();
        }
        return;
    }
    if let Some(target) = print::hit(&this.canvas.targets, p) {
        this.canvas.pressed = Some((target, (x, y)));
        cx.notify();
        return;
    }
    if event.click_count >= 2 && this.model.source.is_some() {
        this.toggle_zoom_fit(cx);
        return;
    }
    if this.canvas.is_over_split_divider(event.position) {
        this.canvas.dragging_split = true;
        this.canvas.update_split_from_x(event.position.x);
        cx.notify();
    } else if !this.canvas.fit {
        this.canvas.dragging_pan = Some((event.position, this.canvas.pan));
    }
}

/// The pointer over the canvas: the target it's over lights, the trim marks
/// lift while it's over the print; crop's cursor follows its handles. A
/// drag inward from a trim mark starts cropping.
fn hover(this: &mut NegsApp, event: &MouseMoveEvent, cx: &mut Context<NegsApp>) {
    let (x, y) = xy(event.position);
    let p = negs_app::crop::Point { x, y };
    if let Some((Target::Mark(handle), start)) = this.canvas.pressed
        && print::inward(handle, start, x, y) > print::MARK_DRAG
    {
        this.canvas.pressed = None;
        this.model.begin_crop();
        this.after_model_update(cx);
        cx.notify();
        return;
    }
    let hot = print::hit(&this.canvas.targets, p);
    let over = this.canvas.print_rect.is_some_and(|r| r.contains(p));
    let crop_hover = match (&this.model.crop_session, this.canvas.crop_drag) {
        (Some(session), None) => crop_view::target_at(rect_of(this.canvas.last_canvas_bounds), session, p),
        _ => this.canvas.crop_hover,
    };
    if (hot, over, crop_hover) != (this.canvas.hot, this.canvas.over_print, this.canvas.crop_hover) {
        this.canvas.hot = hot;
        this.canvas.over_print = over;
        this.canvas.crop_hover = crop_hover;
        cx.notify();
    }
}

/// A crop drag anywhere in the window (from the root's mouse move).
pub fn crop_drag_move(this: &mut NegsApp, position: Point<Pixels>, cx: &mut Context<NegsApp>) -> bool {
    let Some(drag) = this.canvas.crop_drag else { return false };
    let canvas = rect_of(this.canvas.last_canvas_bounds);
    let (x, y) = xy(position);
    if let Some(session) = this.model.crop_session.as_mut() {
        crop_view::drag(canvas, session, drag, negs_app::crop::Point { x, y });
    }
    this.model.crop_changed();
    cx.notify();
    true
}

/// The crop drag's button came up.
pub fn crop_drag_end(this: &mut NegsApp, cx: &mut Context<NegsApp>) -> bool {
    let Some(drag) = this.canvas.crop_drag.take() else { return false };
    if let Some(session) = this.model.crop_session.as_mut() {
        crop_view::release(session, drag);
    }
    this.model.crop_changed();
    cx.notify();
    true
}

/// A click on the print's words, a trim mark or a turn arrow.
pub fn act(this: &mut NegsApp, target: Target, cx: &mut Context<NegsApp>) {
    let model = &mut this.model;
    match target {
        Target::Film | Target::Paper => {
            let paper = target == Target::Paper;
            // Its tray, keeping the other's stock; a second click keeps
            // this one and closes it. With the tiles off: the stock menu.
            // (With the tiles off, Windows offers a menu of stocks: the
            // GPUI app has none yet; the inspector's pickers choose.)
            if model.can_open_tray(paper) {
                model.toggle_tray(paper);
            }
        }
        Target::ReadAs => {
            if model.can_open_tray(false) {
                model.toggle_read_as_tray();
            }
        }
        Target::Lock => {
            let keep = !model.prefs.keep_paper;
            model.set_keep_paper(keep);
            model.dirty |= negs_app::model::dirty::CANVAS;
        }
        Target::Mark(_) => model.begin_crop(),
        Target::TurnLeft => model.turn(false),
        Target::TurnRight => model.turn(true),
    }
    this.after_model_update(cx);
}

/// Compare in the canvas's place (`compare_view.rs`).
fn render_compare(app: &mut NegsApp, ink: SheetInk, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let entity = cx.entity().clone();
    let tokens = app.theme.tokens.clone();
    let serif = app.theme.serif_family.clone();
    let plain = tokens.plain;
    let cursor = app.canvas.compare_cursor;
    let sheet = hex(ink.sheet);
    let view = canvas(
        {
            let entity = entity.clone();
            move |bounds, window, cx| {
                let sf = window.scale_factor();
                entity.update(cx, |this, _| {
                    this.canvas.last_canvas_bounds = bounds;
                    this.canvas.scale_factor = sf;
                });
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            }
        },
        move |bounds: Bounds<Pixels>, hitbox: gpui::Hitbox, window, cx| {
            window.set_cursor_style(cursor, &hitbox);
            entity.update(cx, |this, cx| {
                this.refresh_display_profile(window, cx);
                this.canvas.flush_drops(window);
                for image in this.compare.drops.drain(..) {
                    let _ = window.drop_image(image);
                }
            });
            let mut p = print::Painter { window, cx, serif: serif.clone(), plain };
            crate::compare_view::paint(&entity, &mut p, rect_of(bounds), &ink, &tokens);
        },
    )
    .size_full();
    div()
        .id("compare-canvas")
        .relative()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(sheet)
        .overflow_hidden()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                let (x, y) = xy(event.position);
                let p = negs_app::crop::Point { x, y };
                if let Some(target) = crate::compare_view::mouse_down(this, p, event.click_count) {
                    this.canvas.pressed = Some((target, (x, y)));
                }
                this.after_model_update(cx);
                cx.notify();
            }),
        )
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, event: &MouseUpEvent, _window, cx| {
                if let Some((target, _)) = this.canvas.pressed.take() {
                    let (x, y) = xy(event.position);
                    if print::hit(&this.compare.targets, negs_app::crop::Point { x, y }) == Some(target) {
                        act(this, target, cx);
                    }
                    cx.notify();
                }
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
            let (x, y) = xy(event.position);
            let p = negs_app::crop::Point { x, y };
            let cursor = crate::compare_view::cursor(this, p);
            let hot = print::hit(&this.compare.targets, p);
            if cursor != this.canvas.compare_cursor || hot != this.compare.hot {
                this.canvas.compare_cursor = cursor;
                this.compare.hot = hot;
                cx.notify();
            }
        }))
        .child(view)
}

/// The canvas column with nothing open (`SmallStates.swift`): the trim
/// marks where the print will go, "Open a photograph" with Open the word
/// to click, a line about the app, where to drop a file, and the recent
/// photographs where the label goes.
fn render_empty_state(app: &NegsApp, ink: &SheetInk, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let theme = &app.theme;
    let canvas = rect_of(app.canvas.last_canvas_bounds);
    let local = Rect::new(0.0, 0.0, canvas.width.max(1.0), canvas.height.max(1.0));
    let place = print::fit_rect(local, Some(1.5)).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    let marks = print::trim_marks(place);
    let recent: Vec<std::path::PathBuf> = app.model.prefs.recent_files.iter().take(5).cloned().collect();
    let entity = cx.entity().clone();
    let measure = canvas_probe(entity);
    let accent = hex(ink.accent);
    let mut root = div()
        .id("empty-canvas")
        .relative()
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(hex(ink.sheet))
        .overflow_hidden()
        .drag_over::<ExternalPaths>(move |style, _, _, _| style.border_2().border_color(accent))
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            this.open_dropped_paths(paths.paths(), cx);
        }))
        .child(measure);
    for (m, _) in marks {
        root = root.child(div().absolute().left(px(m.x as f32)).top(px(m.y as f32)).w(px(m.width as f32)).h(px(m.height as f32)).bg(hex(ink.track)));
    }
    root = root.child(
        div()
            .absolute()
            .left(px(place.x as f32))
            .top(px(place.y as f32))
            .w(px(place.width as f32))
            .h(px(place.height as f32))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .font_family(theme.serif_family.clone())
                    .font_weight(gpui::FontWeight(600.0))
                    .text_size(px(26.0))
                    .text_color(hex(ink.paper))
                    .child(
                        div()
                            .id("empty-open")
                            .cursor_pointer()
                            .border_b(px(1.5))
                            .border_color(hex(ink.track))
                            .hover(|s| s.border_color(hex(ink.dim)))
                            .on_click(cx.listener(|this, _, window, cx| this.prompt_open_photo(window, cx)))
                            .child("Open"),
                    )
                    .child(div().child("\u{a0}a photograph")),
            )
            .child(
                div()
                    .font_family(theme.serif_family.clone())
                    .italic()
                    .text_size(px(16.0))
                    .text_color(hex(ink.label))
                    .child("Explore film stocks, shape the print, and make it your own."),
            )
            .child(div().pt(px(6.0)).text_size(px(13.0)).text_color(hex(ink.dim)).child("or drop a RAW, TIFF, PNG or JPEG file, or a folder, here")),
    );
    if !recent.is_empty() {
        let mut row = div()
            .absolute()
            .left(px(place.x as f32))
            .top(px((place.max_y() + 14.0) as f32))
            .flex()
            .items_baseline()
            .gap(px(18.0))
            .child(div().font_family(theme.serif_family.clone()).italic().text_size(px(14.0)).text_color(hex(ink.dim)).child("Recent"));
        for (i, path) in recent.into_iter().enumerate() {
            let name = negs_app::model::file_name(&path);
            let (paper, label) = (hex(ink.paper), hex(ink.label));
            row = row.child(
                div()
                    .id(("recent", i))
                    .cursor_pointer()
                    .font_family(theme.serif_family.clone())
                    .text_size(px(14.0))
                    .text_color(label)
                    .hover(move |s| s.text_color(paper))
                    .on_click(cx.listener(move |this, _, _window, cx| {
                        this.model.open(&path);
                        this.after_model_update(cx);
                        cx.notify();
                    }))
                    .child(name),
            );
        }
        root = root.child(row);
    }
    root
}

/// An invisible canvas that keeps `last_canvas_bounds` current with
/// nothing open, so the empty state lays out on the canvas's size.
fn canvas_probe(entity: gpui::Entity<NegsApp>) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let sf = window.scale_factor();
            entity.update(cx, |this, cx| {
                if this.canvas.last_canvas_bounds != bounds {
                    this.canvas.last_canvas_bounds = bounds;
                    cx.notify();
                }
                this.canvas.scale_factor = sf;
            });
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_never_enlarges_past_one_to_one_and_leaves_the_bands() {
        let canvas = Rect::new(0.0, 0.0, 1000.0, 722.0);
        // A small photograph stays 1:1.
        assert_eq!(fit_zoom(canvas, 2.0, 300.0, 200.0), 1.0);
        // A large one fits 904 × 600 DIPs at 2×.
        let z = fit_zoom(canvas, 2.0, 6000.0, 4000.0);
        assert!((z - (600.0 * 2.0 / 4000.0)).abs() < 1e-9, "{z}");
        assert!(!is_zoomed(true, 5.0, z));
        assert!(!is_zoomed(false, z * 1.004, z));
        assert!(is_zoomed(false, z * 1.25, z));
    }

    #[test]
    fn a_click_reads_where_it_fell_on_the_photograph() {
        let photo = Rect::new(100.0, 50.0, 400.0, 200.0);
        assert_eq!(shown_fraction(photo, (300.0, 100.0), false), Some([0.5, 0.25]));
        assert_eq!(shown_fraction(photo, (300.0, 100.0), true), Some([0.5, 0.75]));
        let outside = shown_fraction(photo, (90.0, 100.0), false).unwrap();
        assert!(outside[0] < 0.0);
        assert_eq!(shown_fraction(Rect::new(0.0, 0.0, 0.0, 0.0), (1.0, 1.0), false), None);
    }
}
