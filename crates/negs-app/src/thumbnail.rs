//! Decodes for the canvas and the film strip, never through the engine:
//! the placeholder shown while a photograph develops, the original for Show
//! Original (non-RAW), and the strip's and grid's thumbnails. On Windows,
//! JPEG, PNG and TIFF go through WIC with the EXIF orientation applied; on
//! macOS/Linux they decode through `image`. RAW files always use their
//! embedded preview (never a full development).

use std::path::Path;

use anyhow::{Context, Result, bail};

use crate::crop::Size;
use crate::model::is_raw;
use crate::picture::Picture;

pub struct Decoded {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The photograph's full size, oriented.
    pub full: Size,
}

#[cfg(windows)]
mod backend {
    use super::*;
    use windows::Win32::Foundation::GENERIC_READ;
    use windows::Win32::Graphics::Imaging::*;
    use windows::Win32::System::Com::*;
    use windows::core::{GUID, PCWSTR, PROPVARIANT};
    use crate::platform::wide;

    /// Initialises COM on a worker thread for WIC and the shell.
    pub fn com_thread() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        }
    }

    pub(crate) fn factory() -> Result<IWICImagingFactory> {
        unsafe { Ok(CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?) }
    }

    fn transform(orientation: u16) -> WICBitmapTransformOptions {
        match orientation {
            2 => WICBitmapTransformFlipHorizontal,
            3 => WICBitmapTransformRotate180,
            4 => WICBitmapTransformFlipVertical,
            5 => WICBitmapTransformOptions(WICBitmapTransformRotate90.0 | WICBitmapTransformFlipHorizontal.0),
            6 => WICBitmapTransformRotate90,
            7 => WICBitmapTransformOptions(WICBitmapTransformRotate270.0 | WICBitmapTransformFlipHorizontal.0),
            8 => WICBitmapTransformRotate270,
            _ => WICBitmapTransformRotate0,
        }
    }

    fn orientation(frame: &IWICBitmapFrameDecode) -> u16 {
        unsafe {
            let Ok(reader) = frame.GetMetadataQueryReader() else { return 1 };
            for query in ["/app1/ifd/{ushort=274}", "/ifd/{ushort=274}"] {
                let name = wide(query);
                let mut value = PROPVARIANT::default();
                if reader.GetMetadataByName(PCWSTR(name.as_ptr()), &mut value).is_ok() {
                    if let Ok(orientation) = u16::try_from(&value) {
                        return orientation;
                    }
                }
            }
            1
        }
    }

    pub fn decode_file(path: &Path, max_pixels: u32, color_managed: bool) -> Result<Decoded> {
        unsafe {
            let factory = factory()?;
            let name = wide(path);
            let decoder = factory.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
            decode_frame(&factory, &decoder.GetFrame(0)?, max_pixels, color_managed, None)
        }
    }

    pub fn decode_jpeg(bytes: &[u8], max_pixels: u32, orientation: Option<u16>) -> Result<Decoded> {
        unsafe {
            let factory = factory()?;
            let stream = factory.CreateStream()?;
            stream.InitializeFromMemory(bytes)?;
            let decoder = factory.CreateDecoderFromStream(&stream, &GUID_ContainerFormatJpeg, WICDecodeMetadataCacheOnDemand)?;
            decode_frame(&factory, &decoder.GetFrame(0)?, max_pixels, false, orientation)
        }
    }

    fn decode_frame(factory: &IWICImagingFactory, frame: &IWICBitmapFrameDecode, max_pixels: u32, color_managed: bool, orientation_given: Option<u16>) -> Result<Decoded> {
        unsafe {
            let (mut w, mut h) = (0, 0);
            frame.GetSize(&mut w, &mut h)?;
            let orientation = orientation_given.unwrap_or_else(|| orientation(frame));
            let turned = orientation >= 5;
            let full = if turned { Size::new(h as f64, w as f64) } else { Size::new(w as f64, h as f64) };
            let scale = (w.max(h) as f64 / max_pixels as f64).max(1.0);
            let (sw, sh) = (((w as f64 / scale).round() as u32).max(1), ((h as f64 / scale).round() as u32).max(1));
            let mut source: IWICBitmapSource = frame.clone().into();
            if (sw, sh) != (w, h) {
                let scaler = factory.CreateBitmapScaler()?;
                scaler.Initialize(&source, sw, sh, WICBitmapInterpolationModeFant)?;
                source = scaler.into();
            }
            if color_managed {
                if let Some(converted) = to_srgb(factory, frame, &source) {
                    source = converted;
                }
            }
            if orientation > 1 {
                if orientation >= 5 {
                    source = factory.CreateBitmapFromSource(&source, WICBitmapCacheOnLoad)?.into();
                }
                let rotator = factory.CreateBitmapFlipRotator()?;
                rotator.Initialize(&source, transform(orientation))?;
                source = rotator.into();
            }
            let converter = factory.CreateFormatConverter()?;
            converter.Initialize(&source, &GUID_WICPixelFormat32bppRGBA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
            let (mut ow, mut oh) = (0, 0);
            converter.GetSize(&mut ow, &mut oh)?;
            let mut rgba = vec![0u8; ow as usize * oh as usize * 4];
            converter.CopyPixels(std::ptr::null(), ow * 4, &mut rgba)?;
            Ok(Decoded { rgba, width: ow, height: oh, full })
        }
    }

    fn to_srgb(factory: &IWICImagingFactory, frame: &IWICBitmapFrameDecode, source: &IWICBitmapSource) -> Option<IWICBitmapSource> {
        unsafe {
            let mut count = 0;
            frame.GetColorContexts(&mut [], &mut count).ok()?;
            if count == 0 {
                return None;
            }
            let mut contexts: Vec<Option<IWICColorContext>> = (0..count).map(|_| factory.CreateColorContext().ok()).collect();
            if contexts.iter().any(Option::is_none) {
                return None;
            }
            frame.GetColorContexts(&mut contexts, &mut count).ok()?;
            let input = contexts.into_iter().next()??;
            let srgb = factory.CreateColorContext().ok()?;
            srgb.InitializeFromExifColorSpace(1).ok()?;
            let transformer = factory.CreateColorTransformer().ok()?;
            let format: GUID = GUID_WICPixelFormat32bppRGBA;
            transformer.Initialize(source, &input, &srgb, &format).ok()?;
            Some(transformer.into())
        }
    }

    pub fn non_raw_header_ratio(path: &Path) -> Option<f64> {
        unsafe {
            let factory = factory().ok()?;
            let name = wide(path);
            let decoder = factory.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
            let frame = decoder.GetFrame(0).ok()?;
            let (mut w, mut h) = (0, 0);
            frame.GetSize(&mut w, &mut h).ok()?;
            if w == 0 || h == 0 {
                return None;
            }
            let turned = orientation(&frame) >= 5;
            Some(if turned { h as f64 / w as f64 } else { w as f64 / h as f64 })
        }
    }
}

