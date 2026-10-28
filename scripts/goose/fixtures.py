#!/usr/bin/env python3
"""Fixture verification, synthetic differential cases, provenance freezing."""
import hashlib,json,pathlib,subprocess,sys
import numpy as np
ROOT=pathlib.Path(__file__).resolve().parents[2]
def digest(p): return hashlib.file_digest(open(p,'rb'),'sha256').hexdigest()
def freeze(d,m,density,total,layers):
    d=pathlib.Path(d); d.mkdir(parents=True,exist_ok=True)
    m=json.loads(json.dumps(m));m['files']={}
    for key,a in [('density',density),('curves-total',total),('curves-layers',layers)]:
        p=d/(key+'.f64le');np.asarray(a,dtype='<f8').tofile(p)
        m['files'][key]={'path':p.name,'bytes':p.stat().st_size,'sha256':digest(p)}
    (d/'metadata.json').write_text(json.dumps(m,indent=2,allow_nan=False)+'\n');return m
def verify(d):
    d=pathlib.Path(d);m=json.loads((d/'metadata.json').read_text())
    assert [m[k] for k in ['format','version','channels','dtype','layout']]==['negs-grain-fixture',1,3,'f64le','HWC']
    assert all(isinstance(m[k],int) and m[k]>0 for k in ['width','height','n_samples','n_layers'])
    expected={'density':m['width']*m['height']*3,'curves-total':m['n_samples']*3,'curves-layers':m['n_samples']*m['n_layers']*3}
    for k,n in expected.items():
        f=m['files'][k];assert f['path']==k+'.f64le';p=d/f['path'];assert f['bytes']==p.stat().st_size==n*8;assert digest(p)==f['sha256']
    assert np.isfinite(m['pixel_size_um']) and m['pixel_size_um']>0
    for v in m['settings'].values():assert np.isfinite(v).all()
    return m
def cases(root):
    # Synthetic densities and curves, explicitly labelled, preserving full-res
    # physical sampling for active microstructure on tiny image dimensions.
    base={'format':'negs-grain-fixture','version':1,'channels':3,'layout':'HWC','dtype':'f64le','pixel_size_um':5.787037037037037,'positive':False,'monochrome':False,'n_samples':11,'n_layers':3,'settings':{'rms_granularity':[6.,8.,10.],'density_min':[.03,.04,.05],'uniformity':[.97,.95,.93],'particle_scale_sublayers':[1.,.5,.25],'blur':.89,'mult_usm_sigma':3.5,'mult_usm_amount':1.5,'blur_dye_clouds_um':2.,'micro_structure':[20.,1000.]},'provenance':{'kind':'synthetic density and monotonic layer curves','generator':'scripts/goose/fixtures.py'}}
    for w,h in [(1,1),(1,17),(19,1),(7,5),(33,29)]:
      for nl in [1,3,4]:
       for positive,mono in [(False,False),(True,False),(False,True)]:
        m=json.loads(json.dumps(base));m.update(width=w,height=h,n_layers=nl,positive=positive,monochrome=mono)
        total=np.linspace(0,3,11)[:,None]*np.array([.9,1,1.1])[None,:]
        if positive: total=total[::-1].copy()
        fractions=np.arange(1,nl+1,dtype=float);fractions/=fractions.sum()
        layers=total[:,None,:]*fractions[None,:,None]
        density=np.resize(np.array([-.1,0,.03,.4,1,2,3,4.]),w*h*3).reshape(h,w,3)
        name=f'{w}x{h}-l{nl}-p{int(positive)}-m{int(mono)}'
        freeze(pathlib.Path(root)/name,m,density,total,layers)
    # Noise-free limit and a zero-density sublayer (calibration edge).
    for name in ['zero-rms','zero-layer','duplicate-axis','nan-curve']:
        m=json.loads(json.dumps(base));m.update(width=7,height=5)
        total=np.linspace(0,3,11)[:,None]*np.ones((1,3));layers=np.repeat(total[:,None,:]/3,3,axis=1)
        if name=='zero-rms':m['settings']['rms_granularity']=[0.,0.,0.]
        if name=='zero-layer':layers[:,0,:]=0;total=layers.sum(axis=1)
        if name=='duplicate-axis':total[5]=total[4];layers[5]=layers[4]
        if name=='nan-curve':layers[3,1,0]=np.nan
        freeze(pathlib.Path(root)/name,m,np.linspace(0,3,7*5*3),total,layers)
def blur_cases(root):
    base=pathlib.Path(root).parent/'tiny/7x5-l3-p0-m0'
    m=verify(base);total=np.fromfile(base/'curves-total.f64le','<f8');layers=np.fromfile(base/'curves-layers.f64le','<f8')
    for w,h in [(1,1),(1,17),(19,1),(7,5),(33,29)]:
        for pattern in ['constant','impulse','ramp','noise']:
            case=json.loads(json.dumps(m));case.update(width=w,height=h)
            if pattern=='constant':a=np.full((h,w),.42)
            elif pattern=='impulse':a=np.zeros((h,w));a[0,0]=8.;a[-1,-1]+=4.
            elif pattern=='ramp':a=np.arange(w*h,dtype=float).reshape(h,w)/max(1,w*h-1)
            else:a=np.random.RandomState(123).normal(size=(h,w))
            case['provenance']={'kind':'synthetic blur pattern','pattern':pattern,'seed':123}
            freeze(pathlib.Path(root)/f'{w}x{h}-{pattern}',case,np.repeat(a[:,:,None],3,axis=2),total,layers)
    independent=json.loads((ROOT/'crates/spektrafilm-core/tests/fixtures/blur_edges_python_28bf883.json').read_text())
    for name,field in independent['fields'].items():
        case=json.loads(json.dumps(m));case.update(width=field['width'],height=field['height'])
        case['provenance']={'kind':'existing independent Python blur edge field','field':name}
        a=np.array(field['data']).reshape(field['height'],field['width'])
        freeze(pathlib.Path(root)/('python-'+name),case,np.repeat(a[:,:,None],3,axis=2),total,layers)

def provenance(d):
    d=pathlib.Path(d);m=verify(d);p=m.setdefault('provenance',{})
    p['source_revision']=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    diff=subprocess.check_output(['git','diff','--binary'],cwd=ROOT);p['tracked_diff_sha256']=hashlib.sha256(diff).hexdigest()
    paths=['crates/spektrafilm-model/src/grain.rs','crates/spektrafilm-math/src/numpy_rng.rs','crates/spektrafilm-math/src/gaussian.rs','data/profiles/kodak_gold_200.json','data/profiles/kodak_portra_endura.json','data/presets/grain.toml','experimental/goose-grain/capture-params.json']
    p['sources']={f:digest(ROOT/f) for f in paths};p['build']=json.loads((ROOT/'target/goose/build.json').read_text())
    (d/'metadata.json').write_text(json.dumps(m,indent=2,allow_nan=False)+'\n')
if __name__=='__main__':
    if sys.argv[1]=='cases':cases(sys.argv[2])
    elif sys.argv[1]=='blur-cases':blur_cases(sys.argv[2])
    elif sys.argv[1]=='provenance':provenance(sys.argv[2])
    else:print(json.dumps(verify(sys.argv[2]),indent=2))
