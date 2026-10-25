//! 1.3.0: The World (Jotaro's Star Platinum: The World, DIO's The World): time stops for everyone but the fighter.
//! Every enemy within TIMESTOP_RANGE has its animation (and with it its movement and attacks) frozen for the
//! move's time; a boss for half of it (a remembrance boss stopped for four seconds would be no fight). Each one's
//! own speed comes back when its time is up (or the fighter leaves the world).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use eldenring::cs::{CSFeManImp, FieldInsHandle, WorldChrMan};
use fromsoftware_shared::FromStatic;

use crate::log;

/// How far The World reaches (metres).
pub const TIMESTOP_RANGE: f32 = 30.0;
/// A boss stays stopped this share of the time.
const BOSS_SHARE: f32 = 0.5;

struct Frozen {
    h: FieldInsHandle,
    speed: f32,
    until: Instant,
}

static FROZEN: Mutex<Vec<Frozen>> = Mutex::new(Vec::new());

/// Time stops for `secs` around the player.
pub fn start(secs: f32) {
    start_within(secs, TIMESTOP_RANGE);
}

/// Every enemy within `range` stops for `secs` (1.3.0: Stun Grenade's blinding flash, 8 m).
pub fn start_within(secs: f32, range: f32) {
    let (Ok(wcm), fe) = (unsafe { WorldChrMan::instance_mut() }, unsafe { CSFeManImp::instance() }.ok()) else { return };
    let bosses: Vec<FieldInsHandle> = fe.map(|f| f.boss_health_displays.iter().filter(|b| b.fmg_id > 0).map(|b| b.field_ins_handle).collect()).unwrap_or_default();
    let now = Instant::now();
    let mut frozen = FROZEN.lock().unwrap();
    let near: Vec<(FieldInsHandle, bool)> = crate::chars::near(wcm, usize::MAX)
        .iter()
        .filter(|e| e.distance <= range)
        .map(|e| unsafe { e.chr_ins.as_ref() })
        .filter(|c| c.modules.data.hp > 0 && (crate::combat::hostile(c.team_type) || bosses.contains(&c.field_ins_handle)))
        .map(|c| (c.field_ins_handle, bosses.contains(&c.field_ins_handle)))
        .collect();
    for (h, boss) in near {
        let Some(c) = wcm.chr_ins_by_handle_mut(&h) else { continue };
        let until = now + Duration::from_secs_f32(if boss { secs * BOSS_SHARE } else { secs });
        match frozen.iter_mut().find(|f| f.h == h) {
            Some(f) => f.until = f.until.max(until),
            None => {
                frozen.push(Frozen { h, speed: c.modules.behavior.animation_speed.max(0.5), until });
                c.modules.behavior.animation_speed = 0.0;
            }
        }
    }
    log!("timestop: {} stopped for {secs:.1} s", frozen.len());
}

/// Every frame: time comes back for those whose stop is over.
pub fn tick() {
    let mut frozen = FROZEN.lock().unwrap();
    if frozen.is_empty() {
        return;
    }
    let now = Instant::now();
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
    frozen.retain(|f| {
        let Some(c) = wcm.chr_ins_by_handle_mut(&f.h) else { return false };
        if now >= f.until || c.modules.data.hp <= 0 {
            c.modules.behavior.animation_speed = f.speed;
            return false;
        }
        // (held at zero: the game's own speed changes would let it move)
        c.modules.behavior.animation_speed = 0.0;
        true
    });
    if frozen.is_empty() {
        log!("timestop: time moves again");
    }
}

/// Leaving the world (a load, a death): everyone's own speed back.
pub fn reset() {
    let mut frozen = FROZEN.lock().unwrap();
    if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
        for f in frozen.iter() {
            if let Some(c) = wcm.chr_ins_by_handle_mut(&f.h) {
                c.modules.behavior.animation_speed = f.speed;
            }
        }
    }
    frozen.clear();
}
