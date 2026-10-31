#!/usr/bin/env python3
"""Finalize evidence identities after renders/review. Refuses an incomplete matrix."""
import argparse
from pathlib import Path
import sys
import time
import shutil

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
from common import load, save, sha, tree


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--run', type=Path, required=True)
    p.add_argument('--review', type=Path, required=True)
    a = p.parse_args()
    out = a.run.resolve()
    config, provenance = load(out / 'config.json'), load(out / 'provenance.json')
    if (out / 'frozen.json').exists():
        raise FileExistsError('run already frozen')
    results = []
    for track in config['tracks']:
        for case in config['cases']:
            assert sha(case['image']) == case['raw_sha256'], f'RAW changed: {case["name"]}'
            assert sha(case['preset']) == case['preset_sha256'], f'preset changed: {case["name"]}'
            for engine in config['selected_engines']:
                dest = out / 'renders' / track / case['name'] / engine
                result = load(dest / 'result.json')  # missing is fatal
                results.append({'track': track, 'case': case['name'], 'engine': engine, 'status': result['status']})
                if result['status'] == 'ok':
                    assert (dest / result['output']).is_file(), 'missing output'
    assert tree(Path(config['python_source']) / 'src')['sha256'] == provenance['python_tree']['sha256'], 'pinned Python source changed'
    from common import ROOT
    for rel, expected in provenance['wasm_assets'].items():
        assert sha(ROOT / rel) == expected, f'WASM changed during run: {rel}'
    for family, spec in provenance['native'].items():
        assert tree(Path(spec['data']))['sha256'] == spec['data_identity']['sha256'], f'{family} data changed'
        for engine in config['engines'].values():
            if engine.get('data') != spec['data']:
                continue
            binary = Path(engine['binary'])
            assert sha(binary) == spec['binaries'][binary.name], f'{family} binary changed'
            helper = Path(engine['helper'])
            assert sha(helper) == spec['binaries'][helper.name], f'{family} helper changed'
    review = a.review.resolve()
    report = load(review / 'report.json')
    assert len(report['cases']) == len(config['cases'])*len(config['tracks']), 'partial review'
    assert 'summary' in report, 'review not complete'
    # Final presentation-only assets/links, before the evidence manifest is
    # frozen. Never alter captures or metric values during finalization.
    for filename in ['index.html', 'site.js', 'site.css']:
        shutil.copy2(Path(__file__).with_name(filename), review / filename)
    if (out / 'parameter-audit.json').exists():
        shutil.copy2(out / 'parameter-audit.json', review / 'parameter-audit.json')
    (review / 'fixtures').mkdir(exist_ok=True)
    for fixture in (out / 'fixtures').glob('*'):
        link = review / 'fixtures' / fixture.name
        if fixture.is_file() and not link.exists():
            link.symlink_to(fixture)
    # Include names and hashes for all artifacts, excluding ephemeral caches,
    # launch logs (may still be flushed), and symlinks already covered by renders.
    files = {}
    for path in sorted(out.rglob('*')):
        rel = path.relative_to(out)
        if any(part in ('numba', 'numba-metrics', 'mpl', 'cache', 'python-experimental', '__pycache__') for part in rel.parts):
            continue
        if len(rel.parts) == 1 and path.suffix == '.log':
            continue  # launcher/server logs can append after finalization
        if path.is_file() and not path.is_symlink() and not path.name.endswith('.tmp'):
            files[rel.as_posix()] = sha(path)
    save(out / 'frozen.json', {'frozen_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
         'policy': 'append new runs; never overwrite these captures or change reference goldens',
         'results': results, 'report_summary': report['summary'], 'files': files,
         'final_harness': tree(Path(__file__).parent), 'initial_harness': provenance['harness']['sha256']})
    print('Frozen:', out / 'frozen.json')


if __name__ == '__main__':
    main()
