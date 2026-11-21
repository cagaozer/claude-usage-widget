//! RAII wrappers so every GDI object is released exactly once.

use windows::core::PCWSTR;
use windows::Win32::Foundation::COLORREF;
use windows::Win32::Graphics::Gdi::*;

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

pub struct Font(pub HFONT);

impl Font {
    /// Segoe UI at `px` pixels tall (already DPI-scaled).
    pub fn segoe(px: i32, bold: bool) -> Font {
        let face: Vec<u16> = "Segoe UI\0".encode_utf16().collect();
        let h = unsafe {
            CreateFontW(
                -px,
                0,
                0,
                0,
                if bold { FW_SEMIBOLD.0 as i32 } else { FW_NORMAL.0 as i32 },
                0,
                0,
                0,
                DEFAULT_CHARSET,
                OUT_DEFAULT_PRECIS,
                CLIP_DEFAULT_PRECIS,
                CLEARTYPE_QUALITY,
                0,
                PCWSTR(face.as_ptr()),
            )
        };
        Font(h)
    }
}

impl Drop for Font {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

pub struct Brush(pub HBRUSH);

impl Brush {
    pub fn solid(c: COLORREF) -> Brush {
        Brush(unsafe { CreateSolidBrush(c) })
    }
}

impl Drop for Brush {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.0.into());
        }
    }
}

/// Off-screen buffer for flicker-free painting. Restores and frees everything on drop.
pub struct MemDc {
    pub dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
}

impl MemDc {
    pub fn new(target: HDC, w: i32, h: i32) -> MemDc {
        unsafe {
            let dc = CreateCompatibleDC(Some(target));
            let bitmap = CreateCompatibleBitmap(target, w, h);
            let old = SelectObject(dc, bitmap.into());
            MemDc { dc, bitmap, old }
        }
    }
}

impl Drop for MemDc {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Selects a font into a DC for the guard's lifetime.
pub struct Selected {
    dc: HDC,
    old: HGDIOBJ,
}

impl Selected {
    pub fn font(dc: HDC, f: &Font) -> Selected {
        Selected { dc, old: unsafe { SelectObject(dc, f.0.into()) } }
    }
}

impl Drop for Selected {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
        }
    }
}
