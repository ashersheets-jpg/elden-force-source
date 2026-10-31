//! 2.0.6: the freeze players hit (Melty: "Froze in eldenring.exe", most of them in a loading screen after a death or a
//! trip, ten minutes and more in) leaves nothing in their logs but "player: gone (loading or title)". The game's tasks run
//! every frame, loading screens included, so when Elden Force's frame task hasn't run for FROZEN, the game is stuck: this
//! writes where the game's main thread is (module + offset, unwound from its suspended context) to the log, which
//! players' Melty sends with the report. Read-only: the thread is only suspended for the moment it takes to read it.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, HMODULE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
use windows_sys::Win32::System::Diagnostics::Debug::{CONTEXT, GetThreadContext};
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

/// One loaded module: where it is, its file name and its function table (.pdata, the unwind data the compiler wrote).
struct Module {
    base: usize,
    size: usize,
    name: String,
    pdata: usize,
    count: usize,
}

impl Module {
    /// `len` bytes at `rva` inside the image, if they are inside it.
    fn bytes(&self, rva: u32, len: usize) -> Option<&[u8]> {
        let end = (rva as usize).checked_add(len)?;
        if end > self.size {
            return None;
        }
        Some(unsafe { std::slice::from_raw_parts((self.base + rva as usize) as *const u8, len) })
    }

    /// The function table's entry (begin, end, unwind info) that holds `rva`.
    fn function(&self, rva: u32) -> Option<(u32, u32, u32)> {
        let entry = |i: usize| {
            let b = unsafe { std::slice::from_raw_parts((self.base + self.pdata + i * 12) as *const u8, 12) };
            let d = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            (d(0), d(4), d(8))
        };
        let (mut lo, mut hi) = (0usize, self.count);
        while lo < hi {
            let mid = (lo + hi) / 2;
            let (begin, end, unwind) = entry(mid);
            if rva < begin {
                hi = mid;
            } else if rva >= end {
                lo = mid + 1;
            } else {
                return Some((begin, end, unwind));
            }
        }
        None
    }
}

/// (2.0.9) Every loaded module with its function table, read from the images' own headers. psapi reads the loader's list
/// without taking the loader's lock, so this never waits on a thread that is loading a DLL.
fn modules() -> Vec<Module> {
    use windows_sys::Win32::System::ProcessStatus::{K32EnumProcessModules, K32GetModuleBaseNameW};
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    let mut out = Vec::new();
    unsafe {
        let me = GetCurrentProcess();
        let mut hs: Vec<HMODULE> = vec![std::ptr::null_mut(); 1024];
        let mut need = 0u32;
        let cap = (hs.len() * std::mem::size_of::<HMODULE>()) as u32;
        if K32EnumProcessModules(me, hs.as_mut_ptr(), cap, &mut need) == 0 {
            return out;
        }
        let n = (need as usize / std::mem::size_of::<HMODULE>()).min(hs.len());
        for &h in &hs[..n] {
            let base = h as usize;
            if base == 0 || *(base as *const u16) != 0x5a4d {
                continue;
            }
            let nt = base + *((base + 0x3c) as *const u32) as usize;
            if *(nt as *const u32) != 0x4550 {
                continue;
            }
            let opt = nt + 24;
            if *(opt as *const u16) != 0x20b {
                continue;
            }
            let size = *((opt + 56) as *const u32) as usize;
            let dirs = *((opt + 108) as *const u32);
            let (mut pdata, mut count) = (0usize, 0usize);
            if dirs > 3 {
                let rva = *((opt + 112 + 3 * 8) as *const u32) as usize;
                let len = *((opt + 112 + 3 * 8 + 4) as *const u32) as usize;
                if rva != 0 && rva + len <= size {
                    (pdata, count) = (rva, len / 12);
                }
            }
            let mut buf = [0u16; 260];
            let k = K32GetModuleBaseNameW(me, h, buf.as_mut_ptr(), buf.len() as u32) as usize;
            out.push(Module { base, size, name: String::from_utf16_lossy(&buf[..k]), pdata, count });
        }
    }
    out.sort_by_key(|m| m.base);
    out
}

