#!/usr/bin/env python3
"""Fresh local RAW/preset comparisons. See README.md. Never reuses an export."""
import argparse
import hashlib
import importlib.metadata
import os
from pathlib import Path
import platform
import re
import subprocess
import sys
import tarfile
import time
import uuid

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import ROOT, PIN, TRACKS, apply_overrides, fixture_path, load, overrides, run, save, sha, tree

DEFAULT_ENGINES = ['web', 'python', 'installed-gpu', 'installed-cpu', 'source-gpu', 'source-cpu']


def prepare(a):
    out = a.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=False)
    source = a.python_source.expanduser().resolve()
    source_commit = subprocess.check_output(['git', '-C', str(source), 'rev-parse', a.python_commit], text=True).strip()
    if source_commit != PIN:
        raise ValueError(f'expected experimental {PIN}, got {source_commit}')
    archived = out / 'python-experimental'
    archived.mkdir()
    with (out / 'python.tar').open('wb') as stream:
        subprocess.run(['git', '-C', str(source), 'archive', source_commit], stdout=stream, check=True)
    with tarfile.open(out / 'python.tar') as archive:
        archive.extractall(archived, filter='data')
    cases = []
    presets = sorted(a.preset_dir.expanduser().resolve().glob('*.json'))
    if len(presets) != 9:
        raise ValueError(f'expected nine presets, got {len(presets)}')
    images = a.input or [
        f'DSC_2498={Path.home()}/Downloads/tag @signatureeditsco - free raws from signatureedits.com -DSC_2498.dng',
        f'IMG_0316={Path.home()}/Downloads/tag @signatureeditsco - free raws from signatureedits.com -IMG_0316.CR2']
    token = 'acc-' + uuid.uuid4().hex[:12]
    historical_path = ROOT / 'target/preset-accuracy/cases.json'
    historical = {c['name']: c for c in load(historical_path)} if historical_path.exists() else {}
    for item in images:
        label, path = item.split('=', 1)
        if not re.fullmatch(r'[A-Za-z0-9_-]+', label):
            raise ValueError('unsafe input label')
        path = Path(path).expanduser().resolve()
        raw_sha = sha(path)
        url = f'results/{token}-{label}{path.suffix}'
        (ROOT / 'web' / url).symlink_to(path)
        for index, preset in enumerate(presets, 1):
            text = load(preset)
            wb = text.get('load_raw', {})
            key = hashlib.sha256((raw_sha + str(sorted(wb.items()))).encode()).hexdigest()[:16]
            case = {'name': f'{label}-{index:02}', 'image_id': label, 'image': str(path), 'url': url,
                    'preset': str(preset), 'label': preset.stem, 'raw_sha256': raw_sha,
                    'preset_sha256': sha(preset), 'wb': wb, 'fixture_id': key}
            old = historical.get(case['name'])
            if old and (case['raw_sha256'], case['preset_sha256']) != (old['raw_sha256'], old['preset_sha256']):
                raise ValueError(f'input changed from supplied baseline: {case["name"]}')
            cases.append(case)
    if len(cases) != 18 or len({c['name'] for c in cases}) != 18:
        raise ValueError('expected 18 unique RAW/preset cases')
    installed = a.installed_app.expanduser().resolve()
    source_tools = a.native_dir.expanduser().resolve()
    # Verify preserved identities BEFORE invoking/building anything.
    manifest = source_tools / 'manifest.json'
    if manifest.exists():
        for rel, expected in load(manifest).items():
            if sha(source_tools / rel) != expected:
                raise ValueError(f'preserved source tool/data changed: {rel}')
    app_manifest = installed.parents[1] / 'manifest.json'
    if app_manifest.exists():
        expected = load(app_manifest)['system']
        for name, digest in expected['executables'].items():
            if sha(installed / 'Contents/MacOS' / name) != digest:
                raise ValueError(f'preserved installed executable changed: {name}')
    import plistlib
    app_info = plistlib.loads((installed / 'Contents/Info.plist').read_bytes())
    engines = {'web': {'label': 'Web / current working tree / Chrome WebGPU', 'decoder': 'Rawler'},
               'python': {'label': f'Python experimental {PIN[:7]} / CPU', 'decoder': 'rawpy / LibRaw'}}
    identities = {}
    for family, directory, data, label in [
            ('installed', installed / 'Contents/MacOS', installed / 'Contents/Resources/data',
             f'Installed Negs {app_info["CFBundleShortVersionString"]} build {app_info["CFBundleVersion"]}'),
            ('source', source_tools, source_tools / 'data', 'Preserved source build cd3bc2b')]:
        identities[family] = {'data': str(data), 'data_identity': tree(data), 'binaries': {}}
        for mode, filename in [('gpu', 'spektrafilm'), ('cpu', 'spektrafilm-f64')]:
            engine = f'{family}-{mode}'
            binary = directory / filename
            help_text = subprocess.check_output([str(binary), 'process', '--help'], text=True)
            (out / f'{engine}-help.txt').write_text(help_text)
            identities[family]['binaries'][filename] = sha(binary)
            engines[engine] = {'label': f'{label} / {"Fast GPU f32" if mode == "gpu" else "Reference CPU f64"}',
                               'binary': str(binary), 'helper': str(directory / 'spektrafilm-native'),
                               'data': str(data), 'backend': 'wgpu' if mode == 'gpu' else 'cpu',
                               'decoder': 'Apple RAW (verify export log)'}
        identities[family]['binaries']['spektrafilm-native'] = sha(directory / 'spektrafilm-native')
    config = {'schema': 1, 'run_id': out.name, 'token': token, 'cases': cases, 'engines': engines,
              'selected_engines': a.engines, 'tracks': a.tracks, 'python_source': str(archived),
              'python_executable': sys.executable, 'server': a.server.rstrip('/'),
              'diagnostic_side': a.diagnostic_side, 'timeout': a.timeout,
              'policy': 'new outputs only; no export cache; exact RGB8 equality required; no production overrides'}
    save(out / 'config.json', config)
    patch = subprocess.check_output(['git', 'diff', '--binary', 'HEAD'], cwd=ROOT)
    (out / 'dirty.patch').write_bytes(patch)
    dependencies = {}
    for name in ['numpy', 'scipy', 'colour-science', 'numba', 'OpenImageIO', 'rawpy', 'PySide6', 'lensfunpy', 'exiv2']:
        try:
            dependencies[name] = importlib.metadata.version(name)
        except importlib.metadata.PackageNotFoundError:
            dependencies[name] = 'not installed'
    assets = {str(path.relative_to(ROOT)): sha(path) for folder in ['web/pkg', 'web/pkg-threads']
              for path in (ROOT / folder).rglob('*') if path.is_file()}
    provenance = {'created_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                  'argv': sys.argv, 'git_commit': subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip(),
                  'git_status': subprocess.check_output(['git', 'status', '--short'], cwd=ROOT, text=True),
                  'dirty_patch_sha256': hashlib.sha256(patch).hexdigest(),
                  'python_commit': source_commit, 'python_archive_sha256': sha(out / 'python.tar'),
                  'python_tree': tree(archived / 'src'), 'native': identities, 'wasm_assets': assets,
                  'harness': tree(Path(__file__).parent), 'production_crates': tree(ROOT / 'crates'),
                  'web_js': {p.name: sha(p) for p in (ROOT / 'web').glob('*.js')},
                  'dependencies': dependencies, 'platform': platform.platform(), 'python_version': sys.version,
                  'node_version': subprocess.check_output(['node', '--version'], text=True).strip()}
    save(out / 'provenance.json', provenance)
    print('Prepared', out, flush=True)
    return out


