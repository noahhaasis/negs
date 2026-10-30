#!/usr/bin/env python3
"""Bridge the native golden corpus/gates to WASM's production worker.

prepare --inputs DIR: writes /results/golden-cases.json (never changes goldens).
reference: native f64, f32 CPU and wgpu on exactly those inputs/params.
compare --prefix NAME: checks float scans and normal display readbacks, exits
nonzero for any missing/failed case. Requires numpy; colour uses golden_compare.
See docs/wasm-correctness.md for commands, scope and diffusion policy.
"""
import argparse
import copy
import concurrent.futures
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("native_golden", ROOT / "scripts/golden/run.py")
golden = importlib.util.module_from_spec(spec)
spec.loader.exec_module(golden)
RESULTS = ROOT / "web/results"
CASES = RESULTS / "golden-cases.json"
OUT = ROOT / "target/wasm-correctness"


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def case_hash(c):
    fields = [c['input_sha256'], c['film'], c['paper'], c['params']]
    if c.get('keep_pixels') or 'budget' in c:
        fields += [c.get('keep_pixels'), c.get('source_sha256'), c.get('budget', 0)]
    return hashlib.sha256(json.dumps(fields, sort_keys=True).encode()).hexdigest()


def seal_cases(cases):
    for c in cases:
        c['input_sha256'] = sha(c['input'])
        c['case_sha256'] = case_hash(c)


def random_fields(params):
    film = params.get('film_render', {})
    return (film.get('grain', {}).get('active', True)
            or film.get('glare', {}).get('active', True)
            or (not params.get('io', {}).get('scan_film', False)
                and params.get('print_render', {}).get('glare', {}).get('active', True)))


def prepare_phone(args):
    cases = []
    for source, filename in [('raf', '0955097116.raf'), ('jpeg', 'card-24mp.jpg')]:
        original = RESULTS / filename
        native = OUT / 'working' / f'{source}.tif'
        for effect in ('none', 'diff_bpm', 'diff_enlarger' if source == 'raf' else 'diff_glimmer'):
            params = golden.merge(golden.effective_params('det'), {} if effect == 'none' else golden.preset(effect))
            cases.append(dict(name=f'phone-{source}-{effect}', image=f'/results/{filename}', input=str(native),
                source_name=source, source_sha256=sha(original), keep_pixels=12000000,
                film=golden.STOCKS[0][0], paper=golden.STOCKS[0][1], params=params, random=False,
                approximate=effect != 'none', budget=160*1024*1024))
    seal_cases(cases)
    args.cases.write_text(json.dumps(cases, indent=1))
    print(f'{len(cases)} phone working-source cases: {args.cases}')