fn module_of(mods: &[Module], a: u64) -> Option<&Module> {
    let a = a as usize;
    let i = mods.partition_point(|m| m.base <= a);
    let m = mods.get(i.checked_sub(1)?)?;
    (a < m.base + m.size).then_some(m)
}

fn name_of(mods: &[Module], a: u64) -> String {
    match module_of(mods, a) {
        Some(m) => format!("{}+{:#x}", m.name, a as usize - m.base),
        None => format!("{a:#x}"),
    }
}

#[repr(C, align(16))]
struct Aligned(CONTEXT);

/// How much of a thread's stack is copied (from its stack pointer up).
const COPY: usize = 256 * 1024;

/// What a thread's registers and stack were when it was stopped: the stack is a copy, so the thread runs on at once.
struct Snapshot {
    regs: [u64; 16],
    rip: u64,
    rsp: u64,
    len: usize,
}

/// (2.0.9) Players' 2.0.8 first launches crashed in ntdll 3 s after "stack not read in 3 s ... the thread was let go":
/// Windows' unwinder took the loader's lock while the main thread (stopped, loading a DLL) held it, and once the thread
/// was let go the unwinder carried on over a stack that was moving, and code with no unwind data (an overlay's hook)
/// sent it to a wild address, inside ntdll. Now: the thread is stopped only to copy its registers and the stack above its
/// stack pointer into a buffer allocated beforehand (no lock, no allocation, nothing that waits), and let go at once.
/// The walk then runs over that copy with this file's own unwinder: every read is checked against the copy, so it can't
/// fault, and it can't take long.
fn snapshot(tid: u32, buf: &mut [u8]) -> Result<Snapshot, String> {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery};
    unsafe {
        let h = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_INFORMATION, 0, tid);
        if h.is_null() {
            return Err(format!("(thread not opened: error {})", GetLastError()));
        }
        let mut boxed = Aligned(std::mem::zeroed());
        let ctx = &mut boxed.0;
        ctx.ContextFlags = CONTEXT_FULL_AMD64;
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        if SuspendThread(h) == u32::MAX {
            let e = GetLastError();
            CloseHandle(h);
            return Err(format!("(thread not stopped: error {e})"));
        }
        // (GetThreadContext wants its CONTEXT 16-byte aligned; windows-sys' CONTEXT is only 8)
        let got = GetThreadContext(h, ctx) != 0;
        let err = if got { 0 } else { GetLastError() };
        let mut len = 0usize;
        if got && ctx.Rsp != 0 && VirtualQuery(ctx.Rsp as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 {
            // (the committed run from the stack pointer's page up: the part of the stack the thread has used)
            let readable = mbi.State == MEM_COMMIT && mbi.Protect & (PAGE_GUARD | PAGE_NOACCESS) == 0 && mbi.Protect != 0;
            let end = mbi.BaseAddress as usize + mbi.RegionSize;
            if readable && end > ctx.Rsp as usize {
                len = (end - ctx.Rsp as usize).min(buf.len());
                std::ptr::copy_nonoverlapping(ctx.Rsp as usize as *const u8, buf.as_mut_ptr(), len);
            }
        }
        ResumeThread(h);
        CloseHandle(h);
        if !got {
            return Err(format!("(registers not read: error {err})"));
        }
        let c = &*ctx;
        let regs = [c.Rax, c.Rcx, c.Rdx, c.Rbx, c.Rsp, c.Rbp, c.Rsi, c.Rdi, c.R8, c.R9, c.R10, c.R11, c.R12, c.R13, c.R14, c.R15];
        Ok(Snapshot { regs, rip: c.Rip, rsp: c.Rsp, len })
    }
}