#[cfg(not(windows))]
mod backend {
    use super::*;
    use image::metadata::Orientation;
    use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, imageops};

    pub fn com_thread() {}

    fn orient_exif(image: DynamicImage, orientation: u16) -> DynamicImage {
        match orientation {
            2 => image.fliph(),
            3 => image.rotate180(),
            4 => image.flipv(),
            5 => image.rotate90().fliph(),
            6 => image.rotate90(),
            7 => image.rotate270().fliph(),
            8 => image.rotate270(),
            _ => image,
        }
    }

    fn finish_decode(image: DynamicImage, max_pixels: u32, orientation_exif: u16) -> Result<Decoded> {
        let (w, h) = (image.width(), image.height());
        if w == 0 || h == 0 {
            bail!("Empty image");
        }
        let turned = orientation_exif >= 5;
        let full = if turned { Size::new(h as f64, w as f64) } else { Size::new(w as f64, h as f64) };
        let scale = (w.max(h) as f64 / max_pixels as f64).max(1.0);
        let (sw, sh) = (((w as f64 / scale).round() as u32).max(1), ((h as f64 / scale).round() as u32).max(1));
        let mut image = if (sw, sh) != (w, h) {
            DynamicImage::ImageRgba8(imageops::resize(&image.to_rgba8(), sw, sh, imageops::FilterType::Triangle))
        } else {
            image
        };
        if orientation_exif > 1 {
            image = orient_exif(image, orientation_exif);
        }
        let rgba = image.to_rgba8();
        let (ow, oh) = rgba.dimensions();
        Ok(Decoded { rgba: rgba.into_raw(), width: ow, height: oh, full })
    }

    pub fn decode_file(path: &Path, max_pixels: u32, _color_managed: bool) -> Result<Decoded> {
        let mut decoder = ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
        let exif = decoder.orientation().unwrap_or(Orientation::NoTransforms).to_exif() as u16;
        let image = DynamicImage::from_decoder(decoder)?;
        finish_decode(image, max_pixels, exif)
    }

    pub fn decode_jpeg(bytes: &[u8], max_pixels: u32, orientation_given: Option<u16>) -> Result<Decoded> {
        let mut decoder = ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Jpeg).into_decoder()?;
        let exif = orientation_given.unwrap_or_else(|| decoder.orientation().unwrap_or(Orientation::NoTransforms).to_exif() as u16);
        let image = DynamicImage::from_decoder(decoder)?;
        finish_decode(image, max_pixels, exif)
    }

    pub fn non_raw_header_ratio(path: &Path) -> Option<f64> {
        let mut decoder = ImageReader::open(path).ok()?.with_guessed_format().ok()?.into_decoder().ok()?;
        let (w, h) = decoder.dimensions();
        if w == 0 || h == 0 {
            return None;
        }
        let exif = decoder.orientation().unwrap_or(Orientation::NoTransforms).to_exif();
        let turned = exif >= 5;
        Some(if turned { h as f64 / w as f64 } else { w as f64 / h as f64 })
    }
}