def native(out, config, case, track, engine):
    dest = out / 'renders' / track / case['name'] / engine
    dest.mkdir(parents=True, exist_ok=False)
    spec = config['engines'][engine]
    result = {'status': 'error', 'engine': engine, 'case': case['name'], 'track': track,
              'overrides': overrides(track), 'decoder': spec['decoder'], 'wb': case['wb']}
    start = time.monotonic()
    try:
        assert sha(case['image']) == case['raw_sha256'], 'RAW changed'
        assert sha(case['preset']) == case['preset_sha256'], 'preset changed'
        # Each version's own importer, with a new helper/no carry-over edit.
        request = {'id': 1, 'command': 'preset_import', 'text': Path(case['preset']).read_text()}
        response = subprocess.run([spec['helper'], '--data-dir', spec['data']], input=__import__('json').dumps(request) + '\n',
                                  text=True, capture_output=True, timeout=60, cwd=ROOT)
        (dest / 'import.log').write_text(response.stderr)
        imported = __import__('json').loads(response.stdout.splitlines()[0])
        save(dest / 'import.json', imported)
        if response.returncode or imported.get('error'):
            raise RuntimeError(f'import failed: {imported}')
        state = imported['result']
        params = state['params']
        image = case['image']
        if track != 'application':
            fixture = fixture_path(out, case)
            meta = load(fixture.with_suffix('.json'))
            assert sha(fixture.with_suffix('.tiff')) == meta['tiff_sha256']
            result['fixture'] = meta
            image = str(fixture.with_suffix('.tiff'))
            params['io'].update(input_interpretation={'mode': 'manual'}, input_color_space='sRGB',
                                input_cctf_decoding=False, photo_long_side_px=None, upscale_factor=1.0)
            result['decoder'] = 'identical float TIFF / manual linear sRGB'
            apply_overrides(params, track)
        save(dest / 'params.json', params)
        result['effective_params'] = params
        command = [spec['binary'], 'process', image, '--output', dest / 'output.png', '--data-dir', spec['data'],
                   '--film', state['film'], '--paper', state['paper'], '--params', dest / 'params.json',
                   '--bit-depth', '8', '--no-metadata', '--timings']
        if params['io']['scan_film']:
            command += ['--scan-film']
        if track != 'application':
            command += ['--input-colour', 'manual', '--raw-out', dest / 'scan.f64']
        run(command, dest / 'export.log', env={'SPEKTRAFILM_BACKEND': spec['backend']}, timeout=config['timeout'])
        log = (dest / 'export.log').read_text()
        backend = re.search(r'Backend: (.+)', log)
        if not backend or not backend[1].startswith('wgpu' if spec['backend'] == 'wgpu' else 'CPU'):
            raise RuntimeError('requested backend not used')
        if 'using simplified path' in log or 'falling back' in log.lower():
            raise RuntimeError('pipeline fallback')
        loaded = re.search(r'Image loaded: (\d+)x(\d+)', log)
        result['source_dimensions'] = list(map(int, loaded.groups())) if loaded else None
        result['input_colour_log'] = re.findall(r'Input colour: (.+)', log)
        if track == 'application':
            result['effective_input_colour'] = {'input_color_space': 'sRGB', 'input_cctf_decoding': False,
                                                'reason': 'CLI io.apply_input_colour for scene-linear RAW'}
            result['float_nonfinite_scan'] = 'not captured; application export is RGB8 only'
        result.update(status='ok', output='output.png', backend=backend[1], output_space='sRGB', output_encoded=True)
        if track != 'application':
            import numpy as np
            scan = np.fromfile(dest / 'scan.f64', '=f8')
            result['nonfinite'] = int(np.count_nonzero(~np.isfinite(scan)))
            if result['nonfinite']:
                raise RuntimeError('nonfinite float scan')
    except Exception as exc:
        result.update(status='error', error=f'{type(exc).__name__}: {exc}')
    result['elapsed_seconds'] = time.monotonic() - start
    save(dest / 'result.json', result)
    return result


