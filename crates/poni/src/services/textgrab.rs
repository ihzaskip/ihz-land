//! Text extractor, as in PowerToys: Alt+Shift+T (or the menu) freezes the
//! screen, a box dragged over some text is read by Windows' own OCR
//! (`Windows.Media.Ocr`, the engine PowerToys uses) and the island puts the
//! text on the clipboard (`AppEvent::TextGrab`). Esc or a right-click cancels.
//!
//! Nothing runs at rest: each grab is one short thread with its own window,
//! gone once the box is drawn. OCR reads in the languages of Windows' own
//! language list, so nothing is downloaded; its library loads on first use.

use crate::events::{AppEvent, EventSink};
use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use windows::core::w;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::*;

/// One grab at a time: the hotkey pressed again mid-grab does nothing.
static BUSY: AtomicBool = AtomicBool::new(false);

/// Freeze the screen and let a box be drawn; the text comes back as
/// `AppEvent::TextGrab` (nothing comes back when cancelled).
pub fn start(sink: EventSink) {
    if BUSY.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let got = unsafe { grab() };
        BUSY.store(false, Ordering::SeqCst);
        if let Some(got) = got {
            sink.send(AppEvent::TextGrab(got));
        }
    });
}

/// A 32-bit top-down DIB selected into its own memory DC.
struct Dib {
    dc: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    px: *mut u32,
    len: usize,
}

impl Dib {
    unsafe fn new(w: i32, h: i32) -> Option<Dib> {
        let bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bmp = CreateDIBSection(None, &bi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, bmp);
        Some(Dib { dc, bmp, old, px: bits as *mut u32, len: (w * h) as usize })
    }

    fn px(&self) -> &[u32] {
        unsafe { std::slice::from_raw_parts(self.px, self.len) }
    }

    fn px_mut(&mut self) -> &mut [u32] {
        unsafe { std::slice::from_raw_parts_mut(self.px, self.len) }
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bmp);
            let _ = DeleteDC(self.dc);
        }
    }
}

/// The frozen screen (as it is and dimmed) and the box being drawn.
struct Overlay {
    bright: Dib,
    dim: Dib,
    from: Option<POINT>,
    to: POINT,
    done: Option<RECT>,
}

thread_local! {
    static OVERLAY: RefCell<Option<Overlay>> = const { RefCell::new(None) };
}

impl Overlay {
    fn sel(&self) -> RECT {
        let Some(a) = self.from else { return RECT::default() };
        let b = self.to;
        RECT { left: a.x.min(b.x), top: a.y.min(b.y), right: a.x.max(b.x), bottom: a.y.max(b.y) }
    }
}

