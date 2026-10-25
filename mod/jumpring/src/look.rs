//! hooks.look: the active fighter's whole look (sheet: fighters outfit, hair_model, hair_rgb, body,
//! height). Outfit and appearance (FaceData) are written into the player's data; the game builds the
//! character from them at load, so a fighter change does a reload in place: warp into the current
//! block, then put the player back where they stood once the ground there has loaded. Height is the
//! model scale and applies live. The player's own FaceData is snapshotted to face.bin before the first
//! change and written back for Your Tarnished, so their own appearance always returns.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eldenring::cs::{CSHavokMan, GameMan, PlayerGameData, PlayerIns};
use eldenring::position::{HavokPosition, PositionDelta};
use eldenring::rotation::Quaternion;
use fromsoftware_shared::FromStatic;

use crate::appearance::{self, FACE_LEN, OFF_BODY, OFF_BROW_RGB, OFF_HAIR_MODEL, OFF_HAIR_RGB};
use crate::log;
use crate::outfit::Outfits;
use crate::sheets::Fighter;

const OFF_BEARD_RGB: usize = 0x100;
const OFF_SKIN_RGB: usize = 0xB7;

struct Return {
    /// block-local position (stable across loads; havok coordinates get re-based)
    block_pos: (f32, f32, f32),
    orientation: Quaternion,
    since: Instant,
    /// the load has happened (the player object went away and came back)
    loaded: bool,
    /// frames the player has been seen standing at the spot (released after enough of them)
    settled_frames: u32,
    /// the havok <-> block offset last frame (it must stop changing before we trust it)
    last_offset: Option<(f32, f32, f32)>,
    /// let go at (the watch afterwards puts them back if they slip)
    released: Option<Instant>,
    /// times put back after slipping
    retries: u32,
    /// the body's position last frame (havok), while watching
    last_pos: Option<(f32, f32, f32)>,
    /// falling over the void since
    void_since: Option<Instant>,
    /// 0.14.0: HP when the change began (nothing during the hold may cost the player any of it)
    hp_before: i32,
    /// frames the body was seen held at the spot touching ground with no map ray under it (standing on
    /// an object: a bridge, a ship, a lift; the map ray doesn't see those)
    touch_frames: u32,
    /// 0.14.1: the havok <-> block offset the spot is worked out with, locked once it was steady (the
    /// block position trails the havok one by a frame: recomputed every frame while we moved the body,
    /// the spot chased itself and the player swayed ~3 m for the whole hold, never let go), and how many
    /// frames in a row the offset has held still
    lock: Option<(f32, f32, f32)>,
    steady_n: u32,
    /// 0.15.0: times the offset was re-locked (a block position that never follows the body would
    /// re-lock forever and walk the spot away)
    relocks: u32,
    /// 0.15.0: the map tile they changed on (for the grace fallback when the reload never loads it)
    block: eldenring::cs::BlockId,
    /// 0.15.0: this is a trip back to a Site of Grace (grace.rs), not a fighter change
    to_grace: bool,
}

#[derive(Default)]
pub struct Look {
    pub outfits: Outfits,
    pub weapons: crate::weapon::Weapons,
    /// fighter whose look is written into the player's data
    applied: Option<&'static str>,
    /// a reload in place is under way: where to put the player back
    returning: Option<Return>,
    /// set when our own warp ends, so the load-in fighter select is skipped
    pub reloaded_by_us: bool,
    /// lab: hairstyle survey override, and turn to face the camera after the reload
    /// a reload is wanted but the world isn't settled yet (just loaded): do it shortly
    pending_reload: bool,
    /// when the player object appeared
    in_world_since: Option<Instant>,
    pub hair_override: Option<u32>,
    /// lab: armor survey override for the body piece
    pub body_override: Option<i32>,
    pub face_camera: bool,
    /// 0.14.0: the last spot the player really stood on (block position, orientation, when)
    last_ground: Option<((f32, f32, f32), Quaternion, Instant)>,
}

