#!/usr/bin/env python3
import json,pathlib,subprocess,sys
import numpy as np
from fixtures import verify,digest
ROOT=pathlib.Path(__file__).resolve().parents[2];T=ROOT/'target/goose'
def main():
    rows=[]
    for name in sys.argv[1:]:
        fixture=T/'fixtures'/name;verify(fixture);reference=None
        for workers in [1,4]:
            for lang,exe in [('rust',ROOT/'target/release/goose-grain-experiment'),('goose',T/'goose-bench')]:
                print('BEGIN',name,lang,workers,flush=True)
                out=T/'full-validation.f64le';r=subprocess.run([str(exe),'grain',str(fixture),str(workers),'1',str(out)],capture_output=True,text=True,check=True)
                h=digest(out)
                values=np.memmap(out,dtype='<f8',mode='r');assert np.isfinite(values).all();del values
                if reference is None:reference=h
                assert h==reference, f'{name} {lang} {workers}: full output differs'
                rows.append({'fixture':name,'language':lang,'budget':workers,'output_sha256':h,'native':json.loads(r.stdout)})
                out.unlink();print(name,lang,workers,'bit-identical',flush=True)
        (T/('full-validation-'+name+'.json')).write_text(json.dumps([r for r in rows if r['fixture']==name],indent=2)+'\n')
if __name__=='__main__':main()