/// The return addresses from a snapshot, each with rbx as it was in that frame: x64 unwinding (the unwind codes of each
/// function's UNWIND_INFO) over the copied stack. A function with no unwind data is taken for a leaf (return address on
/// top). Stops at anything outside the copy.
fn walk(s: &Snapshot, stack: &[u8], mods: &[Module]) -> Vec<(u64, u64)> {
    let read = |a: u64| -> Option<u64> {
        let o = a.checked_sub(s.rsp)? as usize;
        let b = stack.get(o..o.checked_add(8)?)?;
        Some(u64::from_le_bytes(b.try_into().ok()?))
    };
    let mut r = s.regs;
    let mut rip = s.rip;
    let mut out = Vec::with_capacity(DEPTH);
    for _ in 0..DEPTH {
        if rip == 0 {
            break;
        }
        out.push((rip, r[3]));
        let was = r[4];
        let next = match module_of(mods, rip).and_then(|m| m.function((rip as usize - m.base) as u32).map(|f| (m, f))) {
            Some((m, f)) => unwind(m, f, rip, &mut r, &read),
            None => read(r[4]).map(|ret| {
                r[4] = r[4].wrapping_add(8);
                ret
            }),
        };
        match next {
            // (a frame always pops at least its return address: a stack that didn't move is a wrong turn)
            Some(n) if r[4] > was => rip = n,
            _ => break,
        }
    }
    out
}

/// One frame of x64 unwinding (as Windows' RtlVirtualUnwind, minus the epilog check): undoes the prolog's pushes,
/// allocations and frame register, follows chained unwind info, and returns the caller's address.
fn unwind(m: &Module, f: (u32, u32, u32), rip: u64, r: &mut [u64; 16], read: &dyn Fn(u64) -> Option<u64>) -> Option<u64> {
    let mut at = (rip as usize - m.base) as u32 - f.0;
    let mut info = f.2;
    let mut machine_rip = None;
    for _ in 0..32 {
        if info & 1 != 0 {
            // (an entry that points at another function entry)
            let e = m.bytes(info & !1, 12)?;
            info = u32::from_le_bytes([e[8], e[9], e[10], e[11]]);
            continue;
        }
        let head = m.bytes(info, 4)?;
        let (flags, count, freg, foff) = (head[0] >> 3, head[2] as usize, (head[3] & 15) as usize, (head[3] >> 4) as u64);
        let codes = m.bytes(info.checked_add(4)?, count * 2)?;
        let u16_at = |i: usize| u16::from_le_bytes([codes[2 * i], codes[2 * i + 1]]) as u64;
        let u32_at = |i: usize| u16_at(i) | (u16_at(i + 1) << 16);
        let ran = |i: usize| codes[2 * i] as u32 <= at;
        // (where MOV-saved registers sit: the frame register once the prolog has set it, else the stack pointer)
        let mut frame = r[4];
        if freg != 0 && (0..count).any(|i| codes[2 * i + 1] & 15 == 3 && ran(i)) {
            frame = r[freg].wrapping_sub(foff * 16);
        }
        let mut i = 0;
        while i < count {
            let (op, arg) = (codes[2 * i + 1] & 15, (codes[2 * i + 1] >> 4) as usize);
            let size = match op {
                1 => 2 + (arg != 0) as usize,
                4 | 6 | 8 => 2,
                5 | 9 => 3,
                _ => 1,
            };
            if i + size > count {
                return None;
            }
            if ran(i) {
                match op {
                    0 => {
                        r[arg] = read(r[4])?;
                        r[4] = r[4].wrapping_add(8);
                    }
                    1 => r[4] = r[4].wrapping_add(if arg == 0 { u16_at(i + 1) * 8 } else { u32_at(i + 1) }),
                    2 => r[4] = r[4].wrapping_add(arg as u64 * 8 + 8),
                    3 => r[4] = r[freg].wrapping_sub(foff * 16),
                    4 if arg != 4 => r[arg] = read(frame.wrapping_add(u16_at(i + 1) * 8))?,
                    5 if arg != 4 => r[arg] = read(frame.wrapping_add(u32_at(i + 1)))?,
                    10 => {
                        // (a machine frame: an interrupt or exception pushed rip/cs/flags/rsp/ss, after an error code if arg 1)
                        let o = if arg == 1 { 8 } else { 0 };
                        machine_rip = Some(read(r[4].wrapping_add(o))?);
                        r[4] = read(r[4].wrapping_add(o + 24))?;
                    }
                    _ => {}
                }
            }
            i += size;
        }
        if flags & 4 == 0 {
            break;
        }
        // (chained: the rest of the prolog is described by the parent function's unwind info, all of it executed)
        let e = m.bytes(info.checked_add(4 + (((count + 1) & !1) * 2) as u32)?, 12)?;
        info = u32::from_le_bytes([e[8], e[9], e[10], e[11]]);
        at = u32::MAX;
    }
    if let Some(rip) = machine_rip {
        return Some(rip);
    }
    let ret = read(r[4])?;
    r[4] = r[4].wrapping_add(8);
    Some(ret)
}

