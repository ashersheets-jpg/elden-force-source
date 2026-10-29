//! 2.0.6: the freeze players hit (Melty: "Froze in eldenring.exe", most of them in a loading screen after a death or a
//! trip, ten minutes and more in) leaves nothing in their logs but "player: gone (loading or title)". The game's tasks run
//! every frame, loading screens included, so when Elden Force's frame task hasn't run for FROZEN, the game is stuck: this
//! writes where the game's main thread is (module + offset, unwound from its suspended context) to the log, which
//! players' Melty sends with the report. Read-only: the thread is only suspended for the moment it takes to read it.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HMODULE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, GetThreadContext, RtlLookupFunctionEntry, RtlVirtualUnwind, UNW_FLAG_NHANDLER};
use windows_sys::Win32::System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, GetModuleFileNameW, GetModuleHandleExW};
use windows_sys::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_INFORMATION, THREAD_SUSPEND_RESUME};

const FROZEN: Duration = Duration::from_secs(20);
const DEPTH: usize = 32;
/// CONTEXT_AMD64 | CONTEXT_CONTROL | CONTEXT_INTEGER
const CONTEXT_FULL_AMD64: u32 = 0x0010_0003;

static MAIN: AtomicU32 = AtomicU32::new(0);
static BEAT: AtomicU64 = AtomicU64::new(0);
/// The thread the frame task runs on (the game may run its tasks off the main thread).
static FRAME: AtomicU32 = AtomicU32::new(0);
static BEATS: AtomicU64 = AtomicU64::new(0);

/// How many frames the frame task has run (2.0.7: the overlay's fallback hooking counts the game's frames by it).
pub fn beats() -> u64 {
    BEATS.load(Ordering::Relaxed)
}

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
    BEATS.fetch_add(1, Ordering::Relaxed);
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
#[repr(C, align(16))]
struct Aligned(CONTEXT);

/// (2.0.7: the lab's first freeze at the title logged an empty stack: why it couldn't be read now goes in its place)
fn stack(tid: u32) -> Result<Vec<u64>, String> {
    // (allocated before the thread stops: it may hold the heap's lock)
    let mut out = Vec::with_capacity(DEPTH);
    unsafe {
        let h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, 0, tid);
        if h.is_null() {
            return Err(format!("(thread not opened: error {})", GetLastError()));
        }
        if SuspendThread(h) == u32::MAX {
            let e = GetLastError();
            CloseHandle(h);
            return Err(format!("(thread not stopped: error {e})"));
        }
        // (GetThreadContext wants its CONTEXT 16-byte aligned; windows-sys' CONTEXT is only 8: the call failed with
        // ERROR_NOACCESS whenever the stack put it off 16, and the lab's freeze logged no stack)
        let mut boxed = Aligned(std::mem::zeroed());
        let ctx = &mut boxed.0;
        ctx.ContextFlags = CONTEXT_FULL_AMD64;
        let got = GetThreadContext(h, ctx) != 0;
        let err = if got { 0 } else { GetLastError() };
        if got {
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
                RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, ctx.Rip, f, ctx, &mut data, &mut frame, std::ptr::null_mut());
            }
        }
        ResumeThread(h);
        CloseHandle(h);
        if !got {
            return Err(format!("(registers not read: error {err})"));
        }
    }
    Ok(out)
}

fn named(tid: u32, depth: usize) -> String {
    match stack(tid) {
        Ok(s) => s.into_iter().take(depth).map(name_of).collect::<Vec<_>>().join(" < "),
        Err(e) => e,
    }
}

/// Every thread of the game but this one.
fn threads() -> Vec<u32> {
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let (pid, me) = (GetCurrentProcessId(), GetCurrentThreadId());
        let mut e: THREADENTRY32 = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;
        let mut more = Thread32First(snap, &mut e) != 0;
        while more {
            if e.th32OwnerProcessID == pid && e.th32ThreadID != me {
                out.push(e.th32ThreadID);
            }
            more = Thread32Next(snap, &mut e) != 0;
        }
        CloseHandle(snap);
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
            crate::log::write_now(&format!("watchdog: no frame for {:.0} s; the game's main thread {tid} is at: {}", quiet as f32 / 1000.0, named(tid, DEPTH)));
            let ft = FRAME.load(Ordering::Relaxed);
            if ft != 0 && ft != tid {
                crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", named(ft, DEPTH)));
            }
            // (2.0.7: a freeze is often one thread waiting on another (the render thread on the GPU, a loader on a file):
            // every thread's top frames, once)
            for t in threads() {
                if t != tid && t != ft {
                    crate::log::write_now(&format!("watchdog:   thread {t}: {}", named(t, 10)));
                }
            }
            // (2.0.7: the overlay's own GPU work: was its last frame done, was it inside the game's Present?)
            crate::log::write_now(&format!("watchdog: overlay: {}", hudhook::jr_state::report()));
            #[cfg(feature = "lab")]
            gpu_frames();
            // (a second look a few seconds later tells a deadlock (same place) from a slow loop)
            std::thread::sleep(Duration::from_secs(5));
            crate::log::write_now(&format!("watchdog: 5 s later: {}", named(tid, 12)));
        }
    });
}

/// lab (2.0.7, the title freeze: the main thread in the game's frame-fence poll at er+0x1afe0e3): the game's ring of 6
/// frame fences in its graphics global (er+0x47f6400, game 2.7.2): each one's wanted value and what the GPU has completed
#[cfg(feature = "lab")]
fn gpu_frames() {
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    unsafe {
        let base = GetModuleHandleW(std::ptr::null()) as usize;
        let gfx = *((base + 0x47f6400) as *const usize);
        if gfx == 0 {
            crate::log::write_now("watchdog: gpu: no graphics global");
            return;
        }
        let frame = *((gfx + 0xe80) as *const u64);
        let ring = *((gfx + 0xf08) as *const usize);
        let mut out = format!("watchdog: gpu: game frame {frame}, slot {}; fences:", *((gfx + 0xf1c) as *const u32));
        for i in 0..6 {
            let e = ring + i * 16;
            let (wrap, want) = (*(e as *const usize), *((e + 8) as *const u64));
            let fence = if wrap == 0 { 0 } else { *((wrap + 8) as *const usize) };
            let done = if fence == 0 {
                u64::MAX - 1
            } else {
                let vt = *(fence as *const *const usize);
                let f: unsafe extern "system" fn(usize) -> u64 = std::mem::transmute(*vt.add(8));
                f(fence)
            };
            out.push_str(&format!(" [{i}] {fence:#x} want {want} done {done}"));
        }
        crate::log::write_now(&out);
    }
}
