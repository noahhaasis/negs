#!/usr/bin/env python3
"""Verify a packaged application, then exercise its installed engine paths on CPU.

Pass an Arch package, or --prefix for an installed or extracted payload
(AppImage squashfs-root/usr, or /app inside the Flatpak sandbox).
"""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
APP_ID = 'io.github.noahhaasis.negs'
EXECUTABLES = ['negs', 'spektrafilm-native', 'spektrafilm', 'spektrafilm-f64']


def verify_prefix(prefix, native=True, check_script=ROOT / 'scripts/check_native_macos.py'):
    for name in EXECUTABLES:
        binary = prefix / 'lib/negs' / name
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise ValueError(f'Missing executable: {binary}')
        links = subprocess.run(['ldd', str(binary)], check=True, capture_output=True, text=True)
        if 'not found' in links.stdout:
            raise ValueError(links.stdout)
    required = ['share/negs/data/SPEKTRAFILM_LICENSE.txt', 'share/negs/payload.json',
                'share/licenses/negs/LICENSE', 'share/licenses/negs/THIRD-PARTY-RUST.txt',
                'share/licenses/negs/OFL-BarlowCondensed.txt',
                f'share/metainfo/{APP_ID}.metainfo.xml',
                f'share/icons/hicolor/256x256/apps/{APP_ID}.png']
    if native:
        required += ['bin/negs', 'share/negs/source.tar.gz']
    for name in required:
        if not (prefix / name).is_file():
            raise ValueError(f'Missing packaged file: {name}')
    if shutil.which('desktop-file-validate'):
        subprocess.run(['desktop-file-validate', str(prefix / f'share/applications/{APP_ID}.desktop')], check=True)
    # A scratch working directory and no SPEKTRAFILM_ROOT: nothing may come
    # from a source checkout.
    environment = {k: v for k, v in os.environ.items() if not k.startswith(('SPEKTRAFILM_', 'NEGS_'))}
    with tempfile.TemporaryDirectory(prefix='negs-engine-check-') as work:
        subprocess.run([
            'python3', '-I', str(check_script),
            '--helper', str(prefix / 'lib/negs/spektrafilm-native'),
            '--exporter', str(prefix / 'lib/negs/spektrafilm-f64'),
            '--fast-exporter', str(prefix / 'lib/negs/spektrafilm'),
            '--data-dir', str(prefix / 'share/negs/data'), '--expected-backend', 'cpu',
        ], env=dict(environment, SPEKTRAFILM_BACKEND='cpu'), cwd=work, check=True, timeout=300)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path, nargs='?')
    parser.add_argument('--prefix', type=Path, help='Installed or extracted payload prefix')
    parser.add_argument('--format', choices=['native', 'flatpak', 'appimage'], default='native')
    args = parser.parse_args()
    if bool(args.package) == bool(args.prefix):
        parser.error('Pass an Arch package or --prefix')
    if args.prefix:
        verify_prefix(args.prefix.resolve(), native=args.format == 'native')
    else:
        with tempfile.TemporaryDirectory(prefix='negs-package-check-') as work:
            stage = Path(work)
            subprocess.run(['bsdtar', '-xf', str(args.package.resolve()), '-C', str(stage)], check=True)
            verify_prefix(stage / 'usr')
    print('Packaged binaries, desktop entry and CPU engine smoke checks passed.')


if __name__ == '__main__':
    main()
