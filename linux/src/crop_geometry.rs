//! Local crop interaction geometry, ported from the native macOS presentation model.
pub type Rect = [f64; 4];
pub fn fits(r: Rect, angle: f64, size: (f64, f64)) -> bool {
    let (sin, cos) = (-angle.to_radians()).sin_cos();
    let (w, h) = size;
    [
        (r[0], r[1]),
        (r[0] + r[2], r[1]),
        (r[0], r[1] + r[3]),
        (r[0] + r[2], r[1] + r[3]),
    ]
    .iter()
    .all(|(x, y)| {
        let (dx, dy) = ((x - 0.5) * w, (y - 0.5) * h);
        let (x, y) = (
            0.5 + (dx * cos - dy * sin) / w,
            0.5 + (dx * sin + dy * cos) / h,
        );
        (-1e-6..=1.000001).contains(&x) && (-1e-6..=1.000001).contains(&y)
    })
}
pub fn constrain(start: Rect, target: Rect, angle: f64, size: (f64, f64)) -> Rect {
    if fits(target, angle, size) {
        return target;
    }
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..24 {
        let t = (lo + hi) / 2.;
        let r = std::array::from_fn(|i| start[i] + (target[i] - start[i]) * t);
        if fits(r, angle, size) {
            lo = t;
        } else {
            hi = t;
        }
    }
    std::array::from_fn(|i| start[i] + (target[i] - start[i]) * lo)
}
pub fn inscribe(r: Rect, angle: f64, size: (f64, f64)) -> Rect {
    // Choose the nearest of 21 centers retaining at least 98% of the maximum size.
    let (w, h) = size;
    let (sin, cos) = (-angle.to_radians()).sin_cos();
    let mut candidates = Vec::new();
    for i in 0..=20 {
        let t = i as f64 / 20.;
        let (cx, cy) = (r[0] + r[2] / 2., r[1] + r[3] / 2.);
        let (cx, cy) = (cx + (0.5 - cx) * t, cy + (0.5 - cy) * t);
        let (qx, qy) = (
            w / 2. + (cx - 0.5) * w * cos - (cy - 0.5) * h * sin,
            h / 2. + (cx - 0.5) * w * sin + (cy - 0.5) * h * cos,
        );
        let mut scale: f64 = 1.;
        if qx < 0. || qy < 0. || qx > w || qy > h {
            scale = 0.;
        }
        for (x, y) in [(-1., -1.), (1., -1.), (-1., 1.), (1., 1.)] {
            let (dx, dy) = (x * r[2] * w / 2., y * r[3] * h / 2.);
            let (vx, vy) = (dx * cos - dy * sin, dx * sin + dy * cos);
            if vx > 0. {
                scale = scale.min((w - qx) / vx);
            } else if vx < 0. {
                scale = scale.min(qx / -vx);
            }
            if vy > 0. {
                scale = scale.min((h - qy) / vy);
            } else if vy < 0. {
                scale = scale.min(qy / -vy);
            }
        }
        candidates.push((cx, cy, scale.max(0.)));
    }
    let best = candidates.iter().map(|v| v.2).fold(0f64, f64::max);
    let (cx, cy, scale) = candidates.into_iter().find(|v| v.2 >= best * 0.98).unwrap();
    [
        cx - r[2] * scale / 2.,
        cy - r[3] * scale / 2.,
        r[2] * scale,
        r[3] * scale,
    ]
}
#[cfg(test)]
mod tests {
    #[test]
    fn straighten_stays_on_photo_and_preserves_aspect() {
        for angle in [-45., -23.5, 0., 12.3, 45.] {
            let r = super::inscribe([0., 0., 1., 1.], angle, (6000., 4000.));
            assert!(super::fits(r, angle, (6000., 4000.)));
            assert!((r[2] / r[3] - 1.).abs() < 1e-10);
            let moved = super::constrain(r, [0., 0., r[2], r[3]], angle, (6000., 4000.));
            assert!(super::fits(moved, angle, (6000., 4000.)));
        }
    }
}
