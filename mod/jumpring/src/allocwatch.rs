//! Lab only (2.0.1 crash hunt): the long lab runs end with the game quitting by itself about 30 minutes in: a script's Lua
//! allocation came back empty (luaM_realloc_ throwing its out-of-memory error, lab dump 2026-10-21), one of the game's
//! fixed-size heaps full. Every 30 s each of the game's allocators (its allocator manager's list) is read, through its own
//! getters: how full it is and how much it has grown since the first look, the fullest and the fastest-growing logged.

use std::collections::HashMap;
use std::time::Duration;

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;

/// eldenring.exe 2.7.2.0: the allocator manager's global, its list (begin/end at +8/+0x10 of the object at +0x480, entries
/// {allocator, start, end}) and its lock (+0x488, locked and unlocked by these two functions)
const MANAGER_RVA: usize = 0x4809858;
const LOCK_RVA: usize = 0x1f0f400;
const UNLOCK_RVA: usize = 0x1f0f580;
const LOCK_START: [u8; 12] = [0x48, 0x89, 0x5c, 0x24, 0x10, 0x57, 0x48, 0x83, 0xec, 0x20, 0x48, 0x83];
const UNLOCK_START: [u8; 12] = [0x40, 0x53, 0x48, 0x83, 0xec, 0x20, 0x48, 0x83, 0x79, 0x08, 0x00, 0x48];
/// DLAllocator vtable slots: heap_capacity, heap_size, heap_allocation_count
const CAPACITY: usize = 4;
const SIZE: usize = 5;
const COUNT: usize = 7;

struct Look {
    name: String,
    id: usize,
    backing: usize,
    cap: usize,
    size: usize,
    count: usize,
}

fn class_of(obj: usize, base: usize) -> String {
    unsafe {
        let vt = *(obj as *const usize);
        let col = *((vt - 8) as *const usize);
        if col < base || col > base + 0x6000000 {
            return format!("vt {:#x}", vt.wrapping_sub(base));
        }
        let td = *((col + 12) as *const u32) as usize + base;
        let name = std::ffi::CStr::from_ptr((td + 16) as *const std::ffi::c_char).to_string_lossy();
        // (.?AV...@@ trimmed to what reads)
        name.trim_start_matches(".?AV").trim_end_matches("@@").chars().take(90).collect()
    }
}

fn look(base: usize) -> Option<HashMap<usize, Look>> {
    let mgr = unsafe { *((base + MANAGER_RVA) as *const usize) };
    if mgr == 0 {
        return None;
    }
    let lock: unsafe extern "C" fn(usize, i32) -> u8 = unsafe { std::mem::transmute(base + LOCK_RVA) };
    let unlock: unsafe extern "C" fn(usize) = unsafe { std::mem::transmute(base + UNLOCK_RVA) };
    let mut seen: Vec<usize> = Vec::new();
    unsafe {
        lock(mgr + 0x488, -1);
        let list = *((mgr + 0x480) as *const usize);
        if list != 0 {
            let (begin, end) = (*((list + 8) as *const usize), *((list + 0x10) as *const usize));
            let mut e = begin;
            while e < end && seen.len() < 4096 {
                let a = *(e as *const usize);
                if a != 0 && !seen.contains(&a) {
                    seen.push(a);
                }
                e += 0x18;
            }
        }
        unlock(mgr + 0x488);
    }
    let mut out = HashMap::new();
    for a in seen {
        let vt = unsafe { *(a as *const *const usize) };
        let get = |slot: usize| -> usize {
            let f: unsafe extern "C" fn(usize) -> usize = unsafe { std::mem::transmute(*vt.add(slot)) };
            unsafe { f(a) }
        };
        out.insert(a, Look { name: class_of(a, base), id: get(1) & 0xffff_ffff, backing: get(6), cap: get(CAPACITY), size: get(SIZE), count: get(COUNT) });
    }
    Some(out)
}

/// lab (2.0.2): one reading now: allocator 4101 (the AI scripts' heap candidate) and the total over every allocator
pub fn sample(tag: &str) {
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    if unsafe { std::slice::from_raw_parts((base + LOCK_RVA) as *const u8, 12) } != LOCK_START {
        return;
    }
    let Some(now) = look(base) else { return };
    let total: usize = now.values().map(|l| l.size).sum();
    let ai = now.values().find(|l| l.id == 4101).map(|l| (l.size, l.count)).unwrap_or((0, 0));
    crate::log!("allocwatch: [{tag}] 4101 used {} in {} blocks; all {} allocators {} used", ai.0, ai.1, now.len(), total);
}

pub fn start() {
    let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
    let ok = unsafe { std::slice::from_raw_parts((base + LOCK_RVA) as *const u8, 12) } == LOCK_START
        && unsafe { std::slice::from_raw_parts((base + UNLOCK_RVA) as *const u8, 12) } == UNLOCK_START;
    if !ok {
        crate::log!("allocwatch: the allocator manager is not where 2.7.2.0 has it: off");
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30));
        let Some(now) = look(base) else { continue };
        let mut v: Vec<(&usize, &Look)> = now.iter().collect();
        v.sort_by_key(|(a, _)| **a);
        let mut msg = format!("allocwatch: {} allocators (id cap size backing blocks)", now.len());
        for (a, l) in v {
            let short: String = l.name.replace("?$HeapAllocator@V?$DLDynamicHeap@V?$", "").replace("@DLKR@@", "").chars().take(60).collect();
            msg += &format!("
  {a:#x} {} {} | {} {} {} {}", l.id, short, l.cap, l.size, l.backing, l.count);
        }
        crate::log!("{msg}");
    });
    crate::log!("allocwatch: the game's allocators logged every 2 minutes");
}
