"""Bound GUI regression fixtures to their own visible test window."""
import json
import subprocess
import time


def prepare_window(process, width=1320, height=900):
    def hypr(*args):
        return subprocess.check_output(['hyprctl', *args], text=True)
    deadline = time.monotonic() + 6
    while process.poll() is None and time.monotonic() < deadline:
        clients = json.loads(hypr('-j', 'clients'))
        if any(c.get('pid') == process.pid for c in clients):
            selector = json.dumps(f'pid:{process.pid}')
            hypr('dispatch', f'hl.dsp.window.float({{action="set",window={selector}}})')
            hypr('dispatch', f'hl.dsp.window.resize({{x={width},y={height},window={selector}}})')
            hypr('dispatch', f'hl.dsp.window.center({{window={selector}}})')
            hypr('dispatch', f'hl.dsp.focus({{window={selector}}})')
            return
        time.sleep(.02)
    raise RuntimeError('Test window did not map')