/// What the watchdog needs to read stacks: the module list (fresh for each report) and the stack buffer.
struct Reader {
    mods: Vec<Module>,
    buf: Vec<u8>,
}

impl Reader {
    fn new() -> Reader {
        Reader { mods: modules(), buf: vec![0u8; COPY] }
    }

    fn frames(&mut self, tid: u32) -> Result<Vec<(u64, u64)>, String> {
        let s = snapshot(tid, &mut self.buf)?;
        Ok(walk(&s, &self.buf[..s.len], &self.mods))
    }

    fn names(&self, frames: &[(u64, u64)], depth: usize) -> String {
        frames.iter().take(depth).map(|&(a, _)| name_of(&self.mods, a)).collect::<Vec<_>>().join(" < ")
    }

    fn named(&mut self, tid: u32, depth: usize) -> String {
        match self.frames(tid) {
            Ok(f) => self.names(&f, depth),
            Err(e) => e,
        }
    }

    /// (2.0.9) Players' 2.0.8 freeze: the main thread and another stuck in the game's lock wrapper (er+0x1ed9770, game
    /// 2.7.2: EnterCriticalSection(&lock->cs), the lock kept in rbx), and the thread holding the lock lost in the middle
    /// of the log, which Melty cuts. For a thread waiting there: which thread holds that lock, and how often it entered it.
    fn lock_owner(&self, frames: &[(u64, u64)]) -> Option<(u32, u32)> {
        let er = self.mods.iter().find(|m| m.name.eq_ignore_ascii_case("eldenring.exe"))?;
        // (lea rcx, [rbx+8]; call [EnterCriticalSection]: this game version's wrapper, or nothing)
        if er.bytes(0x1ed9796, 6)? != [0x48, 0x8d, 0x4b, 0x08, 0xff, 0x15] {
            return None;
        }
        let &(_, lock) = frames.iter().find(|&&(a, _)| a == (er.base + 0x1ed97a0) as u64)?;
        // (RTL_CRITICAL_SECTION at lock+8: LockCount, RecursionCount at +8/+0xc, OwningThread (an id) at +0x10)
        let cs = lock.checked_add(8)?;
        let counts = readable_u64(cs.checked_add(8)?)?;
        let owner = readable_u64(cs.checked_add(0x10)?)?;
        Some((owner as u32, (counts >> 32) as u32))
    }
}

/// A u64 of this process's memory, if the page is there to read.
fn readable_u64(a: u64) -> Option<u64> {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery};
    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        if a == 0 || VirtualQuery(a as *const _, &mut mbi, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) == 0 {
            return None;
        }
        let end = mbi.BaseAddress as u64 + mbi.RegionSize as u64;
        if mbi.State != MEM_COMMIT || mbi.Protect & (PAGE_GUARD | PAGE_NOACCESS) != 0 || mbi.Protect == 0 || a.checked_add(8)? > end {
            return None;
        }
        Some(std::ptr::read_unaligned(a as usize as *const u64))
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
fn every_thread(rd: &mut Reader, tid: u32, ft: u32) {
    // (2.0.9: not while the game boots: the first launch's build keeps the main thread busy for half a minute and more,
    // which is no freeze; the main thread's line is enough there)
    if beats() < BOOTED {
        crate::log::write_now("watchdog: (the other threads aren't read while the game boots)");
        return;
    }
    for t in threads() {
        if t != tid && t != ft {
            let line = match rd.frames(t) {
                Ok(f) => match rd.lock_owner(&f) {
                    Some((o, n)) => format!("{} [waits for a game lock held by thread {o}, entered {n}x]", rd.names(&f, 10)),
                    None => rd.names(&f, 10),
                },
                Err(e) => e,
            };
            crate::log::write_now(&format!("watchdog:   thread {t}: {line}"));
        }
    }
}

