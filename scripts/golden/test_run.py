"""Tests for run.py's comparison logic: it must never pass by accident.

    python3 -I -m unittest discover -s scripts/golden -p 'test_*.py'
"""

import copy
import io
import json
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parent))
import run  # noqa: E402

RESULT = {"f64": "a" * 64, "tiff": "b" * 64, "size": [4, 2]}
MANIFEST = {
    "meta": {"data_dir": "d"},
    "inputs": {"synth_ramp": "c" * 64},
    "presets": {"default": {}},
    "cases": {"case1": {"layout": None, "grain": False, "engines": {"f64": RESULT}}},
}


def compare(a, b, **kw):
    """Exit code of `run.py compare --exact` on two in-memory manifests."""
    with tempfile.TemporaryDirectory() as tmp:
        pa, pb = Path(tmp) / "a.json", Path(tmp) / "b.json"
        pa.write_text(json.dumps(a))
        pb.write_text(json.dumps(b))
        args = SimpleNamespace(a=str(pa), b=str(pb), dumps_a=None, dumps_b=None, exact=True,
                               engines=None,
                               allow_input_mismatch=False, report=None, jobs=1,
                               bin_dir=str(run.ROOT / "target" / "release"))
        for k, v in kw.items():
            setattr(args, k, v)
        try:
            with redirect_stdout(io.StringIO()):
                run.cmd_compare(args)
        except SystemExit as e:
            return e.code
    return 0


class CompareTest(unittest.TestCase):
    def test_identical_passes(self):
        self.assertEqual(compare(MANIFEST, MANIFEST), 0)

    def test_tiff_difference_fails_even_with_equal_raw(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"]["tiff"] = "e" * 64
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_raw_difference_fails_in_exact_mode(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"]["f64"] = "e" * 64
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_input_mismatch_stops(self):
        b = copy.deepcopy(MANIFEST)
        b["inputs"]["synth_ramp"] = "f" * 64
        self.assertNotEqual(compare(MANIFEST, b), 0)

    def test_extra_case_in_either_side_fails(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case2"] = copy.deepcopy(b["cases"]["case1"])
        self.assertEqual(compare(MANIFEST, b), 1)
        self.assertEqual(compare(b, MANIFEST), 1)

    def test_extra_engine_fails(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["gpu"] = dict(RESULT)
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_render_error_fails(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"] = {"error": "exit 1"}
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_size_difference_fails(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"]["size"] = [2, 4]
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_preset_difference_fails(self):
        b = copy.deepcopy(MANIFEST)
        b["presets"]["default"] = {"film_render": {}}
        self.assertEqual(compare(MANIFEST, b), 1)

    def test_tolerance_mode_needs_dumps(self):
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"]["f64"] = "e" * 64
        self.assertNotEqual(compare(MANIFEST, b, exact=False), 0)

    def test_dump_not_matching_manifest_fails(self):
        # Equal dumps on disk must not pass when they are not the dumps the
        # manifests describe (stale directory, damaged copy).
        b = copy.deepcopy(MANIFEST)
        b["cases"]["case1"]["engines"]["f64"]["f64"] = "e" * 64
        with tempfile.TemporaryDirectory() as tmp:
            for side in ("a", "b"):
                (Path(tmp) / side).mkdir()
                (Path(tmp) / side / "case1.f64.f64").write_bytes(bytes(4 * 2 * 3 * 8))
            self.assertEqual(compare(MANIFEST, b, exact=False, dumps_a=str(Path(tmp) / "a"),
                                     dumps_b=str(Path(tmp) / "b")), 1)


class MatrixTest(unittest.TestCase):
    def test_names_unique_and_engines_known(self):
        cases = run.matrix()
        self.assertEqual(len(cases), len({c["name"] for c in cases}))
        for c in cases:
            self.assertTrue(c["engines"])
            self.assertTrue(set(c["engines"]) <= set(run.ENGINES))

    def test_every_preset_exists(self):
        for name in run.PRESET_NAMES:
            run.preset(name)

    def test_grain_flag(self):
        self.assertTrue(run.grain_active(run.effective_params("default")))
        self.assertFalse(run.grain_active(run.effective_params("det")))


@unittest.skipUnless((run.ROOT / "target" / "release" / run.exe("golden_compare")).is_file(),
                     "build golden_compare first")
class GoldenCompareTest(unittest.TestCase):
    """The comparison binary itself: gates fail when they should."""

    def check(self, a, b, profile="f64", w=2, h=1):
        import struct
        with tempfile.TemporaryDirectory() as tmp:
            pa, pb = Path(tmp) / "a.f64", Path(tmp) / "b.f64"
            pa.write_bytes(struct.pack(f"<{len(a)}d", *a))
            pb.write_bytes(struct.pack(f"<{len(b)}d", *b))
            return run.golden_compare(run.ROOT / "target" / "release", profile, "f64", pa, pb, w, h)

    def test_identical_pass(self):
        v = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        self.assertTrue(self.check(v, v)["pass"])

    def test_last_bit_passes_f64_gate(self):
        v = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        u = v[:-1] + [0.6000000000000001]
        self.assertTrue(self.check(v, u)["pass"])

    def test_small_real_difference_fails_f64_gate(self):
        v = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        u = v[:-1] + [0.6 + 1e-9]
        self.assertFalse(self.check(v, u)["pass"])

    def test_nan_fails(self):
        v = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        self.assertFalse(self.check(v, v[:-1] + [float("nan")])["pass"])

    def test_wrong_size_fails(self):
        v = [0.1, 0.2, 0.3, 0.4, 0.5, 0.6]
        self.assertFalse(self.check(v, v, w=3)["pass"])

    def test_visible_difference_fails_gpu_gate(self):
        v = [0.5] * 6
        self.assertFalse(self.check(v, [0.5, 0.5, 0.5, 0.6, 0.5, 0.5], profile="gpu")["pass"])


class ArchTest(unittest.TestCase):
    def test_this_python(self):
        self.assertIn(run.binary_arch(sys.executable), ("arm64", "x86_64", "universal"))


if __name__ == "__main__":
    unittest.main()
