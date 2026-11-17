//! Finding the film's edge on a scan of a negative: the clear film along
//! the frame (the rebate, or the gap between frames) and the picture's
//! rectangle inside it. Negs' own, after the ideas in flx-film-convert's
//! notes ("The film base", "Crop to Picture"), not its code.
//!
//! The picture is the only part of the film that was exposed. Round it the
//! film is clear: the rebate along the long edges and the gaps between
//! frames at the short ends. Clear film is the thinnest the film gets, flat,
//! and it ends at the gate's edge in a straight step into the picture. So:
//!
//! - Each edge is read in [`PLACES`] places along it. At each place a strip
//!   is followed in from the frame's edge (past the holder's dark surround,
//!   the film's edge and any sprocket hole) and cut into flat runs; a run
//!   that ends in a step up in density is a candidate.
//! - Along an edge, a level that many places share (most of them) and whose
//!   inner ends lie on a straight line is a band. Sprocket holes (bare light)
//!   show in under half the places, a picture's flat parts don't end on a
//!   straight line, and a band that most of the picture is thinner than is
//!   picture, not clear film. Bare light outside the film (past its edge or
//!   its end) is a brighter band outside a darker one, stepping down by the
//!   film base's orange.
//! - The long edges are used first; the gaps at the short ends where the
//!   long edges show no clean rebate. The base is measured on the bands, in
//!   the middle of each run, away from both its steps.
//! - The picture's rectangle comes from the same bands, never from what the
//!   picture shows: their inner edges, levelled by the long edges' slope (or
//!   the short ends' without them), cut [`INSET`] inside.
//!
//! Everything is tuned in pixels of a frame [`GRID_SIDE`] on its long side;
//! another size is resampled to it first, so the answer doesn't depend on
//! the scan's size.

use spektrafilm_math::image::ImageBuf;
use spektrafilm_math::precision::to_f64;

use crate::convert::Edge;

/// The long side the finder works at.
pub const GRID_SIDE: u32 = 1024;
/// Places read along each edge.
const PLACES: usize = 64;
/// How far in from each edge a band is looked for, as a fraction of the
/// frame across it: a gap between frames can be a quarter of the way.
const DEPTH: f64 = 0.3;
/// The shortest run counted, in pixels: a sliver of film at the frame's very
/// edge, blurred by the holder beside it, isn't measured.
const MIN_RUN: usize = 6;
/// A run is flat while every channel stays this close to its mean (log10).
const FLAT: f64 = 0.035;
/// The least step up in density (log10, channel mean) that ends a band.
const STEP: f64 = 0.04;
/// Places share a level within this (log10, each channel); allows for the
/// light falling off along an edge.
const LEVEL_TOL: f64 = 0.08;
/// A band's inner ends lie this close to a straight line, in pixels.
const EDGE_TOL: f64 = 3.0;
/// The share of the places a band must show in ...
const SUPPORT: f64 = 0.5;
/// ... or, at the base's own level, to bound the picture: a picture as thin
/// as the base at the gate hides its edge there.
const BOUND_SUPPORT: f64 = 0.25;
/// The steepest a band's edge may lean (about 3 degrees): film lies square
/// in a holder, give or take.
const MAX_SLOPE: f64 = 0.05;
/// Bare light outside the film is at least this much brighter than the
/// clear film inside it (log10, channel mean) ...
const BARE_STEP: f64 = 0.2;
/// ... and that step is orange: blue drops more than red by at least this.
const BARE_ORANGE: f64 = 0.15;
/// A band most of the picture is thinner than is picture: more than this
/// share of the frame's middle brighter than it by [`THINNER`].
const THINNER_SHARE: f64 = 0.02;
const THINNER: f64 = 0.1;
/// Edges whose clear film agrees within this (log10, each channel).
const SIDES_AGREE: f64 = 0.12;
/// A band within this of the base bounds the picture.
const BOUND_TOL: f64 = 0.15;
/// The picture is cut this far inside its edges, as a fraction of the frame.
pub const INSET: f64 = 0.006;
/// Levelling angles under this (degrees) are left alone: about a pixel
/// across the frame at [`GRID_SIDE`].
const LEVEL_MIN: f64 = 0.05;

/// The clear film the finder measured.
#[derive(Debug, Clone, PartialEq)]
pub struct ClearFilm {
    /// Its mean linear RGB.
    pub rgb: [f64; 3],
    /// The edges it was measured on.
    pub edges: Vec<Edge>,
}

/// The picture's rectangle, from the film's edges: in the analysed frame's
/// own orientation, as fractions of the frame, after turning the frame
/// `angle_deg` degrees clockwise about its centre to level it. This is
/// `spektrafilm_io::crop::Crop { x: x0, y: y0, width: x1 - x0, height:
/// y1 - y0, angle: angle_deg }`.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
    pub angle_deg: f64,
    /// The edges found on the film; the others are the frame's own.
    pub edges: Vec<Edge>,
}

/// What [`find`] found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilmEdges {
    pub clear_film: Option<ClearFilm>,
    pub picture: Option<Picture>,
}

