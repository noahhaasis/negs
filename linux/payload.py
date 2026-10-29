"""Shared payload identity and per-user OpenBLAS bundling."""
import hashlib
import json
from pathlib import Path
import shutil


def bundle_openblas(prefix, source):
    source = Path(source)
    files = [
        (source / 'lib/libopenblas.so.0', 'lib/negs/libopenblas.so.0'),
        (source / 'share/licenses/openblas/LICENSE', 'share/licenses/negs/OpenBLAS-LICENSE'),
    ]
    for library, _ in files:
        if not library.is_file():
            raise ValueError(f'Cannot create a per-user bundle without {library}')
    for library, relative in files:
        destination = prefix / relative
        destination.parent.mkdir(parents=True, exist_ok=True)
        # Dereference the system symlink: its target is outside the bundle.
        shutil.copy2(library, destination)


def write_identity(prefix, version, format):
    digest = hashlib.sha256()
    for path in sorted([*(prefix / 'lib/negs').iterdir(), *(prefix / 'share/negs/data').rglob('*')]):
        if path.is_file():
            digest.update(str(path.relative_to(prefix)).encode() + b'\0')
            with path.open('rb') as stream:
                digest.update(hashlib.file_digest(stream, 'sha256').digest())
    (prefix / 'share/negs/payload.json').write_text(json.dumps(
        {'version': version, 'format': format, 'digest': digest.hexdigest()}, indent=2) + '\n')
