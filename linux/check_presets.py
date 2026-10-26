#!/usr/bin/env python3
"""Exercise the Presets panel on the desktop: real engine, AT-SPI actions and
keyboard input, with screenshots. Uses its own NEGS_PROFILE_DIR, so the
user's session and presets stay untouched.

Local test presets (NEGS_PRESETS_TEST_DIR, else ~/Work/negs-presets-test) are
copied into the test profile when present. The community files there are
unlicensed: they are used here only and never committed.
"""
from pathlib import Path
import json
import os
import shutil
import subprocess
import time
import gi
from desktop_test import prepare_window

gi.require_version('Atspi', '2.0')
from gi.repository import Atspi, GLib

root = Path(__file__).resolve().parent.parent
out = root / 'target/linux-validation/presets'
profile = out / 'profile'
folder = profile / 'presets'
shutil.rmtree(profile, ignore_errors=True)
out.mkdir(parents=True, exist_ok=True)
for shot in out.glob('*.png'):
    shot.unlink()
photo = root / 'target/linux-validation/ui-demo/DSC_4990.jpg'
local = Path(os.environ.get('NEGS_PRESETS_TEST_DIR', Path.home() / 'Work/negs-presets-test'))
env = dict(os.environ, GTK_A11Y='atspi', NEGS_PROFILE_DIR=str(profile), SPEKTRAFILM_ROOT=str(root),
           NEGS_DATA_DIR=str(root / 'data'), NEGS_APPEARANCE='dark')
binary = root / 'target/linux-ui/release/negs'
checks = []


def wait(fn, description, seconds=20):
    until = time.monotonic() + seconds
    while time.monotonic() < until:
        value = fn()
        if value:
            return value
        time.sleep(.05)
    raise AssertionError(description)


def tree(node):
    pending = [node]
    while pending:
        node = pending.pop()
        yield node
        pending.extend(c for i in range(node.get_child_count()) if (c := node.get_child_at_index(i)) is not None)


def hypr(*args):
    return subprocess.check_output(['hyprctl', *args], text=True)


class App:
    def __init__(self, *args):
        self.log = (out / 'app.log').open('a')
        self.process = subprocess.Popen([binary, *args], env=env, stdout=self.log, stderr=self.log)
        prepare_window(self.process)
        Atspi.init()
        self.desktop = Atspi.get_desktop(0)
        wait(self.application, 'No accessibility tree')

    def application(self):
        d = self.desktop
        return next((d.get_child_at_index(i) for i in range(d.get_child_count())
                     if d.get_child_at_index(i).get_process_id() == self.process.pid), None)

    def nodes(self):
        app = self.application()
        return list(tree(app)) if app else []

    def named(self, name, role=None):
        return next((n for n in self.nodes() if n.get_name() == name and (role is None or n.get_role() == role)), None)

    def starting(self, prefix):
        return next((n for n in self.nodes() if (n.get_name() or '').startswith(prefix)), None)

    def rows(self):
        presets = self.named('Presets', Atspi.Role.LIST)
        if presets is None:
            return []
        items = [presets.get_child_at_index(i) for i in range(presets.get_child_count())]
        return [n.get_name() for n in items if n is not None and n.get_role() == Atspi.Role.LIST_ITEM and n.get_state_set().contains(Atspi.StateType.SHOWING)]

    def act(self, node, name):
        action = node.get_action_iface()
        index = next(i for i in range(action.get_n_actions()) if action.get_action_name(i) == name)
        assert action.do_action(index), name

    def row_action(self, row, name):
        node = wait(lambda: self.named(row, Atspi.Role.LIST_ITEM), f'No preset row {row}')
        self.act(node, name)

    def send(self, *keys):
        selector = json.dumps(f'pid:{self.process.pid}')
        hypr('dispatch', f'hl.dsp.focus({{window={selector}}})')
        active = json.loads(hypr('-j', 'activewindow'))
        assert active.get('pid') == self.process.pid, 'Refusing to send keys to another application'
        if keys:
            subprocess.run(['wtype', *keys], check=True)
            time.sleep(.15)

    def shortcut(self, mods, key):
        # wtype's one-level keymap sends Ctrl+Shift+s as keyval s, which GTK 4
        # doesn't match to <Shift>s; Hyprland sends it through the real keymap.
        self.send()
        selector = json.dumps(f'pid:{self.process.pid}')
        hypr('dispatch', f'hl.dsp.send_shortcut({{mods="{mods}",key="{key}",window={selector}}})')
        time.sleep(.15)

    def portal(self, title):
        """The GTK portal's picker titled `title`, and its accessibility tree."""
        picker = wait(lambda: next((c for c in json.loads(hypr('-j', 'clients')) if c.get('title') == title
                                    and c.get('class') == 'xdg-desktop-portal-gtk'), None), f'No {title} picker')
        d = self.desktop
        portal = wait(lambda: next((d.get_child_at_index(i) for i in range(d.get_child_count())
                                    if d.get_child_at_index(i).get_process_id() == picker['pid']), None), 'No portal tree')
        return picker, portal

    def save_as(self, title, button, suggested, path):
        """Replaces the save picker's `suggested` name with `path` and presses `button`."""
        _, portal = self.portal(title)
        def text(node):
            try:
                return Atspi.Text.get_text(node, 0, -1)
            except GLib.GError:
                return ''
        entry = wait(lambda: next((n for n in tree(portal) if n.get_role() == Atspi.Role.TEXT and text(n) == suggested), None),
                     f'No file name field in {title}')
        assert entry.get_editable_text_iface().set_text_contents(str(path))
        accept = next(n for n in tree(portal) if n.get_name() == button and n.get_role() == Atspi.Role.PUSH_BUTTON)
        self.act(accept, 'click')

    def shot(self, name):
        time.sleep(.5)
        client = next(c for c in json.loads(hypr('-j', 'clients')) if c.get('pid') == self.process.pid)
        (x, y), (w, h) = client['at'], client['size']
        subprocess.run(['grim', '-g', f'{x},{y} {w}x{h}', str(out / f'{name}.png')], check=True)

    def developed(self):
        return self.starting('Developed in')

    def quit(self):
        self.send('-M', 'ctrl', 'q', '-m', 'ctrl')
        self.process.wait(timeout=10)

    def close(self):
        if self.process.poll() is None:
            self.process.terminate()
            self.process.wait(timeout=4)


