#!/usr/bin/env python3
"""Compare the shared RAW loader's Apple/rawler backends on local files.

Build: cargo build --release -p spektrafilm-io --example raw_decoder_probe
Run: python3 scripts/parity/compare_raw_decoders.py --output-dir /tmp/raw-study \
       --input 'z_f_he_star=/path/to/photo.NEF' --input 'z_6=/path/to/other.NEF'

No private absolute paths or photographic pixels are written into results.json.
--artifacts additionally saves private linear arrays/previews in output-dir.
First-call timings mean a fresh process, NOT a cold OS disk/shader cache.
"""
import argparse
import hashlib
import json
import platform
from pathlib import Path
import re
import statistics
import subprocess


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--probe', type=Path, default=Path('target/release/examples/raw_decoder_probe'))
    parser.add_argument('--input', action='append', required=True, help='label=absolute-file-path')
    parser.add_argument('--output-dir', type=Path, required=True)
    parser.add_argument('--iterations', type=int, default=6)
    parser.add_argument('--artifacts', action='store_true')
    args = parser.parse_args()
    if not 2 <= args.iterations <= 100:
        parser.error('iterations must be 2..=100')
    out = args.output_dir.resolve()
    out.mkdir(parents=True, exist_ok=True)
    probe = args.probe.resolve()
    report = {'os': platform.platform(), 'machine': platform.machine(),
              'iterations': args.iterations, 'cases': [],
              'method': 'Sequential fresh-process calls; first iteration reported separately; median of remaining calls. OS caches are not purged. No writing/film rendering in decode_ms. RSS includes probe statistics/conversion buffers; excludes GPU allocations not resident in process.'}
    if platform.system() == 'Darwin':
        report['cpu'] = subprocess.check_output(['sysctl', '-n', 'machdep.cpu.brand_string'], text=True).strip()
    for item in args.input:
        label, filename = item.split('=', 1)
        if not re.fullmatch(r'[a-zA-Z0-9_-]+', label):
            parser.error('labels must contain only letters, digits, underscores and hyphens')
        source = Path(filename).resolve()
        case = {'label': label, 'filename': source.name,
                'sha256': hashlib.file_digest(source.open('rb'), 'sha256').hexdigest(),
                'bytes': source.stat().st_size, 'backends': {}}
        for backend in ['apple', 'rawler']:
            command = [str(probe), '--decoder', backend, '--iterations', str(args.iterations), str(source)]
            if platform.system() == 'Darwin':
                command = ['/usr/bin/time', '-l'] + command
            run = subprocess.run(command, capture_output=True, text=True, timeout=300)
            (out / f'{label}-{backend}.stderr.log').write_text(run.stderr)
            try:
                result = json.loads(run.stdout.strip().splitlines()[-1])
            except (IndexError, json.JSONDecodeError):
                raise RuntimeError(f'{label}/{backend}: invalid probe output: {run.stdout[-1000:]} {run.stderr[-1000:]}')
            result['exit_code'] = run.returncode
            match = re.search(r'(\d+)\s+maximum resident set size', run.stderr)
            if match:
                result['peak_rss_mib'] = int(match.group(1)) / 1024**2
            if result.get('success'):
                times = result['decode_ms']
                result['first_ms'] = times[0]
                result['warm_median_ms'] = statistics.median(times[1:])
                result['warm_min_ms'] = min(times[1:])
                result['warm_max_ms'] = max(times[1:])
                if args.artifacts:
                    subprocess.run([str(probe), '--decoder', backend, '--output', str(out/label/backend), str(source)],
                                   capture_output=True, check=True, text=True, timeout=300)
            case['backends'][backend] = result
            print(label, backend, 'median_ms=', result.get('warm_median_ms'), 'success=', result.get('success'), flush=True)
        report['cases'].append(case)
        (out/'results.json').write_text(json.dumps(report, indent=2))
    print('Results:', out/'results.json')


if __name__ == '__main__':
    main()
