#!/usr/bin/env python3
"""Write license notices for the Rust crates compiled into the Linux executables."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shlex
import subprocess

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'x86_64-unknown-linux-gnu'
# Workspace packages whose executables ship; dev-dependencies are not compiled in.
ROOTS = {ROOT / 'Cargo.toml': ['spektrafilm-cli', 'spektrafilm-native'],
         ROOT / 'linux/Cargo.toml': ['negs-linux']}
LICENSE_NAMES = ('license', 'licence', 'copying', 'notice', 'copyright', 'unlicense')


def cargo():
    toolchain = os.environ.get('NEGS_TOOLCHAIN', '+1.93.1')
    return ['cargo', *shlex.split(toolchain)]


def shipped(manifest, roots):
    # Metadata needs every locked crate, including other platforms'; offline
    # builds (Arch after cargo fetch, Flatpak) have them all vendored.
    offline = ['--offline'] if os.environ.get('NEGS_CARGO_OFFLINE') == '1' else []
    meta = json.loads(subprocess.check_output([
        *cargo(), 'metadata', '--format-version', '1', '--locked', *offline,
        '--filter-platform', TARGET, '--manifest-path', str(manifest)]))
    packages = {p['id']: p for p in meta['packages']}
    nodes = {n['id']: n for n in meta['resolve']['nodes']}
    pending = [p['id'] for p in meta['packages'] if p['name'] in roots and p['source'] is None]
    seen = set()
    while pending:
        current = pending.pop()
        if current in seen:
            continue
        seen.add(current)
        for dep in nodes[current]['deps']:
            if any(kind['kind'] in (None, 'build') for kind in dep['dep_kinds']):
                pending.append(dep['pkg'])
    return [packages[i] for i in seen if packages[i]['source'] is not None]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    crates = {}
    for manifest, roots in ROOTS.items():
        for package in shipped(manifest, roots):
            crates[(package['name'], package['version'])] = package
    texts, sections = {}, []
    for (name, version), package in sorted(crates.items()):
        folder = Path(package['manifest_path']).parent
        files = sorted(p for p in folder.iterdir()
                       if p.is_file() and p.name.lower().startswith(LICENSE_NAMES))
        references = []
        for path in files:
            text = path.read_text(errors='replace').strip()
            digest = hashlib.sha256(text.encode()).hexdigest()[:12]
            texts.setdefault(digest, text)
            references.append(f'{path.name} [{digest}]')
        license = package.get('license') or 'see license file'
        repository = package.get('repository') or ''
        sections.append(f'{name} {version}\n  License: {license}\n  Source: {repository}\n'
                        f"  Files: {', '.join(references) or 'none distributed by the crate'}\n")
    body = ['Third-party Rust crates compiled into Negs for Linux\n',
            '=' * 52 + '\n\n', *[s + '\n' for s in sections],
            '\nLicense texts\n', '=' * 13 + '\n']
    for digest, text in texts.items():
        body.append(f'\n[{digest}]\n{text}\n')
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(''.join(body))
    print(f'{len(crates)} crates, {len(texts)} distinct license texts: {args.output}')


if __name__ == '__main__':
    main()
