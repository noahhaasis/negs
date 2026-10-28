#!/usr/bin/env python3
"""macOS: time full-resolution renders of Negs, spektrafilm-rs and Python spektrafilm.

Usage: run.py --negs-bin DIR --rs-bin DIR --py-src DIR --py-python PYTHON
              --inputs DIR --out DIR [--runs 3] [--images nikon-z6,leica-q3]

Every engine renders the same linear ProPhoto RGB 16-bit TIFF (see
prepare_inputs.py) to a 16-bit sRGB TIFF: Kodak Gold 200 printed on Portra
Endura, with each app's own export settings. Each run is a fresh process
under /usr/bin/time -l, which reports wall time and the kernel's lifetime
peak physical footprint (on Apple Silicon this includes Metal buffers).
One warm-up run per configuration (24 MP image) fills disk and JIT caches;
then the measured runs cycle through all configurations in turn. Results
go to OUT/results.json after every run, with each run's log next to it.
"""
import argparse
import json
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tempfile
import time

FILM, PAPER = 'kodak_gold_200', 'kodak_portra_endura'
SCRIPT_DIR = Path(__file__).resolve().parent


def configs(args):
    negs, rs = Path(args.negs_bin), Path(args.rs_bin)
    negs_input = ['--input-colour', 'ProPhoto RGB', '--input-linear', '--data-dir', str(args.negs_data)]
    rs_data = ['--data-dir', str(Path(args.rs_bin).parents[1] / 'data')]
    return [
        # The Negs app exports with RuntimeParams::default(): direct spectral, no LUTs.
        dict(id='negs-cpu', engine='Negs', path='CPU reference (f64)', backend='cpu',
             cmd=[negs / 'spektrafilm-f64', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *negs_input]),
        # "Use less memory for export" (settings.serial_grain); the GPU grain ignores it.
        dict(id='negs-cpu-less-memory', engine='Negs', path='CPU reference (f64), less memory', backend='cpu',
             params={'settings': {'serial_grain': True}},
             cmd=[negs / 'spektrafilm-f64', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *negs_input]),
        # Not what the app exports: the LUTs the other engines export with, for a like-for-like CPU comparison.
        dict(id='negs-cpu-luts', engine='Negs', path='CPU (f64, LUTs)', backend='cpu',
             params={'settings': {'use_enlarger_lut': True, 'use_scanner_lut': True}},
             cmd=[negs / 'spektrafilm-f64', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *negs_input]),
        dict(id='negs-gpu', engine='Negs', path='GPU (f32, Metal)', backend='wgpu',
             cmd=[negs / 'spektrafilm', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *negs_input]),
        # spektrafilm-rs's GUI turns enlarger and scanner LUTs on for its f64 export.
        dict(id='rs-cpu', engine='spektrafilm-rs', path='CPU (f64, LUTs)', backend='cpu',
             params={'settings': {'use_enlarger_lut': True, 'use_scanner_lut': True}},
             cmd=[rs / 'spektrafilm-f64', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *rs_data]),
        dict(id='rs-gpu', engine='spektrafilm-rs', path='GPU (f32, Metal)', backend='wgpu',
             cmd=[rs / 'spektrafilm', 'process', '{in}', '-o', '{out}', '--film', FILM, '--paper', PAPER, *rs_data]),
        dict(id='python', engine='Python spektrafilm', path='CPU (f64, NumPy + Numba)', backend=None,
             cmd=[args.py_python, '-I', SCRIPT_DIR / 'python_spektrafilm.py', args.py_src, '{in}', '{out}', FILM, PAPER]),
    ]


def sh(*cmd):
    return subprocess.run(cmd, capture_output=True, text=True).stdout.strip()


def swapouts():
    match = re.search(r'Swapouts:\s+(\d+)', sh('vm_stat'))
    return int(match.group(1)) if match else None


def preflight(force):
    problems = []
    if 'AC Power' not in sh('pmset', '-g', 'batt'):
        problems.append('running on battery')
    if re.search(r'lowpowermode\s+1', sh('pmset', '-g')):
        problems.append('Low Power Mode is on')
    others = [line for line in sh('pgrep', '-fl', 'spektrafilm').splitlines() if 'benchmark' not in line]
    if others:
        problems.append('other spektrafilm processes: ' + '; '.join(others))
    for problem in problems:
        print('preflight:', problem, file=sys.stderr)
    if problems and not force:
        sys.exit('preflight failed (pass --force to run anyway)')