/// The FaceData fields a fighter changes: hair model, build sliders, skin, hair, beard and brow colour.
/// Only these are ever written; the rest of the block (and the memory around it) stays the game's.
const FIELDS: [(usize, usize); 6] = [(OFF_HAIR_MODEL, 4), (OFF_BODY, 7), (OFF_SKIN_RGB, 3), (OFF_HAIR_RGB, 3), (OFF_BEARD_RGB, 3), (OFF_BROW_RGB, 3)];
const SNAP_LEN: usize = 23;

/// The player's own values of FIELDS, one file per character (named after the character).
fn snapshot_path(pgd: &PlayerGameData) -> PathBuf {
    let name: String = String::from_utf16_lossy(&pgd.character_name)
        .chars()
        .take_while(|c| *c != '\u{0}')
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    let tag: u32 = pgd.character_name.iter().fold(2166136261u32, |h, c| (h ^ *c as u32).wrapping_mul(16777619));
    crate::log::dir().join(format!("face-{name}-{tag:08x}.bin"))
}

fn take_fields(face: &[u8; FACE_LEN]) -> [u8; SNAP_LEN] {
    let mut out = [0u8; SNAP_LEN];
    let mut at = 0;
    for (off, len) in FIELDS {
        out[at..at + len].copy_from_slice(&face[off..off + len]);
        at += len;
    }
    out
}

fn put_fields(face: *mut u8, snap: &[u8; SNAP_LEN]) {
    let mut at = 0;
    for (off, len) in FIELDS {
        unsafe { std::ptr::copy_nonoverlapping(snap[at..].as_ptr(), face.add(off), len) };
        at += len;
    }
}

/// The player's own values: from their snapshot, or (first change) from the live block, saved now.
/// 0.2.0 kept one whole-block face.bin; its fields become this character's snapshot.
fn own_fields(pgd: &PlayerGameData, face: *mut u8) -> Option<[u8; SNAP_LEN]> {
    let path = snapshot_path(pgd);
    if let Ok(b) = std::fs::read(&path) {
        if let Ok(s) = <[u8; SNAP_LEN]>::try_from(b.as_slice()) {
            return Some(s);
        }
    }
    let legacy = crate::log::dir().join("face.bin");
    let snap = match std::fs::read(&legacy) {
        Ok(b) if b.len() == FACE_LEN => take_fields(b.as_slice().try_into().unwrap()),
        _ => take_fields(&appearance::read(face)),
    };
    std::fs::write(&path, snap).ok()?;
    let _ = std::fs::remove_file(&legacy);
    Some(snap)
}

/// The FaceData block inside a PlayerGameData (search like appearance::find, from the data itself).
fn face_of(pgd: &mut PlayerGameData) -> Option<*mut u8> {
    appearance::find_in(pgd as *mut PlayerGameData as usize)
}

impl Look {
    /// Forget what was written, so the next write applies again (lab survey).
    pub fn force(&mut self) {
        self.applied = None;
    }

