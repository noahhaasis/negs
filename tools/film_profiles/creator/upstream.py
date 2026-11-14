"""Verbatim port of spektrafilm's public-era profile creator, steps 1-8 (negatives).

Source: github.com/andreavolpato/spektrafilm (then "agx-emulsion") at commit b2c7fe6
(2026-02-27), the last public version whose full chain runs. The same functions are at
src/profiles_factory/* in 9428440 (2026-03-16). Copyright (c) Andrea Volpato, GPLv3;
Negs is GPLv3 as well. Every function below is copied unchanged (generated with Python's
ast module, function by function); only the imports and the data loaders at the end of
this header are new. Plotting and the unused fitting models of the original files are
left out, and so are functions this tool does not call.

Do not edit the copied functions: the calibration gate (C200, docs/adding-film-stocks.md
section D8 check 3) compares this port with the original's output to 1e-6.
"""
# ruff: noqa
import copy
from pathlib import Path

import numpy as np
import scipy
import scipy.interpolate
import scipy.ndimage
import scipy.special
import scipy.stats
import scipy.optimize
from scipy.interpolate import CubicSpline
from scipy.optimize import least_squares
import lmfit
import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt
import colour

SPECTRAL_SHAPE = colour.SpectralShape(380, 780, 5)
LOG_EXPOSURE = np.linspace(-3, 4, 256)

# --- new: the original reads these through importlib package resources -------------
DENSITOMETER_DIR = None  # set by the caller: a folder holding <type>/responsivity_{r,g,b}.csv


def load_csv_path(path):
    """Same conversion as the original load_csv, from a file path."""
    conv = lambda x: float(x) if x != b'' else None
    return np.loadtxt(path, delimiter=',', converters=conv).transpose()


def load_densitometer_data(type='status_A', spectral_shape=SPECTRAL_SHAPE):
    """The original load_densitometer_data (utils/io.py), reading from DENSITOMETER_DIR."""
    responsivities = np.zeros((np.size(spectral_shape.wavelengths), 3))
    channels = ['r', 'g', 'b']
    for i, channel in enumerate(channels):
        data = load_csv_path(Path(DENSITOMETER_DIR) / type / ('responsivity_' + channel + '.csv'))
        responsivities[:, i] = interpolate_to_common_axis(data, spectral_shape.wavelengths, extrapolate=False, method='linear')
    responsivities[responsivities < 0] = 0
    responsivities /= np.nansum(responsivities, axis=0)
    return responsivities


def load_raw_data(stock_dir, dye_dir, spectral_shape=SPECTRAL_SHAPE, log_exposure=None):
    """The original load_agx_emulsion_data (utils/io.py) for a colour negative, reading
    the stock's CSVs from stock_dir and the C/M/Y dyes from dye_dir (the donor)."""
    if log_exposure is None:
        log_exposure = np.copy(LOG_EXPOSURE)
    stock_dir, dye_dir = Path(stock_dir), Path(dye_dir)
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
        data = load_csv_path(dye_dir / ('dye_density_' + channel + '.csv'))
        dye_density[:, i] = interpolate_to_common_axis(data, spectral_shape.wavelengths)
    for i, channel in enumerate(['min', 'mid']):
        data = load_csv_path(stock_dir / ('dye_density_' + channel + '.csv'))
        dye_density[:, i + 3] = interpolate_to_common_axis(data, spectral_shape.wavelengths)
    return log_sensitivity, dye_density, spectral_shape.wavelengths, density_curves, log_exposure


def standard_illuminant(type='D65', return_class=False):
    """The original model/illuminants.py standard_illuminant, for D*, T and BB* only."""
    if type[0:2] == 'BB':
        values = colour.colorimetry.blackbody.planck_law(SPECTRAL_SHAPE.wavelengths * 1e-9, np.double(type[2:]))
        spectral_intensity = colour.SpectralDistribution(values, domain=SPECTRAL_SHAPE)
    elif type == 'T':
        spectral_intensity = colour.SDS_LIGHT_SOURCES['Incandescent'].copy().align(SPECTRAL_SHAPE)
    else:
        spectral_intensity = colour.SDS_ILLUMINANTS[type].copy().align(SPECTRAL_SHAPE)
    normalization = np.sum(spectral_intensity.values) / np.size(SPECTRAL_SHAPE.wavelengths)
    spectral_intensity.values = spectral_intensity.values / normalization
    return spectral_intensity if return_class else spectral_intensity[:]



# --- copied verbatim from b2c7fe6 below ---------------------------------------------

# agx_emulsion/utils/io.py:108 @ b2c7fe6
def interpolate_to_common_axis(data, new_x,
                               extrapolate=False, method='akima'):
    x = data[0]
    y = data[1]
    sorted_indexes = np.argsort(x)
    x = x[sorted_indexes]
    y = y[sorted_indexes]
    unique_index = np.unique(x, return_index=True)[1]
    x = x[unique_index]
    y = y[unique_index]
    if method=='cubic':
        interpolator = scipy.interpolate.CubicSpline(x, y, extrapolate=extrapolate)
    if method=='akima':
        interpolator = scipy.interpolate.Akima1DInterpolator(x, y, extrapolate=extrapolate)
    elif method=='linear':
        def interpolator(x_new):
            return np.interp(x_new, x, y) #, left=np.nan, right=np.nan)
    elif method=='smoothing_spline':
        interpolator = scipy.interpolate.make_smoothing_spline(x, y)
    new_data = interpolator(new_x)
    return new_data

