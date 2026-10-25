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
        format!("private {} MB, working {} MB", c.PrivateUsage >> 20, c.WorkingSetSize >> 20)
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
