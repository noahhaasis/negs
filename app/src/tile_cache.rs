//! Shared color-managed preview tile `RenderImage` cache with frame-scoped
//! VRAM cleanup (`window.drop_image`).
//!
//! `negs_app::previews::Bitmap` already stores top-down BGRA8 pixels transformed
//! through the active display's ICC/P3 profile and oriented to match the photo.
//! `TileImageCache` converts each distinct `Arc<Bitmap>` into a GPUI
//! `Arc<RenderImage>` once, keeps the `Arc<Bitmap>` alive alongside it so its
//! pointer address can never be reused while cached, and evicts unused entries
//! on every frame (or on `clear`) to prevent GPU atlas leaks.

use std::collections::HashMap;
use std::sync::Arc;

use gpui::{Bounds, Pixels, RenderImage, Window, point, px, size};
use negs_app::previews::Bitmap;

use crate::color_display::bgra_to_render_image;

struct CachedTile {
    /// Held strongly so `Arc::as_ptr` remains unique for the lifetime of this entry.
    bitmap: Arc<Bitmap>,
    image: Arc<RenderImage>,
    epoch: u64,
}

/// Color-managed `Bitmap` -> GPUI `RenderImage` cache with deterministic VRAM eviction.
#[derive(Default)]
pub struct TileImageCache {
    entries: HashMap<usize, CachedTile>,
    epoch: u64,
    pending_drops: Vec<Arc<RenderImage>>,
}

impl TileImageCache {
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a new render frame. Any entry not touched via [`Self::get_or_insert`]
    /// during this frame will be evicted when [`Self::retain_used`] or
    /// [`Self::evict_unused`] runs.
    pub fn begin_frame(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
    }

    /// Returns the cached `Arc<RenderImage>` for `bitmap`, or uploads a new one
    /// from its color-managed BGRA8 pixels and marks it used in the current frame.
    pub fn get_or_insert(&mut self, bitmap: &Arc<Bitmap>) -> Arc<RenderImage> {
        let key = Arc::as_ptr(bitmap) as usize;
        if let Some(entry) = self.entries.get_mut(&key) {
            if Arc::ptr_eq(&entry.bitmap, bitmap) {
                entry.epoch = self.epoch;
                return entry.image.clone();
            }
        }
        let image = bgra_to_render_image(bitmap.bgra.clone(), bitmap.width, bitmap.height);
        if let Some(old) = self.entries.insert(
            key,
            CachedTile {
                bitmap: Arc::clone(bitmap),
                image: image.clone(),
                epoch: self.epoch,
            },
        ) {
            self.pending_drops.push(old.image);
        }
        image
    }

    /// Evicts every entry not used in the current frame and returns their
    /// `RenderImage` handles for GPU cleanup.
    pub fn evict_unused(&mut self) -> Vec<Arc<RenderImage>> {
        let current = self.epoch;
        let mut evicted = Vec::new();
        self.entries.retain(|_, entry| {
            if entry.epoch == current {
                true
            } else {
                evicted.push(entry.image.clone());
                false
            }
        });
        evicted
    }

    /// Evicts entries not used in the current frame and immediately releases all
    /// dropped `RenderImage`s from GPUI's GPU sprite atlas via `window.drop_image`.
    pub fn retain_used(&mut self, window: &mut Window) {
        let evicted = self.evict_unused();
        self.pending_drops.extend(evicted);
        self.flush_drops(window);
    }

    /// Moves every cached entry into the pending VRAM drop queue without needing a `Window`.
    pub fn invalidate(&mut self) {
        for (_, entry) in self.entries.drain() {
            self.pending_drops.push(entry.image);
        }
    }

    /// Releases every cached `RenderImage` immediately via `window.drop_image`.
    pub fn clear(&mut self, window: &mut Window) {
        self.invalidate();
        self.flush_drops(window);
    }

