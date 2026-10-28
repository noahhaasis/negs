#!/usr/bin/env python3
"""Install the recorded official macOS arm64 release in target/goose only."""
import hashlib,json,pathlib,tarfile,urllib.request
ROOT=pathlib.Path(__file__).resolve().parents[2]
pin=json.loads((ROOT/'experimental/goose-grain/toolchain-pin.json').read_text())
tc=ROOT/'target/goose/toolchain';tc.mkdir(parents=True,exist_ok=True)
for name,url,sha in [('goose.tar.gz',f'https://github.com/aardappel/goose/releases/download/{pin["release"]}/{pin["compiler_artifact"]}',pin['compiler_sha256']),('source.tar.gz',f'https://github.com/aardappel/goose/archive/refs/tags/{pin["release"]}.tar.gz',pin['source_sha256'])]:
    p=tc/name
    if not p.exists():urllib.request.urlretrieve(url,p)
    assert hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()==sha
    with tarfile.open(p) as archive:archive.extractall(tc,filter='data')
print('Verified and extracted',pin['release'],pin['commit'])
