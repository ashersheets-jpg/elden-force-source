//! 1.3.0 (lab: Starscourge Radahn is never on the game's by-distance character list, even 2 m away: a big open-world boss,
//! so abilities' targeting, their listed damage, the hit log and the parry never counted him). The characters near the
//! player: the game's by-distance list (its first `n`), then any character with a boss health bar it left out.

use std::ptr::NonNull;

use eldenring::cs::{CSFeManImp, ChrIns, WorldChrMan};
use fromsoftware_shared::FromStatic;

/// one character and its distance from the player (the game's list entry, the same fields)
pub struct Near {
    pub chr_ins: NonNull<ChrIns>,
    pub distance: f32,
}

pub fn near(wcm: &WorldChrMan, n: usize) -> Vec<Near> {
    let mut v: Vec<Near> = wcm.chr_inses_by_distance.iter().take(n).map(|e| Near { chr_ins: e.chr_ins, distance: e.distance }).collect();
    let Ok(fe) = (unsafe { CSFeManImp::instance() }) else { return v };
    for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()) {
        if v.iter().any(|e| unsafe { e.chr_ins.as_ref() }.field_ins_handle == b.field_ins_handle) {
            continue;
        }
        let Some(c) = wcm.chr_ins_by_handle(&b.field_ins_handle) else { continue };
        let distance = wcm.main_player.as_ref().map_or(0.0, |p| {
            let (a, q) = (p.chr_ins.modules.physics.position, c.modules.physics.position);
            ((a.0 - q.0).powi(2) + (a.1 - q.1).powi(2) + (a.2 - q.2).powi(2)).sqrt()
        });
        v.push(Near { chr_ins: NonNull::from(c), distance });
    }
    v
}