    /// Write `f`'s appearance and outfit into the player's data. Returns true if anything changed.
    pub fn write(&mut self, pgd: &mut PlayerGameData, f: &'static Fighter) -> bool {
        if self.applied == Some(f.id) {
            return false;
        }
        let Some(face) = face_of(pgd) else {
            log!("look: FaceData not found");
            return false;
        };
        let own = f.hair_model.is_none() && f.hair_rgb.is_none() && f.skin_rgb.is_none() && f.body.is_none();
        let Some(base) = own_fields(pgd, face) else {
            log!("look: could not save the own-appearance snapshot; not changing appearance");
            return false;
        };
        // Every fighter starts from the player's own values, so looks never stack; Your Tarnished gets them back.
        let mut b = appearance::read(face);
        put_fields(b.as_mut_ptr(), &base);
        if !own {
            // 1.3.0: a whole-body skin has its own head and hair: the player's own hair is shaved (hairstyle id 0, lab
            // hair survey), or it shows through behind short-haired models (Gojo, Yuji, Ichigo)
            let skin_hair = f.skin.is_some().then_some(0u32);
            if let Some(h) = self.hair_override.or(skin_hair).or(f.hair_model) {
                b[OFF_HAIR_MODEL..OFF_HAIR_MODEL + 4].copy_from_slice(&h.to_le_bytes());
            }
            if let Some(c) = f.hair_rgb {
                for off in [OFF_HAIR_RGB, OFF_BEARD_RGB, OFF_BROW_RGB] {
                    b[off..off + 3].copy_from_slice(&c);
                }
            }
            if let Some(c) = f.skin_rgb {
                b[OFF_SKIN_RGB..OFF_SKIN_RGB + 3].copy_from_slice(&c);
            }
            if let Some([head, chest, abdomen, arms, legs]) = f.body {
                b[OFF_BODY..OFF_BODY + 7].copy_from_slice(&[head, chest, abdomen, arms, legs, arms, legs]);
            }
        }
        put_fields(face, &take_fields(&b));
        let outfit = match (f.outfit, self.body_override) {
            (Some(mut o), Some(b)) => {
                o[1] = b;
                Some(o)
            }
            (o, _) => o,
        };
        crate::outfit::HIDE_BODY.store(f.skin.is_some(), std::sync::atomic::Ordering::Relaxed);
        self.outfits.apply(pgd, outfit, models_of(f), false);
        crate::weapon::show_for(f);
        self.weapons.apply(pgd, f.bare_hands, f.move_weapon, false);
        self.applied = Some(f.id);
        log!("look: {} written", f.name);
        true
    }

    /// Ask the game to rebuild the world around the player (a ~1 s load) and come back to this spot.
    /// 0.11.0: the player is held at the old spot from the first frame after the load (no gravity,
    /// nothing hurts) until the ground there has loaded. 0.10.0 left them at the block's default spawn
    /// meanwhile (in the open world, under the map) and they could fall to their death before it came.
    pub fn reload_in_place(&mut self, player: &PlayerIns) {
        // Warping in the first frames after a load (block id not valid yet) drops the game to the
        // title screen: wait until the player has been in the world for a moment.
        let settled = self.in_world_since.map(|t| t.elapsed() > Duration::from_secs(3)).unwrap_or(false);
        // 0.14.0: and until they have stood on the ground for a moment (at load-in the game may still be
        // placing them; 0.13.0 took that spot, found no ground there, held them for 60 s and let go)
        let stood = self.last_ground.is_some() || self.in_world_since.is_some_and(|t| t.elapsed() > Duration::from_secs(10));
        // 0.14.1: never for a dead player (0.14.0 started one at 0 HP and carried it into the respawn)
        if player.chr_ins.modules.data.hp <= 0 {
            self.pending_reload = true;
            return;
        }
        // 0.15.0: never toward an unloaded block (m255: the player out of the world): that "reload" took
        // 23 s and left them at the world's origin, in the void
        if !settled || !stood || player.current_block_id.area() == 0 || player.current_block_id.area() == 0xFF || self.returning.is_some() {
            self.pending_reload = true;
            return;
        }
        self.pending_reload = false;
        let Ok(gm) = (unsafe { GameMan::instance_mut() }) else { return };
        let phys = &player.chr_ins.modules.physics;
        // where they stand now, or (in the air, swinging) the last ground they stood on
        let on_ground = phys.is_touching_ground && !phys.is_falling && !crate::movement::BODY_FLOWN.load(std::sync::atomic::Ordering::Relaxed);
        let (block_pos, orientation) = match (&self.last_ground, on_ground) {
            (Some((b, q, _)), false) => (*b, *q),
            _ => ((player.block_position.x, player.block_position.y, player.block_position.z), phys.orientation),
        };
        self.returning = Some(Return {
            block_pos,
            orientation,
            since: Instant::now(),
            loaded: false,
            settled_frames: 0,
            last_offset: None,
            released: None,
            retries: 0,
            last_pos: None,
            void_since: None,
            hp_before: player.chr_ins.modules.data.hp,
            touch_frames: 0,
            lock: None,
            steady_n: 0,
            relocks: 0,
            block: player.current_block_id,
            to_grace: false,
        });
        gm.move_map_target = player.current_block_id;
        gm.initial_area_entity_id = 0;
        gm.warp_requested = true;
        log!("look: reload in place requested (tile {}, anim {})", player.current_block_id, crate::movement::current_anim(player));
    }

