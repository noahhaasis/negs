"""Fail-closed regression tests for the WASM/native golden bridge."""
import argparse
import contextlib
import importlib.util
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import numpy as np

spec = importlib.util.spec_from_file_location('wasm_golden', Path(__file__).with_name('golden.py'))
g = importlib.util.module_from_spec(spec)
spec.loader.exec_module(g)


@unittest.skipUnless((g.ROOT / 'target/release/golden_compare').exists(), 'build golden_compare first')
class ComparisonTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.out, self.results = self.root / 'out', self.root / 'results'
        (self.out / 'reference').mkdir(parents=True)
        self.results.mkdir()
        self.patches = [patch.object(g, 'OUT', self.out), patch.object(g, 'RESULTS', self.results)]
        for p in self.patches:
            p.start()
            self.addCleanup(p.stop)
        source = self.root / 'source.tif'
        source.write_bytes(b'fixed input')
        self.case = dict(name='case', input=str(source), film='film', paper='paper',
                         params=g.golden.effective_params('det'), random=False)
        g.seal_cases([self.case])
        self.cases = self.results / 'cases.json'
        self.args = argparse.Namespace(cases=self.cases, only=None, prefix='test', backend='gpu')
        self.values = np.full((1, 2, 3), .25, '<f4')
        for engine in ('gpu', 'f64', 'f32cpu', 'exactgpu'):
            self.values.astype('<f8').tofile(self.out / 'reference' / f'case.{engine}.f64')
        self.values.tofile(self.results / 'test-case.f32')
        self.results.joinpath('test-case.ppm').write_bytes(b'P6\n2 1\n255\n' + bytes([64]*6))
        self.run = {'info': {'backend': 'wgpu (GPU)', 'budget': 0}, 'rows': [dict(name='case',
                    width=2, height=1, backend='wgpu (GPU)', case_sha256=self.case['case_sha256'])]}

    def compare(self):
        self.cases.write_text(json.dumps([self.case]))
        (self.results / 'test-run.json').write_text(json.dumps(self.run))
        with contextlib.redirect_stdout(io.StringIO()):
            failed = g.compare(self.args)
        return failed, json.loads((self.out / 'test-report.json').read_text())

    def test_identical_passes_original_gates(self):
        fail, report = self.compare()
        self.assertFalse(fail)
        self.assertEqual([x['profile'] for x in report['cases'][0]['checks']], ['gpu', 'gpu_vs_ref'])

    def test_grain_off_does_not_receive_a_statistical_gate(self):
        self.case['random'] = True  # a bad label must not relax the actual params
        fail, report = self.compare()
        self.assertFalse(fail)
        self.assertEqual([x['profile'] for x in report['cases'][0]['checks']], ['gpu', 'gpu_vs_ref'])

    def test_missing_case_fails(self):
        self.run['rows'] = []
        self.assertTrue(self.compare()[0])

    def test_missing_scan_fails(self):
        (self.results / 'test-case.f32').unlink()
        self.assertTrue(self.compare()[0])

    def test_nonfinite_fails(self):
        self.values[0, 0, 0] = np.nan
        self.values.tofile(self.results / 'test-case.f32')
        self.assertTrue(self.compare()[0])

    def test_wrong_size_fails(self):
        self.values[:, :1].tofile(self.results / 'test-case.f32')
        self.assertTrue(self.compare()[0])

    def test_cpu_fallback_fails(self):
        self.run['rows'][0]['backend'] = 'CPU (rayon)'
        self.assertTrue(self.compare()[0])

    def test_changed_parameters_fail(self):
        self.case['params']['post'] = {'brightness': 1}
        g.seal_cases([self.case])
        self.assertTrue(self.compare()[0])

    def test_changed_input_fails(self):
        Path(self.case['input']).write_bytes(b'changed')
        self.assertTrue(self.compare()[0])

    def test_outside_numeric_gate_fails(self):
        (self.values + .1).tofile(self.results / 'test-case.f32')
        self.assertTrue(self.compare()[0])

    def test_display_path_error_fails(self):
        self.results.joinpath('test-case.ppm').write_bytes(b'P6\n2 1\n255\n' + bytes([66]*6))
        self.assertTrue(self.compare()[0])

    def test_diffusion_exception_cannot_cover_other_effects(self):
        self.case.update(approximate=True, budget=160*1024*1024)
        g.seal_cases([self.case])
        self.run['rows'][0].update(case_sha256=self.case['case_sha256'], budget=self.case['budget'])
        self.assertTrue(self.compare()[0])

    def test_prepare_does_not_alias_stock_scan_flags(self):
        inputs = self.root / 'inputs'
        inputs.mkdir()
        hashes = {}
        for name in g.golden.PHOTOS + g.golden.SYNTH:
            f = inputs / f'{name}.tif'
            f.write_bytes(name.encode())
            hashes[name] = g.sha(f)
        manifest = self.root / 'manifest.json'
        manifest.write_text(json.dumps({'inputs': hashes}))
        with contextlib.redirect_stdout(io.StringIO()):
            g.prepare(argparse.Namespace(inputs=inputs, manifest=manifest, cases=self.cases))
        cases = json.loads(self.cases.read_text())
        self.assertFalse(cases[0]['params']['io']['scan_film'])
        self.assertTrue(cases[4]['params']['io']['scan_film'])
        self.assertFalse(cases[5]['params']['io']['scan_film'])

    def test_unknown_case_cannot_be_skipped(self):
        self.cases.write_text(json.dumps([self.case]))
        self.args.only = 'unknown'
        with self.assertRaises(ValueError):
            g.selected(self.args)


if __name__ == '__main__':
    unittest.main()
