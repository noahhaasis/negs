"""Attribute output differences from upstream experimental to port items.

Renders a fixture with Python spektrafilm experimental (pinned 28bf883) at
its defaults, except that grain and glare are off and input/output are linear
sRGB (the same settings as `check_parity.py --experimental-defaults`). Then
it reverts one ticket 16 item at a time to what Negs did before the port
(upstream main 3bb2c2d behaviour plus Negs' own data) and reports how far
each revert moves the output. Rows don't add up; the items interact.

With --rust-binary it also renders the f64 CLI on the same input and
compares it with every variant. A variant that is closer to Rust than
experimental itself names an item that Rust doesn't match yet; the
`all_negs` variant reproduced the pre-port engine to 0 8-bit steps.

The reverts patch private 28bf883 functions, so another upstream commit may
need changes here. Imports come only from --python-source; Python caches go
into the output directory. See README.md in this directory.
"""

import argparse
import contextlib
import dataclasses
import json
import os
from pathlib import Path
import subprocess
import sys

sys.dont_write_bytecode = True  # keep this checkout free of caches too
sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_parity import (  # noqa: E402
    CASES, EXPERIMENTAL, ROOT, apply_params, build_cli, make_fixtures,
    python_overrides, reference_api, shared_params,
)


# Pre-port Negs DIR gammas (`pipeline.rs::apply_film_specific_params`):
# by polarity, then the stock overrides from ticket 14.
NEGS_COUPLERS = {
    "negative": {"gamma_samelayer_rgb": (0.336, 0.319, 0.273), "gamma_interlayer_r_to_gb": (0.353, 0.302),
                 "gamma_interlayer_g_to_rb": (0.154, 0.353), "gamma_interlayer_b_to_rg": (0.168, 0.226)},
    "positive": {"gamma_samelayer_rgb": (0.12, 0.08, 0.06), "gamma_interlayer_r_to_gb": (0.12, 0.06),
                 "gamma_interlayer_g_to_rb": (0.08, 0.06), "gamma_interlayer_b_to_rg": (0.06, 0.06)},
    "fujifilm_provia_100f": {"gamma_samelayer_rgb": (0.156, 0.104, 0.078), "gamma_interlayer_r_to_gb": (0.156, 0.078),
                             "gamma_interlayer_g_to_rb": (0.104, 0.078), "gamma_interlayer_b_to_rg": (0.078, 0.078)},
    "fujifilm_velvia_100": {"gamma_samelayer_rgb": (0.108, 0.072, 0.054), "gamma_interlayer_r_to_gb": (0.108, 0.054),
                            "gamma_interlayer_g_to_rb": (0.072, 0.054), "gamma_interlayer_b_to_rg": (0.054, 0.054)},
}
# Pre-port Negs defaults for the remaining items.
NEGS_HALATION = {"halation_strength": (0.05, 0.015, 0.0), "halation_first_sigma_um": (65.0, 65.0, 65.0)}
NEGS_NEUTRAL_FALLBACK = (0.0, 65.0, 55.0)  # schema defaults when the database has no entry
NEGS_INPUT_GAMUT = {"boundary": "locus", "knee": (0.0, 1.0, 6.0)}
NEGS_OUTPUT_GAMUT = {"algorithm": "cam16ucs", "knee": (0.0, 1.0, 6.0), "lightness_compression": (0.7, 1.0, 2.2)}
NEGS_ILLUMINANTS = ("D50", "D55", "D65")  # anything else fell back to D55 (reference) / D50 (viewing)

ITEMS = {
    "1_film_stored_curves": "film curves: stored instead of the fitted model",
    "9_profiles_negs": "profiles: Negs data (main's) instead of experimental's",
    "2_couplers_negs_gammas": "DIR gammas: Negs' hard-coded values instead of couplers.toml",
    "2_couplers_linear": "DIR: Langmuir saturation off",
    "2_couplers_tail_006": "DIR: diffusion tail weight 0.06 instead of 0.03",
    "4_halation_no_preset": "halation: schema defaults instead of the use/antihalation preset",
    "5_wb_main": "white balance: window normalised on the bare illuminant",
    "5_npf_negs": "neutral print filters: Negs' database and fallback",
    "6_gamut_main": "gamut: main's defaults (locus, CAM16-UCS, reference centre)",
    "8_autoexposure_rgb": "auto-exposure: metered on input RGB, no midgray scale",
    "0_illuminant_fallback": "illuminants T/K75P: D55/D50 fallback (pre-16.2 bug)",
}
AGGREGATES = {
    "all_negs": [item for item in ITEMS if item != "0_illuminant_fallback"],
    "all_negs_illuminant_fallback": list(ITEMS),
}


