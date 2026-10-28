#!/usr/bin/env python3
"""Bundle the AppDir's non-host shared libraries and record the ownership ledger.

Libraries the host must provide are listed in HOST by soname: glibc, the
compiler runtime, GLib, the text and cairo stacks, and everything graphics
drivers also load (Wayland, X11, EGL/GL, Vulkan, DRM, zstd, expat...). A
bundled copy of those could be picked up by host drivers and crash them.
Every other dependency is copied into usr/lib, its dependencies followed,
and each executable/library gets a relative RUNPATH; no LD_LIBRARY_PATH.
"""
import argparse
import json
from pathlib import Path
import re
import shutil
import subprocess

HOST = {
    # glibc and compiler runtime
    'libc.so.6', 'libm.so.6', 'libdl.so.2', 'libpthread.so.0', 'librt.so.1', 'libresolv.so.2',
    'libutil.so.1', 'ld-linux-x86-64.so.2', 'libgcc_s.so.1', 'libstdc++.so.6',
    # GLib (bundled GTK 4.16 builds against Ubuntu 24.04's GLib 2.80)
    'libglib-2.0.so.0', 'libgobject-2.0.so.0', 'libgio-2.0.so.0', 'libgmodule-2.0.so.0',
    # text, fonts, cairo, images loaded by drivers or ubiquitous
    'libpango-1.0.so.0', 'libpangocairo-1.0.so.0', 'libpangoft2-1.0.so.0', 'libharfbuzz.so.0',
    'libfribidi.so.0', 'libfontconfig.so.1', 'libfreetype.so.6', 'libcairo.so.2',
    'libcairo-gobject.so.2', 'libpixman-1.so.0', 'libgdk_pixbuf-2.0.so.0', 'libpng16.so.16',
    'libtiff.so.6', 'libz.so.1',
    # display protocols, graphics loaders and drivers
    'libwayland-client.so.0', 'libwayland-egl.so.1', 'libwayland-cursor.so.0', 'libxkbcommon.so.0',
    'libX11.so.6', 'libX11-xcb.so.1', 'libXext.so.6', 'libXi.so.6', 'libXrandr.so.2',
    'libXcursor.so.1', 'libXdamage.so.1', 'libXfixes.so.3', 'libXinerama.so.1', 'libXrender.so.1',
    'libxcb.so.1', 'libXau.so.6', 'libXdmcp.so.6', 'libEGL.so.1', 'libGL.so.1', 'libGLX.so.0',
    'libGLdispatch.so.0', 'libOpenGL.so.0', 'libvulkan.so.1', 'libdrm.so.2', 'libgbm.so.1',
    # common system libraries shared with drivers and GLib
    'libzstd.so.1', 'liblzma.so.5', 'libbz2.so.1.0', 'libexpat.so.1', 'libffi.so.8',
    'libpcre2-8.so.0', 'libmount.so.1', 'libblkid.so.1', 'libselinux.so.1', 'libuuid.so.1',
    'libbrotlidec.so.1', 'libbrotlicommon.so.1', 'libjbig.so.0', 'libLerc.so.4',
    'libdeflate.so.0', 'libwebp.so.7', 'libsharpyuv.so.0',
}
LDD = re.compile(r'^\s*(\S+)\s+=>\s+(\S+)\s+\(')


def is_elf(path):
    with path.open('rb') as stream:
        return stream.read(4) == b'\x7fELF'


def dependencies(path, library_path):
    """Direct (DT_NEEDED) dependencies with the paths the loader resolves.

    Only direct dependencies: a RUNPATH governs an object's own NEEDED
    entries, and dependencies of host libraries stay the host's business."""
    output = subprocess.run(['ldd', str(path)], check=True, capture_output=True, text=True,
                            env={'LD_LIBRARY_PATH': library_path}).stdout
    if 'not found' in output:
        raise SystemExit(f'Unresolved dependency of {path}:\n{output}')
    resolved = {m.group(1): Path(m.group(2)) for m in map(LDD.match, output.splitlines()) if m}
    dynamic = subprocess.run(['objdump', '-p', str(path)], check=True, capture_output=True, text=True).stdout
    needed = re.findall(r'^\s*NEEDED\s+(\S+)$', dynamic, flags=re.M)
    return [(soname, resolved[soname]) for soname in needed if soname in resolved]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('appdir', type=Path)
    parser.add_argument('--search', default='/opt/negs-gtk/lib', help='Private dependency prefix')
    parser.add_argument('--ledger', type=Path, required=True)
    args = parser.parse_args()
    usr = args.appdir / 'usr'
    lib = usr / 'lib'
    executables = sorted(p for p in (lib / 'negs').iterdir() if p.is_file() and is_elf(p))
    pending, bundled, host = list(executables), {}, {}
    while pending:
        current = pending.pop()
        for soname, resolved in dependencies(current, args.search):
            if soname in HOST:
                host.setdefault(soname, set()).add(current.name)
            elif soname not in bundled:
                target = lib / soname
                shutil.copy2(resolved.resolve(), target)
                target.chmod(0o755)
                bundled[soname] = str(resolved)
                pending.append(target)
    for path in executables:
        subprocess.run(['patchelf', '--set-rpath', '$ORIGIN/..', str(path)], check=True)
    for soname in bundled:
        subprocess.run(['patchelf', '--set-rpath', '$ORIGIN', str(lib / soname)], check=True)
    # Verify the closure resolves from the AppDir alone, with no search path.
    for path in [*executables, *(lib / s for s in bundled)]:
        for soname, resolved in dependencies(path, ''):
            if soname not in HOST and not resolved.resolve().is_relative_to(args.appdir.resolve()):
                raise SystemExit(f'{path.name} resolves {soname} outside the AppDir: {resolved}')
    floor = {}
    for path in [*executables, *(lib / s for s in bundled)]:
        symbols = subprocess.run(['objdump', '-T', str(path)], check=True, capture_output=True, text=True).stdout
        for family, version in re.findall(r'\b(GLIBC|GLIBCXX|CXXABI|GCC)_([0-9.]+)\b', symbols):
            key = tuple(int(x) for x in version.split('.'))
            if key > floor.get(family, (0,)):
                floor[family] = key
    ledger = {
        'bundled': dict(sorted(bundled.items())),
        'host': {k: sorted(v) for k, v in sorted(host.items())},
        'symbol_floor': {k: '.'.join(map(str, v)) for k, v in sorted(floor.items())},
    }
    args.ledger.write_text(json.dumps(ledger, indent=2) + '\n')
    print(json.dumps({'bundled': sorted(bundled), 'symbol_floor': ledger['symbol_floor']}, indent=2))


if __name__ == '__main__':
    main()
