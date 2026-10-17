"""Compare an extracted macOS app's f32 CPU/Metal renders to live Python arrays."""

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

import numpy as np

from check_parity import metrics


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--app", type=Path, required=True)
    parser.add_argument("--reference-runs", type=Path, nargs="+", required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    app = args.app.resolve()
    binary = app / "Contents/MacOS/spektrafilm"
    report = {"binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "atol": 3e-3, "rtol": 0, "cases": []}
    for run in args.reference_runs:
        reference = json.loads((run / "report.json").read_text())
        if not reference["passed"]:
            raise ValueError(f"reference run failed: {run}")
        for name in ["gradient4-lut-on", "gamut-stress4-lut-on", "adversarial32-lut-on"]:
            directory = run / name
            expected = np.load(directory / "python-rgb_out.npy")
            for backend in ["cpu", "wgpu"]:
                case_dir = out / f"{run.name}-{name}-{backend}"
                case_dir.mkdir()
                command = [str(binary), "process", str(directory / "input.tif"),
                           "--output", str(case_dir / "render.png"), "--raw-out", str(case_dir / "rgb.f64"),
                           "--film", reference["film"], "--paper", reference["paper"],
                           "--data-dir", str(app / "Contents/Resources/data"),
                           "--params", str(directory / "params.json")]
                if reference["scan_film"]:
                    command += ["--scan-film"]
                env = {**os.environ, "SPEKTRAFILM_BACKEND": backend,
                       "PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "XDG_CACHE_HOME": str(out / "cache")}
                result = subprocess.run(command, cwd=out, env=env, capture_output=True, text=True)
                log = result.stdout + result.stderr
                (case_dir / "render.log").write_text(log)
                if result.returncode or (backend == "wgpu" and "Backend: wgpu (GPU)" not in log):
                    raise RuntimeError(f"{case_dir.name}: requested backend did not run; inspect render.log")
                actual = np.fromfile(case_dir / "rgb.f64", dtype="=f8").reshape(expected.shape)
                case = {"id": case_dir.name, "backend": backend,
                        **metrics(actual, expected, report["atol"], np)}
                report["cases"].append(case)
                report["passed"] = all(c["passed"] for c in report["cases"])
                (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
                print(f"{'PASS' if case['passed'] else 'FAIL'} {case['id']}: {case['max_abs']:.3e}", flush=True)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
