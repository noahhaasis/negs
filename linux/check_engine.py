#!/usr/bin/env python3
"""Run the opt-in GTK/engine audit in the actual Wayland desktop."""
import argparse
import json
import os
from pathlib import Path
import struct
import subprocess
import zlib

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out', type=Path, default=root / 'target/linux-validation/engine-ui-audit')
parser.add_argument('--raw', type=Path, help='Also develop a real RAW, change WB, and export with metadata')
args = parser.parse_args()
out = args.out.resolve()
if not out.is_relative_to(root / 'target'):
    raise SystemExit('Keep generated private validation files under target/.')
if not os.environ.get('WAYLAND_DISPLAY'):
    raise SystemExit('Run from the logged-in Wayland session.')
out.mkdir(parents=True, exist_ok=True)
for name in ['result.json', 'look.cube', 'preview.png', 'fast.png', 'reference.png',
             'quality35.jpg', 'quality95.jpg', 'cancelled.png']:
    (out / name).unlink(missing_ok=True)

def chunk(name, data):
    return struct.pack('>I', len(data)) + name + data + struct.pack('>I', zlib.crc32(name + data))

width, height = 192, 128
pixels = b''.join(b'\0' + bytes(v for x in range(width)
                    for v in (x * 255 // width, y * 255 // height, 100)) for y in range(height))
(out / 'source.png').write_bytes(b'\x89PNG\r\n\x1a\n'
    + chunk(b'IHDR', struct.pack('>2I5B', width, height, 8, 2, 0, 0, 0))
    + chunk(b'IDAT', zlib.compress(pixels)) + chunk(b'IEND', b''))
env = dict(os.environ, NEGS_PROFILE_DIR=str(out / 'profile'),
           NEGS_ENGINE_AUDIT=str(out / 'result.json'), NEGS_STARTUP_REPORT=str(out / 'startup.json'),
           NEGS_QUIT_AFTER_MS='95000')
env.pop('NEGS_UI_ONLY', None)
if args.raw:
    env['NEGS_RAW_AUDIT'] = str(args.raw.resolve())
with (out / 'app.log').open('w') as log:
    process = subprocess.Popen([root / 'target/linux-ui/release/negs', '--fresh'], cwd=root,
                               env=env, stdout=log, stderr=log)
    try:
        code = process.wait(timeout=105)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait()
        raise SystemExit('GTK engine audit timed out; inspect ' + str(out / 'app.log'))
report = json.loads((out / 'result.json').read_text()) if (out / 'result.json').exists() else {}
print(json.dumps(report, indent=2))
if code or report.get('status') != 'PASS':
    raise SystemExit('GTK engine audit failed; inspect ' + str(out / 'app.log'))
