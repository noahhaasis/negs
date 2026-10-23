//! Decodes for the canvas and the film strip, never through the engine:
//! the placeholder shown while a photograph develops, the original for Show
//! Original (non-RAW), and film strip thumbnails. JPEG, PNG and TIFF go
//! through WIC with the EXIF orientation applied, like the engine's loader;
//! RAW files use their embedded preview (never a full development).

use std::path::Path;

use anyhow::{Context, Result, bail};
use windows::Win32::Foundation::GENERIC_READ;
use windows::Win32::Graphics::Imaging::*;
use windows::Win32::System::Com::*;
use windows::core::{GUID, PCWSTR, PROPVARIANT};

use crate::crop::Size;
use crate::model::is_raw;
use crate::picture::Picture;
use crate::platform::wide;

/// Initialises COM on a worker thread for WIC and the shell.
pub fn com_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
    }
}

fn factory() -> Result<IWICImagingFactory> {
    unsafe { Ok(CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?) }
}

/// EXIF orientation as a WIC transform.
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

pub struct Decoded {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    /// The photograph's full size, oriented.
    pub full: Size,
}

/// Decodes `path` at most `max_pixels` on its long side, oriented, as RGBA;
/// `color_managed` converts an embedded profile to sRGB.
pub fn wic(path: &Path, max_pixels: u32, color_managed: bool) -> Result<Decoded> {
    unsafe {
        let factory = factory()?;
        let name = wide(path);
        let decoder = factory.CreateDecoderFromFilename(PCWSTR(name.as_ptr()), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand)?;
        let frame = decoder.GetFrame(0)?;
        let (mut w, mut h) = (0, 0);
        frame.GetSize(&mut w, &mut h)?;
        let orientation = orientation(&frame);
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
            if let Some(converted) = to_srgb(&factory, &frame, &source) {
                source = converted;
            }
        }
        if orientation > 1 {
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

/// The frame's embedded profile converted to sRGB, if it has one.
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

/// A RAW file's embedded preview, oriented.
pub fn raw_preview(path: &Path, max_pixels: u32) -> Result<Decoded> {
    let source = rawler::rawsource::RawSource::new(path)?;
    let decoder = rawler::get_decoder(&source)?;
    let params = rawler::decoders::RawDecodeParams::default();
    let image = decoder
        .preview_image(&source, &params)
        .ok()
        .flatten()
        .or_else(|| decoder.thumbnail_image(&source, &params).ok().flatten())
        .context("No embedded preview")?;
    let orientation = decoder.raw_metadata(&source, &params).ok().and_then(|m| m.exif.orientation).unwrap_or(1);
    let mut image = image.thumbnail(max_pixels, max_pixels);
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
    com_thread();
    let decoded = if is_raw(path) { raw_preview(path, 2048).ok()? } else { wic(path, 2048, false).ok()? };
    Some((Picture::new(decoded.rgba, decoded.width, decoded.height), decoded.full))
}

/// Show Original for JPEG, PNG and TIFF: the file itself with its colour
/// profile, at the preview's size.
pub fn original(path: &Path, max_pixels: u32) -> Result<Picture> {
    com_thread();
    let decoded = wic(path, max_pixels, true)?;
    Ok(Picture::new(decoded.rgba, decoded.width, decoded.height))
}

/// A film strip thumbnail: Explorer's cached thumbnail (which uses an
/// installed RAW codec), else the embedded preview for RAW files.
pub fn strip_thumbnail(path: &Path, pixels: i32) -> Option<(Vec<u8>, u32, u32)> {
    use windows::Win32::Graphics::Gdi::*;
    use windows::Win32::UI::Shell::*;
    unsafe {
        let name = wide(path);
        let shell: Option<IShellItemImageFactory> = SHCreateItemFromParsingName(PCWSTR(name.as_ptr()), None).ok();
        if let Some(Ok(bitmap)) = shell.map(|f| f.GetImage(windows::Win32::Foundation::SIZE { cx: pixels, cy: pixels }, SIIGBF_THUMBNAILONLY | SIIGBF_BIGGERSIZEOK)) {
            let mut info = BITMAP::default();
            GetObjectW(bitmap, std::mem::size_of::<BITMAP>() as i32, Some(&mut info as *mut _ as *mut _));
            let (w, h) = (info.bmWidth.max(1) as u32, info.bmHeight.unsigned_abs().max(1));
            let header = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bgra = vec![0u8; (w * h * 4) as usize];
            let dc = GetDC(None);
            let lines = GetDIBits(dc, bitmap, 0, h, Some(bgra.as_mut_ptr() as *mut _), &header as *const _ as *mut _, DIB_RGB_COLORS);
            ReleaseDC(None, dc);
            let _ = DeleteObject(bitmap);
            if lines > 0 {
                for p in bgra.chunks_exact_mut(4) {
                    p.swap(0, 2);
                    p[3] = 255;
                }
                return Some((bgra, w, h));
            }
        }
    }
    let decoded = if is_raw(path) { raw_preview(path, pixels as u32).ok()? } else { wic(path, pixels as u32, false).ok()? };
    Some((decoded.rgba, decoded.width, decoded.height))
}