/// (2.0.9) Where a thread is, and if it waits for a game lock, who holds it and where that thread is: one line, written
/// last in a report (Melty sends a long log's start and end, and cuts the middle where every thread's line is).
fn summary(rd: &mut Reader, who: &str, tid: u32) -> String {
    match rd.frames(tid) {
        Ok(f) => match rd.lock_owner(&f) {
            Some((o, n)) => {
                let holder = rd.named(o, 14);
                format!("{who} {tid} at {} | waits for a game lock held by thread {o} (entered {n}x), which is at: {holder}", rd.names(&f, 8))
            }
            None => format!("{who} {tid} at {}", rd.names(&f, 12)),
        },
        Err(e) => format!("{who} {tid}: {e}"),
    }
}

/// Frames the game has drawn before the watchdog reads every thread (the overlay waits for as many).
const BOOTED: u64 = 600;

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
        let mut rd = Reader::new();
        crate::log::write_now(&format!("watchdog: the game's main thread {tid} is at: {}", rd.named(tid, DEPTH)));
        let ft = FRAME.load(Ordering::Relaxed);
        if ft != 0 && ft != tid {
            crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", rd.named(ft, DEPTH)));
        }
        every_thread(&mut rd, tid, ft);
        crate::log::write_now(&format!("watchdog: summary (no player for {:.0} s): {}", gone as f32 / 1000.0, summary(&mut rd, "main thread", tid)));
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
            let mut rd = Reader::new();
            crate::log::write_now(&format!("watchdog: the game's main thread {tid} is at: {}", rd.named(tid, DEPTH)));
            crate::log::write_now(&format!("watchdog: {}", summary(&mut rd, "main thread", tid)));
            let ft = FRAME.load(Ordering::Relaxed);
            if ft != 0 && ft != tid {
                crate::log::write_now(&format!("watchdog: the frame task's thread {ft} is at: {}", rd.named(ft, DEPTH)));
            }
            every_thread(&mut rd, tid, ft);
            // (2.0.7: the overlay's own GPU work: was its last frame done, was it inside the game's Present?)
            crate::log::write_now(&format!("watchdog: overlay: {}", hudhook::jr_state::report()));
            #[cfg(feature = "lab")]
            gpu_frames();
            // (a second look a few seconds later tells a deadlock (same place) from a slow loop)
            std::thread::sleep(Duration::from_secs(5));
            crate::log::write_now(&format!("watchdog: 5 s later: {}", rd.named(tid, 12)));
            crate::log::write_now(&format!("watchdog: summary (no frame for {:.0} s): {}", now_ms().saturating_sub(BEAT.load(Ordering::Relaxed)) as f32 / 1000.0, summary(&mut rd, "main thread", tid)));
        }
    });
}

/// lab (2.0.9): "watchdogstress.on" beside the logs: every thread's stack read over and over from the DLL's start (the
/// boot, the first launch's build, the world), 20 ms apart, to show the reader never faults, never stalls the game and
/// always lets the threads go. A line every 30 s with the counts.
#[cfg(feature = "lab")]
pub fn stress() {
    if !crate::log::dir().join("watchdogstress.on").exists() {
        return;
    }
    std::thread::spawn(|| {
        let mut rd = Reader::new();
        let (mut reads, mut frames, mut missed, mut deepest) = (0u64, 0u64, 0u64, 0usize);
        let (t0, mut said, mut listed) = (std::time::Instant::now(), std::time::Instant::now(), std::time::Instant::now());
        loop {
            if listed.elapsed() > Duration::from_secs(2) {
                rd.mods = modules();
                listed = std::time::Instant::now();
            }
            for t in threads() {
                match snapshot(t, &mut rd.buf) {
                    Ok(s) => {
                        let n = walk(&s, &rd.buf[..s.len], &rd.mods).len();
                        (reads, frames, deepest) = (reads + 1, frames + n as u64, deepest.max(n));
                    }
                    Err(_) => missed += 1,
                }
            }
            if said.elapsed() > Duration::from_secs(30) {
                crate::log::write_now(&format!("lab: watchdog stress: {reads} stacks read ({frames} frames, deepest {deepest}), {missed} not read, {} modules, {:.0} s", rd.mods.len(), t0.elapsed().as_secs_f32()));
                said = std::time::Instant::now();
            }
            std::thread::sleep(Duration::from_millis(20));
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
