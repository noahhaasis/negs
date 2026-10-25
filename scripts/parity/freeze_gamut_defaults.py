"""Freeze upstream experimental's gamut-compression defaults.

Stores the `InputGamutCompressSpec` and `OutputGamutCompressSpec` dataclass
defaults, and the default `RuntimePhotoParams.io` specs, for
crates/spektrafilm-core/tests/experimental_gamut.rs. The input compression
center is not a spec field upstream: the Hanatos LUT builder hard-codes D65
(`utils/spectral_upsampling.py`), which the script checks in the source.
The output must never be populated from Rust results.

Usage, from the repository root:
    ../spektrafilm-env/bin/python -I scripts/parity/freeze_gamut_defaults.py \
        --python-source <28bf883 tree> \
        --output crates/spektrafilm-core/tests/fixtures/gamut_defaults_python_28bf883.json
"""

import argparse
import dataclasses
import json
import os
from pathlib import Path
import sys
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import git_info, tree_hash  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True, help="upstream experimental tree")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    source = args.python_source.resolve()
    cache = Path(tempfile.mkdtemp(prefix="freeze-gamut-defaults-"))
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(cache / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import spektrafilm
    from spektrafilm import init_params
    from spektrafilm.utils.gamut_compression import InputGamutCompressSpec, OutputGamutCompressSpec

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    builder = (source / "src/spektrafilm/utils/spectral_upsampling.py").read_text()
    if "remap_tc_lut_for_compression(raw_lut, d65_xy, gamut_compress)" not in builder:
        raise RuntimeError("the Hanatos LUT builder no longer centres compression on D65")
    io = init_params().io
    fixture = {
        "reference": {**git_info(source), "tree_sha256": tree_hash(source / "src/spektrafilm")},
        "input": dataclasses.asdict(InputGamutCompressSpec()),
        "input_center": "D65",
        "output": dataclasses.asdict(OutputGamutCompressSpec()),
        "params_io_input": dataclasses.asdict(io.input_gamut_compress),
        "params_io_output": dataclasses.asdict(io.output_gamut_compress),
    }
    print(json.dumps(fixture, indent=1))
    args.output.write_text(json.dumps(fixture, indent=1) + "\n")


if __name__ == "__main__":
    main()