# agx_emulsion/utils/measure.py:13 @ b2c7fe6
def measure_slopes_at_exposure(log_exposure, density_curves, 
                               log_exposure_reference=0.0,
                               log_exposure_range=np.log10(2**2)):
    le_ref = log_exposure_reference
    log_exposure_0 = le_ref - log_exposure_range/2
    log_exposure_1 = le_ref + log_exposure_range/2
    gamma = np.zeros((3,))
    for i in range(3):
        sel = ~np.isnan(density_curves[:,i])
        density_1 = CubicSpline(log_exposure[sel], density_curves[sel,i])(log_exposure_1)
        density_0 = CubicSpline(log_exposure[sel], density_curves[sel,i])(log_exposure_0)
        gamma[i] = (density_1-density_0)/(log_exposure_1-log_exposure_0)
    return gamma

# agx_emulsion/utils/measure.py:27 @ b2c7fe6
def measure_density_min(log_exposure, density_curves, info_type):
    
    dc = density_curves
    le = log_exposure
    density_min = np.zeros(3)
    
    # fitting model for the toe
    def curve_toe(e, k):
        gamma = k[0]
        e0    = k[1]
        d0    = k[2]
        c1    = k[3] 
        y = (  gamma/c1 * np.log10(1 + 10**(c1 * (e - e0) ) ) 
            ) + d0
        return y
    
    # starting parameters of fit
    if info_type=='positive':
        k0 = (-1, 0, 0.05, -2)
    else:
        k0 = (1, 0, 0.05, 2)
    
    # fit all the toes and save density min
    for i in np.arange(3):
        data = np.copy(dc[:,i])
        # mask data above the toe (1/5 of density range)
        data[data>((np.nanmax(data)-np.nanmin(data))/5+np.nanmin(data))] = np.nan
        def residues(k):
            res = data - curve_toe(le, k)
            res = np.nan_to_num(res)
            return res
        fit = least_squares(residues, k0)
        k = fit.x
        density_min[i] = k[2]
    return density_min

# agx_emulsion/model/density_curves.py:11 @ b2c7fe6
def density_curve_model_norm_cdfs(loge,
                        x=[
                              0, 1, 2, # centers
                              0.5, 0.5, 0.5, # amplitudes
                              0.3, 0.5, 0.7, # sigmas
                             ],
                        type='negative',
                        number_of_layers = 3,
                        ):
    centers    = x[0:3]
    amplitudes = x[3:6]
    sigmas     = x[6:9]
    
    dloge_curve = np.zeros(loge.shape)
    for i, (center, amplitude, sigma) in enumerate(zip(centers,amplitudes,sigmas)):
        if i <= number_of_layers-1:
            if type=='positive':
                dloge_curve += scipy.stats.norm.cdf( -(loge-center)/sigma )*amplitude
            else:
                dloge_curve += scipy.stats.norm.cdf(  (loge-center)/sigma )*amplitude
    return dloge_curve

# agx_emulsion/model/density_curves.py:44 @ b2c7fe6
def density_curve_layers(loge,
                         x,
                         type='negative',
                         number_of_layers=3):  
    N = number_of_layers
    centers    = x[0  : N  ]
    amplitudes = x[N  : 2*N]
    sigmas     = x[2*N: 3*N]
    
    dloge_curve = np.zeros((loge.shape[0], 3))
    for i, (center, amplitude, sigma) in enumerate(zip(centers,amplitudes,sigmas)):
        if i <= number_of_layers-1:
            if type=='positive':
                dloge_curve[:,i] += scipy.stats.norm.cdf( -(loge-center)/sigma )*amplitude
            else:
                dloge_curve[:,i] += scipy.stats.norm.cdf(  (loge-center)/sigma )*amplitude
    return dloge_curve

# agx_emulsion/model/density_curves.py:62 @ b2c7fe6
def guess_start_and_bounds_norm_cdfs(loge, data, type):
    range = np.max(data) - np.min(data)
    if np.logical_or(type=='positive', type=='paper'):
        x0 = [
            np.mean(loge)-0.25, np.mean(loge), np.mean(loge)+0.25, # centers
            0.5, 1, 0.5, # amplitudes
            0.2, 0.3, 0.5, # sigmas
            ]
        x_lb = [
            np.min(loge), np.min(loge), np.min(loge),
            0.25, 0.25, 0.25,
            0.05, 0.05, 0.05,
            ]
        x_ub = [
            np.max(data), np.max(data), np.max(data),
            5.0, 5.0, 5.0,
            1, 1, 1,
            ]
    elif type=='negative':
        density_max_layer = 1.35
        x0 = [
            np.mean(loge)-0.5, np.mean(loge), np.mean(loge)+0.5, # centers
            0.5, 0.5, 0.5, # amplitudes
            0.3, 0.6, 0.9, # sigmas
            ]
        x_lb = [
                np.min(loge), np.min(loge), np.min(loge),
                0.2, 0.2, 0.2, # 0.35 in order to have a larger sensitive layer
                0.05, 0.05, 0.05,
                ]
        x_ub = [
                np.max(data), np.max(data), np.max(data),
                density_max_layer, density_max_layer, density_max_layer,
                2, 2, 2,
                ]
    return x0, (x_lb, x_ub)

