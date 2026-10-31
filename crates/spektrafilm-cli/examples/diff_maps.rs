//! Pictures of how two renders differ, for a comparison page: a heat map of
//! CIEDE2000 over the frame (max-pooled, so a small spot shows), and crops of
//! A, B, their difference (×8 around grey) and an optional reference C, at the
//! largest difference inside the frame, at its edge, and where it's typical.
//!
//!   cargo run --release -p spektrafilm-cli --example diff_maps -- \
//!       A.f64 B.f64 A.png B.png C.png WIDTH HEIGHT OUT_DIR
//!
//! The .f64 dumps are `spektrafilm process --raw-out`; prints the crops'
//! places and their own ΔE statistics as JSON.

use image::{Rgb, RgbImage};

fn read_f64(path: &str) -> Vec<f64> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    bytes.chunks_exact(8).map(|b| f64::from_le_bytes(b.try_into().unwrap())).collect()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [a, b, pa, pb, pc, w, h, out] = <[String; 8]>::try_from(args).expect("A.f64 B.f64 A.png B.png C.png WIDTH HEIGHT OUT_DIR");
    let (w, h): (usize, usize) = (w.parse().unwrap(), h.parse().unwrap());
    let (a, b) = (read_f64(&a), read_f64(&b));
    let de: Vec<f64> = a.chunks_exact(3).zip(b.chunks_exact(3)).map(|(p, q)| if p == q { 0.0 } else { ciede2000(lab(p), lab(q)) }).collect();
    std::fs::create_dir_all(&out).unwrap();

    // The heat map at a quarter of the size, each pixel the largest ΔE of its
    // 4 × 4 block: black at 0, through blue (0.5) and yellow (1) to red from 2.
    let s = 4;
    let (hw, hh) = (w.div_ceil(s), h.div_ceil(s));
    let mut heat = RgbImage::new(hw as u32, hh as u32);
    for y in 0..hh {
        for x in 0..hw {
            let mut m: f64 = 0.0;
            for yy in y * s..((y + 1) * s).min(h) {
                for xx in x * s..((x + 1) * s).min(w) {
                    m = m.max(de[yy * w + xx]);
                }
            }
            heat.put_pixel(x as u32, y as u32, ramp(m));
        }
    }
    heat.save(format!("{out}/heat.png")).unwrap();

    let crop = 480;
    let worst = |inside: bool| {
        let margin = crop / 2 + 64;
        (0..w * h)
            .filter(|&i| {
                let (x, y) = (i % w, i / w);
                let interior = x >= margin && x + margin < w && y >= margin && y + margin < h;
                interior == inside
            })
            .max_by(|&i, &j| de[i].total_cmp(&de[j]))
            .unwrap()
    };
    let places = [("interior", worst(true)), ("edge", worst(false)), ("typical", (h / 2) * w + w / 2)];
    let images: Vec<RgbImage> = [&pa, &pb, &pc].iter().map(|p| image::open(p.as_str()).unwrap().to_rgb8()).collect();
    let mut report = Vec::new();
    for (name, i) in places {
        let (cx, cy) = (i % w, i / w);
        let x0 = cx.saturating_sub(crop / 2).min(w - crop);
        let y0 = cy.saturating_sub(crop / 2).min(h - crop);
        for (tag, img) in ["a", "b", "c"].iter().zip(&images) {
            image::imageops::crop_imm(img, x0 as u32, y0 as u32, crop as u32, crop as u32).to_image().save(format!("{out}/{name}-{tag}.png")).unwrap();
        }
        let mut diff = RgbImage::new(crop as u32, crop as u32);
        let mut patch = Vec::new();
        for y in 0..crop {
            for x in 0..crop {
                let k = (y0 + y) * w + x0 + x;
                patch.push(de[k]);
                let px = std::array::from_fn(|c| (128.0 + 8.0 * 255.0 * (b[k * 3 + c] - a[k * 3 + c])).round().clamp(0.0, 255.0) as u8);
                diff.put_pixel(x as u32, y as u32, Rgb(px));
            }
        }
        diff.save(format!("{out}/{name}-diff.png")).unwrap();
        patch.sort_by(f64::total_cmp);
        let n = patch.len();
        report.push(format!(
            "{{\"place\":\"{name}\",\"x\":{cx},\"y\":{cy},\"crop\":[{x0},{y0},{crop}],\"de_at\":{:.3},\"mean\":{:.3},\"p99\":{:.3},\"max\":{:.3}}}",
            de[i],
            patch.iter().sum::<f64>() / n as f64,
            patch[n * 99 / 100],
            patch[n - 1]
        ));
    }
    println!("[{}]", report.join(","));
}

