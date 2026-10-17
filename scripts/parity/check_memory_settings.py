#!/usr/bin/env python3
"""Check exact image output across memory-related settings.

Compare same-precision binaries from before and after a memory change. The
fixture includes values above white, and --raw-out is compared byte-for-byte.
The executable log must confirm that both renders used the requested backend.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np
import tifffile


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline', required=True, type=Path)
    parser.add_argument('--candidate', required=True, type=Path)
    parser.add_argument('--candidate-data-dir', type=Path)
    parser.add_argument('--backend', choices=('cpu', 'wgpu'), default='cpu')
    parser.add_argument('--candidate-force-staging', action='store_true')
    parser.add_argument('--output-dir', required=True, type=Path)
    args = parser.parse_args()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)

    y, x, c = np.indices((173, 259, 3))
    fixture = (((x * 7 + y * 13 + c * 29) % 257) / 128).astype(np.float32)
    source = out / 'fixture.tiff'
    tifffile.imwrite(source, fixture, photometric='rgb')

    base = {'io': {'input_color_space': 'sRGB', 'input_cctf_decoding': False},
            'settings': {'use_enlarger_lut': True, 'use_scanner_lut': True}}
    off = {'film_render': {'halation': {'active': False},
                           'dir_couplers': {'active': False},
                           'grain': {'active': False}, 'glare': {'active': False}},
           'print_render': {'glare': {'active': False}},
           'scanner': {'unsharp_mask': [0, 0]}}
    cases = [
        ('default', 'kodak_gold_200', None, False, {}),
        ('lut_off', 'kodak_gold_200', None, False,
         {'settings': {'use_enlarger_lut': False, 'use_scanner_lut': False}}),
        ('effects_off', 'kodak_gold_200', None, False, off),
        ('optics', 'kodak_gold_200', None, False,
         {'camera': {'diffusion_filter': {'active': True}, 'lens_blur_um': 7},
          'enlarger': {'diffusion_filter': {'active': True}, 'lens_blur': 0.8},
          'scanner': {'lens_blur': 0.9, 'unsharp_mask': [1.3, 0.5]}}),
        ('scanner_iir', 'kodak_gold_200', None, False,
         {'scanner': {'lens_blur': 3.6, 'unsharp_mask': [1.3, 0.5]},
          'film_render': {'grain': {'active': False}}}),
        ('grain_layers', 'kodak_gold_200', None, False,
         {'film_render': {'grain': {'n_sub_layers': 3, 'blur': 0.65}}}),
        ('grain_serial', 'kodak_gold_200', None, False,
         {'film_render': {'grain': {'n_sub_layers': 3, 'blur': 0.65}},
          'settings': {'serial_grain': True}}),
        ('grain_no_blur', 'kodak_gold_200', None, False,
         {'film_render': {'grain': {'blur': 0.0}}}),
        ('halation_dir', 'kodak_gold_200', None, False,
         {'film_render': {'grain': {'active': False},
                          'halation': {'boost_ev': 1.2}}}),
        ('glare', 'kodak_gold_200', None, False,
         {'film_render': {'glare': {'blur': 1.1}},
          'print_render': {'glare': {'blur': 1.1}}}),
        ('scan_optics', 'kodak_gold_200', None, True,
         {'camera': {'diffusion_filter': {'active': True}},
          'scanner': {'lens_blur': 1.1, 'unsharp_mask': [1.3, 0.5]}}),
        ('bw_print', 'kodak_doublex', 'kodak_2302', False,
         {'film_render': {'development_time': 4, 'grain': {'n_sub_layers': 3}},
          'print_render': {'development_time': 2}}),
        ('bw_scan', 'kodak_trix', None, True,
         {'film_render': {'grain': {'n_sub_layers': 2}}}),
        ('slide_scan', 'fujifilm_velvia_100', None, True, {}),
    ]

    binaries = {'baseline': args.baseline.resolve(),
                'candidate': args.candidate.resolve()}
    report = {'binaries': {name: digest(path) for name, path in binaries.items()},
              'fixture_sha256': digest(source), 'backend': args.backend,
              'candidate_force_staging': args.candidate_force_staging,
              'cases': {}}
    env = {**os.environ, 'SPEKTRAFILM_BACKEND': args.backend,
           'RUST_LOG': 'info'}
    for name, film, paper, scan, override in cases:
        params = {**base, **override,
                  'settings': {**base['settings'], **override.get('settings', {})}}
        (out / f'{name}.json').write_text(json.dumps(params, indent=2) + '\n')
        for variant, binary in binaries.items():
            stem = out / f'{name}-{variant}'
            command = [str(binary), 'process', str(source), '--film', film,
                       '--params', str(out / f'{name}.json'),
                       '-o', str(stem.with_suffix('.tiff')),
                       '--raw-out', str(stem.with_suffix('.raw'))]
            if paper:
                command += ['--paper', paper]
            if scan:
                command.append('--scan-film')
            if variant == 'candidate' and args.candidate_data_dir:
                command += ['--data-dir', str(args.candidate_data_dir.resolve())]
            run_env = env.copy()
            if variant == 'candidate' and args.candidate_force_staging:
                run_env['SPEKTRAFILM_WGPU_FORCE_STAGING'] = '1'
            run = subprocess.run(command, env=run_env, capture_output=True)
            log = run.stdout + run.stderr
            stem.with_suffix('.log').write_bytes(log)
            run.check_returncode()
            marker = (b'Backend: CPU (rayon)' if args.backend == 'cpu'
                      else b'gpu-resident fast path complete')
            if marker not in log:
                raise RuntimeError(f'{stem}: {args.backend} backend not confirmed')
        same = ((out / f'{name}-baseline.raw').read_bytes()
                == (out / f'{name}-candidate.raw').read_bytes())
        report['cases'][name] = same
        print(name, same, flush=True)
    report['passed'] = all(report['cases'].values())
    (out / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    return 0 if report['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
