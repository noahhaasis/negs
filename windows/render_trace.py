"""Summarises a render benchmark run (windows/measure_render.ps1).

For each benchmark step (one film change), from the trace (NEGS_RENDER_TRACE):
  first_ms     edit -> the first new frame presented (Vulkan present or software blit)
  settled_ms   edit -> the last settled frame presented before the step went idle
  *_composited the same, to DWM's composition after that present (DwmFlush)
  renders      the step's render requests; renders_ms their helper elapsed_ms
  matches      brightness solves in the step
Steps 1 and 2 are warm-up (the first use of each film's pipeline in this
helper) and are left out of the medians. Memory: the helper's working set at
the start of each measured step (idle) and its peak working set.

Usage: python -I render_trace.py RUN_FOLDER [--warmup N]
Writes RUN_FOLDER/summary.json and prints it.
"""

import csv
import json
import statistics
import sys
from pathlib import Path


def main():
    run = Path(sys.argv[1])
    warmup = int(sys.argv[sys.argv.index("--warmup") + 1]) if "--warmup" in sys.argv else 2
    events = [json.loads(line) for line in (run / "trace.jsonl").read_text().splitlines() if line.strip()]
    starts = [e for e in events if e["event"] == "start"]
    frequency = starts[0]["qpc_frequency"]

    def ms(a, b):
        return (b - a) * 1000.0 / frequency

    steps = []
    edits = [i for i, e in enumerate(events) if e["event"] == "bench_edit"]
    idles = [i for i, e in enumerate(events) if e["event"] == "bench_idle"]
    for index in edits:
        step = events[index]["step"]
        end = next((i for i in idles if i > index), len(events))
        window = events[index:end]
        edit = next(e for e in window if e["event"] == "edit")
        presents = [e for e in window if e["event"] == "present"]
        composited = [e for e in window if e["event"] == "composited"]
        settled = [e for e in presents if e["frame"]["kind"] == "settled"]
        settled_comp = [e for e in composited if e["frame"]["kind"] == "settled"]
        replies = [e for e in window if e["event"] == "render_reply"]
        record = {
            "step": step,
            "film": events[index]["film"],
            "first_ms": ms(edit["qpc"], presents[0]["qpc"]) if presents else None,
            "settled_ms": ms(edit["qpc"], settled[-1]["qpc"]) if settled else None,
            "first_composited_ms": ms(edit["qpc"], composited[0]["qpc"]) if composited else None,
            "settled_composited_ms": ms(edit["qpc"], settled_comp[-1]["qpc"]) if settled_comp else None,
            "frames": [e["frame"]["kind"] for e in presents],
            "renders": len([e for e in window if e["event"] == "render_request"]),
            "renders_ms": [r.get("elapsed_ms") for r in replies],
            "cached_pipeline": [r.get("cached_pipeline") for r in replies],
            "pipeline_ms": [r.get("pipeline_ms") for r in replies],
            "backend": sorted({str(r.get("backend")) for r in replies}),
            "matches": len([e for e in window if e["event"] == "match_start"]),
            "match_ms": [e.get("elapsed_ms") for e in window if e["event"] == "match_solved"],
            "qpc_start": edit["qpc"],
        }
        steps.append(record)

    measured = [s for s in steps if s["step"] > warmup]

    def median(key):
        values = [s[key] for s in measured if s[key] is not None]
        return round(statistics.median(values), 1) if values else None

    memory = {}
    samples_file = run / "helper-memory.csv"
    if samples_file.exists():
        rows = list(csv.DictReader(samples_file.read_text().splitlines()))
        if rows:
            peak = max(int(r["peak_working_set"]) for r in rows)
            idle = []
            for s in measured:
                before = [r for r in rows if int(r["qpc"]) <= s["qpc_start"]]
                if before:
                    idle.append(int(before[-1]["working_set"]))
            memory = {
                "helper_peak_working_set_mb": round(peak / 2**20),
                "helper_idle_working_set_mb": round(statistics.median(idle) / 2**20) if idle else None,
                "helper_pids": sorted({r["pid"] for r in rows}),
            }

    meta = json.loads((run / "run.json").read_text(encoding="utf-8-sig")) if (run / "run.json").exists() else {}
    done = any(e["event"] == "bench_done" for e in events)
    summary = {
        "run": run.name,
        "meta": meta,
        "completed": done,
        "timeout": any(e["event"] == "bench_timeout" for e in events),
        # The render error that ended the run.
        "failed": next((e["error"] for e in reversed(events) if e["event"] == "render_reply" and "error" in e), "unknown")
        if any(e["event"] == "bench_failed" for e in events) else None,
        "warmup_steps": warmup,
        "measured_steps": len(measured),
        "median_first_ms": median("first_ms"),
        "median_settled_ms": median("settled_ms"),
        "median_first_composited_ms": median("first_composited_ms"),
        "median_settled_composited_ms": median("settled_composited_ms"),
        "memory": memory,
        "steps": [{k: v for k, v in s.items() if k != "qpc_start"} for s in steps],
    }
    (run / "summary.json").write_text(json.dumps(summary, indent=1))
    print(json.dumps({k: v for k, v in summary.items() if k != "steps"}, indent=1))
    for s in steps:
        print(s["step"], s["film"], "first", None if s["first_ms"] is None else round(s["first_ms"]),
              "settled", None if s["settled_ms"] is None else round(s["settled_ms"]),
              "frames", s["frames"], "renders", s["renders"], "matches", s["matches"],
              "render ms", [None if x is None else round(x) for x in s["renders_ms"]], "cached", s["cached_pipeline"])
    if not done:
        sys.exit("the benchmark didn't finish")


if __name__ == "__main__":
    main()
