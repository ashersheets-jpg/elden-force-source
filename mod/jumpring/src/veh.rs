//! 2.0.1 crash hunt (lab): the game catches its own crashes and exits, and the dump Windows writes is of that exit (a
//! heap check failing in a static destructor), not of what went wrong. A first-chance exception handler logs every fault
//! before the game's handler runs: its code, where, and the stack's return addresses into the game and our DLL.
//! 2.0.7: on in the released build too, so the logs Melty sends with a player's crash say where it was (players' crashes
//! in eldenring.exe could only be guessed at): faults in the game, our DLL and the graphics libraries, and heap or stack
//! failures anywhere, each address a few times, a few dozen lines at most.

use std::sync::atomic::{AtomicU32, Ordering};

use windows_sys::Win32::System::Diagnostics::Debug::{AddVectoredExceptionHandler, EXCEPTION_POINTERS};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

#[cfg(feature = "lab")]
const MAX: u32 = 200;
#[cfg(not(feature = "lab"))]
const MAX: u32 = 40;
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

pub(crate) fn module_of(a: usize) -> Option<String> {
    for (name, w) in [("er", windows_sys::w!("eldenring.exe")), ("jr", windows_sys::w!("jumpring.dll")), ("ntdll", windows_sys::w!("ntdll.dll")), ("kb", windows_sys::w!("KERNELBASE.dll")), ("d3d12core", windows_sys::w!("D3D12Core.dll")), ("layers", windows_sys::w!("d3d12SDKLayers.dll")), ("d3d12", windows_sys::w!("d3d12.dll")), ("dxgi", windows_sys::w!("dxgi.dll")), ("questpath", windows_sys::w!("QuestPath.dll")), ("steamoverlay", windows_sys::w!("gameoverlayrenderer64.dll")), ("me2", windows_sys::w!("modengine2.dll")), ("ucrt", windows_sys::w!("ucrtbase.dll"))] {
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
    // 2.0.7: any other DLL by its file name (other mods in the game: the game folder's mod loader and its mods)
    unsafe {
        use windows_sys::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
        let mut h = std::ptr::null_mut();
        if a < 0x10000 || GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, a as *const u16, &mut h) == 0 {
            return None;
        }
        let mut buf = [0u16; 260];
        let n = GetModuleFileNameW(h, buf.as_mut_ptr(), buf.len() as u32) as usize;
        let path = String::from_utf16_lossy(&buf[..n]);
        let file = path.rsplit(['\\', '/']).next().unwrap_or("?").to_string();
        Some(format!("{file}+{:#x}", a - h as usize))
    }
}

/// 2.0.7: the memory around `a` (0x180 bytes before, 0x80 after), pages that can be read only, for a fault's log
pub(crate) fn hexdump(name: &str, a: usize) -> String {
    use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, PAGE_GUARD, PAGE_NOACCESS};
    let mut out = format!("\n  {name} {a:#x}");
    if a < 0x10000 {
        return out;
    }
    let readable = |p: usize| unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        VirtualQuery(p as _, &mut mbi, std::mem::size_of_val(&mbi)) != 0
            && mbi.State == MEM_COMMIT
            && mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) == 0
            && mbi.Protect != 0
    };
    unsafe {
        let mut mbi: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        if VirtualQuery(a as _, &mut mbi, std::mem::size_of_val(&mbi)) != 0 {
            out += &format!(" (region {:#x} +{:#x}, alloc {:#x}, type {:#x})", mbi.BaseAddress as usize, mbi.RegionSize, mbi.AllocationBase as usize, mbi.Type);
        }
    }
    let start = (a & !0xf).saturating_sub(0x180);
    for line in (start..(a & !0xf) + 0x80).step_by(16) {
        if !readable(line) {
            continue;
        }
        let b = unsafe { std::slice::from_raw_parts(line as *const u8, 16) };
        let words: Vec<String> = b.chunks(8).map(|c| format!("{:016x}", u64::from_le_bytes(c.try_into().unwrap()))).collect();
        out += &format!("\n    {line:x}{} {}", if line == (a & !0xf) { ">" } else { ":" }, words.join(" "));
    }
    out
}

static HEAP_DUMPS: AtomicU32 = AtomicU32::new(0);

static LUA_THROWS: AtomicU32 = AtomicU32::new(0);

