"""Freeze independent gamut references from Python experimental 28bf883.

See docs/handoff-experimental-gamut.md for the live runs and freeze command.
The output must never be populated from Rust results.
"""

import argparse
import dataclasses
import hashlib
import importlib.metadata
import io
import json
import os
from pathlib import Path
import subprocess
import sys
import tarfile

from check_parity import apply_params, tree_hash


COMMIT = "28bf883e1672e884307edc75852549376e13644e"
MODULE = "src/spektrafilm/utils/gamut_compression.py"
ROOT = Path(__file__).resolve().parents[2]


def require(condition, message):
    if not condition:
        raise ValueError(message)


def pinned_tree_hash(repo, commit):
    """Use Git objects to verify the entire reference, including profiles/LUTs."""
    archive = subprocess.check_output(["git", "-C", str(repo), "archive", commit, "src/spektrafilm"])
    digest = hashlib.sha256()
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        for member in sorted(tar.getmembers(), key=lambda m: m.name):
            if member.isfile():
                name = Path(member.name).relative_to("src/spektrafilm").as_posix()
                digest.update(name.encode() + b"\0")
                digest.update(hashlib.sha256(tar.extractfile(member).read()).digest())
    return digest.hexdigest()


def validate_report(report, source_hash):
    require(report.get("passed") is True, "reference run did not pass")
    require(report.get("experimental_gamut") and report.get("gamut_stress"), "missing gamut/stress settings")
    require(report.get("python", {}).get("tree_sha256") == source_hash, "reference tree hash mismatch")
    require(0 <= report.get("atol", float("inf")) <= 1e-6 and report.get("rtol") == 0, "invalid tolerance")
    cases = report.get("cases", [])
    required = {f"{name}-lut-{mode}" for name in ["midgray", "gradient4", "ramp16", "gamut-stress4"] for mode in ["off", "on"]}
    require(required <= {c.get("id") for c in cases}, "incomplete reference run")
    stages = {"log_e_film", "cmy_film", "rgb_out"}
    if not report.get("scan_film"):
        stages.add("cmy_print")
    require(all(c.get("passed") is True and set(c.get("stages", {})) == stages and
                all(s.get("passed") is True and 0 <= s.get("max_abs", float("inf")) <= 1e-6
                    for s in c["stages"].values()) for c in cases), "invalid stage results")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--bw-runs", nargs="+", type=Path, required=True,
                        help="passing check_parity --experimental-gamut --gamut-stress directories")
    args = parser.parse_args()
    source = args.python_source.resolve()
    # Verify the implementation even for git archives without a .git directory.
    pinned = subprocess.check_output(["git", "-C", str(ROOT.parent / "spektrafilm"),
                                      "show", f"{COMMIT}:{MODULE}"])
    if (source / MODULE).read_bytes() != pinned:
        parser.error(f"reference gamut implementation differs from {COMMIT}")
    source_hash = tree_hash(source / "src/spektrafilm")
    require(source_hash == pinned_tree_hash(ROOT.parent / "spektrafilm", COMMIT),
            "full Python reference tree differs from pinned commit")
    sys.dont_write_bytecode = True
    os.environ.setdefault("NUMBA_CACHE_DIR", "/tmp/spektrafilm-gamut-numba")
    os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-gamut-mpl")
    os.environ.setdefault("XDG_CACHE_HOME", "/tmp/spektrafilm-gamut-cache")
    sys.path.insert(0, str(source / "src"))
    import numpy as np
    from spektrafilm.utils.gamut_compression import (
        InputGamutCompressSpec, OutputGamutCompressSpec, compress_rgb,
        inscribed_locus_hull, remap_tc_lut_for_compression,
    )

    preset = json.loads((ROOT / "data/presets/experimental-gamut.json").read_text())["io"]
    for values in preset.values():
        for name in ["knee", "lightness_compression"]:
            if name in values:
                values[name] = tuple(values[name])
    input_spec = InputGamutCompressSpec(**{k: v for k, v in preset["input_gamut_compress"].items() if k != "center"})
    output_spec = OutputGamutCompressSpec(**preset["output_gamut_compress"])
    # Fail if the claimed experimental defaults ever get out of sync.
    require(input_spec == InputGamutCompressSpec(), "input preset differs from pinned defaults")
    require(output_spec == OutputGamutCompressSpec(), "output preset differs from pinned defaults")
    size = 16
    i, j, c = np.indices((size, size, 3))
    lut = ((i * i + 3 * j + 7 * c) % 97) / 97.0
    cases = []
    for white, detail in [([0.3127, 0.3290], 5.0), ([0.33242, 0.34743], 2.0),
                          ([0.3127, 0.3290], 12.0)]:
        spec = dataclasses.replace(input_spec, hull_detail=detail)
        hull = inscribed_locus_hull(np.array(white), detail)
        cases.append({"white": white, "spec": dataclasses.asdict(spec),
                      "hull_every_64": hull[::64].tolist(),
                      "remapped_lut": remap_tc_lut_for_compression(lut, np.array(white), spec).ravel().tolist()})
    rgb = np.array([[0, 0, 0], [1e-6, 1e-6, 1e-6], [0.18, 0.18, 0.18],
                    [1, 1, 1], [2, 2, 2], [1, 0, 0], [0, 1, 0], [0, 0, 1],
                    [1.7, -0.1, 0.3], [-0.1, 1.5, 1.5], [0.05, 0.15, 0.45]])
    fixture = {
        "reference_commit": COMMIT,
        "module_sha256": hashlib.sha256(pinned).hexdigest(),
        "source_tree_sha256": source_hash,
        "generator": "scripts/parity/freeze_gamut_reference.py",
        "dependencies": {name: importlib.metadata.version(name) for name in
                         ["numpy", "scipy", "colour-science", "numba", "OpenImageIO"]},
        "lut_recipe": "((i*i + 3*j + 7*c) % 97) / 97.0, shape (16,16,3)",
        "input_cases": cases,
        "output": {"spec": dataclasses.asdict(output_spec), "rgb": rgb.tolist(),
                   "expected": compress_rgb(rgb, output_spec, output_color_space="sRGB").tolist()},
    }
    if args.bw_runs:
        import OpenImageIO as oiio
        from spektrafilm import init_params, digest_params
        from spektrafilm.runtime.pipeline import SimulationPipeline
        pipeline_cases = []
        for run in args.bw_runs:
            report = json.loads((run / "report.json").read_text())
            validate_report(report, source_hash)
            for name in ["gradient4-lut-off", "gamut-stress4-lut-off"]:
                directory = run / name
                params = json.loads((directory / "params.json").read_text())
                require(params["io"]["input_gamut_compress"] == json.loads(json.dumps(preset["input_gamut_compress"])), "input preset mismatch")
                require(params["io"]["output_gamut_compress"] == json.loads(json.dumps(preset["output_gamut_compress"])), "output preset mismatch")
                image = oiio.ImageBuf(str(directory / "input.tif"))
                pixels = image.get_pixels(oiio.FLOAT)
                require(pixels.shape == (4, 4, 3) and np.isfinite(pixels).all(), "invalid input pixels")
                # Recompute the reference instead of trusting the saved pass
                # flag or arrays (which could have changed after that run).
                reference = init_params(report["film"], report["paper"])
                overrides = json.loads(json.dumps(params))
                for key in ["input_gamut_compress", "output_gamut_compress", "scan_film"]:
                    overrides["io"].pop(key, None)
                for group in ["film_render", "print_render"]:
                    time = overrides[group].pop("development_time", None)
                    if time is not None:
                        rendering = getattr(reference, group)
                        rendering.chemistry = dataclasses.replace(rendering.chemistry, development_time=time)
                apply_params(reference, overrides)
                reference.io.input_gamut_compress = input_spec
                reference.io.output_gamut_compress = output_spec
                if report["scan_film"]:
                    reference.workflow.route = "input > film > scan"
                expected = SimulationPipeline(digest_params(reference)).process(pixels.astype(np.float64))
                saved = np.load(directory / "python-rgb_out.npy")
                require(saved.shape == expected.shape and np.isfinite(saved).all() and
                        np.allclose(saved, expected, atol=1e-12, rtol=0), "saved Python output differs from fresh reference")
                pipeline_cases.append({
                    "name": name, "film": report["film"], "paper": report["paper"],
                    "scan_film": report["scan_film"],
                    "reference_tree_sha256": report["python"]["tree_sha256"],
                    "params": params, "input": pixels.ravel().tolist(),
                    "expected_rgb": expected.ravel().tolist(),
                })
        fixture["pipeline_cases"] = pipeline_cases
        stocks = {case["film"] for case in pipeline_cases} | {case["paper"] for case in pipeline_cases if not case["scan_film"]}
        fixture["profile_sha256"] = {
            name: hashlib.sha256((source / "src/spektrafilm/data/profiles" / f"{name}.json").read_bytes()).hexdigest()
            for name in sorted(stocks)
        }
    args.output.write_text(json.dumps(fixture, indent=2, allow_nan=False) + "\n")


if __name__ == "__main__":
    main()
