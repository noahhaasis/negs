"""Exercise Original between Render and Save using a real input, with a timeout.

Usage: python -I windows/check_original.py INPUT
Outputs and logs stay in a disposable directory; no photographs enter git.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('input', type=Path)
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    helper = root / 'target/release/spektrafilm-native.exe'
    catalog_run = subprocess.run([str(helper), '--data-dir', str(root / 'data')],
                                 input='{"id":1,"command":"catalog"}\n',
                                 text=True, capture_output=True, timeout=60, check=True)
    catalog = json.loads(catalog_run.stdout)['result']
    params = catalog['defaults']
    params['io']['upscale_factor'] = 0.1
    with tempfile.TemporaryDirectory(prefix='negs-original-') as folder:
        scratch = Path(folder)
        commands = [
            dict(id=1, command='render', input=str(args.input.resolve()),
                 film='kodak_gold_200', paper='kodak_endura_premier', params=params,
                 limit_preview=True, preview_path=str(scratch / 'render.rgba')),
            dict(id=2, command='save', render_id=1, output=str(scratch / 'before.png')),
            dict(id=3, command='original', input=str(args.input.resolve()), params=params,
                 limit_preview=True, preview_path=str(scratch / 'original.rgba')),
            dict(id=4, command='save', render_id=1, output=str(scratch / 'after.png')),
        ]
        run = subprocess.run([str(helper), '--data-dir', str(root / 'data')],
                             input=''.join(json.dumps(c) + '\n' for c in commands),
                             text=True, capture_output=True, timeout=180, check=True)
        replies = [json.loads(line) for line in run.stdout.splitlines()]
        assert len(replies) == 4, run.stderr
        for i, reply in enumerate(replies, 1):
            assert reply['id'] == i and 'error' not in reply, reply
        rendered, original = replies[0]['result'], replies[2]['result']
        assert (rendered['width'], rendered['height']) == (original['width'], original['height'])
        assert len((scratch / 'original.rgba').read_bytes()) == original['width'] * original['height'] * 4
        assert (scratch / 'before.png').read_bytes() == (scratch / 'after.png').read_bytes()
        print(json.dumps(dict(passed=True, matching_framing=True, retained_save_identical=True,
                              width=original['width'], height=original['height'])))


if __name__ == '__main__':
    main()
