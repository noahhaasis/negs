#!/usr/bin/env python3
"""Inventory the Goose grain experiment without installing tools or rendering.

Run from any directory. The report is local and may contain private input paths;
only source-baseline.json belongs in version control.
"""

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess


ROOT = Path(__file__).resolve().parents[2]


def command(args):
    try:
        result = subprocess.run(
            args, cwd=ROOT, capture_output=True, text=True, timeout=20,
        )
        return {"command": args, "exit_code": result.returncode,
                "stdout": result.stdout.strip(), "stderr": result.stderr.strip()}
    except (OSError, subprocess.TimeoutExpired) as error:
        return {"command": args, "error": str(error)}


def file_info(path):
    path = Path(path)
    result = {"path": str(path), "exists": path.exists()}
    if path.is_file():
        result["bytes"] = path.stat().st_size
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--out", type=Path,
                        default=ROOT / "target/goose/preflight.json")
    args = parser.parse_args()
    baseline = json.loads(
        (ROOT / "experimental/goose-grain/source-baseline.json").read_text()
    )
    sources = {}
    for name, expected in baseline["files"].items():
        path = ROOT / name
        actual = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
        sources[name] = {"sha256": actual,
                         "matches_preparation": actual == expected["sha256"]}

    tools = {}
    for name, version_args in (
        ("rustc", ["--version"]), ("cargo", ["--version"]),
        ("clang", ["--version"]), ("python3", ["--version"]),
        ("cmake", ["--version"]), ("goose", ["version"]),
    ):
        executable = shutil.which(name)
        tools[name] = {"executable": executable}
        if executable:
            tools[name]["version"] = command([executable, *version_args])

    candidates = [
        ROOT / "target/memory-study/leica-q3.dng",
        ROOT / "target/release/spektrafilm-f64",
        Path.home() / "Downloads/Z 6/DSC_4990.NEF",
        Path.home() / "Downloads/7858115920.dng",
    ]
    registry = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "registry/src"
    dependencies = []
    if registry.is_dir():
        for index in sorted(registry.iterdir()):
            if index.is_dir():
                for pattern in ("rand_mt-5.0.0", "libm-0.2.16"):
                    dependencies.extend(str(path) for path in sorted(index.glob(pattern)))

    report = {
        "format": "negs-goose-preflight", "version": 1,
        "created_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "root": str(ROOT), "system": platform.platform(),
        "architecture": platform.machine(), "cpu_budget_hint": os.cpu_count(),
        "git_head": command(["git", "rev-parse", "HEAD"]),
        "git_status": command(["git", "status", "--short"]),
        "tools": tools, "sources": sources,
        "all_sources_match_preparation": all(
            item["matches_preparation"] for item in sources.values()
        ),
        "private_input_candidates": [file_info(path) for path in candidates],
        "dependency_source_directories": dependencies,
        "limitations": [
            "No build, test, render, Goose installation or benchmark was performed.",
            "Existing executable provenance has not been verified; rebuild before capture.",
            "CPU count is a hint, not a verified available worker budget.",
        ],
    }
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(json.dumps(report, indent=2) + "\n")
    print(f"Report: {args.out}")
    print(f"Sources match preparation: {report['all_sources_match_preparation']}")
    print(f"Goose available: {bool(tools['goose']['executable'])}")
    print("No rendering or benchmarks performed.")


if __name__ == "__main__":
    main()
