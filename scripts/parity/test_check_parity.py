"""Checks that the parity gate detects drift and invalid output, and that
parameters reach both upstream APIs under the right names."""

from dataclasses import dataclass, field
from pathlib import Path
import sys
import unittest

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import (  # noqa: E402
    CASES, EXPERIMENTAL, MAIN, apply_params, expand_bw_stage, make_fixtures, metrics,
    parse_args, python_overrides, reference_api, shared_params, summarize, summary_markdown,
)


class ComparisonTests(unittest.TestCase):
    def test_bw_expansion_checks_all_rust_lanes(self):
        expected = np.full((2, 2, 1), 0.25)
        actual = np.full((2, 2, 3), 0.25)
        expanded = expand_bw_stage(expected, actual.shape, True, np)
        self.assertTrue(metrics(actual, expanded, 0, np)["passed"])
        actual[0, 0, 2] += 0.01
        self.assertFalse(metrics(actual, expanded, 1e-6, np)["passed"])
        with self.assertRaises(ValueError):
            metrics(actual, expand_bw_stage(expected, actual.shape, False, np), 1e-6, np)

    def test_tolerance_is_distinct_from_byte_equality(self):
        expected = np.array([0.25], dtype=np.float64)
        rounded = np.nextafter(expected, np.inf)
        result = metrics(rounded, expected, 1e-6, np)
        self.assertTrue(result["passed"])
        self.assertFalse(result["byte_identical"])
        self.assertFalse(metrics(rounded, expected, 0, np)["passed"])
        self.assertTrue(metrics(expected.copy(), expected, 0, np)["byte_identical"])

    def test_one_bad_value_fails_even_when_mean_is_small(self):
        expected = np.zeros(1000)
        actual = expected.copy()
        actual[-1] = 1e-4
        result = metrics(actual, expected, 1e-6, np)
        self.assertLess(result["mean_abs"], 1e-6)
        self.assertFalse(result["passed"])

    def test_nonfinite_and_shape_mismatch_are_errors(self):
        for value in [np.nan, np.inf, -np.inf]:
            with self.assertRaises(ValueError):
                metrics(np.array([value]), np.array([value]), 1e-6, np)
        with self.assertRaises(ValueError):
            metrics(np.zeros((1, 3)), np.zeros(3), 1e-6, np)


# Minimal stand-ins for the two upstream schemas.
@dataclass(frozen=True)
class Chemistry:
    development_time: float | None = None
    active: bool = True
    gamma_factor: float = 1.0


@dataclass(frozen=True)
class InputGamut:
    active: bool = True
    algorithm: str = "xy"
    boundary: str = "inscribed_hull"


@dataclass
class Grain:
    active: bool = True


@dataclass
class FilmRender:
    chemistry: Chemistry = field(default_factory=Chemistry)
    grain: Grain = field(default_factory=Grain)


@dataclass
class IO:
    input_gamut_compress: InputGamut = field(default_factory=InputGamut)


@dataclass
class Workflow:
    route: str = "input > film > print > scan"


@dataclass
class ExperimentalParams:
    film_render: FilmRender = field(default_factory=FilmRender)
    io: IO = field(default_factory=IO)
    workflow: Workflow = field(default_factory=Workflow)


@dataclass
class MainFilmRender:
    density_curve_gamma: float = 1.0


@dataclass
class MainParams:
    film_render: MainFilmRender = field(default_factory=MainFilmRender)


class ParameterMappingTests(unittest.TestCase):
    def test_api_detection(self):
        self.assertEqual(reference_api(ExperimentalParams()), EXPERIMENTAL)
        self.assertEqual(reference_api(MainParams()), MAIN)

    def test_experimental_names(self):
        shared = {
            "film_render": {"development_time": 4.0, "density_curves_morph": {"active": True, "gamma_factor": 1.4}},
            "print_render": {"development_time": 2.0},
            "io": {"scan_film": True, "input_gamut_compress": {"algorithm": "off", "center": "D65"}},
        }
        python = python_overrides(shared, EXPERIMENTAL)
        self.assertEqual(python["workflow"], {"route": "input > film > scan"})
        self.assertEqual(python["film_render"],
                         {"chemistry": {"development_time": 4.0, "active": True, "gamma_factor": 1.4}})
        self.assertEqual(python["print_render"], {"chemistry": {"development_time": 2.0}})
        self.assertEqual(python["io"], {"input_gamut_compress": {"active": False}})
        # The Rust JSON keeps its own names.
        self.assertEqual(shared["io"]["scan_film"], True)
        self.assertIn("density_curves_morph", shared["film_render"])

    def test_main_names(self):
        shared = {"film_render": {"density_curves_morph": {"active": True}},
                  "print_render": {"density_curves_morph": {"active": True}},
                  "io": {"scan_film": False}}
        python = python_overrides(shared, MAIN)
        # Main has no film morph parameter (the caller bakes the curves).
        self.assertEqual(python["film_render"], {})
        self.assertEqual(python["print_render"], {"density_curves_morph": {"active": True}})
        self.assertEqual(python["io"], {"scan_film": False})
        with self.assertRaises(ValueError):
            python_overrides({"film_render": {"development_time": 4.0}}, MAIN)

    def test_frozen_groups_are_replaced(self):
        params = ExperimentalParams()
        original = params.film_render.chemistry
        result = apply_params(params, {
            "film_render": {"chemistry": {"development_time": 4.0}, "grain": {"active": False}},
            "io": {"input_gamut_compress": {"active": False}},
            "workflow": {"route": "input > film > scan"},
        })
        self.assertIs(result, params)
        self.assertEqual(params.film_render.chemistry, Chemistry(development_time=4.0))
        self.assertIsNot(params.film_render.chemistry, original)
        self.assertFalse(params.film_render.grain.active)
        self.assertEqual(params.io.input_gamut_compress, InputGamut(active=False))
        self.assertEqual(params.workflow.route, "input > film > scan")

    def test_unknown_names_are_errors(self):
        for overrides in [{"film_render": {"density_curve_gamma": 1.0}},
                          {"film_render": {"chemistry": {"gamma": 1.0}}}]:
            with self.assertRaises(AttributeError):
                apply_params(ExperimentalParams(), overrides)


