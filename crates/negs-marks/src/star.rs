//! A rating star drawn by hand (round 22, `r22-edge.html`, A; HandStar.swift):
//! three strokes through the middle, the upright one first, each seeded from
//! the file name and the star's place in the row.

use crate::{Path, Profile, Pt, Random, hash, lerp, pt, ribbon};

/// The box each star is drawn in (round (0, 0)), and the gap between two.
pub const STAR_SIZE: f64 = 12.0;
pub const STAR_GAP: f64 = 2.5;

const PROFILE: Profile = Profile(&[(0.0, 0.9), (0.15, 1.75), (0.85, 1.55), (1.0, 0.7)]);

/// The page's seed: the name, "·star" and the star's place, from 0.
pub fn star_seed(name: &str, index: usize) -> u32 {
    hash(&format!("{name}\u{00B7}star{index}"))
}

/// The three strokes of the `index`th star in `name`'s row, round (0, 0),
/// each a filled outline.
pub fn star_strokes(name: &str, index: usize) -> Vec<Vec<Pt>> {
    let mut random = Random::new(star_seed(name, index));
    let scale = lerp(0.9, 1.15, random.next());
    let tilt = random.jitter(9.0);
    let cx = random.jitter(0.5);
    let cy = random.jitter(0.5);
    let mut strokes = Vec::with_capacity(3);
    for a0 in [90.0, 30.0, 150.0] {
        let a = (a0 + tilt + random.jitter(8.0)) * std::f64::consts::PI / 180.0;
        let length = lerp(4.6, 5.8, random.next());
        let off = random.jitter(0.7);
        let (ca, sa) = (a.cos(), a.sin());
        let from = pt(cx + off - ca * length, cy - sa * length);
        let to_x = cx + ca * length * lerp(0.85, 1.05, random.next());
        let to_y = cy + sa * length * lerp(0.85, 1.05, random.next()) + off;
        strokes.push(stroke(from, pt(to_x, to_y), 0.25, &mut random, |t| PROFILE.width(t) * scale));
    }
    strokes
}

/// The star's strokes as one path round (0, 0).
pub fn star_path(name: &str, index: usize) -> Path {
    let mut path = Path::default();
    for stroke in star_strokes(name, index) {
        path.0.extend(Path::polygon(&stroke).0);
    }
    path
}