unsafe extern "system" fn handler(info: *mut EXCEPTION_POINTERS) -> i32 {
    let rec = unsafe { &*(*info).ExceptionRecord };
    let code = rec.ExceptionCode as u32;
    // 2.0.6: a C++ throw out of luaM_realloc_ (its out-of-memory error, luaD_throw, which nothing catches and the game then
    // quits): its stack, and every allocator as it is now
    #[cfg(feature = "lab")]
    if code == 0xE06D7363 && LUA_THROWS.fetch_add(1, Ordering::Relaxed) < 6 {
        let ctx = unsafe { &*(*info).ContextRecord };
        let sp = ctx.Rsp as usize;
        let top = stack_base();
        let end = if sp < top && top - sp < 0x100_0000 { (sp + 0x2000).min(top) } else { sp };
        let er = unsafe { GetModuleHandleW(windows_sys::w!("eldenring.exe")) } as usize;
        let mut lua = false;
        let mut frames = String::new();
        let mut n = 0;
        for k in 0..(end - sp) / 8 {
            let v = unsafe { ((sp + k * 8) as *const usize).read_volatile() };
            // (inside luaM_realloc_ (er+0x203b360): its out-of-memory throw, not the scripts' ordinary pcall'd errors)
            if v >= er + 0x203b360 && v < er + 0x203b460 {
                lua = true;
            }
            if let Some(m) = module_of(v) {
                if n < 40 {
                    frames += &format!(" {m}");
                    n += 1;
                }
            }
        }
        if lua {
            crate::log::write_now(&format!("VEH: Lua threw (C++ exception) on thread {} params {:x?}
  stack:{frames}", unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() }, &rec.ExceptionInformation[..(rec.NumberParameters as usize).min(4)]));
            crate::allocwatch::dump_now("at the Lua throw");
        } else {
            LUA_THROWS.fetch_sub(1, Ordering::Relaxed);
        }
        return 0;
    }
    // 2.0.6: the D3D12 debug layer's messages (d3ddebug.rs), when it is on
    #[cfg(feature = "lab")]
    if code == 0x40010006 && crate::d3ddebug::ON.load(Ordering::Relaxed) {
        crate::d3ddebug::print(&rec.ExceptionInformation[..(rec.NumberParameters as usize).min(2)], unsafe { (*(*info).ContextRecord).Rsp } as usize);
        return 0;
    }
    // (routine: C++ throws, debugger prints, thread names, guard pages the game's own code uses)
    if matches!(code, 0xE06D7363 | 0x40010006 | 0x4001000A | 0x406D1388 | 0x80000001) || code < 0x80000000 {
        return 0;
    }
    let at = rec.ExceptionAddress as usize;
    // (released build: only what can end the game: a fault in the game, our DLL or the graphics libraries, or a heap / stack
    // failure anywhere (the game's own handled faults at start are in other libraries))
    #[cfg(not(feature = "lab"))]
    {
        let fatal_anywhere = matches!(code, 0xC0000374 | 0xC0000409 | 0xC00000FD | 0xC0000420);
        let ours = module_of(at).is_some_and(|m| ["er+", "jr+", "dxgi+", "d3d12", "questpath+", "steamoverlay+", "me2+"].iter().any(|p| m.starts_with(p)));
        if !fatal_anywhere && !ours {
            return 0;
        }
    }
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
    // 2.0.7: a fault in the game's memory allocator (the rare start-up fault at er+0x1f64f4d: a free block's tree node
    // found overwritten): every register, and the memory around the blocks they point at, which shows what wrote there
    if module_of(at).is_some_and(|m| m.starts_with("er+0x1f5") || m.starts_with("er+0x1f6")) && HEAP_DUMPS.fetch_add(1, Ordering::Relaxed) < 2 {
        msg += &format!(
            "\n  rax {:#x} rbx {:#x} rsi {:#x} rdi {:#x} rbp {:#x} r9 {:#x} r10 {:#x} r11 {:#x} r12 {:#x} r13 {:#x} r14 {:#x} r15 {:#x}",
            ctx.Rax, ctx.Rbx, ctx.Rsi, ctx.Rdi, ctx.Rbp, ctx.R9, ctx.R10, ctx.R11, ctx.R12, ctx.R13, ctx.R14, ctx.R15
        );
        for (name, a) in [("rdx", ctx.Rdx), ("r10", ctx.R10), ("rbx", ctx.Rbx), ("r13", ctx.R13), ("r15", ctx.R15)] {
            msg += &hexdump(name, a as usize);
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
