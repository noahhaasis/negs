#!/usr/bin/env python3
"""Exercise new GTK presentation controls and GIO folder invalidation on Wayland."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import time
import gi

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
root = Path(__file__).resolve().parent.parent
out = root / 'target/linux-validation/ui-controls'
photos = out / 'photos'
photos.mkdir(parents=True, exist_ok=True)
for path in photos.iterdir():
    path.unlink()
for path in (root / 'target/linux-validation/ui-demo').glob('*.jpg'):
    shutil.copyfile(path, photos / path.name)
report = out / 'native-controls.json'
report.unlink(missing_ok=True)
env = dict(os.environ, GTK_A11Y='atspi', NEGS_PROFILE_DIR=str(out / 'profile'),
           NEGS_UI_AUDIT=str(report), NEGS_INSPECTOR_MODE='eager', NEGS_QUIT_AFTER_MS='22000')

def wait_for(fn, description, seconds=6):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = fn()
        if value:
            return value
        time.sleep(.03)
    raise AssertionError(description)

def tree(node):
    pending = [node]
    while pending:
        node = pending.pop()
        yield node
        pending.extend(node.get_child_at_index(i) for i in range(node.get_child_count()) if node.get_child_at_index(i))

with (out / 'app.log').open('w') as log:
    p = subprocess.Popen([str(root / 'target/linux-ui/release/negs'), '--fresh', str(photos / 'DSC_4990.jpg')], env=env, stdout=log, stderr=log)
    try:
        wait_for(report.exists, 'Native GTK control audit failed; see app.log')
        assert json.loads(report.read_text())['status'] == 'PASS'
        Atspi.init()
        desktop = Atspi.get_desktop(0)
        app = wait_for(lambda: next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count()) if desktop.get_child_at_index(i).get_process_id() == p.pid), None), 'No AT-SPI application')
        def named(name):
            return next((n for n in tree(app) if n.get_name() == name), None)
        def buttons():
            return {n.get_name(): n for n in tree(app) if n.get_role() == Atspi.Role.PUSH_BUTTON and n.get_name().endswith('.jpg')}
        before = wait_for(lambda: (b if len(b := buttons()) == 6 else None), 'Folder did not populate')
        stable_path = before['DSC_4997.jpg'].path
        # Native file monitor notices add, rename and delete without rebuilding unaffected cells.
        added = photos / 'photo10.jpg'
        shutil.copyfile(photos / 'DSC_5003.jpg', added)
        wait_for(lambda: 'photo10.jpg' in buttons(), 'GIO add event did not update the strip')
        added.rename(photos / 'photo2.jpg')
        wait_for(lambda: 'photo2.jpg' in buttons() and 'photo10.jpg' not in buttons(), 'GIO rename event did not update the strip')
        (photos / 'photo2.jpg').unlink()
        wait_for(lambda: len(buttons()) == 6, 'GIO delete event did not update the strip')
        assert buttons()['DSC_4997.jpg'].path == stable_path
        # Changing a file in place updates its visible thumbnail and current camera preview.
        spin = named('Print exposure value').get_value_iface()
        assert spin.set_current_value(1.25)
        replacement = out / 'replacement.jpg'
        subprocess.run(['magick', '-size', '640x480', 'xc:#7c3452', str(replacement)], check=True)
        shutil.copyfile(replacement, photos / 'DSC_4990.jpg')
        wait_for(lambda: named('640 × 480 px'), 'Modified selected source did not reload its camera preview')
        wait_for(lambda: any(n.get_description() == 'Source 640 × 480 pixels' for n in tree(app)), 'Visible modified thumbnail did not invalidate')
        assert abs(spin.get_current_value() - 1.25) < 1e-8, 'External source refresh reset the current edit'
        checks = json.loads(report.read_text())['checks'] + ['Native folder notifications handle add/rename/delete while preserving unaffected cells', 'In-place source modification refreshes visible thumbnail and camera preview while preserving edits']
        (out / 'result.json').write_text(json.dumps(dict(status='PASS', checks=checks), indent=2) + '\n')
        print('PASS:', *checks, sep='\n  ')
    finally:
        if p.poll() is None:
            p.terminate()
        p.wait(timeout=3)
