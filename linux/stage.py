#!/usr/bin/env python3
"""Stage the Linux payload for an Arch package, per-user bundle, Flatpak or AppImage.

Every format shares one layout under the prefix: lib/negs/ holds the four
executables, share/negs/data the engine data, plus desktop metadata and
licenses. Only the native format adds the Omarchy integration and its
launcher; Flatpak and AppImage supply their own launchers. Never includes
target photos.
"""
import argparse
from pathlib import Path
import shutil
import subprocess
import tomllib
from payload import bundle_openblas, write_identity

APP_ID = 'io.github.noahhaasis.negs'
root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--destdir', type=Path, required=True)
parser.add_argument('--prefix', default='/usr')
parser.add_argument('--format', choices=['native', 'flatpak', 'appimage'], default='native')
parser.add_argument('--engine-target', type=Path, default=root / 'target')
parser.add_argument('--ui-target', type=Path, default=root / 'target/linux-ui')
parser.add_argument('--source-archive', type=Path)
parser.add_argument('--bundle-openblas', type=Path, help='Optional extracted official OpenBLAS /usr tree')
parser.add_argument('--notices', type=Path, help='Pre-generated Rust crate notices (default: generate)')
args = parser.parse_args()
if args.format != 'flatpak' and not args.source_archive:
    parser.error('--source-archive is required for native and AppImage payloads')
prefix = args.destdir / args.prefix.lstrip('/')

def copy(source, relative, mode=None):
    target = prefix / relative
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(source, target)
    if mode is not None:
        target.chmod(mode)

copy(args.ui_target / 'release/negs', 'lib/negs/negs', 0o755)
for name in ['spektrafilm', 'spektrafilm-f64', 'spektrafilm-native']:
    copy(args.engine_target / 'release' / name, 'lib/negs/' + name, 0o755)
shutil.copytree(root / 'data', prefix / 'share/negs/data', dirs_exist_ok=True)
for size in [128, 256, 512]:
    copy(root / f'linux/icons/negs-{size}.png', f'share/icons/hicolor/{size}x{size}/apps/{APP_ID}.png')
for source, target in [
    (f'linux/{APP_ID}.desktop', f'share/applications/{APP_ID}.desktop'),
    (f'linux/{APP_ID}.metainfo.xml', f'share/metainfo/{APP_ID}.metainfo.xml'),
    ('LICENSE', 'share/licenses/negs/LICENSE'),
    ('crates/spektrafilm-io/fonts/OFL-BarlowCondensed.txt', 'share/licenses/negs/OFL-BarlowCondensed.txt'),
    ('docs/linux-app.md', 'share/doc/negs/linux-app.md'),
]:
    copy(root / source, target)
if args.notices:
    copy(args.notices, 'share/licenses/negs/THIRD-PARTY-RUST.txt')
else:
    subprocess.run(['python3', root / 'linux/notices.py', prefix / 'share/licenses/negs/THIRD-PARTY-RUST.txt'], check=True)
if args.source_archive:
    copy(args.source_archive, 'share/negs/source.tar.gz')
if args.format == 'native':
    copy(root / 'linux/negs-launcher', 'bin/negs', 0o755)
    for source, target in [
        ('linux/install_omarchy_rules.py', 'share/negs/install_omarchy_rules.py'),
        ('linux/install.py', 'share/negs/install.py'),
        ('linux/omarchy/negs-window-rules.lua', 'share/negs/omarchy/negs-window-rules.lua'),
        ('docs/linux-core-validation.md', 'share/doc/negs/linux-core-validation.md'),
    ]:
        copy(root / source, target)
if args.bundle_openblas:
    bundle_openblas(prefix, args.bundle_openblas)

# Immutable payload identity for the preview cache: relocation (an AppImage
# mount) keeps it, any changed executable, library or data file replaces it.
version = tomllib.loads((root / 'linux/Cargo.toml').read_text())['package']['version']
write_identity(prefix, version, args.format)
if shutil.which('desktop-file-validate'):
    subprocess.run(['desktop-file-validate', prefix / f'share/applications/{APP_ID}.desktop'], check=True)
print(prefix)
