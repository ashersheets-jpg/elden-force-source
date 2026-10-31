//! Lab only (2.0.9), "poolcheck.on" beside the logs: a player's 2.0.8 crash a second after a fighter switch (er+0xe1fdc2)
//! was the game's small-block pool handing out a block from a free list whose next pointer was 2: something wrote into a
//! freed block. The pools (game 2.7.2): an allocator object with 32 size classes of 16 bytes (list heads at +0x50, 32
//! bytes apart), each a list of 256 KB pages (a header in the page's last 0x30 bytes: +0 the free list, +8 the blocks
//! handed out, +0x18 the link),
//! and its lock at +0x450 (taken through the game's wrapper er+0x1ed9770, a critical section at +8). Every pool in the
//! game's data is found once, and every 2 s each one's free lists are walked under its own lock: a pointer that leaves
//! its page is logged with the block before it, so the write shows up the moment it happens, crash or not.

use std::time::{Duration, Instant};

use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CRITICAL_SECTION, LeaveCriticalSection};

const PAGE: usize = 0x40000;
const CLASSES: usize = 32;
const LOCK: usize = 0x450;
const WRAPPER: usize = 0x1ed9770;

fn rd(a: usize) -> usize {
    unsafe { std::ptr::read_volatile(a as *const usize) }
}

/// The game image's .data section (start, end).
fn data_section(base: usize) -> Option<(usize, usize)> {
    unsafe {
        let nt = base + *((base + 0x3c) as *const u32) as usize;
        let n = *((nt + 6) as *const u16) as usize;
        let opt_size = *((nt + 20) as *const u16) as usize;
        let first = nt + 24 + opt_size;
        for i in 0..n {
            let s = first + i * 40;
            let name = std::slice::from_raw_parts(s as *const u8, 8);
            if name.starts_with(b".data\0") {
                let va = *((s + 12) as *const u32) as usize;
                let size = *((s + 8) as *const u32) as usize;
                return Some((base + va, base + va + size));
            }
        }
    }
    None
}

/// Does `a` look like a pool object: every size class an empty list (its back link pointing at itself: the game's own
/// test) or a list of 256 KB-aligned pages outside the image, at least one of them used. Returns (classes used, lock ok).
fn pool_shape(a: usize, image: (usize, usize)) -> Option<(usize, bool)> {
    let mut used = 0;
    let page = |n: usize| n > 0x10000 && (n < image.0 || n >= image.1) && (n.wrapping_add(0x18)) % PAGE == 0;
    for i in 0..CLASSES {
        let s = a + 0x50 + i * 32;
        let (next, prev) = (rd(s), rd(s + 8));
        if prev == s {
            continue;
        }
        if !page(next) || !page(prev) {
            return None;
        }
        used += 1;
    }
    // (LockCount of the critical section at +LOCK+8: -1 when nobody holds it, more negative while it's held or waited on)
    let count = rd(a + LOCK + 8 + 8) as u32 as i32;
    Some((used, count < 0 && count > -4096))
}

fn is_pool(a: usize, image: (usize, usize)) -> bool {
    matches!(pool_shape(a, image), Some((used, true)) if used > 0)
}

fn pools(base: usize) -> Vec<usize> {
    let Some((start, end)) = data_section(base) else { return Vec::new() };
    let image = (base, end);
    let mut out = Vec::new();
    let mut a = start;
    while a + LOCK + 0x30 < end {
        if is_pool(a, image) {
            out.push(a);
            a += LOCK;
        }
        a += 8;
    }
    out
}

/// One pool's free lists, under its lock. Returns (pages, free blocks) or the first damage found.
fn check(base: usize, pool: usize) -> Result<(usize, usize), String> {
    let enter: unsafe extern "C" fn(usize, i32) -> u32 = unsafe { std::mem::transmute(base + WRAPPER) };
    unsafe { enter(pool + LOCK, -1) };
    let r = walk(base, pool);
    unsafe { LeaveCriticalSection((pool + LOCK + 8) as *mut CRITICAL_SECTION) };
    r
}

