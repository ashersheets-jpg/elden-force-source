//! hooks.save: JumpRing plays on its own save (ER0000.jr2) so the player's normal save is never touched.
//! The game names its save from one UTF-16 ".sl2" literal; we rewrite it in memory before the title
//! screen reads saves. On first launch the normal save is copied, so the player keeps their character.

use std::path::PathBuf;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{PAGE_READWRITE, VirtualProtect};
use windows_sys::Win32::System::ProcessStatus::{GetModuleInformation, MODULEINFO};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

use crate::log;

const FROM: [u16; 4] = [b'.' as u16, b's' as u16, b'l' as u16, b'2' as u16];
const TO: [u16; 4] = [b'.' as u16, b'j' as u16, b'r' as u16, b'2' as u16];

pub fn redirect() {
    unsafe {
        let base = GetModuleHandleW(std::ptr::null());
        let mut info: MODULEINFO = std::mem::zeroed();
        if GetModuleInformation(GetCurrentProcess(), base, &mut info, size_of::<MODULEINFO>() as u32) == 0 {
            log!("save: could not read the game module; refusing to continue on the normal save");
            crate::log::drain();
            std::process::exit(1);
        }
        let image = std::slice::from_raw_parts(info.lpBaseOfDll as *const u16, info.SizeOfImage as usize / 2);
        let hits: Vec<usize> = image.windows(4).enumerate().filter(|(_, w)| *w == FROM).map(|(i, _)| i).collect();
        if hits.len() != 1 {
            log!("save: expected one .sl2 literal, found {}; refusing to continue on the normal save", hits.len());
            crate::log::drain();
            std::process::exit(1);
        }
        let p = (info.lpBaseOfDll as *mut u16).add(hits[0]);
        let mut old = 0;
        VirtualProtect(p as _, 8, PAGE_READWRITE, &mut old);
        std::ptr::copy_nonoverlapping(TO.as_ptr(), p, 4);
        VirtualProtect(p as _, 8, old, &mut old);
        log!("save: using ER0000.jr2 (normal save untouched)");
    }
    seed_from_normal_save();
}

/// First launch: copy each Steam account's ER0000.sl2 to ER0000.jr2 so the player keeps their character.
fn seed_from_normal_save() {
    let Some(appdata) = std::env::var_os("APPDATA").map(PathBuf::from) else { return };
    let Ok(dirs) = std::fs::read_dir(appdata.join("EldenRing")) else { return };
    for d in dirs.flatten() {
        let (sl2, jr2) = (d.path().join("ER0000.sl2"), d.path().join("ER0000.jr2"));
        if sl2.is_file() && !jr2.exists() {
            match std::fs::copy(&sl2, &jr2) {
                Ok(_) => log!("save: first launch, copied your normal save into {}", jr2.display()),
                Err(e) => log!("save: could not copy {}: {e}", sl2.display()),
            }
        }
    }
}
