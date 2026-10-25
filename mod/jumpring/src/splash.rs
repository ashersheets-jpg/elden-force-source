//! 1.1.1: a small window shown while the first launch makes the fighters' outfits, moves and title
//! screen (recolor.rs holds the game's file reads until they exist). Players saw nothing, or a game
//! window that stopped responding, for a minute or more and closed it.

use std::sync::atomic::{AtomicUsize, Ordering};

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::*;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::*;

/// Outfit pieces made, of how many.
pub static PROGRESS: (AtomicUsize, AtomicUsize) = (AtomicUsize::new(0), AtomicUsize::new(0));

const W: i32 = 600;
const H: i32 = 170;

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub fn show() {
    std::thread::spawn(|| unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let class = wide("EldenForcePreparing");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(proc_),
            hInstance: inst,
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(std::ptr::null_mut(), IDC_ARROW),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            class.as_ptr(),
            wide("Elden Force").as_ptr(),
            WS_POPUP | WS_VISIBLE | WS_BORDER,
            (sw - W) / 2,
            (sh - H) / 2,
            W,
            H,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            inst,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return;
        }
        SetTimer(hwnd, 1, 250, None);
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_TIMER => {
                if crate::recolor::done() {
                    DestroyWindow(hwnd);
                } else {
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_PAINT => {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let dc = BeginPaint(hwnd, &mut ps);
                let bg = CreateSolidBrush(0x0014_1210);
                let r = RECT { left: 0, top: 0, right: W, bottom: H };
                FillRect(dc, &r, bg);
                DeleteObject(bg as _);
                SetBkMode(dc, TRANSPARENT as _);
                let face = wide("Segoe UI");
                let big = CreateFontW(30, 0, 0, 0, 800, 0, 0, 0, 0, 0, 0, 0, 0, face.as_ptr());
                let small = CreateFontW(18, 0, 0, 0, 400, 0, 0, 0, 0, 0, 0, 0, 0, face.as_ptr());
                let old = SelectObject(dc, big as _);
                SetTextColor(dc, 0x0060_C8E8);
                let mut t = RECT { left: 24, top: 18, right: W - 24, bottom: 56 };
                let title = wide("ELDEN FORCE");
                DrawTextW(dc, title.as_ptr(), -1, &mut t, DT_LEFT | DT_SINGLELINE);
                SelectObject(dc, small as _);
                SetTextColor(dc, 0x00E0_E8EE);
                let (done, total) = (PROGRESS.0.load(Ordering::Relaxed), PROGRESS.1.load(Ordering::Relaxed));
                let step = if total == 0 { "Reading your Elden Ring files...".to_string() } else { format!("Building the fighters' outfits: {done} of {total}") };
                let body = wide(&format!(
                    "First launch: making the fighters' looks and moves from your own Elden Ring files.\nThis happens once and takes about a minute. The game starts by itself.\n\n{step}"
                ));
                let mut b = RECT { left: 24, top: 60, right: W - 24, bottom: H - 12 };
                DrawTextW(dc, body.as_ptr(), -1, &mut b, DT_LEFT | DT_WORDBREAK);
                SelectObject(dc, old);
                DeleteObject(big as _);
                DeleteObject(small as _);
                EndPaint(hwnd, &ps);
                0
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
