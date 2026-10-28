#!/usr/bin/env python3
"""Refuse to publish unless a release directory holds exactly the expected assets.

A download glob alone would silently publish a release with a missing,
skipped or renamed platform output. Every checksum file must also match.
  check_release_assets.py VERSION DIRECTORY
"""
import argparse
import hashlib
from pathlib import Path
import sys


def expected(version):
    v = version
    return {
        # macOS
        'appcast.xml', 'BUILD-INFO.txt', 'INSTALL.txt', 'SHA256SUMS',
        f'Negs-{v}-macos-universal.dmg', f'Negs-{v}-macos-universal.zip',
        f'Negs-{v}-macos-universal-source.tar.gz',
        # Windows
        'SHA256SUMS-windows.txt', f'Negs-{v}-windows-x64-Setup.exe', f'Negs-{v}-windows-x64-portable.zip',
        # Linux: Arch package, per-user bundle, source and downstream recipes
        'SHA256SUMS-linux', f'negs-{v}-1-x86_64.pkg.tar.zst', f'negs-{v}-linux-x86_64.tar.gz',
        f'negs-{v}.tar.gz', f'negs-{v}-linux-recipes.tar.gz',
        # Linux: Flatpak bundle and AppImage with its LGPL corresponding source
        'SHA256SUMS-flatpak', f'negs-{v}-linux-x86_64.flatpak',
        'SHA256SUMS-appimage', f'Negs-{v}-x86_64.AppImage', f'negs-{v}-appimage-sources.tar',
    }


def check(version, directory):
    problems = []
    present = {p.name for p in directory.iterdir() if p.is_file()}
    wanted = expected(version)
    problems += [f'missing {name}' for name in sorted(wanted - present)]
    problems += [f'unexpected {name}' for name in sorted(present - wanted)]
    covered = set()
    for sums in sorted(n for n in present & wanted if n.startswith('SHA256SUMS')):
        for line in (directory / sums).read_text().splitlines():
            if not line.strip():
                continue
            digest, name = line.split(maxsplit=1)
            name = name.lstrip('*')
            covered.add(name)
            path = directory / name
            if not path.is_file():
                problems.append(f'{sums} lists missing {name}')
                continue
            with path.open('rb') as stream:
                if hashlib.file_digest(stream, 'sha256').hexdigest() != digest.lower():
                    problems.append(f'{sums}: checksum mismatch for {name}')
    binaries = {n for n in wanted if not n.startswith('SHA256SUMS') and n not in {'appcast.xml', 'BUILD-INFO.txt', 'INSTALL.txt'}}
    problems += [f'no checksum for {name}' for name in sorted(binaries - covered)]
    return problems


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('version')
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    problems = check(args.version, args.directory)
    if problems:
        sys.exit('Release assets are incomplete:\n  ' + '\n  '.join(problems))
    print(f'All {len(expected(args.version))} release assets present with matching checksums.')


if __name__ == '__main__':
    main()
