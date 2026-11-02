#!/usr/bin/env python3
"""Cross-platform golden test: same input, same output within a tight delta.

Renders a fixed matrix of inputs × stocks × presets with the f64 reference,
the f32 CPU engine and the GPU engine. Each machine keeps its raw output
dumps (`--raw-out`, the buffer the TIFF is quantised from); two machines are
compared value by value against the gates in gates.json.

    run.py run --label NAME --out manifests/NAME.json --dump-dir DIR
    run.py compare A.json B.json --dumps-a DIR --dumps-b DIR [--exact]
    run.py quality NAME.json --dumps DIR          # GPU vs f64 on one machine
    run.py probe --out-dir DIR                    # math, sampler, stage dumps
    run.py compare-probe DIR_A DIR_B

Every subcommand exits 0 only when everything passed. See README.md.
"""

import argparse
import concurrent.futures
import hashlib
import json
import os
import platform
import re
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
PRESETS = ROOT / "scripts" / "gpu_cpu_quality" / "params"
GATES = HERE / "gates.json"

# Float TIFF inputs are linear sRGB, like decoded RAWs in the app.
BASE_PARAMS = {"io": {"input_color_space": "sRGB", "input_cctf_decoding": False}}

PHOTOS = ["photo_7858115920", "photo_DSC_4985", "photo_DSC_4990", "photo_DSC_4997",
          "photo_DSC_5003", "photo_DSC_5009"]
SYNTH = ["synth_edges", "synth_gamut", "synth_noise", "synth_ramp"]
PRESET_INPUTS = ["photo_7858115920", "synth_noise"]
LAYOUT_INPUT = "photo_7858115920"
PROBE_INPUTS = ["synth_gamut", "synth_noise", "photo_DSC_4990"]

STOCKS = [
    ("kodak_portra_400", "kodak_portra_endura"),
    ("kodak_gold_200", "kodak_portra_endura"),
    ("fujifilm_pro_400h", "fujifilm_crystal_archive_typeii"),
    ("kodak_vision3_500t", "kodak_2383"),
    ("fujifilm_velvia_100", "scan"),
    ("kodak_doublex", "kodak_2302"),
    ("kodak_trix", "scan"),
]
PRESET_NAMES = ["det", "colour", "grain_on", "grain_off", "grain_on_coarse", "grain_off_coarse",
                "boost", "diff_bpm", "diff_cinebloom", "diff_enlarger", "diff_glimmer", "dir_wide",
                "halation_big", "lens_cam_large", "lens_cam_small", "lens_scan_large",
                "lens_scan_small", "morph", "preflash", "unsharp_iir", "unsharp_strong",
                # Grain off, so `quality` checks the GPU preflash against the reference
                # where the GPU folds print exposure and white correction (09a66ad).
                "preflash_exposure", "preflash_white"]
LAYOUTS = {
    "border_35mm": ["--border", "35mm"],
    "border_keyline": ["--border", "keyline", "--instagram"],
    "crop_rotate": ["--crop", "0.1,0.05,0.8,0.85,3.5"],
    "resize": ["--long-edge", "600"],
}

# name: (binary, SPEKTRAFILM_BACKEND, scalar type, gate profile)
ENGINES = {
    "f64": ("spektrafilm-f64", "cpu", "f64", "f64"),
    "f32cpu": ("spektrafilm", "cpu", "f32", "f32cpu"),
    "gpu": ("spektrafilm", "wgpu", "f32", "gpu"),
}
ALL_ENGINES = list(ENGINES)

# Pipeline stage dumps, in pipeline order (see SPEKTRAFILM_DUMP_* in spektrafilm-core).
STAGES = ["FILM_SENSITIVITY", "TC_LUT", "FILM_RAW", "FILM_PRE_LOG", "FILM_LOG_RAW",
          "FILM_PRE_GRAIN", "FILM_DENSITY", "PRINT_DENSITY"]

RENDER_TIMEOUT_S = 1800
ANSI = re.compile(r"\x1b\[[0-9;]*m")


def exe(name):
    return name + ".exe" if os.name == "nt" else name


