#!/usr/bin/env python3
"""Run only after builds and validation finish; never run batches concurrently."""
import pathlib,subprocess,sys
ROOT=pathlib.Path(__file__).resolve().parents[2]
for name in ['nikon24','leica60']:
    fixture=f'target/goose/fixtures/{name}'
    assert (ROOT/f'target/goose/full-validation-{name}.json').exists()
    for budget in [1,4]:
        cmd=[sys.executable,'scripts/goose/benchmark.py',fixture,'--workers',str(budget)]
        print('START',name,'grain budget',budget,flush=True);subprocess.run(cmd,cwd=ROOT,check=True)
# Supporting diagnostics use the same native implementations on a frozen photo.
for sigma in ['0.89','3.5']:
    subprocess.run([sys.executable,'scripts/goose/benchmark.py','target/goose/fixtures/nikon24','--mode','blur','--sigma',sigma],cwd=ROOT,check=True)
subprocess.run([sys.executable,'scripts/goose/benchmark.py','target/goose/fixtures/nikon24','--mode','samplers'],cwd=ROOT,check=True)
