"""Adversarial checks for the fixture generator's provenance gates."""

import copy
import unittest

from freeze_gamut_reference import require, validate_report


class ProvenanceTests(unittest.TestCase):
    def report(self):
        return {
            "passed": True, "experimental_gamut": True, "gamut_stress": True,
            "atol": 1e-6, "rtol": 0, "python": {"tree_sha256": "pinned"},
            "cases": [{"id": f"{name}-lut-{mode}", "passed": True,
                       "stages": {s: {"passed": True, "max_abs": 1e-12} for s in
                                  ["log_e_film", "cmy_film", "cmy_print", "rgb_out"]}}
                      for name in ["midgray", "gradient4", "ramp16", "gamut-stress4"] for mode in ["off", "on"]],
        }

    def test_forged_pass_flag_does_not_override_failed_or_missing_cases(self):
        report = self.report()
        validate_report(report, "pinned")
        for mutation in [lambda r: r["cases"].clear(),
                         lambda r: r["cases"][0].update(passed=False),
                         lambda r: r["cases"][0]["stages"].clear(),
                         lambda r: r["cases"][0]["stages"].pop("log_e_film"),
                         lambda r: r.update(atol=0.01),
                         lambda r: r["python"].update(tree_sha256="modified")]:
            changed = copy.deepcopy(report)
            mutation(changed)
            with self.assertRaises(ValueError):
                validate_report(changed, "pinned")

    def test_nonfinite_or_excessive_error_is_rejected_even_if_marked_passed(self):
        for value in [float("nan"), float("inf"), -1, 1e-3]:
            report = self.report()
            report["cases"][0]["stages"]["rgb_out"]["max_abs"] = value
            with self.assertRaises(ValueError):
                validate_report(report, "pinned")

    def test_gate_does_not_depend_on_python_assertions(self):
        with self.assertRaises(ValueError):
            require(False, "must fail even under python -O")


if __name__ == "__main__":
    unittest.main()