/// None: cancelled. Some(Err): the screen or OCR failed.
unsafe fn grab() -> Option<Result<String, String>> {
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
    let (w, h) = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));
    let (Some(bright), Some(mut dim)) = (Dib::new(w, h), Dib::new(w, h)) else {
        return Some(Err("couldn't copy the screen".into()));
    };
    let screen = GetDC(None);
    let _ = BitBlt(bright.dc, 0, 0, w, h, screen, x, y, SRCCOPY | CAPTUREBLT);
    ReleaseDC(None, screen);
    for (d, s) in dim.px_mut().iter_mut().zip(bright.px()) {
        *d = (*s >> 1) & 0x007f_7f7f;
    }
    OVERLAY.with(|o| *o.borrow_mut() = Some(Overlay { bright, dim, from: None, to: POINT::default(), done: None }));

    let inst = GetModuleHandleW(None).unwrap_or_default();
    let class = w!("PoniTextGrab");
    let wc = WNDCLASSW {
        lpfnWndProc: Some(wndproc),
        hInstance: inst.into(),
        hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
        lpszClassName: class,
        ..Default::default()
    };
    RegisterClassW(&wc); // 0 on the second grab: already registered
    let hwnd = CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW, class, w!("Extract text"), WS_POPUP | WS_VISIBLE, x, y, w, h, None, None, inst, None);
    if let Ok(hwnd) = hwnd {
        let _ = SetForegroundWindow(hwnd);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    let ov = OVERLAY.with(|o| o.borrow_mut().take())?;
    let r = ov.done?;
    let (bw, bh) = ((r.right - r.left) as usize, (r.bottom - r.top) as usize);
    if bw < 4 || bh < 4 {
        return None; // a click, not a box
    }
    let src = ov.bright.px();
    let mut px = Vec::with_capacity(bw * bh * 4);
    for row in r.top as usize..r.bottom as usize {
        for p in &src[row * w as usize + r.left as usize..][..bw] {
            px.extend_from_slice(&(p | 0xff00_0000).to_le_bytes());
        }
    }
    drop(ov);
    Some(read(px, bw as u32, bh as u32).map_err(|e| {
        crate::pwarn!("text extractor: {e}");
        "Add a language with text recognition in Windows Settings".to_string()
    }))
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let pt = || POINT { x: (lp.0 & 0xffff) as i16 as i32, y: ((lp.0 >> 16) & 0xffff) as i16 as i32 };
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(hwnd, &mut ps);
            OVERLAY.with(|o| {
                if let Some(ov) = o.borrow().as_ref() {
                    let s = ov.sel();
                    let u = ps.rcPaint;
                    // Dimmed around the box, as it is inside it, then a white frame.
                    ExcludeClipRect(dc, s.left, s.top, s.right, s.bottom);
                    let _ = BitBlt(dc, u.left, u.top, u.right - u.left, u.bottom - u.top, ov.dim.dc, u.left, u.top, SRCCOPY);
                    SelectClipRgn(dc, None);
                    if s.right > s.left {
                        let _ = BitBlt(dc, s.left, s.top, s.right - s.left, s.bottom - s.top, ov.bright.dc, s.left, s.top, SRCCOPY);
                        let f = RECT { left: s.left - 1, top: s.top - 1, right: s.right + 1, bottom: s.bottom + 1 };
                        FrameRect(dc, &f, HBRUSH(GetStockObject(WHITE_BRUSH).0));
                    }
                }
            });
            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            OVERLAY.with(|o| {
                if let Some(ov) = o.borrow_mut().as_mut() {
                    ov.from = Some(pt());
                    ov.to = pt();
                }
            });
            SetCapture(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            OVERLAY.with(|o| {
                if let Some(ov) = o.borrow_mut().as_mut().filter(|ov| ov.from.is_some()) {
                    let a = ov.sel();
                    ov.to = pt();
                    let b = ov.sel();
                    let u = RECT { left: a.left.min(b.left) - 2, top: a.top.min(b.top) - 2, right: a.right.max(b.right) + 2, bottom: a.bottom.max(b.bottom) + 2 };
                    let _ = InvalidateRect(hwnd, Some(&u), false);
                }
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let _ = ReleaseCapture();
            OVERLAY.with(|o| {
                if let Some(ov) = o.borrow_mut().as_mut().filter(|ov| ov.from.is_some()) {
                    ov.to = pt();
                    ov.done = Some(ov.sel());
                }
            });
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_RBUTTONDOWN => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_KEYDOWN if wp.0 == VK_ESCAPE.0 as usize => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        // Switched away (Alt+Tab, Win): don't leave a frozen screen behind.
        WM_ACTIVATE if (wp.0 & 0xffff) as u32 == WA_INACTIVE => {
            let _ = DestroyWindow(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// OCR of a BGRA box, one line of text per line it reads.
fn read(px: Vec<u8>, w: u32, h: u32) -> windows::core::Result<String> {
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
    let (px, w, h) = shrink(px, w, h, OcrEngine::MaxImageDimension()?);
    let writer = DataWriter::new()?;
    writer.WriteBytes(&px)?;
    let bitmap = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, w as i32, h as i32)?;
    bitmap.CopyFromBuffer(&writer.DetachBuffer()?)?;
    let result = engine.RecognizeAsync(&bitmap)?.get()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        lines.push(line.Text()?.to_string_lossy());
    }
    Ok(lines.join("\r\n"))
}

/// OCR refuses images over `max` pixels a side: scale such a box down
/// (nearest pixel, which is plenty for text this large).
fn shrink(px: Vec<u8>, w: u32, h: u32, max: u32) -> (Vec<u8>, u32, u32) {
    let big = w.max(h);
    if big <= max || max == 0 {
        return (px, w, h);
    }
    let (nw, nh) = ((w * max / big).max(1), (h * max / big).max(1));
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    for y in 0..nh {
        let sy = (y * h / nh) as usize;
        for x in 0..nw {
            let i = (sy * w as usize + (x * w / nw) as usize) * 4;
            out.extend_from_slice(&px[i..i + 4]);
        }
    }
    (out, nw, nh)
}

#[cfg(test)]
mod tests {
    use super::shrink;

    #[test]
    fn shrinks_only_boxes_over_the_limit() {
        let px = vec![7u8; 10 * 4 * 4];
        assert_eq!(shrink(px.clone(), 10, 4, 2600).1, 10);
        let (out, w, h) = shrink(px, 10, 4, 5);
        assert_eq!((w, h, out.len()), (5, 2, 5 * 2 * 4));
    }

    /// OCR of the top-left of the screen as it is: `-- --ignored` on a desktop.
    #[test]
    #[ignore]
    fn reads_the_screen() {
        use windows::Win32::Graphics::Gdi::*;
        let (w, h) = (900, 500);
        unsafe {
            let dib = super::Dib::new(w, h).unwrap();
            let screen = GetDC(None);
            let _ = BitBlt(dib.dc, 0, 0, w, h, screen, 0, 0, SRCCOPY);
            ReleaseDC(None, screen);
            let px: Vec<u8> = dib.px().iter().flat_map(|p| (p | 0xff00_0000).to_le_bytes()).collect();
            let text = super::read(px, w as u32, h as u32).expect("ocr");
            println!("{text}");
        }
    }
}