/// Finds the clear film round the picture and the picture's rectangle in
/// `image`, a scan of a negative in linear RGB (the whole frame).
pub fn find(image: &ImageBuf) -> FilmEdges {
    if image.width < 8 || image.height < 8 {
        return FilmEdges::default();
    }
    let grid = Grid::new(image);
    let middle = grid.middle_levels();
    let bands: Vec<(Edge, Vec<Band>)> = Edge::ALL.iter().map(|&edge| (edge, grid.bands(edge, &middle))).collect();
    let chosen: Vec<(Edge, &Band)> = bands.iter().filter_map(|(edge, b)| side_base(b).map(|band| (*edge, band))).collect();
    let long = |edge: Edge| grid.long_edges().contains(&edge);
    let Some(base_sides) = choose_base(&chosen, long) else {
        return FilmEdges::default();
    };
    let rgb = grid.measure(&base_sides);
    let base_level = mean3(&base_sides.iter().map(|(_, b)| b.level).collect::<Vec<_>>());
    let clear_film = ClearFilm { rgb, edges: base_sides.iter().map(|(e, _)| *e).collect() };
    // Every edge with a band at the base's level bounds the picture; where
    // the picture is as thin as the base at the gate, fewer places show it.
    let bounds: Vec<(Edge, Band)> = bands
        .iter()
        .filter_map(|(edge, list)| {
            let band = list
                .iter()
                .filter(|b| !b.picture_like && max_diff(b.level, base_level) <= BOUND_TOL)
                .min_by(|a, b| max_diff(a.level, base_level).total_cmp(&max_diff(b.level, base_level)))
                .cloned()
                .or_else(|| {
                    let need = (BOUND_SUPPORT * PLACES as f64).ceil() as usize;
                    grid.band_at(*edge, &grid.runs_along(*edge), base_level, need, &middle).filter(|b| !b.picture_like)
                })?;
            Some((*edge, band))
        })
        .collect();
    let picture = grid.picture(&bounds);
    FilmEdges { clear_film: Some(clear_film), picture }
}

impl Edge {
    const ALL: [Edge; 4] = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
}

/// One flat run along a strip, in from the frame's edge.
#[derive(Debug, Clone)]
struct Run {
    place: usize,
    start: usize,
    end: usize,
    /// Mean log10 RGB.
    level: [f64; 3],
    /// Where it steps up into denser film, in pixels from the frame's edge.
    edge: f64,
}

/// A level that many places along an edge share, ending on a line.
#[derive(Debug, Clone)]
struct Band {
    level: [f64; 3],
    /// One run per place, those on the line.
    runs: Vec<Run>,
    /// The inner edge, depth = slope · along + intercept (pixels).
    slope: f64,
    intercept: f64,
    /// Most of the frame's middle is thinner than it: picture, not film.
    picture_like: bool,
}

/// The frame at [`GRID_SIDE`], linear and in log10.
struct Grid {
    w: usize,
    h: usize,
    linear: Vec<[f64; 3]>,
    log: Vec<[f64; 3]>,
}

impl Grid {
    fn new(image: &ImageBuf) -> Self {
        let long = image.width.max(image.height) as f64;
        let scale = GRID_SIDE as f64 / long;
        let size = |v: u32| ((v as f64 * scale).round() as usize).max(1);
        let (w, h) = (size(image.width), size(image.height));
        let pixel = |x: u32, y: u32| {
            let p = image.get(x, y);
            [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]
        };
        let linear: Vec<[f64; 3]> = if w == image.width as usize && h == image.height as usize {
            (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| pixel(x as u32, y as u32)).collect()
        } else if w < image.width as usize {
            let reduced = crate::preview::downsample_area(image, (w as u32, h as u32));
            reduced.data.chunks_exact(3).map(|p| [to_f64(p[0]), to_f64(p[1]), to_f64(p[2])]).collect()
        } else {
            // Enlarged by nearest neighbour: a step stays a step.
            let (sx, sy) = (image.width as f64 / w as f64, image.height as f64 / h as f64);
            (0..h)
                .flat_map(|y| (0..w).map(move |x| (x, y)))
                .map(|(x, y)| {
                    let src_x = (((x as f64 + 0.5) * sx) as u32).min(image.width - 1);
                    let src_y = (((y as f64 + 0.5) * sy) as u32).min(image.height - 1);
                    pixel(src_x, src_y)
                })
                .collect()
        };
        let log = linear
            .iter()
            .map(|p| p.map(|v| if v.is_finite() { v.max(1e-5).log10() } else { -5.0 }))
            .collect();
        Grid { w, h, linear, log }
    }

    fn long_edges(&self) -> [Edge; 2] {
        if self.w >= self.h { [Edge::Top, Edge::Bottom] } else { [Edge::Left, Edge::Right] }
    }

    /// Pixels along the edge and in across it.
    fn along_len(&self, edge: Edge) -> usize {
        match edge {
            Edge::Left | Edge::Right => self.h,
            Edge::Top | Edge::Bottom => self.w,
        }
    }
    fn across_len(&self, edge: Edge) -> usize {
        match edge {
            Edge::Left | Edge::Right => self.w,
            Edge::Top | Edge::Bottom => self.h,
        }
    }
    /// The pixel `depth` in from `edge`, `along` it.
    fn index(&self, edge: Edge, along: usize, depth: usize) -> usize {
        let (x, y) = match edge {
            Edge::Left => (depth, along),
            Edge::Right => (self.w - 1 - depth, along),
            Edge::Top => (along, depth),
            Edge::Bottom => (along, self.h - 1 - depth),
        };
        y * self.w + x
    }
    /// A point `depth` in from `edge` and `along` it, in continuous pixels
    /// of the frame (pixel `i` covers `[i, i + 1)`).
    fn point(&self, edge: Edge, along: f64, depth: f64) -> (f64, f64) {
        let (w, h) = (self.w as f64, self.h as f64);
        match edge {
            Edge::Left => (depth, along),
            Edge::Right => (w - depth, along),
            Edge::Top => (along, depth),
            Edge::Bottom => (along, h - depth),
        }
    }
    /// The pixels of the strip at `place`, along.
    fn strip(&self, edge: Edge, place: usize) -> (usize, usize) {
        let len = self.along_len(edge);
        let a = place * len / PLACES;
        let b = ((place + 1) * len / PLACES).max(a + 1).min(len);
        (a, b)
    }