def prepare(args):
    inputs = Path(args.inputs).resolve()
    historical = json.loads(Path(args.manifest).read_text())
    cases, records = [], {}
    for img in golden.PHOTOS + golden.SYNTH:
        path = inputs / f"{img}.tif"
        records[img] = {"sha256": sha(path), "historical_match": sha(path) == historical["inputs"][img]}
        if not records[img]['historical_match']:
            raise ValueError(f"Not the historical golden input: {path}")
        for film, paper in golden.STOCKS:
            params = copy.deepcopy(golden.effective_params("det"))
            params["io"]["scan_film"] = paper == "scan"
            cases.append(dict(name=f"{img}--{film}", image=f"/results/golden-inputs/{path.name}",
                              input=str(path), film=film, paper=film if paper == "scan" else paper,
                              params=params, random=False,
                              golden_name=f"{img}__{film}__{paper}__det"))
    # Isolate effects from random fields: the old presets mostly leave grain ON.
    for img in golden.PRESET_INPUTS:
        for pname in golden.PRESET_NAMES:
            if pname in ("det", "grain_on", "grain_off", "grain_on_coarse", "grain_off_coarse"):
                continue
            params = golden.merge(golden.effective_params(pname), golden.preset("det"))
            cases.append(dict(name=f"{img}--{pname}", image=f"/results/golden-inputs/{img}.tif",
                              input=str(inputs / f"{img}.tif"), film=golden.STOCKS[0][0], paper=golden.STOCKS[0][1],
                              params=params, random=False))
    # Shared GPU PCG streams can be gated statistically against native wgpu;
    # CPU MT streams cannot be compared pixelwise against the GPU/f64 stream.
    for img in golden.PRESET_INPUTS:
        for pname in ("default", "grain_on", "grain_off", "grain_on_coarse", "grain_off_coarse"):
            cases.append(dict(name=f"{img}--{pname}", image=f"/results/golden-inputs/{img}.tif",
                              input=str(inputs / f"{img}.tif"), film=golden.STOCKS[0][0], paper=golden.STOCKS[0][1],
                              params=golden.effective_params(pname), random=random_fields(golden.effective_params(pname)),
                              golden_name=f"{img}__{golden.STOCKS[0][0]}__{golden.STOCKS[0][1]}__{pname}"))
    link = RESULTS / "golden-inputs"
    if not link.exists() and not link.is_symlink():
        link.symlink_to(inputs, target_is_directory=True)
    if link.resolve() != inputs:
        raise ValueError(f"{link} points to {link.resolve()}, not {inputs}")
    seal_cases(cases)
    args.cases.write_text(json.dumps(cases, indent=1))
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "inputs.json").write_text(json.dumps(records, indent=1))
    print(f"{len(cases)} cases; {sum(v['historical_match'] for v in records.values())}/10 historical inputs match")


def selected(args):
    cases = json.loads(args.cases.read_text())
    if args.only:
        names = set(args.only.split(','))
        cases = [c for c in cases if c['name'] in names]
        if {c['name'] for c in cases} != names:
            raise ValueError(f"Unknown cases: {names - {c['name'] for c in cases}}")
    if not cases:
        raise ValueError("No cases selected")
    return cases