fn walk(base: usize, pool: usize) -> Result<(usize, usize), String> {
    let (mut pages, mut blocks) = (0, 0);
    for i in 0..CLASSES {
        let size = (i + 1) * 16;
        let s = pool + 0x50 + i * 32;
        let mut node = rd(s);
        let mut n = 0;
        while node != s {
            n += 1;
            if n > 100_000 || node.wrapping_add(0x18) % PAGE != 0 {
                return Err(format!("pool er+{:#x} class {size}: page list broken at {node:#x}", pool - base));
            }
            // (a page's header is its last 0x30 bytes: the blocks lie between the page's start and it)
            let page = node - 0x18;
            let first = node + 0x18 - PAGE;
            pages += 1;
            let (mut f, mut prev, mut k) = (rd(page), 0usize, 0);
            while f != 0 {
                if f < first || f >= page || k > PAGE / 16 {
                    let around = if prev != 0 { dump(prev, size.max(32)) } else { String::new() };
                    return Err(format!(
                        "pool er+{:#x} class {size}: page {page:#x} (handed out {}) free list entry {k} is {f:#x}, written in the block at {prev:#x}: {around}",
                        pool - base,
                        rd(page + 8)
                    ));
                }
                prev = f;
                f = rd(f);
                k += 1;
                blocks += 1;
            }
            node = rd(node);
        }
    }
    Ok((pages, blocks))
}

fn dump(a: usize, len: usize) -> String {
    (0..len.min(64) / 8).map(|i| format!("{:016x}", rd(a + i * 8))).collect::<Vec<_>>().join(" ")
}

pub fn start() {
    if !crate::log::dir().join("poolcheck.on").exists() {
        return;
    }
    std::thread::spawn(|| {
        let base = unsafe { GetModuleHandleW(std::ptr::null()) } as usize;
        // (after the boot: the pools fill as the game starts)
        std::thread::sleep(Duration::from_secs(40));
        let mut found = pools(base);
        if found.is_empty() {
            // (what's near: addresses with the size classes' shape, whatever the lock looks like)
            if let Some((start, end)) = data_section(base) {
                let mut near = Vec::new();
                let mut a = start;
                while a + LOCK + 0x30 < end && near.len() < 12 {
                    if let Some((used, lock)) = pool_shape(a, (base, end)) {
                        if used > 0 {
                            near.push(format!("er+{:#x} ({used} used, lock {lock}, count {:#x})", a - base, rd(a + LOCK + 16) as u32));
                            a += LOCK;
                        }
                    }
                    a += 8;
                }
                crate::log::write_now(&format!("lab: poolcheck: none found; near: {}", near.join(", ")));
                // (the game's HeapStrategy object er+0x3b47628: its +8, and the pool the thunk er+0xe17920 makes of it)
                let obj = base + 0x3b47628;
                let inner = rd(obj + 8);
                crate::log::write_now(&format!("lab: poolcheck: obj er+0x3b47628: {} | +8 = {inner:#x}", dump(obj, 64)));
                for p in [inner.wrapping_add(0x28), base + 0x3b47638] {
                    if p > 0x10000 {
                        let q = |o: usize| format!("{:#x}", rd(p + o));
                        crate::log::write_now(&format!("lab: poolcheck: P {p:#x}: +0x48 {} +0x50 {} {} {} {} +0x70 {} {} | +0x450 {} {} {} {} {}", q(0x48), q(0x50), q(0x58), q(0x60), q(0x68), q(0x70), q(0x78), q(0x450), q(0x458), q(0x460), q(0x468), q(0x470)));
                    }
                }
            }
        }
        crate::log::write_now(&format!("lab: poolcheck: {} pools: {}", found.len(), found.iter().map(|p| format!("er+{:#x}", p - base)).collect::<Vec<_>>().join(" ")));
        let (mut said, mut rounds) = (Instant::now(), 0u64);
        loop {
            std::thread::sleep(Duration::from_secs(2));
            rounds += 1;
            let (mut pages, mut blocks) = (0, 0);
            let mut i = 0;
            while i < found.len() {
                match check(base, found[i]) {
                    Ok((p, b)) => {
                        pages += p;
                        blocks += b;
                        i += 1;
                    }
                    Err(e) => {
                        crate::log::write_now(&format!("lab: poolcheck: DAMAGE: {e}"));
                        // (one report per pool: it is damaged for good now)
                        found.remove(i);
                    }
                }
            }
            if said.elapsed() > Duration::from_secs(60) {
                crate::log::write_now(&format!("lab: poolcheck: round {rounds}, {} pools fine, {pages} pages, {blocks} free blocks", found.len()));
                said = Instant::now();
            }
        }
    });
}