    /// The median log10 RGB across the strip at each depth.
    fn profile(&self, edge: Edge, place: usize, depth: usize) -> Vec<[f64; 3]> {
        let (a, b) = self.strip(edge, place);
        let mut values = Vec::with_capacity(b - a);
        (0..depth)
            .map(|t| {
                let mut out = [0.0; 3];
                for c in 0..3 {
                    values.clear();
                    values.extend((a..b).map(|s| self.log[self.index(edge, s, t)][c]));
                    out[c] = median(&mut values);
                }
                out
            })
            .collect()
    }

    /// The channel mean of the log10 RGB over the frame's middle half, sorted.
    fn middle_levels(&self) -> Vec<f64> {
        let mut levels: Vec<f64> = (self.h / 4..self.h - self.h / 4)
            .flat_map(|y| (self.w / 4..self.w - self.w / 4).map(move |x| (x, y)))
            .map(|(x, y)| mean_of(self.log[y * self.w + x]))
            .collect();
        levels.sort_by(f64::total_cmp);
        levels
    }

    /// The flat runs ending in a step along `edge`, at every place.
    fn runs_along(&self, edge: Edge) -> Vec<Run> {
        let depth = ((self.across_len(edge) as f64 * DEPTH).round() as usize).min(self.across_len(edge));
        (0..PLACES).flat_map(|place| runs(place, &self.profile(edge, place, depth))).collect()
    }

    fn centre(&self, edge: Edge, place: usize) -> f64 {
        let (a, b) = self.strip(edge, place);
        (a + b) as f64 / 2.0
    }

    /// The bands along `edge`, brightest first.
    fn bands(&self, edge: Edge, middle: &[f64]) -> Vec<Band> {
        let mut pool = self.runs_along(edge);
        let need = (SUPPORT * PLACES as f64).ceil() as usize;
        let mut bands = Vec::new();
        while !pool.is_empty() {
            // The level most places share.
            let support = |level: [f64; 3]| places_near(&pool, level).len();
            let best = pool
                .iter()
                .map(|r| (support(r.level), r))
                .max_by(|(n, a), (m, b)| n.cmp(m).then(mean_of(a.level).total_cmp(&mean_of(b.level))))
                .map(|(n, r)| (n, r.level));
            let Some((n, seed)) = best else { break };
            if n < need {
                break;
            }
            let level = median_level(&places_near(&pool, seed));
            let band = self.band_at(edge, &pool, level, need, middle);
            pool.retain(|r| max_diff(r.level, level) > LEVEL_TOL);
            bands.extend(band);
        }
        bands.sort_by(|a, b| mean_of(b.level).total_cmp(&mean_of(a.level)));
        bands
    }

    /// The runs at `level` whose inner ends lie on one straight line, in at
    /// least `need` places: past a sprocket hole, the band inside it.
    fn band_at(&self, edge: Edge, pool: &[Run], level: [f64; 3], need: usize, middle: &[f64]) -> Option<Band> {
        let near: Vec<&Run> = pool.iter().filter(|r| max_diff(r.level, level) <= LEVEL_TOL).collect();
        let points: Vec<(usize, f64, f64)> = near.iter().map(|r| (r.place, self.centre(edge, r.place), r.edge)).collect();
        let (slope, intercept) = consensus_line(&points)?;
        // Each place's run nearest the line.
        let mut on_line: Vec<Run> = Vec::new();
        for run in near {
            let off = (run.edge - (slope * self.centre(edge, run.place) + intercept)).abs();
            if off > EDGE_TOL {
                continue;
            }
            match on_line.iter_mut().find(|c| c.place == run.place) {
                Some(c) if (c.edge - (slope * self.centre(edge, c.place) + intercept)).abs() <= off => {}
                Some(c) => *c = run.clone(),
                None => on_line.push(run.clone()),
            }
        }
        if on_line.len() < need {
            return None;
        }
        let (slope, intercept) = least_squares(&on_line.iter().map(|r| (self.centre(edge, r.place), r.edge)).collect::<Vec<_>>());
        let level = median_level(&on_line.iter().collect::<Vec<_>>());
        let thinner = middle.len() - middle.partition_point(|&v| v <= mean_of(level) + THINNER);
        let picture_like = thinner as f64 > THINNER_SHARE * middle.len() as f64;
        Some(Band { level, runs: on_line, slope, intercept, picture_like })
    }

    /// The mean linear RGB of the clear film in the middle of the bands'
    /// runs, places far off the rest left out.
    fn measure(&self, sides: &[(Edge, &Band)]) -> [f64; 3] {
        let mut patches: Vec<([f64; 3], [f64; 3])> = Vec::new();
        for (edge, band) in sides {
            for run in &band.runs {
                let (a, b) = self.strip(*edge, run.place);
                let len = run.end - run.start;
                let (d0, d1) = (run.start + len / 4, (run.end - len / 4).max(run.start + len / 4 + 1));
                let mut sum = [0.0; 3];
                let mut n = 0.0;
                for s in a..b {
                    for t in d0..d1 {
                        let p = self.linear[self.index(*edge, s, t)];
                        if p.iter().all(|v| v.is_finite()) {
                            for c in 0..3 {
                                sum[c] += p[c];
                            }
                            n += 1.0;
                        }
                    }
                }
                if n > 0.0 {
                    let rgb = sum.map(|v| v / n);
                    patches.push((rgb, rgb.map(|v| v.max(1e-5).log10())));
                }
            }
        }
        let logs: Vec<[f64; 3]> = patches.iter().map(|p| p.1).collect();
        let centre = median_level(&logs.iter().collect::<Vec<_>>());
        let kept: Vec<[f64; 3]> = patches.iter().filter(|p| max_diff(p.1, centre) <= 0.03).map(|p| p.0).collect();
        if kept.is_empty() {
            mean3(&patches.iter().map(|p| p.0).collect::<Vec<_>>())
        } else {
            mean3(&kept)
        }
    }

