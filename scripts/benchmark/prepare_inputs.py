#!/usr/bin/env python3
"""Decode each RAW once into the shared benchmark input.

Usage: prepare_inputs.py SPEKTRAFILM_SRC OUT_DIR NAME=RAW [NAME=RAW ...]

Uses Python spektrafilm's own RAW loader (rawpy, as-shot white balance, no
lens correction) and writes linear ProPhoto RGB as a 16-bit TIFF: the input
Python spektrafilm and spektrafilm-rs assume by default. Run it with the
spektrafilm virtualenv's Python.
"""
import sys
from pathlib import Path

src, out_dir, *pairs = sys.argv[1:]
sys.path.insert(0, src)
from spektrafilm.utils.io import save_image_oiio  # noqa: E402
from spektrafilm.utils.raw_file_processor import load_and_process_raw_file  # noqa: E402

Path(out_dir).mkdir(parents=True, exist_ok=True)
for pair in pairs:
    name, raw = pair.split('=', 1)
    rgb = load_and_process_raw_file(raw, output_colorspace='ProPhoto RGB', output_cctf_encoding=False)
    out = Path(out_dir) / f'{name}.tiff'
    save_image_oiio(str(out), rgb, bit_depth=16, color_space='ProPhoto RGB', cctf_encoding=False)
    print(f'{out}: {rgb.shape[1]}x{rgb.shape[0]}')
