// Elden Force's prepare step (1.2.0), for src/bin/prepare.rs. The starter (elden_force_start.exe) runs this before Elden Ring
// starts: it builds Elden Force's animations (the fighters' moves, the enemies' airborne reactions) from
// the player's own game files, so the game never has to while it loads. It sits next to jumpring.dll
// (modengine2/jumpring/) and finds the game through Steam, or takes its folder as the first argument.
// Exit code 0: ready (made or already up to date); 1: not made (the game will make them itself).

use std::path::PathBuf;

fn reg_string(path: &str, name: &str) -> Option<String> {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW};
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let (k, n) = (wide(path), wide(name));
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    let r = unsafe { RegGetValueW(HKEY_CURRENT_USER, k.as_ptr(), n.as_ptr(), RRF_RT_REG_SZ, std::ptr::null_mut(), buf.as_mut_ptr() as _, &mut len) };
    (r == 0).then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1)]))
}

/// Elden Ring's Game folder, from Steam's library list.
fn find_game() -> Option<PathBuf> {
    let steam = PathBuf::from(reg_string(r"Software\Valve\Steam", "SteamPath")?);
    let vdf = std::fs::read_to_string(steam.join("steamapps").join("libraryfolders.vdf")).ok()?;
    let mut libraries = vec![steam.clone()];
    for line in vdf.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("\"path\"") {
            let p = rest.trim().trim_matches('"').replace("\\\\", "\\");
            libraries.push(PathBuf::from(p));
        }
    }
    libraries
        .into_iter()
        .map(|l| l.join("steamapps").join("common").join("ELDEN RING").join("Game"))
        .find(|g| g.join("eldenring.exe").exists())
}

pub fn prepare_main() -> i32 {
    crate::log::init_named("prepare.log");
    let game = std::env::args().nth(1).map(PathBuf::from).filter(|g| g.join("eldenring.exe").exists()).or_else(find_game);
    let Some(game) = game else {
        crate::log!("prepare: Elden Ring not found; the game will make the animations itself");
        return 1;
    };
    crate::log!("prepare: Elden Ring at {}", game.display());
    let _ = crate::ermario::archive::GAME_DIR.set(game);
    let t0 = std::time::Instant::now();
    match crate::anims::prepare() {
        Ok(what) => {
            crate::log!("prepare: animations {what} ({:.1}s)", t0.elapsed().as_secs_f32());
            let t1 = std::time::Instant::now();
            let outfits = crate::recolor::prepare();
            crate::log!("prepare: outfits {outfits} ({:.1}s)", t1.elapsed().as_secs_f32());
            0
        }
        Err(e) => {
            crate::log!("prepare: {e}; the game will make the animations itself");
            1
        }
    }
}
