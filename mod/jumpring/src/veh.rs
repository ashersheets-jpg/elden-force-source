//! Lab only (2.0.1 crash hunt): the game catches its own crashes and exits, and the dump Windows writes is of that exit (a
//! heap check failing in a static destructor), not of what went wrong. A first-chance exception handler logs every fault
//! before the game's handler runs: its code, where, and the stack's return addresses into the game and our DLL.

use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

const MAX: u32 = 200;
static SEEN: AtomicU32 = AtomicU32::new(0);
/// the thread inside the handler now
static IN: AtomicU32 = AtomicU32::new(0);
/// each fault address logged a few times only (the game's own handled faults repeat)
static ADDRS: std::sync::Mutex<Vec<(usize, u32)>> = std::sync::Mutex::new(Vec::new());

/// the thread's stack top (TEB StackBase)
fn stack_base() -> usize {
    let v: usize;
    unsafe { std::arch::asm!("mov {}, gs:[0x8]", out(reg) v) };
    v
}

fn module_of(a: usize) -> Option<String> {
    for (name, w) in [("er", windows_sys::w!("eldenring.exe")), ("jr", windows_sys::w!("jumpring.dll")), ("ntdll", windows_sys::w!("ntdll.dll")), ("kb", windows_sys::w!("KERNELBASE.dll")), ("d3d12core", windows_sys::w!("D3D12Core.dll"))] {
        let b = unsafe { GetModuleHandleW(w) } as usize;
        if b == 0 {
            continue;
        }
        let mut mi: windows_sys::Win32::System::ProcessStatus::MODULEINFO = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            windows_sys::Win32::System::ProcessStatus::K32GetModuleInformation(
                windows_sys::Win32::System::Threading::GetCurrentProcess(), b as _, &mut mi, std::mem::size_of_val(&mi) as u32)
        } != 0;
        if ok && a >= b && a < b + mi.SizeOfImage as usize {
            return Some(format!("{name}+{:#x}", a - b));
        }
    }
    None
}

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    let rec = unsafe { &*(*info).ExceptionRecord };
    let code = rec.ExceptionCode as u32;
    // (routine: C++ throws, debugger prints, thread names, guard pages the game's own code uses)
    if matches!(code, 0xE06D7363 | 0x40010006 | 0x4001000A | 0x406D1388 | 0x80000001) || code < 0x80000000 {
        return 0;
    }
    let at = rec.ExceptionAddress as usize;
    // (a fault in this handler itself is never logged: no recursion)
    if module_of(at).is_some_and(|m| m.starts_with("jr+")) && IN.load(Ordering::Relaxed) == unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() } {
        return 0;
    }
    match ADDRS.try_lock() {
        Ok(mut a) => match a.iter_mut().find(|e| e.0 == at) {
            Some(e) if e.1 >= 3 => return 0,
            Some(e) => e.1 += 1,
            None => a.push((at, 1)),
        },
        Err(_) => return 0,
    }
    if SEEN.fetch_add(1, Ordering::Relaxed) >= MAX {
        return 0;
    }
    IN.store(unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }, Ordering::Relaxed);
    let ctx = unsafe { &*(*info).ContextRecord };
    let mut msg = format!(
        "VEH: exception {code:#x} at {} thread {} params {:x?} rcx {:#x} rdx {:#x} r8 {:#x}",
        module_of(at).unwrap_or(format!("{at:#x}")),
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() },
        &rec.ExceptionInformation[..(rec.NumberParameters as usize).min(4)],
        ctx.Rcx,
        ctx.Rdx,
        ctx.R8
    );
    // the stack's words that point into code we know (return addresses, mostly)
    let sp = ctx.Rsp as usize;
    let top = stack_base();
    let mut found = 0;
    msg += "\n  stack:";
    // (only within this thread's own stack: the faulting frame's sp is on this same thread)
    let end = if sp < top && top - sp < 0x100_0000 { (sp + 0x800).min(top) } else { sp };
    for k in 0..(end - sp) / 8 {
        let p = (sp + k * 8) as *const usize;
        let v = unsafe { p.read_volatile() };
        if let Some(m) = module_of(v) {
            msg += &format!(" {m}");
            found += 1;
            if found >= 28 {
                break;
            }
        }
    }
    crate::log::write_now(&msg);
    IN.store(0, Ordering::Relaxed);
    0
}

pub fn start() {
    let h = unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
    crate::log!("veh: first-chance exception log {}", if h.is_null() { "failed" } else { "on" });
}
