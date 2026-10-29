//! 1.3.0 (the user: "optimize the game as best as you can"): lab builds time each part of the frame task and log, every
//! ten seconds, each part's average and worst cost (perf::span). Release builds: an empty guard, nothing measured.

#[cfg(feature = "lab")]
mod imp {
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    struct Tally {
        total: Duration,
        worst: Duration,
        n: u32,
    }

    static TALLY: Mutex<Option<(Instant, HashMap<&'static str, Tally>)>> = Mutex::new(None);

    pub struct Span {
        name: &'static str,
        t0: Instant,
    }

    /// the game process's memory: private bytes and working set (MB), to watch long sessions for growth
    fn mem() -> String {
        use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS_EX};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        if unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut c as *mut _ as *mut _, c.cb) } == 0 {
            return "?".into();
        }
        format!("private {} MB, working {} MB, ours {} MB (peak {} MB)", c.PrivateUsage >> 20, c.WorkingSetSize >> 20,
            super::OURS.load(std::sync::atomic::Ordering::Relaxed) >> 20, super::OURS_PEAK.load(std::sync::atomic::Ordering::Relaxed) >> 20) + &format!(" [{}]", super::by_size())
            // (2.0.7: vilonly_dio's bullet-effect pool sat full (192 of 192, no bullets alive) from 24 min on: watched)
            + &format!(" | pool {} {}", crate::ability::pool_count(), crate::ability::pool_stats())
    }

    pub fn span(name: &'static str) -> Span {
        Span { name, t0: Instant::now() }
    }

    impl Drop for Span {
        fn drop(&mut self) {
            let d = self.t0.elapsed();
            let mut g = TALLY.lock().unwrap();
            let (since, m) = g.get_or_insert_with(|| (Instant::now(), HashMap::new()));
            let t = m.entry(self.name).or_insert(Tally { total: Duration::ZERO, worst: Duration::ZERO, n: 0 });
            t.total += d;
            t.worst = t.worst.max(d);
            t.n += 1;
            if self.name == "frame" && since.elapsed() > Duration::from_secs(10) {
                let mut v: Vec<_> = m.iter().map(|(k, t)| (*k, t.total.as_micros() as f64 / t.n.max(1) as f64, t.worst.as_micros(), t.n)).collect();
                v.sort_by(|a, b| b.1.total_cmp(&a.1));
                let s: Vec<String> = v.iter().map(|(k, avg, worst, n)| format!("{k} {avg:.0}/{worst} us x{n}")).collect();
                crate::log!("perf: {} | mem {}", s.join(", "), mem());
                *since = Instant::now();
                m.clear();
            }
        }
    }
}

#[cfg(feature = "lab")]
pub use imp::span;

#[cfg(not(feature = "lab"))]
pub struct Span;
#[cfg(not(feature = "lab"))]
#[inline(always)]
pub fn span(_: &'static str) -> Span {
    Span
}

/// lab (2.0.7 crash hunt): every byte our DLL holds from its own heap (Rust's allocator), to see how much of the game
/// process's memory is ours
#[cfg(feature = "lab")]
pub static OURS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
#[cfg(feature = "lab")]
pub static OURS_PEAK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// live bytes per allocation size class (2^k bytes, k = 0..40), to see what kind of data grows
#[cfg(feature = "lab")]
pub static BY_SIZE: [std::sync::atomic::AtomicIsize; 40] = [const { std::sync::atomic::AtomicIsize::new(0) }; 40];

#[cfg(feature = "lab")]
fn class(n: usize) -> usize {
    (usize::BITS - n.max(1).leading_zeros()) as usize % 40
}

/// lab: the size classes holding the most live bytes, "k:MB" (2^k-byte allocations)
#[cfg(feature = "lab")]
pub fn by_size() -> String {
    let mut v: Vec<(usize, isize)> = BY_SIZE.iter().enumerate().map(|(k, a)| (k, a.load(std::sync::atomic::Ordering::Relaxed))).collect();
    v.sort_by_key(|(_, b)| -b);
    v.iter().take(5).map(|(k, b)| format!("2^{k}:{:.1}", *b as f64 / 1048576.0)).collect::<Vec<_>>().join(" ")
}

