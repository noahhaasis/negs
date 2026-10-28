#!/usr/bin/env python3
"""Build the pinned native experiment. Run from the repository root."""
import hashlib,json,os,pathlib,subprocess
ROOT=pathlib.Path(__file__).resolve().parents[2]
os.chdir(ROOT)
target=ROOT/'target/goose';tc=target/'toolchain';goose=tc/'goose-v2026.3-macos-arm64/goose'
env=dict(os.environ,GOOSE_HOME=str(target/'cache'),CARGO_TARGET_DIR=str(ROOT/'target'))
def run(a): subprocess.run(list(map(str,a)),env=env,check=True)
run(['cargo','build','--release','--locked','--manifest-path','experimental/goose-grain/Cargo.toml'])
with (target/'goose-build.log').open('w') as log:
    subprocess.run([str(goose),'-O2','--standalone','--stacks','-o',str(target/'bench.c'),'experimental/goose-grain/bench.goose'],env=env,stdout=log,stderr=log,check=True)
run(['clang','-O3','-ffp-contract=off',target/'bench.c','experimental/goose-grain/support.c','target/release/libgoose_grain_experiment.a','-o',target/'goose-bench','-lm','-lpthread'])
files=['goose.tar.gz','source.tar.gz']
recorded=json.loads((ROOT/'experimental/goose-grain/toolchain-pin.json').read_text())
for file,key in [('goose.tar.gz','compiler_sha256'),('source.tar.gz','source_sha256')]:
    assert hashlib.sha256((tc/file).read_bytes()).hexdigest()==recorded[key]
pin={'release':recorded['release'],'release_commit':recorded['commit'],'runtime':subprocess.check_output([goose,'version'],env=env,text=True).strip(),'artifacts':{f:hashlib.sha256((tc/f).read_bytes()).hexdigest() for f in files},'rustc':subprocess.check_output(['rustc','--version'],text=True).strip(),'clang':subprocess.check_output(['clang','--version'],text=True).splitlines()[0],'rust_policy':'opt-level=3, thin LTO, codegen-units=1, default target CPU, no fast math','goose_policy':'Goose -O2 inlining, standalone generated C, clang -O3 -ffp-contract=off, default target CPU','binary_sha256':{str(p.relative_to(ROOT)):hashlib.sha256(p.read_bytes()).hexdigest() for p in [ROOT/'target/release/goose-grain-experiment',target/'goose-bench']}}
(target/'build.json').write_text(json.dumps(pin,indent=2)+'\n')
print('Built both native drivers; provenance in target/goose/build.json')
