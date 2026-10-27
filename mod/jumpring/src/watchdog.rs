//! 2.0.6: the freeze players hit (Melty: "Froze in eldenring.exe", most of them in a loading screen after a death or a
//! trip, ten minutes and more in) leaves nothing in their logs but "player: gone (loading or title)". The game's tasks run
//! every frame, loading screens included, so when Elden Force's frame task hasn't run for FROZEN, the game is stuck: this
//! writes where the game's main thread is (module + offset, unwound from its suspended context) to the log, which
//! players' Melty sends with the report. Read-only: the thread is only suspended for the moment it takes to read it.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, HMODULE};
use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, UNW_FLAG_NHANDLER};
use windows_sys::Win32::System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW};
use windows_sys::Win32::System::Threading::{GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME};

const FROZEN: Duration = Duration::from_secs(20);
const DEPTH: usize = 32;
/// CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER
const CONTEXT_FULL_AMD64: u32 = 0x0010_0003;

static MAIN: AtomicU32 = AtomicU32::new(0);
static BEAT: AtomicU64 = AtomicU64::new(0);
/// The thread the frame task runs on (the game may run its tasks off the main thread).
static FRAME: AtomicU32 = AtomicU32::new(0);

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// From DllMain (ModEngine2 loads us on the game's main thread).
pub fn note_main_thread() {
    MAIN.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

/// Every frame, from the frame task.
pub fn beat() {
    BEAT.store(now_ms(), Ordering::Relaxed);
    FRAME.store(unsafe { GetCurrentThreadId() }, Ordering::Relaxed);
}

fn name_of(a: u64) -> String {
    let mut m: HMODULE = std::ptr::null_mut();
    let ok = unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, a as usize as *const u16, &mut m) } != 0;
    if !ok || m.is_null() {
        return format!("{a:#x}");
    }
    let mut buf = [0u16; 260];
    let n = unsafe { GetModuleFileNameW(m, buf.as_mut_ptr(), buf.len() as u32) } as usize;
    let path = String::from_utf16_lossy(&buf[..n]);
    let file = path.rsplit('\\').next().unwrap_or(&path).to_string();
    format!("{file}+{:#x}", a - m as u64)
}

/// The suspended thread's return addresses, unwound with the modules' own unwind tables.
fn stack(tid: u32) -> Vec<u64> {
    // (allocated before the thread stops: it may hold the heap's lock)
    let mut out = Vec::with_capacity(DEPTH);
    unsafe {
        let h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, 0, tid);
        if h.is_null() {
            return out;
        }
        if SuspendThread(h) == u32::MAX {
            CloseHandle(h);
            return out;
        }
        let mut ctx: CONTEXT = std::mem::zeroed();
        ctx.ContextFlags = CONTEXT_FULL_AMD64;
        if GetThreadContext(h, &mut ctx) != 0 {
            for _ in 0..DEPTH {
                if ctx.Rip == 0 {
                    break;
                }
                out.push(ctx.Rip);
                let mut base = 0u64;
                let f = RtlLookupFunctionEntry(ctx.Rip, &mut base, std::ptr::null_mut());
                if f.is_null() {
                    // a leaf function: the return address is on top of the stack
                    if ctx.Rsp == 0 {
                        break;
                    }
                    ctx.Rip = *(ctx.Rsp as *const u64);
                    ctx.Rsp += 8;
                    continue;
                }
                let mut data = std::ptr::null_mut();
                let mut frame = 0u64;
                RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, ctx.Rip, f, &mut ctx, &mut data, &mut frame, std::ptr::null_mut());
            }
        }
        ResumeThread(h);
        CloseHandle(h);
    }
    out
}

/// After the boot: watch the frame task's heartbeat. Writes once per freeze (again only after frames came back).
pub fn start() {
    std::thread::spawn(|| {
        let mut reported = false;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let last = BEAT.load(Ordering::Relaxed);
            let tid = MAIN.load(Ordering::Relaxed);
            if last == 0 || tid == 0 {
                continue;
            }
            let quiet = now_ms().saturating_sub(last);
            if quiet < FROZEN.as_millis() as u64 {
                reported = false;
                continue;
            }
            if reported {
                continue;
            }
            reported = true;
            let s: Vec<String> = stack(tid).into_iter().map(name_of).collect();
            crate::log::write_now(&format!("watchdog: no frame for {:.0} s; the game's main thread {tid} is at: {}", quiet as f32 / 1000.0, s.join(" < ")));
            let ft = FRAME.load(Ordering::Relaxed);
            if ft != 0 && ft != tid {
                let s: Vec<String> = stack(ft).into_iter().map(name_of).collect();
                crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", s.join(" < ")));
            }
            // (a second look a few seconds later tells a deadlock (same place) from a slow loop)
            std::thread::sleep(Duration::from_secs(5));
            let s: Vec<String> = stack(tid).into_iter().take(12).map(name_of).collect();
            crate::log::write_now(&format!("watchdog: 5 s later: {}", s.join(" < ")));
        }
    });
}
