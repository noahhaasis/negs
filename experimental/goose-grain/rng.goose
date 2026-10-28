// Faithful port of spektrafilm-math/src/numpy_rng.rs and rand_mt 5.0.0.
// Dependency provenance and licenses: see NOTICE.md.
import std;
import math;
extern fn grain_lgamma(x: f64) -> f64;
struct Rng { mt: u32[624], index: i64, cached: bool, normal: f64 }
fn seeded(seed: u32) -> Rng {
    var r = Rng { mt: [0;624], index:624, cached:false, normal:0.0 }; r.mt[0]=seed; r.index=624;
    for i in 1..624 { r.mt[i]=1812433253 * (r.mt[i-1] ^ (r.mt[i-1] >> 30)) + (i as u32); }
    return r;
}
fn next(r: Rng&) -> u32 {
    if r.index==624 {
        for i in 624 {
            let y=(r.mt[i] & 0x80000000) | (r.mt[(i+1)%624] & 0x7fffffff);
            r.mt[i]=r.mt[(i+397)%624] ^ (y>>1) ^ (if (y&1)!=0 { 0x9908b0df } else { 0 });
        }
        r.index=0;
    }
    var y=r.mt[r.index]; r.index++;
    y ^= y>>11; y ^= (y<<7)&0x9d2c5680; y ^= (y<<15)&0xefc60000; y ^= y>>18;
    return y;
}
fn uniform(r: Rng&) -> f64 { let a=next(r)>>5; let b=next(r)>>6; return (a as f64*67108864.0+b as f64)/9007199254740992.0; }
fn normal(r: Rng&) -> f64 {
    if r.cached {let v=r.normal; r.cached=false; r.normal=0.0; return v;}
    while true {
        let x1=2.0*uniform(r)-1.0; let x2=2.0*uniform(r)-1.0; let r2=x1*x1+x2*x2;
        if r2<1.0 && r2!=0.0 {let f=sqrt(-2.0*log(r2)/r2); r.normal=f*x1; r.cached=true; return f*x2;}
    }
    return 0.0;
}
fn poisson(r: Rng&, lam: f64) -> u64 {
    if lam==0.0 {return 0;}
    if !(lam>=10.0) {
        let enlam=exp(-lam); var x: u64=0; var prod=1.0;
        while true {prod*=uniform(r); if prod>enlam {x++;} else {return x;}}
    } else {
        let slam=sqrt(lam); let loglam=log(lam); let b=0.931+2.53*slam; let a=-0.059+0.02483*b;
        let invalpha=1.1239+1.1328/(b-3.4); let vr=0.9277-3.6224/(b-2.0);
        while true {
            let u=uniform(r)-0.5; let v=uniform(r); let us=0.5-abs(u);
            let k=floor((2.0*a/us+b)*u+lam+0.43) as i64;
            if us>=0.07 && v<=vr {return k as! u64;}
            if k<0 || (us<0.013 && v>us) {continue;}
            let lhs=log(v)+log(invalpha)-log(a/(us*us)+b);
            let rhs=-lam+(k as f64)*loglam-grain_lgamma((k as f64)+1.0);
            if lhs<=rhs {return k as u64;}
        }
    }
    return 0;
}
fn inversion(r: Rng&, n: u64, p: f64) -> u64 {
    let q=1.0-p; let qn=exp((n as f64)*log(q)); let np=(n as f64)*p;
    let bound=min(n as f64,np+10.0*sqrt(np*q+1.0)) as u64;
    var x: u64=0; var px=qn; var u=uniform(r);
    while u>px {x++; if x>bound {x=0; px=qn; u=uniform(r);} else {u-=px; px=((n-x+1) as f64*p*px)/((x as f64)*q);}}
    return x;
}
fn btpe(rng: Rng&, n: u64, p: f64) -> u64 {
    let r=min(p,1.0-p); let q=1.0-r; let nf=n as f64; let fm=nf*r+r; let m=floor(fm) as i64;
    let p1=floor(2.195*sqrt(nf*r*q)-4.6*q)+0.5; let xm=m as f64+0.5; let xl=xm-p1; let xr=xm+p1;
    let c=0.134+20.5/(15.3+m as f64); let a1=(fm-xl)/(fm-xl*r); let laml=a1*(1.0+a1/2.0);
    let a2=(xr-fm)/(xr*q); let lamr=a2*(1.0+a2/2.0); let p2=p1*(1.0+2.0*c); let p3=p2+c/laml; let p4=p3+c/lamr; let nrq=nf*r*q;
    while true {
        let u=uniform(rng)*p4; var v=uniform(rng); var cand: i64=0;
        if u<=p1 {cand=floor(xm-p1*v+u) as i64; return (if p>0.5 {n as i64-cand} else {cand}) as u64;}
        else if u<=p2 {let x=xl+(u-p1)/c; v=v*c+1.0-abs((m as f64)-x+0.5)/p1; if v>1.0 {continue;} cand=floor(x) as i64;}
        else if u<=p3 {cand=floor(xl+log(v)/laml) as i64; if cand<0 {continue;} v*=(u-p2)*laml;}
        else {cand=floor(xr-log(v)/lamr) as i64; if cand>n as i64 {continue;} v*=(u-p3)*lamr;}
        let k=abs(cand-m);
        if k<=20 || k as f64>=nrq/2.0-1.0 {
            let s=r/q; let astep=s*(nf+1.0); var f=1.0;
            if m<cand {for i in m+1..cand+1 {f*=astep/(i as f64)-s;}}
            else if m>cand {for i in cand+1..m+1 {f/=astep/(i as f64)-s;}}
            if v>f {continue;}
        } else {
            let kf=k as f64; let rho=(kf/nrq)*((kf*(kf/3.0+0.625)+0.16666666666666666)/nrq+0.5);
            let t=-kf*kf/(2.0*nrq); let alog=log(v);
            if alog<t-rho {return (if p>0.5 {n as i64-cand} else {cand}) as u64;}
            if alog>t+rho {continue;}
            let x1=(cand+1) as f64; let f1=(m+1) as f64; let z=(n as i64-m+1) as f64; let w=(n as i64-cand+1) as f64;
            let x2=x1*x1; let f2=f1*f1; let z2=z*z; let w2=w*w;
            let bound=xm*log(f1/x1)+(nf-m as f64+0.5)*log(z/w)+(cand-m) as f64*log(w*r/(x1*q))
                +(13680.0-(462.0-(132.0-(99.0-140.0/f2)/f2)/f2)/f2)/f1/166320.0
                +(13680.0-(462.0-(132.0-(99.0-140.0/z2)/z2)/z2)/z2)/z/166320.0
                +(13680.0-(462.0-(132.0-(99.0-140.0/x2)/x2)/x2)/x2)/x1/166320.0
                +(13680.0-(462.0-(132.0-(99.0-140.0/w2)/w2)/w2)/w2)/w/166320.0;
            if alog>bound {continue;}
        }
        return (if p>0.5 {n as i64-cand} else {cand}) as u64;
    }
    return 0;
}
fn binomial(r: Rng&, n: u64, p: f64) -> u64 {
    if p<=0.5 {if p*(n as f64)<=30.0 {return inversion(r,n,p);} else {return btpe(r,n,p);}}
    let q=1.0-p; if q*(n as f64)<=30.0 {return n-inversion(r,n,q);} else {return n-btpe(r,n,q);}
}
