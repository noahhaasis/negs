#!/usr/bin/env python3
"""Full-pixel all-pair metrics and local tiled static site; new review directory only."""
import argparse
import csv
import hashlib
import itertools
import os
from pathlib import Path
import shutil
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import load, save, sha


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--run', type=Path, required=True)
    p.add_argument('--output-dir', type=Path, required=True)
    p.add_argument('--tracks', nargs='+', choices=['application', 'same-input', 'deterministic'])
    p.add_argument('--case', action='append', help='case IDs for an explicitly partial smoke review')
    p.add_argument('--wait-seconds', type=int, default=0, help='global deadline to wait for in-flight render results')
    a = p.parse_args()
    out, source = a.output_dir.resolve(), a.run.resolve()
    out.mkdir(parents=True, exist_ok=False)
    os.environ['NUMBA_CACHE_DIR'] = str(source / 'numba-metrics')
    os.environ.setdefault('NUMBA_NUM_THREADS', '4')
    import numpy as np
    from PIL import Image, ImageCms
    from metrics import lab8, rgb_metrics, float_metrics
    Image.MAX_IMAGE_PIXELS = None
    icc = ImageCms.ImageCmsProfile(ImageCms.createProfile('sRGB')).tobytes()
    config = load(source / 'config.json')
    deadline = time.monotonic() + a.wait_seconds
    report = {'generation_status': 'running', 'expected_case_tracks': len(config['cases'])*len(a.tracks or config['tracks']),
              'schema': 1, 'run_id': config['run_id'], 'created_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
              'run_directory': str(source), 'engines': config['engines'], 'cases': [],
              'method': {'numeric_source': 'lossless exported RGB8, no ICC display conversion; every pixel, no sampling',
                         'equality': 'strict equality of all three 8-bit RGB channels, NOT compressed file bytes',
                         'delta_e': 'CIEDE2000, D65; colour-science sRGB matrix/CCTF; full pixels; quantile linear interpolation',
                         'heatmap': 'max absolute RGB8 channel error; fit image max-pooled in non-overlapping blocks; 1:1 tiles exact',
                         'grain': 'unmodified per-pixel metrics include stochastic grain; no statistical substitute for equality',
                         'float': 'unclipped encoded-sRGB scan; native --raw-out f64, Python f64, Web renderRaw f32; relative floor 1e-12',
                         'diagnostics': 'separate resized linear-sRGB float32 fixture; deterministic disables only grain and film/print glare',
                         'cache': 'none; new review directory required'}}
    shutil.copy2(source / 'provenance.json', out / 'provenance.json')
    shutil.copy2(source / 'config.json', out / 'config.json')
    if (source / 'parameter-audit.json').exists():
        shutil.copy2(source / 'parameter-audit.json', out / 'parameter-audit.json')
    else:
        save(out / 'parameter-audit.json', {'status': 'not run; use audit_params.py --run to generate'})
    for filename in ['index.html', 'site.js', 'site.css']:
        shutil.copy2(Path(__file__).with_name(filename), out / filename)
    rows = []
    for track in a.tracks or config['tracks']:
        for case in config['cases']:
            if a.case and case['name'] not in a.case:
                continue
            print('COMPARE', track, case['name'], flush=True)
            entry = {**case, 'track': track, 'renders': {}, 'pairs': {}}
            if track != 'application':
                fixture_folder = out / 'fixtures'
                fixture_folder.mkdir(exist_ok=True)
                entry['fixture_downloads'] = {}
                for extension in ['json', 'tiff', 'f32']:
                    original = source / 'fixtures' / f'{case["fixture_id"]}.{extension}'
                    link = fixture_folder / original.name
                    if original.exists():
                        if not link.exists():
                            link.symlink_to(original)
                        entry['fixture_downloads'][extension] = link.relative_to(out).as_posix()
            report['cases'].append(entry)
            arrays, labs, scans = {}, {}, {}
            content, digests, pair_cache = {}, {}, {}
            folder = out / 'images' / track / case['name']
            folder.mkdir(parents=True)
            for engine in config['selected_engines']:
                dest = source / 'renders' / track / case['name'] / engine
                result_path = dest / 'result.json'
                while not result_path.exists() and time.monotonic() < deadline:
                    time.sleep(2)
                result = load(result_path) if result_path.exists() else {'status': 'missing', 'error': 'no result.json; not a pass'}
                entry['renders'][engine] = result
                if result['status'] != 'ok':
                    continue
                try:
                    full_path = dest / result['output']
                    image = Image.open(full_path)
                    if image.mode != 'RGB':
                        raise RuntimeError(f'expected RGB8, got {image.mode}')
                    rgb = np.asarray(image).copy()
                    result['width'], result['height'] = image.size
                    result['output_sha256'] = sha(full_path)
                    result['nonfinite_rgb8'] = 0
                    digest = hashlib.sha256(rgb).hexdigest()
                    result['rgb_pixels_sha256'] = digest
                    digests[engine] = digest
                    assets = folder / engine
                    assets.mkdir()
                    # Original export + metadata links only. Never expose RAWs or preset files.
                    for filename in [result['output'], 'result.json', 'params.json', 'export.log', 'scan.f64', 'scan.f32']:
                        if (dest / filename).exists():
                            (assets / filename).symlink_to(dest / filename)
                    result['download'] = (assets / result['output']).relative_to(out).as_posix()
                    result['metadata_url'] = (assets / 'result.json').relative_to(out).as_posix()
                    content_key = (digest, rgb.shape)
                    if content_key in content:
                        # Deduplicate only VERIFIED identical pixels within this
                        # review, never reuse a prior engine export or run.
                        canonical = content[content_key]
                        arrays[engine], labs[engine] = canonical['rgb'], canonical['lab']
                        result.update(preview=canonical['preview'], tile_base=canonical['tile_base'], tile_size=512)
                    else:
                        preview = image.copy()
                        preview.thumbnail((960, 960), Image.Resampling.LANCZOS)
                        preview.save(assets / 'preview.png', icc_profile=icc)
                        result['preview'] = (assets / 'preview.png').relative_to(out).as_posix()
                        result['tile_base'] = assets.relative_to(out).as_posix()
                        result['tile_size'] = 512
                        for y in range(0, image.height, 512):
                            for x in range(0, image.width, 512):
                                image.crop((x, y, min(x+512, image.width), min(y+512, image.height))).save(
                                    assets / f'tile-{x//512}-{y//512}.png', icc_profile=icc)
                        arrays[engine] = rgb
                        labs[engine] = lab8(rgb)
                        content[content_key] = {'rgb': rgb, 'lab': labs[engine], 'preview': result['preview'], 'tile_base': result['tile_base']}
                    for filename, dtype in [('scan.f64', '=f8'), ('scan.f32', '<f4')]:
                        if (dest / filename).exists():
                            scan = np.fromfile(dest / filename, dtype).astype(np.float64)
                            if scan.size != rgb.size:
                                raise RuntimeError('float scan/output dimensions incompatible')
                            scans[engine] = scan
                            result['scan_sha256'] = sha(dest / filename)
                            result['scan_download'] = (assets / filename).relative_to(out).as_posix()
                            result['scan_dtype'] = dtype
                            if not np.isfinite(scan).all():
                                raise RuntimeError('nonfinite float output')
                            break
                except Exception as exc:
                    result.update(status='error', error=f'asset validation: {type(exc).__name__}: {exc}')
                    arrays.pop(engine, None)
                    labs.pop(engine, None)
                    scans.pop(engine, None)
            for one, two in itertools.combinations(config['selected_engines'], 2):
                key = one + '--' + two
                if one not in arrays or two not in arrays:
                    metric = {'status': 'missing-or-error', 'exact_equal': False}
                else:
                    cache_key = (digests[one], arrays[one].shape, digests[two], arrays[two].shape)
                    if cache_key not in pair_cache:
                        pair_cache[cache_key] = rgb_metrics(arrays[one], arrays[two], (labs[one], labs[two]))
                    metric = {**pair_cache[cache_key]}
                    if metric['status'] == 'compared':
                        h, w, _ = arrays[one].shape
                        step = max(1, int(np.ceil(max(h, w)/768)))
                        # Fixed scale, max-pool rather than normalize each photograph.
                        hh, ww = int(np.ceil(h/step)), int(np.ceil(w/step))
                        pooled = np.zeros((hh*step, ww*step), dtype=np.uint8)
                        for y in range(0, h, 128):
                            pooled[y:min(h, y+128), :w] = np.abs(
                                arrays[one][y:y+128].astype(np.int16)-arrays[two][y:y+128].astype(np.int16)).max(axis=2).astype(np.uint8)
                        pooled = pooled.reshape(hh, step, ww, step).max(axis=(1, 3))
                        heat = folder / f'{key}.diff.png'
                        Image.fromarray(pooled).save(heat)
                        metric['difference_preview'] = heat.relative_to(out).as_posix()
                        metric['difference_pool_block_px'] = step
                        if one in scans and two in scans:
                            metric['float_scan'] = float_metrics(scans[one], scans[two])
                entry['pairs'][key] = {'a': one, 'b': two, **metric}
                rows.append({'track': track, 'case': case['name'], 'a': one, 'b': two,
                             **{k: v for k, v in metric.items() if not isinstance(v, (dict, list))},
                             **{'de_' + k: v for k, v in metric.get('delta_e_2000', {}).items()}})
            save(out / 'report.json', report)
            del arrays, labs, scans, content, pair_cache
    fields = list(dict.fromkeys(k for row in rows for k in row))
    with (out / 'metrics.csv').open('w', newline='') as stream:
        writer = csv.DictWriter(stream, fields)
        writer.writeheader()
        writer.writerows(rows)
    pairs = [pair for case in report['cases'] for pair in case['pairs'].values()]
    results = [r for case in report['cases'] for r in case['renders'].values()]
    report['summary'] = {'case_tracks': len(report['cases']), 'successful_renders': sum(r['status'] == 'ok' for r in results),
                         'failed_or_missing_renders': sum(r['status'] != 'ok' for r in results),
                         'compared_pairs': sum(p['status'] == 'compared' for p in pairs),
                         'dimension_mismatches': sum(p['status'] == 'dimension-mismatch' for p in pairs),
                         'missing_pairs': sum(p['status'] == 'missing-or-error' for p in pairs),
                         'strict_equality_passes': sum(p.get('exact_equal', False) for p in pairs), 'all_passed': bool(pairs) and all(p.get('exact_equal', False) for p in pairs)}
    report['generation_status'] = 'complete'
    report['track_summaries'] = {}
    for track in dict.fromkeys(c['track'] for c in report['cases']):
        selected = [c for c in report['cases'] if c['track'] == track]
        selected_pairs = [p for c in selected for p in c['pairs'].values()]
        report['track_summaries'][track] = {'cases': len(selected),
            'render_errors_or_missing': sum(r['status'] != 'ok' for c in selected for r in c['renders'].values()),
            'compared_pairs': sum(p['status'] == 'compared' for p in selected_pairs),
            'dimension_mismatches': sum(p['status'] == 'dimension-mismatch' for p in selected_pairs),
            'strict_equality_passes': sum(p.get('exact_equal', False) for p in selected_pairs),
            'all_passed': all(p.get('exact_equal', False) for p in selected_pairs)}
    save(out / 'report.json', report)
    (out / 'REPORT.md').write_text('# Local RAW / preset accuracy review\n\n' +
        '\n'.join(f'- {k}: {v}' for k, v in report['summary'].items()) +
        '\n\nNo dimension alignment or tolerances were applied. See report.json for all pairs, metadata and errors.\n' +
        '\nServe locally: `python3 -m http.server 8140 --bind 127.0.0.1 --directory "' + str(out) + '"`\n')
    print(report['summary'], flush=True)


if __name__ == '__main__':
    main()
