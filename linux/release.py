#!/usr/bin/env python3
"""Prepare immutable Linux sources, release assets and downstream package recipes."""
import argparse
import gzip
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
# Exclude private validation material and historical image-heavy review docs.
SOURCE_PATHS = [
    '.gitattributes', 'Cargo.toml', 'Cargo.lock', 'LICENSE', 'crates', 'data',
    'linux', 'scripts', 'assets/negs-icon.png', 'assets/negs-icon.svg',
    'docs/linux-app.md', 'docs/linux-core-validation.md',
    'docs/linux-ui-parity.md', 'docs/linux-ui-validation.md',
    'docs/linux-border-performance.md',
    'docs/linux-release.md',
    'docs/linux-post-main-validation.md',
]


def git(*args, root=ROOT):
    return subprocess.check_output(['git', '-C', str(root), *args])


def metadata(root=ROOT, env=os.environ):
    version = tomllib.loads((root / 'Cargo.toml').read_text())['workspace']['package']['version']
    ui_version = tomllib.loads((root / 'linux/Cargo.toml').read_text())['package']['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+', version) or ui_version != version:
        raise ValueError('Root and Linux Cargo versions must match (major.minor.patch)')
    tag = env.get('RELEASE_TAG', '')
    if tag and tag != f'v{version}':
        raise ValueError(f'Tag {tag!r} does not match Cargo version v{version}')
    repository = env.get('RELEASE_REPOSITORY') or 'noahhaasis/negs'
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('RELEASE_REPOSITORY must be owner/repo')
    return dict(version=version, tag=f'v{version}', repository=repository,
                commit=git('rev-parse', 'HEAD', root=root).decode().strip(),
                epoch=int(git('show', '-s', '--format=%ct', 'HEAD', root=root)))