def reference(args):
    refs = OUT / "reference"
    refs.mkdir(parents=True, exist_ok=True)
    historical = json.loads((golden.HERE / 'manifests/macos-arm64.json').read_text())
    def render_case(c):
        params = refs / f"{c['name']}.json"
        params.write_text(json.dumps(c['params']))
        for engine in args.engines.split(','):
            if c.get('approximate') and engine == 'f64':
                continue
            if engine == 'exactgpu' and not c.get('approximate'):
                continue
            binary, backend, _, _ = golden.ENGINES['gpu' if engine == 'exactgpu' else engine]
            binary = ROOT / 'target/release' / binary
            dump = refs / f"{c['name']}.{engine}.f64"
            budget = c.get('budget', 0) if engine == 'gpu' else 0
            signature = hashlib.sha256(('v4' + str(budget) + sha(binary) + sha(c['input']) + params.read_text()).encode()).hexdigest()
            if args.golden_dumps and c.get('golden_name'):
                original_case = historical['cases'][c['golden_name']]
                record = original_case['engines'].get(engine)
                if record:
                    expected = copy.deepcopy(golden.effective_params(original_case['preset']))
                    scan = original_case['paper'] == 'scan'
                    if 'scan_film' in c['params'].get('io', {}) or scan:
                        expected.setdefault('io', {})['scan_film'] = scan
                    paper = original_case['film'] if scan else original_case['paper']
                    if (c['params'] != expected or c['film'] != original_case['film'] or c['paper'] != paper
                            or sha(c['input']) != historical['inputs'][original_case['input']]):
                        raise ValueError(f"Case differs from historical golden: {c['name']}")
                    original = Path(args.golden_dumps) / f"{c['golden_name']}.{engine}.f64"
                    if sha(original) != record['f64']:
                        raise ValueError(f'Historical dump hash mismatch: {original}')
                    dump.write_bytes(original.read_bytes())
                    continue
            stamp = dump.with_suffix('.stamp')
            if dump.exists() and stamp.exists() and stamp.read_text() == signature:
                continue
            output = refs / f"{c['name']}.scratch.tiff"
            cmd = [str(binary), 'process', c['input'], '-o', str(output), '--film', c['film'], '--paper', c['paper'],
                   '--params', str(params), '--raw-out', str(dump), '--no-metadata', '--data-dir', str(ROOT / 'data')]
            if c['params'].get('io', {}).get('scan_film'):
                cmd.append('--scan-film')
            log = refs / f"{c['name']}.{engine}.log"
            code, text = golden.run_logged(cmd, golden.clean_env(backend, 1, {'SPEKTRAFILM_DIFFUSION_GRID_BUDGET': str(budget)}), log)
            log.write_text(text)
            if code:
                raise RuntimeError(f"{c['name']} {engine}: {log}")
            if engine in ('gpu', 'exactgpu') and ('Backend: wgpu (GPU)' not in text or 'gpu-resident fast path complete' not in text):
                raise RuntimeError(f"GPU fallback or nonresident render: {log}")
            stamp.write_text(signature)
            output.unlink(missing_ok=True)
        print(c['name'], flush=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        list(pool.map(render_case, selected(args)))


def ppm(path):
    magic, size, peak, data = path.read_bytes().split(b'\n', 3)
    w, h = map(int, size.split())
    if magic != b'P6' or peak != b'255' or len(data) != w*h*3:
        raise ValueError(f"Malformed PPM: {path}")
    return np.frombuffer(data, np.uint8).reshape(h, w, 3)


def compare(args):
    run = json.loads((RESULTS / f'{args.prefix}-run.json').read_text())
    cases = selected(args)
    rows = {r['name']: r for r in run['rows']}
    is_gpu = run['info']['backend'].startswith('wgpu')
    if is_gpu != (args.backend == 'gpu'):
        raise ValueError(f"Wrong backend: {run['info']}")
    reports = []
    for c in cases:
        row = {'name': c['name'], 'checks': [], 'failures': []}
        try:
            meta = rows[c['name']]
            io_params = c['params'].get('io', {})
            if io_params.get('output_color_space', 'sRGB') != 'sRGB' or not io_params.get('output_cctf_encoding', True):
                raise ValueError('these golden colour metrics require encoded sRGB output')
            w, h = meta['width'], meta['height']
            if c['case_sha256'] != case_hash(c) or meta.get('case_sha256') != c['case_sha256']:
                raise ValueError('stale/different input or parameters')
            if sha(c['input']) != c['input_sha256']:
                raise ValueError('input changed since prepare')
            if meta['backend'] != run['info']['backend']:
                raise ValueError('backend changed')
            if meta.get('budget', run['info']['budget']) != c.get('budget', run['info']['budget']):
                raise ValueError('wrong diffusion budget')
            raw = np.fromfile(RESULTS / f"{args.prefix}-{c['name']}.f32", '<f4')
            if raw.size != w*h*3 or not np.isfinite(raw).all():
                raise ValueError('invalid size or nonfinite scan')
            raw = raw.reshape(h, w, 3).astype('<f8')
            dump = OUT / f"{args.prefix}-scan.f64"
            raw.tofile(dump)
            checks = [('gpu' if is_gpu else 'f32cpu', 'gpu' if is_gpu else 'f32cpu')]
            if random_fields(c['params']):
                checks = [('gpu' if is_gpu else 'f32cpu', 'gpu_grain' if is_gpu else 'cpu_grain')]
            else:
                if c.get('approximate'):
                    active = any(c['params'].get(stage, {}).get('diffusion_filter', {}).get('active', False)
                                 for stage in ('camera', 'enlarger'))
                    if not is_gpu or not active or not c.get('budget'):
                        raise ValueError('diffusion exception without active budgeted GPU diffusion')
                checks.append(('exactgpu', 'phone_diffusion') if c.get('approximate') else ('f64', 'gpu_vs_ref'))
            if c.get('keep_pixels'):
                decoded = np.fromfile(RESULTS / f"{args.prefix}-{c['source_name']}-input.f32", '<f4')
                native = np.fromfile(c['input'] + '.f32', '<f4')
                if decoded.size != native.size or not np.isfinite(decoded).all():
                    raise ValueError('invalid decoded source')
                delta = np.abs(decoded.astype(np.float64) - native)
                if np.any(delta > 1e-5 + 1e-5*np.maximum(np.abs(decoded), np.abs(native))):
                    row['failures'].append(f"decoded source max error {delta.max()}")
                row['decoded_max_abs'] = float(delta.max())
            for engine, profile in checks:
                ref = OUT / 'reference' / f"{c['name']}.{engine}.f64"
                gates = Path(__file__).with_name('golden-gates.json') if profile == 'phone_diffusion' else golden.GATES
                cmd = [str(ROOT / 'target/release/golden_compare'), '--gates', str(gates), '--profile', profile,
                       str(ref), str(dump), str(w), str(h)]
                p = subprocess.run(cmd, capture_output=True, text=True, check=False)
                if not p.stdout.strip():
                    raise ValueError(f"Comparator failed: {p.stderr.strip()}")
                result = json.loads(p.stdout)
                row['checks'].append({'against': engine, 'reference_sha256': sha(ref), **result})
                if p.returncode or not result['pass']:
                    row['failures'].append(f"{engine}/{profile}: {result['failures']}")
            # Production Rgba8 output must agree with its raw scan, not just
            # the diagnostic Scan variant. Allow only one quantisation step.
            display = ppm(RESULTS / f"{args.prefix}-{c['name']}.ppm")
            quantised = np.floor(np.clip(raw, 0, 1)*255 + .5).astype(np.int16)
            if display.shape != raw.shape:
                raise ValueError('display dimensions differ')
            step = int(np.abs(quantised - display.astype(np.int16)).max())
            row['display_step'] = step
            if step > 1:
                row['failures'].append(f'normal display vs raw scan: {step} steps')
        except (OSError, ValueError, KeyError) as e:
            row['failures'].append(str(e))
        reports.append(row)
        worst = max((r['stats'].get('de00_max', 0) for r in row['checks']), default=0)
        print(f"{'FAIL' if row['failures'] else 'PASS'} {c['name']} max ΔE {worst:.6f} {'; '.join(row['failures'])}", flush=True)
    failed = sum(bool(r['failures']) for r in reports)
    report = {'info': run['info'], 'passed': len(reports)-failed, 'failed': failed, 'cases': reports}
    (OUT / f'{args.prefix}-report.json').write_text(json.dumps(report, indent=1))
    print(f"{len(reports)-failed}/{len(reports)} pass; {failed} fail")
    return bool(failed)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--cases', type=Path, default=CASES)
    sub = p.add_subparsers(dest='command', required=True)
    prepare_parser = sub.add_parser('prepare')
    prepare_parser.add_argument('--inputs', required=True)
    prepare_parser.add_argument('--manifest', default=str(golden.HERE / 'manifests/macos-arm64.json'))
    sub.add_parser('prepare-phone')
    ref = sub.add_parser('reference')
    ref.add_argument('--only')
    ref.add_argument('--golden-dumps', help='Verified historical native dumps; never regenerated or overwritten')
    ref.add_argument('--jobs', type=int, default=3)
    ref.add_argument('--engines', default='f64,f32cpu,gpu')
    comp = sub.add_parser('compare')
    comp.add_argument('--prefix', required=True)
    comp.add_argument('--backend', choices=('gpu', 'cpu'), default='gpu')
    comp.add_argument('--only')
    args = p.parse_args()
    return {'prepare': prepare, 'prepare-phone': prepare_phone, 'reference': reference, 'compare': compare}[args.command](args)


if __name__ == '__main__':
    sys.exit(main())
