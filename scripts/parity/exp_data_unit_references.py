"""Print the reference values of the inline Rust parity tests (full chain in
stages/mod.rs, taps in stages/debug_compare.rs, preflash in enlarger.rs, the
print B&W correction in color_reference.rs, the print morph in
print_morph.rs) from experimental 28bf883 via exp_data_reference.py.

Usage: EXPDATA_REFERENCE=/path/to/28bf883 python -I scripts/parity/exp_data_unit_references.py
"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
import numpy as np
import exp_data_reference as R
from spektrafilm.model.illuminants import standard_illuminant
from spektrafilm.utils.morph_curves import FilmChemistryParams, apply_print_curves_morph


def fmt(v):
    return "[" + ", ".join(repr(float(x)) for x in np.ravel(v)) + "]"


def no_effects(p, when):
    if when == "post":
        p.camera.auto_exposure = False
        p.film_render.halation.active = False
        p.film_render.dir_couplers.active = False


# A. full chain (stages/mod.rs)
def full_chain_tweak(p, when):
    no_effects(p, when)
    if when == "post":
        p.scanner.unsharp_mask = (0.0, 0.0)
params = R.make_params("kodak_portra_400", "kodak_portra_endura", tweak=full_chain_tweak)
out, _ = R.render(params, np.full((1, 1, 3), 0.184))
print("A midgray", fmt(out[0, 0]))
grad = np.array([0.05 + 0.5 * ((i * 37) % 256) / 255.0 for i in range(48)]).reshape(4, 4, 3)
params = R.make_params("kodak_portra_400", "kodak_portra_endura", tweak=full_chain_tweak)
out, _ = R.render(params, grad)
for px in out.reshape(-1, 3):
    print("A", fmt(px) + ",")

# B. taps (debug_compare.rs)
for tap in ["log_e_film", "cmy_film", "log_e_print", "cmy_print"]:
    params = R.make_params("kodak_portra_400", "kodak_portra_endura", tweak=no_effects)
    out, _ = R.render(params, np.full((1, 1, 3), 0.184), collect=tap)
    print("B", tap, fmt(out[0, 0]))

# C. preflash (enlarger.rs)
def preflash_tweak(p, when):
    if when == "pre":
        p.settings.neutral_print_filters_from_database = False
    else:
        p.enlarger.preflash_exposure = 0.0625
params = R.make_params("kodak_portra_400", "kodak_portra_endura", tweak=preflash_tweak)
pipe = R.pl.SimulationPipeline(params)
st = pipe._printing_stage
sens = np.nan_to_num(10 ** st._print.data.log_sensitivity)
print("C preflash", fmt(st._compute_raw_preflash(standard_illuminant(st._enlarger.illuminant), sens)),
      "filters", st._enlarger.c_filter_neutral, st._enlarger.m_filter_neutral, st._enlarger.y_filter_neutral)

# D. print B&W correction (color_reference.rs)
def bw_tweak(p, when):
    if when == "post":
        p.scanner.white_correction = True
        p.scanner.black_correction = True
        p.camera.auto_exposure = False
params = R.make_params("kodak_portra_400", "kodak_portra_endura", linear=False, tweak=bw_tweak)
pipe = R.pl.SimulationPipeline(params)
pipe.process(np.full((1, 1, 3), 0.184))
cr = pipe._color_reference_service
corr = cr.black_white_printing_exposure_correction()
m = (cr._white_level - cr._black_level) / (cr._y_white - cr._y_black + 1e-10)
q = cr._black_level - m * cr._y_black
print("D m", fmt(m), "q", fmt(q), "printing_exposure_correction", fmt(corr),
      "filming", cr.black_white_filming_exposure_correction())

# E. print morph (print_morph.rs)
endura = R.load_profile("kodak_portra_endura")
morph = FilmChemistryParams(active=True, gamma_factor=1.1, gamma_factor_fast=0.9, gamma_factor_slow=1.2,
                            gamma_factor_red=1.05, gamma_factor_green=0.95, gamma_factor_blue=1.0,
                            developer_exhaustion=0.3)
curves = apply_print_curves_morph(endura.data.log_exposure, endura.data.density_curves_model, morph,
                                  profile_type=endura.info.type)
for i in [0, 32, 64, 100, 128, 160, 200, 255]:
    print("E", i, fmt(curves[i]))
