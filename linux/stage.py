#!/usr/bin/env python3
"""Stage an Arch package or relocatable per-user bundle; never include target photos."""
import argparse
from pathlib import Path
import shutil
import subprocess

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--destdir', type=Path, required=True)
parser.add_argument('--prefix', default='/usr')
parser.add_argument('--engine-target', type=Path, default=root / 'target')
parser.add_argument('--ui-target', type=Path, default=root / 'target/linux-ui')
parser.add_argument('--source-archive', type=Path, required=True)
parser.add_argument('--bundle-openblas', type=Path, help='Optional extracted official OpenBLAS /usr tree')
args = parser.parse_args()
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
copy(root / 'linux/negs-launcher', 'bin/negs', 0o755)
shutil.copytree(root / 'data', prefix / 'share/negs/data', dirs_exist_ok=True)
for source, target in [
    ('assets/negs-icon.png', 'share/icons/hicolor/256x256/apps/dev.negs.app.png'),
    ('LICENSE', 'share/licenses/negs/LICENSE'),
    ('linux/install_omarchy_rules.py', 'share/negs/install_omarchy_rules.py'),
    ('linux/install.py', 'share/negs/install.py'),
    ('linux/omarchy/negs-window-rules.lua', 'share/negs/omarchy/negs-window-rules.lua'),
    ('docs/linux-app.md', 'share/doc/negs/linux-app.md'),
    ('docs/linux-core-validation.md', 'share/doc/negs/linux-core-validation.md'),
]:
    copy(root / source, target)
copy(args.source_archive, 'share/negs/source.tar.gz')
if args.bundle_openblas:
    copy(args.bundle_openblas / 'lib/libopenblas.so.0', 'lib/negs/libopenblas.so.0')
    copy(args.bundle_openblas / 'share/licenses/openblas/LICENSE', 'share/licenses/negs/OpenBLAS-LICENSE')
desktop = prefix / 'share/applications/dev.negs.app.desktop'
desktop.parent.mkdir(parents=True, exist_ok=True)
desktop.write_text('''[Desktop Entry]
Type=Application
Name=Negs
Comment=Photographic film simulation
Exec=negs %f
Icon=dev.negs.app
Terminal=false
Categories=Graphics;Photography;
StartupWMClass=dev.negs.app
MimeType=image/jpeg;image/png;image/tiff;image/x-adobe-dng;image/x-canon-cr2;image/x-canon-cr3;image/x-canon-crw;image/x-fuji-raf;image/x-hasselblad-3fr;image/x-hasselblad-fff;image/x-nikon-nef;image/x-nikon-nrw;image/x-olympus-orf;image/x-panasonic-rw;image/x-panasonic-rw2;image/x-pentax-pef;image/x-phaseone-iiq;image/x-samsung-srw;image/x-sigma-x3f;image/x-sony-arw;image/x-sony-sr2;image/x-sony-srf;
''')
if shutil.which('desktop-file-validate'):
    subprocess.run(['desktop-file-validate', desktop], check=True)
print(prefix)