def tool(bin_dir, name):
    path = (Path(bin_dir) / exe(name)).resolve()
    if not path.is_file():
        sys.exit(f"missing binary {path}; build it first (see README.md)")
    return path


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def tree_hash(root):
    """SHA-256 over the sorted relative paths and contents of every file."""
    h = hashlib.sha256()
    # Sort the strings: Windows paths compare case-insensitively.
    for rel in sorted(p.relative_to(root).as_posix() for p in Path(root).rglob("*") if p.is_file()):
        h.update(rel.encode() + b"\0" + sha256(Path(root) / rel).encode() + b"\n")
    return h.hexdigest()


def binary_arch(path):
    """CPU architecture of a Mach-O, PE or ELF executable."""
    with open(path, "rb") as f:
        head = f.read(4096)
    if head[:4] in (b"\xca\xfe\xba\xbe", b"\xbe\xba\xfe\xca"):
        return "universal"
    if head[:4] == b"\xcf\xfa\xed\xfe":
        cpu = struct.unpack_from("<I", head, 4)[0]
        return {0x01000007: "x86_64", 0x0100000C: "arm64"}.get(cpu, hex(cpu))
    if head[:2] == b"MZ":
        pe = struct.unpack_from("<I", head, 0x3C)[0]
        machine = struct.unpack_from("<H", head, pe + 4)[0]
        return {0x8664: "x86_64", 0xAA64: "arm64"}.get(machine, hex(machine))
    if head[:4] == b"\x7fELF":
        machine = struct.unpack_from("<H", head, 0x12)[0]
        return {0x3E: "x86_64", 0xB7: "arm64"}.get(machine, hex(machine))
    return "unknown"


def command_output(cmd, cwd=ROOT):
    try:
        return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, encoding="utf-8",
                              errors="replace", timeout=60).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return None


def cpu_model():
    if sys.platform == "darwin":
        return command_output(["sysctl", "-n", "machdep.cpu.brand_string"])
    if os.name == "nt":
        name = command_output(["powershell", "-NoProfile", "-Command",
                               "(Get-CimInstance Win32_Processor).Name"])
        return (name or os.environ.get("PROCESSOR_IDENTIFIER", "")).strip()
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                return line.split(":", 1)[1].strip()
    except OSError:
        pass
    return platform.processor()


def preset(name):
    if name == "default":
        return {}
    path = PRESETS / f"{name}.json"
    if not path.exists():
        path = PRESETS / "fx" / f"{name}.json"
    return json.loads(path.read_text())


def merge(a, b):
    out = dict(a)
    for k, v in b.items():
        out[k] = merge(out[k], v) if isinstance(v, dict) and isinstance(out.get(k), dict) else v
    return out


def effective_params(name):
    return merge(BASE_PARAMS, preset(name))


def grain_active(params):
    # RuntimeParams defaults grain to active.
    return params.get("film_render", {}).get("grain", {}).get("active", True)


def matrix():
    """Every case: name, input, film, paper, preset, layout, engines."""
    cases = {}

    def add(img, film, paper, pname, layout, engines):
        name = f"{img}__{film}__{paper}__{pname}" + (f"__{layout}" if layout else "")
        case = cases.setdefault(name, {"name": name, "input": img, "film": film, "paper": paper,
                                       "preset": pname, "layout": layout, "engines": []})
        case["engines"] = [e for e in ALL_ENGINES if e in case["engines"] or e in engines]

    for img in PHOTOS + SYNTH:
        for film, paper in STOCKS:
            add(img, film, paper, "default", None, ALL_ENGINES)
            # Grain off: the GPU can be checked against the f64 reference pixel by pixel.
            add(img, film, paper, "det", None, ["f64", "gpu"])
    for img in PRESET_INPUTS:
        for pname in PRESET_NAMES:
            add(img, "kodak_portra_400", "kodak_portra_endura", pname, None, ALL_ENGINES)
    for layout in LAYOUTS:
        add(LAYOUT_INPUT, "kodak_portra_400", "kodak_portra_endura", "default", layout, ALL_ENGINES)
    return list(cases.values())


def clean_env(backend, threads, extra=None):
    """Inherited engine, thread and wgpu settings would change what runs."""
    env = {k: v for k, v in os.environ.items()
           if not k.startswith(("SPEKTRAFILM_", "WGPU_")) and k not in ("RAYON_NUM_THREADS", "RUST_LOG")}
    env.update(SPEKTRAFILM_BACKEND=backend, NO_COLOR="1",
               RUST_LOG="warn,spektrafilm_gpu=info,spektrafilm_core::pipeline=info")
    if threads:
        env["RAYON_NUM_THREADS"] = str(threads)
    env.update(extra or {})
    return env


