//! The window's place across launches, in the preferences the Windows app
//! keeps (`window`: show command, left, top in pixels, width and height in
//! DIPs; `window_pixels`: the DPI it was saved at and the size in pixels),
//! so a profile from `windows/` reopens where it was. A place on no display
//! any more is dropped for the centred default.

use gpui::{Bounds, Pixels, WindowBounds, point, px, size};

const SW_SHOWNORMAL: i32 = 1;
const SW_SHOWMAXIMIZED: i32 = 3;

/// The preferences' two fields for `bounds` at `scale`.
pub fn to_prefs(bounds: WindowBounds, scale: f32) -> ([i32; 5], [i32; 3]) {
    let (show, b) = match bounds {
        WindowBounds::Maximized(b) => (SW_SHOWMAXIMIZED, b),
        WindowBounds::Windowed(b) | WindowBounds::Fullscreen(b) => (SW_SHOWNORMAL, b),
    };
    let s = scale.max(0.5);
    let (x, y) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    (
        [show, (x * s).round() as i32, (y * s).round() as i32, w.round() as i32, h.round() as i32],
        [(96.0 * s).round() as i32, (w * s).round() as i32, (h * s).round() as i32],
    )
}

/// The saved place, if it still lands on one of `displays` (their bounds
/// in DIPs).
pub fn from_prefs(window: Option<[i32; 5]>, pixels: Option<[i32; 3]>, displays: &[Bounds<Pixels>]) -> Option<WindowBounds> {
    let [show, x, y, w, h] = window?;
    let scale = pixels.map_or(1.0, |[dpi, ..]| (dpi as f32 / 96.0).max(0.5));
    if w < 200 || h < 150 {
        return None;
    }
    let b = Bounds { origin: point(px(x as f32 / scale), px(y as f32 / scale)), size: size(px(w as f32), px(h as f32)) };
    // At least a corner of the title bar on a display, or it can't be moved.
    let grab = point(b.origin.x + px(60.0), b.origin.y + px(16.0));
    if !displays.iter().any(|d| d.contains(&grab)) {
        return None;
    }
    Some(if show == SW_SHOWMAXIMIZED { WindowBounds::Maximized(b) } else { WindowBounds::Windowed(b) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_place_round_trips_and_one_off_screen_is_dropped() {
        let b = Bounds { origin: point(px(40.0), px(32.0)), size: size(px(1267.2), px(833.6)) };
        let (window, pixels) = to_prefs(WindowBounds::Windowed(b), 1.25);
        assert_eq!(window, [1, 50, 40, 1267, 834]);
        assert_eq!(pixels, [120, 1584, 1042]);
        let screen = [Bounds { origin: point(px(0.0), px(0.0)), size: size(px(2752.0), px(1152.0)) }];
        let back = from_prefs(Some(window), Some(pixels), &screen).unwrap();
        let WindowBounds::Windowed(r) = back else { panic!("windowed") };
        assert_eq!((f32::from(r.origin.x), f32::from(r.origin.y)), (40.0, 32.0));
        assert!(from_prefs(Some([3, 50, 40, 1267, 834]), Some(pixels), &screen).is_some_and(|b| matches!(b, WindowBounds::Maximized(_))));
        assert!(from_prefs(Some([1, -5000, 40, 1267, 834]), Some(pixels), &screen).is_none());
        assert!(from_prefs(None, None, &screen).is_none());
    }
}