    /// The picture's rectangle from the bands that bound it.
    fn picture(&self, bounds: &[(Edge, Band)]) -> Option<Picture> {
        if bounds.is_empty() {
            return None;
        }
        let (w, h) = (self.w as f64, self.h as f64);
        // The tilt (clockwise, y down) each band's line shows.
        let tilt = |edge: Edge, slope: f64| match edge {
            Edge::Top | Edge::Right => slope.atan(),
            Edge::Bottom | Edge::Left => -slope.atan(),
        };
        let long = self.long_edges();
        let levelling: Vec<&(Edge, Band)> = if bounds.iter().any(|(e, _)| long.contains(e)) {
            bounds.iter().filter(|(e, _)| long.contains(e)).collect()
        } else {
            bounds.iter().collect()
        };
        let weight: f64 = levelling.iter().map(|(_, b)| b.runs.len() as f64).sum();
        let theta = levelling.iter().map(|(e, b)| tilt(*e, b.slope) * b.runs.len() as f64).sum::<f64>() / weight;
        let mut angle_deg = -theta.to_degrees();
        if angle_deg.abs() < LEVEL_MIN {
            angle_deg = 0.0;
        }
        let a = angle_deg.to_radians();
        let (sin, cos) = a.sin_cos();
        let (cx, cy) = (w / 2.0, h / 2.0);
        // Turned to level, as the crop turns the photograph.
        let level_point = |(x, y): (f64, f64)| (cx + (x - cx) * cos - (y - cy) * sin, cy + (x - cx) * sin + (y - cy) * cos);
        // [left, right, top, bottom]: found edges first, cut inside.
        let mut rect: [Option<f64>; 4] = [None; 4];
        for (edge, band) in bounds {
            let strip_centre = |place: usize| {
                let (s0, s1) = self.strip(*edge, place);
                (s0 + s1) as f64 / 2.0
            };
            let mut values: Vec<f64> = band
                .runs
                .iter()
                .map(|r| {
                    let along = strip_centre(r.place);
                    let p = level_point(self.point(*edge, along, band.slope * along + band.intercept));
                    match edge {
                        Edge::Left | Edge::Right => p.0,
                        Edge::Top | Edge::Bottom => p.1,
                    }
                })
                .collect();
            let v = median(&mut values);
            let (slot, value) = match edge {
                Edge::Left => (0, v + INSET * w),
                Edge::Right => (1, v - INSET * w),
                Edge::Top => (2, v + INSET * h),
                Edge::Bottom => (3, v - INSET * h),
            };
            rect[slot] = Some(value);
        }
        let found: Vec<Edge> = bounds.iter().map(|(e, _)| *e).collect();
        let [x0, x1, y0, y1] = on_the_frame(rect, a, w, h);
        // The frame's own edges are cut inside too: the holder or a sliver
        // of the film's edge too thin to measure often shows there.
        let inset = |found: Option<f64>, v: f64, by: f64| if found.is_some() { v } else { v + by };
        let (x0, x1) = (inset(rect[0], x0, INSET * w), inset(rect[1], x1, -INSET * w));
        let (y0, y1) = (inset(rect[2], y0, INSET * h), inset(rect[3], y1, -INSET * h));
        let (x0, x1, y0, y1) = ((x0 / w).max(0.0), (x1 / w).min(1.0), (y0 / h).max(0.0), (y1 / h).min(1.0));
        // A picture this small is something else.
        if !(x1 - x0 > 0.3 && y1 - y0 > 0.3) {
            return None;
        }
        Some(Picture { x0, y0, x1, y1, angle_deg, edges: found })
    }
}

/// The levelled rectangle `[left, right, top, bottom]` (pixels of the `w` ×
/// `h` frame turned by `a` radians about its centre; `None` where no film
/// edge was found, the frame's own edge) brought inside the frame: every
/// corner on the turned frame, and the rectangle within `w` × `h`, as the
/// crop needs.
///
/// The frame's own edges come in first. A found edge comes in too where it
/// would still reach past the frame (a picture close to the frame's edge,
/// levelled): only as far as it must, so the crop keeps nearly the most
/// of the picture.
fn on_the_frame(rect: [Option<f64>; 4], a: f64, w: f64, h: f64) -> [f64; 4] {
    let [mut x0, mut x1, mut y0, mut y1] = [rect[0].unwrap_or(0.0), rect[1].unwrap_or(w), rect[2].unwrap_or(0.0), rect[3].unwrap_or(h)];
    if a == 0.0 {
        // Unturned: the caller clamps it to the frame.
        return [x0, x1, y0, y1];
    }
    let (sin, cos) = a.sin_cos();
    let (cx, cy) = (w / 2.0, h / 2.0);
    // A point of the levelled frame, back on the scan.
    let source = |(x, y): (f64, f64)| (cx + (x - cx) * cos + (y - cy) * sin, cy - (x - cx) * sin + (y - cy) * cos);
    let on_scan = |p: (f64, f64)| {
        let (sx, sy) = source(p);
        (-1e-9..=w + 1e-9).contains(&sx) && (-1e-9..=h + 1e-9).contains(&sy)
    };
    let within = |(x, y): (f64, f64)| (-1e-9..=w + 1e-9).contains(&x) && (-1e-9..=h + 1e-9).contains(&y);
    // A tenth of a pixel at a time, each corner off the frame moving its
    // sides: in the first pass only those not found, then any.
    const STEP_PX: f64 = 0.1;
    for any in [false, true] {
        for _ in 0..20_000 {
            let corners = [((x0, y0), 0, 2), ((x1, y0), 1, 2), ((x0, y1), 0, 3), ((x1, y1), 1, 3)];
            let mut moved = false;
            for (corner, side_x, side_y) in corners {
                if on_scan(corner) && (!any || within(corner)) {
                    continue;
                }
                // Past the frame's own edge across, or along: that side; off
                // the turned scan: both.
                let (x, y) = corner;
                let past_x = !(-1e-9..=w + 1e-9).contains(&x);
                let past_y = !(-1e-9..=h + 1e-9).contains(&y);
                let sides: &[usize] = match (any && (past_x || past_y), past_x, past_y) {
                    (true, true, false) => &[side_x],
                    (true, false, true) => &[side_y],
                    _ => &[side_x, side_y],
                };
                for &side in sides {
                    if any || rect[side].is_none() {
                        match side {
                            0 => x0 += STEP_PX,
                            1 => x1 -= STEP_PX,
                            2 => y0 += STEP_PX,
                            _ => y1 -= STEP_PX,
                        }
                        moved = true;
                    }
                }
            }
            if !moved || x1 - x0 < 1.0 || y1 - y0 < 1.0 {
                break;
            }
        }
    }
    [x0, x1, y0, y1]
}