def files():
    return sorted(p.name for p in folder.glob('*.json') if not p.name.startswith('.')) if folder.is_dir() else []


(out / 'app.log').write_text('')
app = App('--fresh', str(photo))
try:
    wait(app.developed, 'The photograph did not develop', 60)
    app.send('-k', 'F8')
    wait(lambda: app.named('No Presets'), 'F8 did not show the empty Presets page')
    app.shot('1-empty')
    checks.append('F8 shows the Presets page; an empty folder shows the empty state with Import')

    # New Preset names the file after the film, at the bottom, renaming it.
    app.shortcut('CTRL SHIFT', 'S')
    wait(lambda: files() == ['Kodak Portra 400.json'], 'Ctrl+Shift+S did not save a preset named after the film')
    wait(lambda: app.named('Preset name', Atspi.Role.TEXT), 'New Preset did not start renaming')
    app.shot('2-renaming')
    app.send('Mine')
    app.send('-k', 'Return')
    wait(lambda: files() == ['Mine.json'], 'Enter did not keep the typed name')
    checks.append('Ctrl+Shift+S saves the film, paper and Lab settings named after the film; Enter keeps the typed name')
    app.shortcut('CTRL SHIFT', 'S')
    wait(lambda: 'Kodak Portra 400.json' in files(), 'Second New Preset did not save')
    wait(lambda: app.named('Preset name', Atspi.Role.TEXT), 'Second New Preset did not start renaming')
    assert app.rows()[-1] == 'Kodak Portra 400', app.rows()
    app.send('-k', 'Escape')
    wait(lambda: files() == ['Mine.json'], 'Escape did not remove the new preset')
    checks.append('New presets go to the bottom; Escape while naming one removes it')

    # Files copied in by hand appear when the panel opens again; refused files are listed with the reason.
    folder.joinpath('00 Refused.json').write_text('{"output_gamut_compress": {"algorithm": "jzazbz"}}')
    copied = sorted(local.glob('*.json')) if local.is_dir() else []
    for path in copied:
        shutil.copyfile(path, folder / path.name)
    app.send('-k', 'F8')
    app.send('-k', 'F8')
    wait(lambda: app.starting('The preset uses jzazbz'), 'The refused file has no reason')
    wait(lambda: len(app.rows()) >= min(2 + len(copied), 8), 'Copied files were not listed')
    assert app.rows()[:2] == ['Mine', '00 Refused'], app.rows()
    app.shot('3-full')
    checks.append(f'Reopening the panel lists files copied in by hand ({len(copied)} local test presets) after the ordered ones; a refused file shows its reason')

    # Applying: one undo step, redo; the Adjustments page shows the preset's film.
    target = next((p.stem for p in copied if p.stem.startswith('02 ')), None)
    if target is None:
        folder.joinpath('Double-X.json').write_text('{"simulation": {"film_stock": "kodak_doublex", "print_paper": "kodak_2302"}}')
        app.send('-k', 'F8')
        app.send('-k', 'F8')
        target = 'Double-X'
    app.row_action(target, 'preset.apply')
    wait(lambda: app.named(f'Applied preset “{target}”'), 'Applying did not report the preset')
    wait(app.developed, 'The preset did not develop', 60)
    app.shot('4-applied')
    film = 'Kodak Portra 800 (Push 2)' if target.startswith('02 ') else 'Kodak Double-X 5222'
    app.send('-k', 'F7')
    wait(lambda: app.named(film), 'The Adjustments page does not show the preset’s film')
    app.shot('5-applied-adjustments')
    app.send('-M', 'ctrl', 'z', '-m', 'ctrl')
    wait(lambda: app.named('Kodak Portra 400') and not app.named(film), 'Undo did not restore the film')
    app.shortcut('CTRL SHIFT', 'Z')
    wait(lambda: app.named(film), 'Redo did not apply the preset again')
    checks.append(f'Applying “{target}” develops it; Ctrl+Z restores the film, Ctrl+Shift+Z applies it again')

    # An invalid name keeps the old one and says why.
    app.send('-k', 'F8')
    app.row_action('Mine', 'preset.rename')
    wait(lambda: app.named('Preset name', Atspi.Role.TEXT), 'Rename did not start editing')
    app.send('a/b')
    app.send('-k', 'Return')
    wait(lambda: app.named('“a/b” can’t be used as a preset name.'), 'An invalid name was not explained')
    app.shot('6-invalid-name')
    app.send('-k', 'Escape')
    assert 'Mine.json' in files()
    checks.append('An invalid name keeps the old name and says why')

    # Delete asks first.
    app.row_action('00 Refused', 'preset.delete')
    wait(lambda: app.named('Delete the preset “00 Refused”?'), 'Delete did not ask first')
    app.shot('7-delete')
    app.act(app.named('Delete', Atspi.Role.PUSH_BUTTON), 'click')
    wait(lambda: '00 Refused.json' not in files(), 'Delete did not remove the file')
    checks.append('Delete asks for confirmation, then removes a refused file')
    wait(lambda: not app.named('Delete the preset “00 Refused”?'), 'The Delete dialog stayed open')
    time.sleep(1)
    # Import Presets… opens the file picker. The GTK portal's multi-select
    # picker doesn't accept a typed path, so the import itself is covered by
    # presets::tests::imports_copy_the_bytes_and_go_where_dropped.
    imports = out / 'import'
    shutil.rmtree(imports, ignore_errors=True)
    imports.mkdir()
    app.shortcut('CTRL SHIFT', 'O')
    picker, portal = app.portal('Import Presets')
    time.sleep(.8)
    (x, y), (w, h) = picker['at'], picker['size']
    subprocess.run(['grim', '-g', f'{x},{y} {w}x{h}', str(out / '8-import-picker.png')], check=True)
    cancel = wait(lambda: next((n for n in tree(portal) if n.get_name() == 'Cancel' and n.get_role() == Atspi.Role.PUSH_BUTTON), None),
                  'The picker has no Cancel button')
    app.act(cancel, 'click')
    wait(lambda: not any(c.get('title') == 'Import Presets' for c in json.loads(hypr('-j', 'clients'))), 'The picker did not cancel')
    checks.append('Ctrl+Shift+O opens the Import Presets picker; cancelling changes nothing')
    imported = (folder / 'Mine.json').read_text()

    # Export… writes the file byte for byte where the user picks.
    exported = imports / 'Exported.json'
    app.row_action('Mine', 'preset.export')
    app.save_as('Export Preset', 'Export', 'Mine.json', exported)
    wait(exported.is_file, 'Export did not write the file')
    assert exported.read_text() == imported
    checks.append('Export… copies the preset byte for byte')
    app.quit()
    app.close()

    # The open page and the applied settings survive a restart.
    session = json.loads((profile / 'session.json').read_text())
    assert session['preferences']['pane'] == 'presets', session['preferences']
    app = App()
    wait(lambda: app.named('Mine', Atspi.Role.LIST_ITEM), 'The Presets page was not restored')
    wait(app.developed, 'The restored photograph did not develop', 60)
    app.send('-k', 'F7')
    wait(lambda: app.named(film), 'The applied preset’s film was not restored')
    checks.append('The Presets page and the applied preset survive a restart')
    app.quit()
finally:
    app.close()
report = dict(status='PASS', checks=checks, screenshots=sorted(p.name for p in out.glob('*.png')))
(out / 'result.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