# agx_emulsion/model/density_curves.py:203 @ b2c7fe6
def fit_density_curve(loge, data,
                      type='negative',
                      model='norm_cdfs'):
    if model=='norm_cdfs':
        model_function = density_curve_model_norm_cdfs
        guesses = guess_start_and_bounds_norm_cdfs
    if model=='log_line':
        model_function = density_curve_model_log_line
        guesses = guess_start_and_bounds_log_line
    nan_sel = np.isnan(data)
    loge = loge[~nan_sel]
    data = data[~nan_sel]
    x0, bounds = guesses(loge, data, type)
    print('density curves x0',x0)
    residues = lambda x: data - model_function(loge, x, type)
    fit = scipy.optimize.least_squares(residues,x0,bounds=bounds)
    return fit.x

# agx_emulsion/model/density_curves.py:183 @ b2c7fe6
def compute_density_curves(log_exposure, parameters, type, model='norm_cdfs'):
    density_out = np.zeros((np.size(log_exposure), 3))
    if model=='norm_cdfs':
        model_function = density_curve_model_norm_cdfs
    if model=='log_line':
        model_function = density_curve_model_log_line
    for i in np.arange(3):
        density_out[:,i] = model_function(log_exposure, parameters[i], type)
    return density_out

# agx_emulsion/model/density_curves.py:193 @ b2c7fe6
def compute_density_curves_layers(log_exposure, parameters, type):
    density_out = np.zeros((np.size(log_exposure), 3, 3))
    for i in np.arange(3):
        density_out[:,:,i] = density_curve_layers(log_exposure, parameters[i], type)
    return density_out

# agx_emulsion/profiles/reconstruct.py:25 @ b2c7fe6
def shift_stretch(wl, spectrum, amp=1.0, width=1.0, shift=0.0):
    center = wl[np.nanargmax(spectrum)]
    sel = ~np.isnan(spectrum)
    lam = 100
    sp = scipy.interpolate.make_smoothing_spline(wl[sel], spectrum[sel], lam=lam)
    spectrum_out = sp((wl-center)/width + center + shift)
    sp = scipy.interpolate.make_smoothing_spline(wl, spectrum_out, lam=lam)
    spectrum_out = sp(wl)
    spectrum_out[spectrum_out<0] = 0
    spectrum_out[~sel] = np.nan
    return amp*spectrum_out

# agx_emulsion/profiles/reconstruct.py:36 @ b2c7fe6
def shift_stretch_cmy(wl, cmy, da0, dw0, ds0, # amplitude, stretch, shift
                               da1, dw1, ds1,
                               da2, dw2, ds2):
    c = shift_stretch(wl, cmy[:,0], da0, dw0, ds0)
    m = shift_stretch(wl, cmy[:,1], da1, dw1, ds1)
    y = shift_stretch(wl, cmy[:,2], da2, dw2, ds2)
    return np.vstack([c,m,y]).T

# agx_emulsion/profiles/reconstruct.py:43 @ b2c7fe6
def gaussian_profiles(wl, p_couplers):
    density = np.zeros((np.size(wl), np.size(p_couplers, axis=0)))
    for i, ps in enumerate(p_couplers):
        density[:,i] += ps[0] * np.exp( -(wl-ps[2])**2/(2*ps[1]**2) )
    return density