def load_negs_profile(negs_data, stock, profile_from_dict):
    path = negs_data / "profiles" / f"{stock}.json"
    if not path.is_file():
        return None
    data = json.loads(path.read_text())
    model = data["data"].get("density_curves_model")
    if model and model.get("model_type") == "cdfs":
        model["model_type"] = "norm_cdfs"  # main's name for the same model
    return profile_from_dict(data)


@contextlib.contextmanager
def patched(modules, items):
    """Install the function-level reverts for `items`; restore on exit."""
    su, pl, filming = modules["su"], modules["pl"], modules["filming"]
    saved = [(su, "compute_hanatos2025_tc_lut"), (pl, "refresh_density_curves_from_model"),
             (pl, "select_development_time"), (pl, "apply_print_curves_morph_with_layers"),
             (filming.FilmingStage, "_auto_exposure"), (filming.FilmingStage, "_midgray_autoexposure_scale")]
    originals = [(owner, name, getattr(owner, name)) for owner, name in saved]
    try:
        if "5_wb_main" in items or "6_gamut_main" in items:
            su.compute_hanatos2025_tc_lut = main_tc_lut(
                modules, main_wb="5_wb_main" in items, reference_center="6_gamut_main" in items)
        if "1_film_stored_curves" in items:
            install_stored_film_curves(pl)
        if "8_autoexposure_rgb" in items:
            install_rgb_autoexposure(modules)
        yield
    finally:
        for owner, name, original in originals:
            setattr(owner, name, original)


def main_tc_lut(modules, *, main_wb, reference_center):
    """28bf883's `compute_hanatos2025_tc_lut` with main's window normalisation
    on the bare illuminant and/or main's gamut centre on the reference
    illuminant. Only the paths that Negs' defaults use are implemented."""
    su, standard_illuminant = modules["su"], modules["standard_illuminant"]
    np = modules["np"]

    def compute(sensitivity, adaptation, gamut_compress=None):
        spectra_lut = su.HANATOS2025_SPECTRA_LUT
        if adaptation.spectral_gaussian_blur > 0 or (
                adaptation.apply_surface and adaptation.surface_params is not None):
            raise NotImplementedError("spectral blur and adaptation surface aren't reverted")
        if adaptation.apply_window and adaptation.window_params is not None:
            window = su.eval_spectral_bandpass_window(adaptation.window_params)
            if main_wb:
                neutral = standard_illuminant(adaptation.reference_illuminant)
            else:
                white_tc = su._tri2quad(su._illuminant_to_xy(adaptation.reference_illuminant))
                neutral = su.apply_lut_cubic_2d(spectra_lut, np.asarray(white_tc).reshape(1, 1, 2))[0, 0]
            normalization = (np.sum(sensitivity * neutral[:, None] * window, axis=0)
                             / np.sum(sensitivity * neutral[:, None], axis=0))
            raw_lut = su.contract("ijl,lm->ijm", spectra_lut, sensitivity * (window / normalization))
        else:
            raw_lut = su.contract("ijl,lm->ijm", spectra_lut, sensitivity)
        if gamut_compress is not None and gamut_compress.active:
            from spektrafilm.utils.gamut_compression import remap_tc_lut_for_compression
            center = adaptation.reference_illuminant if reference_center else "D65"
            raw_lut = remap_tc_lut_for_compression(raw_lut, su._illuminant_to_xy(center), gamut_compress)
        return raw_lut
    return compute


