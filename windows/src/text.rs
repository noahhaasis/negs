//! Text for the canvas (badges, the straighten dial), rasterised with GDI
//! into a coverage mask the GPU colours. Greyscale antialiasing, so it can be
//! drawn on any background.

use windows::Win32::Graphics::Gdi::*;
use windows::core::PCWSTR;

use crate::platform::wide;

pub struct Mask {
    pub coverage: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// `text` in Segoe UI at `pixels` height (the font's em size in device pixels).
pub fn render(text: &str, pixels: i32, weight: i32) -> Option<Mask> {
    unsafe {
        let screen = GetDC(None);
        let dc = CreateCompatibleDC(screen);
        ReleaseDC(None, screen);
        let face = wide("Segoe UI");
        let mut lf = LOGFONTW { lfHeight: -pixels, lfWeight: weight, lfQuality: ANTIALIASED_QUALITY, ..Default::default() };
        lf.lfFaceName[..face.len()].copy_from_slice(&face);
        let font = CreateFontIndirectW(&lf);
        let old = SelectObject(dc, font);
        let text = wide(text);
        let chars = &text[..text.len() - 1];
        let mut size = windows::Win32::Foundation::SIZE::default();
        let _ = GetTextExtentPoint32W(dc, chars, &mut size);
        let (width, height) = ((size.cx + 2).max(1) as u32, (size.cy + 2).max(1) as u32);
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = CreateDIBSection(dc, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let old_bitmap = SelectObject(dc, bitmap);
        SetBkMode(dc, OPAQUE);
        SetBkColor(dc, windows::Win32::Foundation::COLORREF(0));
        SetTextColor(dc, windows::Win32::Foundation::COLORREF(0xffffff));
        let _ = TextOutW(dc, 1, 1, chars);
        let _ = GdiFlush();
        let pixels = std::slice::from_raw_parts(bits as *const u8, (width * height * 4) as usize);
        // White on black: the green channel is the coverage.
        let coverage = pixels.chunks_exact(4).map(|p| p[1]).collect();
        SelectObject(dc, old_bitmap);
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap);
        let _ = DeleteObject(font);
        let _ = DeleteDC(dc);
        let _ = PCWSTR::null();
        Some(Mask { coverage, width, height })
    }
}
