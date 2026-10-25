#!/usr/bin/env python3
"""Prepare source-based B&W profiles, validation figures, and calibration report.

Uses checked-in source CSVs; no PDF or upstream checkout is required. Requires
numpy, scipy, colour-science, matplotlib, Pillow. Outputs stay provisional
unless --output-dir explicitly points at the production profile directory.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path

os.environ.setdefault("MPLCONFIGDIR", "/tmp/spektrafilm-bw-mpl")
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import numpy as np
import colour
from PIL import Image
from scipy.interpolate import PchipInterpolator
from scipy.optimize import brentq, least_squares
from scipy.special import ndtr

ROOT = Path(__file__).resolve().parents[2]
SOURCES = ROOT / "data/profile-sources/bw-expansion"
FIGURES = ROOT / "docs/bw-expansion-review"


def read_points(name):
    return np.loadtxt(SOURCES / f"{name}.csv", delimiter=",", skiprows=1)


def write_json(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2, allow_nan=False) + "\n")


def mitchell(x):
    """Mitchell-Netravali B=C=1/3, matching the existing spectral LUT sampler."""
    x = abs(x)
    if x < 1:
        return (7*x**3 - 12*x*x + 16/3)/6
    if x < 2:
        return (-7/3*x**3 + 12*x*x - 20*x + 32/3)/6
    return 0.0


def reference_gray_spectrum(data_dir):
    """Independently calculate the reference gray's Hanatos emitted spectrum.

    The spectrum is an RGB reconstruction, not a physical film measurement.
    Colour-science supplies the observer, D55 SPD, and CAT16 adaptation; the
    existing bundled irradiance table supplies the 81-sample spectrum.
    """
    shape = colour.SpectralShape(380, 780, 5)
    cmf = colour.MSDS_CMFS["CIE 1931 2 Degree Standard Observer"].copy().align(shape).values
    illu = colour.SDS_ILLUMINANTS["D55"].copy().align(shape).values
    white_xyz = np.sum(illu[:, None]*cmf, axis=0)
    white_xy = white_xyz[:2]/white_xyz.sum()
    xyz = colour.RGB_to_XYZ(np.full(3, 0.184), colourspace="sRGB",
                           illuminant=white_xy, chromatic_adaptation_transform="CAT16")
    xy = xyz[:2]/xyz.sum()
    tc = np.array([(1-xy[0])**2, xy[1]/(1-xy[0])])
    lut = np.load(data_dir / "luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy").astype(float)
    pos = tc*(lut.shape[0]-1)
    base = np.floor(pos).astype(int)
    value = np.zeros(81)
    total_weight = 0.0
    for i in range(-1, 3):
        for j in range(-1, 3):
            weight = mitchell(pos[0]-base[0]-i)*mitchell(pos[1]-base[1]-j)
            value += weight*lut[base[0]+i, base[1]+j]
            total_weight += weight
    return value/total_weight*xyz.sum()


def film_response(points, baseline, ceiling):
    x, y = points[:, 0], points[:, 1]
    p = PchipInterpolator(x, y)
    left_slope, right_slope = float(p.derivative()(x[0])), float(p.derivative()(x[-1]))
    if not (baseline < y[0] < y[-1] < ceiling and left_slope > 0 and right_slope > 0):
        raise ValueError("Invalid film continuation")

    def response(at):
        at = np.asarray(at, dtype=float)
        result = p(np.clip(at, x[0], x[-1]))
        low = baseline+(y[0]-baseline)*np.exp(np.minimum(0, at-x[0])*left_slope/(y[0]-baseline))
        high = ceiling-(ceiling-y[-1])*np.exp(-np.maximum(0, at-x[-1])*right_slope/(ceiling-y[-1]))
        return np.where(at < x[0], low, np.where(at > x[-1], high, result))
    return response


def fit_response(points, target_response, base, ceiling, kind):
    """Positive CDF mixture; layers are mathematical components, not emulsions.

    Give source observations full weight and the explicit continuation prior
    0.3 weight. Pin the asymptotic density to the recipe, then validate the
    final rescaled fit against the source (not just the optimizer's residual).
    """
    n = 4 if kind == "film" else 3
    x = np.linspace(-5, 7, 500) if kind == "film" else np.linspace(-.5, 5, 500)
    target = target_response(x)-base
    initial = np.r_[np.linspace(-2, 3, n) if kind == "film" else np.linspace(1.8, 2.7, n),
                    np.full(n, target.max()/n), np.full(n, 1 if kind == "film" else .3)]
    lower = np.r_[np.full(n, -6), np.full(n, .001), np.full(n, .035)]
    upper = np.r_[np.full(n, 9), np.full(n, 4), np.full(n, 4)]

    def evaluate(v, at):
        return np.sum(ndtr((np.asarray(at)[..., None]-v[:n])/v[2*n:])*v[n:2*n], axis=-1)

    def residual(v):
        return np.r_[.3*(evaluate(v, x)-target),
                     evaluate(v, points[:, 0])-(points[:, 1]-base),
                     10*(sum(v[n:2*n])-(ceiling-base))]

    result = least_squares(residual, initial, bounds=(lower, upper), max_nfev=4000,
                           ftol=1e-12, xtol=1e-12, gtol=1e-12)
    if not result.success:
        raise ValueError(f"{kind} fit did not converge: {result.message}")
    v = result.x
    v[n:2*n] *= (ceiling-base)/sum(v[n:2*n])
    order = np.argsort(v[:n])
    model = dict(model_type="norm_cdfs", centers=[v[:n][order].tolist()],
                 amplitudes=[v[n:2*n][order].tolist()], sigmas=[v[2*n:][order].tolist()])
    return model, lambda at: evaluate(v, at)+base


def make_profile(recipe, kind, log_exposure, densities, base, sensitivity, model):
    cfg = recipe[kind]
    info = dict(stock=cfg["id"], name=cfg["name"], type="negative",
                support="film" if kind == "film" else "paper",
                stage="filming" if kind == "film" else "printing", use="still",
                antihalation="strong" if kind == "film" else "no",
                target_print=recipe["paper"]["id"] if kind == "film" else None,
                channel_model="bw", densitometer="diffuse_visual",
                log_sensitivity_density_over_min=1.0 if kind == "film" else 0.0,
                reference_illuminant="D55", viewing_illuminant="D50")
    # Store full-precision model parameters; sampled fields come from this model.
    data = dict(wavelengths=list(range(380, 781, 5)),
                log_sensitivity=[[None if v == 0 else round(float(np.log10(v)), 10)] for v in sensitivity],
                channel_density=[[1.0] for _ in range(81)],
                base_density=[[round(float(base), 10)] for _ in range(81)],
                log_exposure=np.round(log_exposure, 10).tolist(),
                density_curves=np.round(densities[:, None], 10).tolist(),
                development_time=[cfg["development_time_minutes"]],
                density_curves_model=model)
    centers, amplitudes, sigmas = [np.array(model[k][0]) for k in ("centers", "amplitudes", "sigmas")]
    data["density_curves_layers"] = (ndtr((log_exposure[:, None]-centers)/sigmas)*amplitudes)[:, :, None].tolist()
    source = recipe["sources"]["kodak" if kind == "film" else "ilford"]
    return dict(metadata=dict(version="bw-expansion-1", created="2026-10-18",
                copyright="New preparation: spektrafilm-rs contributors (Codex-assisted), 2026. Manufacturer source rights retained.",
                license="New preparation and profile: CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/ . Manufacturer graph excerpts retain their original rights.",
                citation="Spektrafilm by Andrea Volpato and contributors; https://github.com/andreavolpato/spektrafilm . New calibration, not an upstream stock profile.",
                datasource=f"{source['document']}; {source['url']}; SHA256 {source['sha256']}. Reproduce with scripts/bw_expansion/prepare.py; source points, assumptions and conditions: data/profile-sources/bw-expansion/recipe.json. " + cfg["sensitivity_note"]),
                info=info, data=data)


def overlay(recipe, name, points, at, fitted, ylabel):
    spec = recipe["kodak_graphs"].get(name)
    fig, axes = plt.subplots(1, 2, figsize=(12, 5))
    im = Image.open(FIGURES / f"{name}-source.png")
    axes[0].imshow(im)
    if spec:
        x0, y0, _, _ = spec["crop_pdf_pt"]
        axes[0].scatter((points[:, 2]-x0)*3, (points[:, 3]-y0)*3,
                        s=6, facecolors="none", edgecolors="#e04b22", linewidths=.5)
    else:
        axes[0].scatter(points[:, 2], points[:, 3], s=15, facecolors="none", edgecolors="#e04b22", linewidths=.7)
    axes[0].axis("off")
    axes[0].set_title("Source graph + extracted points")
    axes[1].plot(at, fitted, color="#17666e", label="Prepared response")
    axes[1].scatter(points[:, 0], points[:, 1], s=8, color="#e04b22", label="Source points")
    axes[1].axvspan(points[0, 0], points[-1, 0], alpha=.05, color="#17666e")
    axes[1].set(xlabel="Wavelength (nm)" if "sensitivity" in name else "Source log exposure", ylabel=ylabel)
    axes[1].grid(alpha=.2)
    axes[1].legend(fontsize=8)
    fig.tight_layout()
    fig.savefig(FIGURES / f"{name}-validation.png", dpi=140)
    plt.close(fig)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output-dir", type=Path, default=ROOT / "target/bw-profiles")
    parser.add_argument("--data-dir", type=Path, default=ROOT / "data")
    args = parser.parse_args()
    recipe = json.loads((SOURCES / "recipe.json").read_text())
    film_points = read_points("trix400-d76-8min")
    sens_points = read_points("trix400-sensitivity")
    paper_points = read_points("ilford-mgrc-grade2")
    fcfg, pcfg = recipe["film"], recipe["paper"]
    total_film = film_response(film_points, fcfg["base_density"], fcfg["assumed_total_density_ceiling"])
    paper_pchip = PchipInterpolator(paper_points[:, 0], paper_points[:, 1])
    total_paper = lambda x: paper_pchip(np.clip(x, *paper_points[[0, -1], 0]))
    base_paper = float(paper_points[0, 1])
    film_model, total_film = fit_response(film_points, total_film, fcfg["base_density"], fcfg["assumed_total_density_ceiling"], "film")
    paper_model, total_paper = fit_response(paper_points, total_paper, base_paper, float(paper_points[-1, 1]), "paper")
    film_origin = brentq(lambda x: total_film(x)-fcfg["base_density"]-fcfg["midgray_net_density"], *film_points[[0, -1], 0])
    paper_origin = brentq(lambda x: total_paper(x)+np.log10(pcfg["midgray_reflectance"]), *paper_points[[0, -1], 0])
    for model, origin in [(film_model, film_origin), (paper_model, paper_origin)]:
        model["centers"][0] = [x-origin for x in model["centers"][0]]
    sample = recipe["sampling"]
    log_e = np.linspace(sample["log_exposure_min"], sample["log_exposure_max"], sample["count"])
    film_net = total_film(log_e+film_origin)-fcfg["base_density"]
    paper_net = total_paper(log_e+paper_origin)-base_paper
    wavelengths = np.arange(380, 781, 5)
    valid = wavelengths <= fcfg["sensitivity_last_nm"]
    film_sensitivity = np.zeros(81)
    film_sensitivity[valid] = 10**PchipInterpolator(sens_points[:, 0], sens_points[:, 1])(wavelengths[valid])
    gray_spectrum = reference_gray_spectrum(args.data_dir)
    raw_gray_before = float(gray_spectrum @ film_sensitivity)
    film_sensitivity /= raw_gray_before
    paper_sensitivity = (wavelengths <= 550).astype(float)
    profiles = {
        "film": make_profile(recipe, "film", log_e, film_net, fcfg["base_density"], film_sensitivity, film_model),
        "paper": make_profile(recipe, "paper", log_e, paper_net, base_paper, paper_sensitivity, paper_model),
    }
    report = dict(film_source_exposure_origin=film_origin, paper_source_exposure_origin=paper_origin,
                  film_sensitivity_log_shift=-np.log10(raw_gray_before),
                  film_midgray_raw=float(gray_spectrum @ film_sensitivity),
                  paper_base_reflection_density=base_paper,
                  paper_max_reflection_density=float(paper_points[-1, 1]),
                  paper_unexposed_reflectance=10**(-base_paper),
                  paper_maximum_density_reflectance=10**(-float(paper_points[-1, 1])),
                  source_points_sha256={p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(SOURCES.glob("*.csv"))},
                  spectral_lut_sha256=hashlib.sha256((args.data_dir / "luts/spectral_upsampling/hanatos2025_irradiance_xy_tc.npy").read_bytes()).hexdigest(),
                  profiles={})
    for kind, points, origin, base in [("film", film_points, film_origin, fcfg["base_density"]), ("paper", paper_points, paper_origin, base_paper)]:
        p = profiles[kind]
        curve = np.array(p["data"]["density_curves"])[:, 0]
        error = np.interp(points[:, 0]-origin, p["data"]["log_exposure"], curve)+base-points[:, 1]
        tolerance = fcfg["density_tolerance"] if kind == "film" else recipe["paper_graph"]["density_tolerance"]
        if np.max(np.abs(error)) > tolerance or not np.isfinite(curve).all() or np.min(curve) < -1e-9 or np.any(np.diff(curve) < -1e-9):
            raise ValueError(f"{kind} failed physical/source checks: max density error {max(abs(error))}")
        path = args.output_dir / (p["info"]["stock"] + ".json")
        write_json(path, p)
        report["profiles"][kind] = dict(id=p["info"]["stock"], max_source_density_error=float(max(abs(error))),
                                       minimum_sampled_net_density=float(curve.min()),
                                       tolerance=tolerance, sha256=hashlib.sha256(path.read_bytes()).hexdigest())
    x = np.linspace(film_points[0, 0]-1, film_points[-1, 0]+2, 700)
    overlay(recipe, "trix400-d76-8min", film_points, x, total_film(x), "Total diffuse density")
    x = np.linspace(sens_points[0, 0], sens_points[-1, 0], 700)
    overlay(recipe, "trix400-sensitivity", sens_points, x, PchipInterpolator(sens_points[:, 0], sens_points[:, 1])(x), "Source log sensitivity (before common shift)")
    x = np.linspace(-.1, 4.3, 700)
    overlay(recipe, "ilford-mgrc-grade2", paper_points, x, total_paper(x), "Total reflection density")
    write_json(FIGURES / "preparation-report.json", report)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
