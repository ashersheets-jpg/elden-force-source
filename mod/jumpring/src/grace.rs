//! 0.15.0: back to a Site of Grace. When a fighter ends up stuck out of the world (in the void after a
//! fighter change, under the map, inside rock), the mod no longer pops them about trying to find solid
//! ground (0.14.x: that could loop for minutes, the body stuck under a floor or out of sight): it does what
//! a player would do with the map, a warp to the last grace they rested at. Runes and items stay.
//!
//! The grace is the one Elden Ring would respawn them at (CSLuaEventScriptImitation.bonfire_entity_id,
//! set when resting), with the mod's own record of the last grace it saw them sit at as a fallback. The
//! warp asks for the grace's block (BonfireWarpParam) and its player spawn point (the grace's entity id
//! minus 1000, where the map's own travel puts you).

use std::sync::atomic::{AtomicU32, Ordering};

use eldenring::cs::{BlockId, BonfireWarpParam, CSLuaEventManImp, GameMan, SoloParamRepository};
use fromsoftware_shared::FromStatic;

use crate::log;

/// The last grace the mod saw the player sit at (bonfire entity id).
static LAST_SEEN: AtomicU32 = AtomicU32::new(0);

/// The player object went away (a load: maybe another character): read the saved grace again.
pub fn forget() {
    LAST_SEEN.store(0, Ordering::Relaxed);
    TRIED.store(false, Ordering::Relaxed);
}

static TRIED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Every frame: remember the grace the player is resting at (and, per character, across sessions:
/// right after loading a save the game itself doesn't say).
pub fn tick(at_grace: bool, pgd: &eldenring::cs::PlayerGameData) {
    if LAST_SEEN.load(Ordering::Relaxed) == 0 {
        if !TRIED.swap(true, Ordering::Relaxed) {
            if let Some(id) = std::fs::read_to_string(crate::builds::char_path(pgd, "grace")).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
                LAST_SEEN.store(id, Ordering::Relaxed);
                log!("grace: last rested at {id} (saved)");
            }
        }
    }
    if !at_grace {
        return;
    }
    if let Some(id) = game_grace() {
        if LAST_SEEN.swap(id, Ordering::Relaxed) != id {
            log!("grace: resting at {id}");
            let p = crate::builds::char_path(pgd, "grace");
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, id.to_string());
        }
    }
}

fn game_grace() -> Option<u32> {
    let m = unsafe { CSLuaEventManImp::instance() }.ok()?;
    let id = m.lua_event_script_imitation.as_ref()?.bonfire_entity_id;
    (id > 0).then_some(id)
}

/// The grace to go back to: the one they last rested at, or else the nearest one to `near` (the map
/// tile they are on: Elden Ring doesn't say which grace it would respawn them at until they rest).
pub fn respawn_grace_near(near: Option<(BlockId, (f32, f32, f32))>) -> Option<u32> {
    game_grace().or_else(|| Some(LAST_SEEN.load(Ordering::Relaxed)).filter(|id| *id > 0)).or_else(|| near.and_then(|(b, p)| nearest(b, p)))
}

pub fn respawn_grace() -> Option<u32> {
    respawn_grace_near(None)
}

/// The grace nearest to `pos` on tile `block` (same area; open world tiles by grid distance, then by
/// distance on the tile).
fn nearest(block: BlockId, pos: (f32, f32, f32)) -> Option<u32> {
    if block.area() == 0xFF || block.area() == 0 {
        return None;
    }
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    (0..3000)
        .filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i))
        .filter(|r| r.bonfire_entity_id() > 0 && r.area_no() == block.area())
        .min_by_key(|r| {
            let tiles = (r.grid_x_no() as i32 - block.block() as i32).abs() + (r.grid_z_no() as i32 - block.region() as i32).abs();
            let d = ((r.pos_x() - pos.0).powi(2) + (r.pos_z() - pos.2).powi(2)).sqrt();
            tiles * 100_000 + d as i32
        })
        .map(|r| r.bonfire_entity_id())
}

/// The tile a grace is on and its spot there (block coordinates), from its BonfireWarpParam row.
fn place_of(entity: u32) -> Option<(BlockId, (f32, f32, f32))> {
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    let row = (0..3000).filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i)).find(|r| r.bonfire_entity_id() == entity)?;
    Some((BlockId::from_parts(row.area_no(), row.grid_x_no(), row.grid_z_no(), 0), (row.pos_x(), row.pos_y(), row.pos_z())))
}

/// A trip to a grace waiting for look.rs to make it.
static TRIP: std::sync::Mutex<Option<(BlockId, (f32, f32, f32))>> = std::sync::Mutex::new(None);
/// Trips made lately (a trip that keeps failing must not loop).
static TRIPS: std::sync::Mutex<Vec<std::time::Instant>> = std::sync::Mutex::new(Vec::new());

/// Lab (1.3.0): a trip to any spot of a map (Meruem's arena, Fortissax's dream, has no grace).
#[cfg(feature = "lab")]
pub fn trip_to(tile: BlockId, at: (f32, f32, f32)) {
    *TRIP.lock().unwrap() = Some((tile, at));
}

