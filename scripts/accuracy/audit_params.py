#!/usr/bin/env python3
"""Audit recorded application imports against independent Python GUI mapping."""
import argparse
import os
from pathlib import Path
import sys

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import load, save, sha


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run', type=Path, required=True)
    a = parser.parse_args()
    config = load(a.run / 'config.json')
    os.environ['QT_QPA_PLATFORM'] = 'offscreen'
    os.environ['NUMBA_CACHE_DIR'] = str(a.run / 'numba')
    os.environ['MPLCONFIGDIR'] = str(a.run / 'mpl')
    os.environ['XDG_CACHE_HOME'] = str(a.run / 'cache')
    sys.path.insert(0, str(Path(config['python_source']) / 'src'))
    sys.path.insert(1, str(Path(__file__).resolve().parents[1] / 'parity'))
    import spektrafilm
    from spektrafilm_gui.persistence import gui_state_from_dict
    from spektrafilm_gui.params_mapper import build_params_from_state
    from check_presets import compare_values
    assert Path(spektrafilm.__file__).resolve().is_relative_to(Path(config['python_source']))
    report = {'method': 'check_presets.compare_values scope; f32-normalized photographic preset fields; input/runtime metadata excluded explicitly by that comparator',
              'source': spektrafilm.__file__, 'adapter_sha256': sha(Path(__file__)), 'cases': []}
    for case in config['cases']:
        state = gui_state_from_dict(load(case['preset']))
        params = build_params_from_state(state)
        result = {'case': case['name'], 'engines': {}}
        for engine in ['web', 'installed-gpu', 'source-gpu']:
            dest = a.run / 'renders/application' / case['name'] / engine
            try:
                imported = load(dest / 'result.json')['state'] if engine == 'web' else load(dest / 'import.json')['result']
                differences = compare_values(params, state, imported['params'], imported)
                result['engines'][engine] = {'status': 'compared', 'differences': differences, 'equal_in_scope': not differences}
            except Exception as exc:
                result['engines'][engine] = {'status': 'error', 'error': str(exc), 'equal_in_scope': False}
        report['cases'].append(result)
    save(a.run / 'parameter-audit.json', report)
    print('Parameter import checks:', sum(e['equal_in_scope'] for c in report['cases'] for e in c['engines'].values()),
          '/', len(report['cases'])*3)


if __name__ == '__main__':
    main()
