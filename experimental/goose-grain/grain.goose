import std;
import math;
import rng;
import blur;
extern fn log1p(x: f64) -> f64;
struct Settings { rms: f64[3], dmin: f64[3], uniformity: f64[3], scales: f64[3], blur: f64, usm_sigma: f64, usm_amount: f64, dye: f64, micro: f64[2] }
struct Config { w: i64, h: i64, ns: i64, nl: i64, px: f64, positive: bool, mono: bool, s: Settings }
struct Params { dmin: f64[], dmax: f64[], npp: f64[] }
fn fmax(a: f64, b: f64) -> f64 {if is_nan(a) {return b;}if is_nan(b) {return a;}return if a>b {a} else {b};}
fn finite(v: f64) -> bool {return !is_nan(v) && !is_inf(v);}
fn nan_to_num(v: f64) -> f64 {return if is_nan(v) {0.0} else if is_inf(v) {if v>0.0 {1.7976931348623157e308} else {-1.7976931348623157e308}} else {v};}
fn params(c: Config&, layers: f64[:]) -> Params {
    let s=c.s;var dmax: f64[>..]=[];dmax.resize(c.nl*3,-1.0/0.0);
    for k in c.ns {for l in c.nl {for ch in 3 {let i=l*3+ch;let v=layers[(k*c.nl+l)*3+ch];if !is_nan(v)&&v>dmax[i] {dmax[i]=v;}}}}
    var total: f64[3]=[0.0,0.0,0.0];for l in c.nl {for ch in 3 {total[ch]+=dmax[l*3+ch];}}
    var fractions: f64[>..]=[];var dmin: f64[>..]=[];
    for l in c.nl {for ch in 3 {let f=dmax[l*3+ch]/total[ch];fractions.push(f);dmin.push(f*s.dmin[ch]);dmax[l*3+ch]+=f*s.dmin[ch];}}
    var coarsest: f64[3]=[0.0,0.0,0.0];
    for ch in 3 {
        var peak=-1.0/0.0;
        for k in c.ns {var sum=0.0;for l in c.nl {let i=l*3+ch;if fractions[i]==0.0 {continue;}let d=nan_to_num(layers[(k*c.nl+l)*3+ch])+dmin[i];let weight=s.scales[min(l,2)]/fractions[i];sum+=weight*d*(dmax[i]-s.uniformity[ch]*d);}if sum>peak {peak=sum;}}
        let sigma=s.rms[ch]/1000.0; coarsest[ch]=sigma*sigma*(PI*24.0*24.0)/max(peak,1e-9);
    }
    var npp: f64[>..]=[];let pixel_area=c.px*c.px;
    for l in c.nl {for ch in 3 {npp.push(pixel_area*fractions[l*3+ch]/(coarsest[ch]*s.scales[min(l,2)]));}}
    return Params { dmin, dmax, npp };
}
fn ordered_sum(a: f64[:]) -> f64 {
    var chunks: f64[>..]=[];var start=0;
    while start<a.len {var sum=0.0;for i in start..min(start+16384,a.len) {sum+=a[i];}chunks.push(sum);start+=16384;}
    var sum=0.0;for v in chunks {sum+=v;}return sum;
}
fn channel(input: f64[:], total: f64[:], layers: f64[:], c: Config&, lp: Params&, ch: i64) -> f64[>..] {
    let s=c.s;let n=c.w*c.h;let sign=if c.positive {-1.0} else {1.0};
    var xs: f64[>..]=[];for k in c.ns {xs.push(sign*total[k*3+ch]);}
    var inv: f64[>..]=[];for k in c.ns-1 {let dx=xs[k+1]-xs[k];inv.push(if dx!=0.0 {1.0/dx} else {0.0});}
    var grain: f64[>..]=[];grain.resize(n,0.0);
    for l in c.nl {
        let idx=l*3+ch;let dmax=lp.dmax[idx];let npp=lp.npp[idx];let uni=s.uniformity[ch];
        var density: f64[>..]=[];
        for i in n {
            let x=sign*input[i]; var d=0.0;
            if x<=xs[0] {d=layers[l*3+ch];} else if x>=xs[c.ns-1] {d=layers[((c.ns-1)*c.nl+l)*3+ch];}
            else {var low=0;var high=c.ns;while low<high {let mid=low+(high-low)/2;if xs[mid]<=x {low=mid+1;} else {high=mid;}}low--;let t=(x-xs[low])*inv[low];let y0=layers[(low*c.nl+l)*3+ch];let y1=layers[((low+1)*c.nl+l)*3+ch];d=y0+t*(y1-y0);}
            density.push(d+lp.dmin[idx]);
        }
        if finite(npp)&&npp>0.0 {
            let od=dmax/npp;var r=seeded((ch+10*l) as u32);var counts: u32[>..]=[];
            for d in density {let p=clamp(d/dmax,1e-6,1.0-1e-6);let sat=1.0-p*uni*(1.0-1e-6);counts.push(min(poisson(r,npp/sat),4294967295) as u32);}
            for i in n {let p=clamp(density[i]/dmax,1e-6,1.0-1e-6);let sat=1.0-p*uni*(1.0-1e-6);density[i]=(binomial(r,counts[i],p) as f64)*od*sat;}
            let dye_sigma=s.dye*sqrt(od);
            if dye_sigma>0.0 {let g=blur(density,c.w,c.h,dye_sigma);for i in n {grain[i]+=g[i];}}
            else {for i in n {grain[i]+=density[i];}}
        } else {for i in n {grain[i]+=clamp(density[i]/dmax,1e-6,1.0-1e-6)*dmax;}}
    }
    let sigma=s.micro[1]*0.001/c.px;let blurpx=s.micro[0]/c.px;
    if sigma>0.05 {
        let sigma2=log1p(sigma*sigma);let stdev=sqrt(sigma2);var r=seeded((100+ch) as u32);
        var clumps: f64[>..]=[];for i in n {clumps.push(exp(normal(r)*stdev-0.5*sigma2));}
        if blurpx>0.4 {let g=blur(clumps,c.w,c.h,blurpx);for i in n {grain[i]*=g[i];}}
        else {for i in n {grain[i]*=clumps[i];}}
    }
    if s.blur>0.0 {let g=blur(grain,c.w,c.h,s.blur);for i in n {grain[i]=g[i];}}
    if s.usm_sigma>0.0 && s.usm_amount>0.0 {
        for i in n {grain[i]=fmax(grain[i],0.0);}
        var g=blur(grain,c.w,c.h,s.usm_sigma);let mass=ordered_sum(grain);
        let a=s.usm_amount;let a1=1.0+a;
        for i in n {let d=grain[i];g[i]=if d<=0.0 {0.0} else {pow(fmax(d,1e-6),a1)/pow(fmax(g[i],1e-6),a)};}
        let sharp=ordered_sum(g);if sharp>0.0 {let scale=mass/sharp;for i in n {g[i]*=scale;}}
        for i in n {grain[i]=g[i];}
    }
    for i in n {grain[i]-=s.dmin[ch];}return grain;
}
fn apply(input: f64[:], total: f64[:], layers: f64[:], c: Config&) {
    let lp=params(c,layers);
    // Assemble only after all channels have finished reading the input.
    var planes: f64[>..]=[];
    for ch in (if c.mono {1} else {3}) {var plane: f64[>..]=[];for i in c.w*c.h {plane.push(input[i*3+ch]);}let p=channel(plane,total,layers,c,lp,ch);planes.append(p);}
    for i in c.w*c.h {for ch in 3 {input[i*3+ch]=planes[(if c.mono {0} else {ch})*c.w*c.h+i];}}
}
// Supported workers exchange copied flat jobs, never pointer tokens.
struct Job { ch: i64, c: Config, total: f64[], layers: f64[], lp: Params, input: f64[] }
struct Plane { ch: i64, values: f64[] }
thread_fn grain_worker() {
    loop {
        let j=qget<Job>();if j.ch<0 {break;}
        qput(Plane {ch:j.ch,values:channel(j.input,j.total,j.layers,j.c,j.lp,j.ch)});
    }
}
fn apply_parallel(input: f64[:], total: f64[:], layers: f64[:], c: Config&) {
    let lp=params(c,layers);let channels=if c.mono {1} else {3};
    for ch in channels {var plane: f64[>..]=[];for i in c.w*c.h {plane.push(input[i*3+ch]);}qput(Job {ch:ch,c:c,total:total,layers:layers,lp:copy(lp),input:copy(plane)});}
    for done in channels {let p=qget<Plane>();for i in c.w*c.h {if c.mono {for ch in 3 {input[i*3+ch]=p.values[i];}} else {input[i*3+p.ch]=p.values[i];}}}
}