class OptionTests(unittest.TestCase):
    def test_isolation_turns_effects_off_unless_asked(self):
        params = shared_params(True)
        self.assertFalse(params["camera"]["auto_exposure"])
        self.assertFalse(params["film_render"]["halation"]["active"])
        self.assertFalse(params["film_render"]["dir_couplers"]["active"])
        self.assertEqual(params["scanner"]["unsharp_mask"], [0.0, 0.0])
        params = shared_params(False, dir_couplers=True, halation=True, auto_exposure=True,
                               scanner_usm=True, lens_blur=[2.0, 1.0, 0.5])
        self.assertTrue(params["camera"]["auto_exposure"])
        self.assertTrue(params["film_render"]["halation"]["active"])
        self.assertTrue(params["film_render"]["dir_couplers"]["active"])
        self.assertEqual(params["scanner"]["unsharp_mask"], [0.7, 0.7])
        self.assertEqual((params["camera"]["lens_blur_um"], params["enlarger"]["lens_blur"],
                          params["scanner"]["lens_blur"]), (2.0, 1.0, 0.5))

    def test_defaults_only_switch_off_grain_and_glare(self):
        params = shared_params(False, defaults=True)
        self.assertEqual(set(params), {"film_render", "print_render", "io", "settings"})
        self.assertEqual(params["film_render"], {"grain": {"active": False}, "glare": {"active": False}})
        self.assertEqual(params["print_render"], {"glare": {"active": False}})

    def test_named_cases(self):
        _, args = parse_args(["--python-source", "x", "--output-dir", "y", "--case", "all"])
        self.assertEqual([r[0] for r in args.routes], list(CASES))
        self.assertEqual(args.fixtures, ["midgray", "gradient4", "ramp16", "scene"])
        _, args = parse_args(["--python-source", "x", "--output-dir", "y", "--case", "provia-scan",
                              "--fixture", "scene", "--gamut-stress"])
        self.assertEqual(args.routes, [("provia-scan", "fujifilm_provia_100f", "kodak_portra_endura", True)])
        self.assertEqual(args.fixtures, ["scene", "gamut-stress4"])
        _, args = parse_args(["--python-source", "x", "--output-dir", "y", "--film", "kodak_trix", "--scan-film"])
        self.assertEqual(args.routes, [("kodak_trix--scan", "kodak_trix", "kodak_portra_endura", True)])
        with self.assertRaises(SystemExit):
            parse_args(["--python-source", "x", "--output-dir", "y", "--case", "portra-endura",
                        "--film", "kodak_gold_200"])

    def test_scene_fixture_has_structure_and_highlight(self):
        scene = make_fixtures(["scene"], np)["scene"]
        self.assertEqual(scene.shape, (256, 384, 3))
        self.assertEqual(scene.dtype, np.float32)
        self.assertEqual(float(scene.max()), 8.0)
        self.assertGreaterEqual(float(scene.min()), 0.0)
        # Saturated colours and a 1-pixel checkerboard.
        self.assertGreater(float((scene.max(axis=2) - scene.min(axis=2)).max()), 0.5)
        self.assertGreater(float(np.abs(np.diff(scene[80, 46:70, 0])).min()), 0.4)


class SummaryTests(unittest.TestCase):
    def test_first_failing_stage_follows_pipeline_order(self):
        def stage(max_abs):
            return {"max_abs": max_abs, "passed": max_abs <= 1e-6}
        report = {"cases": [
            {"route": "r", "lut": "off", "fixture": "a",
             "stages": {"log_e_film": stage(0), "cmy_film": stage(0), "rgb_out": stage(1e-3)},
             "quantized_linear_rgb": {"max_levels": 2}},
            {"route": "r", "lut": "off", "fixture": "b",
             "stages": {"log_e_film": stage(0), "cmy_film": stage(5e-6), "rgb_out": stage(1e-7)},
             "quantized_linear_rgb": {"max_levels": 0}},
            {"route": "r", "lut": "on", "fixture": "a", "stages": {}, "error": "RuntimeError: x"},
        ]}
        summary = summarize(report)
        self.assertEqual(summary[0]["first_failing_stage"], "cmy_film")
        self.assertEqual(summary[0]["stages"]["rgb_out"], 1e-3)
        self.assertEqual(summary[0]["max_8bit"], 2)
        table = summary_markdown(summary)
        self.assertIn("| r | off | cmy_film | 0.0e+00 | 5.0e-06 | — | 1.0e-03 | 2 |", table)
        self.assertIn("error: a: RuntimeError: x", table)


if __name__ == "__main__":
    unittest.main()
