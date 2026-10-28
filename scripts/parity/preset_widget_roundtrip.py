"""Measure what Python spektrafilm's GUI does to a preset Negs exports.

Python's GUI loads a preset into its widgets and renders from them, and its
spin boxes hold fewer decimals than a Negs export. For each preset (Negs's
export of every .json under --preset-dir, through the native helper's
`preset_import` and `preset_export`), this loads the file as the GUI does,
pushes it into the widgets (offscreen Qt), collects it back, and reports
every value the widgets changed. It then renders both states in Python
(enlarger and scanner LUTs off, grain and glare off) and reports the
largest rgb_out difference, in 8-bit levels of the sRGB-encoded output.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/preset_widget_roundtrip.py \
        --python-source <28bf883 tree> --helper target/release/spektrafilm-native \
        --preset-dir <presets> --output-dir /tmp/negs-preset-widgets
"""

import argparse
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import ROOT, git_info, make_fixtures  # noqa: E402
from check_presets import Helper, leaves  # noqa: E402


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True)
    parser.add_argument("--helper", type=Path, required=True, help="spektrafilm-native")
    parser.add_argument("--preset-dir", type=Path, action="append", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args(argv)
    source = args.python_source.resolve()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(out / folder))
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    from qtpy import QtWidgets
    from spektrafilm import digest_params
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm_gui.params_mapper import build_params_from_state
    from spektrafilm_gui.persistence import gui_state_from_dict, gui_state_to_dict
    from spektrafilm_gui.state_bridge import apply_gui_state, collect_gui_state
    from spektrafilm_gui.widgets import create_widget_bundle

    app = QtWidgets.QApplication.instance() or QtWidgets.QApplication([])
    widgets = create_widget_bundle()
    helper = Helper(args.helper.resolve(), ROOT / "data")
    image = make_fixtures(["scene"], np)["scene"].astype(np.float64)

    def render(state):
        params = build_params_from_state(state)
        params.settings.use_enlarger_lut = False
        params.settings.use_scanner_lut = False
        params.film_render.grain.active = False
        params.film_render.glare.active = False
        params.print_render.glare.active = False
        params.io.input_color_space = "sRGB"
        params.io.input_cctf_decoding = False
        params = digest_params(params, apply_stocks_specifics=False)
        return np.asarray(SimulationPipeline(params).process(image), dtype=np.float64)

    report = {"python": git_info(source), "rust": git_info(ROOT), "presets": []}
    for directory in args.preset_dir:
        for file in sorted(directory.rglob("*.json")):
            imported = helper.call("preset_import", text=file.read_text(encoding="utf-8"))
            text = helper.call("preset_export", film=imported["film"], paper=imported["paper"],
                               params=imported["params"])["text"]
            (out / f"{file.stem}.json").write_text(text, encoding="utf-8")
            loaded = gui_state_from_dict(json.loads(text))
            apply_gui_state(loaded, widgets=widgets)
            app.processEvents()
            collected = collect_gui_state(widgets=widgets)
            before, after = dict(leaves(gui_state_to_dict(loaded))), dict(leaves(gui_state_to_dict(collected)))
            changed = {}
            for path, value in before.items():
                if path.startswith(("display.", "convert.")) or after.get(path) == value:
                    continue
                if isinstance(value, (list, tuple)):
                    value = list(value)
                changed[path] = {"export": value, "widgets": after.get(path)}
            levels = int(np.abs(np.rint(np.clip(render(loaded), 0, 1) * 255)
                                - np.rint(np.clip(render(collected), 0, 1) * 255)).max())
            entry = {"preset": file.stem, "changed": changed, "rgb_out_max_8bit_levels": levels}
            report["presets"].append(entry)
            print(f"{file.stem[:34]:34} {len(changed):3} values changed, render differs by up to {levels} levels",
                  flush=True)
            for path, change in changed.items():
                print(f"    {path}: {change['export']} -> {change['widgets']}", flush=True)
    (out / "report.json").write_text(json.dumps(report, indent=2, default=str) + "\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
