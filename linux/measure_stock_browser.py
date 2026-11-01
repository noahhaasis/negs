#!/usr/bin/env python3
"""Compares the stock dropdowns with the film/paper preview grids in the real app.

Usage: measure_stock_browser.py PHOTO [--seconds N]

Three launches on the same photo: the old dropdowns, the grids collapsed (the
default) and both grids open. Reports memory (PSS, private) of the GTK process
and the engine helper once everything has settled, the thumbnail batch time,
and a window capture of each.
"""
import argparse, json, os, re, subprocess, time
from pathlib import Path

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser()
parser.add_argument('photo', type=Path)
parser.add_argument('--seconds', type=float, default=14)
parser.add_argument('--out', type=Path, default=root / 'target/linux-validation/stock-browser')
args = parser.parse_args()
args.out.mkdir(parents=True, exist_ok=True)


def memory(pid):
    try:
        text = Path(f'/proc/{pid}/smaps_rollup').read_text()
    except OSError:
        return None
    kb = {m[1]: int(m[2]) for m in re.finditer(r'^(\w+):\s+(\d+) kB', text, re.M)}
    private = kb.get('Private_Clean', 0) + kb.get('Private_Dirty', 0)
    return {'rss_mb': kb['Rss'] / 1024, 'pss_mb': kb['Pss'] / 1024, 'private_mb': private / 1024}


def children(pid):
    # The helper is spawned from a worker thread: check every task.
    found = []
    for task in Path(f'/proc/{pid}/task').iterdir():
        try:
            found += [int(c) for c in (task / 'children').read_text().split()]
        except OSError:
            pass
    return found


# (environment, Settings → Film and paper previews)
variants = {
    'dropdown': ({}, False),
    'grid-collapsed': ({}, True),
    'grid-expanded': ({'NEGS_STOCK_EXPAND': 'film,paper'}, True),
}
results = {}
for name, (extra, previews) in variants.items():
    profile = args.out / f'profile-{name}'
    profile.mkdir(exist_ok=True)
    # A photo argument restores Settings but not the edit.
    (profile / 'session.json').write_text(json.dumps({'schema': 1, 'preferences': {'stock_previews': previews}}))
    env = dict(os.environ, NEGS_PROFILE_DIR=str(profile), NEGS_INSPECTOR_MODE='eager',
               NEGS_QUIT_AFTER_MS=str(int(args.seconds * 1000) + 3000),
               NEGS_WIDGET_CAPTURE=str(args.out / f'{name}.png'),
               NEGS_WIDGET_CAPTURE_MS=str(int(args.seconds * 1000)),
               NEGS_STOCK_THUMB_LOG='1', **extra)
    log = (args.out / f'{name}.log').open('w')
    p = subprocess.Popen(['bash', str(root / 'linux/run.sh'), str(args.photo.resolve())],
                         env=env, stdout=log, stderr=log)
    time.sleep(args.seconds + 1)
    gui = memory(p.pid)
    helpers = [memory(c) for c in children(p.pid)]
    p.wait(timeout=30)
    log.close()
    text = (args.out / f'{name}.log').read_text()
    batches = re.findall(r'stock thumbnails: (\d+) in (\d+) ms', text)
    results[name] = {'gui': gui, 'helper': helpers, 'thumbnail_batches': batches}
    print(name, json.dumps(results[name]))
(args.out / 'results.json').write_text(json.dumps(results, indent=2))
print('captures in', args.out)