def process_cmd(bin_dir, engine, case, inputs, tiff, dump, params_file, data_dir):
    binary = tool(bin_dir, ENGINES[engine][0])
    cmd = [str(binary), "process", str(Path(inputs) / f"{case['input']}.tif"), "-o", str(tiff),
           "--film", case["film"], "--params", str(params_file), "--raw-out", str(dump),
           "--data-dir", str(data_dir)]
    cmd += ["--scan-film"] if case["paper"] == "scan" else ["--paper", case["paper"]]
    cmd += LAYOUTS[case["layout"]] if case["layout"] else []
    return cmd


def run_logged(cmd, env, log):
    try:
        proc = subprocess.run(cmd, env=env, capture_output=True, text=True, encoding="utf-8",
                              errors="replace", timeout=RENDER_TIMEOUT_S)
        out = ANSI.sub("", proc.stdout + proc.stderr)
        code = proc.returncode
    except subprocess.TimeoutExpired as e:
        out = ANSI.sub("", (e.stdout or b"").decode("utf-8", "replace") if isinstance(e.stdout, bytes)
                       else (e.stdout or "")) + f"\nTIMEOUT after {RENDER_TIMEOUT_S} s"
        code = -1
    if code != 0:
        log.parent.mkdir(parents=True, exist_ok=True)
        log.write_text(" ".join(cmd) + "\n\n" + out, encoding="utf-8")
    return code, out


def render(args, work, case, engine):
    name = case["name"]
    _, backend, scalar, _ = ENGINES[engine]
    dump_dir = Path(args.dump_dir)
    tiff = work / f"{name}.{engine}.tiff"
    dump = dump_dir / f"{name}.{engine}.f64"
    log = dump_dir / "logs" / f"{name}.{engine}.log"
    cmd = process_cmd(args.bin_dir, engine, case, args.inputs, tiff, dump,
                      work / f"params_{case['preset']}.json", args.data_dir)
    code, out = run_logged(cmd, clean_env(backend, args.threads), log)
    if code != 0:
        return name, engine, {"error": f"exit {code}, log {log}"}
    size = re.search(r"Saved: .*\((\d+)x(\d+)\)", out)
    if not size:
        log.write_text(out, encoding="utf-8")
        return name, engine, {"error": f"no 'Saved:' line, log {log}"}
    w, h = int(size.group(1)), int(size.group(2))
    result = {"f64": sha256(dump), "tiff": sha256(tiff), "size": [w, h]}
    if engine == "gpu":
        adapter = re.search(r'wgpu backend initialized adapter="?(.+?)"? backend=(\w+)', out)
        result["adapter"] = adapter.group(1) if adapter else None
        result["gpu_backend"] = adapter.group(2) if adapter else None
        result["resident"] = "gpu-resident fast path complete" in out
        if "using wgpu GPU backend" not in out or not result["resident"]:
            log.write_text(out, encoding="utf-8")
            return name, engine, {"error": f"did not run the resident GPU path, log {log}", **result}
    # The TIFF must be exactly the quantisation of its own dump.
    check = subprocess.run([str(tool(args.bin_dir, "golden_compare")), "--gates", str(GATES),
                            "--profile", "self", "--scalar", scalar, str(dump), str(dump), str(w),
                            str(h), "--tiff-a", str(tiff), "--tiff-b", str(tiff)],
                           capture_output=True, text=True, encoding="utf-8")
    tiff.unlink()
    if check.returncode != 0:
        return name, engine, {"error": f"TIFF self-check failed: {check.stdout or check.stderr}",
                              **result}
    return name, engine, result


