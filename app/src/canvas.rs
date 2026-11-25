//! Color-managed exact-fit photo canvas with zoom/pan, Before/After toggle,
//! draggable Split-Screen divider, vector `negs-marks` overlay, drag-and-drop,
//! and VRAM-safe `RenderImage` caching.

use std::sync::Arc;

use gpui::{
    Bounds, ContentMask, Context, Corners, ExternalPaths, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, RenderImage, ScrollDelta, ScrollWheelEvent,
    Window, canvas, div, fill, point, prelude::*, px, size,
};
use negs_app::border::{BorderSettings, BorderStyle};
use negs_app::crop::{CropSettings, Orientation};
use negs_app::picture::Picture;

use crate::NegsApp;
use crate::color_display::{
    DisplayColorProfile, PreparedImage, SourceRegion, color_manage_border_hsla,
    oriented_cropped_dimensions, prepare_exact_fit, prepare_zoomed_tile,
};
use crate::marks_gpui::marks_overlay_badge;

const CANVAS_PADDING_DIPS: f64 = 24.0;
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
    /// Last window scale factor.
    pub scale_factor: f32,
    /// The photograph isn't drawn: it's flying to or from the grid.
    pub hide_photo: bool,
    /// Active monitor's color profile.
    pub display_profile: DisplayColorProfile,
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
            scale_factor: 1.0,
            hide_photo: false,
            display_profile: DisplayColorProfile::default(),
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
    }

    /// Releases any replaced `RenderImage`s from GPUI's sprite atlas.
    pub fn flush_drops(&mut self, window: &mut Window) {
        for img in self.pending_drops.drain(..) {
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
        let sf = self.scale_factor.max(1.0) as f64;
        let avail_w = ((f32::from(self.last_canvas_bounds.size.width) as f64 - 2.0 * CANVAS_PADDING_DIPS)
            .max(64.0))
            * sf;
        let avail_h = ((f32::from(self.last_canvas_bounds.size.height) as f64 - 2.0 * CANVAS_PADDING_DIPS)
            .max(64.0))
            * sf;
        if doc_w <= 0.0 || doc_h <= 0.0 {
            return 1.0;
        }
        (avail_w / doc_w).min(avail_h / doc_h).min(1.0).max(MIN_ZOOM)
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

/// Renders either the darkroom empty state or the live color-managed photo canvas.
pub fn render_canvas(
    app: &mut NegsApp,
    window: &mut Window,
    cx: &mut Context<NegsApp>,
) -> impl IntoElement {
    let theme = app.theme.clone();
    app.canvas.flush_drops(window);

    if app.model.source.is_none() {
        return render_empty_state(app, cx).into_any_element();
    }

    // Current rating & flag for the vector `negs-marks` overlay.
    let (photo_name, rating, flag) = app.current_marks();
    let marks_badge = marks_overlay_badge(photo_name, rating, flag, &theme);

    let entity = cx.entity().clone();
    let accent = theme.accent;
    let badge_bg = theme.badge_bg;
    let on_photo = theme.on_photo;
    let canvas_bg = theme.canvas;
    let split_mode = app.canvas.split_mode;
    let before_after = app.canvas.before_after;
    let has_original = app.model.original.is_some();
    let loading_original = (split_mode || before_after) && !has_original;
    let develop_error = app.model.develop_error.clone();
    let hide_photo = app.canvas.hide_photo;

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
                bounds
            }
        },
        {
            let entity = entity.clone();
            move |bounds: Bounds<Pixels>, _prepaint: Bounds<Pixels>, window, cx| {
                let sf = window.scale_factor().max(1.0);
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

                    let orientation = this.model.state.orientation();
                    let crop = this.model.state.crop;
                    let border = Some(this.model.state.border);
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
                let (doc_w, doc_h) = match active_border {
                    Some(b) => b.document(oriented_w, oriented_h, &film),
                    None => (oriented_w, oriented_h),
                };

                // Compute effective zoom (physical pixels per document pixel).
                let avail_w_phys = ((f32::from(bounds.size.width) as f64 - 2.0 * CANVAS_PADDING_DIPS)
                    .max(64.0))
                    * sf as f64;
                let avail_h_phys = ((f32::from(bounds.size.height) as f64 - 2.0 * CANVAS_PADDING_DIPS)
                    .max(64.0))
                    * sf as f64;
                let effective_zoom = if fit {
                    (avail_w_phys / doc_w as f64)
                        .min(avail_h_phys / doc_h as f64)
                        .min(1.0)
                        .max(MIN_ZOOM)
                } else {
                    zoom
                };

                let target_w = ((doc_w as f64 * effective_zoom).round() as u32).max(1);
                let target_h = ((doc_h as f64 * effective_zoom).round() as u32).max(1);

                let canvas_cx_phys =
                    (f32::from(bounds.origin.x) + f32::from(bounds.size.width) * 0.5) * sf;
                let canvas_cy_phys =
                    (f32::from(bounds.origin.y) + f32::from(bounds.size.height) * 0.5) * sf;
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

                // Compute visible source region for zoomed-in (>= 100%) native tile mode.
                let use_zoomed_tile =
                    effective_zoom >= 0.999 || (oriented_w <= target_w && oriented_h <= target_h);
                let visible_region = if use_zoomed_tile {
                    let vp_x0 = f32::from(bounds.origin.x) * sf;
                    let vp_y0 = f32::from(bounds.origin.y) * sf;
                    let vp_x1 = vp_x0 + f32::from(bounds.size.width) * sf;
                    let vp_y1 = vp_y0 + f32::from(bounds.size.height) * sf;

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
                    this.canvas.last_photo_bounds = photo_bounds;

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

                // Clip all image painting to the canvas viewport bounds.
                window.with_content_mask(Some(ContentMask { bounds }), |window| {
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
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            this.open_dropped_paths(paths.paths(), cx);
        }))
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _window, cx| {
            let dy = match event.delta {
                ScrollDelta::Pixels(p) => f32::from(p.y) as f64,
                ScrollDelta::Lines(l) => l.y as f64 * 28.0,
            };
            if dy.abs() > 0.01 && this.model.source.is_some() {
                let factor = (1.0 + dy * 0.0025).clamp(0.75, 1.35);
                this.canvas.zoom_by(factor);
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, _window, cx| {
                if this.theme_menu_open {
                    this.theme_menu_open = false;
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
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
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
            cx.listener(|this, _event: &MouseUpEvent, _window, cx| {
                if this.canvas.dragging_split || this.canvas.dragging_pan.is_some() {
                    this.canvas.dragging_split = false;
                    this.canvas.dragging_pan = None;
                    cx.notify();
                }
            }),
        )
        .child(photo_canvas)
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

fn render_empty_state(app: &NegsApp, cx: &mut Context<NegsApp>) -> impl IntoElement {
    let theme = &app.theme;
    div()
        .id("empty-canvas")
        .flex_1()
        .w_full()
        .min_h_0()
        .bg(theme.canvas)
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_4()
        .on_drop(cx.listener(|this, paths: &ExternalPaths, _window, cx| {
            this.open_dropped_paths(paths.paths(), cx);
        }))
        .child(
            div()
                .font_family(theme.serif_family.clone())
                .italic()
                .text_size(px(28.0))
                .text_color(theme.paper)
                .child("Open a photograph"),
        )
        .child(
            div()
                .text_size(px(13.0))
                .text_color(theme.dim)
                .child("Drop a RAW, JPEG, PNG, or TIFF here — or choose one from disk."),
        )
        .child(
            div()
                .id("empty-open-btn")
                .mt_2()
                .px_5()
                .py_2()
                .rounded(px(5.0))
                .bg(theme.fill)
                .text_color(theme.on_fill)
                .text_size(px(13.0))
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .on_click(cx.listener(|this, _, window, cx| {
                    this.prompt_open_photo(window, cx);
                }))
                .child("Open Photograph…"),
        )
        .child(
            div()
                .mt_4()
                .text_size(px(11.0))
                .text_color(theme.dim.opacity(0.75))
                .child(format!(
                    "Shortcuts: ⌘/Ctrl+O Open · \\ Original · Shift+\\ Split · Z Fit/100% · 1–5 Rate · P/X/U Flag · T Theme · Display ICC: {}",
                    app.canvas.display_profile.label
                )),
        )
}
