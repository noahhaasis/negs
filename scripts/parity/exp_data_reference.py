"""Python experimental 28bf883 as the exp-data branch renders it (ticket 16).

This branch's items (profiles, model curves, white balance, neutral filter
database, upsampling methods) stay at experimental. Everything the other wave-1
branches port is reverted to what Negs does on this branch (validated by the
phase-0 ablation in docs/experimental-port-plan.md): gamut defaults and
centre, DIR presets / Langmuir / tail, halation presets, grain-preset
density_min, auto-exposure metering, the T/K75P illuminant fallback, and
PCHIP (not tetrahedral) 3D-LUT interpolation.

Import it with two environment variables set to clean `git archive` trees:
EXPDATA_REFERENCE (28bf883) and, for EXPDATA_WB_MAIN=1 (the profiles-only
state), EXPDATA_MAIN_REFERENCE (3bb2c2d, for its filter database). Once the
other branches are integrated, drop the matching reverts in `make_params`.
"""
import dataclasses
import json
import os
import sys
from pathlib import Path

EXP = Path(os.environ["EXPDATA_REFERENCE"]).resolve()
MAIN = Path(os.environ.get("EXPDATA_MAIN_REFERENCE", "/nonexistent")).resolve()
for var, folder in [("NUMBA_CACHE_DIR", "numba"), ("MPLCONFIGDIR", "mpl"), ("XDG_CACHE_HOME", "cache")]:
    os.environ.setdefault(var, f"/tmp/spektrafilm-exp-data-{folder}")
sys.dont_write_bytecode = True
sys.path.insert(0, str(EXP / "src"))

import numpy as np
import spektrafilm
from spektrafilm import init_params, digest_params
from spektrafilm.runtime import pipeline as pl
from spektrafilm.runtime.stages import filming as filming_mod
from spektrafilm.utils import spectral_upsampling as su
from spektrafilm.utils.autoexposure import measure_autoexposure_ev
from spektrafilm.data.profiles_loader import profile_from_dict

assert Path(spektrafilm.__file__).resolve().is_relative_to(EXP), spektrafilm.__file__
NEGS_DATA = Path(__file__).resolve().parents[2] / "data"

MAIN_COUPLERS = {
    "negative": dict(gamma_samelayer_rgb=(0.336, 0.319, 0.273), gamma_interlayer_r_to_gb=(0.353, 0.302),
                     gamma_interlayer_g_to_rb=(0.154, 0.353), gamma_interlayer_b_to_rg=(0.168, 0.226)),
    "positive": dict(gamma_samelayer_rgb=(0.12, 0.08, 0.06), gamma_interlayer_r_to_gb=(0.12, 0.06),
                     gamma_interlayer_g_to_rb=(0.08, 0.06), gamma_interlayer_b_to_rg=(0.06, 0.06)),
    "fujifilm_provia_100f": dict(gamma_samelayer_rgb=(0.156, 0.104, 0.078), gamma_interlayer_r_to_gb=(0.156, 0.078),
                                 gamma_interlayer_g_to_rb=(0.104, 0.078), gamma_interlayer_b_to_rg=(0.078, 0.078)),
    "fujifilm_velvia_100": dict(gamma_samelayer_rgb=(0.108, 0.072, 0.054), gamma_interlayer_r_to_gb=(0.108, 0.054),
                                gamma_interlayer_g_to_rb=(0.072, 0.054), gamma_interlayer_b_to_rg=(0.054, 0.054)),
}


# Commit-1 state of exp-data: white balance and the filter database still at main.
WB_MAIN = os.environ.get("EXPDATA_WB_MAIN") == "1"
MAIN_NPF = (json.loads((MAIN / "src/spektrafilm/data/filters/neutral_print_filters.json").read_text())
            if WB_MAIN else {})


def tc_lut_main_center(sensitivity, adaptation, gamut_compress=None):
    """Experimental's hanatos tc LUT with the input-gamut centre at the film
    reference illuminant (Negs main) instead of D65 (exp-defaults ports that)."""
    spectra_lut = su.HANATOS2025_SPECTRA_LUT
    assert not adaptation.spectral_gaussian_blur > 0
    if adaptation.apply_window and adaptation.window_params is not None:
        window = su.eval_spectral_bandpass_window(adaptation.window_params)
        if WB_MAIN:
            from spektrafilm.model.illuminants import standard_illuminant
            neutral = standard_illuminant(adaptation.reference_illuminant)
        else:
            white_tc = su._tri2quad(su._illuminant_to_xy(adaptation.reference_illuminant))
            neutral = su.apply_lut_cubic_2d(spectra_lut, np.asarray(white_tc).reshape(1, 1, 2))[0, 0]
        norm = (np.sum(sensitivity * neutral[:, None] * window, axis=0)
                / np.sum(sensitivity * neutral[:, None], axis=0))
        raw_lut = su.contract('ijl,lm->ijm', spectra_lut, sensitivity * (window / norm))
    else:
        raw_lut = su.contract('ijl,lm->ijm', spectra_lut, sensitivity)
    assert not (adaptation.apply_surface and adaptation.surface_params is not None)
    if gamut_compress is not None and gamut_compress.active:
        from spektrafilm.utils.gamut_compression import remap_tc_lut_for_compression
        raw_lut = remap_tc_lut_for_compression(
            raw_lut, su._illuminant_to_xy(adaptation.reference_illuminant), gamut_compress)
    return raw_lut


