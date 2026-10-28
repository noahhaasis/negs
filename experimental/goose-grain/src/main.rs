use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use spektrafilm_math::{gaussian::gaussian_blur_channel, image::ImageBuf, numpy_rng::*};
use spektrafilm_model::grain::*;
use std::{fs, path::Path, time::Instant};
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn write_f64(p: &Path, a: &[f64]) {
    let b: Vec<u8> = a.iter().flat_map(|v| v.to_le_bytes()).collect();
    fs::write(p, b).unwrap();
}
fn read_f64(p: &Path, n: usize) -> Vec<f64> {
    let b = fs::read(p).unwrap();
    assert_eq!(b.len(), n * 8);
    b.chunks_exact(8)
        .map(|c| f64::from_le_bytes(c.try_into().unwrap()))
        .collect()
}
fn arr(m: &Value, k: &str) -> [f64; 3] {
    std::array::from_fn(|i| m[k][i].as_f64().unwrap())
}
fn settings(m: &Value) -> GrainSettings {
    GrainSettings {
        rms_granularity: arr(m, "rms_granularity"),
        density_min: arr(m, "density_min"),
        uniformity: arr(m, "uniformity"),
        particle_scale_sublayers: arr(m, "particle_scale_sublayers"),
        blur: m["blur"].as_f64().unwrap(),
        mult_usm_sigma: m["mult_usm_sigma"].as_f64().unwrap(),
        mult_usm_amount: m["mult_usm_amount"].as_f64().unwrap(),
        blur_dye_clouds_um: m["blur_dye_clouds_um"].as_f64().unwrap(),
        micro_structure: [
            m["micro_structure"][0].as_f64().unwrap(),
            m["micro_structure"][1].as_f64().unwrap(),
        ],
    }
}
fn capture(input: &str, out: &str) {
    use spektrafilm_core::{
        params::RuntimeParams, pipeline::Pipeline, profile::load_profile_by_name,
    };
    let dir = Path::new(out);
    fs::create_dir_all(dir).unwrap();
    // Set the diagnostic hook before creating worker threads.
    unsafe {
        std::env::set_var("SPEKTRAFILM_DUMP_FILM_PRE_GRAIN", dir.join("density.f64le"));
    }
    let recipe = fs::read("experimental/goose-grain/capture-params.json").unwrap();
    let mut params: RuntimeParams = serde_json::from_slice(&recipe).unwrap();
    params.io.input_color_space = "sRGB".into();
    params.io.input_cctf_decoding = false;
    use spektrafilm_io::raw::{RawDecodeOptions, RawDecoder, RawlerDecoder};
    let image = RawlerDecoder
        .decode(Path::new(input), RawDecodeOptions::default())
        .unwrap();
    let (w, h) = (image.width, image.height);
    let data = Path::new("data");
    let p = Pipeline::new_with_spectral(
        load_profile_by_name(data, "kodak_gold_200").unwrap(),
        load_profile_by_name(data, "kodak_portra_endura").unwrap(),
        params,
        data,
    )
    .unwrap();
    let _ = p.process(image, &spektrafilm_gpu::cpu_backend::CpuBackend);
    let s = spektrafilm_core::grain::grain_settings(&p.film, &p.params.film_render.grain);
    let c = spektrafilm_core::grain::grain_curves(&p.film);
    let px = spektrafilm_core::stages::filming::grain_pixel_size_um(&p.params, w, h);
    write_f64(
        &dir.join("curves-total.f64le"),
        &c.total.iter().flatten().copied().collect::<Vec<_>>(),
    );
    write_f64(
        &dir.join("curves-layers.f64le"),
        &c.layers.iter().flatten().copied().collect::<Vec<_>>(),
    );
    let lp = layer_grain_params(&c, &s, px);
    let dye: Vec<_> = (0..c.n_layers)
        .map(|l| {
            (0..3)
                .map(|ch| lp.dye_cloud_sigma(s.blur_dye_clouds_um, l, ch))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut files = json!({});
    for name in ["density", "curves-total", "curves-layers"] {
        let filename = format!("{name}.f64le");
        let b = fs::read(dir.join(&filename)).unwrap();
        files[name] = json!({"path":filename,"bytes":b.len(),"sha256":hash(&b)});
    }
    assert_eq!(
        files["density"]["bytes"],
        json!(w as usize * h as usize * 3 * 8)
    );
    let meta = json!({"format":"negs-grain-fixture","version":1,"width":w,"height":h,"channels":3,"layout":"HWC","dtype":"f64le","pixel_size_um":px,"positive":p.film.is_positive(),"monochrome":p.params.film_render.grain.monochrome,"n_samples":c.n_samples(),"n_layers":c.n_layers,"files":files,"settings":{"rms_granularity":s.rms_granularity,"density_min":s.density_min,"uniformity":s.uniformity,"particle_scale_sublayers":s.particle_scale_sublayers,"blur":s.blur,"mult_usm_sigma":s.mult_usm_sigma,"mult_usm_amount":s.mult_usm_amount,"blur_dye_clouds_um":s.blur_dye_clouds_um,"micro_structure":s.micro_structure},"effects":{"multilayer":c.n_layers>1,"dye_sigmas":dye,"micro_sigma":s.micro_structure[1]*0.001/px,"micro_blur":s.micro_structure[0]/px,"pixel_blur":s.blur,"sharpen_sigma":s.mult_usm_sigma,"sharpen_amount":s.mult_usm_amount},"provenance":{"input_sha256":hash(&fs::read(input).unwrap()),"recipe":serde_json::from_slice::<Value>(&recipe).unwrap(),"capture_args":std::env::args().collect::<Vec<_>>()}});
    fs::write(
        dir.join("metadata.json"),
        serde_json::to_vec_pretty(&meta).unwrap(),
    )
    .unwrap();
    eprintln!("captured {w}x{h} at {px} um/px");
}
fn sample_values() -> Vec<f64> {
    let mut values = Vec::new();
    for seed in [0, 1, 100, 5489, u32::MAX] {
        let mut rng = rand_mt::Mt::new(seed);
        for _ in 0..1300 {
            values.push(rng.next_u32() as f64);
        }
        for _ in 0..1000 {
            values.push(rk_double(&mut rng));
        }
        for lam in [
            0., 0.001, 1., 9.999999, 10., 10.000001, 30., 100., 1000., 100000.,
        ] {
            for _ in 0..1000 {
                values.push(rk_poisson(&mut rng, lam) as f64);
            }
            values.push(rng.next_u32() as f64);
        }
        for n in [0, 1, 20, 60, 61, 100, 1000, 100000] {
            for p in [
                0., 0.000001, 0.01, 0.3, 0.499999, 0.5, 0.500001, 0.9, 0.999999, 1.,
            ] {
                for _ in 0..200 {
                    values.push(rk_binomial(&mut rng, n, p) as f64);
                }
                values.push(rng.next_u32() as f64);
            }
        }
        let mut g = GaussRng::from_mt(rng);
        for _ in 0..1001 {
            values.push(g.gauss());
        }
        values.push(g.rng.next_u32() as f64);
    }
    values
}
fn probes(out: &str) {
    write_f64(Path::new(out), &sample_values());
}
fn main() {
    let a: Vec<_> = std::env::args().collect();
    if a[1] == "capture" {
        capture(&a[2], &a[3]);
        return;
    }
    if a[1] == "probes" {
        probes(&a[2]);
        return;
    }
    if a[1] == "samplers" {
        let iters: usize = a[2].parse().unwrap();
        let mut times = Vec::new();
        let mut sums = Vec::new();
        for i in 0..=iters {
            let t = Instant::now();
            let values = sample_values();
            let ms = t.elapsed().as_secs_f64() * 1000.;
            let sum: f64 = values.iter().sum();
            if i > 0 {
                times.push(ms);
                sums.push(sum);
            }
        }
        println!(
            "{}",
            json!({"language":"rust","workers":1,"ms":times,"checksums":sums})
        );
        return;
    }
    let dir = Path::new(&a[2]);
    let m: Value = serde_json::from_slice(&fs::read(dir.join("metadata.json")).unwrap()).unwrap();
    assert_eq!(m["format"], "negs-grain-fixture");
    assert_eq!(m["version"], 1);
    assert_eq!(m["channels"], 3);
    assert_eq!(m["layout"], "HWC");
    assert_eq!(m["dtype"], "f64le");
    assert_eq!(m["files"].as_object().unwrap().len(), 3);
    assert!(
        m["pixel_size_um"].as_f64().unwrap().is_finite()
            && m["pixel_size_um"].as_f64().unwrap() > 0.0
    );
    for key in ["density", "curves-total", "curves-layers"] {
        let f = &m["files"][key];
        assert_eq!(f["path"].as_str().unwrap(), format!("{key}.f64le"));
        let b = fs::read(dir.join(f["path"].as_str().unwrap())).unwrap();
        assert_eq!(b.len() as u64, f["bytes"].as_u64().unwrap());
        assert_eq!(hash(&b), f["sha256"].as_str().unwrap());
    }
    let (w, h) = (
        u32::try_from(m["width"].as_u64().unwrap()).unwrap(),
        u32::try_from(m["height"].as_u64().unwrap()).unwrap(),
    );
    assert!(w > 0 && h > 0);
    let n = m["n_samples"].as_u64().unwrap() as usize;
    let l = m["n_layers"].as_u64().unwrap() as usize;
    assert!(n > 1 && l > 0);
    let rows = |v: Vec<f64>| v.chunks_exact(3).map(|r| [r[0], r[1], r[2]]).collect();
    let c = GrainCurves {
        total: rows(read_f64(&dir.join("curves-total.f64le"), n * 3)),
        layers: rows(read_f64(&dir.join("curves-layers.f64le"), n * l * 3)),
        n_layers: l,
    };
    let original = ImageBuf::from_data(
        w,
        h,
        read_f64(&dir.join("density.f64le"), w as usize * h as usize * 3),
    );
    let s = settings(&m["settings"]);
    let workers: usize = a[3].parse().unwrap();
    let iters: usize = a[4].parse().unwrap();
    rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build_global()
        .unwrap();
    let serial = a.get(7).map_or(false, |x| x == "serial");
    let mut times = Vec::new();
    let mut hashes = Vec::new();
    let mut rss = Vec::new();
    for i in 0..=iters {
        let input = original.clone();
        let plane = if a[1] == "blur" {
            input.extract_channel(0)
        } else {
            Vec::new()
        };
        let sigma: f32 = a.get(6).map_or(0.0, |x| x.parse().unwrap_or(0.0));
        let t = Instant::now();
        let output = if a[1] == "blur" {
            gaussian_blur_channel(&plane, w, h, sigma)
        } else {
            apply_grain(
                input,
                m["pixel_size_um"].as_f64().unwrap(),
                &s,
                &c,
                m["positive"].as_bool().unwrap(),
                m["monochrome"].as_bool().unwrap(),
                serial,
            )
            .data
        };
        let ms = t.elapsed().as_secs_f64() * 1000.;
        let retained = goose_grain_experiment::grain_rss();
        // Borrow binary64 bytes for untimed hashing/writing. Avoid a full-size
        // checksum allocation that would distort the process memory metric.
        assert!(cfg!(target_endian = "little"));
        let b =
            unsafe { std::slice::from_raw_parts(output.as_ptr().cast::<u8>(), output.len() * 8) };
        let digest = hash(b);
        if i > 0 {
            times.push(ms);
            hashes.push(digest);
            rss.push(retained);
        }
        if i == iters && a[5] != "-" {
            fs::write(&a[5], b).unwrap();
        }
    }
    println!(
        "{}",
        json!({"language":"rust","workers":workers,"serial_channels":serial,"ms":times,"hashes":hashes,"retained_rss_bytes":rss})
    );
}