def provenance(args, engines):
    rev = command_output(["git", "rev-parse", "HEAD"])
    status = command_output(["git", "status", "--porcelain", "--untracked-files=no"])
    return {
        "label": args.label,
        "platform": platform.platform(),
        "python": platform.python_version(),
        "cpu": cpu_model(),
        "cpu_count": os.cpu_count(),
        "threads": args.threads or "all",
        "git_rev": rev,
        "git_dirty_hash": hashlib.sha256((status or "").encode()).hexdigest() if status else None,
        "cargo_lock": sha256(ROOT / "Cargo.lock"),
        "rustc": command_output(["rustc", "-vV"]),
        "binaries": {e: {"sha256": sha256(tool(args.bin_dir, ENGINES[e][0])),
                         "arch": binary_arch(tool(args.bin_dir, ENGINES[e][0]))} for e in engines},
        "data_dir": tree_hash(args.data_dir),
        "presets_dir": tree_hash(PRESETS),
    }


def cmd_run(args):
    engines = args.engines.split(",")
    unknown = set(engines) - set(ENGINES)
    if unknown:
        sys.exit(f"unknown engines {sorted(unknown)}")
    cases = [c for c in matrix() if not args.filter or args.filter in c["name"]]
    jobs = [(c, e) for c in cases for e in c["engines"] if e in engines]
    if not jobs:
        sys.exit("no cases match")
    inputs = {}
    for name in sorted({c["input"] for c in cases}):
        path = Path(args.inputs) / f"{name}.tif"
        if not path.is_file():
            sys.exit(f"missing input {path}; make it with golden_inputs (see README.md)")
        inputs[name] = sha256(path)
    Path(args.dump_dir).mkdir(parents=True, exist_ok=True)
    (Path(args.dump_dir) / "logs").mkdir(exist_ok=True)
    presets = {p: effective_params(p) for p in sorted({c["preset"] for c in cases})}
    manifest = {"meta": provenance(args, engines), "inputs": inputs, "presets": presets,
                "cases": {}}
    failed = 0
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for pname, params in presets.items():
            (work / f"params_{pname}.json").write_text(json.dumps(params))
        done = 0
        with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
            for name, engine, result in pool.map(lambda j: render(args, work, *j), jobs):
                case = next(c for c in cases if c["name"] == name)
                entry = manifest["cases"].setdefault(name, {
                    k: case[k] for k in ("input", "film", "paper", "preset", "layout")})
                entry["grain"] = grain_active(presets[case["preset"]])
                entry.setdefault("engines", {})[engine] = result
                done += 1
                if "error" in result:
                    failed += 1
                    print(f"FAILED {name} {engine}: {result['error']}", file=sys.stderr)
                if done % 25 == 0 or done == len(jobs):
                    print(f"{done}/{len(jobs)}", file=sys.stderr)
    manifest["cases"] = dict(sorted(manifest["cases"].items()))
    Path(args.out).parent.mkdir(parents=True, exist_ok=True)
    Path(args.out).write_text(json.dumps(manifest, indent=1) + "\n")
    print(f"wrote {args.out}: {len(cases)} cases, {len(jobs)} renders, {failed} failed")
    sys.exit(1 if failed else 0)


def golden_compare(bin_dir, profile, scalar, a, b, w, h):
    proc = subprocess.run([str(tool(bin_dir, "golden_compare")), "--gates", str(GATES),
                           "--profile", profile, "--scalar", scalar, str(a), str(b), str(w), str(h)],
                          capture_output=True, text=True, encoding="utf-8")
    if proc.returncode == 2 or not proc.stdout.strip():
        return {"pass": False, "failures": [proc.stderr.strip()], "stats": {}}
    return json.loads(proc.stdout)


def summarise(stats):
    """Every scalar statistic, so a report can calibrate any gate."""
    return {k: v for k, v in stats.items() if isinstance(v, (int, float))}


