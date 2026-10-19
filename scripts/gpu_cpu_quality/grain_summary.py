import json, sys
import numpy as np
r = json.load(open(sys.argv[1]))
print("granularity σ(ΔL*) by tone (CPU A | CPU B | GPU)  and σ(Δa*), σ(Δb*)")
for a, b, g in zip(r["cpuA"]["bins"], r["cpuB"]["bins"], r["gpu"]["bins"]):
    print(f" L*{a['L']}: L {a['std'][0]:.3f} | {b['std'][0]:.3f} | {g['std'][0]:.3f}   a {a['std'][1]:.3f}|{b['std'][1]:.3f}|{g['std'][1]:.3f}   b {a['std'][2]:.3f}|{b['std'][2]:.3f}|{g['std'][2]:.3f}   meanL {a['mean'][0]:+.3f}|{b['mean'][0]:+.3f}|{g['mean'][0]:+.3f}  skew {a['skew_L']:+.2f}|{b['skew_L']:+.2f}|{g['skew_L']:+.2f}")
for k in ("cpuA", "cpuB", "gpu"):
    print(k, "corr", {kk: round(v, 3) for kk, v in r[k]["corr"].items()}, "std_all", [round(v, 3) for v in r[k]["std_all"]])
na, nb, ng = (np.array(r[k]["nps_L"]) for k in ("cpuA", "cpuB", "gpu"))
print("NPS ratio B/A per freq band:", np.round(nb / na, 2)[::4])
print("NPS ratio GPU/A per freq band:", np.round(ng / na, 2)[::4])
for k in ("lowpass_de_cpuB_vs_cpuA", "lowpass_de_gpu_vs_cpuA", "lowpass_de_nograin_gpu_vs_cpuA"):
    print(k, {kk: round(v, 3) for kk, v in r[k].items()})
