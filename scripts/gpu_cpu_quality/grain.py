"""Statistical comparison of grain between engines.

Grain is random, so pixels can't be compared one to one. For each engine the
grain residual is (grain on) - (grain off) in CIELAB, same input, same engine.
Two CPU realizations (A: full frame, B: input shifted by one pixel, so the RNG
stream lands on different pixels) give the noise floor: how much two runs of
the *reference* differ from each other. The GPU passes if it is no further
from CPU A than CPU B is.

Usage: grain.py DIR W H OUT.json
DIR holds cpuA_ng, cpuA_g, cpuB_ng, cpuB_g, gpu_ng, gpu_g (.f64 raw-out dumps).
cpuB_* are (W-1)x(H-1); cpuB(x, y) corresponds to cpuA(x+1, y+1).
"""
import json
import sys

import colour
import numpy as np
from scipy.ndimage import gaussian_filter

D65 = colour.CCS_ILLUMINANTS["CIE 1931 2 Degree Standard Observer"]["D65"]
SRGB = colour.RGB_COLOURSPACES["sRGB"]
L_BINS = np.arange(0, 101, 10)
TILE = 256


def load(path, w, h):
    return np.memmap(path, dtype="<f8", mode="r", shape=(h, w, 3))


def linear(encoded):
    return colour.models.eotf_sRGB(np.clip(encoded, 0.0, 1.0))


def lab(encoded, rows=512):
    out = np.empty(encoded.shape, dtype=np.float32)
    for y in range(0, encoded.shape[0], rows):
        chunk = np.asarray(encoded[y:y + rows], dtype=np.float64)
        out[y:y + rows] = colour.XYZ_to_Lab(colour.RGB_to_XYZ(linear(chunk), SRGB), D65)
    return out


def nps(field):
    """Radially averaged noise power spectrum (Welch over Hann-windowed tiles)."""
    h, w = field.shape
    win = np.outer(np.hanning(TILE), np.hanning(TILE)).astype(np.float32)
    acc = np.zeros((TILE, TILE))
    n = 0
    for y in range(0, h - TILE + 1, TILE):
        for x in range(0, w - TILE + 1, TILE):
            t = field[y:y + TILE, x:x + TILE]
            t = (t - t.mean()) * win
            acc += np.abs(np.fft.fft2(t)) ** 2
            n += 1
    acc /= n * (win ** 2).sum()
    f = np.fft.fftfreq(TILE)
    r = np.hypot(*np.meshgrid(f, f))
    edges = np.linspace(0, 0.5, 33)
    idx = np.digitize(r.ravel(), edges) - 1
    radial = [float(acc.ravel()[idx == i].mean()) for i in range(32)]
    return radial


def stats(ng, g):
    l_ng = lab(ng)
    resid = lab(g)
    resid -= l_ng
    out = {"bins": []}
    level = l_ng[..., 0]
    for lo, hi in zip(L_BINS[:-1], L_BINS[1:]):
        m = (level >= lo) & (level < hi)
        n = int(m.sum())
        if n < 20000:
            continue
        r = resid[m]
        dl = r[:, 0]
        z = (dl - dl.mean()) / (dl.std() + 1e-12)
        out["bins"].append({
            "L": [int(lo), int(hi)], "n": n,
            "std": [float(v) for v in r.std(axis=0)],
            "mean": [float(v) for v in r.mean(axis=0)],
            "skew_L": float((z ** 3).mean()),
            "kurt_L": float((z ** 4).mean() - 3.0),
        })
    flat = resid.reshape(-1, 3)
    c = np.corrcoef(flat.T)
    out["corr"] = {"L_a": float(c[0, 1]), "L_b": float(c[0, 2]), "a_b": float(c[1, 2])}
    out["std_all"] = [float(v) for v in flat.std(axis=0)]
    out["nps_L"] = nps(resid[..., 0])
    return out


def lowpass_de(a, b, sigma=4.0):
    """ΔE00 between two grain renders after averaging the grain away."""
    lin = lambda x: gaussian_filter(linear(np.asarray(x, dtype=np.float32)), (sigma, sigma, 0))
    la, lb = lin(a), lin(b)
    to = lambda x: colour.XYZ_to_Lab(colour.RGB_to_XYZ(x.astype(np.float64), SRGB), D65)
    de = np.concatenate([colour.delta_E(to(la[y:y + 512]), to(lb[y:y + 512]), method="CIE 2000").ravel()
                         for y in range(0, la.shape[0], 512)])
    return {"mean": float(de.mean()), "p99": float(np.percentile(de, 99)), "max": float(de.max())}


def main():
    d, w, h, out_path = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
    # Common region: cpuA[1:, 1:] lines up with cpuB.
    crop = lambda a: a[1:, 1:]
    a_ng, a_g = crop(load(f"{d}/cpuA_ng.f64", w, h)), crop(load(f"{d}/cpuA_g.f64", w, h))
    b_ng, b_g = load(f"{d}/cpuB_ng.f64", w - 1, h - 1), load(f"{d}/cpuB_g.f64", w - 1, h - 1)
    g_ng, g_g = crop(load(f"{d}/gpu_ng.f64", w, h)), crop(load(f"{d}/gpu_g.f64", w, h))
    result = {
        "cpuA": stats(a_ng, a_g),
        "cpuB": stats(b_ng, b_g),
        "gpu": stats(g_ng, g_g),
        "lowpass_de_cpuB_vs_cpuA": lowpass_de(b_g, a_g),
        "lowpass_de_gpu_vs_cpuA": lowpass_de(g_g, a_g),
        "lowpass_de_nograin_gpu_vs_cpuA": lowpass_de(g_ng, a_ng),
    }
    with open(out_path, "w") as f:
        json.dump(result, f)
    print(out_path)


if __name__ == "__main__":
    main()