# agx_emulsion/profiles/reconstruct.py:52 @ b2c7fe6
def make_reconstruct_dye_density_params(model='model_a'):
    params = lmfit.Parameters()
    # dyes
    params.add('dye_amp0', value=1.0, min=0.5, max=1.5)
    params.add('dye_amp1', value=1.0, min=0.5, max=1.5)
    params.add('dye_amp2', value=1.0, min=0.5, max=1.5)
    params.add('dye_width0', value=1.0, min=0.9, max=1.1)
    params.add('dye_width1', value=1.0, min=0.9, max=1.1)
    params.add('dye_width2', value=1.0, min=0.9, max=1.1)
    params.add('dye_shift0', value=0.0, min=-30, max=30)
    params.add('dye_shift1', value=0.0, min=-30, max=30)
    params.add('dye_shift2', value=0.0, min=-30, max=30)
    
    if model[:6]=='filter':
        params.add('lp_c_width', value=20, min=12, max=30) # test
        params.add('hp_m_width', value=20, min=12, max=30)
        params.add('lp_y_width', value=20, min=12, max=30)
        params.add('lp_m_width', value=20, min=12, max=30)
        params.add('hp_c_width', value=20, min=12, max=30)
        params.add('lp_c_wl', value=420, min=400, max=440) # test
        params.add('hp_m_wl', value=500, min=460, max=500)
        params.add('lp_y_wl', value=500, min=490, max=530)
        params.add('lp_m_wl', value=600, min=590, max=650)
        params.add('hp_c_wl', value=600, min=570, max=600)
    
    if model=='filters_neg':
        # negative components to make density zero on densitometer
        params.add('high_y',  value=-0.02, min=-0.1, max=0.0)
        params.add('low_m',   value=-0.02, min=-0.1, max=0.0)
        params.add('high_m',  value=-0.02, min=-0.1, max=0.0)
        params.add('low_c',   value=-0.02, min=-0.1, max=0.0)
        params.add('low_c_y', value=-0.02, min=-0.1, max=0.0) # test
    
    if model[:10]=='filteramps':
        params.add('lp_c_amp', value=0.8, min=0.0, max=1.0)
        params.add('hp_m_amp', value=0.8, min=0.0, max=1.0)
        params.add('lp_y_amp', value=0.8, min=0.0, max=1.0)
        params.add('lp_m_amp', value=0.8, min=0.0, max=1.0)
        params.add('hp_c_amp', value=0.8, min=0.0, max=1.0)

    if model=='dmid_dmin':
        # couplers
        params.add('cpl_amp0', value=0.1, min=0.05, max=0.5)
        params.add('cpl_amp1', value=0.1, min=0.05, max=0.5)
        params.add('cpl_amp2', value=0.1, min=0.05, max=0.5)
        params.add('cpl_amp3', value=0.03, min=0.0, max=0.5)
        params.add('cpl_amp4', value=0.1, min=0.05, max=0.5)
        params.add('cpl_width0', value=20, min=10, max=40)
        params.add('cpl_width1', value=20, min=10, max=40)
        params.add('cpl_width2', value=20, min=10, max=40)
        params.add('cpl_width3', value=40, min=10, max=50)
        params.add('cpl_width4', value=20, min=10, max=40)
        params.add('cpl_max0', value=435, min=420, max=450)
        params.add('cpl_max1', value=560, min=540, max=600)
        params.add('cpl_max2', value=475, min=455, max=490)
        params.add('cpl_max3', value=700, min=650, max=720)
        params.add('cpl_max4', value=510, min=495, max=535)
        # others
        params.add('dmax1', value=2.3, min=2.0, max=5)
        params.add('dmax0', value=2.3, min=2.0, max=5)
        params.add('dmax2', value=2.3, min=2.0, max=5)
        # params.add('dmax3', value=2.3, min=0.5, max=5)
        # params.add('dmax4', value=2.3, min=0.5, max=5)
        params.add('fog0', value=0.07, min=0.05, max=0.10, vary=True)
        params.add('fog1', value=0.07, min=0.05, max=0.10, vary=True)
        params.add('fog2', value=0.07, min=0.05, max=0.10, vary=True)
        params.add('scat400', value=0.65, min=0.62, max=1.0)
        params.add('base', value=0.05, min=0, max=0.15)
    return params