/// A line from `a` to `b` in 8 steps, bowed a little to one side.
fn stroke(a: Pt, b: Pt, bow: f64, random: &mut Random, width: impl Fn(f64) -> f64) -> Vec<Pt> {
    let length = (b.x - a.x).hypot(b.y - a.y);
    let l = if length == 0.0 { 1.0 } else { length };
    let (nx, ny) = (-(b.y - a.y) / l, (b.x - a.x) / l);
    let q = (random.next() - 0.5) * 2.0 * bow;
    let points: Vec<Pt> = (0..=8)
        .map(|s| {
            let t = s as f64 / 8.0;
            let d = (std::f64::consts::PI * t).sin() * q;
            pt(lerp(a.x, b.x, t) + nx * d, lerp(a.y, b.y, t) + ny * d)
        })
        .collect();
    ribbon(&points, width)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::check_outline;

    type Star = (&'static str, usize, u32, [(usize, [(f64, f64); 3]); 3]);

    /// From HandStarTests.swift: the page's own script on name + "·star" +
    /// index: the hash, and per stroke its point count, first point, the
    /// point a third of the way round and its last.
    const FROM_THE_PAGE: &[Star] = &[
        ("DSC_6435.NEF", 0, 3_315_392_284, [
            (28, [(-1.99, -4.77), (-0.09, 3.89), (-1.98, -4.98)]),
            (28, [(-5.62, -3.29), (4.14, 3.07), (-5.77, -3.44)]),
            (28, [(4.47, -3.68), (-5.1, 3.21), (4.67, -3.76)]),
        ]),
        ("DSC_6435.NEF", 1, 3_332_169_903, [
            (28, [(0.02, -5.01), (-1.06, 4.73), (0.09, -5.22)]),
            (28, [(-3.96, -2.24), (3.76, 3.89), (-4.08, -2.42)]),
            (28, [(5.12, -2.12), (-5.19, 2.22), (5.34, -2.16)]),
        ]),
        ("DSC_6435.NEF", 4, 3_248_281_808, [
            (28, [(0.18, -5.59), (-0.78, 4.84), (0.27, -5.78)]),
            (28, [(-4.36, -2.06), (4.13, 2.18), (-4.53, -2.19)]),
            (28, [(4.23, -2.86), (-3.94, 1.73), (4.44, -2.91)]),
        ]),
        ("IMG_0001.JPG", 0, 948_015_264, [
            (28, [(-0.13, -4.8), (-0.51, 5.99), (-0.04, -5.03)]),
            (28, [(-4.51, -2.73), (3.9, 4.59), (-4.63, -2.95)]),
            (28, [(4.02, -2.84), (-4.55, 2.54), (4.26, -2.92)]),
        ]),
        ("IMG_0001.JPG", 2, 981_570_502, [
            (28, [(0.07, -4.46), (0.67, 5.6), (0.13, -4.69)]),
            (28, [(-3.9, -1.28), (4.56, 2.95), (-4.06, -1.45)]),
            (28, [(4.36, -2.93), (-3.96, 3.07), (4.58, -3.01)]),
        ]),
        ("DSC_7041.NEF", 3, 1_708_573_339, [
            (28, [(-1.51, -5.29), (1.08, 6.06), (-1.48, -5.53)]),
            (28, [(-5.01, -1.3), (4.69, 1.77), (-5.21, -1.44)]),
            (28, [(4.05, -3.18), (-4.47, 2.1), (4.28, -3.25)]),
        ]),
        ("f03.jpg", 0, 2_243_906_042, [
            (28, [(0.02, -5.24), (-0.79, 5.83), (0.11, -5.46)]),
            (28, [(-5.49, -2.1), (4.59, 3.45), (-5.65, -2.26)]),
            (28, [(4.93, -2.24), (-5.38, 2.27), (5.17, -2.27)]),
        ]),
        // A name past ASCII: the hash runs over UTF-16, as the page's does.
        ("Ålesund 2024.dng", 1, 124_155_722, [
            (28, [(0.2, -5.65), (-0.5, 5.5), (0.28, -5.89)]),
            (28, [(-4.49, -1.42), (5.21, 2.24), (-4.69, -1.57)]),
            (28, [(3.82, -3.61), (-4.21, 2.26), (4.07, -3.68)]),
        ]),
    ];

    #[test]
    fn matches_the_drawing() {
        for (name, index, seed, strokes) in FROM_THE_PAGE {
            let label = format!("{name} #{index}");
            assert_eq!(star_seed(name, *index), *seed, "{label}");
            let got = star_strokes(name, *index);
            assert_eq!(got.len(), 3, "{label}");
            for (points, (count, want)) in got.iter().zip(strokes) {
                check_outline(&label, points, *count, *want);
            }
        }
    }

    #[test]
    fn the_same_star_every_time_and_each_one_its_own() {
        let first = star_path("DSC_6435.NEF", 2);
        assert_eq!(first, star_path("DSC_6435.NEF", 2));
        let row: Vec<_> = (0..5).map(|i| star_path("DSC_6435.NEF", i)).collect();
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(row[i], row[j]);
            }
        }
        assert_ne!(first, star_path("DSC_6436.NEF", 2));
    }

    #[test]
    fn stays_in_its_box() {
        for n in 0..200 {
            let name = format!("DSC_{}.NEF", 7000 + n);
            for index in 0..5 {
                let b = star_path(&name, index).bounds().unwrap();
                assert!(b.x >= -8.0 && b.y >= -8.0 && b.max_x() <= 8.0 && b.max_y() <= 8.0, "{name} #{index} {b:?}");
                assert!(b.w > 7.0 && b.h > 7.0, "{name}");
                assert!(b.x <= 0.0 && b.max_x() >= 0.0 && b.y <= 0.0 && b.max_y() >= 0.0, "{name}");
            }
        }
    }
}