    /// 0.15.0: a trip to a Site of Grace asked for by grace.rs: the same load and hold as a fighter
    /// change, to the grace's tile and its spot (BonfireWarpParam's position: the grace's block
    /// coordinates). The game's own placement after such a load can be the tile's origin, deep under
    /// the ground in much of the open world: the hold puts them on the spot whatever it did.
    fn start_grace_trip(&mut self, player: &PlayerIns, tile: eldenring::cs::BlockId, at: (f32, f32, f32)) {
        let Ok(gm) = (unsafe { GameMan::instance_mut() }) else { return };
        self.pending_reload = false;
        self.returning = Some(Return {
            block_pos: at,
            orientation: player.chr_ins.modules.physics.orientation,
            since: Instant::now(),
            loaded: false,
            settled_frames: 0,
            last_offset: None,
            released: None,
            retries: 0,
            last_pos: None,
            void_since: None,
            hp_before: player.chr_ins.modules.data.hp.max(1),
            touch_frames: 0,
            lock: None,
            steady_n: 0,
            relocks: 0,
            block: tile,
            to_grace: true,
        });
        gm.move_map_target = tile;
        gm.initial_area_entity_id = 0;
        gm.warp_requested = true;
        log!("look: to the grace at ({:.1}, {:.1}, {:.1}) on tile {tile}", at.0, at.1, at.2);
    }

    /// A trip that can't be finished: the game's own way back to the last grace (a death; runes drop
    /// where they fell, as in Elden Ring). Only when the map would not load at all.
    fn last_resort(&mut self, player: &mut PlayerIns, why: &str) {
        log!("look: {why}; the game's own way back (death at the last grace)");
        self.returning = None;
        crate::movement::allow_death();
        player.chr_ins.chr_flags1c5.set_is_invincible(false);
        player.chr_ins.modules.physics.gravity_disabled = false;
        player.chr_ins.modules.data.hp = 0;
    }

    /// Every frame in world: height, keep the outfit right, finish a reload in place.
    pub fn tick(&mut self, player: &mut PlayerIns, f: &'static Fighter, scale: f32) {
        self.in_world_since.get_or_insert_with(Instant::now);
        if let Some((tile, at)) = crate::grace::take_trip() {
            if self.returning.as_ref().is_none_or(|r| r.loaded) && player.chr_ins.modules.data.hp > 0 {
                self.start_grace_trip(player, tile, at);
            }
        }
        if self.pending_reload {
            self.reload_in_place(player);
        }
        let ctrl = &mut player.chr_ins.chr_ctrl;
        ctrl.scale_size_x = f.height * scale;
        ctrl.scale_size_y = f.height * scale;
        ctrl.scale_size_z = f.height * scale;
        // the board comes and goes mid-air: redrawn at once
        // (1.3.0: and Luffy's Gear Fourth body)
        let board = BOARD.load(std::sync::atomic::Ordering::Relaxed) | if GEAR4.load(std::sync::atomic::Ordering::Relaxed) { 0x80 } else { 0 }
            | if SWING_PROP.load(std::sync::atomic::Ordering::Relaxed) && crate::recolor::swing_skin(f.id).is_some() { 0x40 } else { 0 };
        let live = BOARD_SHOWN.swap(board, std::sync::atomic::Ordering::Relaxed) != board;
        crate::outfit::HIDE_BODY.store(f.skin.is_some(), std::sync::atomic::Ordering::Relaxed);
        #[cfg(feature = "lab")]
        let hold = crate::lab::OUTFIT_HOLD.load(std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(feature = "lab"))]
        let hold = false;
        if !hold {
            self.outfits.apply(unsafe { player.player_game_data.as_mut() }, f.outfit, models_of(f), live);
        }
        crate::weapon::show_for(f);
        self.weapons.apply(unsafe { player.player_game_data.as_mut() }, f.bare_hands, f.move_weapon, false);
        // (1.3.0) a sword fighter's empty left hand throws no punch while the sword is held in one hand (binds.rs); in play only
        // (HUD state 3: the game's menus keep LB for their tabs)
        let one_handed = player.chr_asm.equipment.arm_style == eldenring::cs::ChrAsmArmStyle::OneHanded;
        crate::binds::HIDE_LEFT_HAND.store(f.move_weapon.is_some() && !crate::weapon::DUAL.load(std::sync::atomic::Ordering::Relaxed) && one_handed && crate::game_hud_state() == 3, std::sync::atomic::Ordering::Relaxed);

