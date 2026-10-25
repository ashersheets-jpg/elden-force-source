//! Lab only, JUMPRING_HEAPHOOK=1 or heaphook.on beside the log (2.0.1 crash hunt): players' most common crash is Windows' heap corruption (c0000374), raised
//! inside a heap free in the game's own code (lab longplay, 31 minutes in: no Windows heap was damaged a second earlier, so the
//! free itself was bad: a block freed twice, or one whose header was overwritten). Every Windows heap allocation and free in the
//! process goes through here: each block's allocation call stack is kept; a free of a block already freed, or of a block that
//! no longer validates, is logged with the stacks of who made it, who freed it first and who frees it now, and skipped so the
//! session goes on (and can catch more).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::collections::{HashMap, VecDeque};
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use retour::RawDetour;
use windows_sys::Win32::System::Diagnostics::Debug::RtlCaptureStackBackTrace;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Memory::HeapValidate;

const DEPTH: usize = 12;
type Stack = [usize; DEPTH];
/// freed blocks remembered (to see a second free of one)
const FREED_KEPT: usize = 400_000;
/// reports at most
const MAX_REPORTS: u32 = 30;

type Alloc = unsafe extern "system" fn(*mut c_void, u32, usize) -> *mut c_void;
type Free = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> u8;
type ReAlloc = unsafe extern "system" fn(*mut c_void, u32, *mut c_void, usize) -> *mut c_void;

static ALLOC: OnceLock<RawDetour> = OnceLock::new();
static FREE: OnceLock<RawDetour> = OnceLock::new();
static REALLOC: OnceLock<RawDetour> = OnceLock::new();

#[derive(Default)]
struct Book {
    live: HashMap<usize, (usize, Stack)>,
    freed: HashMap<usize, (usize, Stack, Stack)>,
    order: VecDeque<usize>,
    reports: u32,
}

static BOOK: Mutex<Option<Book>> = Mutex::new(None);

/// the thread doing our bookkeeping (its own allocations go straight through): no thread-local here, the game's threads that
/// were running before this DLL loaded have no storage for one (lab: the first version crashed reading it)
static OWNER: AtomicU32 = AtomicU32::new(0);

fn tid() -> u32 {
    unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }
}

/// ntdll's own code range: its calls back into these come from inside a heap call, holding that heap's lock
static NTDLL: (AtomicUsize, AtomicUsize) = (AtomicUsize::new(0), AtomicUsize::new(0));

fn from_ntdll(s: &Stack) -> bool {
    let (lo, hi) = (NTDLL.0.load(Ordering::Relaxed), NTDLL.1.load(Ordering::Relaxed));
    lo <= s[0] && s[0] < hi
}

/// Run `f` as the bookkeeper unless this thread already is (then None). ntdll's own nested calls never wait for it (they
/// hold a heap's lock that our bookkeeping's own allocations need: waiting deadlocked the game at start) and skip it when
/// busy; every other caller waits (a skipped allocation left its address looking freed: false double frees)
fn guarded<R>(s: &Stack, f: impl FnOnce() -> R) -> Option<R> {
    let me = tid();
    if OWNER.load(Ordering::Acquire) == me {
        return None;
    }
    let g = if from_ntdll(s) {
        match LOCK.try_lock() {
            Ok(g) => g,
            Err(_) => return None,
        }
    } else {
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    };
    OWNER.store(me, Ordering::Release);
    let r = f();
    OWNER.store(0, Ordering::Release);
    drop(g);
    Some(r)
}

static LOCK: Mutex<()> = Mutex::new(());

fn stack() -> Stack {
    let mut s = [0usize; DEPTH];
    unsafe { RtlCaptureStackBackTrace(2, DEPTH as u32, s.as_mut_ptr() as *mut *mut c_void, std::ptr::null_mut()) };
    s
}