/// The flat runs along a profile that end in a step up in density.
fn runs(place: usize, p: &[[f64; 3]]) -> Vec<Run> {
    let n = p.len();
    let mut found: Vec<(usize, usize, [f64; 3])> = Vec::new();
    let mut t = 0;
    while t < n {
        let mut sum = p[t];
        let mut e = t + 1;
        while e < n {
            let len = (e - t) as f64;
            if (0..3).any(|c| (p[e][c] - sum[c] / len).abs() > FLAT) {
                break;
            }
            for c in 0..3 {
                sum[c] += p[e][c];
            }
            e += 1;
        }
        let len = e - t;
        if len >= MIN_RUN {
            let level = sum.map(|v| v / len as f64);
            // A run broken by a slow drift goes on as one.
            match found.last_mut() {
                Some(last) if t - last.1 <= 2 && max_diff(last.2, level) <= FLAT => {
                    let (l0, l1) = ((last.1 - last.0) as f64, len as f64);
                    last.2 = [0, 1, 2].map(|c| (last.2[c] * l0 + level[c] * l1) / (l0 + l1));
                    last.1 = e;
                }
                _ => found.push((t, e, level)),
            }
            t = e;
        } else {
            t += 1;
        }
    }
    let mean_at = |t: usize| mean_of(p[t]);
    found
        .into_iter()
        .filter_map(|(start, end, level)| {
            // What follows, past the step's blur.
            if end + 10 > n {
                return None;
            }
            let after = (end + 3..end + 10).map(mean_at).sum::<f64>() / 7.0;
            let here = mean_of(level);
            if here - after < STEP {
                return None;
            }
            // Where it crosses halfway, between pixel centres.
            let mid = (here + after) / 2.0;
            let lo = end.saturating_sub(2).max(start + 1);
            let crossing = (lo..end + 10).find(|&t| mean_at(t) < mid)?;
            let (a, b) = (mean_at(crossing - 1), mean_at(crossing));
            let f = if a > b { ((a - mid) / (a - b)).clamp(0.0, 1.0) } else { 0.5 };
            Some(Run { place, start, end, level, edge: crossing as f64 - 0.5 + f })
        })
        .collect()
}

/// The clear film along one edge: its brightest band that isn't picture,
/// unless that is bare light outside the film, a brighter band outside a
/// darker one stepping down by the base's orange.
fn side_base(bands: &[Band]) -> Option<&Band> {
    let film: Vec<&Band> = bands.iter().filter(|b| !b.picture_like).collect();
    let first = *film.first()?;
    if let Some(&next) = film.get(1) {
        let step = [0, 1, 2].map(|c| first.level[c] - next.level[c]);
        let outside = median_edge(first) < median_edge(next);
        if outside && mean_of(step) >= BARE_STEP && step[2] - step[0] >= BARE_ORANGE {
            return Some(next);
        }
    }
    Some(first)
}

fn median_edge(band: &Band) -> f64 {
    let mut edges: Vec<f64> = band.runs.iter().map(|r| r.edge).collect();
    median(&mut edges)
}

/// The edges the base is measured on: the long edges' rebate, or the gaps
/// between frames at the short ends without it. Where two disagree the
/// thinner film is the base along the long edges (fog only adds density),
/// and at the ends the denser (bare light past the film's end shows at one
/// end only).
fn choose_base<'a>(chosen: &[(Edge, &'a Band)], long: impl Fn(Edge) -> bool) -> Option<Vec<(Edge, &'a Band)>> {
    let pick = |set: Vec<(Edge, &'a Band)>, thinnest: bool| -> Option<Vec<(Edge, &'a Band)>> {
        let key = |b: &Band| mean_of(b.level);
        let lead = if thinnest {
            set.iter().max_by(|a, b| key(a.1).total_cmp(&key(b.1)))?
        } else {
            set.iter().min_by(|a, b| key(a.1).total_cmp(&key(b.1)))?
        };
        let level = lead.1.level;
        Some(set.iter().filter(|(_, b)| max_diff(b.level, level) <= SIDES_AGREE).copied().collect())
    };
    let long_set: Vec<_> = chosen.iter().filter(|(e, _)| long(*e)).copied().collect();
    let short_set: Vec<_> = chosen.iter().filter(|(e, _)| !long(*e)).copied().collect();
    let from_long = pick(long_set, true);
    let from_short = pick(short_set.clone(), false);
    match (from_long, from_short) {
        (Some(l), Some(s)) => {
            // Both ends sharing a thinner level than the long edges show:
            // a flat stretch of picture passed for rebate.
            let shared = s.len() == 2 && max_diff(s[0].1.level, s[1].1.level) <= SIDES_AGREE;
            let thinner = mean_of(s[0].1.level) - mean_of(l[0].1.level) > SIDES_AGREE;
            Some(if shared && thinner { s } else { l })
        }
        (Some(l), None) => Some(l),
        (None, s) => s,
    }
}

