use super::vendor::{fuji_radial, sony_radial};
use super::*;

fn opcode_list(ops: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut b = (ops.len() as u32).to_be_bytes().to_vec();
    for (id, body) in ops {
        for v in [*id, 0x0103_0000, 0, body.len() as u32] {
            b.extend(v.to_be_bytes());
        }
        b.extend(body);
    }
    b
}

fn doubles(v: &[f64]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_be_bytes()).collect()
}

fn warp_body(planes: &[[f64; 6]], centre: (f64, f64)) -> Vec<u8> {
    let mut b = (planes.len() as u32).to_be_bytes().to_vec();
    for p in planes {
        b.extend(doubles(p));
    }
    b.extend(doubles(&[centre.0, centre.1]));
    b
}

fn dng_profile(bytes: &[u8]) -> Profile {
    Profile { source: Source::Dng, model: Model::Dng(dng::parse(bytes).unwrap()) }
}

#[test]
fn spline_is_linear_between_knots_and_constant_outside() {
    let (x, y) = ([0.0, 0.5, 1.0], [1.0, 2.0, 4.0]);
    assert_eq!(spline(&x, &y, -1.0), 1.0);
    assert_eq!(spline(&x, &y, 0.25), 1.5);
    assert_eq!(spline(&x, &y, 0.75), 3.0);
    assert_eq!(spline(&x, &y, 2.0), 4.0);
}

#[test]
fn dng_opcodes_parse_and_skip_unknown_ones() {
    let warp = [1.0, -0.05, 0.01, 0.0, 0.001, -0.002];
    let bytes = opcode_list(&[
        (4, vec![0; 8]),
        (1, warp_body(&[warp], (0.5, 0.5))),
        (3, doubles(&[-0.2, 0.05, 0.0, 0.0, 0.0, 0.4, 0.6])),
    ]);
    let ops = dng::parse(&bytes).unwrap();
    assert_eq!(ops.warp.as_ref().unwrap().planes, vec![warp]);
    assert_eq!(ops.vignette.as_ref().unwrap().centre, (0.4, 0.6));
    // Identity warps and empty vignettes are not corrections.
    let identity = opcode_list(&[(1, warp_body(&[[1.0, 0.0, 0.0, 0.0, 0.0, 0.0]], (0.5, 0.5)))]);
    assert!(dng::parse(&identity).is_none());
    // Truncated data is rejected, not read past.
    assert!(dng::parse(&bytes[..bytes.len() - 3]).is_none());
}

#[test]
fn dng_warp_maps_points_as_the_specification_says() {
    // 200 x 100 area, centre in the middle: m is the half diagonal.
    let k = [0.98, -0.05, 0.0, 0.0, 0.002, -0.001];
    let p = dng_profile(&opcode_list(&[(1, warp_body(&[k], (0.5, 0.5)))]));
    let lens = Prepared::new(&p, 200.0, 100.0, None);
    let m = 100f64.hypot(50.0);
    let (x, y) = (180.0, 20.0);
    let (dx, dy) = ((x - 100.0) / m, (y - 50.0) / m);
    let r2 = dx * dx + dy * dy;
    let f = k[0] + k[1] * r2;
    let want_x = 100.0 + m * (f * dx + k[4] * 2.0 * dx * dy + k[5] * (r2 + 2.0 * dx * dx));
    let want_y = 50.0 + m * (f * dy + k[5] * 2.0 * dx * dy + k[4] * (r2 + 2.0 * dy * dy));
    let (sx, sy) = lens.source(1, x, y);
    assert!((sx - want_x).abs() < 1e-9 && (sy - want_y).abs() < 1e-9);
    // One plane applies to all three.
    assert_eq!(lens.source(0, x, y), lens.source(2, x, y));
}

#[test]
fn dng_area_offsets_the_optical_centre() {
    // The developed image starts 10 px into a 220 x 120 active area.
    let k = [0.95, 0.0, 0.0, 0.0, 0.0, 0.0];
    let p = dng_profile(&opcode_list(&[(1, warp_body(&[k], (0.5, 0.5)))]));
    let lens = Prepared::new(&p, 200.0, 100.0, Some([-10.0, -10.0, 220.0, 120.0]));
    // The area centre (100, 50) in image pixels stays fixed.
    assert_eq!(lens.source(1, 100.0, 50.0), (100.0, 50.0));
}

#[test]
fn dng_vignette_gain_at_the_corner_is_the_polynomial() {
    let k = [0.3, 0.1, 0.0, 0.0, 0.0];
    let mut body = doubles(&k);
    body.extend(doubles(&[0.5, 0.5]));
    let p = dng_profile(&opcode_list(&[(3, body)]));
    let lens = Prepared::new(&p, 200.0, 100.0, None);
    assert!((lens.gain(0.0, 0.0) - 1.4).abs() < 1e-12);
    assert_eq!(lens.gain(100.0, 50.0), 1.0);
    assert!(!p.warps());
}

fn constant(w: u32, h: u32, rgb: [f32; 3]) -> ImageBuf {
    let data = (0..w * h).flat_map(|_| rgb.map(from_f32)).collect();
    ImageBuf::from_data(w, h, data)
}

#[test]
fn warps_keep_a_flat_field_flat_and_fill_the_frame() {
    // Barrel correction with lateral CA: every output pixel samples real
    // data, so a constant image stays constant (no black corners, weights
    // sum to one).
    let planes = [
        [1.0, 0.08, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.07, 0.0, 0.0, 0.0, 0.0],
        [1.0, 0.06, 0.0, 0.0, 0.0, 0.0],
    ];
    let p = dng_profile(&opcode_list(&[(1, warp_body(&planes, (0.5, 0.5)))]));
    let mut img = constant(64, 48, [0.2, 1.5, -0.1]);
    apply(&mut img, &p, None);
    for px in img.data.chunks_exact(3) {
        for (v, want) in px.iter().zip([0.2, 1.5, -0.1]) {
            assert!((to_f32(*v) - want).abs() < 1e-5, "{v} vs {want}");
        }
    }
    let lens = Prepared::new(&p, 64.0, 48.0, None);
    assert!(fill_zoom(&lens, 64.0, 48.0, 1.0) > 1.0);
}

