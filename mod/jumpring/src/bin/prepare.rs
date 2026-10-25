//! Elden Force's prepare step (src/prepare.rs): elden_force_prepare.exe, run by the starter before Elden Ring.
#![windows_subsystem = "windows"]
fn main() {
    std::process::exit(jumpring::prepare_main());
}