# agx_emulsion/profiles/reconstruct.py:122 @ b2c7fe6
def density_mid_min_model(params, wl, cmy_model, model):
    dmin = np.zeros_like(wl)
    dye = shift_stretch_cmy(wl, cmy_model, params['dye_amp0'], params['dye_width0'], params['dye_shift0'],
                                        params['dye_amp1'], params['dye_width1'], params['dye_shift1'],
                                        params['dye_amp2'], params['dye_width2'], params['dye_shift2'],)
    if model[:7]=='filters':
        hp_m = high_pass_filter(wl, params['hp_m_wl'], params['hp_m_width'])
        lp_y = low_pass_filter( wl, params['lp_y_wl'], params['lp_y_width'])
        lp_m = low_pass_filter( wl, params['lp_m_wl'], params['lp_m_width'])
        hp_c = high_pass_filter(wl, params['hp_c_wl'], params['hp_c_width'])
        lp_c = low_pass_filter( wl, params['lp_c_wl'], params['lp_c_width'])
        filters = np.stack((1-(1-hp_c)*(1-lp_c),
                            hp_m*lp_m,
                            lp_y), axis=1)
    if model=='filters':
        cmy = dye*filters
    if model=='filters_neg':
        cmy = dye*filters
        cmy[:,2] += ((1-lp_y)*(1-hp_c))*6*params['high_y']
        cmy[:,1] += (1-lp_m)*params['low_m']
        cmy[:,1] += (1-hp_m)*params['high_m']
        cmy[:,0] += ((1-lp_y)*(1-hp_c))**6*params['low_c']
        cmy[:,0] += ((1-hp_m)*(1-lp_c))**6*params['low_c_y'] # test
    
    ########## filters with amplitudes ##########
    
    if model[:10]=='filteramps':
        hp_m = high_pass_filter(wl, params['hp_m_wl'], params['hp_m_width'], params['hp_m_amp'])
        lp_y = low_pass_filter(wl, params['lp_y_wl'], params['lp_y_width'], params['lp_y_amp'])
        lp_m = low_pass_filter(wl, params['lp_m_wl'], params['lp_m_width'], params['lp_m_amp'])
        hp_c = high_pass_filter(wl, params['hp_c_wl'], params['hp_c_width'], params['hp_c_amp'])
        lp_c = low_pass_filter(wl, params['lp_c_wl'], params['lp_c_width'], params['lp_c_amp']) # test
        filters = np.stack((1-(1-hp_c)*(1-lp_c),
                            hp_m*lp_m,
                            lp_y), axis=1)
    if model=='filteramps':
        cmy = dye*filters
    if model=='filteramps_gauss':
        hp_c_gauss = high_pass_gaussian(wl, params['hp_c_wl'], params['hp_c_width'], params['hp_c_amp'])
        lp_c_gauss = low_pass_gaussian(wl, params['lp_c_wl'], params['lp_c_width'], params['lp_c_amp'])
        hp_m_gauss = high_pass_gaussian(wl, params['hp_m_wl'], params['hp_m_width'], params['hp_m_amp'])
        lp_m_gauss = low_pass_gaussian(wl, params['lp_m_wl'], params['lp_m_width'], params['lp_m_amp'])
        lp_y_gauss = low_pass_gaussian(wl, params['lp_y_wl'], params['lp_y_width'], params['lp_y_amp'])
        filters = np.stack((1-(1-hp_c)*(1-lp_c),
                            hp_m*lp_m,
                            lp_y), axis=1)
        gauss = np.stack((hp_c_gauss+lp_c_gauss,
                        hp_m_gauss+lp_m_gauss,
                        lp_y_gauss), axis=1)
        # filters[:,0] += lp_c # test
        cmy = dye*filters - gauss
        
    ########## dmid dmin ##########
    
    if model=='dmid_dmin':
        channels_couplers_gaussians = [0,0,1,1,2]
        p_couplers = [[params['cpl_amp0'], params['cpl_width0'], params['cpl_max0']],
                    [params['cpl_amp1'], params['cpl_width1'], params['cpl_max1']],
                    [params['cpl_amp2'], params['cpl_width2'], params['cpl_max2']],
                    [params['cpl_amp3'], params['cpl_width3'], params['cpl_max3']],
                    [params['cpl_amp4'], params['cpl_width4'], params['cpl_max4']]]
        cpl = gaussian_profiles(wl, p_couplers)
        cpl_cmy = np.zeros((np.size(wl), 3))
        for i in range(5):
            cpl_cmy[:,channels_couplers_gaussians[i]] += cpl[:,i]
        cmy = dye-cpl_cmy
        dmin, _, _, _, _, _ = density_min_model(params, wl, cmy, cpl, channels_couplers_gaussians)
        filters = cpl
    
    return cmy, dye, filters, dmin

# agx_emulsion/profiles/reconstruct.py:193 @ b2c7fe6
def density_min_model(params, wl, cmy, cpl, channels_couplers_gaussians):
    dcmy = [params['dye_amp0'], params['dye_amp1'], params['dye_amp2']]
    dmax = np.array([params['dmax0'], 
                    params['dmax1'], 
                    params['dmax2'],
                    # params['dmax3'],
                    # params['dmax4']
                    ])
    fog = np.array([params['fog0'],
                    params['fog1'],
                    params['fog2'],])
    base = np.ones_like(wl)*params['base']
    scattering = -np.log10(1-params['scat400']*400**4/wl**4)
    cpl_cmy = np.zeros_like(cmy)
    for i in range(5):
        # cpl_cmy[:,channels_couplers_gaussians[i]] += cpl[:,i]/dcmy[channels_couplers_gaussians[i]]*dmax[i]
        cpl_cmy[:,channels_couplers_gaussians[i]] += cpl[:,i]/dcmy[channels_couplers_gaussians[i]]*dmax[channels_couplers_gaussians[i]]
    dmin = np.sum(cpl_cmy + fog*cmy, axis=1) + scattering + base
    return dmin, cpl_cmy, scattering, fog, base, dmax

# agx_emulsion/profiles/reconstruct.py:215 @ b2c7fe6
def slopes_of_concentrations(log_exposure, density_curves, dstm_cm):
    c = np.zeros_like(density_curves)
    for i in range(3):
        for j in range(3):
            c[:,i] += np.linalg.inv(dstm_cm)[i,j] * density_curves[:,j] # TODO fix order of indexes of crosstalk matrix according to formalism
    gammas = measure_slopes_at_exposure(log_exposure, c)
    return gammas

# agx_emulsion/profiles/reconstruct.py:223 @ b2c7fe6
def residual_simple(params, wl, cmy_model, data, dstm, paper_sens, log_exposure, density_curves, model='model_a', biases=(1,2,2)):
    cmy, _, _, min_sim = density_mid_min_model(params, wl, cmy_model, model)
        
    # bias for out of diagonal crosstalk
    paper_cm = compute_densitometer_crosstalk_matrix(paper_sens, cmy)
    out_of_diagonal_crosstalk = paper_cm.flatten()[[1,2,3,5,6,7]]
    # bias for parallel gammas
    dstm_cm = compute_densitometer_crosstalk_matrix(dstm, cmy)
    gammas = slopes_of_concentrations(log_exposure, density_curves, dstm_cm)
    diff_gammas= gammas - np.mean(gammas)
    
    mid_minus_min_sim = np.sum(cmy, axis=1)
    sim = np.concatenate((mid_minus_min_sim, biases[0]*min_sim))
    res = data - sim
    res = np.concatenate((res, biases[1]*out_of_diagonal_crosstalk, biases[2]*diff_gammas)) # add crosstalk matrix to the loss function
    # 20 is an empirical bias to balance the weight of the crosstalk matrix in the loss function
    return res