fn ramp(v: f64) -> Rgb<u8> {
    let stops: [(f64, [f64; 3]); 5] = [(0.0, [12.0, 12.0, 16.0]), (0.5, [40.0, 90.0, 200.0]), (1.0, [240.0, 200.0, 40.0]), (2.0, [230.0, 40.0, 30.0]), (4.0, [255.0, 255.0, 255.0])];
    let v = v.min(4.0);
    let i = stops.iter().rposition(|s| s.0 <= v).unwrap().min(stops.len() - 2);
    let (p, q) = (stops[i], stops[i + 1]);
    let t = ((v - p.0) / (q.0 - p.0)).clamp(0.0, 1.0);
    Rgb(std::array::from_fn(|c| (p.1[c] + (q.1[c] - p.1[c]) * t) as u8))
}

// From src/bin/golden_compare.rs.
/// Lab (D65) of a clipped, sRGB-encoded pixel.
fn lab(rgb: &[f64]) -> [f64; 3] {
    // colour-science's sRGB normalised primary matrix and D65 (x 0.3127, y 0.3290).
    const M: [[f64; 3]; 3] = [
        [0.412_390_799_265_959_5, 0.357_584_339_383_878, 0.180_480_788_401_834_3],
        [0.212_639_005_871_510_36, 0.715_168_678_767_756, 0.072_192_315_360_733_71],
        [0.019_330_818_715_591_82, 0.119_194_779_794_625_98, 0.950_532_152_249_660_7],
    ];
    const WHITE: [f64; 3] = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
    let lin = |c: f64| {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let l = [lin(rgb[0]), lin(rgb[1]), lin(rgb[2])];
    let f = |t: f64| {
        const E: f64 = 216.0 / 24389.0;
        const K: f64 = 24389.0 / 27.0;
        if t > E { t.cbrt() } else { (K * t + 16.0) / 116.0 }
    };
    let xyz: Vec<f64> = (0..3)
        .map(|r| (M[r][0] * l[0] + M[r][1] * l[1] + M[r][2] * l[2]) / WHITE[r])
        .collect();
    let (fx, fy, fz) = (f(xyz[0]), f(xyz[1]), f(xyz[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// CIEDE2000 (Sharma, Wu & Dalal 2005), kL = kC = kH = 1.
fn ciede2000(p: [f64; 3], q: [f64; 3]) -> f64 {
    use std::f64::consts::PI;
    let deg = |r: f64| r * 180.0 / PI;
    let rad = |d: f64| d * PI / 180.0;
    let (l1, a1, b1) = (p[0], p[1], p[2]);
    let (l2, a2, b2) = (q[0], q[1], q[2]);
    let c_bar = ((a1 * a1 + b1 * b1).sqrt() + (a2 * a2 + b2 * b2).sqrt()) / 2.0;
    let g = 0.5 * (1.0 - (c_bar.powi(7) / (c_bar.powi(7) + 25f64.powi(7))).sqrt());
    let (a1p, a2p) = ((1.0 + g) * a1, (1.0 + g) * a2);
    let (c1p, c2p) = ((a1p * a1p + b1 * b1).sqrt(), (a2p * a2p + b2 * b2).sqrt());
    let hue = |b: f64, a: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            let h = deg(b.atan2(a));
            if h < 0.0 { h + 360.0 } else { h }
        }
    };
    let (h1p, h2p) = (hue(b1, a1p), hue(b2, a2p));
    let dlp = l2 - l1;
    let dcp = c2p - c1p;
    let dhp = if c1p * c2p == 0.0 {
        0.0
    } else if (h2p - h1p).abs() <= 180.0 {
        h2p - h1p
    } else if h2p - h1p > 180.0 {
        h2p - h1p - 360.0
    } else {
        h2p - h1p + 360.0
    };
    let dhp_big = 2.0 * (c1p * c2p).sqrt() * rad(dhp / 2.0).sin();
    let lp_bar = (l1 + l2) / 2.0;
    let cp_bar = (c1p + c2p) / 2.0;
    let hp_bar = if c1p * c2p == 0.0 {
        h1p + h2p
    } else if (h1p - h2p).abs() <= 180.0 {
        (h1p + h2p) / 2.0
    } else if h1p + h2p < 360.0 {
        (h1p + h2p + 360.0) / 2.0
    } else {
        (h1p + h2p - 360.0) / 2.0
    };
    let t = 1.0 - 0.17 * rad(hp_bar - 30.0).cos() + 0.24 * rad(2.0 * hp_bar).cos()
        + 0.32 * rad(3.0 * hp_bar + 6.0).cos()
        - 0.20 * rad(4.0 * hp_bar - 63.0).cos();
    let d_theta = 30.0 * (-((hp_bar - 275.0) / 25.0).powi(2)).exp();
    let rc = 2.0 * (cp_bar.powi(7) / (cp_bar.powi(7) + 25f64.powi(7))).sqrt();
    let sl = 1.0 + 0.015 * (lp_bar - 50.0).powi(2) / (20.0 + (lp_bar - 50.0).powi(2)).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let rt = -rad(2.0 * d_theta).sin() * rc;
    ((dlp / sl).powi(2) + (dcp / sc).powi(2) + (dhp_big / sh).powi(2)
        + rt * (dcp / sc) * (dhp_big / sh))
        .sqrt()
}

