// Port of spektrafilm-math/src/gaussian.rs. Strict evaluation order.
import std;
import math;
fn reflect(i: i64, n: i64) -> i64 {let j=i%(2*n); return if j<n {j} else {2*n-1-j};}
fn filter(src: f64[:], dst: f64[:], a: f64, b1: f64, b2: f64, b3: f64) {
    var w1=src[0]; var w2=w1; var w3=w1;
    for j in src.len {let v=a*src[j]+b1*w1+b2*w2+b3*w3;dst[j]=v;w3=w2;w2=w1;w1=v;}
    w1=dst[dst.len-1];w2=w1;w3=w1;
    for k in dst.len {let j=dst.len-1-k;let v=a*dst[j]+b1*w1+b2*w2+b3*w3;dst[j]=v;w3=w2;w2=w1;w1=v;}
}
fn blur_pass(src: f64[:], dst: f64[:], w: i64, h: i64, sigma: f64, horizontal: bool) {
    if sigma<3.0 {
        let radius=(3.0*sigma+0.5) as i64; var kernel: f64[>..]=[];var sum=0.0;
        let s2=2.0*sigma*sigma;
        for i in 2*radius+1 {let x=i as f64-radius as f64;let v=exp(-x*x/s2);kernel.push(v);sum+=v;}
        for i in kernel.len {kernel[i]/=sum;}
        for y in h {for x in w {var v=0.0;for k in kernel.len {
            let xx=if horizontal {reflect(x+k-radius,w)} else {x};
            let yy=if horizontal {y} else {reflect(y+k-radius,h)};
            v+=src[yy*w+xx]*kernel[k];
        }dst[y*w+x]=v;}}
    } else {
        let q=if sigma>=2.5 {0.98711*sigma-0.96330} else {3.97156-4.14554*sqrt(1.0-0.26891*sigma)};
        let q2=q*q; let q3=q2*q;
        let b0=1.57825+2.44413*q+1.4281*q2+0.422205*q3;
        let b1=(2.44413*q+2.85619*q2+1.26661*q3)/b0;
        let b2=-(1.4281*q2+1.26661*q3)/b0; let b3=0.422205*q3/b0;let a=1.0-b1-b2-b3;
        if horizontal {for y in h {filter(src[y*w..(y+1)*w],dst[y*w..(y+1)*w],a,b1,b2,b3);}}
        else {
            var cols: f64[>..]=[];cols.resize(w*h,0.0);
            for x in w {var col: f64[>..]=[];for y in h {col.push(src[y*w+x]);}filter(col,cols[x*h..(x+1)*h],a,b1,b2,b3);}
            for y in h {for x in w {dst[y*w+x]=cols[x*h+y];}}
        }
    }
}
fn blur(src: f64[:], w: i64, h: i64, sigma_input: f64) -> f64[>..] {
    // Rust rounds sigma through f32 before either pass, even in f64 mode.
    let sigma=(sigma_input as f32) as f64;
    var chan: f64[>..]=[];chan.append(src);if sigma<=0.0 {return chan;}
    var tmp: f64[>..]=[];tmp.resize(w*h,0.0);
    blur_pass(chan,tmp,w,h,sigma,true);blur_pass(tmp,chan,w,h,sigma,false);return chan;
}