/// 2.0.7: allocations that found the PC out of memory and waited for it (each wait 50 ms). Before, Rust's allocator
/// ended the game at the first failure (players' crash reports in jumpring.dll, during the first launch's build).
pub static ALLOC_WAITS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// An allocation that failed, tried again every 50 ms for up to 10 s while other work frees memory. (No logging here:
/// logging allocates. The count is in ALLOC_WAITS.)
fn patient(mut f: impl FnMut() -> *mut u8) -> *mut u8 {
    for _ in 0..200 {
        ALLOC_WAITS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let p = f();
        if !p.is_null() {
            return p;
        }
    }
    std::ptr::null_mut()
}

#[cfg(not(feature = "lab"))]
pub struct Patient;

#[cfg(not(feature = "lab"))]
unsafe impl std::alloc::GlobalAlloc for Patient {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::System.alloc(l) };
        if !p.is_null() { p } else { patient(|| unsafe { std::alloc::System.alloc(l) }) }
    }
    unsafe fn alloc_zeroed(&self, l: std::alloc::Layout) -> *mut u8 {
        let p = unsafe { std::alloc::System.alloc_zeroed(l) };
        if !p.is_null() { p } else { patient(|| unsafe { std::alloc::System.alloc_zeroed(l) }) }
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) };
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        let q = unsafe { std::alloc::System.realloc(p, l, n) };
        if !q.is_null() { q } else { patient(|| unsafe { std::alloc::System.realloc(p, l, n) }) }
    }
}

#[cfg(not(feature = "lab"))]
#[global_allocator]
static PATIENT: Patient = Patient;

#[cfg(feature = "lab")]
pub struct Counting;

#[cfg(feature = "lab")]
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: std::alloc::Layout) -> *mut u8 {
        let mut p = unsafe { std::alloc::System.alloc(l) };
        if p.is_null() {
            p = patient(|| unsafe { std::alloc::System.alloc(l) });
        }
        if !p.is_null() {
            let now = OURS.fetch_add(l.size(), std::sync::atomic::Ordering::Relaxed) + l.size();
            OURS_PEAK.fetch_max(now, std::sync::atomic::Ordering::Relaxed);
            BY_SIZE[class(l.size())].fetch_add(l.size() as isize, std::sync::atomic::Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: std::alloc::Layout) {
        unsafe { std::alloc::System.dealloc(p, l) };
        OURS.fetch_sub(l.size(), std::sync::atomic::Ordering::Relaxed);
        BY_SIZE[class(l.size())].fetch_sub(l.size() as isize, std::sync::atomic::Ordering::Relaxed);
    }
    unsafe fn realloc(&self, p: *mut u8, l: std::alloc::Layout, n: usize) -> *mut u8 {
        let mut q = unsafe { std::alloc::System.realloc(p, l, n) };
        if q.is_null() {
            q = patient(|| unsafe { std::alloc::System.realloc(p, l, n) });
        }
        if !q.is_null() {
            OURS.fetch_sub(l.size(), std::sync::atomic::Ordering::Relaxed);
            let now = OURS.fetch_add(n, std::sync::atomic::Ordering::Relaxed) + n;
            OURS_PEAK.fetch_max(now, std::sync::atomic::Ordering::Relaxed);
            BY_SIZE[class(l.size())].fetch_sub(l.size() as isize, std::sync::atomic::Ordering::Relaxed);
            BY_SIZE[class(n)].fetch_add(n as isize, std::sync::atomic::Ordering::Relaxed);
        }
        q
    }
}

#[cfg(feature = "lab")]
#[global_allocator]
static COUNTING: Counting = Counting;
