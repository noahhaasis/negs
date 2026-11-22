//! The tone graph for the print curves (ticket 20, prototype), a port of
//! `ToneGraph.swift`: the paper's real response to a ramp of greys, with the
//! photograph's film exposure behind it; dragging it sets Print exposure and
//! the print curves' gammas. Adapted from SpektraMac's Print tab "Contrast"
//! graph (GPL-3.0). The window that draws it is `ui::tone_graph`.
//!
//! The curve maths, handles, hits and drags run in the shared core
//! (core/src/rules/tone, through `negs_core_rules`); the helper's JSON and
//! the requests stay Rust there. This module keeps its API and tests.

pub use negs_core_rules::tone_graph::*;

#[cfg(test)]
use crate::json;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A synthetic curve, linear in film exposure: the paper gets 0.3 log
    /// less per stop, and the print brightens by 0.05 per stop.
    fn curve(controls: Controls) -> Curve {
        let stops: Vec<f64> = (0..=160).map(|i| -8.0 + i as f64 * 0.1).collect();
        let result = json!({
            "film": stops.iter().map(|s| [s, s, s]).collect::<Vec<_>>(),
            "paper": stops.iter().map(|s| [-0.3 * s, -0.3 * s, -0.3 * s]).collect::<Vec<_>>(),
            "rgb": stops.iter().map(|s| { let v = 0.5 + 0.05 * s; [v, v, v] }).collect::<Vec<_>>(),
        });
        Curve::parse(&result, request("film", "paper", &json!({})), controls).unwrap()
    }

    const GEOMETRY: Geometry = Geometry { width: 300.0, height: 170.0, inset: 8.0 };

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn indexes_interpolate_and_clamp_to_the_ends() {
        let falling = [3.0, 2.0, 1.0, 0.0];
        assert!(close(index_of_decreasing(&falling, 1.5), 1.5));
        assert_eq!(index_of_decreasing(&falling, 9.0), 0.0);
        assert_eq!(index_of_decreasing(&falling, -9.0), 3.0);
        assert!(close(index_of_increasing(&[0.0, 1.0, 2.0], 0.25), 0.25));
        assert!(close(interpolate(&[0.0, 10.0, 20.0], 1.5), 15.0));
        assert_eq!(interpolate(&[0.0, 10.0], 7.0), 10.0);
        assert_eq!(interpolate(&[], 1.0), 0.0);
    }

    #[test]
    fn controls_are_one_while_the_print_curves_are_off() {
        let params = json!({"enlarger": {"print_exposure": 1.5},
            "print_render": {"density_curves_morph": {"active": false, "gamma_factor": 1.3, "gamma_factor_fast": 0.8, "gamma_factor_slow": 0.0}}});
        let off = Controls::read(|p| json::get(&params, p));
        assert_eq!(off, Controls { print_exposure: 1.5, ..Controls::default() });
        let mut on = params.clone();
        json::set(&mut on, ACTIVE, json!(true));
        let read = Controls::read(|p| json::get(&on, p));
        // A gamma of 0 isn't one the engine uses: 1.
        assert_eq!(read, Controls { print_exposure: 1.5, gamma: 1.3, fast: 0.8, slow: 1.0 });
    }

    #[test]
    fn malformed_replies_are_refused() {
        let controls = Controls::default();
        let r = json!({});
        assert!(Curve::parse(&json!({"film": [[0, 0, 0]], "rgb": [[0, 0, 0]]}), r.clone(), controls).is_none());
        assert!(Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0]]}), r.clone(), controls).is_none());
        assert!(Curve::parse(&json!({"film": [[0, 0], [1, 1]], "rgb": [[0, 0], [1, 1]]}), r.clone(), controls).is_none());
        // Scanning the film directly: no paper, the curve alone.
        let scan = Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "paper": null, "rgb": [[0, 0, 0], [1, 1, 1]]}), r, controls).unwrap();
        assert!(scan.paper.is_none());
        assert!(scan.predicted(Controls { print_exposure: 2.0, ..controls }).handles(None).is_none());
        assert!(FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4]]})).is_none());
        assert!(FilmHistogram::parse(&json!({"stops": [8, -8], "counts": [[1, 2], [3, 4], [5, 6]]})).is_none());
        let histogram = FilmHistogram::parse(&json!({"stops": [-8, 8], "counts": [[1, 2], [3, 4], [5, 6]]})).unwrap();
        assert_eq!(histogram.bins(None), vec![3.0, 4.0]);
        assert_eq!(histogram.bins(Some(2)), vec![5.0, 6.0]);
    }

    #[test]
    fn a_curve_stands_in_only_for_the_same_settings() {
        let params = json!({"enlarger": {"print_exposure": 1, "y_filter_shift": 0}});
        let base = Curve::parse(&json!({"film": [[0, 0, 0], [1, 1, 1]], "rgb": [[0, 0, 0], [1, 1, 1]]}), request("f", "p", &params), Controls::default()).unwrap();
        let mut moved = params.clone();
        json::set(&mut moved, PRINT_EXPOSURE, json!(2.5));
        json::set(&mut moved, GAMMA, json!(1.2));
        assert!(base.matches(&request("f", "p", &moved)));
        json::set(&mut moved, "enlarger.y_filter_shift", json!(3));
        assert!(!base.matches(&request("f", "p", &moved)));
        assert!(!base.matches(&request("f", "other", &params)));
    }

    #[test]
    fn prediction_is_exact_for_print_exposure_and_gamma() {
        let base = curve(Controls::default());
        let same = base.predicted(Controls::default());
        assert_eq!(same.rgb, base.rgb);
        // Twice the light on the paper: each grey prints like one log10(2)
        // higher on the paper exposure axis, i.e. log10(2)/0.3 stops darker.
        let brighter = base.predicted(Controls { print_exposure: 2.0, ..Controls::default() });
        let shift = 2f64.log10() / 0.3;
        for i in [40, 80, 120] {
            let expected = 0.5 + 0.05 * (base.stops[i] - shift);
            assert!((brighter.rgb[1][i] - expected).abs() < 1e-9, "{i}: {} vs {expected}", brighter.rgb[1][i]);
        }
        let handles = brighter.handles(None).unwrap();
        assert!((handles.1 - shift).abs() < 1e-9);
        // Gamma 1.25 about mid-grey: the paper exposure scales by it.
        let steeper = base.predicted(Controls { gamma: 1.25, ..Controls::default() });
        for i in [50, 110] {
            assert!((steeper.rgb[0][i] - (0.5 + 0.05 * 1.25 * base.stops[i])).abs() < 1e-9);
        }
        // Slow gamma changes only the shadows' side, above paper exposure 0.
        let slow = base.predicted(Controls { slow: 1.5, ..Controls::default() });
        assert!((slow.rgb[0][120] - base.rgb[0][120]).abs() < 1e-12);
        assert!(slow.rgb[0][40] < base.rgb[0][40]);
    }

    #[test]
    fn handles_sit_at_paper_exposure_zero_and_two_stops_either_side() {
        let display = curve(Controls::default()).predicted(Controls::default());
        let (shadows, middle, highlights) = display.handles(None).unwrap();
        assert!(close(shadows, -2.0) && close(middle, 0.0) && close(highlights, 2.0));
        let points = GEOMETRY.handle_points(&display, None);
        assert_eq!(points.len(), 3);
        assert!(close(points[1].1.0, 150.0));
        assert!(close(points[1].1.1, GEOMETRY.y(0.5)));
        let at = points[1].1;
        assert_eq!(GEOMETRY.hit(&display, None, (at.0 + 3.0, at.1), 10.0, 12.0), Some(Handle::Middle));
        let t = 4.0;
        let on_curve = (GEOMETRY.x(t), GEOMETRY.y(0.5 + 0.05 * t) + 5.0);
        assert!(matches!(GEOMETRY.hit(&display, None, on_curve, 10.0, 12.0), Some(Handle::Curve(s)) if (s - t).abs() < 1e-9));
        assert_eq!(GEOMETRY.hit(&display, None, (on_curve.0, on_curve.1 + 30.0), 10.0, 12.0), None);
    }

    #[test]
    fn the_middle_handle_puts_mid_grey_under_the_pointer() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        let drag = Drag::start(Handle::Middle, &base, &display, None).unwrap();
        // One stop right: paper −0.3 there, so 10^0.3 = 1.995 → 2.00.
        let (path, value) = drag.value(base.controls, None, &GEOMETRY, (GEOMETRY.x(1.0), 0.0)).unwrap();
        assert_eq!((path, value), (PRINT_EXPOSURE, 2.0));
        // Far left: clamped to the slider's minimum.
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (0.0, 0.0)).unwrap().1, EXPOSURE_RANGE.0);
    }

    #[test]
    fn contrast_drags_scale_the_paper_exposure_at_their_tone() {
        let base = curve(Controls::default());
        let display = base.predicted(base.controls);
        // Dragging the curve at +2 stops (paper −0.6) to the brightness the
        // base curve has at +2.4 stops (paper −0.72): gamma 1.2.
        let drag = Drag::start(Handle::Curve(2.0), &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, point), Some((GAMMA, 1.2)));
        // The shadows' handle at −2 stops, moved to the base's −2.5: slow 1.25.
        let drag = Drag::start(Handle::Shadows, &base, &display, None).unwrap();
        assert!(close(drag.tone, -2.0));
        let point = (GEOMETRY.x(-2.0), GEOMETRY.y(0.5 - 0.05 * 2.5));
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, point), Some((SLOW, 1.25)));
        // A drag at mid-grey can't scale anything.
        let drag = Drag::start(Handle::Curve(0.0), &base, &display, None).unwrap();
        assert_eq!(drag.value(base.controls, None, &GEOMETRY, (150.0, 20.0)), None);
        // Solved on the drag's own curve: a gamma already raised mid-drag
        // is divided out of the next slow value.
        let drag = Drag::start(Handle::Highlights, &base, &display, None).unwrap();
        let point = (GEOMETRY.x(2.0), GEOMETRY.y(0.5 + 0.05 * 2.4));
        let raised = Controls { gamma: 1.2, ..base.controls };
        assert_eq!(drag.value(raised, None, &GEOMETRY, point), Some((FAST, 1.0)));
    }
}