/// For each place, the run nearest `level` within [`LEVEL_TOL`].
fn places_near(pool: &[Run], level: [f64; 3]) -> Vec<&Run> {
    let mut best: Vec<&Run> = Vec::new();
    for run in pool.iter().filter(|r| max_diff(r.level, level) <= LEVEL_TOL) {
        match best.iter_mut().find(|b| b.place == run.place) {
            Some(b) if max_diff(b.level, level) <= max_diff(run.level, level) => {}
            Some(b) => *b = run,
            None => best.push(run),
        }
    }
    best
}

fn median_level(runs: &[&impl Leveled]) -> [f64; 3] {
    [0, 1, 2].map(|c| {
        let mut v: Vec<f64> = runs.iter().map(|r| r.level()[c]).collect();
        median(&mut v)
    })
}

trait Leveled {
    fn level(&self) -> [f64; 3];
}
impl Leveled for Run {
    fn level(&self) -> [f64; 3] {
        self.level
    }
}
impl Leveled for [f64; 3] {
    fn level(&self) -> [f64; 3] {
        *self
    }
}

/// The line through two of the `(place, along, depth)` points, no steeper
/// than [`MAX_SLOPE`], that passes within [`EDGE_TOL`] of a point at the
/// most places.
fn consensus_line(points: &[(usize, f64, f64)]) -> Option<(f64, f64)> {
    let mut best: Option<(usize, f64, (f64, f64))> = None;
    let mut seen = [false; PLACES];
    for (i, a) in points.iter().enumerate() {
        for b in &points[i + 1..] {
            if b.0 == a.0 {
                continue;
            }
            let slope = (b.2 - a.2) / (b.1 - a.1);
            if !(slope.abs() <= MAX_SLOPE) {
                continue;
            }
            let intercept = a.2 - slope * a.1;
            seen.iter_mut().for_each(|s| *s = false);
            let mut spread = 0.0;
            for p in points {
                let off = (p.2 - (slope * p.1 + intercept)).abs();
                if off <= EDGE_TOL && !seen[p.0] {
                    seen[p.0] = true;
                    spread += off;
                }
            }
            let count = seen.iter().filter(|s| **s).count();
            let better = match best {
                None => true,
                Some((n, s, _)) => count > n || (count == n && spread < s),
            };
            if better {
                best = Some((count, spread, (slope, intercept)));
            }
        }
    }
    best.map(|(_, _, line)| line)
}

