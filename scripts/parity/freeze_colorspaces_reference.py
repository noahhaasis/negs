"""Freeze colour-space references from Python experimental 28bf883 and colour-science.

Covers every RGB conversion the engine ports: the colourspace matrices,
whitepoints and CCTFs (`colour.RGB_COLOURSPACES`), the CAT02/CAT16 inverses,
the illuminant whites the input CAT adapts to, the input projection
(`_rgb_to_tc_b`), the scan's `XYZ_to_RGB`, the output gamut compressor and the
output encoding (`RGB_to_RGB(cs, cs, apply_cctf_encoding=True)`) for each
output space, and a few full pipelines across input and output spaces.
crates/spektrafilm-core/tests/colorspaces.rs checks Rust against it. The
output must never be populated from Rust results.

    ~/Projects/spektrafilm-env/bin/python -I scripts/parity/freeze_colorspaces_reference.py \
        --python-source <28bf883 checkout> \
        --output crates/spektrafilm-core/tests/fixtures/colorspaces_python_28bf883.json
"""

import argparse
import dataclasses
import hashlib
import importlib.metadata
import json
import math
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import apply_params, python_overrides, shared_params, tree_hash  # noqa: E402
from freeze_gamut_reference import COMMIT, ROOT, pinned_tree_hash, require  # noqa: E402

# The spaces of scripts/gen_colorspaces.py.
SPACES = [
    "sRGB", "DCI-P3", "Display P3", "Adobe RGB (1998)", "ITU-R BT.2020",
    "ProPhoto RGB", "ACES2065-1", "ACEScg", "DaVinci Wide Gamut", "V-Gamut",
]
ILLUMINANTS = ["D50", "D55", "D65", "T", "K75P"]
# (name, film, paper, scan_film, input space, decode, output space, encode,
# output algorithm).
PIPELINES = [
    ("prophoto-srgb", "kodak_portra_400", "kodak_portra_endura", False,
     "ProPhoto RGB", False, "sRGB", True, "oklch"),
    ("rec2020-p3", "kodak_portra_400", "kodak_portra_endura", False,
     "ITU-R BT.2020", False, "Display P3", True, "oklch"),
    ("aces-prophoto", "kodak_portra_400", "kodak_portra_endura", False,
     "ACES2065-1", False, "ProPhoto RGB", True, "oklch"),
    ("p3-rec2020-scan", "fujifilm_provia_100f", "kodak_portra_endura", True,
     "Display P3", True, "ITU-R BT.2020", True, "cam16ucs"),
    ("vlog-acescg", "kodak_portra_400", "kodak_portra_endura", False,
     "V-Gamut", True, "ACEScg", False, "cam16ucs"),
    ("dwg-adobe", "kodak_gold_200", "fujifilm_crystal_archive_typeii", False,
     "DaVinci Wide Gamut", True, "Adobe RGB (1998)", False, "oklrab"),
    ("adobe-dcip3", "kodak_portra_400", "kodak_portra_endura", False,
     "Adobe RGB (1998)", True, "DCI-P3", False, "oklch"),
]
# Output gamut compressor cases: oklch (the default) for every space, the
# other perceptual algorithms for spaces with other whitepoints.
GAMUT = [("oklch", s) for s in SPACES] + [
    ("cam16ucs", s) for s in ["sRGB", "DCI-P3", "ProPhoto RGB", "ACES2065-1"]
] + [("oklrab", s) for s in ["Display P3", "ProPhoto RGB"]] + [("aces_rgc", "ProPhoto RGB")]


