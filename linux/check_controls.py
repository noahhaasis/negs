#!/usr/bin/env python3
"""Exercise new GTK presentation controls and GIO folder invalidation on Wayland."""
from pathlib import Path
import json
import os
import shutil
import subprocess
import time
import gi
from desktop_test import prepare_window

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
env = dict(os.environ, NEGS_UI_ONLY="1", GTK_A11Y='atspi', NEGS_PROFILE_DIR=str(out / 'profile'),
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
    prepare_window(p)
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
        # A fresh edit starts on Gold 200, whose grain preset the lanes show.
        granularity = named('Red granularity value').get_value_iface()
        assert abs(granularity.get_current_value() - 5.) < 1e-9, 'Grain lane does not show the Gold 200 preset'
        assert granularity.set_current_value(21.5)
        wait_for(lambda: abs(granularity.get_current_value() - 21.5) < 1e-9, 'Grain lane did not update')
        reset = wait_for(lambda: named('Reset Red granularity'), 'No per-control reset for the grain lane')
        assert reset.get_action_iface().do_action(0)
        wait_for(lambda: abs(granularity.get_current_value() - 5.) < 1e-9, 'Reset did not return the lane to the preset')
        assert granularity.set_current_value(21.5)
        tilt = named('Tilt value').get_value_iface()
        assert tilt.set_current_value(-.3)
        wait_for(lambda: abs(tilt.get_current_value() + .3) < 1e-9, 'Film Base tilt did not update')
        balance = named('Balance Print')
        assert balance is not None and not balance.get_state_set().contains(Atspi.StateType.SENSITIVE)
        checks.append('Grain preset lane, its reset button, Film Base and the Balance Print button work through the platform accessibility tree')
    finally:
        if p.poll() is None:
            subprocess.run(['hyprctl', 'dispatch', f'hl.dsp.window.close({{window="pid:{p.pid}"}})'], check=True, stdout=subprocess.DEVNULL)
            try:
                p.wait(timeout=4)
            except subprocess.TimeoutExpired:
                p.terminate()
        p.wait(timeout=3)
values = json.loads((out / 'profile/session.json').read_text())['state']['values']
assert values['Grain.Red granularity'] == 21.5 and values['Film Base.Tilt'] == -.3
assert 'Grain.Green granularity' not in values, 'Unedited lanes must stay unset'
env.pop('NEGS_UI_AUDIT')
with (out / 'app.log').open('a') as log:
    p = subprocess.Popen([str(root / 'target/linux-ui/release/negs')], env=env, stdout=log, stderr=log)
    prepare_window(p)
    try:
        desktop = Atspi.get_desktop(0)
        app = wait_for(lambda: next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count()) if desktop.get_child_at_index(i).get_process_id() == p.pid), None), 'No AT-SPI application')
        granularity = wait_for(lambda: named('Red granularity value'), 'Restored inspector has no grain lane').get_value_iface()
        assert abs(granularity.get_current_value() - 21.5) < 1e-9, 'Grain lane edit was not restored'
        assert abs(named('Tilt value').get_value_iface().get_current_value() + .3) < 1e-9, 'Film Base edit was not restored'
        assert abs(named('Green granularity value').get_value_iface().get_current_value() - 5.) < 1e-9
        assert named('Reset Adjustments').get_action_iface().do_action(0)
        wait_for(lambda: abs(granularity.get_current_value() - 5.) < 1e-9, 'Reset Adjustments did not restore the preset')
        checks.append('Grain lane and Film Base edits persist in the session; Reset Adjustments returns the lane to its preset')
        (out / 'result.json').write_text(json.dumps(dict(status='PASS', checks=checks), indent=2) + '\n')
        print('PASS:', *checks, sep='\n  ')
    finally:
        if p.poll() is None:
            p.terminate()
        p.wait(timeout=3)
