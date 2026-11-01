"""Shared local accuracy harness helpers; never touches production/goldens."""
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
PIN = "28bf883e1672e884307edc75852549376e13644e"
TRACKS = ("application", "same-input", "deterministic")


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def tree(path):
    files = {p.relative_to(path).as_posix(): sha(p) for p in sorted(Path(path).rglob('*'))
             if p.is_file() and '__pycache__' not in p.parts and p.suffix != '.pyc'}
    return {'sha256': hashlib.sha256(json.dumps(files, sort_keys=True).encode()).hexdigest(), 'files': files}


def save(path, value):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    # Runtime parameters can legitimately contain Infinity (Langmuir knees).
    # Store those as strings in strict JSON rather than invalid JS literals.
    text = json.dumps(value, indent=2, default=str)
    value = json.loads(text, parse_constant=lambda s: s)
    temp = path.with_suffix(path.suffix + '.tmp')
    temp.write_text(json.dumps(value, indent=2, allow_nan=False) + '\n')
    temp.replace(path)


def load(path):
    return json.loads(Path(path).read_text())


def run(command, log, *, env=None, timeout=1200):
    save(str(log) + '.command.json', {'argv': list(map(str, command)), 'environment_overrides': env or {}})
    with Path(log).open('w') as stream:
        result = subprocess.run(list(map(str, command)), cwd=ROOT, env={**os.environ, **(env or {})},
                                stdout=stream, stderr=subprocess.STDOUT, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'exit {result.returncode}; see {log}')


def overrides(track):
    return ({'film_render.grain.active': False, 'film_render.glare.active': False,
             'print_render.glare.active': False} if track == 'deterministic' else {})


def apply_overrides(params, track):
    for path, value in overrides(track).items():
        group = params
        parts = path.split('.')
        for part in parts[:-1]:
            group = group[part]
        group[parts[-1]] = value


def fixture_path(out, case):
    # Both WB choices get independently decoded fixtures, shared by presets
    # with exactly the same RAW development configuration.
    return Path(out) / 'fixtures' / case['fixture_id']
