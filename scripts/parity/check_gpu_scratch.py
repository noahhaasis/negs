#!/usr/bin/env python3
"""Exact same-backend GPU regression; requires numpy, tifffile and a GPU.

Compares a pre-change f32 CLI to the candidate. Rejects CPU fallback, exercises
partial workgroups, optional effects, direct film scans and repeated rendering.
This is a correctness harness; --iters is not a memory benchmark.
"""
import argparse
import copy
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import tifffile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline', type=Path, required=True)
parser.add_argument('--candidate', type=Path, default=Path('target/release/spektrafilm'))
parser.add_argument('--output-dir', type=Path, required=True)
parser.add_argument('--candidate-force-staging', action='store_true')
args = parser.parse_args()
root = args.output_dir
root.mkdir(parents=True, exist_ok=True)
base = {'io': {'input_color_space': 'sRGB', 'input_cctf_decoding': False}}
none = {'film_render': {'halation': {'active': False}, 'dir_couplers': {'active': False},
                        'grain': {'active': False}, 'glare': {'active': False}},
        'print_render': {'glare': {'active': False}}, 'scanner': {'unsharp_mask': [0, 0]}}
optics = {'camera': {'diffusion_filter': {'active': True}, 'lens_blur_um': 7},
          'enlarger': {'diffusion_filter': {'active': True}},
          'scanner': {'lens_blur': 0.9},
          'film_render': {'halation': {'boost_ev': 1.2}}}
cases = [('default', {}), ('disabled', none), ('optics', optics)]
for effect in ('halation', 'dir_couplers', 'grain'):
    only = copy.deepcopy(none)
    only['film_render'][effect]['active'] = True
    cases.append((effect + '_only', only))
only = copy.deepcopy(none)
only['print_render']['glare'] = {'active': True, 'blur': 0}
cases.append(('glare_only', only))
only = copy.deepcopy(none)
only['scanner']['unsharp_mask'] = [1.3, 0.5]
cases.append(('unsharp_only', only))
only = copy.deepcopy(none)
only['camera'] = optics['camera']
only['enlarger'] = optics['enlarger']
only['scanner']['lens_blur'] = 1.1
cases.append(('optics_only', only))
cases.extend([('scan', {'io': {'scan_film': True}}),
              ('scan_optics', {**optics, 'io': {'scan_film': True}})])
env = {**os.environ, 'SPEKTRAFILM_BACKEND': 'wgpu', 'RUST_LOG': 'info'}
results = {}
for w, h in [(128, 96), (259, 173)]:
    y, x, c = np.indices((h, w, 3))
    fixture = (((x * 7 + y * 13 + c * 29) % 257) / 128).astype(np.float32)
    source = root / f'fixture-{w}.tiff'
    tifffile.imwrite(source, fixture, photometric='rgb')
    for name, overrides in cases:
        params = {**base, **overrides, 'io': {**base['io'], **overrides.get('io', {})}}
        tag = f'{name}-{w}'
        params_path = root / f'{tag}.json'
        params_path.write_text(json.dumps(params))
        for variant, binary in [('baseline', args.baseline), ('candidate', args.candidate)]:
            stem = root / f'{tag}-{variant}'
            command = [str(binary.resolve()), 'process', str(source), '--film', 'kodak_gold_200',
                       '--params', str(params_path), '-o', str(stem.with_suffix('.tiff')),
                       '--raw-out', str(stem.with_suffix('.raw')), '--iters', '3']
            if params['io'].get('scan_film'):
                command.append('--scan-film')
            run_env = env.copy()
            if variant == 'candidate' and args.candidate_force_staging:
                run_env['SPEKTRAFILM_WGPU_FORCE_STAGING'] = '1'
            run = subprocess.run(command, env=run_env, capture_output=True)
            log = run.stdout + run.stderr
            stem.with_suffix('.log').write_bytes(log)
            run.check_returncode()
            if log.count(b'gpu-resident fast path complete') != 3:
                raise SystemExit(f'{stem}: expected three GPU resident renders; inspect log')
        results[tag] = ((root / f'{tag}-baseline.raw').read_bytes()
                        == (root / f'{tag}-candidate.raw').read_bytes())
        print(tag, results[tag], flush=True)
(root / 'report.json').write_text(json.dumps(results, indent=2) + '\n')
if not all(results.values()):
    raise SystemExit('GPU output differs; see artifacts')
