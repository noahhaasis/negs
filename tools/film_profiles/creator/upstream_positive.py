"""Verbatim port of the parts of spektrafilm's public-era creator that only slides need.

Source: github.com/andreavolpato/spektrafilm (then "agx-emulsion") at commit b2c7fe6
(2026-02-27), copyright (c) Andrea Volpato, GPLv3 (Negs is GPLv3 as well), as for
creator/upstream.py. balance_metameric_neutral is copied unchanged from
agx_emulsion/profiles/balance.py:62; the loader is agx_emulsion/utils/io.py:151
load_agx_emulsion_data restricted to its type='positive' branch, reading from a folder
as upstream.load_raw_data does. Only the imports are new.

Do not edit the copied function: check_slide_port.py compares this port, run in the
slide order, with the original b2c7fe6 code (docs/adding-film-stocks.md D11).
"""
# ruff: noqa
from pathlib import Path

import numpy as np
import scipy
import scipy.optimize
import colour
from colour.models import RGB_COLOURSPACE_sRGB

from creator.upstream import (SPECTRAL_SHAPE, LOG_EXPOSURE, load_csv_path, interpolate_to_common_axis,
                              standard_illuminant)

# agx_emulsion/config.py:10 @ b2c7fe6
STANDARD_OBSERVER_CMFS = colour.MSDS_CMFS["CIE 1931 2 Degree Standard Observer"].copy().align(SPECTRAL_SHAPE)


def load_raw_data_positive(stock_dir, spectral_shape=SPECTRAL_SHAPE, log_exposure=None):
    """The original load_agx_emulsion_data for type='positive' with no donors: the
    stock's own sensitivities, curves and C/M/Y dyes; D-min and D-mid are not loaded
    (left 0, as the original leaves them)."""
    if log_exposure is None:
        log_exposure = np.copy(LOG_EXPOSURE)
    stock_dir = Path(stock_dir)
    log_sensitivity = np.zeros((np.size(spectral_shape.wavelengths), 3))
    for i, channel in enumerate(['r', 'g', 'b']):
        data = load_csv_path(stock_dir / ('log_sensitivity_' + channel + '.csv'))
        log_sensitivity[:, i] = interpolate_to_common_axis(data, spectral_shape.wavelengths)
    dh_curve_r = load_csv_path(stock_dir / 'density_curve_r.csv')
    dh_curve_g = load_csv_path(stock_dir / 'density_curve_g.csv')
    dh_curve_b = load_csv_path(stock_dir / 'density_curve_b.csv')
    log_exposure_shift = (np.max(dh_curve_g[0, :]) + np.min(dh_curve_g[0, :])) / 2
    p_denc_r = interpolate_to_common_axis(dh_curve_r, log_exposure + log_exposure_shift)
    p_denc_g = interpolate_to_common_axis(dh_curve_g, log_exposure + log_exposure_shift)
    p_denc_b = interpolate_to_common_axis(dh_curve_b, log_exposure + log_exposure_shift)
    density_curves = np.array([p_denc_r, p_denc_g, p_denc_b]).transpose()
    dye_density = np.zeros((np.size(spectral_shape.wavelengths), 5))
    for i, channel in enumerate(['c', 'm', 'y']):
        data = load_csv_path(stock_dir / ('dye_density_' + channel + '.csv'))
        dye_density[:, i] = interpolate_to_common_axis(data, spectral_shape.wavelengths)
    return log_sensitivity, dye_density, spectral_shape.wavelengths, density_curves, log_exposure


# --- copied verbatim from b2c7fe6 below ---------------------------------------------

# agx_emulsion/profiles/balance.py:62 @ b2c7fe6
def balance_metameric_neutral(profile, midgray_value=0.184):
    illuminant = standard_illuminant(profile.info.viewing_illuminant)

    def rgb_mid(mid, illuminant=illuminant):
        light = 10**(-mid)*illuminant[:]
        light[np.isnan(light)] = 0

        normalization = np.sum(illuminant * STANDARD_OBSERVER_CMFS[:, 1], axis=0)
        xyz = np.einsum('k,kl->l', light, STANDARD_OBSERVER_CMFS[:]) / normalization
        illuminant_xyz = np.einsum('k,kl->l', illuminant, STANDARD_OBSERVER_CMFS[:]) / normalization
        illuminant_xy = colour.XYZ_to_xy(illuminant_xyz)
        rgb = colour.XYZ_to_RGB(xyz, RGB_COLOURSPACE_sRGB, apply_cctf_encoding=False, illuminant=illuminant_xy)
        return rgb

    def midscale_neutral(density_cmy, dye_density=profile.data.dye_density):
        mid = np.sum(dye_density[:,:3] * density_cmy, axis=1) + dye_density[:,3]
        return mid

    transmittance_0 = midgray_value
    # density_0 = np.log10(1/transmittance_0)
    # density_0 += np.nanmean(profile.data.dye_density[:,3])
    # transmittance_0 = 10**(-density_0)
    rgb_0 = np.ones(3)*transmittance_0

    def residues(params):
        mid = midscale_neutral(density_cmy=params)
        rgb = rgb_mid(mid)
        res = rgb_0 - rgb
        return res

    fit = scipy.optimize.least_squares(residues, [1.0, 1.0, 1.0])
    d_cmy_metameric = fit.x
    profile.info.density_midscale_neutral = d_cmy_metameric[1]
    d_cmy_scale = d_cmy_metameric / d_cmy_metameric[1]
    mid = midscale_neutral(d_cmy_metameric)
    # rgb = rgb_mid(mid, viewing_illuminant=p.info.viewing_illuminant)
    print('--- Balance Metameric Neutral')
    print('Density CMY of metameric neutral: ', d_cmy_metameric)
    print('Apllied density scale factors: ', d_cmy_scale)
    profile.data.dye_density[:,4] = mid
    profile.data.dye_density[:, :3] *= d_cmy_scale
    return profile
