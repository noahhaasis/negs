#!/usr/bin/env python3
"""Check actual bundle ownership, conflict prevention and non-destructive removal."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--bundle', type=Path, default=root / 'target/linux-package/bundle')
args = parser.parse_args()
out = root / 'target/linux-validation'
out.mkdir(exist_ok=True)
checks = []
with tempfile.TemporaryDirectory(prefix='install-check-', dir=out) as folder:
    work = Path(folder)
    prefix = work / 'prefix'
    config = work / 'config'
    (config / 'hypr').mkdir(parents=True)
    original = '-- isolated test configuration\n'
    (config / 'hypr/hyprland.lua').write_text(original)
    settings = config / 'negs/session.json'
    settings.parent.mkdir()
    settings.write_text('{"preserve":true}')
    env = dict(os.environ, XDG_CONFIG_HOME=str(config))
    command = ['python3', str(args.bundle / 'share/negs/install.py'), '--prefix', str(prefix), '--no-reload']
    def install():
        return subprocess.run(command + ['--from', str(args.bundle)], env=env, capture_output=True, text=True)
    first = install()
    assert first.returncode == 0, first.stderr
    manifest = prefix / 'share/negs/install-manifest.json'
    before = manifest.read_bytes()
    assert install().returncode == 0
    assert manifest.read_bytes() == before
    assert (config / 'hypr/hyprland.lua').read_text().count('require("hypr.negs-window-rules")') == 1
    assert any(p.read_text() == original for p in (config / 'hypr').glob('hyprland.lua.bak.negs-*'))
    checks.append('Real bundle installs idempotently; Omarchy include is unique and original config is backed up')
    launcher = prefix / 'bin/negs'
    saved = launcher.read_bytes()
    launcher.write_text('modified by user')
    refused = install()
    assert refused.returncode != 0 and 'modified file' in refused.stderr
    assert launcher.read_text() == 'modified by user'
    launcher.write_bytes(saved)
    checks.append('Installer refuses to overwrite a modified owned file before changing the installation')
    readme = prefix / 'share/doc/negs/linux-app.md'
    readme.write_text('keep this modification')
    foreign = prefix / 'share/negs/foreign-file'
    foreign.write_text('keep foreign file')
    removed = subprocess.run(command + ['--uninstall'], env=env, capture_output=True, text=True)
    assert removed.returncode == 0, removed.stderr
    assert not launcher.exists() and not manifest.exists()
    assert readme.read_text() == 'keep this modification'
    assert foreign.read_text() == 'keep foreign file'
    assert settings.read_text() == '{"preserve":true}'
    checks.append('Uninstall removes owned files and preserves modified/unowned files and settings')
with tarfile.open(args.bundle / 'share/negs/source.tar.gz') as source:
    names = source.getnames()
    assert not any('/target/' in n or '/.git/' in n or '/docs/bw-expansion-review/' in n or n.endswith('/docs/linux-agent-handoff.md') for n in names)
    assert any(n.endswith('/linux/src/runtime.rs') for n in names)
checks.append('Packaged source contains the connected editor and excludes private validation assets/handoff')
report = dict(status='PASS', checks=checks)
(out / 'package-install.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
