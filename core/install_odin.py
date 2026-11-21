#!/usr/bin/env python3
"""Installs the core's pinned Odin (core/ODIN_VERSION) into ~/.negs/odin/<tag>/.

    python3 core/install_odin.py

For CI and new machines; build.py finds it there. Does nothing when the
pinned build is already installed. Downloads the official release archive
from GitHub (macOS arm64, Linux amd64/arm64, Windows amd64).
"""

from __future__ import annotations

import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
import zipfile
from pathlib import Path

CORE = Path(__file__).resolve().parent
TAG, COMMIT = (CORE / "ODIN_VERSION").read_text().split()
WINDOWS = sys.platform == "win32"


def asset() -> str:
    machine = platform.machine().lower()
    arm = machine in ("arm64", "aarch64")
    if sys.platform == "darwin":
        if not arm:
            sys.exit("[core] Odin has no Intel macOS release; build on Apple silicon")
        return f"odin-macos-arm64-{TAG}.tar.gz"
    if WINDOWS:
        return f"odin-windows-amd64-{TAG}.zip"
    return f"odin-linux-{'arm64' if arm else 'amd64'}-{TAG}.tar.gz"


def installed(dest: Path) -> bool:
    exe = dest / ("odin.exe" if WINDOWS else "odin")
    if not exe.is_file():
        return False
    out = subprocess.run([str(exe), "version"], capture_output=True, text=True).stdout
    return COMMIT in out


def main() -> None:
    dest = Path.home() / ".negs" / "odin" / TAG
    if installed(dest):
        print(f"[core] Odin {TAG} already in {dest}")
        return
    name = asset()
    url = f"https://github.com/odin-lang/Odin/releases/download/{TAG}/{name}"
    with tempfile.TemporaryDirectory() as tmp:
        archive = Path(tmp) / name
        print(f"[core] downloading {url}")
        with urllib.request.urlopen(url) as response, open(archive, "wb") as out:
            shutil.copyfileobj(response, out)
        unpacked = Path(tmp) / "unpacked"
        if name.endswith(".zip"):
            with zipfile.ZipFile(archive) as z:
                z.extractall(unpacked)
        else:
            with tarfile.open(archive) as t:
                t.extractall(unpacked, filter="tar")
        exe = "odin.exe" if WINDOWS else "odin"
        found = next((p.parent for p in unpacked.rglob(exe) if p.is_file()), None)
        if found is None:
            sys.exit(f"[core] no {exe} in {name}")
        if dest.exists():
            shutil.rmtree(dest)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.move(str(found), str(dest))
    if not WINDOWS:
        (dest / "odin").chmod(0o755)
    if not installed(dest):
        sys.exit(f"[core] {dest} doesn't report Odin {COMMIT}")
    print(f"[core] installed Odin {TAG} in {dest}")


if __name__ == "__main__":
    main()