fn least_squares(points: &[(f64, f64)]) -> (f64, f64) {
    let n = points.len() as f64;
    let (mx, my) = (points.iter().map(|p| p.0).sum::<f64>() / n, points.iter().map(|p| p.1).sum::<f64>() / n);
    let sxx: f64 = points.iter().map(|p| (p.0 - mx).powi(2)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    let slope = if sxx > 0.0 { sxy / sxx } else { 0.0 };
    (slope, my - slope * mx)
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    let mid = values.len() / 2;
    let (_, m, _) = values.select_nth_unstable_by(mid, f64::total_cmp);
    *m
}

fn mean_of(v: [f64; 3]) -> f64 {
    (v[0] + v[1] + v[2]) / 3.0
}

fn mean3(set: &[[f64; 3]]) -> [f64; 3] {
    let n = set.len().max(1) as f64;
    [0, 1, 2].map(|c| set.iter().map(|p| p[c]).sum::<f64>() / n)
}

fn max_diff(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|c| (a[c] - b[c]).abs()).fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use spektrafilm_math::precision::from_f64;

    /// The clear film's linear RGB in the synthetic scans: an orange base.
    const BASE: [f64; 3] = [0.6, 0.25, 0.11];
    /// Bare light: a sprocket hole, or past the film.
    const BARE: [f64; 3] = [0.95, 0.9, 0.85];
    /// The holder round the film.
    const HOLDER: [f64; 3] = [0.003, 0.003, 0.003];

    fn hash(x: u32, y: u32, c: u32) -> f64 {
        let mut h = x.wrapping_mul(374_761_393) ^ y.wrapping_mul(668_265_263) ^ c.wrapping_mul(2_147_483_647);
        h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
        (h ^ (h >> 16)) as f64 / u32::MAX as f64
    }

    /// The base under a varied picture, at least 0.1 denser than the base.
    fn picture(x: f64, y: f64) -> [f64; 3] {
        let (xi, yi) = (x as u32, y as u32);
        [0, 1, 2].map(|c| {
            let d = 0.35 + 0.25 * (x * 0.071 + c as f64).sin() * (y * 0.053 - c as f64).cos() + 0.15 * hash(xi, yi, c as u32);
            BASE[c] * 10f64.powf(-d)
        })
    }

    fn scan(w: u32, h: u32, f: impl Fn(f64, f64) -> [f64; 3]) -> ImageBuf {
        let mut image = ImageBuf::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let rgb = f(x as f64 + 0.5, y as f64 + 0.5);
                let i = ((y * w + x) * 3) as usize;
                for c in 0..3 {
                    image.data[i + c] = from_f64(rgb[c]);
                }
            }
        }
        image
    }

    fn close(a: [f64; 3], b: [f64; 3], tol: f64) -> bool {
        (0..3).all(|c| (a[c] / b[c] - 1.0).abs() < tol)
    }

    #[test]
    fn clear_film_on_one_edge() {
        // The gap between frames down the left, the picture running off
        // the other three edges.
        let found = find(&scan(900, 600, |x, y| if x < 120.0 { BASE } else { picture(x, y) }));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Left]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
        let p = found.picture.expect("picture");
        assert_eq!(p.edges, vec![Edge::Left]);
        assert!((p.x0 - (120.0 / 900.0 + INSET)).abs() < 0.002, "{p:?}");
        for (got, want) in [(p.y0, INSET), (p.x1, 1.0 - INSET), (p.y1, 1.0 - INSET)] {
            assert!((got - want).abs() < 1e-9, "{p:?}");
        }
        assert_eq!(p.angle_deg, 0.0);
    }

    #[test]
    fn rebate_on_both_long_edges() {
        let found = find(&scan(900, 600, |x, y| if !(70.0..530.0).contains(&y) { BASE } else { picture(x, y) }));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Top, Edge::Bottom]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
        let p = found.picture.expect("picture");
        assert!((p.y0 - (70.0 / 600.0 + INSET)).abs() < 0.002 && (p.y1 - (530.0 / 600.0 - INSET)).abs() < 0.002, "{p:?}");
        assert!((p.x0 - INSET).abs() < 1e-9 && (p.x1 - (1.0 - INSET)).abs() < 1e-9 && p.angle_deg == 0.0, "{p:?}");
    }

    /// 35 mm film in a holder: the holder's dark surround, the film's edge,
    /// sprocket holes in the rebate, then the gate's edge.
    fn strip(x: f64, y: f64) -> [f64; 3] {
        let from_edge = y.min(600.0 - y);
        if from_edge < 20.0 {
            HOLDER
        } else if from_edge < 95.0 {
            let hole = (35.0..65.0).contains(&from_edge) && (x % 119.0) < 50.0;
            if hole { BARE } else { BASE }
        } else {
            picture(x, y)
        }
    }

    #[test]
    fn sprocket_holes_are_not_the_base() {
        let found = find(&scan(900, 600, strip));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Top, Edge::Bottom]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
        let p = found.picture.expect("picture");
        assert!((p.y0 - (95.0 / 600.0 + INSET)).abs() < 0.002 && (p.y1 - (505.0 / 600.0 - INSET)).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn bare_light_past_the_film_is_not_the_base() {
        // The end of the roll: bare light past the film on the right, then
        // the film's clear end, then the picture.
        let found = find(&scan(900, 600, |x, y| {
            if x > 860.0 {
                BARE
            } else if x > 760.0 {
                BASE
            } else {
                picture(x, y)
            }
        }));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Right]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
        let p = found.picture.expect("picture");
        assert!((p.x1 - (760.0 / 900.0 - INSET)).abs() < 0.002, "{p:?}");
    }

    #[test]
    fn no_clear_film_finds_nothing() {
        let found = find(&scan(900, 600, picture));
        assert_eq!(found, FilmEdges::default());
    }

    /// A bright sky in the scene: dense, even film along the top, down to a
    /// straight horizon over thinner ground.
    fn sky(x: f64, y: f64) -> [f64; 3] {
        if y < 200.0 {
            let d = 0.9 + 0.0004 * y;
            BASE.map(|v| v * 10f64.powf(-d))
        } else {
            let ground = picture(x, y);
            [0, 1, 2].map(|c| ground[c] * 1.6)
        }
    }

    #[test]
    fn a_bright_sky_at_the_edge_is_not_clear_film() {
        assert_eq!(find(&scan(900, 600, sky)), FilmEdges::default());
        // With a rebate along the bottom, that is the base.
        let found = find(&scan(900, 600, |x, y| if y > 540.0 { BASE } else { sky(x, y) }));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Bottom]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
        assert_eq!(found.picture.expect("picture").edges, vec![Edge::Bottom]);

        // The worst case: the sky ends on a straight horizon over denser
        // snow, an even band ending in a straight step up in density, as
        // clear film does. Darker trees in the middle are thinner film.
        let snow = |x: f64, y: f64| {
            // Within the depth a band is looked for.
            if y < 120.0 {
                BASE.map(|v| v * 10f64.powf(-0.9))
            } else if (300.0..600.0).contains(&x) && (250.0..420.0).contains(&y) {
                picture(x, y)
            } else {
                BASE.map(|v| v * 10f64.powf(-(1.25 + 0.1 * hash(x as u32, y as u32, 0))))
            }
        };
        assert_eq!(find(&scan(900, 600, snow)), FilmEdges::default());
    }

    #[test]
    fn thin_shadows_at_the_edge_are_not_clear_film() {
        // The scene's dark foreground runs off the bottom: film nearly as
        // thin as the base, but uneven.
        let shadows = |x: f64, y: f64| {
            if y > 420.0 {
                let (xi, yi) = (x as u32, y as u32);
                [0, 1, 2].map(|c| BASE[c] * 10f64.powf(-(0.02 + 0.08 * hash(xi / 3, yi / 3, c as u32))))
            } else {
                picture(x, y)
            }
        };
        assert_eq!(find(&scan(900, 600, shadows)).clear_film, None);
        let found = find(&scan(900, 600, |x, y| if x < 100.0 { BASE } else { shadows(x, y) }));
        let clear = found.clear_film.expect("clear film");
        assert_eq!(clear.edges, vec![Edge::Left]);
        assert!(close(clear.rgb, BASE, 0.005), "{:?}", clear.rgb);
    }

    #[test]
    fn a_tilted_frame_is_levelled() {
        // The strip of film turned 1 degree clockwise in the holder.
        let tilt = 1f64.to_radians();
        let (sin, cos) = tilt.sin_cos();
        let (cx, cy) = (450.0, 300.0);
        let found = find(&scan(900, 600, |x, y| {
            // Back to the film's own square coordinates.
            let (dx, dy) = (x - cx, y - cy);
            let (fx, fy) = (cx + dx * cos + dy * sin, cy - dx * sin + dy * cos);
            if !(0.0..600.0).contains(&fy) { HOLDER } else { strip(fx, fy) }
        }));
        let clear = found.clear_film.expect("clear film");
        assert!(close(clear.rgb, BASE, 0.01), "{:?}", clear.rgb);
        let p = found.picture.expect("picture");
        assert!((p.angle_deg + 1.0).abs() < 0.05, "{p:?}");
        // Turned back, the gate's edges are where they were on the film.
        assert!((p.y0 - (95.0 / 600.0 + INSET)).abs() < 0.003 && (p.y1 - (505.0 / 600.0 - INSET)).abs() < 0.003, "{p:?}");
        // The frame's own edges come in to stay on the turned frame.
        assert!(p.x0 > 0.0 && p.x1 < 1.0, "{p:?}");
    }

    /// Whether the levelled rectangle lies on the `w` × `h` frame turned by
    /// `a` radians, and within it, as the crop needs.
    fn on_frame([x0, x1, y0, y1]: [f64; 4], a: f64, w: f64, h: f64) -> bool {
        let (sin, cos) = a.sin_cos();
        let (cx, cy) = (w / 2.0, h / 2.0);
        [(x0, y0), (x1, y0), (x0, y1), (x1, y1)].iter().all(|&(x, y)| {
            let (sx, sy) = (cx + (x - cx) * cos + (y - cy) * sin, cy - (x - cx) * sin + (y - cy) * cos);
            let tol = 1e-6;
            (-tol..=w + tol).contains(&sx) && (-tol..=h + tol).contains(&sy) && (-tol..=w + tol).contains(&x) && (-tol..=h + tol).contains(&y)
        })
    }

    #[test]
    fn a_levelled_picture_stays_inside_the_frame() {
        let (w, h) = (900.0, 600.0);
        let a = 3f64.to_radians();
        // Film edges found all round, close to the frame's edges: levelled,
        // the corners reach past the scan, one side past the frame itself.
        let found = [Some(-2.0), Some(896.0), Some(6.0), Some(594.0)];
        let fitted = on_the_frame(found, a, w, h);
        assert!(on_frame(fitted, a, w, h), "{fitted:?}");
        // Brought in only as far as it must: still most of the picture.
        let [x0, x1, y0, y1] = fitted;
        assert!(x0 >= 0.0 && x1 - x0 > 0.85 * w && y1 - y0 > 0.8 * h, "{fitted:?}");
        // Turned the other way too, and with the frame's own edges.
        for (rect, a) in [(found, -a), ([None, Some(880.0), Some(4.0), None], a), ([None; 4], -a)] {
            let fitted = on_the_frame(rect, a, w, h);
            assert!(on_frame(fitted, a, w, h), "{rect:?} {fitted:?}");
        }
        // Found edges well inside are left as they are.
        let inside = [Some(120.0), Some(780.0), Some(90.0), Some(510.0)];
        assert_eq!(on_the_frame(inside, a, w, h), [120.0, 780.0, 90.0, 510.0]);
    }

    #[test]
    fn a_levelled_frame_with_a_rebate_near_the_edge_crops_inside_it() {
        // A strip of film turned 3 degrees, its rebates and the gaps between
        // frames all round the picture, so close to the scan's edges that the
        // gate's corners are off it.
        let tilt = 3f64.to_radians();
        let (sin, cos) = tilt.sin_cos();
        let (cx, cy) = (450.0, 300.0);
        let found = find(&scan(900, 600, |x, y| {
            let (dx, dy) = (x - cx, y - cy);
            let (fx, fy) = (cx + dx * cos + dy * sin, cy - dx * sin + dy * cos);
            if !(-60.0..960.0).contains(&fx) || !(-10.0..610.0).contains(&fy) {
                HOLDER
            } else if !(2.0..898.0).contains(&fx) || !(45.0..555.0).contains(&fy) {
                BASE
            } else {
                picture(fx, fy)
            }
        }));
        let p = found.picture.expect("picture");
        let rect = [p.x0 * 900.0, p.x1 * 900.0, p.y0 * 600.0, p.y1 * 600.0];
        assert!(on_frame(rect, p.angle_deg.to_radians(), 900.0, 600.0), "{p:?}");
        assert!((0.0..p.x1).contains(&p.x0) && p.x1 <= 1.0 && (0.0..p.y1).contains(&p.y0) && p.y1 <= 1.0, "{p:?}");
    }

    #[test]
    fn the_answer_does_not_depend_on_the_scan_size() {
        let small = find(&scan(450, 300, |x, y| if x < 60.0 { BASE } else { picture(x * 2.0, y * 2.0) }));
        let large = find(&scan(1800, 1200, |x, y| if x < 240.0 { BASE } else { picture(x / 2.0, y / 2.0) }));
        let (s, l) = (small.picture.unwrap(), large.picture.unwrap());
        assert!((s.x0 - l.x0).abs() < 0.002, "{s:?} {l:?}");
        assert!(close(small.clear_film.unwrap().rgb, large.clear_film.unwrap().rgb, 0.005));
    }
}
