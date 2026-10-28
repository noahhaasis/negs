#!/usr/bin/env python3
"""Headless full-resolution render with Python spektrafilm, as its GUI does.

Usage: python_spektrafilm.py SPEKTRAFILM_SRC INPUT OUTPUT FILM PAPER

Builds the parameters from the GUI's default state for the stock pair
(including its export settings: enlarger, scanner and convert LUTs), runs
the full-resolution simulation in float64 like the GUI's Scan, and saves a
16-bit sRGB TIFF. The input is linear ProPhoto RGB (prepare_inputs.py).
"""
import sys
import time

src, input_path, output_path, film, paper = sys.argv[1:]
sys.path.insert(0, src)
import numpy as np  # noqa: E402
from spektrafilm import simulate  # noqa: E402
from spektrafilm.utils.io import load_image_oiio, save_image_oiio  # noqa: E402
from spektrafilm_gui.params_mapper import build_params_from_state  # noqa: E402
from spektrafilm_gui.state import build_default_gui_state  # noqa: E402

start = time.monotonic()
image = np.double(load_image_oiio(input_path)[..., :3])
params = build_params_from_state(build_default_gui_state(film_stock=film, print_paper=paper))
params.settings.preview_mode = False
params.io.input_color_space = 'ProPhoto RGB'
params.io.input_cctf_decoding = False
loaded = time.monotonic()
result = simulate(image, params, print_timings=True)
simulated = time.monotonic()
save_image_oiio(output_path, result[..., :3], bit_depth=16,
                color_space=params.io.output_color_space, cctf_encoding=params.io.output_cctf_encoding)
saved = time.monotonic()
print(f'load {loaded - start:.2f} s, simulate {simulated - loaded:.2f} s, save {saved - simulated:.2f} s')
