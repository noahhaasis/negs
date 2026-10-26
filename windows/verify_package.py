#!/usr/bin/env python3
"""Verify committed Windows packages and prepare release assets."""
import hashlib
import io
from pathlib import Path
import re
import shutil
import subprocess
import tomllib
import zipfile


def main():
    root = Path(__file__).resolve().parents[1]
    git = ["git", "-c", f"safe.directory={root.as_posix()}", "-C", str(root)]
    commit = subprocess.check_output(git + ["rev-parse", "HEAD"], text=True).strip()
    version = tomllib.loads((root / "windows/Cargo.toml").read_text())["package"]["version"]
    engine_version = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["package"]["version"]
    installer_version = re.search(r"^AppVersion=(.+)$", (root / "windows/installer.iss").read_text(), re.M).group(1).strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", version) or version != engine_version or version != installer_version:
        raise ValueError("Windows app, installer and workspace release versions must match")
    dist = root / "target/windows-dist"
    sha = lambda data: hashlib.sha256(data).hexdigest()
    with zipfile.ZipFile(dist / "Negs-portable.zip") as package:
        if package.testzip() is not None:
            raise ValueError("Portable archive CRC failure")
        for name in ["Negs.exe", "spektrafilm-native.exe", "spektrafilm.exe", "spektrafilm-f64.exe"]:
            built = root / ("windows/target/release" if name == "Negs.exe" else "target/release") / name
            if sha(package.read(name)) != sha(built.read_bytes()):
                raise ValueError(f"Packaged executable differs from build: {name}")
        if package.read("LICENSE") != (root / "LICENSE").read_bytes():
            raise ValueError("Packaged license differs from source")
        if not any(name.startswith("data/") for name in package.namelist()):
            raise ValueError("Missing packaged data")
        with zipfile.ZipFile(io.BytesIO(package.read("source.zip"))) as source:
            if source.testzip() is not None or source.comment.decode() != commit:
                raise ValueError("Source archive is corrupt or does not identify HEAD")
            for name in ["README.md", "windows/src/main.rs", "windows/installer.iss", "windows/verify_package.py"]:
                if source.read(name) != subprocess.check_output(git + ["show", f"HEAD:{name}"]):
                    raise ValueError(f"Source archive differs from HEAD: {name}")
    setup = dist / "Negs-Setup.exe"
    with setup.open("rb") as stream:
        if stream.read(2) != b"MZ":
            raise ValueError("Installer is missing its executable header")
    output = root / "target/windows-release"
    output.mkdir(parents=True, exist_ok=True)
    stem = f"Negs-{version}-windows-x64"
    checksums = []
    for original, suffix in [("Negs-Setup.exe", "-Setup.exe"), ("Negs-portable.zip", "-portable.zip")]:
        name = stem + suffix
        shutil.copy2(dist / original, output / name)
        checksums.append(f"{sha((output / name).read_bytes())}  {name}\n")
    (output / "SHA256SUMS-windows.txt").write_text("".join(checksums), encoding="utf-8")
    print(f"Verified Windows {version} packages from {commit}")


if __name__ == "__main__":
    main()
