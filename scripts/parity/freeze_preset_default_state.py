"""Freeze upstream experimental's default GUI state for preset import.

Python spektrafilm's GUI loads a saved state by merging it into
`PROJECT_DEFAULT_GUI_STATE` (the digested Kodak Gold 200 / Supra Endura
state), so a file's missing fields mean that state's values. This writes the
state as `spektrafilm_gui.persistence.gui_state_to_dict` saves it, the file
form `crates/spektrafilm-core/src/presets.rs` merges imported presets onto.
The output must never be populated from Rust results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/freeze_preset_default_state.py \
        --python-source <28bf883 tree> \
        --output data/presets/spektrafilm-gui-default-28bf883.json
"""

import argparse
import json
import os
from pathlib import Path
import sys
import tempfile


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.python_source.resolve()
    cache = Path(tempfile.mkdtemp(prefix="freeze-preset-default-"))
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(cache / folder))
    os.environ.setdefault("QT_QPA_PLATFORM", "offscreen")
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import spektrafilm_gui
    from spektrafilm_gui.persistence import gui_state_to_dict
    from spektrafilm_gui.state import PROJECT_DEFAULT_GUI_STATE

    if not Path(spektrafilm_gui.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm_gui.__file__}")
    state = gui_state_to_dict(PROJECT_DEFAULT_GUI_STATE)
    # As the GUI saves it (persistence.save_gui_state_to_path), with a newline.
    args.output.write_text(json.dumps(state, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
