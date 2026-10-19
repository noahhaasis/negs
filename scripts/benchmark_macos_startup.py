#!/usr/bin/env python3
"""Alternate baseline/candidate real-window benchmarks; build both with
-DSPEKTRAFILM_BENCHMARK. Does not read/write the user's saved editing session.
"""
import argparse
import json
import os
from pathlib import Path
import re
import statistics
import subprocess
import threading
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", required=True, type=Path)
    parser.add_argument("--candidate", required=True, type=Path)
    parser.add_argument("--root", required=True, type=Path)
    parser.add_argument("--photo", required=True, type=Path)
    parser.add_argument("--session", type=Path, help="Also benchmark candidate restoring this isolated session")
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    results = []
    variants = [("baseline", args.baseline, None), ("candidate", args.candidate, None)]
    if args.session:
        variants.append(("restored", args.candidate, args.session))
    for run in range(args.runs):
        for name, executable, session in variants if run % 2 == 0 else reversed(variants):
            env = dict(os.environ, SPEKTRAFILM_ROOT=str(args.root.resolve()), SPEKTRAFILM_BENCHMARK_PHOTO=str(args.photo.resolve()))
            env.pop("SPEKTRAFILM_BENCHMARK_SESSION", None)
            if session:
                env["SPEKTRAFILM_BENCHMARK_SESSION"] = str(session.resolve())
            started = time.perf_counter()
            process = subprocess.Popen([str(executable.resolve())], env=env, stderr=subprocess.PIPE, stdout=subprocess.DEVNULL, text=True)
            timeout = threading.Timer(90, process.kill)
            timeout.daemon = True
            timeout.start()
            record = dict(variant=name, run=run + 1)
            for line in process.stderr:
                if "benchmark:" not in line:
                    continue
                print(name, line.strip(), flush=True)
                elapsed = (time.perf_counter() - started) * 1000
                if match := re.search(r"editor ready in (\d+) ms", line):
                    record.update(ready_after_launch_ms=int(match[1]), ready_from_process_ms=round(elapsed, 1))
                if match := re.search(r"opened and developed in (\d+) ms", line):
                    record.update(preview_after_launch_ms=int(match[1]), preview_from_process_ms=round(elapsed, 1))
                if "slider drag:" in line:
                    record["interaction"] = line.strip()
            code = process.wait(timeout=90)
            timeout.cancel()
            if code != 0 or "preview_from_process_ms" not in record:
                raise RuntimeError(f"Benchmark did not complete: {record}")
            results.append(record)
    summary = {name: {metric: statistics.median(row[metric] for row in results if row["variant"] == name)
                      for metric in ["ready_from_process_ms", "preview_from_process_ms", "ready_after_launch_ms", "preview_after_launch_ms"]}
               for name, _, _ in variants}
    args.output.write_text(json.dumps(dict(summary=summary, runs=results), indent=2) + "\n")
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