def sha256(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def source_recipe(template, meta, digest, remote=False):
    recipe, count = re.subn(r'^pkgver=.*$', f"pkgver={meta['version']}", template, flags=re.M)
    if count != 1:
        raise ValueError('PKGBUILD must contain one pkgver')
    recipe, count = re.subn(r"^sha256sums=.*$", f"sha256sums=('{digest}')", recipe, flags=re.M)
    if count != 1 or 'SKIP' in recipe:
        raise ValueError('PKGBUILD must contain one source checksum')
    recipe = re.sub(r'^url=.*$', f"url='https://github.com/{meta['repository']}'", recipe, flags=re.M)
    if remote:
        recipe = recipe.replace('source=("negs-$pkgver.tar.gz")',
                                'source=("$url/releases/download/v$pkgver/negs-$pkgver.tar.gz")')
    return recipe


def prepare(out, root=ROOT, env=os.environ):
    meta = metadata(root, env)
    # A source archive must describe this checkout, including release tooling.
    if git('status', '--porcelain', '--untracked-files=no', root=root).strip():
        raise ValueError('Commit tracked changes before creating release sources')
    assets, build = out / 'assets', out / 'build'
    if assets.exists() and any(assets.iterdir()):
        raise ValueError('Use an empty release output directory')
    assets.mkdir(parents=True, exist_ok=True)
    build.mkdir(parents=True, exist_ok=True)
    archive = assets / f"negs-{meta['version']}.tar.gz"
    with archive.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed:
        # Stream rather than retaining the whole source archive in memory.
        child = subprocess.Popen(['git', '-C', str(root), 'archive', '--format=tar',
                                  f"--prefix=negs-{meta['version']}/", 'HEAD', '--', *SOURCE_PATHS],
                                 stdout=subprocess.PIPE)
        try:
            shutil.copyfileobj(child.stdout, compressed)
        finally:
            child.stdout.close()
        if child.wait():
            raise ValueError('git archive failed')
    meta['source_sha256'] = sha256(archive)
    shutil.copy2(archive, build / archive.name)
    template = (root / 'linux/arch/PKGBUILD').read_text()
    (build / 'PKGBUILD').write_text(source_recipe(template, meta, meta['source_sha256']))
    (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    return meta


def write_recipe(directory, recipe):
    directory.mkdir(parents=True, exist_ok=True)
    (directory / 'PKGBUILD').write_text(recipe)
    result = subprocess.check_output(['makepkg', '--printsrcinfo'], cwd=directory, text=True)
    (directory / '.SRCINFO').write_text(result)
    (directory / 'LICENSE').write_text('SPDX-License-Identifier: 0BSD\n\n'
        'Copyright (c) 2026 Negs contributors\n\n'
        'Permission to use, copy, modify, and/or distribute this software for any purpose '
        'with or without fee is hereby granted.\n\n'
        'THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH '
        'REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY '
        'AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT, '
        'INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM '
        'LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR '
        'OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR '
        'PERFORMANCE OF THIS SOFTWARE.\n')


def finish(out, package, root=ROOT, env=os.environ):
    meta = json.loads((out / 'metadata.json').read_text())
    if metadata(root, env) != {key: value for key, value in meta.items() if key != 'source_sha256'}:
        raise ValueError('Checkout or release metadata changed since prepare')
    assets = out / 'assets'
    archive = assets / f"negs-{meta['version']}.tar.gz"
    if sha256(archive) != meta['source_sha256']:
        raise ValueError('Source archive changed since prepare')
    expected = f"negs-{meta['version']}-1-x86_64.pkg.tar.zst"
    if package.name != expected:
        raise ValueError(f'Expected package {expected}')
    shutil.copy2(package, assets / package.name)
    bundle = assets / f"negs-{meta['version']}-linux-x86_64.tar.gz"
    with tempfile.TemporaryDirectory(prefix='negs-release-') as work:
        stage = Path(work)
        subprocess.run(['bsdtar', '-xf', str(package.resolve()), '-C', str(stage), 'usr'], check=True)
        # Same package bytes in the per-user bundle and the AUR binary recipe.
        with bundle.open('wb') as raw, gzip.GzipFile(filename='', mode='wb', fileobj=raw, mtime=0) as compressed:
            with tarfile.open(fileobj=compressed, mode='w|') as tar:
                for path in sorted((stage / 'usr').rglob('*')):
                    info = tar.gettarinfo(path, arcname=str(path.relative_to(stage / 'usr')))
                    info.uid = info.gid = 0
                    info.uname = info.gname = ''
                    info.mtime = meta['epoch']
                    if info.isfile():
                        with path.open('rb') as stream:
                            tar.addfile(info, stream)
                    else:
                        tar.addfile(info)
    maintainer = env.get('AUR_MAINTAINER') or f"{meta['repository'].split('/')[0]} <https://github.com/{meta['repository'].split('/')[0]}>"
    if '\n' in maintainer or '\r' in maintainer:
        raise ValueError('AUR_MAINTAINER must be a single line')
    header = f'# Maintainer: {maintainer}\n'
    template = (root / 'linux/arch/PKGBUILD').read_text()
    source = header + source_recipe(template, meta, meta['source_sha256'], remote=True)
    write_recipe(out / 'recipes/aur/negs', source)
    binary = header + f'''pkgname=negs-bin
pkgver={meta['version']}
pkgrel=1
pkgdesc='Native GTK4 photographic film simulation editor (binary release)'
arch=('x86_64')
url='https://github.com/{meta['repository']}'
license=('GPL-3.0-only')
depends=('glibc' 'gcc-libs' 'gtk4>=4.16' 'libadwaita>=1.6' 'openblas' 'vulkan-icd-loader' 'python' 'hicolor-icon-theme' 'desktop-file-utils')
optdepends=('vulkan-driver: hardware accelerated preview and Fast export')
provides=('negs')
conflicts=('negs')
options=('!strip' '!debug')
source=("$url/releases/download/v$pkgver/negs-$pkgver-linux-x86_64.tar.gz")
sha256sums=('{sha256(bundle)}')

package() {{
    install -d "$pkgdir/usr"
    cp -a "$srcdir/bin" "$srcdir/lib" "$srcdir/share" "$pkgdir/usr/"
    mv "$pkgdir/usr/share/licenses/negs" "$pkgdir/usr/share/licenses/$pkgname"
}}
'''
    write_recipe(out / 'recipes/aur/negs-bin', binary)
    # Omarchy builds from source against its own channel's system libraries.
    omarchy = out / 'recipes/omarchy/pkgbuilds/negs'
    write_recipe(omarchy, source)
    (omarchy / '.omarchy').mkdir(exist_ok=True)
    (omarchy / '.omarchy/package.json').write_text(json.dumps({
        'source': 'local',
        'upstream': {'watch': {'github': meta['repository'],
                               'pattern': r'v(?P<version>[0-9]+\.[0-9]+\.[0-9]+)',
                               'allow_prerelease': True}},
    }, indent=2) + '\n')
    recipes = assets / f"negs-{meta['version']}-linux-recipes.tar.gz"
    subprocess.run(['tar', '--sort=name', f"--mtime=@{meta['epoch']}", '--owner=0', '--group=0',
                    '--numeric-owner', '-czf', str(recipes), '-C', str(out / 'recipes'), 'aur', 'omarchy'], check=True)
    sums = ''.join(f'{sha256(path)}  {path.name}\n' for path in sorted(assets.iterdir()) if path.name != 'SHA256SUMS-linux')
    (assets / 'SHA256SUMS-linux').write_text(sums)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('command', choices=['prepare', 'finish'])
    parser.add_argument('--out', type=Path, default=ROOT / 'target/linux-release')
    parser.add_argument('--package', type=Path)
    args = parser.parse_args()
    out = args.out.resolve()
    if args.command == 'prepare':
        print(json.dumps(prepare(out), indent=2))
    elif args.package:
        finish(out, args.package)
    else:
        parser.error('finish requires --package')


if __name__ == '__main__':
    main()
