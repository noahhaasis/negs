"""Independent checks of numeric semantics and fail-closed review behavior."""
import os
from pathlib import Path
import sys
import tempfile
import unittest

os.environ.setdefault('NUMBA_CACHE_DIR', str(Path(tempfile.gettempdir()) / 'negs-accuracy-test-numba'))
os.environ.setdefault('NUMBA_NUM_THREADS', '2')
sys.path.insert(0, str(Path(__file__).resolve().parent))
import numpy as np
from metrics import lab8, delta_e, rgb_metrics, float_metrics


class AccuracyTests(unittest.TestCase):
    def test_lab_matches_colour_independently(self):
        import colour
        rgb = np.random.default_rng(42).integers(0, 256, (9, 13, 3), dtype=np.uint8)
        xyz = colour.sRGB_to_XYZ(rgb / 255.)
        expected = colour.XYZ_to_Lab(xyz).reshape(-1, 3)
        np.testing.assert_allclose(lab8(rgb), expected, rtol=0, atol=1e-10)

    def test_delta_e_matches_colour_full_random_and_published_pair(self):
        import colour
        rng = np.random.default_rng(71)
        a = rng.uniform([-10, -120, -120], [110, 120, 120], (4096, 3))
        b = rng.uniform([-10, -120, -120], [110, 120, 120], (4096, 3))
        a[0], b[0] = [50, 2.6772, -79.7751], [50, 0, -82.7485]
        actual = delta_e(a, b)
        self.assertAlmostEqual(actual[0], 2.0425, places=4)
        np.testing.assert_allclose(actual, colour.delta_E(a, b, method='CIE 2000'), rtol=0, atol=1e-10)

    def test_strict_rgb_not_channel_fraction(self):
        a = np.zeros((2, 2, 3), np.uint8)
        b = a.copy()
        b[0, 0, 0] = 1
        b[1, 1] = 2
        metric = rgb_metrics(a, b)
        self.assertFalse(metric['exact_equal'])
        self.assertEqual(metric['exact_rgb_pixel_fraction'], .5)
        self.assertEqual(metric['within_one_code_rgb_pixel_fraction'], .75)
        self.assertEqual(metric['max_abs_channel_codes'], 2)
        self.assertEqual(metric['mean_abs_channel_codes'], 7/12)
        self.assertTrue(rgb_metrics(a, a)['exact_equal'])

    def test_dimensions_never_pass_or_align(self):
        result = rgb_metrics(np.zeros((2, 3, 3), np.uint8), np.zeros((3, 2, 3), np.uint8))
        self.assertEqual(result['status'], 'dimension-mismatch')
        self.assertFalse(result['exact_equal'])

    def test_nonfinite_float_is_failure_even_matching(self):
        result = float_metrics(np.array([np.nan, np.inf]), np.array([np.nan, np.inf]))
        self.assertEqual(result['status'], 'nonfinite-failure')
        self.assertFalse(result['exact_equal'])

    def test_float_zero_handling_and_precision(self):
        result = float_metrics(np.array([0., 1e-13, -.3]), np.array([0., 0., -.30001]))
        self.assertTrue(np.isfinite(result['max_relative']))
        self.assertFalse(result['all_components_within_1e_6'])
        self.assertFalse(result['exact_equal'])


if __name__ == '__main__':
    unittest.main()