def install_stored_film_curves(pl):
    """Keep the film profile's stored curves (after the development-time
    selection) instead of the model-derived, chemistry-morphed ones. Profiles
    are marked with `_negs_keep_stored`, which survives the pipeline's copy."""
    refresh, select = pl.refresh_density_curves_from_model, pl.select_development_time
    stored = {}

    def refresh_except_film(profile_data, profile_type="negative"):
        if getattr(profile_data, "_negs_keep_stored", False):
            return profile_data
        return refresh(profile_data, profile_type)

    def select_and_keep(profile_data, development_time):
        result = select(profile_data, development_time)
        if getattr(profile_data, "_negs_keep_stored", False):
            stored["curves"] = profile_data.density_curves.copy()
        return result

    def morph_to_stored(log_exposure, model, chemistry, profile_type="negative"):
        # Only the pipeline's film bake calls this; the print morph lives in
        # the printing stage's own import.
        return stored["curves"], None

    pl.refresh_density_curves_from_model = refresh_except_film
    pl.select_development_time = select_and_keep
    pl.apply_print_curves_morph_with_layers = morph_to_stored


def install_rgb_autoexposure(modules):
    """Main's auto-exposure: meter the input RGB preview, not film raw, and
    leave the print-balance midgray unscaled."""
    stage, measure = modules["filming"].FilmingStage, modules["measure_autoexposure_ev"]

    def auto_exposure(self, raw):
        if not self._camera.auto_exposure:
            return raw
        ev = measure(self._resize_service.full_frame_preview, self._io.input_color_space,
                     self._io.input_cctf_decoding, method=self._camera.auto_exposure_method)
        return raw * 2 ** ev

    stage._auto_exposure = auto_exposure
    stage._midgray_autoexposure_scale = lambda self, raw_midgray: 1.0


def render(modules, film, paper, scan_film, items, image, negs_data, neutral_filters):
    np, pl = modules["np"], modules["pl"]
    shared = shared_params(False, defaults=True)
    shared["io"]["scan_film"] = scan_film
    with patched(modules, items):
        params = apply_params(modules["init_params"](film, paper), python_overrides(shared, EXPERIMENTAL))
        if "9_profiles_negs" in items:
            for attr, stock in [("film", film), ("print", paper)]:
                profile = load_negs_profile(negs_data, stock, modules["profile_from_dict"])
                if profile is not None:
                    setattr(params, attr, profile)
        if "0_illuminant_fallback" in items:
            if params.film.info.reference_illuminant not in NEGS_ILLUMINANTS:
                params.film = params.film.update_info(reference_illuminant="D55")
            for attr in ["film", "print"]:
                profile = getattr(params, attr)
                if profile.info.viewing_illuminant not in NEGS_ILLUMINANTS:
                    setattr(params, attr, profile.update_info(viewing_illuminant="D50"))
        if "6_gamut_main" in items:
            params.io.input_gamut_compress = dataclasses.replace(params.io.input_gamut_compress, **NEGS_INPUT_GAMUT)
            params.io.output_gamut_compress = dataclasses.replace(params.io.output_gamut_compress, **NEGS_OUTPUT_GAMUT)
        params = modules["digest_params"](params)
        couplers = params.film_render.dir_couplers
        if "2_couplers_negs_gammas" in items:
            polarity = "positive" if params.film.is_positive else "negative"
            for key, value in {**NEGS_COUPLERS[polarity], **NEGS_COUPLERS.get(film, {})}.items():
                setattr(couplers, key, value)
        if "2_couplers_linear" in items:
            couplers.langmuir_donor_k_rgb = (np.inf,) * 3
            couplers.langmuir_receiver_k_rgb = (np.inf,) * 3
        if "2_couplers_tail_006" in items:
            couplers.diffusion_tail_weight = 0.06
        if "4_halation_no_preset" in items:
            for key, value in NEGS_HALATION.items():
                setattr(params.film_render.halation, key, value)
        if "5_npf_negs" in items and not scan_film:
            entry = (neutral_filters.get(params.print.info.stock, {}).get(params.enlarger.illuminant, {})
                     .get(params.film.info.stock))
            c, m, y = NEGS_NEUTRAL_FALLBACK if entry is None else (float(v) for v in entry)
            params.enlarger.c_filter_neutral, params.enlarger.m_filter_neutral, params.enlarger.y_filter_neutral = c, m, y
        if "1_film_stored_curves" in items:
            params.film.data._negs_keep_stored = True
        pipeline = pl.SimulationPipeline(params)
        return np.asarray(pipeline.process(image.astype(np.float64)), dtype=np.float64)