    /// Flushes all queued `RenderImage` drops against GPUI's sprite atlas.
    pub fn flush_drops(&mut self, window: &mut Window) {
        for img in self.pending_drops.drain(..) {
            let _ = window.drop_image(img);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn pending_drop_count(&self) -> usize {
        self.pending_drops.len()
    }
}

/// Computes centered `object-fit: cover` bounds for an image of size `img_w × img_h`
/// filling `frame` cleanly without distortion. Intended to be painted inside a
/// `window.with_content_mask(Some(ContentMask { bounds: frame }), ...)`.
pub fn cover_bounds(frame: Bounds<Pixels>, img_w: u32, img_h: u32) -> Bounds<Pixels> {
    let fw = f32::from(frame.size.width);
    let fh = f32::from(frame.size.height);
    if fw <= 0.0 || fh <= 0.0 || img_w == 0 || img_h == 0 {
        return frame;
    }
    let scale = (fw / img_w as f32).max(fh / img_h as f32);
    let draw_w = img_w as f32 * scale;
    let draw_h = img_h as f32 * scale;
    let ox = f32::from(frame.origin.x) + (fw - draw_w) * 0.5;
    let oy = f32::from(frame.origin.y) + (fh - draw_h) * 0.5;
    Bounds {
        origin: point(px(ox), px(oy)),
        size: size(px(draw_w), px(draw_h)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_bitmap(w: u32, h: u32, fill: [u8; 4]) -> Arc<Bitmap> {
        let mut bgra = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            bgra.extend_from_slice(&fill);
        }
        Arc::new(Bitmap {
            bgra,
            width: w,
            height: h,
            source: None,
        })
    }

    #[test]
    fn cache_hit_returns_same_render_image_and_holds_bitmap() {
        let mut cache = TileImageCache::new();
        let bmp = test_bitmap(4, 3, [10, 20, 30, 255]);
        assert_eq!(Arc::strong_count(&bmp), 1);

        cache.begin_frame();
        let first = cache.get_or_insert(&bmp);
        assert_eq!(cache.len(), 1);
        assert_eq!(Arc::strong_count(&bmp), 2, "cache keeps Bitmap alive to prevent pointer reuse");

        let second = cache.get_or_insert(&bmp);
        assert!(Arc::ptr_eq(&first, &second), "second lookup must hit cache");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_miss_inserts_distinct_render_images() {
        let mut cache = TileImageCache::new();
        let a = test_bitmap(4, 4, [255, 0, 0, 255]);
        let b = test_bitmap(4, 4, [0, 255, 0, 255]);

        cache.begin_frame();
        let img_a = cache.get_or_insert(&a);
        let img_b = cache.get_or_insert(&b);
        assert!(!Arc::ptr_eq(&img_a, &img_b));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn frame_eviction_drops_only_unused_entries() {
        let mut cache = TileImageCache::new();
        let a = test_bitmap(8, 6, [1, 2, 3, 255]);
        let b = test_bitmap(8, 6, [4, 5, 6, 255]);

        cache.begin_frame();
        let img_a = cache.get_or_insert(&a);
        let img_b = cache.get_or_insert(&b);
        assert!(cache.evict_unused().is_empty());

        // Frame 2: only touch `a`, so `b` must be evicted.
        cache.begin_frame();
        let img_a2 = cache.get_or_insert(&a);
        assert!(Arc::ptr_eq(&img_a, &img_a2));

        let evicted = cache.evict_unused();
        assert_eq!(evicted.len(), 1);
        assert!(Arc::ptr_eq(&evicted[0], &img_b));
        assert_eq!(cache.len(), 1);
        assert_eq!(Arc::strong_count(&b), 1, "evicted Bitmap strong ref released");

        // Frame 3: touch nothing -> `a` is evicted too.
        cache.begin_frame();
        let evicted2 = cache.evict_unused();
        assert_eq!(evicted2.len(), 1);
        assert!(Arc::ptr_eq(&evicted2[0], &img_a));
        assert!(cache.is_empty());
    }

    #[test]
    fn invalidate_queues_all_images_for_drop() {
        let mut cache = TileImageCache::new();
        let a = test_bitmap(2, 2, [0, 0, 0, 255]);
        let b = test_bitmap(2, 2, [255, 255, 255, 255]);

        cache.begin_frame();
        let _ = cache.get_or_insert(&a);
        let _ = cache.get_or_insert(&b);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.pending_drop_count(), 0);

        cache.invalidate();
        assert!(cache.is_empty());
        assert_eq!(cache.pending_drop_count(), 2);
        assert_eq!(Arc::strong_count(&a), 1);
        assert_eq!(Arc::strong_count(&b), 1);
    }

    #[test]
    fn cover_bounds_fills_frame_and_centers_excess() {
        let frame = Bounds {
            origin: point(px(10.0), px(20.0)),
            size: size(px(212.0), px(118.0)),
        };
        // 3:2 photo inside 9:5 tile -> width matches 212, height is 141.33 centered vertically.
        let covered = cover_bounds(frame, 300, 200);
        assert!((f32::from(covered.size.width) - 212.0).abs() < 0.01);
        assert!(f32::from(covered.size.height) > 140.0);
        assert!((f32::from(covered.origin.x) - 10.0).abs() < 0.01);
        assert!(f32::from(covered.origin.y) < 20.0);

        // Panoramic 3:1 photo inside 9:5 tile -> height matches 118, width overflows horizontally.
        let wide = cover_bounds(frame, 300, 100);
        assert!((f32::from(wide.size.height) - 118.0).abs() < 0.01);
        assert!((f32::from(wide.size.width) - 354.0).abs() < 0.01);
        assert!(f32::from(wide.origin.x) < 10.0);
    }
}
