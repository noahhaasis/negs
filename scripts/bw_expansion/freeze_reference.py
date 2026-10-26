#!/usr/bin/env python3
"""Freeze independent Python outputs after checking the pinned reference tree.

The only permitted changes to the extracted Python tree are the two new
profiles, which must match this repository byte-for-byte. No Rust outputs are
used as expected values. The full comparison report and raw arrays are checked.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

import numpy as np
import OpenImageIO as oiio

ROOT = Path(__file__).resolve().parents[2]
COMMIT = "28bf883e1672e884307edc75852549376e13644e"
NEW = {"kodak_trix400_d76_8min", "ilford_mgrc_deluxe_grade2"}


def require(ok, message):
    if not ok:
        raise ValueError(message)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--python-source", type=Path, required=True)
    parser.add_argument("--git-source", type=Path, required=True)
    parser.add_argument("--parity-dir", type=Path, required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "crates/spektrafilm-core/tests/fixtures/bw_expansion_python.json")
    args = parser.parse_args()
    tree = subprocess.check_output(["git", "-C", str(args.git_source), "ls-tree", "-rz", COMMIT, "src/spektrafilm"])
    expected_paths = set()
    digest = hashlib.sha256()
    for entry in tree.split(b"\0"):
        if not entry:
            continue
        meta, name = entry.split(b"\t")
        path = name.decode()
        expected_paths.add(path)
        blob = subprocess.check_output(["git", "-C", str(args.git_source), "cat-file", "blob", meta.split()[2].decode()])
        actual = (args.python_source / path).read_bytes()
        require(actual == blob, f"Modified pinned reference: {path}")
        digest.update(name+b"\0"+hashlib.sha256(blob).digest())
    allowed = {f"src/spektrafilm/data/profiles/{n}.json" for n in NEW}
    actual_paths = {p.relative_to(args.python_source).as_posix() for p in (args.python_source / "src/spektrafilm").rglob("*")
                    if p.is_file() and "__pycache__" not in p.parts and p.suffix != ".pyc"}
    require(actual_paths == expected_paths | allowed, "Unexpected files in reference tree")
    for n in NEW:
        require((args.python_source / f"src/spektrafilm/data/profiles/{n}.json").read_bytes() == (ROOT / f"data/profiles/{n}.json").read_bytes(), f"Profile mismatch: {n}")
    report = json.loads((args.parity_dir / "report.json").read_text())
    require(report["passed"] and report["atol"] == 1e-6 and report["rtol"] == 0, "Invalid comparison report")
    matrix = {f"{fixture}-lut-{lut}" for fixture in ["midgray", "gradient4", "ramp16", "gamut-stress4", "adversarial32"]
              for lut in ["off", "on"]}
    require(matrix <= {case["id"] for case in report["cases"]}, "Incomplete comparison matrix")
    for case in report["cases"]:
        require(set(case["stages"]) == {"log_e_film", "cmy_film", "cmy_print", "rgb_out"}, "Missing stage")
        for stage in case["stages"]:
            p = args.parity_dir / case["id"]
            py = np.load(p / f"python-{stage}.npy")
            if py.shape[-1] == 1:
                py = np.repeat(py, 3, axis=2)
            rust = np.fromfile(p / f"rust-{stage}.f64", dtype="<f8").reshape(py.shape)
            require(np.isfinite(py).all() and np.isfinite(rust).all() and np.max(abs(py-rust)) <= 1e-6, f"Invalid raw arrays: {case['id']} {stage}")
    fixture = {"provenance": {"python_commit": COMMIT, "pinned_tree_sha256": digest.hexdigest(),
                "new_profiles_sha256": {n: hashlib.sha256((ROOT / f"data/profiles/{n}.json").read_bytes()).hexdigest() for n in sorted(NEW)},
                "method": "Python SimulationPipeline outputs from check_parity.py; full pinned src/spektrafilm tree checked against Git objects; only two identical new profiles added. No physical reference claim."}, "cases": []}
    for name in ["gradient4-lut-off", "gamut-stress4-lut-off", "adversarial32-lut-off"]:
        path = args.parity_dir / name
        image = oiio.ImageBuf(str(path / "input.tif")).get_pixels(oiio.FLOAT)
        # Save full input, including its float32 rounding, and Python output.
        fixture["cases"].append(dict(name=name, film=report["film"], paper=report["paper"],
            width=image.shape[1], height=image.shape[0], input_rgb=image.astype(float).ravel().tolist(),
            expected_rgb=np.load(path / "python-rgb_out.npy").ravel().tolist(),
            params=json.loads((path / "params.json").read_text())))
    args.output.write_text(json.dumps(fixture, indent=2, allow_nan=False)+"\n")
    print(f"Verified {len(expected_paths)} pinned files; froze {len(fixture['cases'])} independent Python cases.")


if __name__ == "__main__":
    main()
