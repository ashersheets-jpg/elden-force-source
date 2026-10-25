//! Lab only (2.0.1 crash hunt): players' most common crash (91 of them since 0.8.0, 2.0.0's main one) is Windows' heap
//! corruption (c0000374), found long after the bad write, most of it 15-20 minutes into play. Every Windows heap in the
//! process is checked once a second (HeapValidate), and the first second one fails is logged: what the log says happened in
//! that second points at the code that wrote where it shouldn't.

use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Memory::{GetProcessHeaps, HeapValidate};

const EVERY: Duration = Duration::from_secs(1);

fn heaps() -> Vec<HANDLE> {
    let n = unsafe { GetProcessHeaps(0, std::ptr::null_mut()) } as usize;
    let mut v: Vec<HANDLE> = vec![std::ptr::null_mut(); n + 8];
    let got = unsafe { GetProcessHeaps(v.len() as u32, v.as_mut_ptr()) } as usize;
    v.truncate(got.min(v.len()));
    v
}

pub fn start() {
    if std::env::var_os("JUMPRING_NO_HEAPCHECK").is_some() {
        return;
    }
    std::thread::spawn(|| {
        let started = Instant::now();
        let mut bad: Vec<usize> = Vec::new();
        let mut last_report = Instant::now();
        let mut slowest = Duration::ZERO;
        loop {
            std::thread::sleep(EVERY);
            let t0 = Instant::now();
            let hs = heaps();
            for (i, h) in hs.iter().enumerate() {
                let ok = unsafe { HeapValidate(*h, 0, std::ptr::null()) } != 0;
                let key = *h as usize;
                if !ok && !bad.contains(&key) {
                    bad.push(key);
                    crate::log::write_now(&format!(
                        "HEAPCHECK: heap {i} of {} ({key:#x}) CORRUPT, first seen {:.1} s after start (it was fine a second before)",
                        hs.len(),
                        started.elapsed().as_secs_f32()
                    ));
                }
            }
            slowest = slowest.max(t0.elapsed());
            if last_report.elapsed() > Duration::from_secs(60) {
                last_report = Instant::now();
                crate::log!("heapcheck: {} heaps, {} corrupt, slowest check {:.1} ms", hs.len(), bad.len(), slowest.as_secs_f32() * 1000.0);
            }
        }
    });
}
