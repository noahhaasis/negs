import std;
import os;
import json;
import grain;
extern fn grain_sha256(data: const u8[:], expected: const u8[:]) -> bool;
extern fn grain_read_f64(data: const u8[:], i: i64) -> f64;
extern fn grain_rss() -> u64;
extern fn grain_checksum(data: const u8[:]) -> u64;
fn num(n: json::Node?, key: u8[:]) -> f64 {let v,ok=json::as_f64(json::member(n,key));assert(ok);return v;}
fn integer(n: json::Node?, key: u8[:]) -> i64 {let v,ok=json::as_i64(json::member(n,key));assert(ok);return v;}
fn boolean(n: json::Node?, key: u8[:]) -> bool {let v,ok=json::as_bool(json::member(n,key));assert(ok);return v;}
fn text(n: json::Node?, key: u8[:]) -> const u8[:] {let v,ok=json::as_string(json::member(n,key));assert(ok);return v;}
fn triple(n: json::Node?, key: u8[:]) -> f64[3] {
    var v: f64[3]=[0.0;3];var i=0;let arr=json::member(n,key);assert(arr);
    json::each(arr) {item=>let x,ok=json::as_f64(item);assert(ok&&i<3);v[i]=x;i++;};assert(i==3);return v;
}
fn load(dir: u8[:], files: json::Node?, key: u8[:], count: i64) -> f64[>..] {
    let f=json::member(files,key);let name=text(f,"path");assert(name==str(key,".f64le"));
    var b: u8[>..]=[];assert(read_file(str(dir,"/",name),b));assert(b.len==count*8&&b.len==integer(f,"bytes"));
    assert(grain_sha256(b,text(f,"sha256")));var a: f64[>..]=[];
    for i in count {a.push(grain_read_f64(b,i));}return a;
}
fn sample_values() -> f64[>..] {
    var values: f64[>..]=[];
    for seed in [0,1,100,5489,4294967295] {
        var r=seeded(seed as u32);
        for i in 1300 {values.push(next(r));}for i in 1000 {values.push(uniform(r));}
        for lam in [0.0,0.001,1.0,9.999999,10.0,10.000001,30.0,100.0,1000.0,100000.0] {for i in 1000 {values.push(poisson(r,lam));}values.push(next(r));}
        for n in [0,1,20,60,61,100,1000,100000] {for p in [0.0,0.000001,0.01,0.3,0.499999,0.5,0.500001,0.9,0.999999,1.0] {for i in 200 {values.push(binomial(r,n as u64,p));}values.push(next(r));}}
        for i in 1001 {values.push(normal(r));}values.push(next(r));
    }
    return values;
}
fn probes(out: u8[:]) {let values=sample_values();assert(write_file(out,bytes_of(values)));
}
fn main() {
    let args=args();assert(args.len>=3);
    if args[1]=="probes" {probes(args[2]);return;}
    if args[1]=="samplers" {let iters,ok=parse_int(args[2]);assert(ok);var times: f64[>..]=[];var sums: f64[>..]=[];for i in iters+1 {let t=clock_ns();let values=sample_values();let dt=clock_ns()-t;let sum=ordered_sum(values);if i>0 {times.push(dt/1000000.0);sums.push(sum);}}print("{\"language\":\"goose\",\"workers\":1,\"ms\":",times,",\"checksums\":",sums,"}");return;}
    let dir=args[2];var metadata: u8[>..]=[];assert(read_file(str(dir,"/metadata.json"),metadata));
    var doc: json::Node[>..]=[];var decoded: u8[>..]=[];let root,err=json::parse(metadata,doc,decoded);guard root else {abort(err);}
    assert(text(root,"format")=="negs-grain-fixture"&&integer(root,"version")==1&&integer(root,"channels")==3&&text(root,"dtype")=="f64le"&&text(root,"layout")=="HWC");
    let sn=json::member(root,"settings");let micro=json::member(sn,"micro_structure");var micro_v: f64[2]=[0.0;2];var mi=0;assert(micro);json::each(micro) {item=>let v,ok=json::as_f64(item);assert(ok&&mi<2);micro_v[mi]=v;mi++;};assert(mi==2);
    let s=Settings {rms:triple(sn,"rms_granularity"),dmin:triple(sn,"density_min"),uniformity:triple(sn,"uniformity"),scales:triple(sn,"particle_scale_sublayers"),blur:num(sn,"blur"),usm_sigma:num(sn,"mult_usm_sigma"),usm_amount:num(sn,"mult_usm_amount"),dye:num(sn,"blur_dye_clouds_um"),micro:micro_v};
    let c=Config {w:integer(root,"width"),h:integer(root,"height"),ns:integer(root,"n_samples"),nl:integer(root,"n_layers"),px:num(root,"pixel_size_um"),positive:boolean(root,"positive"),mono:boolean(root,"monochrome"),s:s};
    assert(c.w>0&&c.h>0&&c.ns>1&&c.nl>0&&finite(c.px)&&c.px>0.0);
    for i in 3 {assert(finite(s.rms[i])&&finite(s.dmin[i])&&finite(s.uniformity[i])&&finite(s.scales[i]));}
    assert(finite(s.blur)&&finite(s.usm_sigma)&&finite(s.usm_amount)&&finite(s.dye)&&finite(s.micro[0])&&finite(s.micro[1]));
    let files=json::member(root,"files");let original=load(dir,files,"density",c.w*c.h*3);let total=load(dir,files,"curves-total",c.ns*3);let layers=load(dir,files,"curves-layers",c.ns*c.nl*3);
    let workers,ok1=parse_int(args[3]);let iters,ok2=parse_int(args[4]);assert(ok1&&ok2&&workers>=1&&workers<=4&&iters>=1);
    let serial_channels=args.len>7&&args[7]=="serial";
    var ids: i64[>..]=[];if workers>1 {for i in workers-1 {ids.push(thread_spawn(grain_worker));}}
    var ms: f64[>..]=[];var rss: u64[>..]=[];var checksums: u64[>..]=[];
    for i in iters+1 {
        var input=copy(original);
        var plane: f64[>..]=[];var sigma=0.0;
        if args[1]=="blur" {let value,ok=parse_flt(args[6]);assert(ok);sigma=value;for j in c.w*c.h {plane.push(input[j*3]);}}
        let t=clock_ns();
        if args[1]=="blur" {
            let result=blur(plane,c.w,c.h,sigma);
            let dt=clock_ns()-t;if i>0 {ms.push(dt/1000000.0);rss.push(grain_rss());}
            // Consume all values, and write the last output outside timing.
            let checksum=grain_checksum(bytes_of(result));if i>0 {checksums.push(checksum);}if i==iters&&args[5]!="-" {assert(write_file(args[5],bytes_of(result)));}
        } else {
            if workers==1||serial_channels {apply(input,total,layers,c);} else {apply_parallel(input,total,layers,c);}
            let dt=clock_ns()-t;if i>0 {ms.push(dt/1000000.0);rss.push(grain_rss());}
            let checksum=grain_checksum(bytes_of(input));if i>0 {checksums.push(checksum);}if i==iters&&args[5]!="-" {assert(write_file(args[5],bytes_of(input)));}
        }
    }
    if workers>1 {for i in workers-1 {qput(Job {ch:-1,c:c,total:[],layers:[],lp:Params {dmin:[],dmax:[],npp:[]},input:[]});}for id in ids {thread_wait(id);}}
    print("{\"language\":\"goose\",\"workers\":",workers,",\"grain_workers\":",ids.len,",\"serial_channels\":",serial_channels,",\"ms\":",ms,",\"retained_rss_bytes\":",rss,",\"checksums\":",checksums,"}");
}
