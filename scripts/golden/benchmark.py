#!/usr/bin/env python3
"""Measure full-size exports without changing the correctness harness.

Uses only Python's standard library. Keep --out under target/: logs, images
and raw dumps are private. GPU profiling is a separate run because submitting
and waiting after each stage changes scheduling (including watchdog behavior).
"""

import argparse
import datetime
import importlib.util
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

spec = importlib.util.spec_from_file_location("golden", Path(__file__).with_name("run.py"))
golden = importlib.util.module_from_spec(spec)
spec.loader.exec_module(golden)


def watchdog_events(start, end):
    """A successful export can still coincide with a Windows display reset."""
    if os.name != "nt":
        return None
    command = (
        "$ErrorActionPreference='Stop'; "
        f"$start=[DateTime]::Parse('{start}').ToLocalTime(); "
        f"$end=[DateTime]::Parse('{end}').ToLocalTime(); "
        "try { $events=@(Get-WinEvent -FilterHashtable "
        "@{LogName='System';Id=4101;StartTime=$start;EndTime=$end} "
        "-ErrorAction Stop) } catch { "
        "if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') {$events=@()} "
        "else {throw} }; "
        "$rows=@($events | ForEach-Object { "
        "@{id=$_.Id;utc=$_.TimeCreated.ToUniversalTime().ToString('o')} }); "
        "ConvertTo-Json -InputObject $rows -Compress")
    result = subprocess.run(["powershell", "-NoProfile", "-Command", command],
                            capture_output=True, text=True, timeout=30)
    if result.returncode:
        raise RuntimeError("cannot query Windows display-reset events: " + result.stderr)
    return json.loads(result.stdout)