pub use backend::{com_thread, decode_file, decode_jpeg};
#[cfg(windows)]
pub(crate) use backend::factory;

/// Windows compatibility aliases.
#[cfg(windows)]
pub fn wic(path: &Path, max_pixels: u32, color_managed: bool) -> Result<Decoded> {
    decode_file(path, max_pixels, color_managed)
}

#[cfg(windows)]
pub fn wic_jpeg(bytes: &[u8], max_pixels: u32, orientation: Option<u16>) -> Result<Decoded> {
    decode_jpeg(bytes, max_pixels, orientation)
}

/// A RAW file's embedded preview, oriented: the smallest JPEG in it big
/// enough, found from its header and decoded scaled; else (CR3, a
/// file it can't read) the one rawler finds, decoded whole and shrunk.
pub fn raw_preview(path: &Path, max_pixels: u32) -> Result<Decoded> {
    let started = std::time::Instant::now();
    if let Some(previews) = crate::embedded_preview::find(path) {
        if let Some(jpeg) = previews.for_size(max_pixels) {
            let found = started.elapsed();
            let decoded = crate::embedded_preview::bytes(path, &jpeg)
                .context("Short preview")
                .and_then(|b| decode_jpeg(&b, max_pixels, previews.orientation));
            crate::trace::event("raw_preview", || {
                let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
                serde_json::json!({"fast": true, "find_ms": ms(found), "decode_ms": ms(started.elapsed() - found), "preview": [jpeg.width, jpeg.height], "ok": decoded.is_ok()})
            });
            if let Ok(decoded) = decoded {
                return Ok(Decoded { full: Size::new(decoded.width as f64, decoded.height as f64), ..decoded });
            }
        }
    }
    rawler_preview(path, max_pixels)
}

/// rawler's preview, decoded whole and shrunk.
fn rawler_preview(path: &Path, max_pixels: u32) -> Result<Decoded> {
    let started = std::time::Instant::now();
    let source = rawler::rawsource::RawSource::new(path)?;
    let decoder = rawler::get_decoder(&source)?;
    let params = rawler::decoders::RawDecodeParams::default();
    let opened = started.elapsed();
    let image = decoder
        .preview_image(&source, &params)
        .ok()
        .flatten()
        .or_else(|| decoder.thumbnail_image(&source, &params).ok().flatten())
        .context("No embedded preview")?;
    let decoded = started.elapsed();
    let (pw, ph) = (image.width(), image.height());
    let orientation = decoder.raw_metadata(&source, &params).ok().and_then(|m| m.exif.orientation).unwrap_or(1);
    let read_metadata = started.elapsed();
    let mut image = image.thumbnail(max_pixels, max_pixels);
    crate::trace::event("raw_preview", || {
        let ms = |d: std::time::Duration| d.as_secs_f64() * 1000.0;
        serde_json::json!({"open_ms": ms(opened), "decode_ms": ms(decoded - opened), "metadata_ms": ms(read_metadata - decoded), "shrink_ms": ms(started.elapsed() - read_metadata), "preview": [pw, ph]})
    });
    image = match orientation {
        2 => image.fliph(),
        3 => image.rotate180(),
        4 => image.flipv(),
        5 => image.rotate90().fliph(),
        6 => image.rotate90(),
        7 => image.rotate270().fliph(),
        8 => image.rotate270(),
        _ => image,
    };
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    if width == 0 || height == 0 {
        bail!("Empty preview");
    }
    Ok(Decoded { rgba: rgba.into_raw(), width, height, full: Size::new(width as f64, height as f64) })
}

