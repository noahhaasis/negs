"""RGB8 exact metrics and full-pixel CIEDE2000 (D65, colour's sRGB matrix)."""
import math
import numpy as np
from numba import njit, prange


@njit(cache=True, parallel=True)
def lab8(rgb):
    flat = rgb.reshape(-1, 3)
    lab = np.empty(flat.shape, np.float64)
    for i in prange(len(flat)):
        rgb_l = np.empty(3, np.float64)
        for c in range(3):
            v = flat[i, c] / 255.
            rgb_l[c] = v / 12.92 if v <= .04045 else ((v + .055) / 1.055) ** 2.4
        r, g, b = rgb_l
        x = (.4124*r + .3576*g + .1805*b) / .9504559270516716
        y = .2126*r + .7152*g + .0722*b
        z = (.0193*r + .1192*g + .9505*b) / 1.0890577507598784
        fx = x ** (1/3) if x > (6/29)**3 else x / (3*(6/29)**2) + 4/29
        fy = y ** (1/3) if y > (6/29)**3 else y / (3*(6/29)**2) + 4/29
        fz = z ** (1/3) if z > (6/29)**3 else z / (3*(6/29)**2) + 4/29
        lab[i] = (116*fy-16, 500*(fx-fy), 200*(fy-fz))
    return lab


@njit(cache=True, parallel=True)
def delta_e(a, b):
    values = np.empty(len(a), np.float64)
    pi = math.pi
    for i in prange(len(a)):
        l1, a1, b1 = a[i]
        l2, a2, b2 = b[i]
        c1 = math.hypot(a1, b1)
        c2 = math.hypot(a2, b2)
        cm = (c1+c2)/2
        g = .5*(1-math.sqrt(cm**7/(cm**7+25.**7)))
        ap1, ap2 = (1+g)*a1, (1+g)*a2
        cp1, cp2 = math.hypot(ap1, b1), math.hypot(ap2, b2)
        hp1 = math.atan2(b1, ap1) % (2*pi)
        hp2 = math.atan2(b2, ap2) % (2*pi)
        dl = l2-l1
        dc = cp2-cp1
        dh = hp2-hp1
        if cp1*cp2 == 0:
            dh = 0.
        elif dh > pi:
            dh -= 2*pi
        elif dh < -pi:
            dh += 2*pi
        dhp = 2*math.sqrt(cp1*cp2)*math.sin(dh/2)
        lm, cpm = (l1+l2)/2, (cp1+cp2)/2
        if cp1*cp2 == 0:
            hm = hp1+hp2
        elif abs(hp1-hp2) <= pi:
            hm = (hp1+hp2)/2
        elif hp1+hp2 < 2*pi:
            hm = (hp1+hp2+2*pi)/2
        else:
            hm = (hp1+hp2-2*pi)/2
        t = 1-.17*math.cos(hm-pi/6)+.24*math.cos(2*hm)+.32*math.cos(3*hm+pi/30)-.20*math.cos(4*hm-63*pi/180)
        sl = 1+.015*(lm-50)**2/math.sqrt(20+(lm-50)**2)
        sc = 1+.045*cpm
        sh = 1+.015*cpm*t
        rt = -2*math.sqrt(cpm**7/(cpm**7+25.**7))*math.sin(pi/3*math.exp(-((hm*180/pi-275)/25)**2))
        dl, dc, dhp = dl/sl, dc/sc, dhp/sh
        values[i] = math.sqrt(max(0., dl*dl+dc*dc+dhp*dhp+rt*dc*dhp))
    return values


def rgb_metrics(a, b, labs=None):
    if a.shape != b.shape:
        return {'status': 'dimension-mismatch', 'a_dimensions': [a.shape[1], a.shape[0]],
                'b_dimensions': [b.shape[1], b.shape[0]], 'exact_equal': False}
    # Chunk integer statistics to keep full-resolution pair memory bounded.
    exact = within = total_abs = maximum = 0
    bias = np.zeros(3)
    for start in range(0, a.shape[0], 128):
        d = a[start:start+128].astype(np.int16)-b[start:start+128].astype(np.int16)
        ad = np.abs(d)
        exact += int(np.all(ad == 0, axis=2).sum())
        within += int(np.all(ad <= 1, axis=2).sum())
        total_abs += int(ad.sum())
        maximum = max(maximum, int(ad.max()))
        bias += d.sum(axis=(0, 1))
    count = a.shape[0]*a.shape[1]
    if exact == count:
        de = np.zeros(1)  # every ΔE is exactly zero; avoid recomputing identities
    else:
        la, lb = labs if labs is not None else (lab8(a), lab8(b))
        de = delta_e(la, lb)
    return {'status': 'compared', 'bit_depth': 8, 'pixels': count, 'exact_equal': exact == count,
            'exact_rgb_pixel_fraction': exact/count, 'within_one_code_rgb_pixel_fraction': within/count,
            'max_abs_channel_codes': maximum, 'mean_abs_channel_codes': total_abs/(count*3),
            'signed_channel_bias_codes': (bias/count).tolist(),
            'delta_e_2000': {'mean': float(de.mean()), 'p99': float(np.quantile(de, .99)),
                            'p999': float(np.quantile(de, .999)), 'max': float(de.max()),
                            'percent_over_1': float(np.mean(de > 1)*100), 'percent_over_2': float(np.mean(de > 2)*100)}}


def float_metrics(a, b):
    if a.shape != b.shape:
        return {'status': 'dimension-mismatch', 'exact_equal': False}
    na, nb = int((~np.isfinite(a)).sum()), int((~np.isfinite(b)).sum())
    if na or nb:
        return {'status': 'nonfinite-failure', 'a_nonfinite': na, 'b_nonfinite': nb, 'exact_equal': False}
    diff = np.abs(a-b)
    relative = diff / np.maximum(np.maximum(np.abs(a), np.abs(b)), 1e-12)
    return {'status': 'compared', 'a_nonfinite': na, 'b_nonfinite': nb,
            'exact_equal': bool(np.array_equal(a, b)), 'max_abs': float(diff.max()),
            'mean_abs': float(diff.mean()), 'max_relative': float(relative.max()), 'mean_relative': float(relative.mean()),
            'relative_zero_handling': 'abs(a-b)/max(abs(a),abs(b),1e-12)',
            'all_components_within_1e_6': bool(np.all(diff <= 1e-6)),
            'gate_scope': 'diagnostic only; existing stage-gate scope is not changed'}
