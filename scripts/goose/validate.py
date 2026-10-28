#!/usr/bin/env python3
"""Differential native validation. No performance claims from this run."""
import json,pathlib,subprocess,hashlib,shutil
import numpy as np
from fixtures import verify,digest
ROOT=pathlib.Path(__file__).resolve().parents[2];T=ROOT/'target/goose'
EXES={'rust':ROOT/'target/release/goose-grain-experiment','goose':T/'goose-bench'}
def run(lang,args):return subprocess.run([str(EXES[lang]),*map(str,args)],capture_output=True,text=True,check=True,timeout=120)
def compare(a,b):
    r=np.fromfile(a,'<f8');g=np.fromfile(b,'<f8');assert r.shape==g.shape
    equal=(r==g)|(np.isnan(r)&np.isnan(g));finite=np.isfinite(r)&np.isfinite(g)
    error=np.abs(r[finite]-g[finite]);bits=int(np.count_nonzero(r.view('u8')!=g.view('u8')))
    result={'values':r.size,'bit_differences':bits,'max_abs':float(error.max(initial=0)),'nonfinite':int(np.count_nonzero(~finite)),'rust_sha256':digest(a),'goose_sha256':digest(b)}
    assert bits==0,result
    return result
def main():
    rows=[]
    for lang in EXES:run(lang,['probes',T/(lang+'-probes.bin')])
    rows.append({'case':'samplers-and-next-draws',**compare(T/'rust-probes.bin',T/'goose-probes.bin')})
    for fixture in sorted((T/'fixtures/tiny').iterdir()):
        print(fixture.name,flush=True)
        verify(fixture)
        ref=T/'rust-validation.bin';out=T/'goose-validation.bin'
        run('rust',['grain',fixture,1,1,ref]);
        for lang,workers in [('goose',1),('goose',4),('rust',4)]:
            run(lang,['grain',fixture,workers,1,out]);rows.append({'case':fixture.name,'language':lang,'workers':workers,**compare(ref,out)})
        run('rust',['grain',fixture,4,1,out,0,'serial']);rows.append({'case':fixture.name,'language':'rust','workers':4,'serial_channels':True,**compare(ref,out)})
        run('goose',['grain',fixture,4,1,out,0,'serial']);rows.append({'case':fixture.name,'language':'goose','workers':4,'serial_channels':True,**compare(ref,out)})
    for fixture in sorted((T/'fixtures/tiny').iterdir()):
        if '-l3-p0-m0' not in fixture.name:continue
        for sigma in [0.,.01,.2,.89,2.999999,3.,3.000001,12.]:
            for lang in EXES:run(lang,['blur',fixture,1,1,T/(lang+'-blur.bin'),sigma])
            rows.append({'case':'blur-'+fixture.name,'sigma':sigma,**compare(T/'rust-blur.bin',T/'goose-blur.bin')})
    from fixtures import blur_cases
    blur_cases(T/'fixtures/blur')
    for fixture in sorted((T/'fixtures/blur').iterdir()):
        for sigma in [0.,.01,.2,.89,2.999999,3.,3.000001,12.]:
            for lang in EXES:run(lang,['blur',fixture,1,1,T/(lang+'-blur.bin'),sigma])
            rows.append({'case':'pattern-blur-'+fixture.name,'sigma':sigma,**compare(T/'rust-blur.bin',T/'goose-blur.bin')})
    # Both loaders reject truncated, trailing, corrupt and dimension-mismatched input.
    src=next((T/'fixtures/tiny').iterdir());bad=T/'fixtures/malformed';bad.mkdir(exist_ok=True)
    for p in src.iterdir():shutil.copyfile(p,bad/p.name)
    orig=(bad/'density.f64le').read_bytes();meta=(bad/'metadata.json').read_text()
    rejects=[]
    for kind in ['truncated','trailing','hash','dimensions']:
        (bad/'density.f64le').write_bytes(orig);(bad/'metadata.json').write_text(meta)
        if kind=='truncated':(bad/'density.f64le').write_bytes(orig[:-1])
        if kind=='trailing':(bad/'density.f64le').write_bytes(orig+b'\0')
        if kind=='hash':(bad/'density.f64le').write_bytes(bytes([orig[0]^1])+orig[1:])
        if kind=='dimensions':m=json.loads(meta);m['width']+=1;(bad/'metadata.json').write_text(json.dumps(m))
        for lang in EXES:
            result=subprocess.run([str(EXES[lang]),'grain',str(bad),'1','1','-'],capture_output=True)
            assert result.returncode!=0;rejects.append({'language':lang,'kind':kind,'rejected':True})
    report={'exact_equality_required':True,'rows':rows,'malformed':rejects}
    (T/'validation.json').write_text(json.dumps(report,indent=2)+'\n')
    print(f'PASS: {len(rows)} native differential cases and {len(rejects)} malformed-file checks')
if __name__=='__main__':main()