        // 0.14.0: the last ground they really stood on (not in our flight or swing, steady for 0.5 s)
        if self.returning.is_none() {
            let ph = &player.chr_ins.modules.physics;
            let flown = crate::movement::BODY_FLOWN.load(std::sync::atomic::Ordering::Relaxed);
            if ph.is_touching_ground && !ph.is_falling && !flown && player.chr_ins.modules.data.hp > 0 && self.in_world_since.is_some_and(|t| t.elapsed() > Duration::from_millis(1500)) {
                let b = (player.block_position.x, player.block_position.y, player.block_position.z);
                self.last_ground = Some((b, ph.orientation, Instant::now()));
            }
        }
        #[cfg(feature = "lab")]
        {
            static LAST: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);
            let mut l = LAST.lock().unwrap();
            if self.returning.is_some() && l.is_none_or(|t| t.elapsed() > Duration::from_millis(500)) {
                *l = Some(Instant::now());
                let r = self.returning.as_ref().unwrap();
                let p = player.chr_ins.modules.physics.position;
                log!("lab look state: loaded {} released {} settled {} touch {} body ({:.1}, {:.1}, {:.1})", r.loaded, r.released.is_some(), r.settled_frames, r.touch_frames, p.0, p.1, p.2);
            }
        }
        let Some(r) = &self.returning else { return };
        if !r.loaded {
            // Still before the load; if the game never starts one, give up after a while.
            if r.since.elapsed() > Duration::from_secs(10) {
                log!("look: the reload never started");
                self.returning = None;
            }
            return;
        }
        // 0.12.0: the game places the player at the block's default spawn for a while after the load,
        // and in the open world that is under the map. Hold them at the old spot (no gravity, nothing
        // hurts) until the game has really taken the position: at least HOLD_MIN, the havok <-> block
        // offset steady, the ground there, and the body seen standing on it for SETTLE_FRAMES frames.
        // Then watch for WATCH: a slip (below the spot or away from it) puts them back and holds again.
        // (0.11.0 let go on the first frame the ground answered; the game then moved them to its own
        // spawn and they fell to their death.)
        const HOLD_MIN: Duration = Duration::from_millis(1200);
        const SETTLE_FRAMES: u32 = 20;
        const WATCH: Duration = Duration::from_secs(6);
        let r = self.returning.as_mut().unwrap();
        let block0 = r.block;
        let block_pos0 = r.block_pos;
        let (cur_h, cur_b) = (player.chr_ins.modules.physics.position, &player.block_position);
        let offset = (cur_h.0 - cur_b.x, cur_h.1 - cur_b.y, cur_h.2 - cur_b.z);
        let steady = r.last_offset.is_some_and(|o| (o.0 - offset.0).abs() + (o.1 - offset.1).abs() + (o.2 - offset.2).abs() < 0.01);
        r.last_offset = Some(offset);
        r.steady_n = if steady { r.steady_n + 1 } else { 0 };
        // 0.15.0: right after the load the body sits at the world's origin (0, 0, 0) for a while with no
        // block: an offset read then is no offset at all
        let placed = (cur_h.0.abs() + cur_h.1.abs() + cur_h.2.abs()) > 0.01 && player.current_block_id.area() != 0xFF;
        if !placed {
            r.steady_n = 0;
            // 0.15.0: the game put them nowhere (the tile's origin with no tile: in much of the open world
            // that is deep under the ground, and the map never streams in). Right after such a load the
            // physics coordinates are the tile's own: the body goes to the spot by them, the game then
            // finds the tile and loads the map around it (lab, the player's own save: back at their grace).
            let waited = r.since.elapsed();
            if waited > Duration::from_millis(300) {
                if waited > Duration::from_secs(12) {
                    self.last_resort(player, "the map never loaded after the trip");
                    return;
                }
                let ph = &mut player.chr_ins.modules.physics;
                ph.position = HavokPosition(r.block_pos.0, r.block_pos.1 + 0.3, r.block_pos.2, ph.position.3);
                ph.chr_proxy_pos_update_requested = true;
                ph.gravity_disabled = true;
                player.chr_ins.modules.fall.fall_timer = 0.0;
                player.chr_ins.chr_flags1c5.set_is_invincible(true);
                crate::movement::set_no_dead(crate::movement::NO_DEAD_LOOK, true);
                give_back(player, r.hp_before);
            }
            return;
        }
        match r.lock {
            None if r.steady_n >= 3 => r.lock = Some(offset),
            // a re-base (the offset settled somewhere else): the spot moves with it
            Some(lo) if r.steady_n >= 10 && (lo.0 - offset.0).abs() + (lo.1 - offset.1).abs() + (lo.2 - offset.2).abs() > 1.0 => {
                #[cfg(feature = "lab")]
                log!("lab look: offset re-based by ({:.1}, {:.1}, {:.1})", offset.0 - lo.0, offset.1 - lo.1, offset.2 - lo.2);
                r.lock = Some(offset);
                r.relocks += 1;
            }
            _ => {}
        }
        let lo = if r.released.is_some() { offset } else { r.lock.unwrap_or(offset) };
        let target = (r.block_pos.0 + lo.0, r.block_pos.1 + lo.1, r.block_pos.2 + lo.2);
        let ground = unsafe { CSHavokMan::instance() }.ok().and_then(|h| {
            crate::motion::ray(player, &HavokPosition(target.0, target.1 + 1.5, target.2, 0.0), PositionDelta(0.0, -4.0, 0.0)).or_else(|| {
                // 0.14.0: a taller look (the saved height a little off): the nearest ground within 4 m of it
                crate::motion::ray(player, &HavokPosition(target.0, target.1 + 4.0, target.2, 0.0), PositionDelta(0.0, -12.0, 0.0))
                    .filter(|g| (g.1 - target.1).abs() < 4.0)
            })
        });
        let waited = r.since.elapsed();
        // standing on something the map ray doesn't see (an object): the body held at the spot and the
        // game saying it touches ground, for a while
        let touching = player.chr_ins.modules.physics.is_touching_ground && (cur_h.0 - target.0).abs() < 0.3 && (cur_h.2 - target.2).abs() < 0.3;
        r.touch_frames = if ground.is_none() && touching && steady { r.touch_frames + 1 } else { 0 };
        // and after 8 s, the spot itself (they stood there just before the change; the watch afterwards
        // still catches a slip): never again a minute held over nothing
        let late = waited > Duration::from_secs(8) && steady && (cur_h.0 - target.0).abs() < 0.3 && (cur_h.2 - target.2).abs() < 0.3;
        let stand_y = ground.map(|g| g.1 + 0.05).or_else(|| ((r.touch_frames > 90 && waited > Duration::from_secs(3)) || late).then_some(target.1 + 0.05));

        if let Some(at) = r.released {
            // watching after the let-go
            // a slip is the game's doing, not the player's: dropping well below the spot while near it,
            // or the body jumping more than 10 m in one frame (re-placed at the block's spawn). Running,
            // dashing or swinging away is not.
            let (dx, dz) = (cur_h.0 - target.0, cur_h.2 - target.2);
            let near = (dx * dx + dz * dz).sqrt() < 6.0;
            let jumped = r.last_pos.is_some_and(|p| ((p.0 - cur_h.0).powi(2) + (p.1 - cur_h.1).powi(2) + (p.2 - cur_h.2).powi(2)).sqrt() > 10.0) && steady;
            r.last_pos = Some((cur_h.0, cur_h.1, cur_h.2));
            // falling with nothing below (the void) for a moment counts too
            let ph = &player.chr_ins.modules.physics;
            let void = ph.is_falling && !ph.is_touching_ground && crate::motion::ground_below(player, cur_h.0, cur_h.1 + 0.3, cur_h.2, 300.0).is_none();
            if !void {
                r.void_since = None;
            }
            let in_void = void && r.void_since.get_or_insert_with(Instant::now).elapsed() > Duration::from_millis(800);
            let slipped = jumped || in_void || (near && cur_h.1 < stand_y.unwrap_or(target.1) - 3.0);
            if slipped && r.retries < 5 {
                r.retries += 1;
                r.released = None;
                r.settled_frames = 0;
                r.last_pos = None;
                log!("look: slipped off the spot after the reload ({:.1} m away, {:.1} m below); holding again", (dx * dx + dz * dz).sqrt(), stand_y.unwrap_or(target.1) - cur_h.1);
            } else if at.elapsed() > WATCH || slipped {
                give_back(player, r.hp_before);
                self.returning = None;
                crate::movement::set_no_dead(crate::movement::NO_DEAD_LOOK, false);
                if slipped {
                    crate::grace::warp_back("kept slipping off the spot after a fighter change", block0, block_pos0);
                }
            }
            return;
        }

        // nothing during the hold costs HP
        give_back(player, r.hp_before);
        let give_up = waited > Duration::from_secs(15) || r.relocks > 3;
        // 0.15.0: the body is only ever moved through a locked (steady) offset. Before it locks, it is
        // held where the game put it. (0.14.3 moved it through the unsteady one: when the block position
        // didn't follow the body, every frame's offset carried the previous frame's move, and the "spot"
        // ran away by kilometres: the player appeared in the void after a fighter change.)
        let far = ((cur_h.0 - target.0).powi(2) + (cur_h.2 - target.2).powi(2)).sqrt();
        if !give_up && (r.lock.is_none() || far > 1500.0) {
            let phys = &mut player.chr_ins.modules.physics;
            phys.gravity_disabled = true;
            player.chr_ins.modules.fall.fall_timer = 0.0;
            player.chr_ins.chr_flags1c5.set_is_invincible(true);
            crate::movement::set_no_dead(crate::movement::NO_DEAD_LOOK, true);
            r.settled_frames = 0;
            return;
        }
        {
            let phys = &mut player.chr_ins.modules.physics;
            if !give_up {
                phys.position = HavokPosition(target.0, stand_y.unwrap_or(target.1 + 0.05), target.2, phys.position.3);
                phys.chr_proxy_pos_update_requested = true;
            }
            phys.gravity_disabled = !give_up;
            phys.orientation = r.orientation;
            phys.interpolated_orientation = r.orientation;
        }
        player.chr_ins.modules.fall.fall_timer = 0.0;
        player.chr_ins.chr_flags1c5.set_is_invincible(!give_up);
        // nothing ends the fighter until the watch after the let-go is over
        crate::movement::set_no_dead(crate::movement::NO_DEAD_LOOK, !give_up);
        // the body is where we put it last frame (the game took it)
        let there = (cur_h.0 - target.0).abs() < 0.3 && (cur_h.2 - target.2).abs() < 0.3 && stand_y.is_some_and(|y| (cur_h.1 - y).abs() < 0.5);
        #[cfg(feature = "lab")]
        if stand_y.is_some() && !there {
            log!("lab look: held at {:.2}s but the body is at ({:.1}, {:.1}, {:.1}), the spot ({:.1}, {:.1}, {:.1}), offset steady {steady}", waited.as_secs_f32(), cur_h.0, cur_h.1, cur_h.2, target.0, stand_y.unwrap_or(0.0), target.2);
        }
        r.settled_frames = if there && steady && stand_y.is_some() { r.settled_frames + 1 } else { 0 };
        if (r.settled_frames >= SETTLE_FRAMES && waited >= HOLD_MIN) || give_up {
            let mut q = r.orientation;
            if std::mem::take(&mut self.face_camera) {
                // turn 180 degrees about Y (the camera starts behind the player)
                // (0, 1, 0, 0) * q, quaternions as (x, y, z, w)
                q = Quaternion(q.2, q.3, -q.0, -q.1);
            }
            let phys = &mut player.chr_ins.modules.physics;
            phys.orientation = q;
            phys.interpolated_orientation = q;
            phys.gravity_disabled = false;
            player.chr_ins.chr_flags1c5.set_is_invincible(false);
            if give_up {
                log!("look: the old spot never came back ({:.1}s, {} re-locks)", waited.as_secs_f32(), r.relocks);
                crate::movement::set_no_dead(crate::movement::NO_DEAD_LOOK, false);
                let to_grace = r.to_grace;
                self.returning = None;
                if to_grace {
                    self.last_resort(player, "no ground at the grace either");
                } else {
                    // 0.15.0: rather than leave them over nothing, back to their grace (as with the map)
                    crate::grace::warp_back("the spot never came back after a fighter change", block0, block_pos0);
                }
            } else {
                log!("look: back at {} after {:.1}s", if r.to_grace { "the grace" } else { "the old spot" }, waited.as_secs_f32());
                r.released = Some(Instant::now());
            }
        }
    }

    /// A reload in place is holding the player at their spot (nothing else may move them).
    pub fn holding(&self) -> bool {
        self.returning.as_ref().is_some_and(|r| r.loaded && r.released.is_none())
    }

    /// 1.1.3: a reload in place (or a grace trip) of ours is under way: its hold must run from the first
    /// frame after the load.
    pub fn busy(&self) -> bool {
        self.returning.is_some()
    }

    /// The player object went away (loading). Our own warp is under way if `returning` is set.
    pub fn on_unload(&mut self) {
        self.last_ground = None;
        // a hold that never got to its load (died first) is dropped
        if self.returning.as_ref().is_some_and(|r| r.loaded) {
            self.returning = None;
        }
        self.outfits.forget();
        self.weapons.forget();
        self.in_world_since = None;
        if let Some(r) = &mut self.returning {
            r.loaded = true;
            r.since = Instant::now();
            self.reloaded_by_us = true;
        }
    }
}

