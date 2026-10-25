//! 0.16.0: QuestPath (imCioco's in-game quest tracker, Nexus Mods 10363) when the player has it.
//! Its permissions don't allow it to be shared in Elden Force's download, so it isn't included: a
//! player who downloads it from Nexus Mods puts its QuestPath folder (QuestPath.dll and QuestPath.ini)
//! in Documents\Elden Force, and Elden Force loads it at start, the way a mod loader would. Hold R3 (or
//! Page Up) to open it. Without it nothing changes.

use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::path::PathBuf;

use crate::log;

/// Documents (wherever Windows keeps it, OneDrive included).
fn documents() -> Option<PathBuf> {
    use windows_sys::Win32::System::Com::CoTaskMemFree;
    use windows_sys::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath};
    unsafe {
        let mut p: *mut u16 = std::ptr::null_mut();
        if SHGetKnownFolderPath(&FOLDERID_Documents, 0, std::ptr::null_mut(), &mut p) != 0 || p.is_null() {
            return None;
        }
        let len = (0..).take_while(|&i| *p.add(i) != 0).count();
        let s = std::ffi::OsString::from_wide(std::slice::from_raw_parts(p, len));
        CoTaskMemFree(p as _);
        Some(PathBuf::from(s))
    }
}

/// Where a player may put it: Documents\Elden Force\QuestPath (or QuestPath.dll straight in
/// Documents\Elden Force), or a QuestPath folder beside Elden Force's ModEngine2.
fn candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(d) = documents() {
        let ef = d.join("Elden Force");
        v.push(ef.join("QuestPath").join("QuestPath.dll"));
        v.push(ef.join("QuestPath.dll"));
        // a QuestPath folder unzipped inside its own folder (QuestPath\QuestPath\QuestPath.dll)
        v.push(ef.join("QuestPath").join("QuestPath").join("QuestPath.dll"));
    }
    if let Some(me2) = crate::recolor::mod_dir().and_then(|m| m.parent().map(|p| p.to_path_buf())) {
        v.push(me2.join("QuestPath").join("QuestPath.dll"));
    }
    v
}

/// Load QuestPath if the player has it (boot thread, before the game's task runner starts).
pub fn load() {
    use windows_sys::Win32::System::LibraryLoader::{LoadLibraryExW, LOAD_WITH_ALTERED_SEARCH_PATH};
    let Some(dll) = candidates().into_iter().find(|p| p.is_file()) else {
        log!("questpath: not installed (optional: put its QuestPath folder in Documents\\Elden Force)");
        return;
    };
    let wide: Vec<u16> = dll.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let h = unsafe { LoadLibraryExW(wide.as_ptr(), std::ptr::null_mut(), LOAD_WITH_ALTERED_SEARCH_PATH) };
    if h.is_null() {
        log!("questpath: {} would not load (error {})", dll.display(), std::io::Error::last_os_error());
    } else {
        log!("questpath: loaded {} (hold R3 or press Page Up for the quest tracker)", dll.display());
    }
}
