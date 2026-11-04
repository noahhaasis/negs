"""Neutral balance for slides viewed directly: transmitted light at mid-grey should keep the viewing light's colour."""
import numpy as np, colour

WL = np.arange(380, 781, 5)
SH = colour.SpectralShape(380, 780, 5)
CMF = colour.MSDS_CMFS["CIE 1931 2 Degree Standard Observer"].copy().align(SH).values


def illum(name="D50"):
    return colour.SDS_ILLUMINANTS[name].copy().align(SH).values


def xy_of(densities, dyes, base, ill):
    """densities: (3,) net CMY; dyes: (81,3) channel_density; base: (81,)."""
    D = np.nan_to_num(base) + np.nan_to_num(dyes) @ densities
    XYZ = (ill * 10 ** -D) @ CMF
    return XYZ[:2] / XYZ.sum(), XYZ[1] / (ill @ CMF)[1]


def solve(dyes, base, green=0.5, ill_name="D50"):
    """Net densities (r, green, b) whose transmitted light has the illuminant's chromaticity."""
    from scipy.optimize import least_squares
    ill = illum(ill_name); target = (ill @ CMF)[:2] / (ill @ CMF).sum()
    f = lambda p: (xy_of(np.array([p[0], green, p[1]]), dyes, base, ill)[0] - target) * 1e3
    r = least_squares(f, [green, green])
    return np.array([r.x[0], green, r.x[1]]), float(np.abs(r.fun).max() / 1e3)


def solve_neutral_grey(dyes, base, Y=0.18, ill_name="D50"):
    """Net densities whose transmitted light has the illuminant's chromaticity and luminance factor Y.

    This is how Negs' shipped slides sit at mid-grey (Provia 100F, Velvia 100: neutral, Y = 0.18)."""
    from scipy.optimize import least_squares
    ill = illum(ill_name); target = (ill @ CMF)[:2] / (ill @ CMF).sum()

    def f(p):
        xy, y = xy_of(p, dyes, base, ill)
        return np.r_[(xy - target) * 1e3, (np.log10(y) - np.log10(Y)) * 1e2]
    r = least_squares(f, [0.5, 0.5, 0.5])
    return r.x, float(np.abs(r.fun).max())
