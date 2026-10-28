#!/usr/bin/env python3
"""Summarize run.py results: medians per configuration and image, as Markdown.

Usage: summarize.py RESULTS_JSON [SUMMARY_JSON]
Prints one table per image (median wall time, engine time and peak footprint,
with min–max ranges) and optionally writes the medians as JSON.
"""
import json
from statistics import median
import sys

GIB = 2 ** 30
IMAGES = {'nikon-z6': 'Nikon Z 6, 24.5 MP', 'leica-q3': 'Leica Q3, 60.5 MP'}

results = json.load(open(sys.argv[1]))
summary = {}
for image in dict.fromkeys(run['image'] for run in results['runs']):
    print(f'\n### {IMAGES.get(image, image)}\n')
    print('| Engine | Path | Wall time | Engine time | Peak memory | Swapped out |')
    print('|---|---|---:|---:|---:|---:|')
    for config in results['configs']:
        runs = [r for r in results['runs'] if r['config'] == config['id'] and r['image'] == image]
        if not runs:
            continue
        wall = [r['wall_s'] for r in runs]
        engine = [r['pipeline_s'] for r in runs if r['pipeline_s'] is not None]
        peak = [r['peak_footprint_bytes'] / GIB for r in runs]
        swapped = max(r['swapouts'] or 0 for r in runs) * 16384 / GIB
        summary.setdefault(image, {})[config['id']] = dict(
            runs=len(runs), wall_s=median(wall), wall_range=[min(wall), max(wall)],
            engine_s=median(engine) if engine else None,
            peak_gib=median(peak), peak_range=[min(peak), max(peak)], max_swapped_out_gib=swapped)
        print(f'| {config["engine"]} | {config["path"]} | {median(wall):.1f} s ({min(wall):.1f}–{max(wall):.1f}) '
              f'| {median(engine):.1f} s | {median(peak):.1f} GiB ({min(peak):.1f}–{max(peak):.1f}) '
              f'| {swapped:.1f} GiB |')
if len(sys.argv) > 2:
    json.dump(dict(host=results['host'], started=results['started'], configs=results['configs'], medians=summary, runs=results['runs']),
              open(sys.argv[2], 'w'), indent=2)
