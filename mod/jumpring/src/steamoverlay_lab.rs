//! lab (2.0.7, the title freeze): "nosteamoverlay.on" beside the logs keeps Steam's in-game overlay
//! (gameoverlayrenderer64.dll, which also hooks the game's Present and draws its "Shift+Tab" notice a few seconds in)
//! out of the game: its load is refused. Tells whether the freeze needs both overlays.
//! Also "refusedll.on" beside the logs, holding names (one per line, or comma-separated; any part of the file name):
//! those DLLs are refused too (the startup heap fault: other mods in the game folder's mods\, e.g. AdjustTheFov).

use std::sync::OnceLock;

use windows_sys::Win32::Foundation::HMODULE;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

type LoadEx = unsafe extern "system" fn(*const u16, *mut core::ffi::c_void, u32) -> HMODULE;
static ORIG: OnceLock<retour::RawDetour> = OnceLock::new();
static REFUSE: OnceLock<Vec<String>> = OnceLock::new();

unsafe extern "system" fn hk(name: *const u16, file: *mut core::ffi::c_void, flags: u32) -> HMODULE {
    if !name.is_null() {
        let n = (0..).take_while(|&i| unsafe { *name.add(i) } != 0).count();
        let s = String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(name, n) }).to_ascii_lowercase();
        if REFUSE.get().is_some_and(|r| r.iter().any(|x| s.contains(x.as_str()))) {
            crate::log!("lab steam overlay: load refused ({s})");
            windows_sys::Win32::Foundation::SetLastError(126);
            return std::ptr::null_mut();
        }
    }
    let f: LoadEx = unsafe { std::mem::transmute(ORIG.get().unwrap().trampoline()) };
    unsafe { f(name, file, flags) }
}

pub fn install() {
    let mut refuse: Vec<String> = std::fs::read_to_string(crate::log::dir().join("refusedll.on"))
        .unwrap_or_default()
        .split([',', '\n', '\r'])
        .map(|x| x.trim().to_ascii_lowercase())
        .filter(|x| !x.is_empty())
        .collect();
    let steam = crate::log::dir().join("nosteamoverlay.on").exists();
    if steam {
        refuse.push("gameoverlayrenderer".into());
    }
    if refuse.is_empty() {
        return;
    }
    crate::log!("lab steam overlay: refusing {refuse:?}");
    let _ = REFUSE.set(refuse);
    unsafe {
        if steam && !GetModuleHandleW(windows_sys::w!("gameoverlayrenderer64.dll")).is_null() {
            crate::log!("lab steam overlay: already loaded at start (can't keep it out)");
            return;
        }
        let kb = GetModuleHandleW(windows_sys::w!("kernelbase.dll"));
        let Some(f) = GetProcAddress(kb, c"LoadLibraryExW".as_ptr() as _) else { return };
        match retour::RawDetour::new(f as *const (), hk as *const ()).and_then(|d| d.enable().map(|_| d)) {
            Ok(d) => {
                let _ = ORIG.set(d);
                crate::log!("lab steam overlay: kept out");
            }
            Err(e) => crate::log!("lab steam overlay: hook failed: {e}"),
        }
    }
}
