//! model/compare.rs's pure helpers, copied verbatim from the frozen
//! oracle/model_compare.rs.txt (the rest of that module is the model's frame
//! pipeline and can't build here); tests/model_compare.rs checks the copies still
//! read as the source does.

/// `image` at half its size, each pixel the mean of four.
fn halved<C: std::ops::Deref<Target = [u8]>>(image: &image::ImageBuffer<image::Rgba<u8>, C>) -> image::RgbaImage {
    let (width, height) = image.dimensions();
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let source: &[u8] = image.as_raw();
    let stride = width as usize * 4;
    let mut out = vec![0u8; w as usize * h as usize * 4];
    for (y, row) in out.chunks_exact_mut(w as usize * 4).enumerate() {
        let y0 = (2 * y).min(height as usize - 1);
        let y1 = (2 * y + 1).min(height as usize - 1);
        for (x, pixel) in row.chunks_exact_mut(4).enumerate() {
            let x0 = (2 * x).min(width as usize - 1) * 4;
            let x1 = (2 * x + 1).min(width as usize - 1) * 4;
            for c in 0..4 {
                let sum = source[y0 * stride + x0 + c] as u32 + source[y0 * stride + x1 + c] as u32 + source[y1 * stride + x0 + c] as u32 + source[y1 * stride + x1 + c] as u32;
                pixel[c] = ((sum + 2) / 4) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(w, h, out).expect("sized")
}

/// The camera's JPEG laid over the RAW's frame (`width` × `height`), so it
/// lines up with the developed image: one of the same shape (within 1%) is
/// stretched to it, another sits centred on black.
pub fn over_frame(rgba: &[u8], width: u32, height: u32, frame: (u32, u32)) -> Option<(Vec<u8>, u32, u32)> {
    let picture = image::RgbaImage::from_raw(width, height, rgba.to_vec())?;
    let aspect = frame.0 as f64 / frame.1.max(1) as f64;
    let long = width.max(height) as f64;
    let out = if aspect >= 1.0 { (long as u32, (long / aspect).round() as u32) } else { ((long * aspect).round() as u32, long as u32) };
    let same = ((width as f64 / height as f64) / aspect - 1.0).abs() < 0.01;
    if same {
        let resized = image::imageops::resize(&picture, out.0.max(1), out.1.max(1), image::imageops::FilterType::Triangle);
        return Some((resized.into_raw(), out.0, out.1));
    }
    let scale = (out.0 as f64 / width as f64).min(out.1 as f64 / height as f64);
    let (w, h) = (((width as f64 * scale).round() as u32).max(1), ((height as f64 * scale).round() as u32).max(1));
    let resized = image::imageops::resize(&picture, w, h, image::imageops::FilterType::Triangle);
    let mut canvas = image::RgbaImage::from_pixel(out.0.max(1), out.1.max(1), image::Rgba([0, 0, 0, 255]));
    image::imageops::overlay(&mut canvas, &resized, ((out.0 - w.min(out.0)) / 2) as i64, ((out.1 - h.min(out.1)) / 2) as i64);
    Some((canvas.into_raw(), out.0, out.1))
}


/// The copied `halved`, for the tests (the copy itself stays private, as written).
pub fn halved_oracle(image: &image::RgbaImage) -> image::RgbaImage {
    halved(image)
}
