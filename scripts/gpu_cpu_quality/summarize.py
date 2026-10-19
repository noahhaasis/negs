"""Turn pair.sh result lines and grain.json files into Markdown tables and one JSON.

Usage: summarize.py RESULTS.txt RUNS_DIR OUT.json
RESULTS.txt holds the lines pair.sh prints ("NAME ENGINE {json}"), with NAME
"m_<mode>_<image>_<film>".
"""
import glob
import json
import os
import sys

import numpy as np


PROFILES = {os.path.basename(p)[:-5] for p in glob.glob(
    os.path.join(os.path.dirname(__file__), "..", "..", "data", "profiles", "*.json"))}


def split_case(rest):
    """`<image tag>_<film>` → (tag, film); the tag may contain underscores."""
    parts = rest.split("_")
    for i in range(1, len(parts)):
        if "_".join(parts[i:]) in PROFILES:
            return "_".join(parts[:i]), "_".join(parts[i:])
    return rest, ""


def pair_rows(path):
    rows = []
    for line in open(path):
        name, engine, payload = line.rstrip("\n").split(" ", 2)
        if engine == "FAILED":
            continue
        _, mode, rest = name.split("_", 2)
        image, film = split_case(rest)
        rows.append({"mode": mode, "image": image, "film": film, "engine": engine, **json.loads(payload)})
    return rows


def grain_rows(runs):
    rows = []
    for path in sorted(glob.glob(f"{runs}/grain_*/grain.json")):
        r = json.load(open(path))
        case = os.path.basename(os.path.dirname(path))[len("grain_"):]

        def worst_std_dev(k):
            # Skip channels with almost no grain (a*, b* of B&W stocks), where a
            # relative difference is meaningless.
            devs = [abs(x["std"][c] / a["std"][c] - 1)
                    for a, x in zip(r["cpuA"]["bins"], r[k]["bins"]) for c in range(3) if a["std"][c] > 0.2]
            return 100 * max(devs)

        def worst_mean_shift(k):
            return max(abs(x["mean"][0] - a["mean"][0]) for a, x in zip(r["cpuA"]["bins"], r[k]["bins"]))

        def worst_nps(k):
            # Skip the lowest band: it holds few samples and leaks image
            # content, so even two CPU runs differ there by several percent.
            ratio = np.array(r[k]["nps_L"][1:]) / np.array(r["cpuA"]["nps_L"][1:])
            return 100 * float(np.max(np.abs(ratio - 1)))

        rows.append({
            "case": case,
            "std_dev_pct": {"cpuB": worst_std_dev("cpuB"), "gpu": worst_std_dev("gpu")},
            "mean_L_shift": {"cpuB": worst_mean_shift("cpuB"), "gpu": worst_mean_shift("gpu")},
            "nps_dev_pct": {"cpuB": worst_nps("cpuB"), "gpu": worst_nps("gpu")},
            "lowpass_de_mean": {"cpuB": r["lowpass_de_cpuB_vs_cpuA"]["mean"], "gpu": r["lowpass_de_gpu_vs_cpuA"]["mean"]},
        })
    return rows


def main():
    results, runs, out = sys.argv[1:4]
    pairs, grains = pair_rows(results), grain_rows(runs)
    json.dump({"pairs": pairs, "grain": grains}, open(out, "w"), indent=1)
    for mode in ("colour", "det"):
        print(f"\n### {mode}\n")
        print("| Film | Photos | Mean ΔE00 | Worst p99 | Worst p99.9 | Worst max | Pixels > ΔE 1 (worst) | Max 8-bit step |")
        print("|---|---:|---:|---:|---:|---:|---:|---:|")
        films = sorted({p["film"] for p in pairs if p["mode"] == mode})
        for film in films:
            rs = [p for p in pairs if p["mode"] == mode and p["film"] == film]
            print(f"| {film} | {len(rs)} | {np.mean([p['de00_mean'] for p in rs]):.3f} | {max(p['de00_p99'] for p in rs):.3f} "
                  f"| {max(p['de00_p999'] for p in rs):.3f} | {max(p['de00_max'] for p in rs):.2f} "
                  f"| {100 * max(p['frac_de_gt_1'] for p in rs):.3f}% | {max(p['max_8bit_step'] for p in rs)} |")
    print("\n### grain\n")
    print("| Case | Grain σ dev: 2nd CPU run | GPU | Tone shift ΔL*: 2nd CPU | GPU | Spectrum dev: 2nd CPU | GPU | Low-pass ΔE: 2nd CPU | GPU |")
    print("|---|---:|---:|---:|---:|---:|---:|---:|---:|")
    for g in grains:
        print(f"| {g['case']} | {g['std_dev_pct']['cpuB']:.2f}% | {g['std_dev_pct']['gpu']:.2f}% "
              f"| {g['mean_L_shift']['cpuB']:.3f} | {g['mean_L_shift']['gpu']:.3f} "
              f"| {g['nps_dev_pct']['cpuB']:.1f}% | {g['nps_dev_pct']['gpu']:.1f}% "
              f"| {g['lowpass_de_mean']['cpuB']:.3f} | {g['lowpass_de_mean']['gpu']:.3f} |")


if __name__ == "__main__":
    main()
