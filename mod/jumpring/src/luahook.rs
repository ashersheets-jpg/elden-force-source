//! Lab only, luahook.on beside the log (2.0.1 crash hunt). The crash players and the lab hit most (the game's DL_PANIC "given
//! memory block seems to be improper or freed already", or Windows' heap corruption at the next teardown) is a block of the
//! behaviour scripts' Lua memory freed twice, found in the Lua garbage collector freeing a table (lab dumps, 2026-10-21).
//! Every Lua allocation and free goes through one game function (luaM_realloc_, eldenring.exe 2.7.2.0 +0x203b360): here
//! each block's maker and freer are kept, and a second free of a block is logged with both stacks before the game panics.

use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use retour::RawDetour;
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

/// luaM_realloc_(L, block, osize, nsize) in eldenring.exe 2.7.2.0, and its first bytes (hooked only when they match)
const REALLOC_RVA: usize = 0x203b360;
const PROLOGUE: [u8; 16] = [0x48, 0x89, 0x5c, 0x24, 0x08, 0x48, 0x89, 0x6c, 0x24, 0x10, 0x48, 0x89, 0x74, 0x24, 0x18, 0x48];
const DEPTH: usize = 10;
const FREED_KEPT: usize = 300_000;
const MAX_REPORTS: u32 = 20;

type Stack = [usize; DEPTH];
type Realloc = unsafe extern "C" fn(usize, usize, usize, usize) -> usize;

static HOOK: OnceLock<RawDetour> = OnceLock::new();
static BASE: AtomicU64 = AtomicU64::new(0);
static CALLS: AtomicU64 = AtomicU64::new(0);
static UNKNOWN: AtomicU64 = AtomicU64::new(0);

#[derive(Default)]
struct Book {
    /// block -> (Lua state, size, who made it)
    live: HashMap<usize, (usize, u32, Stack)>,
    /// block -> (Lua state, size, who made it, who freed it, thread that freed it)
    freed: HashMap<usize, (usize, u32, Stack, Stack, u32)>,
    order: VecDeque<usize>,
    reports: u32,
}

static BOOK: Mutex<Option<Book>> = Mutex::new(None);

fn stack() -> Stack {
    let mut s = [0usize; DEPTH];
    unsafe { RtlCaptureStackBackTrace(2, DEPTH as u32, s.as_mut_ptr() as *mut *mut c_void, std::ptr::null_mut()) };
    s
}

/// the stack as module offsets where they are in the exe (eldenring.exe+... reads straight against the disassembly)
fn show(s: &Stack) -> String {
    let base = BASE.load(Ordering::Relaxed) as usize;
    s.iter()
        .filter(|a| **a != 0)
        .map(|a| if *a >= base && *a < base + 0x6000000 { format!("er+{:#x}", a - base) } else { format!("{a:#x}") })
        .collect::<Vec<_>>()
        .join(" ")
}

fn tid() -> u32 {
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

unsafe extern "C" fn hk_realloc(l: usize, block: usize, osize: usize, nsize: usize) -> usize {
    let orig: Realloc = unsafe { std::mem::transmute(HOOK.get().unwrap().trampoline()) };
    CALLS.fetch_add(1, Ordering::Relaxed);
    let (osize, nsize) = (osize as u32, nsize as u32);
    let now = if block != 0 { Some(stack()) } else { None };
    // a free (or a move's free of the old block): marked freed before the game frees it, so a second free on another
    // thread meanwhile is seen as one
    if let (Some(s), true) = (now, block != 0) {
        if let Ok(mut g) = BOOK.lock() {
            if let Some(b) = g.as_mut() {
                match b.live.remove(&block) {
                    Some((ls, size, made)) => {
                        if nsize == 0 {
                            b.freed.insert(block, (ls, size, made, s, tid()));
                            b.order.push_back(block);
                            while b.order.len() > FREED_KEPT {
                                if let Some(old) = b.order.pop_front() {
                                    b.freed.remove(&old);
                                }
                            }
                        } else {
                            // a resize: back as live until the result says where it went
                            b.live.insert(block, (ls, size, made));
                        }
                    }
                    None => {
                        if let Some((ls, size, made, first, ftid)) = b.freed.get(&block).cloned() {
                            if b.reports < MAX_REPORTS {
                                b.reports += 1;
                                let msg = format!(
                                    "LUAHOOK: block {block:#x} ({size} bytes, Lua state {ls:#x}) FREED TWICE (now by state {l:#x}, osize {osize} nsize {nsize}, thread {})\n  made by: {}\n  first freed by (thread {ftid}): {}\n  freed now by: {}",
                                    tid(),
                                    show(&made),
                                    show(&first),
                                    show(&s)
                                );
                                drop(g);
                                crate::log::write_now(&msg);
                            }
                        } else if UNKNOWN.fetch_add(1, Ordering::Relaxed) < 5 {
                            drop(g);
                            crate::log::write_now(&format!("luahook: a block not seen made: {block:#x} ({osize} -> {nsize}) by {}", show(&s)));
                        }
                    }
                }
            }
        }
    }
    let r = unsafe { orig(l, block, osize as usize, nsize as usize) };
    if nsize != 0 && r != 0 {
        let made = now.unwrap_or_else(stack);
        if let Ok(mut g) = BOOK.lock() {
            if let Some(b) = g.as_mut() {
                if block != 0 && r != block {
                    // moved: the old block went
                    if let Some((ls, size, m)) = b.live.remove(&block) {
                        b.freed.insert(block, (ls, size, m, made, tid()));
                        b.order.push_back(block);
                    }
                }
                b.freed.remove(&r);
                b.live.insert(r, (l, nsize, made));
            }
        }
    }
    r
}

pub fn start() {
    let flag = crate::log::dir().join("luahook.on");
    if !flag.exists() {
        crate::log!("luahook: off (no {})", flag.display());
        return;
    }
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    BASE.store(base as u64, Ordering::Relaxed);
    let target = (base + REALLOC_RVA) as *const ();
    let bytes = unsafe { std::slice::from_raw_parts(target as *const u8, PROLOGUE.len()) };
    if bytes != PROLOGUE {
        crate::log!("luahook: luaM_realloc_ not where 2.7.2.0 has it ({:02x?}): off", bytes);
        return;
    }
    *BOOK.lock().unwrap() = Some(Book::default());
    match unsafe { RawDetour::new(target, hk_realloc as *const ()) } {
        Ok(d) => {
            let _ = HOOK.set(d);
            if unsafe { HOOK.get().unwrap().enable() }.is_ok() {
                crate::log!("luahook: watching every Lua allocation (eldenring.exe at {base:#x})");
                std::thread::spawn(|| loop {
                    std::thread::sleep(std::time::Duration::from_secs(60));
                    let (live, freed) = BOOK.lock().ok().and_then(|g| g.as_ref().map(|b| (b.live.len(), b.freed.len()))).unwrap_or((0, 0));
                    crate::log!("luahook: {} calls, {live} live, {freed} freed kept, {} not seen made", CALLS.load(Ordering::Relaxed), UNKNOWN.load(Ordering::Relaxed));
                });
            } else {
                crate::log!("luahook: could not enable the hook");
            }
        }
        Err(e) => crate::log!("luahook: could not hook ({e})"),
    }
}
