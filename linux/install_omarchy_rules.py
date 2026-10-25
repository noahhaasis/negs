#!/usr/bin/env python3
"""Install only Negs's opacity exception into the user's existing Hyprland config."""
from pathlib import Path
import os
import shutil
import time

source = Path(__file__).resolve().parent / "omarchy/negs-window-rules.lua"
config = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config"))) / "hypr"
main = config / "hyprland.lua"
target = config / "negs-window-rules.lua"
include = 'require("hypr.negs-window-rules")'
if not main.is_file():
    raise SystemExit(f"No existing Hyprland Lua configuration at {main}")
text = main.read_text()
rules = source.read_text()
stamp = str(time.time_ns())

def write_with_backup(path, data):
    if path.exists() and path.read_text() == data:
        return
    if path.exists():
        shutil.copy2(path, path.with_name(path.name + ".bak.negs-" + stamp))
    temporary = path.with_name(path.name + ".tmp.negs-" + stamp)
    temporary.write_text(data)
    if path.exists():
        shutil.copymode(path, temporary)
    temporary.replace(path)

write_with_backup(target, rules)
if include not in text.splitlines():
    write_with_backup(main, text.rstrip() + "\n\n-- Keep Negs photographs fully opaque.\n" + include + "\n")
print(f"Installed {target}")
print("Reload Hyprland and check configerrors before using the application.")