def render_rust(binary, rust_data, film, paper, scan_film, image, directory, oiio, np):
    directory.mkdir(parents=True, exist_ok=True)
    h, w, _ = image.shape
    buffer = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, "float"))
    buffer.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), image)
    if not buffer.write(str(directory / "input.tif")):
        raise RuntimeError(buffer.geterror())
    shared = shared_params(False, defaults=True)
    shared["io"]["scan_film"] = scan_film
    (directory / "params.json").write_text(json.dumps(shared, indent=2) + "\n")
    with (directory / "rust.log").open("w") as log:
        subprocess.run(
            [str(binary), "process", str(directory / "input.tif"), "--data-dir", str(rust_data),
             "--output", str(directory / "rust.png"), "--film", film, "--paper", paper,
             "--params", str(directory / "params.json"), "--raw-out", str(directory / "rust.f64"),
             *(["--scan-film"] if scan_film else [])],
            env={**os.environ, "SPEKTRAFILM_BACKEND": "cpu"}, cwd=ROOT,
            stdout=log, stderr=subprocess.STDOUT, check=True,
        )
    return np.fromfile(directory / "rust.f64", dtype="=f8").reshape(h, w, 3)


def stats(actual, reference, colour, np):
    """Linear max difference, sRGB-encoded 8-bit steps and ΔE00 on linear
    output (negative values clipped to 0)."""
    def encode(v):
        return colour.cctf_encoding(np.clip(v, 0, 1), function="sRGB") * 255

    def lab(v):
        return colour.XYZ_to_Lab(colour.RGB_to_XYZ(np.clip(v, 0, None), "sRGB"))
    steps = np.abs(encode(actual) - encode(reference))
    delta_e = colour.delta_E(lab(actual), lab(reference), method="CIE 2000")
    return {"max_linear": float(np.abs(actual - reference).max()),
            "max_8bit": float(steps.max()), "p99_8bit": float(np.percentile(steps, 99)),
            "mean_8bit": float(steps.mean()), "mean_dE00": float(delta_e.mean()),
            "p99_dE00": float(np.percentile(delta_e, 99)), "max_dE00": float(delta_e.max())}


def format_stats(s):
    return (f"ΔE00 mean {s['mean_dE00']:5.2f} p99 {s['p99_dE00']:5.2f} max {s['max_dE00']:6.2f} | "
            f"8-bit max {s['max_8bit']:5.1f} p99 {s['p99_8bit']:5.1f} | linear max {s['max_linear']:.2e}")


def parse_args(argv=None):
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--python-source", type=Path, required=True,
                        help="upstream experimental checkout (with src/spektrafilm)")
    parser.add_argument("--output-dir", type=Path, required=True, help="new directory for all artifacts")
    parser.add_argument("--case", action="append", choices=[*CASES, "all"],
                        help="named route from check_parity.py (repeatable; default: all)")
    parser.add_argument("--fixture", default="scene", choices=["midgray", "gradient4", "ramp16", "scene"])
    parser.add_argument("--item", action="append", choices=list(ITEMS),
                        help="revert only these items (repeatable; default: all, plus the aggregates)")
    parser.add_argument("--negs-data", type=Path, default=ROOT / "data",
                        help="Negs data directory for the profile and neutral-filter reverts")
    rust = parser.add_mutually_exclusive_group()
    rust.add_argument("--rust-binary", type=Path, help="also compare this f64 CLI with every variant")
    rust.add_argument("--build-rust", action="store_true",
                      help="build the f64 CLI from this checkout and compare it with every variant")
    parser.add_argument("--rust-data", type=Path, default=ROOT / "data", help="data directory for the CLI")
    parser.add_argument("--save-arrays", action="store_true", help="keep every variant's output as .npy")
    args = parser.parse_args(argv)
    names = list(CASES) if not args.case or "all" in args.case else list(dict.fromkeys(args.case))
    args.routes = [(name, *CASES[name]) for name in names]
    if args.item:
        args.variants = {item: [item] for item in dict.fromkeys(args.item)}
    else:
        args.variants = {**{item: [item] for item in ITEMS}, **AGGREGATES}
    return parser, args