def run_one(config, image, inputs, out_dir, label):
    with tempfile.TemporaryDirectory(dir=out_dir) as work:
        work = Path(work)
        cmd = [str(part) for part in config['cmd']]
        cmd = [part.replace('{in}', str(inputs / f'{image}.tiff')).replace('{out}', str(work / 'out.tiff')) for part in cmd]
        if 'params' in config:
            (work / 'params.json').write_text(json.dumps(config['params']))
            cmd += ['--params', str(work / 'params.json')]
        env = dict(os.environ)
        if config['backend']:
            env['SPEKTRAFILM_BACKEND'] = config['backend']
        swap_before, start = swapouts(), time.monotonic()
        proc = subprocess.run(['/usr/bin/time', '-l', *cmd], capture_output=True, text=True, env=env)
        wall = time.monotonic() - start
        swap_after = swapouts()
        log = proc.stdout + proc.stderr
        (out_dir / 'logs').mkdir(exist_ok=True)
        (out_dir / 'logs' / f'{label}.log').write_text(' '.join(cmd) + '\n\n' + log)
        if proc.returncode != 0:
            raise SystemExit(f'{label} failed with {proc.returncode}; see logs/{label}.log')
        if config['backend'] == 'wgpu' and 'Backend: wgpu' not in log:
            raise SystemExit(f'{label} did not run on the GPU; see logs/{label}.log')
        if config['backend'] == 'cpu' and 'Backend: CPU' not in log:
            raise SystemExit(f'{label} did not run on the CPU backend; see logs/{label}.log')
        field = lambda name: int(re.search(rf'(\d+)\s+{name}', log).group(1))
        # The engine's own render time, without process start, loading and saving.
        rust = re.search(r'^Pipeline: (\d+) ms', log, re.M)
        python = re.search(r'simulate ([\d.]+) s', log)
        return dict(
            wall_s=round(wall, 3),
            pipeline_s=int(rust.group(1)) / 1000 if rust else float(python.group(1)) if python else None,
            time_real_s=float(re.search(r'([\d.]+) real', log).group(1)),
            user_s=float(re.search(r'([\d.]+) user', log).group(1)),
            peak_footprint_bytes=field('peak memory footprint'),
            max_rss_bytes=field('maximum resident set size'),
            swapouts=None if swap_before is None else swap_after - swap_before,
        )


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--negs-bin', required=True)
    parser.add_argument('--negs-data', default=Path(__file__).resolve().parents[2] / 'data')
    parser.add_argument('--rs-bin', required=True)
    parser.add_argument('--py-src', required=True)
    parser.add_argument('--py-python', required=True)
    parser.add_argument('--inputs', required=True, type=Path)
    parser.add_argument('--out', required=True, type=Path)
    parser.add_argument('--runs', type=int, default=3)
    parser.add_argument('--images', default='nikon-z6,leica-q3')
    parser.add_argument('--only', help='comma-separated configuration ids')
    parser.add_argument('--no-warmup', action='store_true')
    parser.add_argument('--force', action='store_true')
    args = parser.parse_args()
    preflight(args.force)
    args.out.mkdir(parents=True, exist_ok=True)
    selected = configs(args)
    if args.only:
        selected = [c for c in selected if c['id'] in args.only.split(',')]
    images = args.images.split(',')
    results = dict(
        host=dict(machine=sh('sysctl', '-n', 'hw.model'), cpu=sh('sysctl', '-n', 'machdep.cpu.brand_string'),
                  memory_bytes=int(sh('sysctl', '-n', 'hw.memsize')), macos=platform.mac_ver()[0]),
        started=time.strftime('%Y-%m-%dT%H:%M:%S%z'),
        configs=[{k: v for k, v in c.items() if k != 'cmd'} for c in selected],
        runs=[],
    )

    def save():
        (args.out / 'results.json').write_text(json.dumps(results, indent=2))

    if not args.no_warmup:
        for config in selected:
            print(f'warm-up {config["id"]} {images[0]}', flush=True)
            run_one(config, images[0], args.inputs, args.out, f'warmup-{config["id"]}-{images[0]}')
    for image in images:
        for rep in range(1, args.runs + 1):
            for config in selected:
                label = f'{config["id"]}-{image}-{rep}'
                result = run_one(config, image, args.inputs, args.out, label)
                results['runs'].append(dict(config=config['id'], image=image, rep=rep, **result))
                save()
                print(f'{label}: {result["wall_s"]:.1f} s, {result["peak_footprint_bytes"] / 2**30:.2f} GiB, '
                      f'swapouts {result["swapouts"]}', flush=True)
    results['finished'] = time.strftime('%Y-%m-%dT%H:%M:%S%z')
    save()


if __name__ == '__main__':
    main()
