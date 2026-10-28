#!/usr/bin/env python3
"""Generate flatpak-builder sources that vendor every crate in both Cargo lockfiles.

Output is equivalent to flatpak-builder-tools' cargo generator for crates.io
packages. Any other source type (git, another registry) is rejected, so a new
dependency kind cannot silently fall back to the network during an offline build.
Run with --check to verify committed manifests match the lockfiles.
"""
import argparse
import json
from pathlib import Path
import sys
import tomllib

ROOT = Path(__file__).resolve().parents[2]
CRATES_IO = 'registry+https://github.com/rust-lang/crates.io-index'
OUTPUTS = {ROOT / 'Cargo.lock': Path(__file__).parent / 'cargo-sources.json',
           ROOT / 'linux/Cargo.lock': Path(__file__).parent / 'cargo-sources-linux.json'}
CONFIG = '''[source.vendored-sources]
directory = "cargo/vendor"

[source.crates-io]
replace-with = "vendored-sources"
'''


def sources(lockfile):
    lock = tomllib.loads(lockfile.read_text())
    result = []
    for package in sorted(lock['package'], key=lambda p: (p['name'], p['version'])):
        source = package.get('source')
        if source is None:
            continue  # workspace member
        if source != CRATES_IO or 'checksum' not in package:
            raise ValueError(f"{lockfile}: {package['name']} {package['version']} uses unsupported source {source!r}")
        name, version, checksum = package['name'], package['version'], package['checksum']
        dest = f'cargo/vendor/{name}-{version}'
        result += [
            {'type': 'archive', 'archive-type': 'tar-gzip',
             'url': f'https://static.crates.io/crates/{name}/{name}-{version}.crate',
             'sha256': checksum, 'dest': dest},
            {'type': 'inline', 'contents': json.dumps({'package': checksum, 'files': {}}),
             'dest': dest, 'dest-filename': '.cargo-checksum.json'},
        ]
    result.append({'type': 'inline', 'contents': CONFIG, 'dest': 'cargo', 'dest-filename': 'config.toml'})
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    stale = []
    for lockfile, output in OUTPUTS.items():
        text = json.dumps(sources(lockfile), indent=2) + '\n'
        if args.check:
            if not output.is_file() or output.read_text() != text:
                stale.append(output.name)
        else:
            output.write_text(text)
            print(f'{output.name}: {text.count(chr(10) + "    " + chr(34) + "type" + chr(34) + ": " + chr(34) + "archive")} crates')
    if stale:
        sys.exit(f"Regenerate with linux/flatpak/cargo_sources.py: {', '.join(stale)}")


if __name__ == '__main__':
    main()