pub fn take_trip() -> Option<(BlockId, (f32, f32, f32))> {
    TRIP.lock().unwrap().take()
}

/// Warp the player to their grace. False when no grace is known (the caller keeps its own fallback).
pub fn warp_back(why: &str, here: BlockId, at: (f32, f32, f32)) -> bool {
    let Some(entity) = respawn_grace_near(Some((here, at))) else {
        log!("grace: {why}, but no grace is known to go back to");
        return false;
    };
    let Some((tile, at)) = place_of(entity) else {
        log!("grace: {why}, but grace {entity} has no warp row");
        return false;
    };
    {
        let mut t = TRIPS.lock().unwrap();
        t.retain(|x| x.elapsed().as_secs() < 90);
        if t.len() >= 2 {
            log!("grace: {why}, but two trips to a grace in a minute and a half already");
            return false;
        }
        t.push(std::time::Instant::now());
    }
    if unsafe { GameMan::instance() }.is_ok_and(|g| g.warp_requested) {
        return true;
    }
    *TRIP.lock().unwrap() = Some((tile, at));
    log!("grace: {why}: back to grace {entity}");
    true
}

/// 0.16.0: a trip asked for from outside the game: %LOCALAPPDATA%\JumpRing\warp.txt holding a Site of
/// Grace's BonfireWarpParam row id (e.g. 63415400 = Windmill Village). Taken once, the file removed.
#[cfg(feature = "lab")]
pub fn requested_trip() {
    let Some(path) = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("JumpRing").join("warp.txt")) else { return };
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    let _ = std::fs::remove_file(&path);
    let first = text.split_whitespace().next().unwrap_or("");
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return };
    // "e<entity>": the grace whose bonfire entity id that is (lab graceinfo lists them)
    let by_entity = first.strip_prefix('e').and_then(|t| t.parse::<u32>().ok()).and_then(|e| (0..3000).filter_map(|i| repo.get_row_by_index::<BonfireWarpParam>(i)).find(|r| r.bonfire_entity_id() == e));
    let row = first.parse::<u32>().unwrap_or(0);
    let Some(r) = by_entity.or_else(|| repo.get::<BonfireWarpParam>(row)) else {
        log!("grace: warp.txt: no grace row {row}");
        return;
    };
    let tile = BlockId::from_parts(r.area_no(), r.grid_x_no(), r.grid_z_no(), 0);
    // "boss" after the row: the field boss of that map tile fights again (its defeated flag,
    // 10 AA BB 0 800 for open-world tile m60_AA_BB, is cleared before the trip reloads the area)
    if text.split_whitespace().nth(1) == Some("boss") {
        respawn_field_boss(r.grid_x_no() as u32, r.grid_z_no() as u32);
    }
    // "on:<flag>" / "off:<flag>": set an event flag first (put back a flag a "boss" trip cleared)
    if let Ok(man) = unsafe { eldenring::cs::CSEventFlagMan::instance_mut() } {
        for t in text.split_whitespace().skip(1) {
            let (state, id) = match t.split_once(':') {
                Some(("on", id)) => (true, id),
                Some(("off", id)) => (false, id),
                _ => continue,
            };
            if let Ok(f) = id.parse::<u32>() {
                man.virtual_memory_flag.set_flag(f, state);
                log!("grace: flag {f} set {state}");
            }
        }
    }
    *TRIP.lock().unwrap() = Some((tile, (r.pos_x(), r.pos_y(), r.pos_z())));
    log!("grace: asked to go to grace row {row} (entity {})", r.bonfire_entity_id());
}

/// 0.16.0 (testing): the field boss on open-world tile m60_<x>_<z> fights again. Its defeated flag is
/// 10<x><z>0800; the neighbouring tiles' flags are logged (the boss may stand on the next tile).
#[cfg(feature = "lab")]
fn respawn_field_boss(x: u32, z: u32) {
    let Ok(man) = (unsafe { eldenring::cs::CSEventFlagMan::instance_mut() }) else { return };
    let id = |x: u32, z: u32| 1_000_000_000u32 + x * 1_000_000 + z * 10_000 + 800;
    let mut on = Vec::new();
    for dx in [-1i32, 0, 1] {
        for dz in [-1i32, 0, 1] {
            let f = id((x as i32 + dx) as u32, (z as i32 + dz) as u32);
            if man.virtual_memory_flag.get_flag(f) {
                on.push((dx.abs() + dz.abs(), f));
            }
        }
    }
    log!("grace: field boss flags set around m60_{x}_{z}: {:?}", on.iter().map(|(_, f)| f).collect::<Vec<_>>());
    // the tile's own boss only (a neighbour's flag may be another boss: the user's first try cleared
    // 1041530800 instead of the Apostle's 1042550800)
    if let Some((_, f)) = on.iter().find(|(d, _)| *d == 0) {
        man.virtual_memory_flag.set_flag(*f, false);
        log!("grace: flag {f} cleared: that boss fights again");
    }
}
