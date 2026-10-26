#!/usr/bin/env python3
"""Install/uninstall an unpacked Negs bundle per user, preserving modified files/settings."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--from', dest='source', type=Path,
                    help='Bundle prefix containing bin/, lib/, share/')
parser.add_argument('--prefix', type=Path, default=Path.home() / '.local')
parser.add_argument('--uninstall', action='store_true')
parser.add_argument('--no-reload', action='store_true')
args = parser.parse_args()
prefix = args.prefix.expanduser().resolve()
manifest = prefix / 'share/negs/install-manifest.json'
owned = json.loads(manifest.read_text()) if manifest.is_file() else {}

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def target(relative):
    path = prefix / relative
    if not path.resolve().is_relative_to(prefix) or relative.startswith('/'):
        raise SystemExit('Unsafe manifest path: ' + relative)
    return path

if args.uninstall:
    directories = set()
    for relative, expected in owned.items():
        path = target(relative)
        if path.is_file():
            if digest(path) != expected:
                print('Preserving modified file:', path)
                continue
            path.unlink()
            directories.update(p for p in path.parents if p != prefix and p.is_relative_to(prefix))
    manifest.unlink(missing_ok=True)
    for path in sorted(directories, key=lambda p: len(p.parts), reverse=True):
        try:
            path.rmdir()
        except OSError:
            pass
    print('Removed installed files. Settings, photographs and Hyprland rule were preserved.')
else:
    if not args.source:
        parser.error('--from is required for installation')
    source = args.source.resolve()
    files = sorted(p for p in source.rglob('*') if p.is_file())
    if not (source / 'lib/negs/negs').is_file():
        raise SystemExit('Not a staged Negs bundle')
    # Complete conflict checking before changing any installed file.
    for path in files:
        relative = str(path.relative_to(source))
        destination = target(relative)
        if destination.exists() and (relative not in owned or digest(destination) != owned[relative]):
            raise SystemExit('Preserving unowned/modified file; resolve first: ' + str(destination))
    current = dict(owned)
    for path in files:
        relative = str(path.relative_to(source))
        destination = target(relative)
        destination.parent.mkdir(parents=True, exist_ok=True)
        temporary = destination.with_name(destination.name + '.tmp-negs-install')
        shutil.copy2(path, temporary)
        if relative == 'share/applications/dev.negs.app.desktop':
            command = str(prefix / 'bin/negs').replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
            temporary.write_text(temporary.read_text().replace('Exec=negs %f', f'Exec="{command}" %f'))
        temporary.replace(destination)
        current[relative] = digest(destination)
    manifest.parent.mkdir(parents=True, exist_ok=True)
    temporary = manifest.with_suffix('.tmp')
    temporary.write_text(json.dumps(current, indent=2) + '\n')
    temporary.replace(manifest)
    config = Path(os.environ.get('XDG_CONFIG_HOME', str(Path.home() / '.config')))
    if (config / 'hypr/hyprland.lua').is_file():
        subprocess.run(['python3', prefix / 'share/negs/install_omarchy_rules.py'], check=True)
        if not args.no_reload and os.environ.get('HYPRLAND_INSTANCE_SIGNATURE'):
            subprocess.run(['hyprctl', 'reload'], check=True)
            errors = subprocess.check_output(['hyprctl', 'configerrors'], text=True).strip()
            if errors:
                raise SystemExit('Hyprland configuration errors:\n' + errors)
    print('Installed:', prefix / 'bin/negs')
applications = prefix / 'share/applications'
if applications.is_dir() and shutil.which('update-desktop-database'):
    subprocess.run(['update-desktop-database', applications], check=True)
