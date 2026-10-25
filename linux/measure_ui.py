#!/usr/bin/env python3
"""Repeat real Hyprland launches; inject a harmless key only into the verified Negs window.

No cold-cache claim: these are process launches with the current OS/driver caches.
"""
import argparse
import json
import os
from pathlib import Path
import statistics
import subprocess
import time

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--runs", type=int, default=5)
parser.add_argument("--benchmark", action="store_true")
parser.add_argument("--inspector-mode",choices=["both","lazy","eager"],default="both")
parser.add_argument("--size", help="Optional floating window size WIDTH,HEIGHT; default uses normal tiling")
parser.add_argument("--appearance", choices=["system", "light", "dark"], default="system")
parser.add_argument("--scene", choices=["lab", "post", "crop", "export"], default="lab")
parser.add_argument("--out", type=Path, default=root / "target/linux-validation/ui-performance")
args = parser.parse_args()
args.out.mkdir(parents=True, exist_ok=True)

def hypr(*cmd):
    return subprocess.check_output(["hyprctl", *cmd], text=True).strip()

monitors = json.loads(hypr("-j", "monitors"))
if any("LOCK" in m.get("solitaryBlockedBy", []) or not m.get("dpmsStatus", True) for m in monitors):
    raise SystemExit("Desktop locked/asleep: unlock it before measuring visible presentation or input.")

reports = []
variants = ["lazy", "eager"] if not args.benchmark else ["lazy"]
if args.inspector_mode != "both":
    variants = [args.inspector_mode]
for mode in variants:
    for run in range(args.runs):
        name = f"{mode}-{run:02}"
        output = args.out / f"{name}.json"
        env = dict(os.environ, NEGS_PROFILE_DIR=str(args.out / "profile"),
                   NEGS_STARTUP_REPORT=str(output), NEGS_APPEARANCE=args.appearance,
                   NEGS_INSPECTOR_MODE=mode, NEGS_UI_SCENE=args.scene,
                   NEGS_QUIT_AFTER_MS="7500" if args.benchmark else "2300")
        if args.benchmark:
            env["NEGS_UI_BENCHMARK"] = "1"
        if run == 0 and not args.benchmark:
            env["NEGS_WIDGET_CAPTURE"] = str(args.out / f"{mode}-widget.png")
        env["NEGS_LAUNCH_MONOTONIC_US"] = str(time.monotonic_ns() // 1000)
        log_path = args.out / f"{name}.log"
        with log_path.open("w") as log:
            cmd = [str(root / "target/linux-ui/release/negs")]
            # First explicit photo launch, then restore the same isolated UI session.
            if run == 0:
                cmd += ["--demo", "--fresh"]
            p = subprocess.Popen(cmd, env=env, stdout=log, stderr=log)
            before_input = None
            deadline = time.monotonic() + 8
            client = None
            while time.monotonic() < deadline and p.poll() is None:
                clients = json.loads(hypr("-j", "clients"))
                client = next((c for c in clients if c.get("pid") == p.pid), None)
                if client:
                    break
                time.sleep(0.01)
            if client:
                selector = f"pid:{p.pid}"
                hypr("dispatch", f'hl.dsp.focus({{ window = {json.dumps(selector)} }})')
                if json.loads(hypr("-j", "activewindow")).get("pid") == p.pid:
                    if args.size:
                        hypr("dispatch", f'hl.dsp.window.float({{ action = "set", window = {json.dumps(selector)} }})')
                        width, height = args.size.split(",")
                        hypr("dispatch", f'hl.dsp.window.resize({{ x = {int(width)}, y = {int(height)}, window = {json.dumps(selector)} }})')
                        hypr("dispatch", f'hl.dsp.window.center({{ window = {json.dumps(selector)} }})')
                    before_input = time.monotonic_ns() // 1000
                    subprocess.run(["wtype", "-k", "F8"], check=True, timeout=2)
            peak_rss_kib = 0
            maximum_threads = 0
            deadline = time.monotonic() + 12
            while p.poll() is None and time.monotonic() < deadline:
                try:
                    fields = dict(line.split(":", 1) for line in Path(f"/proc/{p.pid}/status").read_text().splitlines() if ":" in line)
                    peak_rss_kib = max(peak_rss_kib, int(fields.get("VmHWM", "0").split()[0]))
                    maximum_threads = max(maximum_threads, int(fields.get("Threads", "0")))
                except (FileNotFoundError, ProcessLookupError):
                    pass
                time.sleep(0.02)
            if p.poll() is None:
                p.terminate()
                p.wait(timeout=3)
                raise RuntimeError(f"{name}: launch did not close")
        if p.returncode or not output.exists():
            raise RuntimeError(f"{name}: process failed, see {log_path}")
        report = json.loads(output.read_text())
        report["run"] = name
        report["restored_session"] = run > 0
        report["peak_rss_kib"] = peak_rss_kib
        report["maximum_threads"] = maximum_threads
        report["monitor_evidence"] = monitors
        report["cache_state"] = "Existing OS/driver caches; fresh process. No engine cache exists."
        if before_input is not None:
            sent_ms = (before_input - int(env["NEGS_LAUNCH_MONOTONIC_US"])) / 1000
            report["input_sent_ms"] = sent_ms
            received = report["milestones_ms"].get("first_input_received_ms")
            report["input_delivery_ms"] = None if received is None else received - sent_ms
        report["warnings"] = log_path.read_text().count("WARNING")
        output.write_text(json.dumps(report, indent=2) + "\n")
        reports.append(report)
        print(name, report["milestones_ms"], flush=True)

summary = {"scope": "Native GTK UI only, real unlocked Hyprland session; no film rendering.", "runs": len(reports), "variants": {}}
for mode in variants:
    group = [r for r in reports if r["run"].startswith(mode)]
    metrics = {}
    keys = set().union(*(r["milestones_ms"] for r in group))
    for key in sorted(keys):
        values = [r["milestones_ms"][key] for r in group if key in r["milestones_ms"]]
        metrics[key] = {"median": statistics.median(values), "min": min(values), "max": max(values), "count": len(values)}
    delivery = [r["input_delivery_ms"] for r in group if r.get("input_delivery_ms") is not None]
    metrics["input_delivery_ms"] = {"median": statistics.median(delivery), "count": len(delivery)} if delivery else None
    summary["variants"][mode] = metrics
(args.out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(args.out / "summary.json")