def ae_main(self, raw):
    if not self._camera.auto_exposure:
        return raw
    ev = measure_autoexposure_ev(self._resize_service.full_frame_preview, self._io.input_color_space,
                                 self._io.input_cctf_decoding, method=self._camera.auto_exposure_method)
    return raw * 2 ** ev


su.compute_hanatos2025_tc_lut = tc_lut_main_center
# Experimental made tetrahedral the default 3D-LUT interpolation (main: PCHIP);
# Negs still uses PCHIP. Only matters with use_enlarger_lut / use_scanner_lut.
from spektrafilm.utils import lut as _lut_mod, fast_interp_lut as _fil
_lut_mod.apply_lut_3d = lambda lut, image: _fil.apply_lut_pchip_3d(lut, image)
filming_mod.FilmingStage._auto_exposure = ae_main
filming_mod.FilmingStage._midgray_autoexposure_scale = lambda self, raw: 1.0


def load_profile(stock):
    """Negs' data (verbatim 28bf883 profiles plus the Negs-only stocks)."""
    return profile_from_dict(json.loads((NEGS_DATA / f"profiles/{stock}.json").read_text()))


def make_params(film, paper, scan_film=False, linear=True, tweak=None):
    params = init_params(film, paper or "kodak_portra_endura")
    params.film = load_profile(film)
    params.print = load_profile(paper or "kodak_portra_endura")
    params.film_render.grain.active = False
    params.film_render.glare.active = False
    params.print_render.glare.active = False
    if linear:
        params.io.input_color_space = "sRGB"
        params.io.input_cctf_decoding = False
        params.io.output_color_space = "sRGB"
        params.io.output_cctf_encoding = False
    if scan_film:
        params.workflow.route = "input > film > scan"
    params.io.input_gamut_compress = dataclasses.replace(
        params.io.input_gamut_compress, boundary="locus", knee=(0.0, 1.0, 6.0))
    params.io.output_gamut_compress = dataclasses.replace(
        params.io.output_gamut_compress, algorithm="cam16ucs", knee=(0.0, 1.0, 6.0),
        lightness_compression=(0.7, 1.0, 2.2))
    known = ("D50", "D55", "D65")
    if params.film.info.reference_illuminant not in known:
        params.film = params.film.update_info(reference_illuminant="D55")
    for attr in ("film", "print"):
        prof = getattr(params, attr)
        if prof.info.viewing_illuminant not in known:
            setattr(params, attr, prof.update_info(viewing_illuminant="D50"))
    if tweak:
        tweak(params, "pre")
    params = digest_params(params)
    dc = params.film_render.dir_couplers
    pol = "positive" if params.film.is_positive else "negative"
    vals = dict(MAIN_COUPLERS[pol])
    vals.update(MAIN_COUPLERS.get(film, {}))
    for k, v in vals.items():
        setattr(dc, k, v)
    dc.langmuir_donor_k_rgb = (np.inf,) * 3
    dc.langmuir_receiver_k_rgb = (np.inf,) * 3
    dc.diffusion_tail_weight = 0.06
    params.film_render.halation.halation_strength = (0.05, 0.015, 0.0)
    params.film_render.halation.halation_first_sigma_um = (65.0, 65.0, 65.0)
    # Grain presets (exp-grain) set density_min, which also bounds the scanner LUT.
    params.film_render.grain.density_min = (0.03, 0.03, 0.03)
    if WB_MAIN and params.settings.neutral_print_filters_from_database and not scan_film:
        entry = (MAIN_NPF.get(params.print.info.stock, {}).get(params.enlarger.illuminant, {})
                 .get(params.film.info.stock))
        c, m, y = (0.0, 65.0, 55.0) if entry is None else (float(v) for v in entry)
        params.enlarger.c_filter_neutral, params.enlarger.m_filter_neutral, params.enlarger.y_filter_neutral = c, m, y
    if tweak:
        tweak(params, "post")
    return params


def render(params, image, collect=None):
    pipe = pl.SimulationPipeline(params)
    return np.asarray(pipe.process(np.array(image, dtype=float), collect=collect), dtype=float), pipe