# agx_emulsion/profiles/reconstruct.py:241 @ b2c7fe6
def compute_densitometer_crosstalk_matrix(densitometer_intensity, dye_density):
    crosstalk_matrix = np.zeros((3,3))
    dye_transmittance = 10**(-dye_density[:,0:3])
    for i in np.arange(3): # rgb of densitometer
        for j in np.arange(3): # cmy of dyes
            crosstalk_matrix[i,j] = -np.log10(
                np.nansum(densitometer_intensity[:,i]*dye_transmittance[:,j])
                / np.nansum(densitometer_intensity[:,i])
                )
    return crosstalk_matrix

# agx_emulsion/profiles/reconstruct.py:258 @ b2c7fe6
def reconstruct_dye_density(profile, params=None, control_plot=False, print_params=False,
                            target_print_paper=None, ymc_filter_values=[0.8,0.6,0.2],
                            max_nfev=500, tol=5e-5, model='dmid_dmin', biases=(1,2,0)
                            ):
    cmy_model = profile.data.dye_density[:,:3]
    wl = profile.data.wavelengths
    le = profile.data.log_exposure
    dc = profile.data.density_curves
    dmid = profile.data.dye_density[:,4]
    dmin = profile.data.dye_density[:,3]
    dmid_minus_min = dmid-dmin
    exp = np.concatenate((dmid_minus_min, dmin))
    
    dstm = load_densitometer_data(profile.info.densitometer)
    if target_print_paper is not None:
        paper_sens = 10**target_print_paper.data.log_sensitivity
        illuminant = standard_illuminant('BB3200')
        filtered_illuminat = dichroic_filters.apply(illuminant, values=ymc_filter_values)
        paper_sens = paper_sens * filtered_illuminat[:, None]
        # paper_sens[:,1] = dstm[:,1]
        # paper_sens[:,2] = dstm[:,2]
        
        plt.figure()
        plt.plot(wl, paper_sens)
    else:
        paper_sens = dstm

    if params is None:
        params = make_reconstruct_dye_density_params(model)
        
    out = lmfit.minimize(residual_simple, params,
                         args=(wl, cmy_model, exp, dstm, paper_sens, le, dc, model, biases),
                         nan_policy='omit', method='least_squares',
        **{
        'ftol': tol,
        'xtol': tol,
        'gtol': tol,
        'max_nfev': max_nfev
        })
    
    cmy, cmy_nofilt, filters, dmin_sim = density_mid_min_model(out.params, wl, cmy_model, model)

    dstm_cm = compute_densitometer_crosstalk_matrix(dstm, cmy)
    g = slopes_of_concentrations(le, dc, dstm_cm)
    
    if print_params:
        print('----------------------------------------')
        print('Reconstructed Dye Density Parameters')
        out.params.pretty_print()
        print('Slopes of conc. at le=0:', g)
        print('Crosstalk matrix:')
        print(dstm_cm)
    
    if control_plot:
        
        color = ['tab:cyan', 'tab:pink', 'gold']
        
        fig, axs = plt.subplots(1,3)
        fig.set_tight_layout(tight='rect')
        fig.set_figheight(4)
        fig.set_figwidth(12)
        fig.suptitle(profile.info.name)
    
        for i in range(3):
            axs[0].plot(wl, cmy[:,i], color=color[i], label='CMY'[i])
        axs[0].plot(wl, np.sum(cmy, axis=1), 'k--', label='Sim', alpha=0.5)
        axs[0].plot(wl, dmid_minus_min, 'k', label='Exp')
        axs[0].legend()
        axs[0].set_xlabel('Wavelength (nm)')
        axs[0].set_ylabel('Diffuse Density')
        axs[0].set_title('Midscale neutral minus minimum')

        for i in range(3):
            axs[1].plot(wl, cmy_nofilt[:,i], '--', color=color[i], label='_nolegend_')
            # axs[1].plot(wl, -cpl[:,i], color='tab:purple', label='_nolegend_')
            axs[1].plot(wl, cmy[:,i], color=color[i], alpha=1.0, label='CMY'[i])
        axs[1].text(
                0.5,            # x-position (in axes fraction)
                0.02,           # y-position (in axes fraction) 
                'Dashed lines are without masking couplers.', 
                transform=axs[1].transAxes, 
                ha='center',    # horizontal alignment
                va='bottom',    # vertical alignment
                fontsize=9, 
                color='k'
            )
        axs[1].set_xlabel('Wavelength (nm)')
        axs[1].set_ylabel('Diffuse Density')
        axs[1].set_title('CMY and masking couplers')
        axs[1].legend()
        
        if model=='dmid_dmin':
            dmin_sim, cpl, scat, fog, base, dmax = density_min_model(out.params, wl, cmy, filters, [0,0,1,1,2])
            axs[2].plot(wl, np.ones_like(wl)*base, 'tab:green', label='Base')
            axs[2].plot(wl, base+scat, 'tab:blue', label='Scattering')
            axs[2].plot(wl, base+scat+np.sum(fog*cmy, axis=1) + cpl[:,0], color='tab:cyan', label='Mask C')
            axs[2].plot(wl, base+scat+np.sum(fog*cmy, axis=1) + cpl[:,1], color='tab:pink', label='Mask M')
            axs[2].plot(wl, base+scat+np.sum(fog*cmy, axis=1) + cpl[:,2], color='gold', label='Mask Y')
            axs[2].plot(wl, base+scat+np.sum(fog*cmy, axis=1), color='tab:orange', label='Fog')
            axs[2].plot(wl, dmin_sim, 'k--', label='Sim', alpha=0.5)
        else:
            for i in range(3):
                axs[2].plot(wl, filters[:,i], color=color[i], label='CMY'[i])
        axs[2].plot(wl, dmin, 'k', label='Exp')
        axs[2].set_xlabel('Wavelength (nm)')
        axs[2].set_ylabel('Diffuse Density')
        axs[2].set_title('Minimum density')
        axs[2].set_ylim([0, np.nanmax(dmin)*1.05])
        axs[2].legend()
            
            

    profile.info.density_midscale_neutral = np.nanmax(cmy, axis=0).tolist()
    profile.data.dye_density[:,:3] = cmy/np.nanmax(cmy, axis=0)
    # profile.info.density_midscale_neutral = np.nanmax(cmy[:,1])
    # profile.data.dye_density[:,:3] = cmy/np.nanmax(cmy[:,1])
    return profile