def cmd_compare(args):
    a, b = (json.loads(Path(p).read_text()) for p in (args.a, args.b))
    problems = []
    if a["inputs"] != b["inputs"]:
        diff = sorted(k for k in set(a["inputs"]) | set(b["inputs"])
                      if a["inputs"].get(k) != b["inputs"].get(k))
        msg = f"inputs differ: {diff}"
        if not args.allow_input_mismatch:
            sys.exit(f"{msg} (copy the reference inputs, or pass --allow-input-mismatch)")
        problems.append(msg)
    for key in ("presets",):
        if a.get(key) != b.get(key):
            problems.append(f"{key} differ between the manifests")
    if a["meta"].get("data_dir") != b["meta"].get("data_dir"):
        problems.append("data directories differ")
    if not args.exact and not (args.dumps_a and args.dumps_b):
        sys.exit("tolerance comparison needs --dumps-a and --dumps-b (or pass --exact)")

    report = {"a": a["meta"], "b": b["meta"], "cases": {}}
    counts = {}
    pairs = []
    for name in sorted(set(a["cases"]) | set(b["cases"])):
        ea = a["cases"].get(name, {}).get("engines", {})
        eb = b["cases"].get(name, {}).get("engines", {})
        for engine in sorted(set(ea) | set(eb)):
            if args.engines and engine not in args.engines.split(","):
                continue
            pairs.append((name, engine, ea.get(engine), eb.get(engine)))
    if not pairs:
        sys.exit("nothing to compare")

    def judge(item):
        name, engine, ra, rb = item
        if ra is None or rb is None:
            return name, engine, "FAIL", {"failures": ["only in " + ("B" if ra is None else "A")]}
        if "error" in ra or "error" in rb:
            return name, engine, "FAIL", {"failures": [ra.get("error") or rb.get("error")]}
        if ra["size"] != rb["size"]:
            return name, engine, "FAIL", {"failures": [f"size {ra['size']} vs {rb['size']}"]}
        if ra["f64"] == rb["f64"] and ra["tiff"] == rb["tiff"]:
            return name, engine, "IDENTICAL", {}
        if args.exact:
            what = "raw" if ra["f64"] != rb["f64"] else "TIFF"
            return name, engine, "FAIL", {"failures": [f"{what} hash differs"]}
        _, _, scalar, profile = ENGINES[engine]
        case = a["cases"][name]
        border = engine == "f64" and (case.get("layout") or "").startswith("border")
        if engine == "gpu" and case.get("grain"):
            profile = "gpu_grain"
        fname = f"{name}.{engine}.f64"
        pa, pb = Path(args.dumps_a) / fname, Path(args.dumps_b) / fname
        stale = [str(p) for p, r in ((pa, ra), (pb, rb))
                 if not p.is_file() or sha256(p) != r["f64"]]
        if stale:
            return name, engine, "FAIL", {"failures": [f"dump missing or not the one in the "
                                                       f"manifest: {', '.join(stale)}"]}
        res = golden_compare(args.bin_dir, "f64_border" if border else profile, scalar, pa, pb,
                             *ra["size"])
        status = ("PASS-BORDER" if border else "PASS") if res["pass"] else "FAIL"
        if not res["pass"] and engine != "gpu" and case.get("grain"):
            # A last-bit change can flip a grain count; judge by the grain gate
            # instead, and say so.
            strict = res["failures"]
            res = golden_compare(args.bin_dir, "cpu_grain", scalar, pa, pb, *ra["size"])
            status = "PASS-GRAIN" if res["pass"] else "FAIL-GRAIN"
            res["failures"] = res["failures"] or [f"strict gate: {'; '.join(strict)}"]
        return name, engine, status, {"failures": res["failures"], **summarise(res["stats"])}

    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for name, engine, status, detail in pool.map(judge, pairs):
            counts.setdefault(engine, {}).setdefault(status, 0)
            counts[engine][status] += 1
            report["cases"].setdefault(name, {})[engine] = {"status": status, **detail}
            if status.startswith("FAIL"):
                print(f"{status} {name} {engine}: {'; '.join(map(str, detail.get('failures', [])))}")
    report["counts"] = counts
    report["problems"] = problems
    if args.report:
        Path(args.report).write_text(json.dumps(report, indent=1) + "\n")
    for p in problems:
        print(f"PROBLEM {p}")
    for engine, c in sorted(counts.items()):
        print(f"{engine:7} " + ", ".join(f"{k} {v}" for k, v in sorted(c.items())))
    worst = worst_stats(report)
    for engine, w in sorted(worst.items()):
        print(f"{engine:7} worst: " + ", ".join(f"{k} {v:.3g}" for k, v in w.items()))
    if any(s.startswith("FAIL-GRAIN") for c in counts.values() for s in c):
        print("FAIL-GRAIN: grain is active; run `probe` on both machines to separate pre-grain "
              "drift from a sampler stream divergence.")
    failed = problems or any(s.startswith("FAIL") for c in counts.values() for s in c)
    sys.exit(1 if failed else 0)