/// HP lost while the mod held the player (a reload in place) is given back.
fn give_back(player: &mut PlayerIns, before: i32) {
    let d = &mut player.chr_ins.modules.data;
    if d.hp > 0 && d.hp < before {
        d.hp = before.min(d.max_hp);
    }
}

/// 1.2.0: Asta rides his blade (movement.rs glide): his body shows the board version (recolor::board_model;
/// 0: his own). `foot`: the lab's foot-bound version.
pub static BOARD: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
pub const BOARD_PELVIS: u8 = 1;
pub const BOARD_FOOT: u8 = 2;
/// 1.3.0: Luffy is in Gear Fourth (his Awakening, scripts Step::Gear4): his body shows the Boundman version; Gojo's
/// without the blindfold, Yuji's as Sukuna (recolor::awakened_skin)
pub static GEAR4: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static BOARD_SHOWN: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
/// 1.3.0: the fighter is swinging: Gon's body shows his fishing rod in his hand (recolor::swing_skin)
pub static SWING_PROP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The recoloured model of each of the fighter's pieces, once they exist (hooks.recolor).
fn models_of(f: &Fighter) -> [Option<u16>; 4] {
    if !crate::recolor::READY.load(std::sync::atomic::Ordering::Relaxed) {
        return [None; 4];
    }
    let fi = crate::sheets::FIGHTERS.iter().position(|x| x.id == f.id).unwrap_or(0);
    let board = BOARD.load(std::sync::atomic::Ordering::Relaxed);
    std::array::from_fn(|p| match (p, board) {
        (1, b) if b > 0 && f.id == "asta" => Some(crate::recolor::board_model(fi, b == BOARD_FOOT)),
        (1, _) if crate::recolor::awakened_skin(f.id).is_some() && GEAR4.load(std::sync::atomic::Ordering::Relaxed) => Some(crate::recolor::gear4_model(fi)),
        (1, _) if crate::recolor::swing_skin(f.id).is_some_and(crate::recolor::skin_exists) && SWING_PROP.load(std::sync::atomic::Ordering::Relaxed) => Some(crate::recolor::swing_model(fi)),
        _ => crate::recolor::has_model(f, p).then(|| crate::recolor::new_model(fi, p)),
    })
}
