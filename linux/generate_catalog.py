#!/usr/bin/env python3
"""Extract UI catalog metadata only; no profile curves or processing defaults.

Derived metadata retains the source profiles' CC BY-SA attribution. The full
profiles and licensing are shipped in data/; regenerate after catalog changes.
"""
import json
from pathlib import Path
root = Path(__file__).resolve().parent.parent
stocks = []
for path in (root / 'data/profiles').glob('*.json'):
    profile = json.loads(path.read_text())
    info = profile['info']
    paper = info['support'] == 'paper' or info['stage'] == 'printing'
    bw = info.get('channel_model') == 'bw'
    cinema = paper and ('Vision' in info['name'] or 'Print Film' in info['name'])
    group = ('Black & White' if bw else 'Motion Picture Print' if cinema else 'Color Paper') if paper else ('Black & White' if bw else 'Color Slide' if info['type'] == 'positive' else 'Motion Picture' if info.get('target_print') in ('kodak_2383', 'kodak_2393', 'kodak_2302') else 'Color Negative')
    stocks.append(dict(id=path.stem, name=info['name'], paper=paper, positive=info['type'] == 'positive', bw=bw, group=group, target=info.get('target_print') or '', times=profile['data'].get('development_time') or []))
order = ['Color Negative', 'Color Slide', 'Motion Picture', 'Color Paper', 'Motion Picture Print', 'Black & White']
stocks.sort(key=lambda s: (s['paper'], order.index(s['group']), s['name'].lower()))
(root / 'linux/catalog.json').write_text(json.dumps({'attribution': 'Profile metadata derived from Spektrafilm profiles by Andrea Volpato and contributors, CC BY-SA 4.0. See data/profiles and SPEKTRAFILM_LICENSE.txt. Metadata extraction only; no modifications to profile values.', 'profiles': stocks}, indent=2) + '\n')
