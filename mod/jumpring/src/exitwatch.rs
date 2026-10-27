//! Lab only (2.0.4 crash hunt): the long-play "crash" ~1890 s in is the game exiting by itself right as a villain is
//! summoned (its allocators are already being torn down when the heap check fails). This writes down who asked to exit:
//! detours on ntdll's RtlExitUserProcess (ExitProcess, exit()) and KERNELBASE's TerminateProcess log the calling thread's
//! return addresses into the game and our DLL to logs/exitwatch.txt, synchronously (the log thread may be gone by then).

use std::io::Write;
use std::sync::OnceLock;

use retour::RawDetour;

static EXIT: OnceLock<RawDetour> = OnceLock::new();
static TERMINATE: OnceLock<RawDetour> = OnceLock::new();

fn record(what: &str, code: u32) {
    let mut frames = [std::ptr::null_mut::<core::ffi::c_void>(); 62];
    let n = unsafe { windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace(1, 62, frames.as_mut_ptr(), std::ptr::null_mut()) } as usize;
    let tid = unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let mut s = format!("[unix {now}] {what}({code:#x}) on thread {tid}:");
    for f in &frames[..n] {
        s.push(' ');
        s.push_str(&crate::veh::module_of(*f as usize).unwrap_or_else(|| format!("{:#x}", *f as usize)));
    }
    s.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(crate::log::dir().join("exitwatch.txt")) {
        let _ = f.write_all(s.as_bytes());
        let _ = f.flush();
    }
}

unsafe extern "system" fn hk_exit(code: u32) -> ! {
    record("RtlExitUserProcess", code);
    let f: unsafe extern "system" fn(u32) -> ! = unsafe { std::mem::transmute(EXIT.get().unwrap().trampoline()) };
    unsafe { f(code) }
}

unsafe extern "system" fn hk_terminate(process: *mut core::ffi::c_void, code: u32) -> i32 {
    // (only this process ending matters: a pseudo-handle -1 or our own id)
    let own = process as isize == -1 || unsafe { windows_sys::Win32::System::Threading::GetProcessId(process as _) == windows_sys::Win32::System::Threading::GetCurrentProcessId() };
    if own {
        record("TerminateProcess", code);
    }
    let f: unsafe extern "system" fn(*mut core::ffi::c_void, u32) -> i32 = unsafe { std::mem::transmute(TERMINATE.get().unwrap().trampoline()) };
    unsafe { f(process, code) }
}

pub fn start() {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress};
    unsafe {
        let ntdll = GetModuleHandleA(c"ntdll.dll".as_ptr() as _);
        if let Some(f) = GetProcAddress(ntdll, c"RtlExitUserProcess".as_ptr() as _) {
            if let Ok(d) = RawDetour::new(f as *const (), hk_exit as *const ()).and_then(|d| d.enable().map(|_| d)) {
                let _ = EXIT.set(d);
            }
        }
        let kb = GetModuleHandleA(c"kernelbase.dll".as_ptr() as _);
        if let Some(f) = GetProcAddress(kb, c"TerminateProcess".as_ptr() as _) {
            if let Ok(d) = RawDetour::new(f as *const (), hk_terminate as *const ()).and_then(|d| d.enable().map(|_| d)) {
                let _ = TERMINATE.set(d);
            }
        }
    }
    crate::log!("exitwatch: exit {} terminate {}", EXIT.get().is_some(), TERMINATE.get().is_some());
}
