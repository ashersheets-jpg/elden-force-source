//! 2.0.6: Steam first. Elden Ring started through ModEngine2 quits within seconds when Steam isn't open (it can't sign in
//! to Steam). Up to 2.0.4 the starter (mod/launcher) opened Steam before the game; 2.0.5 ships no programs of its own
//! (Smart App Control refused them), so players who start Elden Force with Steam closed saw the game close by itself
//! again. The game asks Steam first thing (steam_api64's SteamAPI_RestartAppIfNecessary, then SteamAPI_Init, both
//! before its window or files): those two wait here until Steam is open and signed in, as the starter did.

use std::process::Command;
use std::sync::{Once, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};


type Init = unsafe extern "C" fn() -> bool;
type Restart = unsafe extern "C" fn(u32) -> bool;

static INIT: OnceLock<retour::RawDetour> = OnceLock::new();
static RESTART: OnceLock<retour::RawDetour> = OnceLock::new();
static READY: Once = Once::new();

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn reg_dword(key: &str, name: &str) -> u32 {
    let (k, n) = (wide(key), wide(name));
    let mut v = 0u32;
    let mut len = 4u32;
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_DWORD, std::ptr::null_mut(), &mut v as *mut u32 as _, &mut len) };
    if ok == 0 { v } else { 0 }
}

fn reg_string(key: &str, name: &str) -> Option<String> {
    let (k, n) = (wide(key), wide(name));
    let mut buf = vec![0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let ok = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as _, &mut len) };
    if ok != 0 {
        return None;
    }
    let s = String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]);
    (!s.is_empty()).then_some(s)
}

fn alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code) != 0 && code == STILL_ACTIVE as u32;
        CloseHandle(h);
        ok
    }
}

const ACTIVE: &str = r"Software\Valve\Steam\ActiveProcess";

/// Seconds since process `pid` started, or None when it can't be opened.
fn age_secs(pid: u32) -> Option<u64> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return None;
        }
        let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
        let (mut created, mut a, mut b, mut c, mut now) = (z, z, z, z, z);
        let ok = GetProcessTimes(h, &mut created, &mut a, &mut b, &mut c) != 0;
        CloseHandle(h);
        GetSystemTimeAsFileTime(&mut now);
        let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
        ok.then(|| t(now).saturating_sub(t(created)) / 10_000_000)
    }
}

/// Every running steam.exe (the client; steamwebhelper and steamservice are other names).
fn steam_pids() -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut e) != 0;
        while more {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            if String::from_utf16_lossy(&e.szExeFile[..n]).eq_ignore_ascii_case("steam.exe") {
                out.push(e.th32ProcessID);
            }
            more = Process32NextW(snap, &mut e) != 0;
        }
        CloseHandle(snap);
    }
    out
}

/// (as the starter, 1.1.2: Steam's ActiveProcess key can be stale or out of this process's sight while Steam is plainly
/// running; trust it when it names a live Steam, otherwise a steam.exe that has been up a while counts as ready)
fn signed_in() -> bool {
    let pid = reg_dword(ACTIVE, "pid");
    if alive(pid) {
        return reg_dword(ACTIVE, "ActiveUser") != 0;
    }
    steam_pids().into_iter().any(|p| age_secs(p).is_some_and(|s| s >= 15))
}

/// Open Steam if it's closed and wait until it's signed in (once per launch; the game's thread waits here).
fn ensure() {
    READY.call_once(|| {
        if signed_in() {
            return;
        }
        if !steam_pids().is_empty() {
            // running but just started (or still signing in): a moment, not minutes
            let t0 = Instant::now();
            while !signed_in() && t0.elapsed() < Duration::from_secs(60) {
                std::thread::sleep(Duration::from_millis(500));
            }
            crate::log!("steam: running, ready {} after {:.0} s", signed_in(), t0.elapsed().as_secs_f32());
            return;
        }
        let exe = reg_string(r"Software\Valve\Steam", "SteamExe").unwrap_or_else(|| r"C:\Program Files (x86)\Steam\steam.exe".into());
        crate::log!("steam: not open, starting it before the game signs in ({exe})");
        if Command::new(&exe).arg("-silent").spawn().is_err() {
            let _ = Command::new("cmd").args(["/C", "start", "", "steam://open/main"]).spawn();
        }
        // signing in can wait on the player (Steam's own sign-in window): a few minutes
        let t0 = Instant::now();
        while !signed_in() && t0.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(500));
        }
        crate::log!("steam: signed in {} after {:.0} s", signed_in(), t0.elapsed().as_secs_f32());
        // Steam's client needs a moment after sign-in before games can connect to it
        std::thread::sleep(Duration::from_secs(5));
    });
}

unsafe extern "C" fn hk_restart(app: u32) -> bool {
    ensure();
    let orig: Restart = unsafe { std::mem::transmute(RESTART.get().unwrap().trampoline()) };
    unsafe { orig(app) }
}

unsafe extern "C" fn hk_init() -> bool {
    ensure();
    let orig: Init = unsafe { std::mem::transmute(INIT.get().unwrap().trampoline()) };
    unsafe { orig() }
}

/// From DllMain: the game's Steam start-up waits for Steam (steam_api64 is one of the game's own imports, already loaded).
pub fn install() {
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
        let api = GetModuleHandleA(c"steam_api64.dll".as_ptr() as _);
        if api.is_null() {
            return;
        }
        if let Some(f) = GetProcAddress(api, c"SteamAPI_RestartAppIfNecessary".as_ptr() as _) {
            if let Ok(d) = retour::RawDetour::new(f as *const (), hk_restart as *const ()).and_then(|d| d.enable().map(|_| d)) {
                let _ = RESTART.set(d);
            }
        }
        if let Some(f) = GetProcAddress(api, c"SteamAPI_Init".as_ptr() as _) {
            if let Ok(d) = retour::RawDetour::new(f as *const (), hk_init as *const ()).and_then(|d| d.enable().map(|_| d)) {
                let _ = INIT.set(d);
            }
        }
    }
}
