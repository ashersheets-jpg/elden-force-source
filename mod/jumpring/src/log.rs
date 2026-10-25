//! Mod log at %LOCALAPPDATA%/JumpRing/logs/latest.log (hooks.log). The agent reads it after each run,
//! and Melty reads the co-op "Hosting at " line from it.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

static LOG: Mutex<Option<(File, Instant)>> = Mutex::new(None);

pub fn dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("JumpRing")
}

pub fn init() {
    let logs = dir().join("logs");
    let _ = fs::create_dir_all(&logs);
    let path = logs.join("latest.log");
    let _ = fs::rename(&path, logs.join("previous.log"));
    if let Ok(f) = OpenOptions::new().create(true).write(true).truncate(true).open(&path) {
        *LOG.lock().unwrap() = Some((f, Instant::now()));
    }
}

/// The starter's prepare step: its own log (prepare.log), so the game's latest / previous stay the game's.
pub fn init_named(name: &str) {
    SYNC.store(true, std::sync::atomic::Ordering::Relaxed);
    let logs = dir().join("logs");
    let _ = fs::create_dir_all(&logs);
    if let Ok(f) = OpenOptions::new().create(true).write(true).truncate(true).open(logs.join(name)) {
        *LOG.lock().unwrap() = Some((f, Instant::now()));
    }
}

/// 1.3.0 (optimising): lines go to a writer thread, so the game's frame never waits on the disk (each line was written and
/// flushed on the calling thread). Stamped when logged; the writer flushes whenever it has caught up.
static QUEUE: std::sync::OnceLock<Option<std::sync::mpsc::Sender<String>>> = std::sync::OnceLock::new();

fn queue() -> Option<&'static std::sync::mpsc::Sender<String>> {
    QUEUE
        .get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            std::thread::Builder::new()
                .name("jumpring-log".into())
                .spawn(move || {
                    while let Ok(line) = rx.recv() {
                        let mut g = LOG.lock().unwrap();
                        if let Some((f, _)) = g.as_mut() {
                            let _ = f.write_all(line.as_bytes());
                            while let Ok(more) = rx.try_recv() {
                                let _ = f.write_all(more.as_bytes());
                            }
                            let _ = f.flush();
                        }
                    }
                })
                .ok()
                .map(|_| tx)
        })
        .as_ref()
}

/// Lines written at once on the calling thread (the starter: a short-lived process that exits right after its last line)
static SYNC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Every line queued so far written out (before the process exits on purpose).
pub fn drain() {
    if QUEUE.get().is_some_and(|q| q.is_some()) {
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    SYNC.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn write(msg: &str) {
    if SYNC.load(std::sync::atomic::Ordering::Relaxed) {
        return write_now_inner(msg);
    }
    let line = {
        let g = LOG.lock().unwrap();
        let Some((_, t0)) = g.as_ref() else { return };
        format!("[{:>9.3}] {msg}
", t0.elapsed().as_secs_f64())
    };
    match queue() {
        Some(tx) if tx.send(line.clone()).is_ok() => {}
        _ => write_line(&line),
    }
}

/// Written at once on the calling thread (a panic: the process may not live for the writer).
pub fn write_now(msg: &str) {
    let line = {
        let g = LOG.lock().unwrap();
        let Some((_, t0)) = g.as_ref() else { return };
        format!("[{:>9.3}] {msg}
", t0.elapsed().as_secs_f64())
    };
    // (give the writer a moment to empty its queue first, so the panic comes after what led to it)
    std::thread::sleep(std::time::Duration::from_millis(20));
    write_line(&line);
}

fn write_now_inner(msg: &str) {
    let line = {
        let g = LOG.lock().unwrap();
        let Some((_, t0)) = g.as_ref() else { return };
        format!("[{:>9.3}] {msg}
", t0.elapsed().as_secs_f64())
    };
    write_line(&line);
}

fn write_line(line: &str) {
    if let Some((f, _)) = LOG.lock().unwrap().as_mut() {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

#[macro_export]
macro_rules! log {
    ($($t:tt)*) => { $crate::log::write(&format!($($t)*)) };
}