def measure(args, engine, name, profile=False):
    binary, backend, _, _ = golden.ENGINES[engine]
    prefix = args.out / name
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("SPEKTRAFILM_", "WGPU_"))
           and k not in ("RUST_LOG", "RAYON_NUM_THREADS")}
    env.update(SPEKTRAFILM_BACKEND=backend, RUST_LOG="info", NO_COLOR="1")
    if args.pipeline_cache_dir:
        env["SPEKTRAFILM_PIPELINE_CACHE_DIR"] = str(args.pipeline_cache_dir.resolve())
    if args.disable_pipeline_cache:
        env["SPEKTRAFILM_DISABLE_PIPELINE_CACHE"] = "1"
    if profile:
        env["SPEKTRAFILM_GPU_PROFILE"] = "1"
    cmd = [str(golden.tool(args.bin_dir, binary)), "process", str(args.input),
           "--output", str(prefix.with_suffix(".tif")), "--film", args.film,
           "--paper", args.paper, "--params", str(args.out / "params.json"),
           "--timings", "--iters", str(args.iters)]
    if args.raw:
        cmd += ["--raw-out", str(prefix.with_suffix(".bin"))]

    monitor = None
    memory_file = prefix.with_suffix(".memory.csv")
    with memory_file.open("w") as memory:
        if shutil.which("nvidia-smi"):
            monitor = subprocess.Popen(
                ["nvidia-smi", "--query-gpu=timestamp,index,memory.used",
                 "--format=csv,noheader,nounits", "-lms", "100"],
                stdout=memory, stderr=subprocess.DEVNULL)
            time.sleep(0.2)
        start_utc = datetime.datetime.now(datetime.timezone.utc).isoformat()
        start = time.perf_counter()
        try:
            with prefix.with_suffix(".log").open("w", encoding="utf-8") as log:
                result = subprocess.run(cmd, cwd=golden.ROOT, env=env, stdout=log,
                                        stderr=subprocess.STDOUT, timeout=args.timeout)
            code = result.returncode
        except subprocess.TimeoutExpired:
            code = "timeout"
        finally:
            wall_ms = (time.perf_counter() - start) * 1000
            end_utc = datetime.datetime.now(datetime.timezone.utc).isoformat()
            if monitor:
                monitor.terminate()
                monitor.wait()
    log = golden.ANSI.sub("", prefix.with_suffix(".log").read_text(encoding="utf-8"))
    samples = []
    for line in memory_file.read_text().splitlines():
        fields = line.split(",")
        if len(fields) == 3 and fields[1].strip() == "0":
            try:
                samples.append(float(fields[2]))
            except ValueError:
                pass
    row = {"name": name, "engine": engine, "profiled": profile,
           "exit_code": code, "wall_ms": wall_ms,
           "pipeline_ms": [int(v) for v in re.findall(r"Pipeline(?: \(iter \d+\))?: (\d+) ms", log)],
           "load_ms": [int(v) for v in re.findall(r"Image loaded: .*?, (\d+) ms", log)],
           "save_ms": [int(v) for v in re.findall(r"Saved: .*?, (\d+) ms", log)],
           "cli_total_ms": [int(v) for v in re.findall(r"Total: (\d+) ms", log)],
           "gpu_stages_ms": [(stage, float(ms)) for stage, ms in
                             re.findall(r"gpu stage (\w+)\s+([\d.]+) ms", log)],
           "gpu_resident": "gpu-resident fast path complete" in log,
           "memory_baseline_mib": samples[0] if samples else None,
           "memory_peak_mib": max(samples) if samples else None,
           "memory_samples": len(samples)}
    row["start_utc"] = start_utc
    row["end_utc"] = end_utc
    row["watchdog_events"] = watchdog_events(start_utc, end_utc)
    row["pass"] = code == 0 and (engine != "gpu" or
                                   ("Backend: wgpu" in log and row["gpu_resident"])) \
        and not row["watchdog_events"]
    if code == 0:
        row["tiff_sha256"] = golden.sha256(prefix.with_suffix(".tif"))
        if args.raw:
            row["raw_sha256"] = golden.sha256(prefix.with_suffix(".bin"))
    return row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("input", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--label", required=True, help="public machine label; no access details")
    parser.add_argument("--bin-dir", type=Path, default=golden.ROOT / "target/release")
    parser.add_argument("--engines", nargs="+", choices=["gpu", "f64"], default=["gpu", "f64"])
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--iters", type=int, default=1)
    parser.add_argument("--timeout", type=int, default=1800)
    parser.add_argument("--raw", action="store_true")
    profile_mode = parser.add_mutually_exclusive_group()
    profile_mode.add_argument("--profile", action="store_true", help="extra GPU run with per-stage waits")
    profile_mode.add_argument("--profile-only", action="store_true", help="only the GPU run with per-stage waits")
    cache = parser.add_mutually_exclusive_group()
    cache.add_argument("--pipeline-cache-dir", type=Path)
    cache.add_argument("--disable-pipeline-cache", action="store_true")
    parser.add_argument("--film", default="kodak_portra_400")
    parser.add_argument("--paper", default="kodak_portra_endura")
    parser.add_argument("--preset", choices=["default"] + golden.PRESET_NAMES, default="default")
    args = parser.parse_args()
    if args.repeats < 1 or args.iters < 1:
        parser.error("--repeats and --iters must be positive")
    if args.profile_only:
        args.engines = ["gpu"]
    args.input = args.input.resolve()
    args.out = args.out.resolve()
    args.out.mkdir(parents=True, exist_ok=True)
    params = golden.effective_params(args.preset)
    (args.out / "params.json").write_text(json.dumps(params), encoding="utf-8")
    report = {"label": args.label, "commit": golden.command_output(["git", "rev-parse", "HEAD"]),
              "tracked_changes": bool(golden.command_output(["git", "diff", "--name-only", "HEAD"])),
              "gpu_source_sha256": golden.tree_hash(golden.ROOT / "crates/spektrafilm-gpu"),
              "input_sha256": golden.sha256(args.input), "film": args.film, "paper": args.paper,
              "params": params, "iterations_per_process": args.iters,
              "raw_dump_included_in_wall_time": args.raw,
              "pipeline_cache": "disabled" if args.disable_pipeline_cache else
                                "explicit directory" if args.pipeline_cache_dir else "default",
              "memory_note": "GPU 0 whole-device usage sampled at 100 ms; includes desktop, not process VRAM",
              "runs": []}
    for engine in args.engines:
        names = [] if args.profile_only else [(f"{engine}-{i + 1}", False) for i in range(args.repeats)]
        if engine == "gpu" and (args.profile or args.profile_only):
            names.append(("gpu-profile", True))
        for name, profile in names:
            print(f"Starting {name}", flush=True)
            row = measure(args, engine, name, profile)
            report["runs"].append(row)
            (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
            print(json.dumps(row), flush=True)
            if not row["pass"]:
                return 1
    # Same-build repetitions and profiling must preserve the exported bytes.
    report["repeat_identical"] = all(
        len({r["tiff_sha256"] for r in report["runs"] if r["engine"] == engine}) == 1
        and (not args.raw or len({r["raw_sha256"] for r in report["runs"]
                                 if r["engine"] == engine}) == 1)
        for engine in args.engines)
    (args.out / "report.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    return 0 if report["repeat_identical"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
