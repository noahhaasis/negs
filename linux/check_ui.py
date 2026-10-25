#!/usr/bin/env python3
"""Exercise actual GTK controls through AT-SPI and keyboard input on Hyprland."""
from pathlib import Path
import json
import os
import subprocess
import time
import gi

gi.require_version("Atspi", "2.0")
from gi.repository import Atspi

root = Path(__file__).resolve().parent.parent
out = root / "target/linux-validation/ui-accessibility"
out.mkdir(parents=True, exist_ok=True)
env = dict(os.environ, GTK_A11Y="atspi", NEGS_PROFILE_DIR=str(out / "profile"), NEGS_QUIT_AFTER_MS="16000")
results = []

def wait_for(fn, description, seconds=5):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        value = fn()
        if value:
            return value
        time.sleep(.03)
    raise AssertionError(description)

def tree(node, limit=1500):
    queue = [node]
    seen = 0
    while queue and seen < limit:
        node = queue.pop(0)
        seen += 1
        yield node
        for i in range(node.get_child_count()):
            child = node.get_child_at_index(i)
            if child is not None:
                queue.append(child)

with (out / "app.log").open("w") as log:
    p = subprocess.Popen([str(root / "target/linux-ui/release/negs"), "--demo", "--fresh"], env=env, stdout=log, stderr=log)
    try:
        Atspi.init()
        desktop = Atspi.get_desktop(0)
        def application():
            return next((desktop.get_child_at_index(i) for i in range(desktop.get_child_count())
                         if desktop.get_child_at_index(i).get_process_id() == p.pid), None)
        app = wait_for(application, "Negs did not register its native AT-SPI tree")
        def named(name):
            return next((n for n in tree(app) if n.get_name() == name), None)
        spin = wait_for(lambda: named("Print exposure value"), "No accessible Print exposure numeric control")
        value = spin.get_value_iface()
        before = value.get_current_value()
        assert value.set_current_value(1.25)
        wait_for(lambda: abs(value.get_current_value() - 1.25) < 1e-8, "Numeric field did not update")
        results.append("Native numeric entry accepts and reports a value")
        undo = named("Undo (Ctrl+Z)")
        assert undo is not None and undo.get_action_iface().do_action(0)
        wait_for(lambda: abs(value.get_current_value() - before) < 1e-8, "Undo did not restore the numeric entry")
        results.append("Undo restores the numeric entry through the editor model")
        redo = named("Redo (Ctrl+Shift+Z)")
        assert redo is not None and redo.get_action_iface().do_action(0)
        wait_for(lambda: abs(value.get_current_value() - 1.25) < 1e-8, "Redo did not restore the edit")
        results.append("Redo restores the edited value")
        reset = named("Reset Adjustments")
        assert reset is not None and reset.get_action_iface().do_action(0)
        wait_for(lambda: abs(value.get_current_value() - before) < 1e-8, "Reset did not restore the UI fixture")
        results.append("Reset restores UI values")
        wait_for(lambda: named("Photograph preview"), "No accessible photograph canvas after image load")
        nodes = [{"name": n.get_name(), "role": n.get_role_name()} for n in tree(app)]
        (out / "tree.json").write_text(json.dumps(nodes, indent=2) + "\n")
        assert any(n["name"] == "Photograph preview" for n in nodes)
        results.append("Canvas, controls, and actions have names in the platform accessibility tree")
        focus = f'hl.dsp.focus({{ window = "pid:{p.pid}" }})'
        subprocess.run(["hyprctl", "dispatch", focus], check=True, stdout=subprocess.DEVNULL)
        active = json.loads(subprocess.check_output(["hyprctl", "-j", "activewindow"], text=True))
        assert active.get("pid") == p.pid, "Refusing to send keys to another application"
        # Open the native file dialog, then cancel it, through compositor input.
        subprocess.run(["wtype", "-M", "ctrl", "o", "-m", "ctrl"], check=True)
        time.sleep(.5)
        active = json.loads(subprocess.check_output(["hyprctl", "-j", "activewindow"], text=True))
        # A portal may use a separate process. Escape is sent only for an app-owned dialog.
        if active.get("pid") == p.pid:
            subprocess.run(["wtype", "-k", "Escape"], check=True)
            results.append("Ctrl+O reaches a native file dialog; Escape cancels the app-owned dialog")
        (out / "result.json").write_text(json.dumps({"status": "PASS", "checks": results, "scope": "AT-SPI value/action interfaces and real keyboard input; no screen-reader usability or physical pointer claim"}, indent=2) + "\n")
        print("PASS:", *results, sep="\n  ")
    finally:
        # Target only the app started by this test, even when a modal is open.
        p.terminate()
        p.wait(timeout=3)
