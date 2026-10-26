#!/usr/bin/env python3
"""Installed single-instance activation, native save picker and child shutdown."""
import json
import os
from pathlib import Path
import subprocess
import time
import gi
from desktop_test import prepare_window

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi
root = Path(__file__).resolve().parent.parent
out = root / 'target/linux-validation/installed-desktop'
out.mkdir(parents=True, exist_ok=True)
binary = Path.home() / '.local/bin/negs'
output = out / 'native-picker.png'
output.unlink(missing_ok=True)
env = dict(os.environ, XDG_CONFIG_HOME=str(out / 'config'), XDG_CACHE_HOME=str(out / 'cache'),
           GTK_A11Y='atspi', NEGS_QUIT_AFTER_MS='45000', SPEKTRAFILM_ROOT='/tmp/negs-no-checkout-fallback')
env.pop('NEGS_PROFILE_DIR', None)

def wait(fn, label, seconds=10):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        found = fn()
        if found:
            return found
        time.sleep(.05)
    raise AssertionError(label)

def tree(node):
    pending = [node]
    while pending:
        node = pending.pop()
        yield node
        pending.extend(node.get_child_at_index(i) for i in range(node.get_child_count()) if node.get_child_at_index(i))

def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)

def send(*keys):
    assert json.loads(hypr('-j', 'activewindow')).get('pid') == process.pid
    subprocess.run(['wtype', *keys], check=True)

checks = []
with (out / 'app.log').open('w') as log:
    process = subprocess.Popen([binary, root / 'target/linux-validation/ui-demo/DSC_4990.jpg'], env=env, cwd=out, stdout=log, stderr=log)
    try:
        prepare_window(process)
        Atspi.init()
        desktop = Atspi.get_desktop(0)
        app = wait(lambda: next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count()) if desktop.get_child_at_index(i).get_process_id() == process.pid), None), 'No accessibility tree')
        def named(name, role=None):
            return next((n for n in tree(app) if n.get_name() == name and (role is None or n.get_role() == role)), None)
        def developed():
            return any('Developed in' in n.get_name() and 'wgpu (GPU)' in n.get_name() for n in tree(app))
        wait(developed, 'JPEG did not develop on hardware Vulkan')
        for source in [root / 'target/linux-validation/inputs/photo_DSC_4990.tif', root / 'target/linux-validation/engine-ui-final/source.png']:
            result = subprocess.run([binary, source], env=env, cwd=out, capture_output=True, timeout=5)
            assert result.returncode == 0
            wait(lambda: any(n.get_role() == Atspi.Role.FRAME and n.get_name().startswith(source.name + ' —') for n in tree(app)), 'File-open activation did not reach the existing window')
            wait(developed, 'Activated photo did not develop')
            clients = json.loads(hypr('-j', 'clients'))
            assert [c['pid'] for c in clients if c.get('class') == 'dev.negs.app'] == [process.pid]
        checks.append('Installed JPEG, float TIFF and PNG open; GApplication forwards activation to one window')
        send('-M', 'ctrl', 's', '-m', 'ctrl')
        save = wait(lambda: named('Save Preview', Atspi.Role.PUSH_BUTTON), 'No native Save Preview options')
        assert save.get_action_iface().do_action(0)
        wait(lambda: named('Save', Atspi.Role.PUSH_BUTTON), 'No GTK native save picker')
        send('-M', 'ctrl', 'l', '-m', 'ctrl')
        subprocess.run(['wtype', str(output)], check=True)
        send('-k', 'Return')
        time.sleep(.2)
        if not output.exists() and (save := named('Save', Atspi.Role.PUSH_BUTTON)):
            assert save.get_action_iface().do_action(0)
        wait(lambda: output.exists() and output.stat().st_size > 100, 'Native picker did not save real pixels')
        checks.append('Ctrl+S and the actual GTK save picker write a real preview PNG')
        helper_pids = []
        for child in Path('/proc').iterdir():
            if not child.name.isdigit():
                continue
            try:
                executable = (child / 'exe').resolve()
                if executable == Path.home() / '.local/lib/negs/spektrafilm-native':
                    helper_pids.append(int(child.name))
            except OSError:
                pass
        assert helper_pids
        send('-M', 'ctrl', 'q', '-m', 'ctrl')
        process.wait(timeout=8)
        wait(lambda: all(not Path(f'/proc/{pid}').exists() for pid in helper_pids), 'Quit left a helper process behind')
        checks.append('Graceful quit persists the session and reaps the installed helper')
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=4)
report = dict(status='PASS', checks=checks)
(out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
