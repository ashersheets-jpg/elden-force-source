//! hooks.appearance: a fighter's hair (style + colour) and build (body proportions) while they are
//! active. The player's FaceData block (0xFFFFFFFF "FACE", 0x12F bytes, same layout as the save file)
//! lives inside PlayerGameData; we edit it in place and put the player's own values back on switch.
//! The player's original block is snapshotted to %LOCALAPPDATA%/JumpRing/face.bin before any change,
//! so it can be restored even after a crash.
//! Field offsets (relative to the block): hair model 0x14, head..legs 0xB0..0xB6, hair RGB 0xFA..0xFC.

use eldenring::cs::PlayerIns;

use crate::log;

pub const FACE_LEN: usize = 0x12F;
pub const OFF_HAIR_MODEL: usize = 0x14;
pub const OFF_BODY: usize = 0xB0; // head, chest, abdomen, arm R, leg R, arm L, leg L
pub const OFF_HAIR_RGB: usize = 0xFA;
pub const OFF_BROW_RGB: usize = 0x106;

/// Readable bytes from `addr`, up to `max` (stops at the first unreadable page).
fn readable(addr: usize, max: usize) -> usize {
    use windows_sys::Win32::System::Memory::{MEM_COMMIT, MEMORY_BASIC_INFORMATION, PAGE_GUARD, PAGE_NOACCESS, VirtualQuery};
    let mut len = 0;
    while len < max {
        let mut mbi: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe { VirtualQuery((addr + len) as _, &mut mbi, size_of::<MEMORY_BASIC_INFORMATION>()) } == 0 {
            break;
        }
        if mbi.State != MEM_COMMIT || mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD) != 0 {
            break;
        }
        len = (mbi.BaseAddress as usize + mbi.RegionSize) - addr;
    }
    len.min(max)
}

/// Find the FaceData block: "FACE" (version 4, inner size 0x120) in or after PlayerGameData.
/// Returns a pointer to the block start (4 bytes before "FACE", where the save keeps 0xFFFFFFFF).
pub fn find(player: &PlayerIns) -> Option<*mut u8> {
    find_in(player.player_game_data.as_ptr() as usize)
}

/// Same, from a PlayerGameData address.
pub fn find_in(base: usize) -> Option<*mut u8> {
    let len = readable(base, 0x10000);
    let hay = unsafe { std::slice::from_raw_parts(base as *const u8, len) };
    let at = hay.windows(12).position(|w| &w[0..4] == b"FACE" && w[4..8] == [4, 0, 0, 0] && w[8..12] == [0x20, 1, 0, 0])?;
    if at < 4 {
        return None;
    }
    Some((base + at - 4) as *mut u8)
}

pub fn read(face: *mut u8) -> [u8; FACE_LEN] {
    let mut b = [0u8; FACE_LEN];
    unsafe { std::ptr::copy_nonoverlapping(face, b.as_mut_ptr(), FACE_LEN) };
    b
}

pub fn write(face: *mut u8, b: &[u8; FACE_LEN]) {
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), face, FACE_LEN) };
}

/// Log where the block is and its key fields (lab probe).
pub fn probe(player: &PlayerIns) {
    let Some(face) = find(player) else {
        log!("appearance: FACE block not found in PlayerGameData");
        return;
    };
    let b = read(face);
    let off = face as usize - player.player_game_data.as_ptr() as usize;
    log!(
        "appearance: FACE at PlayerGameData+{off:#x} hair model {} body {:?} hair rgb {:?}",
        u32::from_le_bytes(b[OFF_HAIR_MODEL..OFF_HAIR_MODEL + 4].try_into().unwrap()),
        &b[OFF_BODY..OFF_BODY + 7],
        &b[OFF_HAIR_RGB..OFF_HAIR_RGB + 3]
    );
}

/// Lab: log the player's material modifiers (names and first values).
pub fn probe_modifiers(player: &PlayerIns) {
    let m = &player.chr_ins.modules.model_param_modifier;
    for e in m.modifiers.iter() {
        let name = unsafe { e.name.to_string() }.unwrap_or_default();
        log!("appearance: modifier {name}");
    }
}
