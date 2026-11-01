#!/usr/bin/env python3
"""Measures the stock-browser thumbnails against the native helper.

Usage: bench_stock_previews.py PHOTO [LONG_SIDE ...]

Runs the main preview render once (as the app does on open), then a
thumbnail for every film (with its target paper) and every paper (with the
current film), reporting per-thumbnail time and the helper's memory.
"""
import json, os, statistics, subprocess, sys, tempfile, time
from pathlib import Path

root = Path(__file__).resolve().parent.parent
photo = Path(sys.argv[1]).resolve()
sizes = [int(s) for s in sys.argv[2:]] or [256]
helper = root / 'target/release/spektrafilm-native'
proc = subprocess.Popen([str(helper), '--data-dir', str(root / 'data')],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
scratch = Path(tempfile.mkdtemp(prefix='negs-bench-'))
sequence = 0


def memory():
    if not Path('/proc').exists():
        # macOS: resident size and the physical footprint (what Activity Monitor shows).
        rss = int(subprocess.run(['ps', '-o', 'rss=', '-p', str(proc.pid)], capture_output=True, text=True).stdout) // 1024
        line = subprocess.run(['footprint', '-f', 'bytes', str(proc.pid)], capture_output=True, text=True).stdout
        footprint = int(line.split('Footprint:')[1].split()[0]) // 2**20 if 'Footprint:' in line else -1
        return {'RSS': rss, 'footprint': footprint}
    status = Path(f'/proc/{proc.pid}/status').read_text()
    fields = dict(line.split(':', 1) for line in status.splitlines())
    return {k: int(fields[k].split()[0]) // 1024 for k in ('VmRSS', 'VmHWM')}


def request(command):
    global sequence
    sequence += 1
    command['id'] = sequence
    started = time.perf_counter()
    proc.stdin.write(json.dumps(command) + '\n')
    proc.stdin.flush()
    reply = json.loads(proc.stdout.readline())
    if 'error' in reply:
        raise SystemExit(f"{command['command']}: {reply['error']}")
    return reply['result'], (time.perf_counter() - started) * 1000


catalog, _ = request({'command': 'catalog'})
defaults = catalog['defaults']
films = [p for p in catalog['profiles'] if not p['paper']]
papers = [p for p in catalog['profiles'] if p['paper']]
print(f'{len(films)} films, {len(papers)} papers; photo {photo.name}')
print(f'helper after catalog: {memory()} MB')

film, paper = 'kodak_gold_200', 'kodak_portra_endura'
main, ms = request({'command': 'render', 'input': str(photo), 'source_generation': 1,
                    'film': film, 'paper': paper, 'params': defaults,
                    'limit_preview': False, 'preview_path': str(scratch / 'main.rgba')})
print(f"main render {main['width']}x{main['height']} on {main['backend']}: {ms:.0f} ms; helper {memory()} MB")


def thumbnail(film, paper, positive, long_side, name, ev=0.0, crop=None):
    params = json.loads(json.dumps(defaults))
    params['camera']['exposure_compensation_ev'] = ev
    params['io']['scan_film'] = positive
    result, ms = request({'command': 'thumbnail', 'input': str(photo), 'source_generation': 1,
                          'film': film, 'paper': paper, 'params': params,
                          'long_side': long_side, 'crop': crop, 'preview_path': str(scratch / f'{name}.rgba')})
    return result, ms


half = {'x': 0.25, 'y': 0.25, 'width': 0.5, 'height': 0.5, 'angle': 0.0}
tilted = {'x': 0.1, 'y': 0.1, 'width': 0.8, 'height': 0.8, 'angle': 3.0}
passes = [(s, 0.0, 'cold', None) for s in sizes] + [(sizes[-1], 0.5, 'after an exposure edit', None),
          (sizes[-1], 0.5, 'crop 80% + 3deg', tilted), (sizes[-1], 0.5, 'crop 50%', half)]
for long_side, ev, note, crop in passes:
    for label, rows in (('films', [(f['id'], f['target'] if 'target' in f else (f['target_print'] or paper), f['positive']) for f in films]),
                        ('papers', [(film, p['id'], False) for p in papers])):
        times, parts = [], {}
        started = time.perf_counter()
        for i, (f, p, positive) in enumerate(rows):
            result, ms = thumbnail(f, p, positive, long_side, f'{label}-{long_side}-{ev}-{note[:4]}-{i}', ev, crop)
            times.append(ms)
            for k, v in result['timing_ms'].items():
                parts.setdefault(k, []).append(v)
        total = (time.perf_counter() - started) * 1000
        breakdown = ', '.join(f'{k} {statistics.median(v):.0f}' for k, v in parts.items())
        print(f"{label} @ {long_side}px {note} ({result['width']}x{result['height']}): total {total:.0f} ms, "
              f"per thumb median {statistics.median(times):.0f} / max {max(times):.0f} ms "
              f"[{breakdown}]; helper {memory()} MB")

# The retained main render must survive: Save uses it.
_, ms = request({'command': 'render', 'input': str(photo), 'source_generation': 1,
                 'film': film, 'paper': paper, 'params': defaults,
                 'limit_preview': False, 'preview_path': str(scratch / 'main2.rgba')})
print(f'main re-render after thumbnails (should be a cache hit): {ms:.0f} ms')
proc.stdin.close()
proc.wait()
print('thumbnails in', scratch)