# agx_emulsion/profiles/balance.py:10 @ b2c7fe6
def balance_sensitivity(profile, correct_log_exposure=True, band_pass_filter=False):
    ls = profile.data.log_sensitivity
    le = profile.data.log_exposure
    dc = profile.data.density_curves
    ill = standard_illuminant(type=profile.info.reference_illuminant)
    s = 10**np.double(ls)
    
    if band_pass_filter:
        filter_uv = (1, 410, 8)
        filter_ir = (1, 675, 15)
        band_pass_filter = compute_band_pass_filter(filter_uv,
                                                    filter_ir)
        ill *= band_pass_filter
    
    neutral_exposures = np.nansum(ill[:,None]*s, axis=0)
    corr = neutral_exposures[1]/neutral_exposures
    print('--- Balance Sensitivity')
    print('Correction factors for sensitivity:', corr)
    log_exp_correction = np.log10(corr)
    print('Log exposure correction of density curves:', log_exp_correction)
    
    s *= corr
    ls = np.log10(s)
    profile.data.log_sensitivity = ls
    
    if correct_log_exposure:
        dc_out = np.zeros_like(dc)
        for i in np.arange(3):
            dc_out[:,i] = np.interp(le, le+log_exp_correction[i], dc[:,i])
        profile.data.density_curves = dc_out
    
    return profile

# agx_emulsion/profiles/correct.py:7 @ b2c7fe6
def measure_log_exposure_midscale_neutral(profile, reference_channel=None):
    log_exposure_midscale_neutral = np.zeros((3,))
    d_mid = profile.info.density_midscale_neutral
    if np.size(d_mid)==1: 
        d_mid = np.ones(3) * d_mid
    if reference_channel=='green':
        d_mid = np.ones(3) * d_mid[1]
    for i in range(3):
        if profile.info.type=='positive':
            log_exposure_midscale_neutral[i] = np.interp(-d_mid[i], -profile.data.density_curves[:,i], profile.data.log_exposure)
        else:
            log_exposure_midscale_neutral[i] = np.interp(d_mid[i], profile.data.density_curves[:,i], profile.data.log_exposure)
    print('Log exposure midscale neutral:', log_exposure_midscale_neutral)
    return log_exposure_midscale_neutral

# agx_emulsion/profiles/correct.py:22 @ b2c7fe6
def align_midscale_neutral_exposures(profile, reference_channel=None):
    log_exposure_midscale_neutral = measure_log_exposure_midscale_neutral(profile, reference_channel)
    dc = profile.data.density_curves
    le = profile.data.log_exposure
    for i in np.arange(3):
            dc[:,i] = np.interp(le, le-log_exposure_midscale_neutral[i], dc[:,i])
    profile.data.density_curves = dc
    profile.info.log_exposure_midscale_neutral = (np.ones(3)*log_exposure_midscale_neutral[1]).tolist()
    return profile

# agx_emulsion/profiles/factory.py:41 @ b2c7fe6
def fit_density_curves(log_exposure,
                       density,
                       model='norm_cdfs',
                       type='negative'):
    fitted_parameters = np.zeros((3,9))
    for i in np.arange(3):
        fitted_parameters[i] = fit_density_curve(log_exposure, density[:,i],
                                                 type, model=model)
    return fitted_parameters