/// The photograph at once, laid out at its full size so the canvas doesn't
/// jump when the developed render arrives. RAW: the embedded preview.
pub fn placeholder(path: &Path) -> Option<(Picture, Size)> {
    let (rgba, width, height, full) = placeholder_pixels(path)?;
    Some((Picture::new(rgba, width, height), full))
}

/// `placeholder`'s RGBA pixels, kept on the CPU (Compare's stand-in).
pub fn placeholder_pixels(path: &Path) -> Option<(Vec<u8>, u32, u32, Size)> {
    com_thread();
    let decoded = if is_raw(path) { raw_preview(path, 2048).ok()? } else { decode_file(path, 2048, false).ok()? };
    Some((decoded.rgba, decoded.width, decoded.height, decoded.full))
}

/// Show Original for JPEG, PNG and TIFF: the file itself with its colour
/// profile, at the preview's size.
pub fn original(path: &Path, max_pixels: u32) -> Result<(Vec<u8>, u32, u32)> {
    com_thread();
    let decoded = decode_file(path, max_pixels, true)?;
    Ok((decoded.rgba, decoded.width, decoded.height))
}

/// The photograph's shape as shown upright, from its header alone: a RAW's
/// largest embedded preview (when its IFDs say how it's turned), else the
/// file's own size and orientation from its header.
pub fn header_ratio(path: &Path) -> Option<f64> {
    if is_raw(path) {
        let previews = crate::embedded_preview::find(path)?;
        previews.orientation?;
        return previews.ratio();
    }
    backend::non_raw_header_ratio(path)
}

/// A strip or grid thumbnail, as BGRA: the embedded preview of a RAW, the
/// file itself otherwise, `pixels` on its long side at most.
pub fn read(path: &Path, pixels: u32) -> Option<(Vec<u8>, u32, u32)> {
    let decoded = if is_raw(path) { raw_preview(path, pixels).ok()? } else { decode_file(path, pixels, false).ok()? };
    let mut bgra = decoded.rgba;
    for p in bgra.chunks_exact_mut(4) {
        p.swap(0, 2);
        p[3] = 255;
    }
    Some((bgra, decoded.width, decoded.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `NEGS_PREVIEW_FOLDER=<folder>`: each RAW's preview from its header
    /// against rawler's, as the strip reads them: the same shape and
    /// orientation, and nearly the same picture.
    #[test]
    #[ignore]
    fn header_previews_match_rawlers() {
        let Some(folder) = std::env::var_os("NEGS_PREVIEW_FOLDER") else { return };
        com_thread();
        let mut paths: Vec<_> = std::fs::read_dir(folder).unwrap().flatten().map(|e| e.path()).filter(|p| is_raw(p)).collect();
        paths.sort();
        let mut failures = vec![];
        for path in &paths {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            let started = std::time::Instant::now();
            let fast = crate::embedded_preview::find(path)
                .and_then(|p| p.for_size(432).map(|j| (p.clone(), j)))
                .and_then(|(p, j)| crate::embedded_preview::bytes(path, &j).map(|b| (p, b)))
                .map(|(p, b)| decode_jpeg(&b, 432, p.orientation));
            let fast_ms = started.elapsed().as_secs_f64() * 1000.0;
            let started = std::time::Instant::now();
            let slow = rawler_preview(path, 432);
            let slow_ms = started.elapsed().as_secs_f64() * 1000.0;
            match (fast, slow) {
                (Some(Ok(a)), Ok(b)) => {
                    let ratio = |d: &Decoded| d.width as f64 / d.height as f64;
                    let shape = (ratio(&a) - ratio(&b)).abs() / ratio(&b);
                    let diff = if (a.width, a.height) == (b.width, b.height) {
                        a.rgba.iter().zip(&b.rgba).map(|(x, y)| (*x as f64 - *y as f64).abs()).sum::<f64>() / a.rgba.len() as f64
                    } else {
                        f64::NAN
                    };
                    println!("{fast_ms:>6.1} ms vs {slow_ms:>6.1} ms {}x{} vs {}x{} shape {shape:.4} diff {diff:.2} {name}", a.width, a.height, b.width, b.height);
                    if shape > 0.02 || diff > 12.0 {
                        failures.push(name);
                    }
                }
                (None, Ok(_)) => println!("  none   rawler {slow_ms:.0} ms {name}"),
                (fast, slow) => println!("  fast {:?} slow {:?} {name}", fast.map(|f| f.err().map(|e| e.to_string())), slow.err().map(|e| e.to_string())),
            }
        }
        assert!(failures.is_empty(), "{failures:?}");
    }
}
