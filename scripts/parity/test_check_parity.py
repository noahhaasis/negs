"""Checks that the parity gate detects drift and invalid output."""

import unittest

import numpy as np

from check_parity import expand_bw_stage, metrics


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


if __name__ == "__main__":
    unittest.main()
