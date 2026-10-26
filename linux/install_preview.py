#!/usr/bin/env python3
"""Install the built UI preview per user, including Omarchy's opacity exception.

This is a local preview installer, not a published distro package.
"""
import argparse
from pathlib import Path
import os
import runpy
import shlex
import shutil
import subprocess
import tarfile

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--no-reload", action="store_true", help="Install files without contacting a running Hyprland (for staging/tests)")
args = parser.parse_args()
binary = root / "target/linux-ui/release/negs"
if not binary.is_file():
    raise SystemExit("Build first: bash linux/cargo.sh build --release --locked")
config = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
data = Path(os.environ.get("XDG_DATA_HOME", str(Path.home() / ".local/share")))
bin_dir = Path(os.environ.get("XDG_BIN_HOME", str(Path.home() / ".local/bin")))
app = data / "negs-ui-preview"
(app / "bin").mkdir(parents=True, exist_ok=True)
(app / "data").mkdir(exist_ok=True)
# Copy atomically so an already running process can retain its old executable.
temporary = app / "bin/negs.tmp"
shutil.copy2(binary, temporary)
temporary.replace(app / "bin/negs")
shutil.copytree(root / "data/borders", app / "data/borders", dirs_exist_ok=True)
shutil.copy2(root / "LICENSE", app / "LICENSE")
shutil.copy2(root / "docs/linux-app.md", app / "README.md")
documentation = [root / "docs" / name for name in ["linux-app.md", "linux-ui-parity.md", "linux-ui-validation.md"]]
for document in documentation:
    shutil.copy2(document, app / document.name)
with tarfile.open(app / "source.tar.gz", "w:gz") as archive:
    for item in [root / "linux", root / "assets", root / "LICENSE", *documentation]:
        archive.add(item, arcname=str(item.relative_to(root)), filter=lambda entry: None if "__pycache__" in Path(entry.name).parts else entry)
bin_dir.mkdir(parents=True, exist_ok=True)
launcher = bin_dir / "negs-ui-preview"
launcher.write_text('#!/bin/sh\nexport NEGS_UI_ONLY=1\nexec ' + shlex.quote(str(app / "bin/negs")) + ' "$@"\n')
launcher.chmod(0o755)
icon = data / "icons/hicolor/256x256/apps/dev.negs.app.UiPreview.png"
icon.parent.mkdir(parents=True, exist_ok=True)
shutil.copy2(root / "assets/negs-icon.png", icon)
applications = data / "applications"
applications.mkdir(parents=True, exist_ok=True)
exec_path = str(launcher).replace('\\', '\\\\').replace('"', '\\"').replace('`', '\\`').replace('$', '\\$').replace('%', '%%')
(applications / "dev.negs.app.UiPreview.desktop").write_text(
    '[Desktop Entry]\nType=Application\nName=Negs UI Preview\n'
    'Comment=Native photograph editor interface preview\n'
    f'Exec="{exec_path}" %f\nIcon=dev.negs.app.UiPreview\nTerminal=false\n'
    'Categories=Graphics;Photography;\nStartupWMClass=dev.negs.app.UiPreview\n'
    'MimeType=image/jpeg;image/png;image/tiff;image/x-nikon-nef;image/x-adobe-dng;\n'
)
if (config / "hypr/hyprland.lua").is_file():
    runpy.run_path(str(root / "linux/install_omarchy_rules.py"), run_name="__main__")
    if not args.no_reload and os.environ.get("HYPRLAND_INSTANCE_SIGNATURE"):
        subprocess.run(["hyprctl", "reload"], check=True)
        errors = subprocess.check_output(["hyprctl", "configerrors"], text=True).strip()
        if errors:
            raise SystemExit("Installed, but Hyprland reported configuration errors:\n" + errors)
if shutil.which("update-desktop-database"):
    subprocess.run(["update-desktop-database", str(applications)], check=True)
print(f"Installed {launcher}")
print("Launch Negs UI Preview from the app launcher, or run:", launcher)
