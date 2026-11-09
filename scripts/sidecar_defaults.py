#!/usr/bin/env python3
"""Writes a new frozen defaults table for photo sidecars (docs/sidecar-format.md).

A sidecar stores only what differs from the table it names in
`defaultsVersion`, so a table is never edited once released: when the engine's
or the app's defaults change, write the next version and raise
`SidecarDefaults.latest` in macos/Sources/NegsApp/Sidecar.swift. The macOS tests
fail while the newest table differs from what the app starts with, and hash
every table so an old one can't change unnoticed.

    scripts/sidecar_defaults.py --helper target/release/spektrafilm-native --version 2
"""
import argparse
import json
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# What a new photograph starts with besides the engine's parameters, as the
# macOS app encodes `EditState()`, `BorderSettings()` and `CropSettings()`.
# `SidecarDefaultsTests` checks these against the app.
FILM = "kodak_gold_200"
PAPER = "kodak_portra_endura"
BORDER = {"enabled": False, "style": "white", "export": True, "instagram": False, "width": 0.03,
          "filmWidth": 0, "line": 0.004, "color": "#ffffff", "lineColor": "#000000"}
CROP = {"rect": [[0, 0], [1, 1]], "aspect": "original", "custom": [5, 4], "angle": 0}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--helper", type=Path, default=ROOT / "target/release/spektrafilm-native")
    parser.add_argument("--version", type=int, required=True)
    args = parser.parse_args()
    out = ROOT / f"data/sidecar-defaults/v{args.version}.json"
    if out.exists():
        raise SystemExit(f"{out} exists: released tables are frozen. Pick the next version.")
    reply = subprocess.run([str(args.helper), "--data-dir", str(ROOT / "data")], input='{"id":1,"command":"catalog"}\n',
                           capture_output=True, text=True, check=True).stdout
    params = json.loads(reply)["result"]["defaults"]
    # The engine leaves lens correction off for params from before it; the
    # app corrects every new edit (`EditorModel.apply(catalog:)`).
    params["io"]["lens_correction"]["enabled"] = True
    table = {"defaultsVersion": args.version, "film": FILM, "paper": PAPER, "params": params, "border": BORDER, "crop": CROP}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(table, indent=2, sort_keys=True) + "\n")
    print(out)


if __name__ == "__main__":
    main()
