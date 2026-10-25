//! Elden Force's starter (1.1.3). Elden Ring started through ModEngine2 quits within seconds when
//! Steam isn't open (it can't sign in to Steam); players saw the game close by itself. This opens
//! Steam first when it's closed, waits until it is signed in, then runs ModEngine2 with our args.
//! 1.2.0: meanwhile it runs Elden Force's prepare step (jumpring/elden_force_prepare.exe), which builds the
//! animations before the game starts, so the game never builds them while it loads.
#![windows_subsystem = "windows"]

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, FILETIME, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RRF_RT_REG_SZ, RegGetValueW};
use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

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

/// 1.1.2: Steam's ActiveProcess key can be stale or written somewhere this process can't see
/// (Steam started from a sandboxed/packaged app), while Steam is plainly running. The starter then
/// waited its full 5 minutes before the game appeared. Trust the key when it names a live Steam;
/// otherwise a steam.exe that has been up a while counts as ready.
fn steam_signed_in() -> bool {
    let pid = reg_dword(ACTIVE, "pid");
    if alive(pid) {
        return reg_dword(ACTIVE, "ActiveUser") != 0;
    }
    steam_pids().into_iter().any(|p| age_secs(p).is_some_and(|s| s >= 15))
}

fn steam_state() -> String {
    let pids = steam_pids();
    let ages: Vec<String> = pids.iter().map(|&p| format!("{p}:{}s", age_secs(p).map_or("?".into(), |s| s.to_string()))).collect();
    format!("key pid {} alive {} user {}; steam.exe [{}]", reg_dword(ACTIVE, "pid"), alive(reg_dword(ACTIVE, "pid")), reg_dword(ACTIVE, "ActiveUser") != 0, ages.join(", "))
}

fn log(line: &str) {
    let Some(dir) = std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("JumpRing").join("logs")) else { return };
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("starter.log")) {
        let _ = writeln!(f, "{line}");
    }
}

fn main() {
    let _ = std::fs::remove_file(std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("JumpRing/logs/starter.log")).unwrap_or_default());
    let here = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from)).unwrap_or_default();
    // 1.2.0: the animations, built while Steam gets ready (a launch right after the game built them
    // itself sometimes froze loading into the world)
    let t_prep = Instant::now();
    let prepare = {
        use std::os::windows::process::CommandExt;
        Command::new(here.join("jumpring").join("elden_force_prepare.exe")).current_dir(&here).creation_flags(0x0800_0000).spawn()
    };
    if let Err(e) = &prepare {
        log(&format!("prepare: could not start it ({e}); the game will build the animations"));
    }
    log(&format!("steam: {}", steam_state()));
    if steam_signed_in() {
        log("steam: open");
    } else if !steam_pids().is_empty() {
        // Steam is running but just started (or still signing in): give it a moment, not 5 minutes.
        let t0 = Instant::now();
        while !steam_signed_in() && t0.elapsed() < Duration::from_secs(60) {
            std::thread::sleep(Duration::from_millis(500));
        }
        log(&format!("steam: running, ready {} after {:.0}s", steam_signed_in(), t0.elapsed().as_secs_f32()));
    } else {
        let exe = reg_string(r"Software\Valve\Steam", "SteamExe").unwrap_or_else(|| r"C:\Program Files (x86)\Steam\steam.exe".into());
        log(&format!("steam: not open, starting {exe}"));
        if let Err(e) = Command::new(&exe).arg("-silent").spawn() {
            log(&format!("steam: could not start it ({e}); trying the steam:// link"));
            let _ = Command::new("cmd").args(["/C", "start", "", "steam://open/main"]).spawn();
        }
        // Signing in can wait on the player (Steam's own sign-in window): give it a few minutes.
        let t0 = Instant::now();
        while !steam_signed_in() && t0.elapsed() < Duration::from_secs(300) {
            std::thread::sleep(Duration::from_millis(500));
        }
        log(&format!("steam: signed in {} after {:.0}s", steam_signed_in(), t0.elapsed().as_secs_f32()));
        // Steam's client needs a moment after sign-in before games can connect to it.
        std::thread::sleep(Duration::from_secs(5));
    }
    if let Ok(mut child) = prepare {
        // a first build takes about 10 s; after that it only checks. Never hold the game up for long.
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break format!("{s}"),
                Ok(None) if t_prep.elapsed() < Duration::from_secs(180) => std::thread::sleep(Duration::from_millis(100)),
                Ok(None) => {
                    let _ = child.kill();
                    break "too slow, stopped; the game will build the animations".into();
                }
                Err(e) => break format!("{e}"),
            }
        };
        log(&format!("prepare: {status} after {:.1}s", t_prep.elapsed().as_secs_f32()));
    }
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    // 1.3.0 (the user: "It wont let me start the game", the starter double-clicked): with no arguments ModEngine2 has no
    // game to start and quits at once (exit -2). Started by hand, it starts Elden Ring with our config, as Melty's launch does
    if args.is_empty() {
        args = ["-t", "er", "-c", "config_eldenring.toml"].iter().map(|s| s.to_string()).collect();
        log("modengine2: no arguments (started by hand): -t er -c config_eldenring.toml");
    }
    let run = || {
        let t0 = Instant::now();
        let status = Command::new(here.join("modengine2_launcher.exe")).args(&args).current_dir(&here).status();
        log(&format!("modengine2: {status:?} after {:.1}s", t0.elapsed().as_secs_f32()));
        status.ok().and_then(|s| s.code()).unwrap_or(1)
    };
    let mut code = run();
    // 1.1.3: a player's ModEngine2 gave up at once (exit -2) and the game never opened. Once more after
    // a moment (Steam or the previous game may still have been letting go).
    if code != 0 {
        log(&format!("modengine2: failed ({code:#x}); trying once more in 5 s"));
        std::thread::sleep(Duration::from_secs(5));
        code = run();
    }
    std::process::exit(code);
}
