"""Run the real AppRun with a stand-in GUI to inspect its launch contract."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


class AppImageLauncherTests(unittest.TestCase):
    def setUp(self):
        self.work = tempfile.TemporaryDirectory(prefix='negs-apprun-')
        self.addCleanup(self.work.cleanup)
        self.appdir = Path(self.work.name) / 'Negs portable ü'
        binary = self.appdir / 'usr/lib/negs/negs'
        binary.parent.mkdir(parents=True)
        shutil.copy2(Path(__file__).parent / 'appimage/AppRun', self.appdir / 'AppRun')
        binary.write_text(
            f'#!{sys.executable}\n'
            'import json, os, sys\n'
            'print(json.dumps({"env": dict(os.environ), "args": sys.argv[1:]}))\n'
            'sys.exit(int(os.environ.get("TEST_EXIT", "0")))\n'
        )
        binary.chmod(0o755)

    def launch(self, **environment):
        env = dict(os.environ)
        for name in ('DISPLAY', 'WAYLAND_DISPLAY', 'GDK_BACKEND', 'TEST_EXIT'):
            env.pop(name, None)
        env.update(environment)
        result = subprocess.run(
            [str(self.appdir / 'AppRun'), '--fresh', '/photos/a b ü.nef'],
            env=env, capture_output=True, text=True, timeout=10,
        )
        self.assertEqual(result.returncode, int(environment.get('TEST_EXIT', '0')), result.stderr)
        data = json.loads(result.stdout)
        self.assertEqual(data['args'], ['--fresh', '/photos/a b ü.nef'])
        return data['env']

    def test_prefers_x11_on_mixed_and_x11_desktops(self):
        for wayland in ('', 'wayland-0'):
            with self.subTest(wayland=wayland):
                env = self.launch(DISPLAY=':1', WAYLAND_DISPLAY=wayland)
                self.assertEqual(env['GDK_BACKEND'], 'x11,wayland')
                self.assertEqual(env['NEGS_ORIG_GDK_BACKEND'], '')
                self.assertEqual(env['NEGS_ORIG_SAVED'], '1')

    def test_keeps_default_without_an_x_display(self):
        for wayland in ('', 'wayland-0'):
            with self.subTest(wayland=wayland):
                env = self.launch(DISPLAY='', WAYLAND_DISPLAY=wayland)
                self.assertNotIn('GDK_BACKEND', env)

    def test_respects_explicit_backend_even_if_empty(self):
        for backend in ('wayland', 'x11', 'wayland,x11', ''):
            with self.subTest(backend=backend):
                env = self.launch(DISPLAY=':1', WAYLAND_DISPLAY='wayland-0', GDK_BACKEND=backend)
                self.assertEqual(env['GDK_BACKEND'], backend)
                self.assertEqual(env['NEGS_ORIG_GDK_BACKEND'], backend)

    def test_preserves_payload_paths_and_exit_status(self):
        env = self.launch(DISPLAY=':1', TEST_EXIT='37')
        self.assertEqual(env['NEGS_PACKAGE_ROOT'], str(self.appdir / 'usr'))
        self.assertEqual(env['NEGS_DATA_DIR'], str(self.appdir / 'usr/share/negs/data'))


if __name__ == '__main__':
    unittest.main()
