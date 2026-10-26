#!/usr/bin/env python3
"""Verify the packaged application, then exercise its installed engine paths on CPU."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='negs-package-check-') as work:
        stage = Path(work)
        subprocess.run(['bsdtar', '-xf', str(args.package.resolve()), '-C', str(stage)], check=True)
        prefix = stage / 'usr'
        for name in ['negs', 'spektrafilm-native', 'spektrafilm', 'spektrafilm-f64']:
            binary = prefix / 'lib/negs' / name
            if not binary.is_file() or not os.access(binary, os.X_OK):
                raise ValueError(f'Missing executable: {binary}')
            links = subprocess.run(['ldd', str(binary)], check=True, capture_output=True, text=True)
            if 'not found' in links.stdout:
                raise ValueError(links.stdout)
        for name in ['bin/negs', 'share/negs/source.tar.gz', 'share/licenses/negs/LICENSE',
                     'share/negs/data/SPEKTRAFILM_LICENSE.txt',
                     'share/icons/hicolor/256x256/apps/dev.negs.app.png']:
            if not (prefix / name).is_file():
                raise ValueError(f'Missing packaged file: {name}')
        subprocess.run(['desktop-file-validate', str(prefix / 'share/applications/dev.negs.app.desktop')], check=True)
        subprocess.run([
            'python3', '-I', str(ROOT / 'scripts/check_native_macos.py'),
            '--helper', str(prefix / 'lib/negs/spektrafilm-native'),
            '--exporter', str(prefix / 'lib/negs/spektrafilm-f64'),
            '--fast-exporter', str(prefix / 'lib/negs/spektrafilm'),
            '--data-dir', str(prefix / 'share/negs/data'), '--expected-backend', 'cpu',
        ], env=dict(os.environ, SPEKTRAFILM_BACKEND='cpu'), check=True, timeout=300)
    print('Packaged binaries, desktop entry and CPU engine smoke checks passed.')


if __name__ == '__main__':
    main()