def worst_stats(report):
    worst = {}
    for engines in report["cases"].values():
        for engine, d in engines.items():
            for k in ("max_abs", "value_gate_worst", "rms_norm", "de00_max", "de00_p999",
                      "code16_max", "code8_max"):
                if isinstance(d.get(k), (int, float)):
                    w = worst.setdefault(engine, {})
                    w[k] = max(w.get(k, 0), d[k])
    return worst


def cmd_quality(args):
    """GPU vs the f64 reference on the same machine, for cases without grain."""
    m = json.loads(Path(args.manifest).read_text())
    rows = []
    for name, case in m["cases"].items():
        eng = case["engines"]
        if case["grain"] or "f64" not in eng or "gpu" not in eng:
            continue
        if "error" in eng["f64"] or "error" in eng["gpu"]:
            rows.append((name, {"pass": False, "failures": ["render error"], "stats": {}}))
            continue
        rows.append((name, None))

    def judge(row):
        name, res = row
        if res is not None:
            return name, res
        d = Path(args.dumps)
        size = m["cases"][name]["engines"]["f64"]["size"]
        return name, golden_compare(args.bin_dir, "gpu_vs_ref", "f64", d / f"{name}.f64.f64",
                                    d / f"{name}.gpu.f64", *size)

    failed = 0
    worst = {}
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        results = list(pool.map(judge, rows))
    for name, res in results:
        for k in ("de00_max", "de00_p99", "de00_p999", "code8_max", "code16_max"):
            v = res["stats"].get(k)
            if isinstance(v, (int, float)) and v >= worst.get(k, (-1, ""))[0]:
                worst[k] = (v, name)
        if not res["pass"]:
            failed += 1
            print(f"FAIL {name}: {'; '.join(res['failures'])}")
    if args.report:
        Path(args.report).write_text(json.dumps(dict(results), indent=1) + "\n")
    print(f"GPU vs f64: {len(results) - failed}/{len(results)} pass")
    for k, (v, name) in sorted(worst.items()):
        print(f"  worst {k} {v:.4g} ({name})")
    sys.exit(1 if failed or not results else 0)


def cmd_probe(args):
    out = Path(args.out_dir)
    (out / "stages").mkdir(parents=True, exist_ok=True)
    proc = subprocess.run([str(tool(args.bin_dir, "golden_probes")), str(out / "probes")],
                          capture_output=True, text=True, encoding="utf-8")
    if proc.returncode != 0:
        sys.exit(f"golden_probes failed: {proc.stderr}")
    files = {}
    failed = 0
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        for pname in ("default", "det"):
            (work / f"params_{pname}.json").write_text(json.dumps(effective_params(pname)))
        for img in PROBE_INPUTS:
            for pname in ("default", "det"):
                case = {"name": f"{img}__{pname}", "input": img, "film": "kodak_portra_400",
                        "paper": "kodak_portra_endura", "preset": pname, "layout": None}
                for engine in ("f64", "f32cpu"):
                    prefix = out / "stages" / f"{case['name']}.{engine}"
                    extra = {f"SPEKTRAFILM_DUMP_{s}": f"{prefix}.{s}.f64" for s in STAGES}
                    cmd = process_cmd(args.bin_dir, engine, case, args.inputs, work / "o.tiff",
                                      f"{prefix}.OUT.f64", work / f"params_{pname}.json",
                                      args.data_dir)
                    code, _ = run_logged(cmd, clean_env("cpu", 0, extra),
                                         out / "logs" / f"{case['name']}.{engine}.log")
                    if code != 0:
                        failed += 1
                        print(f"FAILED {case['name']} {engine}", file=sys.stderr)
    for p in sorted(q for q in out.rglob("*") if q.is_file() and q.suffix in (".f64", ".f32", ".u64")):
        files[p.relative_to(out).as_posix()] = sha256(p)
    meta = {"label": args.label, "platform": platform.platform(), "cpu": cpu_model(),
            "binaries": {e: binary_arch(tool(args.bin_dir, ENGINES[e][0])) for e in ("f64", "f32cpu")},
            "files": files}
    (out / "probe.json").write_text(json.dumps(meta, indent=1) + "\n")
    print(f"wrote {out / 'probe.json'}: {len(files)} files, {failed} failed renders")
    sys.exit(1 if failed else 0)


