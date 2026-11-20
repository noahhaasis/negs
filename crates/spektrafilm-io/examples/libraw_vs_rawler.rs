//! Prints what rawler and LibRaw read from each RAW: size, CFA, black and
//! white levels, white balance, colour matrix. For checking the LibRaw
//! fallback against rawler on files both decode.
fn main() {
    for path in std::env::args().skip(1) {
        println!("== {path}");
        let bytes = std::fs::read(&path).unwrap();
        let src = rawler::rawsource::RawSource::new_from_slice(&bytes);
        match rawler::decode(&src, &Default::default()) {
            Ok(r) => println!(
                "rawler  {}x{} cfa {:?} black {:?} white {:?} wb {:?} crop {:?} active {:?}\n        xyz {:?}",
                r.width,
                r.height,
                r.camera.cfa.name,
                r.blacklevel.as_vec(),
                r.whitelevel.0,
                r.wb_coeffs,
                r.crop_area,
                r.active_area,
                r.color_matrix
            ),
            Err(e) => println!("rawler  {e:?}"),
        }
        match negs_libraw::unpack(&bytes, negs_libraw::Area::Visible) {
            Ok(m) => println!(
                "libraw  {}x{} (raw {}x{} at {},{}) cfa {} black {:?} white {} cam_mul {:?} crop {:?} decoder {}\n        xyz {:?}",
                m.width, m.height, m.raw_width, m.raw_height, m.left_margin, m.top_margin, m.cfa, m.black, m.white, m.cam_mul, m.crop, m.decoder, m.cam_xyz
            ),
            Err(e) => println!("libraw  {e}"),
        }
    }
}