def finite_or_none(values):
    return [None if math.isnan(v) else v for v in values]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--spektrafilm-repo", type=Path, default=ROOT.parent / "spektrafilm",
                        help="upstream git repository holding the pinned commit")
    args = parser.parse_args()
    source = args.python_source.resolve()
    source_hash = tree_hash(source / "src/spektrafilm")
    require(source_hash == pinned_tree_hash(args.spektrafilm_repo, COMMIT),
            "Python reference tree differs from pinned commit")
    sys.dont_write_bytecode = True
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"),
                             ("XDG_CACHE_HOME", "cache")]:
        require(variable in os.environ, f"set {variable} to a scratch {folder} directory")
    sys.path.insert(0, str(source / "src"))
    import colour
    import numpy as np
    from opt_einsum import contract
    from spektrafilm import digest_params, init_params
    from spektrafilm.config import STANDARD_OBSERVER_CMFS
    from spektrafilm.model.illuminants import standard_illuminant
    from spektrafilm.runtime.params_schema import OutputGamutCompressSpec
    from spektrafilm.runtime.pipeline import SimulationPipeline
    from spektrafilm.utils.gamut_compression import compress_rgb
    from spektrafilm.utils.spectral_upsampling import _illuminant_to_xy, _rgb_to_tc_b

    f32 = lambda a: np.asarray(a, dtype=np.float32).astype(np.float64)  # noqa: E731
    xyz_of_xy = lambda xy: colour.xyY_to_XYZ(colour.xy_to_xyY(np.asarray(xy)))  # noqa: E731

    cctf_x = np.array([-0.5, -0.01, -1e-4, 0.0, 1e-5, 0.0015, 0.002, 0.0026, 0.004, 0.01,
                       0.018, 0.0275, 0.04, 0.0405, 0.0813, 0.125, 0.18, 0.181, 0.25, 0.5,
                       0.75, 1.0, 1.5, 4.0])
    colourspaces = {}
    for name in SPACES:
        cs = colour.RGB_COLOURSPACES[name]
        with np.errstate(invalid="ignore", divide="ignore"):
            encoded = np.asarray(cs.cctf_encoding(cctf_x), dtype=np.float64)
            decoded = np.asarray(cs.cctf_decoding(cctf_x), dtype=np.float64)
        colourspaces[name] = {
            "whitepoint_xyz": xyz_of_xy(cs.whitepoint).tolist(),
            "rgb_to_xyz": cs.matrix_RGB_to_XYZ.tolist(),
            "xyz_to_rgb": cs.matrix_XYZ_to_RGB.tolist(),
            "rgb_to_rgb_self": colour.matrix_RGB_to_RGB(cs, cs).tolist(),
            "cctf_encoded": finite_or_none(encoded.tolist()),
            "cctf_decoded": finite_or_none(decoded.tolist()),
        }
    transforms = colour.adaptation.CHROMATIC_ADAPTATION_TRANSFORMS
    cat = {name: np.linalg.inv(transforms[name]).tolist() for name in ["CAT02", "CAT16"]}
    whites = {name: xyz_of_xy(_illuminant_to_xy(name)).tolist() for name in ILLUMINANTS}

    # Input projection under the D55 reference illuminant, linear and decoded.
    rgb = f32([[0.18, 0.18, 0.18], [0.9, 0.1, 0.05], [0.05, 0.6, 0.2], [0.1, 0.2, 0.8],
               [1.5, 0.7, 0.2], [0.02, 0.01, 0.015], [0.5, 0.45, 0.4], [0.3, 0.0, 0.6]])
    tc_b = []
    for name in SPACES:
        for decode in [False, True]:
            tc, b = _rgb_to_tc_b(rgb, color_space=name, apply_cctf_decoding=decode,
                                 reference_illuminant="D55")
            tc_b.append({"color_space": name, "decode": decode,
                         "tc": np.asarray(tc).tolist(), "b": np.asarray(b).tolist()})

    # The scan's XYZ_to_RGB under the print viewing lamp (K75P).
    illuminant = standard_illuminant("K75P")
    normalization = np.sum(illuminant * STANDARD_OBSERVER_CMFS[:, 1], axis=0)
    illuminant_xyz = contract("k,kl->l", illuminant, STANDARD_OBSERVER_CMFS[:]) / normalization
    illuminant_xy = colour.XYZ_to_xy(illuminant_xyz)
    xyz = np.array([[0.2, 0.18, 0.15], [0.5, 0.3, 0.05], [0.1, 0.2, 0.6], [0.9, 1.0, 1.1],
                    [0.001, 0.0012, 0.0009]])
    scan = {name: np.asarray(colour.XYZ_to_RGB(xyz, colourspace=name, apply_cctf_encoding=False,
                                               illuminant=illuminant_xy)).tolist()
            for name in SPACES}

    # Output gamut compressor at the default knees, per algorithm and space.
    gamut_rgb = np.array([[0.5, 0.4, 0.3], [0.18, 0.18, 0.18], [1.4, 0.1, 0.05],
                          [-0.1, 0.9, 0.2], [0.05, 0.1, 1.3], [1.2, 1.1, 1.0],
                          [0.0, 0.0, 0.0], [0.6, -0.05, 0.7]])
    gamut = []
    for algorithm, name in GAMUT:
        spec = dataclasses.replace(OutputGamutCompressSpec(), algorithm=algorithm)
        gamut.append({"algorithm": algorithm, "color_space": name,
                      "knee": list(spec.knee), "lightness_compression": list(spec.lightness_compression),
                      "expected": compress_rgb(gamut_rgb, spec, output_color_space=name).tolist()})

    # The scan's output encoding, unclipped like the scan: highlights above 1
    # and the unsharp mask's undershoot below 0 included (NaN for the pure
    # gammas there).
    encode_rgb = np.array([[0.01, 0.0015, 0.0031], [0.18, 0.5, 0.97], [0.9, 0.05, 0.3],
                           [0.004, 0.02, 0.75], [1.0, 1.3, 4.0], [-0.002, 0.2, -0.05]])
    with np.errstate(invalid="ignore"):
        encoded_output = {
            name: finite_or_none(np.asarray(colour.RGB_to_RGB(
                encode_rgb, name, name, apply_cctf_decoding=False, apply_cctf_encoding=True,
            )).ravel().tolist())
            for name in SPACES
        }

    # Whole pipelines, 4x4, experimental defaults without grain and glare.
    i, j = np.indices((4, 4))
    image = np.stack([(0.05 + 0.2 * i + 0.03 * j) % 0.95, (0.1 + 0.15 * j + 0.05 * i) % 0.9,
                      (0.6 - 0.1 * i + 0.07 * j) % 0.85], axis=-1)
    image = f32(np.clip(image, 0.02, 0.9))
    pipelines = []
    for (name, film, paper, scan_film, in_space, decode, out_space, encode, algorithm) in PIPELINES:
        overrides = shared_params(False, defaults=True, input_color_space=in_space,
                                  input_cctf_decoding=decode, output_color_space=out_space,
                                  output_cctf_encoding=encode)
        overrides["io"]["scan_film"] = scan_film
        overrides["io"]["output_gamut_compress"] = {"algorithm": algorithm}
        params = digest_params(apply_params(init_params(film, paper),
                                            python_overrides(overrides, "experimental")))
        require(params.io.output_gamut_compress.algorithm == algorithm, "gamut override lost")
        expected = np.asarray(SimulationPipeline(params).process(image.copy()), dtype=np.float64)
        pipelines.append({
            "name": name, "film": film, "paper": paper, "scan_film": scan_film,
            "params": overrides, "input": image.ravel().tolist(),
            # Unclipped, like the scan; NaN where a pure gamma meets the
            # unsharp mask's undershoot.
            "expected_rgb": finite_or_none(expected.ravel().tolist()),
        })

    fixture = {
        "reference_commit": COMMIT,
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_colorspaces_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba", "opt_einsum"]},
        "colourspaces": colourspaces,
        "cctf_x": cctf_x.tolist(),
        "cat_inverse": cat,
        "illuminant_whites": whites,
        "tc_b_rgb": rgb.tolist(),
        "tc_b": tc_b,
        "scan_xyz": xyz.tolist(),
        "scan_rgb": scan,
        "gamut_rgb": gamut_rgb.tolist(),
        "gamut": gamut,
        "encode_rgb": encode_rgb.ravel().tolist(),
        "encoded_output": encoded_output,
        "pipeline_cases": pipelines,
        "profile_sha256": {
            stock: hashlib.sha256((source / "src/spektrafilm/data/profiles" / f"{stock}.json")
                                  .read_bytes()).hexdigest()
            for stock in sorted({p[1] for p in PIPELINES} | {p[2] for p in PIPELINES})
        },
    }
    args.output.write_text(json.dumps(fixture, indent=1, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