def element_diff(pa, pb):
    """(element count, differing count, first differing index) of two binary files."""
    size = 4 if pa.suffix == ".f32" else 8
    xa, xb = pa.read_bytes(), pb.read_bytes()
    if len(xa) != len(xb):
        return None
    n = len(xa) // size
    first, count = None, 0
    for i in range(n):
        if xa[i * size:(i + 1) * size] != xb[i * size:(i + 1) * size]:
            count += 1
            first = i if first is None else first
    return n, count, first


def cmd_compare_probe(args):
    da, db = Path(args.a), Path(args.b)
    fa = json.loads((da / "probe.json").read_text())["files"]
    fb = json.loads((db / "probe.json").read_text())["files"]
    differ = 0
    for name in sorted(k for k in set(fa) | set(fb) if k.startswith("probes/")):
        if fa.get(name) == fb.get(name):
            print(f"same     {name}")
            continue
        differ += 1
        if name not in fa or name not in fb:
            print(f"MISSING  {name}")
            continue
        n, count, first = element_diff(da / name, db / name) or (0, 0, None)
        print(f"DIFFERS  {name}: {count}/{n} elements, first at {first}")
    # Stage dumps: report each case's first differing stage and how far it is off.
    cases = sorted({k.split("/")[1].rsplit(".", 2)[0] for k in fa if k.startswith("stages/")})
    for case in cases:
        first = None
        for stage in STAGES + ["OUT"]:
            key = f"stages/{case}.{stage}.f64"
            if key not in fa and key not in fb:
                continue
            if fa.get(key) != fb.get(key):
                n = (da / key).stat().st_size // 8 if (da / key).exists() else 0
                res = golden_compare(args.bin_dir, "f64", "f64", da / key, db / key, n // 3, 1) \
                    if n % 3 == 0 else {"stats": {}}
                s = summarise(res.get("stats", {}))
                first = first or stage
                print(f"{case:40} {stage:16} differ {s.get('differ')} of {n}, max_abs "
                      f"{s.get('max_abs', 0):.3g}, ×f64-gate {s.get('value_gate_worst', 0):.3g}")
        if first is None:
            print(f"{case:40} identical at every stage")
        else:
            differ += 1
    sys.exit(1 if differ else 0)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)

    def common(p):
        p.add_argument("--bin-dir", default=str(ROOT / "target" / "release"))
        p.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2),
                       help="comparisons (or renders, for run) at once")

    r = sub.add_parser("run")
    common(r)
    r.set_defaults(jobs=1)
    r.add_argument("--inputs", default=str(ROOT / "target" / "golden" / "inputs"))
    r.add_argument("--data-dir", default=str(ROOT / "data"))
    r.add_argument("--out", required=True)
    r.add_argument("--dump-dir", required=True, help="raw f64 dumps, kept for compare")
    r.add_argument("--label", default="")
    r.add_argument("--engines", default=",".join(ALL_ENGINES))
    r.add_argument("--threads", type=int, default=0, help="RAYON_NUM_THREADS; 0 = all cores")
    r.add_argument("--filter", help="only cases whose name contains this")

    c = sub.add_parser("compare")
    common(c)
    c.add_argument("a")
    c.add_argument("b")
    c.add_argument("--dumps-a")
    c.add_argument("--dumps-b")
    c.add_argument("--exact", action="store_true", help="require identical hashes")
    c.add_argument("--allow-input-mismatch", action="store_true")
    c.add_argument("--engines", help="only compare these engines, e.g. gpu")
    c.add_argument("--report", help="write the full JSON report here")

    q = sub.add_parser("quality")
    common(q)
    q.add_argument("manifest")
    q.add_argument("--dumps", required=True)
    q.add_argument("--report")

    p = sub.add_parser("probe")
    common(p)
    p.add_argument("--inputs", default=str(ROOT / "target" / "golden" / "inputs"))
    p.add_argument("--data-dir", default=str(ROOT / "data"))
    p.add_argument("--out-dir", required=True)
    p.add_argument("--label", default="")

    cp = sub.add_parser("compare-probe")
    common(cp)
    cp.add_argument("a")
    cp.add_argument("b")

    args = ap.parse_args()
    {"run": cmd_run, "compare": cmd_compare, "quality": cmd_quality, "probe": cmd_probe,
     "compare-probe": cmd_compare_probe}[args.cmd](args)


if __name__ == "__main__":
    main()
