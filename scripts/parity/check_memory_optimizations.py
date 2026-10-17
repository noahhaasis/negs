#!/usr/bin/env python3
"""Compare f64 CPU outputs with an original binary; needs numpy and tifffile.

Run from the repository root. The candidate is tested with both parallel and
serial grain. Baseline/candidate binaries must be precision-f64 builds.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import tifffile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline', type=Path, required=True)
parser.add_argument('--candidate', type=Path, default=Path('target/release/spektrafilm-f64'))
parser.add_argument('--output-dir', type=Path, required=True)
args = parser.parse_args()
root = args.output_dir
root.mkdir(parents=True, exist_ok=True)
y, x, c = np.indices((96, 128, 3))
fixture = (((x * 7 + y * 13 + c * 29) % 257) / 128).astype(np.float32)
tifffile.imwrite(root / 'fixture.tiff', fixture, photometric='rgb')
cases = [
    ('colour', 'kodak_gold_200', False, True, 1, 0.5),
    ('no_lut', 'kodak_gold_200', False, False, 1, 0.5),
    ('bw', 'kodak_doublex', True, True, 3, 4.0),
    ('slide', 'fujifilm_velvia_100', True, True, 2, 0.0),
]
env = {**os.environ, 'SPEKTRAFILM_BACKEND': 'cpu'}
env.pop('SPEKTRAFILM_SERIAL_GRAIN', None)
results = {}
for name, film, scan, lut, layers, blur in cases:
    params = {
        'settings': {'use_enlarger_lut': lut, 'use_scanner_lut': lut},
        'io': {'input_color_space': 'sRGB', 'input_cctf_decoding': False},
        'film_render': {'grain': {'n_sub_layers': layers}, 'glare': {'blur': blur}},
        'print_render': {'glare': {'blur': blur}},
    }
    for variant, binary, serial in [
        ('baseline', args.baseline, False),
        ('parallel', args.candidate, False),
        ('serial', args.candidate, True),
    ]:
        params['settings']['serial_grain'] = serial
        stem = root / f'{name}-{variant}'
        stem.with_suffix('.json').write_text(json.dumps(params))
        command = [str(binary.resolve()), 'process', str(root / 'fixture.tiff'),
                   '-o', str(stem.with_suffix('.tiff')), '--film', film,
                   '--params', str(stem.with_suffix('.json')),
                   '--raw-out', str(stem.with_suffix('.raw'))]
        if scan:
            command.append('--scan-film')
        result = subprocess.run(command, env=env, capture_output=True)
        stem.with_suffix('.log').write_bytes(result.stdout + result.stderr)
        result.check_returncode()
    original = (root / f'{name}-baseline.raw').read_bytes()
    results[name] = {
        mode: (root / f'{name}-{mode}.raw').read_bytes() == original
        for mode in ('parallel', 'serial')
    }
    print(name, results[name], flush=True)
(root / 'report.json').write_text(json.dumps(results, indent=2) + '\n')
if not all(all(modes.values()) for modes in results.values()):
    raise SystemExit('Output differs from the baseline; see artifacts.')
