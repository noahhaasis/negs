"""Per-stock DIR-coupler presets for ported stocks whose curves fold Negs' coupler pre-correction.

Scales the class-default coupler gammas uniformly (ratios kept) until the shifted exposure axis keeps
a minimum slope of MARGIN (see fold_check.py), the same remedy Negs' shipped Velvia 100 preset applies.
Usage: uv run python coupler_presets.py <profile.json ...>  -> TOML block on stdout."""
import json, sys
from pathlib import Path
import numpy as np
from fold_check import curves, matrix, PRESETS

MARGIN = 0.1


def preset(path):
    p = json.loads(Path(path).read_text()); x, y = curves(p); M, pos = matrix(p)
    silver = (y.max(0) - y) if pos else y
    k = (np.diff(silver @ M, axis=0) / np.diff(x)[:, None]).max()   # steepest coupler shift per unit log E
    if 1 - k >= MARGIN:
        return None, 1.0
    s = (1 - MARGIN) / k
    return M * s, s


def toml_block(stock, M, s):
    f = lambda v: f"{v:.4f}"
    return (f"[{stock}]  # ported: class default x {s:.3f} so the DIR pre-correction does not fold\n"
            f"gamma_samelayer_rgb = [{f(M[0,0])}, {f(M[1,1])}, {f(M[2,2])}]\n"
            f"gamma_interlayer_r_to_gb = [{f(M[0,1])}, {f(M[0,2])}]\n"
            f"gamma_interlayer_g_to_rb = [{f(M[1,0])}, {f(M[1,2])}]\n"
            f"gamma_interlayer_b_to_rg = [{f(M[2,0])}, {f(M[2,1])}]\n")


if __name__ == "__main__":
    for path in sys.argv[1:]:
        stock = json.loads(Path(path).read_text())["info"]["stock"]
        if stock in PRESETS:
            continue
        M, s = preset(path)
        if M is not None:
            print(toml_block(stock, M, s))
