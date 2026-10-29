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

/// (2.0.8) Read a thread's stack on a helper thread, giving it 3 s. The unwinder takes the loader's function-table lock:
/// if the stopped thread holds it (loading a DLL), the helper waits forever, and the thread it stopped with it. In 2.0.6
/// no player's freeze report had even the watchdog's first line. Past the 3 s the thread is let go from here, and the
/// helper gives up at its next step.
fn stack(tid: u32) -> Result<Vec<u64>, String> {
    use std::sync::atomic::{AtomicBool, AtomicU8};
    use std::sync::{Arc, mpsc};
    let h = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, tid) };
    if h.is_null() {
        return Err(format!("(thread not opened: error {})", unsafe { GetLastError() }));
    }
    // (who lets the thread go: STOPPED -> LET_GO once, by the helper when done or by this thread past the 3 s)
    let abandoned = Arc::new(AtomicBool::new(false));
    let state = Arc::new(AtomicU8::new(IDLE));
    let (tx, rx) = mpsc::channel();
    let (gave_up, st) = (abandoned.clone(), state.clone());
    let spawned = std::thread::Builder::new().name("jr-watchdog-stack".into()).spawn(move || {
        let _ = tx.send(stack_now(tid, &gave_up, &st));
    });
    if spawned.is_err() {
        unsafe { CloseHandle(h) };
        return Err("(no helper thread)".into());
    }
    let r = match rx.recv_timeout(Duration::from_secs(3)) {
        Ok(r) => r,
        Err(_) => {
            abandoned.store(true, Ordering::SeqCst);
            if state.compare_exchange(STOPPED, LET_GO, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                unsafe { ResumeThread(h) };
            } else {
                // (not stopped yet: the helper lets it go the moment it does)
                let _ = state.compare_exchange(IDLE, LET_GO, Ordering::SeqCst, Ordering::SeqCst);
            }
            Err("(stack not read in 3 s: the unwinder waited on a lock the thread holds; the thread was let go)".into())
        }
    };
    unsafe { CloseHandle(h) };
    r
}

const IDLE: u8 = 0;
const STOPPED: u8 = 1;
const LET_GO: u8 = 2;

fn stack_now(tid: u32, abandoned: &std::sync::atomic::AtomicBool, state: &std::sync::atomic::AtomicU8) -> Result<Vec<u64>, String> {
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
        if state.compare_exchange(IDLE, STOPPED, Ordering::SeqCst, Ordering::SeqCst).is_err() {
            // (the watchdog gave up before this thread got to stop it)
            ResumeThread(h);
            CloseHandle(h);
            return Err("(abandoned)".into());
        }
        let let_go = || {
            if state.compare_exchange(STOPPED, LET_GO, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
                ResumeThread(h);
            }
        };
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
                if abandoned.load(Ordering::SeqCst) {
                    // (the watchdog let the thread go: its stack has moved on)
                    let_go();
                    CloseHandle(h);
                    return Err("(abandoned)".into());
                }
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
        let_go();
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

/// (2.0.7: a freeze is often one thread waiting on another (the render thread on the GPU, a loader on a file): every
/// thread's top frames, once)
fn every_thread(tid: u32, ft: u32) {
    for t in threads() {
        if t != tid && t != ft {
            crate::log::write_now(&format!("watchdog:   thread {t}: {}", named(t, 10)));
        }
    }
}

/// (2.0.8) Since when the player has been gone (a load, or the title), ms; 0 while in the world. From the frame task.
static GONE_SINCE: AtomicU64 = AtomicU64::new(0);

/// Every frame, from the frame task: is the player in the world?
pub fn player_present(present: bool) {
    if present {
        GONE_SINCE.store(0, Ordering::Relaxed);
    } else if GONE_SINCE.load(Ordering::Relaxed) == 0 {
        GONE_SINCE.store(now_ms(), Ordering::Relaxed);
    }
}

/// 2.0.8: players' freeze reports end in "player: gone (loading or title)" with no watchdog line: the game still drew frames
/// (a loading screen that never ends, or the title) and the watchdog, which watches frames, saw nothing wrong. A minute
/// with no player while frames come: every thread's place, once per load (a load that never ends shows what it waits on),
/// then a line every 2 minutes (6 at most) saying the frames still come. `said` = (the load it's about, lines, frames then).
fn endless_load(tid: u32, said: &mut (u64, u32, u64)) {
    let since = GONE_SINCE.load(Ordering::Relaxed);
    if since == 0 {
        return;
    }
    let gone = now_ms().saturating_sub(since);
    if said.0 != since {
        if gone < 60_000 {
            return;
        }
        *said = (since, 0, beats());
        crate::log::write_now(&format!("watchdog: no player for {:.0} s while the game still draws frames (a load, or the title); where it is:", gone as f32 / 1000.0));
        crate::log::write_now(&format!("watchdog: the game's main thread {tid} is at: {}", named(tid, DEPTH)));
        let ft = FRAME.load(Ordering::Relaxed);
        if ft != 0 && ft != tid {
            crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", named(ft, DEPTH)));
        }
        every_thread(tid, ft);
        return;
    }
    if said.1 < 6 && gone >= 60_000 + 120_000 * (said.1 as u64 + 1) {
        let frames = beats();
        crate::log::write_now(&format!("watchdog: still no player after {:.0} s ({} frames in the last 2 minutes)", gone as f32 / 1000.0, frames - said.2));
        *said = (since, said.1 + 1, frames);
    }
}

/// After the boot: watch the frame task's heartbeat. Writes once per freeze (again only after frames came back).
pub fn start() {
    std::thread::spawn(|| {
        let mut reported = false;
        let mut load_said = (0u64, 0u32, 0u64);
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
                endless_load(tid, &mut load_said);
                continue;
            }
            if reported {
                continue;
            }
            reported = true;
            // (2.0.8: said before any stack is read, so a report shows the freeze even if reading one goes wrong)
            crate::log::write_now(&format!("watchdog: no frame for {:.0} s (the player {}); reading where the game is", quiet as f32 / 1000.0, if GONE_SINCE.load(Ordering::Relaxed) == 0 { "in the world" } else { "gone: loading or title" }));
            crate::log::write_now(&format!("watchdog: the game's main thread {tid} is at: {}", named(tid, DEPTH)));
            let ft = FRAME.load(Ordering::Relaxed);
            if ft != 0 && ft != tid {
                crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", named(ft, DEPTH)));
            }
            every_thread(tid, ft);
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