#[test]
fn vignette_only_profiles_do_not_resample() {
    let mut body = doubles(&[1.0, 0.0, 0.0, 0.0, 0.0]);
    body.extend(doubles(&[0.5, 0.5]));
    let p = dng_profile(&opcode_list(&[(3, body)]));
    // A single bright pixel stays where it is.
    let mut img = constant(9, 9, [0.0; 3]);
    img.set(2, 3, [from_f32(1.0); 3]);
    apply(&mut img, &p, None);
    let lit: Vec<_> = (0..81).filter(|i| to_f32(img.data[i * 3]) != 0.0).collect();
    assert_eq!(lit, vec![3 * 9 + 2]);
    assert!(to_f32(img.data[(3 * 9 + 2) * 3]) > 1.0);
}

/// Values from a Sony ARW (`DSC09488.ARW`).
#[test]
fn sony_coefficients_follow_darktable() {
    let dist = [16., -2., 0., 2., 4., 6., 9., 12., 13., 14., 13., 10., 2., -10., -28., -52., -84.];
    let ca = [
        32., -640., -512., -384., -256., -256., -256., -256., -128., -128., -128., -128., -128., -128., 0., 0., 0.,
        -1024., -768., -640., -512., -512., -512., -640., -512., -512., -512., -512., -512., -512., -512., -512., -384.,
    ];
    let vig = [16., 0., 96., 448., 1024., 1744., 2560., 3472., 4432., 5424., 6432., 7440., 8432., 9424., 10400., 11344., 12256.];
    let r = sony_radial(&dist, &ca, &vig).unwrap();
    assert_eq!(r.knots_dist[0], 0.5 / 15.0);
    assert_eq!(r.cor[1][15], 1.0 - 84.0 / 16384.0);
    assert_eq!(r.cor[0][0], (1.0 - 2.0 / 16384.0) * (1.0 - 640.0 / 2097152.0));
    // No vignetting correction at the centre; about +0.9 EV in the corner.
    assert_eq!(r.vig[0], 1.0);
    let corner_ev = -r.vig[15].log2();
    assert!((0.85..0.95).contains(&corner_ev), "{corner_ev}");
    // Mismatched counts are not Sony data.
    assert!(sony_radial(&dist, &ca[..20], &vig).is_none());
}

/// Values from a Fujifilm X100V RAF: vignetting and CA, no distortion.
#[test]
fn fujifilm_coefficients_follow_darktable() {
    let knots = [502., 710., 870., 1004., 1123., 1230., 1328., 1420., 1506.].map(|k| k / 1420.0);
    let mut dist = vec![416.5];
    dist.extend(knots);
    dist.extend([0.0; 9]);
    let mut vig = vec![416.5];
    vig.extend(knots);
    vig.extend([95.7, 93.7, 91.9, 90.0, 87.9, 85.3, 82.0, 79.5, 69.6]);
    let mut ca = vec![416.5];
    ca.extend(knots);
    ca.extend([48., 42., 38., 32., 26., 22., 18., 12., 12.].map(|v| v / 65536.0));
    ca.extend([-28., -24., -14., -6., 2., 10., 16., 24., 24.].map(|v| v / 65536.0));
    ca.push(416.5);
    let r = fuji_radial(&dist, &ca, &vig).unwrap();
    // A knot without correction is added at the centre.
    assert_eq!((r.knots_vig[0], r.vig[0]), (0.0, 1.0));
    assert!((r.vig[1] - 0.957).abs() < 1e-12);
    // No distortion: green is unmoved, red and blue scale by their CA.
    assert!(r.cor[1].iter().all(|&m| m == 1.0));
    assert!(r.cor[0][15] > 1.0 && r.cor[2][15] > 1.0);
    assert!(fuji_radial(&dist[..18], &ca, &vig).is_none());
}

#[test]
#[ignore = "requires local camera RAWs: set SPEKTRAFILM_LENS_TEST_FILES (colon-separated)"]
fn real_files_have_profiles() {
    let files = std::env::var("SPEKTRAFILM_LENS_TEST_FILES").unwrap();
    for path in files.split(':') {
        let p = read_profile(Path::new(path));
        eprintln!("{path}: {:?}", p.as_ref().map(|p| (p.source, p.warps(), p.vignettes())));
        assert!(p.is_some(), "{path}");
    }
}

#[test]
#[ignore = "benchmark: cargo test --release -- --ignored bench_resample --nocapture"]
fn bench_resample() {
    let planes = [
        [0.99, -0.05, 0.01, 0.0, 0.0, 0.0],
        [0.99, -0.051, 0.01, 0.0, 0.0, 0.0],
        [0.99, -0.052, 0.01, 0.0, 0.0, 0.0],
    ];
    for (name, planes) in [("ca", planes.to_vec()), ("shared", vec![planes[1]])] {
        let p = dng_profile(&opcode_list(&[(1, warp_body(&planes, (0.5, 0.5)))]));
        let mut img = constant(6000, 4000, [0.2, 0.3, 0.4]);
        for _ in 0..3 {
            let t = std::time::Instant::now();
            apply(&mut img, &p, None);
            eprintln!("{name} 24 MP: {:?} ({} threads)", t.elapsed(), rayon::current_num_threads());
        }
    }
}
