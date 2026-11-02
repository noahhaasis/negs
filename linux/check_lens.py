#!/usr/bin/env python3
"""Check the Lens corrections switch in the real GTK app on Wayland.

Opens a RAW with embedded corrections (Sony ARW, Fujifilm RAF or a DNG with
opcodes), one without (--plain, e.g. a Nikon NEF) and a JPEG, then checks
that the switch shows only for the first, starts on, re-renders when turned
off, and is saved in the session.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time

import gi
from desktop_test import prepare_window

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--raw', type=Path, required=True, help='RAW with embedded lens corrections')
parser.add_argument('--plain', type=Path, required=True, help='RAW without lens correction data')
parser.add_argument('--out', type=Path, default=root / 'target/linux-validation/lens')
args = parser.parse_args()
out = args.out.resolve()
if not out.is_relative_to(root / 'target'):
    raise SystemExit('Keep generated private validation files under target/.')
if not os.environ.get('WAYLAND_DISPLAY'):
    raise SystemExit('Run from the logged-in Wayland session.')
shutil.rmtree(out, ignore_errors=True)
photos = out / 'photos'
photos.mkdir(parents=True)
raw = photos / ('a-corrected' + args.raw.suffix.lower())
plain = photos / ('b-plain' + args.plain.suffix.lower())
jpeg = photos / 'c-photo.jpg'
for src, dst in [(args.raw, raw), (args.plain, plain)]:
    os.symlink(src.resolve(), dst)
subprocess.run(['magick', '-size', '640x480', 'gradient:#335577-#ddccaa', str(jpeg)], check=True)
env = dict(os.environ, GTK_A11Y='atspi', NEGS_PROFILE_DIR=str(out / 'profile'),
           NEGS_INSPECTOR_MODE='eager', NEGS_QUIT_AFTER_MS='170000')
env.pop('NEGS_UI_ONLY', None)


def wait_for(fn, description, seconds=60):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = fn()
        if value:
            return value
        time.sleep(.1)
    raise AssertionError(description)


def tree(node):
    pending = [node]
    while pending:
        node = pending.pop()
        yield node
        pending.extend(c for i in range(node.get_child_count()) if (c := node.get_child_at_index(i)))


def screenshot(pid, path):
    clients = json.loads(subprocess.check_output(['hyprctl', '-j', 'clients'], text=True))
    c = next(c for c in clients if c.get('pid') == pid)
    (x, y), (w, h) = c['at'], c['size']
    subprocess.run(['grim', '-g', f'{x},{y} {w}x{h}', str(path)], check=True)


checks = []
with (out / 'app.log').open('w') as log:
    p = subprocess.Popen([str(root / 'target/linux-ui/release/negs'), '--fresh', str(raw)],
                         env=env, stdout=log, stderr=log)
    prepare_window(p, 1500, 1000)
    try:
        Atspi.init()
        desktop = Atspi.get_desktop(0)
        app = wait_for(lambda: next((a for i in range(desktop.get_child_count())
                                     if (a := desktop.get_child_at_index(i)).get_process_id() == p.pid), None),
                       'No AT-SPI application')

        def row():
            def actionable(n):
                a = n.get_action_iface()
                return a is not None and a.get_n_actions() > 0
            return next((n for n in tree(app) if n.get_name() == 'Lens corrections'
                         and n.get_role() != Atspi.Role.LABEL and actionable(n)), None)

        def shown():
            r = row()
            return r if r and r.get_state_set().contains(Atspi.StateType.VISIBLE) else None

        def active(r):
            return r.get_state_set().contains(Atspi.StateType.CHECKED)

        def status():
            return ' | '.join(n.get_name() for n in tree(app) if n.get_role() == Atspi.Role.LABEL
                              and ('Loading' in n.get_name() or 'Rendering' in n.get_name()))

        switch = wait_for(shown, 'The switch did not appear for a RAW with corrections', 120)
        assert active(switch), 'Lens corrections should start on for a newly opened photo'
        time.sleep(4)
        screenshot(p.pid, out / 'on.png')
        # Scroll the inspector until the switch is on screen, for a screenshot.
        for bar in (n for n in tree(app) if n.get_role() == Atspi.Role.SCROLL_BAR):
            value = bar.get_value_iface()
            if not value or value.get_maximum_value() <= 0:
                continue
            target = value.get_minimum_value()
            while target <= value.get_maximum_value() and not row().get_state_set().contains(Atspi.StateType.SHOWING):
                value.set_current_value(target)
                target += 60
                time.sleep(.05)
            if row().get_state_set().contains(Atspi.StateType.SHOWING):
                value.set_current_value(min(value.get_maximum_value(), value.get_current_value() + 200))
                time.sleep(1)
                screenshot(p.pid, out / 'switch.png')
                break
        checks.append(f'{raw.name}: switch shown and on')

        actions = switch.get_action_iface()
        assert actions and actions.do_action(0), 'Could not toggle the switch'
        wait_for(lambda: not active(row()), 'The switch did not turn off')
        time.sleep(8)
        screenshot(p.pid, out / 'off.png')
        checks.append(f'{raw.name}: switch turned off through AT-SPI')

        def open_photo(name):
            button = wait_for(lambda: next((n for n in tree(app) if n.get_role() == Atspi.Role.PUSH_BUTTON
                                            and n.get_name() == name), None), f'No strip button for {name}')
            assert button.get_action_iface().do_action(0)

        open_photo(plain.name)
        wait_for(lambda: not shown(), f'The switch stayed visible for {plain.name}', 120)
        time.sleep(6)
        assert not shown(), f'The switch appeared for {plain.name}'
        checks.append(f'{plain.name}: switch hidden (no correction data)')
        open_photo(jpeg.name)
        time.sleep(8)
        assert not shown(), 'The switch appeared for a JPEG'
        checks.append(f'{jpeg.name}: switch hidden')
        open_photo(raw.name)
        wait_for(shown, 'The switch did not come back for the corrected RAW', 120)
        assert active(row()), 'Reopening a photo turned off earlier should start with corrections on'
        checks.append(f'{raw.name} reopened after it was turned off: switch shown and on again')
        actions = row().get_action_iface()
        assert actions.do_action(0)
        wait_for(lambda: not active(row()), 'The switch did not turn off')
        time.sleep(2)
    finally:
        if p.poll() is None:
            subprocess.run(['hyprctl', 'dispatch', f'hl.dsp.window.close({{window="pid:{p.pid}"}})'],
                           stdout=subprocess.DEVNULL)
            try:
                p.wait(timeout=10)
            except subprocess.TimeoutExpired:
                print('The window did not close; terminating')
                p.terminate()
        p.wait(timeout=5)

session = json.loads((out / 'profile/session.json').read_text())
assert session['schema'] == 3, session['schema']
assert session['state']['choices'].get('Lens corrections') == 'false', session['state']['choices']
checks.append('The session saves the switch (schema 3)')
on, off = out / 'on.png', out / 'off.png'
# Normalised mean absolute difference of the whole window: the switch and
# status text alone stay far below the threshold.
diff = subprocess.run(['magick', 'compare', '-metric', 'MAE', str(on), str(off), 'null:'],
                      capture_output=True, text=True).stderr.split('(')[1].rstrip(')')
assert float(diff) > 0.002, f'Turning lens corrections off barely changed the preview ({diff})'
checks.append(f'Preview changed when turned off (mean difference {float(diff):.4f})')
(out / 'result.json').write_text(json.dumps(dict(status='PASS', checks=checks), indent=2) + '\n')
print('PASS:', *checks, sep='\n  ')