# agx_emulsion/profiles/factory.py:66 @ b2c7fe6
def unmix_density_curves(curves, crosstalk_matrix):
    inverse_cm = np.linalg.inv(crosstalk_matrix)
    density_curves_raw = np.einsum('ij, kj-> ki',inverse_cm, curves)
    density_curves_raw = np.clip(density_curves_raw,0,None)
    return density_curves_raw

# agx_emulsion/profiles/factory.py:302 @ b2c7fe6
def remove_density_min(profile):
    
    le = profile.data.log_exposure
    dc = profile.data.density_curves
    dd = profile.data.dye_density
    wl = profile.data.wavelengths
    type = profile.info.type

    # take care of density min
    # dc_min = np.nanmin(dc, axis=0)
    dc_min = measure_density_min(le, dc, type)
    dc = dc - dc_min
    print('Density curve min values:', dc_min)
    
    # if positive or paper, add b+f to the dye_density min and mid
    if type=='paper' or type=='positive':
        status_a_max_peak = [445, 530, 610] # nm, plus two far values for extrapolation
        smin = np.interp(wl, status_a_max_peak, np.flip(dc_min))
        sigma = 20 # nm
        sigma_points = sigma / np.mean(np.diff(wl))
        smin = scipy.ndimage.gaussian_filter1d(smin, sigma_points)
        dd[:,3] = smin

    # save dye_density
    profile.data.dye_density = dd
    profile.data.density_curves = dc
    return profile

# agx_emulsion/profiles/factory.py:330 @ b2c7fe6
def adjust_log_exposure(profile,
                        speed_point_density=0.2,
                        stops_over_speed_point=3,
                        midgray_transmittance=0.184
                        ):
    if profile.info.type=='paper' or profile.info.type=='positive':
        speed_point_density = np.log10(1/midgray_transmittance)
        stops_over_speed_point = 0
    
    # use the green channel to find the speed point log_exposure
    # then add stops_over_speed_point to the speed point log_exposure
    # finally assign to that log_exposure zero value
    print('Reference density:', speed_point_density)
    print('Stops over reference density:', stops_over_speed_point, 'EV')
    dcg = profile.data.density_curves[:,1]
    dcg = dcg - np.nanmin(dcg)
    le = profile.data.log_exposure
    sel = ~np.isnan(dcg)
    if profile.info.type=='negative' or profile.info.type=='paper':
        le_speed_point = np.interp(speed_point_density, dcg[sel], le[sel])
    if profile.info.type=='positive':
        le_speed_point = np.interp(-speed_point_density, -dcg[sel], le[sel])
    print('Log exposure refenrece:', le_speed_point)
    le_over_speed_point = np.log10(2**stops_over_speed_point)
    le_midgray = le_speed_point + le_over_speed_point
    profile.data.log_exposure = le - le_midgray
    return profile

# agx_emulsion/profiles/factory.py:421 @ b2c7fe6
def unmix_density(profile):
    dc = profile.data.density_curves
    dd = profile.data.dye_density
    # dd = dd / np.nanmax(dd, axis=0)
    
    # cross talk matrix
    ds = load_densitometer_data(type=profile.info.densitometer)
    densitometer_crosstalk_matrix = compute_densitometer_crosstalk_matrix(ds, dd[:,0:3])
    print('densitometer C: ')
    print(densitometer_crosstalk_matrix)
    
    dc = unmix_density_curves(dc, densitometer_crosstalk_matrix)
    
    # dc[dc<0] = 0 # HERE
    profile.data.density_curves = dc
    return profile

# agx_emulsion/profiles/factory.py:438 @ b2c7fe6
def densitometer_normalization(profile):
    dc = profile.data.density_curves
    dd = profile.data.dye_density
    dstm = load_densitometer_data(type=profile.info.densitometer)
    
    M = compute_densitometer_crosstalk_matrix(dstm, dd)
    norm_coeffs = np.diag(M)
    print('Dye density densitometer normalization coefficients:', norm_coeffs)
    dd[:,:3] = dd[:,:3] / norm_coeffs
    dc = dc * norm_coeffs
    
    profile.data.dye_density = dd
    profile.data.density_curves = dc
    return profile

# agx_emulsion/profiles/factory.py:453 @ b2c7fe6
def replace_fitted_density_curves(profile, control_plot=False):
    dc = profile.data.density_curves
    le = profile.data.log_exposure
    type = profile.info.type
    
    # fit density for smoother curves and complete toe and shoulder
    density_curves_fitting_parameters = fit_density_curves(le, dc, type=type)
    print('density_curves_fitting_parameters: ', density_curves_fitting_parameters)
    density_curves_prefit = np.copy(dc)
    dc = compute_density_curves(le, density_curves_fitting_parameters, type=type)
    profile.data.density_curves = dc
    profile.data.density_curves_layers = compute_density_curves_layers(le, density_curves_fitting_parameters, type=type)
    
    if control_plot:
        plt.figure()
        plt.plot(le, dc)
        plt.plot(le, density_curves_prefit, color='gray', linestyle='--')
        plt.legend(('r','g','b'))
        plt.xlabel('Log Exposure')
        plt.ylabel('Density (over B+F)')
    return profile