fn show(s: &Stack) -> String {
    s.iter().filter(|a| **a != 0).map(|a| format!("{a:#x}")).collect::<Vec<_>>().join(" ")
}

fn report(what: &str, ptr: usize, rec: Option<(usize, Stack)>, first_free: Option<Stack>, now: &Stack) {
    let mut msg = format!("HEAPHOOK: {what} block {ptr:#x}");
    if let Some((size, a)) = rec {
        msg += &format!(" ({size} bytes)
  made by: {}", show(&a));
    } else {
        msg += " (not seen made)";
    }
    if let Some(f) = first_free {
        msg += &format!("
  first freed by: {}", show(&f));
    }
    msg += &format!("
  freed now by: {}", show(now));
    crate::log::write_now(&msg);
}

/// Not on our own bookkeeping thread (its allocations go straight through)
fn outside() -> bool {
    OWNER.load(Ordering::Acquire) != tid()
}

unsafe extern "system" fn hk_alloc(heap: *mut c_void, flags: u32, size: usize) -> *mut c_void {
    let orig: Alloc = unsafe { std::mem::transmute(ALLOC.get().unwrap().trampoline()) };
    let p = unsafe { orig(heap, flags, size) };
    if !p.is_null() && outside() {
        // (the stack is taken before the lock: walking it can wait on the loader lock, whose holder may be allocating:
        // lab, the second version hung at start)
        let s = stack();
        guarded(&s, || note_made(p as usize, size, s));
    }
    p
}

fn note_made(p: usize, size: usize, s: Stack) {
    if let Ok(mut b) = BOOK.lock() {
        if let Some(b) = b.as_mut() {
            b.freed.remove(&p);
            b.live.insert(p, (size, s));
        }
    }
}

/// A free (or the free half of a reallocate) about to happen: reported if the block was freed already or no longer
/// validates. Report only: the free always goes ahead (the first version skipped such frees, and a reallocate's own inner
/// free of the old block, already marked freed, was taken for a second free: the game hung). `settle`: mark it freed now.
fn check_free(heap: *mut c_void, ptr: *mut c_void, s: Stack, settle: bool) {
    let key = ptr as usize;
    let Ok(mut g) = BOOK.lock() else { return };
    let Some(b) = g.as_mut() else { return };
    if !b.live.contains_key(&key) {
        if let Some((size, made, first)) = b.freed.get(&key).cloned() {
            if b.reports < MAX_REPORTS {
                b.reports += 1;
                drop(g);
                report("FREED TWICE", key, Some((size, made)), Some(first), &s);
            }
            return;
        }
    }
    // (only blocks seen made here: some heaps' blocks don't validate one by one)
    if b.live.contains_key(&key) && unsafe { HeapValidate(heap, 0, ptr) } == 0 {
        let rec = b.live.get(&key).cloned();
        if b.reports < MAX_REPORTS {
            b.reports += 1;
            drop(g);
            report("DAMAGED (does not validate)", key, rec, None, &s);
            return;
        }
    }
    if settle {
        settle_freed(b, key, s);
    }
}

fn settle_freed(b: &mut Book, key: usize, s: Stack) {
    if let Some((size, made)) = b.live.remove(&key) {
        b.freed.insert(key, (size, made, s));
        b.order.push_back(key);
        while b.order.len() > FREED_KEPT {
            if let Some(old) = b.order.pop_front() {
                b.freed.remove(&old);
            }
        }
    }
}

unsafe extern "system" fn hk_free(heap: *mut c_void, flags: u32, ptr: *mut c_void) -> u8 {
    let orig: Free = unsafe { std::mem::transmute(FREE.get().unwrap().trampoline()) };
    if !ptr.is_null() && outside() {
        let s = stack();
        guarded(&s, || check_free(heap, ptr, s, true));
    }
    unsafe { orig(heap, flags, ptr) }
}

unsafe extern "system" fn hk_realloc(heap: *mut c_void, flags: u32, ptr: *mut c_void, size: usize) -> *mut c_void {
    let orig: ReAlloc = unsafe { std::mem::transmute(REALLOC.get().unwrap().trampoline()) };
    let s = if !ptr.is_null() && outside() {
        let s = stack();
        guarded(&s, || check_free(heap, ptr, s, false));
        Some(s)
    } else {
        None
    };
    let p = unsafe { orig(heap, flags, ptr, size) };
    if !p.is_null() && outside() {
        let made = stack();
        guarded(&made, || {
            if let Ok(mut g) = BOOK.lock() {
                if let Some(b) = g.as_mut() {
                    // (the old block, moved: freed now unless its own inner free already said so)
                    if p != ptr {
                        if let Some(s) = s {
                            settle_freed(b, ptr as usize, s);
                        }
                    }
                    b.freed.remove(&(p as usize));
                    b.live.insert(p as usize, (size, made));
                }
            }
        });
    }
    p
}

pub fn start() {
    // (the variable, or a file "heaphook.on" beside the log: the game is started through Steam, which can drop the variable)
    let flag = crate::log::dir().join("heaphook.on");
    if std::env::var_os("JUMPRING_HEAPHOOK").is_none() && !flag.exists() {
        crate::log!("heaphook: off (no {})", flag.display());
        return;
    }
    *BOOK.lock().unwrap() = Some(Book::default());
    let ntdll = unsafe { GetModuleHandleW(windows_sys::w!("ntdll.dll")) };
    let mut mi: windows_sys::Win32::System::ProcessStatus::MODULEINFO = unsafe { std::mem::zeroed() };
    if unsafe {
        windows_sys::Win32::System::ProcessStatus::K32GetModuleInformation(
            windows_sys::Win32::System::Threading::GetCurrentProcess(), ntdll, &mut mi, std::mem::size_of_val(&mi) as u32)
    } != 0
    {
        NTDLL.0.store(mi.lpBaseOfDll as usize, Ordering::Relaxed);
        NTDLL.1.store(mi.lpBaseOfDll as usize + mi.SizeOfImage as usize, Ordering::Relaxed);
    }
    let get = |n: &[u8]| unsafe { GetProcAddress(ntdll, n.as_ptr()) }.map(|f| f as *const ());
    let (Some(a), Some(f), Some(r)) = (get(b"RtlAllocateHeap\0"), get(b"RtlFreeHeap\0"), get(b"RtlReAllocateHeap\0")) else {
        crate::log!("heaphook: ntdll functions not found");
        return;
    };
    let mk = |t: *const (), h: *const ()| unsafe { RawDetour::new(t, h) };
    match (mk(a, hk_alloc as *const ()), mk(f, hk_free as *const ()), mk(r, hk_realloc as *const ())) {
        (Ok(da), Ok(df), Ok(dr)) => {
            let _ = ALLOC.set(da);
            let _ = FREE.set(df);
            let _ = REALLOC.set(dr);
            unsafe {
                let _ = ALLOC.get().unwrap().enable();
                let _ = REALLOC.get().unwrap().enable();
                let _ = FREE.get().unwrap().enable();
            }
            let base = |n: *const u16| unsafe { GetModuleHandleW(n) } as usize;
            crate::log!("heaphook: watching every heap allocation and free (eldenring.exe {:#x}, jumpring.dll {:#x}, ntdll.dll {:#x},                 KERNELBASE.dll {:#x}, d3d12core.dll {:#x}, dxgi.dll {:#x})", base(std::ptr::null()), base(windows_sys::w!("jumpring.dll")),
                base(windows_sys::w!("ntdll.dll")), base(windows_sys::w!("KERNELBASE.dll")), base(windows_sys::w!("d3d12core.dll")), base(windows_sys::w!("dxgi.dll")));
        }
        _ => crate::log!("heaphook: could not hook the heap functions"),
    }
}