def main(argv=None):
    parser, args = parse_args(argv)
    source = args.python_source.resolve()
    if not (source / "src/spektrafilm/__init__.py").is_file():
        parser.error("--python-source must contain src/spektrafilm/__init__.py")
    negs_data = args.negs_data.resolve()
    if not (negs_data / "profiles").is_dir():
        parser.error("--negs-data must contain profiles/")
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    for variable, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"),
                             ("XDG_CACHE_HOME", "cache")]:
        os.environ.setdefault(variable, str(out / folder))
    sys.dont_write_bytecode = True
    sys.path.insert(0, str(source / "src"))

    import numpy as np
    import colour
    import OpenImageIO as oiio
    import spektrafilm
    from spektrafilm import digest_params, init_params

    if not Path(spektrafilm.__file__).resolve().is_relative_to(source):
        raise RuntimeError(f"wrong reference imported: {spektrafilm.__file__}")
    if reference_api(init_params(*CASES["portra-endura"][:2])) != EXPERIMENTAL:
        parser.error("--python-source must be upstream experimental (workflow.route, chemistry params)")
    from spektrafilm.data.profiles_loader import profile_from_dict
    from spektrafilm.model.illuminants import standard_illuminant
    from spektrafilm.runtime import pipeline as pl
    from spektrafilm.runtime.stages import filming
    from spektrafilm.utils import spectral_upsampling as su
    from spektrafilm.utils.autoexposure import measure_autoexposure_ev
    modules = {"np": np, "su": su, "pl": pl, "filming": filming, "init_params": init_params,
               "digest_params": digest_params, "profile_from_dict": profile_from_dict,
               "standard_illuminant": standard_illuminant,
               "measure_autoexposure_ev": measure_autoexposure_ev}
    neutral_filters = json.loads((negs_data / "filters/neutral_print_filters.json").read_text())
    binary = None
    if args.build_rust:
        binary = build_cli()
    elif args.rust_binary:
        binary = args.rust_binary.resolve()
        if not binary.is_file():
            parser.error("--rust-binary must exist")
    image = make_fixtures([args.fixture], np)[args.fixture]
    report = {"python_source": str(source), "fixture": args.fixture, "negs_data": str(negs_data),
              "rust_binary": None if binary is None else str(binary),
              "items": ITEMS, "aggregates": AGGREGATES, "routes": {}}
    for route, film, paper, scan_film in args.routes:
        print(f"{route} ({film} → {'scan' if scan_film else paper})", flush=True)
        reference = render(modules, film, paper, scan_film, [], image, negs_data, neutral_filters)
        outputs = {"exp": reference}
        result = {"reverts": {}}
        for variant, items in args.variants.items():
            outputs[variant] = render(modules, film, paper, scan_film, items, image, negs_data, neutral_filters)
            result["reverts"][variant] = stats(outputs[variant], reference, colour, np)
            print(f"  {variant:30s} {format_stats(result['reverts'][variant])}", flush=True)
        if binary is not None:
            rust = render_rust(binary, args.rust_data.resolve(), film, paper, scan_film, image,
                               out / route / "rust", oiio, np)
            result["rust_vs"] = {name: stats(rust, output, colour, np) for name, output in outputs.items()}
            baseline = result["rust_vs"]["exp"]["mean_dE00"]
            print("  Rust against each variant (* = closer than experimental):", flush=True)
            for name, s in result["rust_vs"].items():
                mark = "*" if name != "exp" and s["mean_dE00"] < baseline else " "
                print(f"   {mark}{name:30s} {format_stats(s)}", flush=True)
        if args.save_arrays:
            (out / route).mkdir(parents=True, exist_ok=True)
            for name, output in outputs.items():
                np.save(out / route / f"{name}.npy", output)
        report["routes"][route] = {"film": film, "paper": paper, "scan_film": scan_film, **result}
        (out / "ablation.json").write_text(json.dumps(report, indent=2, allow_nan=False) + "\n")
    print(f"Report: {out / 'ablation.json'}", flush=True)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as exc:
        print(f"ERROR: {type(exc).__name__}: {exc}", file=sys.stderr)
        sys.exit(2)
