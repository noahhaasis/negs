#!/usr/bin/env python3
"""Independent process per Python case, pinned imports, GUI-equivalent decode/render/save."""
import argparse
import dataclasses
import os
from pathlib import Path
import sys
import time

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import fixture_path, load, overrides, save, sha


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--run', type=Path, required=True)
    p.add_argument('--case', required=True)
    p.add_argument('--track', choices=['application', 'same-input', 'deterministic'], required=True)
    p.add_argument('--fixture-only', action='store_true', help='decode and verify the shared fixture; no film render')
    a = p.parse_args()
    config = load(a.run / 'config.json')
    case = next(c for c in config['cases'] if c['name'] == a.case)
    dest = (a.run / 'fixture-decodes' / case['fixture_id'] if a.fixture_only else
            a.run / 'renders' / a.track / a.case / 'python')
    dest.mkdir(parents=True, exist_ok=False)
    source = Path(config['python_source'])
    for var, folder in [('NUMBA_CACHE_DIR', 'numba'), ('MPLCONFIGDIR', 'mpl'), ('XDG_CACHE_HOME', 'cache')]:
        os.environ[var] = str(a.run / folder)
    os.environ['QT_QPA_PLATFORM'] = 'offscreen'
    sys.path.insert(0, str(source / 'src'))
    sys.path.insert(1, str(Path(__file__).resolve().parents[1] / 'parity'))
    result = {'engine': 'python', 'track': a.track, 'case': a.case, 'overrides': overrides(a.track),
              'backend': 'Python CPU / numpy-numba', 'status': 'error'}
    start = time.monotonic()
    try:
        assert sha(case['image']) == case['raw_sha256'], 'RAW changed'
        assert sha(case['preset']) == case['preset_sha256'], 'preset changed'
        result['adapter_sha256'] = sha(Path(__file__))
        import numpy as np
        import colour
        import OpenImageIO as oiio
        import spektrafilm
        from spektrafilm import digest_params
        from spektrafilm.runtime.pipeline import SimulationPipeline
        from spektrafilm_gui.persistence import gui_state_from_dict
        from spektrafilm_gui.params_mapper import build_params_from_state
        from spektrafilm.utils.raw_file_processor import load_and_process_raw_file, _read_exif_metadata
        from check_parity import effective_params
        assert Path(spektrafilm.__file__).resolve().is_relative_to(source), spektrafilm.__file__
        result['import'] = spektrafilm.__file__
        state = gui_state_from_dict(load(case['preset']))
        params = build_params_from_state(state)
        result['preset_params'] = effective_params(params)
        result['wb'] = dataclasses.asdict(state.gui_only.load_raw)
        result['camera'] = dataclasses.asdict(_read_exif_metadata(case['image']))
        fixture = fixture_path(a.run, case)
        if a.track == 'application':
            lens = {}
            # Exactly controller.load_raw_image: request the input space/curve
            # selected by the preset. This is a real colour conversion from ACES.
            image = load_and_process_raw_file(case['image'], **{
                'white_balance': state.gui_only.load_raw.white_balance,
                'temperature': state.gui_only.load_raw.temperature,
                'tint': state.gui_only.load_raw.tint,
                'lens_correction': state.gui_only.load_raw.lens_correction,
                'output_colorspace': params.io.input_color_space,
                'output_cctf_encoding': params.io.input_cctf_decoding,
                'lens_info_out': lens})
            result['lens'] = lens
            result['source_dimensions'] = [image.shape[1], image.shape[0]]
            result['quantization'] = 'Python GUI writer: clip to [0,1], multiply 255, truncate to uint8'
            result['decoder'] = 'rawpy / LibRaw (GUI load_and_process_raw_file)'
            result['input_colour'] = {'space': params.io.input_color_space, 'encoded': params.io.input_cctf_decoding}
            # Fixture creation never alters the application source. Explicitly
            # resize a second, linear sRGB copy for the isolated diagnostics.
            if not fixture.with_suffix('.json').exists():
                from scipy.ndimage import zoom
                linear = colour.RGB_to_RGB(image, params.io.input_color_space, 'sRGB',
                                           apply_cctf_decoding=params.io.input_cctf_decoding,
                                           apply_cctf_encoding=False)
                scale = min(1, config['diagnostic_side'] / max(linear.shape[:2]))
                small = zoom(linear, (scale, scale, 1), order=1, prefilter=False).astype('<f4')
                h, w, _ = small.shape
                fixture.parent.mkdir(parents=True, exist_ok=True)
                small.tofile(fixture.with_suffix('.f32'))
                buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, 'float'))
                buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), small)
                if not buf.write(str(fixture.with_suffix('.tiff'))):
                    raise RuntimeError(buf.geterror())
                check = oiio.ImageBuf(str(fixture.with_suffix('.tiff'))).get_pixels(oiio.FLOAT)
                if not np.array_equal(check, small):
                    raise RuntimeError('fixture TIFF transfer changed values')
                save(fixture.with_suffix('.json'), {'width': w, 'height': h, 'space': 'sRGB', 'encoded': False,
                     'dtype': '<f4', 'rgb_sha256': sha(fixture.with_suffix('.f32')),
                     'tiff_sha256': sha(fixture.with_suffix('.tiff')), 'wb': result['wb'],
                     'source_dimensions': result['source_dimensions'], 'orientation': 'upright from LibRaw',
                     'transformation': f'colour.RGB_to_RGB to linear sRGB; scipy.ndimage.zoom order=1 scale={scale}; float32',
                     'nonfinite': int(np.count_nonzero(~np.isfinite(small)))})
                del linear, small, check, buf
        else:
            meta = load(fixture.with_suffix('.json'))
            assert sha(fixture.with_suffix('.f32')) == meta['rgb_sha256']
            image = np.fromfile(fixture.with_suffix('.f32'), '<f4').reshape(meta['height'], meta['width'], 3)
            result['fixture'] = meta
            result['decoder'] = 'identical float fixture; no RAW decode'
            result['source_dimensions'] = [meta['width'], meta['height']]
            params.io.input_color_space = 'sRGB'
            params.io.input_cctf_decoding = False
            params.io.crop = False
            params.io.upscale_factor = 1.0
            if a.track == 'deterministic':
                params.film_render.grain.active = False
                params.film_render.glare.active = False
                params.print_render.glare.active = False
        if a.fixture_only:
            result.update(status='ok', fixture=load(fixture.with_suffix('.json')))
            return 0
        result['source_nonfinite'] = int(np.count_nonzero(~np.isfinite(image)))
        if result['source_nonfinite']:
            raise RuntimeError('nonfinite developed input')
        # Loaded user values, NOT stock-specific presets (check_presets.py).
        params = digest_params(params, apply_stocks_specifics=False)
        result['effective_params'] = effective_params(params)
        result['profile_hashes'] = {name: __import__('hashlib').sha256(
            np.asarray(getattr(params, name).data.density_curves, dtype='<f8').tobytes()).hexdigest()
            for name in ['film', 'print']}
        save(dest / 'params.json', result)
        output = np.asarray(SimulationPipeline(params).process(image.astype(np.float64), collect='rgb_out'))
        del image
        result['nonfinite'] = int(np.count_nonzero(~np.isfinite(output)))
        if result['nonfinite']:
            raise RuntimeError('nonfinite scan output')
        # Float scan is retained before save conversion, clipping or quantization.
        if a.track != 'application':
            output.astype('<f8').tofile(dest / 'scan.f64')
        result['float_range'] = [float(output.min()), float(output.max())]
        saving_space = state.simulation.workflow.saving_color_space
        saving_encoded = state.simulation.workflow.saving_cctf_encoding
        if (params.io.output_color_space, params.io.output_cctf_encoding) != (saving_space, saving_encoded):
            output = colour.RGB_to_RGB(output, params.io.output_color_space, saving_space,
                                      apply_cctf_decoding=params.io.output_cctf_encoding,
                                      apply_cctf_encoding=saving_encoded)
        if saving_space != 'sRGB' or not saving_encoded:
            raise RuntimeError('this RGB8 review requires encoded sRGB saving settings')
        # Same GUI writer, explicit uint8 export rather than a screenshot.
        from spektrafilm.utils.io import save_image_oiio
        save_image_oiio(str(dest / 'output.png'), output, bit_depth=8,
                        color_space=saving_space, cctf_encoding=saving_encoded)
        result.update(status='ok', width=output.shape[1], height=output.shape[0], output='output.png',
                      output_space=saving_space, output_encoded=saving_encoded)
    except Exception as exc:
        import traceback
        traceback.print_exc()
        result['error'] = f'{type(exc).__name__}: {exc}'
    finally:
        result['elapsed_seconds'] = time.monotonic() - start
        save(dest / 'result.json', result)
    return 0 if result['status'] == 'ok' else 2


if __name__ == '__main__':
    sys.exit(main())
