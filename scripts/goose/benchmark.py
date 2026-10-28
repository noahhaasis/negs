#!/usr/bin/env python3
"""Alternate native batches; use INTERNAL durations only, time(1) for RSS only."""
import argparse,json,pathlib,subprocess,statistics,re,hashlib,platform,os
import numpy as np
from fixtures import verify,digest
ROOT=pathlib.Path(__file__).resolve().parents[2];T=ROOT/'target/goose'
EXES={'rust':ROOT/'target/release/goose-grain-experiment','goose':T/'goose-bench'}
def main():
    p=argparse.ArgumentParser();p.add_argument('fixture');p.add_argument('--workers',type=int,default=1);p.add_argument('--mode',default='grain',choices=['grain','blur','samplers']);p.add_argument('--sigma',default='3.5');p.add_argument('--batch-iters',type=int,default=7);p.add_argument('--batches',type=int,default=2);a=p.parse_args()
    counts=[a.batch_iters]*a.batches
    assert min(counts)>=7
    fixture=pathlib.Path(a.fixture).resolve();meta=verify(fixture)
    name=f'{fixture.name}-{a.mode}-w{a.workers}'+(('-s'+a.sigma) if a.mode=='blur' else '')
    d=T/'runs'/name;d.mkdir(parents=True,exist_ok=True)
    build=json.loads((T/'build.json').read_text())
    for lang,exe in EXES.items():assert digest(exe)==build['binary_sha256'][str(exe.relative_to(ROOT))]
    env={k:v for k,v in os.environ.items() if k!='SPEKTRAFILM_STAGE_TIMINGS' and not k.startswith('SPEKTRAFILM_DUMP_')}
    rows=[];reference=None
    # AB, BA batches: each process has a fresh warmup and >=7 measured calls.
    for batch in range(a.batches):
        for lang in (['rust','goose'] if batch%2==0 else ['goose','rust']):
            output=d/(lang+'-output.f64le')
            args=['samplers',str(counts[batch])] if a.mode=='samplers' else [a.mode,str(fixture),str(a.workers),str(counts[batch]),str(output),a.sigma]
            print('BEGIN',name,'batch',batch,lang,flush=True)
            result=subprocess.run(['/usr/bin/time','-l',str(EXES[lang]),*args],text=True,capture_output=True,check=False,env=env)
            (d/f'{batch}-{lang}.stdout.json').write_text(result.stdout);(d/f'{batch}-{lang}.stderr.txt').write_text(result.stderr)
            result.check_returncode()
            row=json.loads(result.stdout);row['batch']=batch
            peak=re.search(r'(\d+)\s+maximum resident set size',result.stderr);row['peak_rss_bytes']=int(peak[1]) if peak else None
            if a.mode!='samplers':
                row['output_sha256']=digest(output)
                if reference is None:reference=row['output_sha256']
                assert row['output_sha256']==reference, 'Full-size output differs; no performance conclusion permitted'
                # Keep outputs only until the alternate executable has checked the hash.
                output.unlink()
            rows.append(row)
            (d/'raw.json').write_text(json.dumps(rows,indent=2)+'\n')
            print(name,batch,lang,'median',statistics.median(row['ms']),flush=True)
    summary={}
    for lang in EXES:
        calls=[x for r in rows if r['language']==lang for x in r['ms']]
        summary[lang]={'median_ms':statistics.median(calls),'min_ms':min(calls),'max_ms':max(calls),'p25_ms':float(np.percentile(calls,25)),'p75_ms':float(np.percentile(calls,75)),'calls':len(calls),'peak_rss_bytes':max(r['peak_rss_bytes'] or 0 for r in rows if r['language']==lang),'retained_rss_bytes':max([x for r in rows if r['language']==lang for x in r.get('retained_rss_bytes',[])],default=None)}
    report={'fixture':fixture.name,'metadata_sha256':digest(fixture/'metadata.json'),'width':meta['width'],'height':meta['height'],'effects':meta.get('effects'),'mode':a.mode,'sigma':a.sigma if a.mode=='blur' else None,'workers':a.workers,'build':build,'platform':platform.platform(),'restoration':'fresh deep copy of immutable input immediately before each timer; warm cache, no explicit cache flush','timed_boundary':'complete native operation; includes calibration, RNG init, scratch and required worker copies/assembly; excludes startup, fixture load, pools, restoration and checksum','rows':rows,'summary':summary,'rust_over_goose':summary['rust']['median_ms']/summary['goose']['median_ms'],'batch_ratios':[]}
    for batch in range(a.batches):
        pair={r['language']:statistics.median(r['ms']) for r in rows if r['batch']==batch};report['batch_ratios'].append(pair['rust']/pair['goose'])
    (d/'results.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(summary,indent=2))
if __name__=='__main__':main()