def render(out, engines=None, tracks=None):
    config = load(out / 'config.json')
    # Sidecar per invocation; never relabel old pixels under a newer harness.
    identity = out / 'executions' / (time.strftime('%Y%m%dT%H%M%S') + '-' + uuid.uuid4().hex[:8] + '.json')
    save(identity, {'argv': sys.argv, 'harness': tree(Path(__file__).parent),
                    'prepared_harness': load(out / 'provenance.json')['harness']['sha256']})
    engines = engines or config['selected_engines']
    tracks = tracks or config['tracks']
    for track in tracks:
        for engine in engines:
            if engine == 'web':
                log = out / f'browser-{track}.log'
                run(['node', ROOT / 'web/cdp.mjs', config['server'] + '/?engine=gpu',
                     ROOT / 'scripts/accuracy/browser.mjs', '3000'], log,
                    env={'ACCURACY_RUN': str(out), 'ACCURACY_TRACK': track}, timeout=config['timeout'] * 18)
                continue
            for case in config['cases']:
                print('START', track, engine, case['name'], flush=True)
                if engine == 'python':
                    dest = out / 'renders' / track / case['name'] / engine
                    if dest.exists():
                        raise FileExistsError(f'refusing to overwrite {dest}')
                    log = out / f'python-{track}-{case["name"]}.log'
                    try:
                        run([sys.executable, '-I', Path(__file__).with_name('python_adapter.py'), '--run', out,
                             '--case', case['name'], '--track', track], log, timeout=config['timeout'])
                    except Exception as exc:
                        if not (dest / 'result.json').exists():
                            save(dest / 'result.json', {'status': 'error', 'engine': engine, 'track': track,
                                 'case': case['name'], 'error': str(exc)})
                    result = load(dest / 'result.json')
                else:
                    result = native(out, config, case, track, engine)
                print('DONE', track, engine, case['name'], result['status'], result.get('error', ''), flush=True)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('action', choices=['prepare', 'render', 'fixtures', 'all'])
    p.add_argument('--output-dir', type=Path, required=True)
    p.add_argument('--python-source', type=Path, default=Path.home() / 'Projects/spektrafilm')
    p.add_argument('--python-commit', default=PIN)
    p.add_argument('--preset-dir', type=Path, default=Path.home() / 'Downloads/Presets')
    p.add_argument('--input', action='append', help='label=RAW path (twice)')
    p.add_argument('--installed-app', type=Path, default=ROOT / 'target/preset-accuracy/macos-apps/system/Negs.app')
    p.add_argument('--native-dir', type=Path, default=ROOT / 'target/preset-accuracy/native-binaries')
    p.add_argument('--engines', nargs='+', choices=DEFAULT_ENGINES)
    p.add_argument('--tracks', nargs='+', choices=TRACKS)
    p.add_argument('--server', default='http://localhost:8137')
    p.add_argument('--diagnostic-side', type=int, default=768)
    p.add_argument('--timeout', type=int, default=1200)
    a = p.parse_args()
    if not 16 <= a.diagnostic_side <= 65535 or a.timeout <= 0:
        p.error('diagnostic side must be 16..65535 and timeout must be positive')
    if a.action in ['prepare', 'all']:
        a.engines = a.engines or DEFAULT_ENGINES
        a.tracks = a.tracks or list(TRACKS)
        prepare(a)
    if a.action == 'fixtures':
        out = a.output_dir.resolve()
        config = load(out / 'config.json')
        seen = set()
        for case in config['cases']:
            if case['fixture_id'] in seen:
                continue
            seen.add(case['fixture_id'])
            run([sys.executable, '-I', Path(__file__).with_name('python_adapter.py'), '--run', out,
                 '--case', case['name'], '--track', 'application', '--fixture-only'],
                out / f'fixture-{case["fixture_id"]}.log', timeout=a.timeout)
    if a.action in ['render', 'all']:
        render(a.output_dir.resolve(), a.engines, a.tracks)


if __name__ == '__main__':
    main()
