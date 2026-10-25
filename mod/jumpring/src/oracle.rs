//! Lab oracle: logs HP drops of nearby characters ("HIT c#### npc=… hp a->b") so a test run proves
//! abilities really damage enemies. Built only with the `lab` feature.

use std::collections::HashMap;

use eldenring::cs::WorldChrMan;

use crate::log;

#[derive(Default)]
pub struct HitOracle {
    hp: HashMap<usize, i32>,
}

impl HitOracle {
    pub fn tick(&mut self, wcm: &WorldChrMan, last_ability: &str) {
        // JR_DUMMY (lab runs): enemies never die, so every ability can be measured on the same one
        let dummy = std::env::var_os("JR_DUMMY").is_some();
        let player = wcm.main_player.as_ref().map(|p| &p.chr_ins as *const _ as usize);
        let mut seen = HashMap::with_capacity(self.hp.len());
        for e in wcm.chr_inses_by_distance.iter().take(48) {
            let key = e.chr_ins.as_ptr() as usize;
            if Some(key) == player {
                continue;
            }
            let chr = unsafe { &mut *e.chr_ins.as_ptr() };
            let mut hp = chr.modules.data.hp;
            if let Some(&old) = self.hp.get(&key) {
                if hp < old {
                    log!("HIT c{:04} npc={} hp {}->{} (-{}) after {}", chr.character_id, chr.npc_param_id, old, hp, old - hp, last_ability);
                    if dummy && matches!(chr.team_type, 6 | 7) && hp * 3 < chr.modules.data.max_hp {
                        chr.modules.data.hp = chr.modules.data.max_hp;
                        hp = chr.modules.data.max_hp;
                    }
                }
            }
            seen.insert(key, hp);
        }
        self.hp = seen;
    }

    /// Lab key F1: put the player 5 m from the nearest living hostile (team 6) to test hits on.
    pub fn go_to_target(&self, wcm: &mut WorldChrMan) {
        self.go_to(wcm, false)
    }

    /// Lab key [: the same, to the toughest hostile around (team 6 or 7, most max HP): bosses.
    pub fn go_to_boss(&self, wcm: &mut WorldChrMan) {
        self.go_to(wcm, true)
    }

    fn go_to(&self, wcm: &mut WorldChrMan, boss: bool) {
        use eldenring::cs::CSHavokMan;
        use eldenring::position::{HavokPosition, PositionDelta};
        use fromsoftware_shared::FromStatic;
        let Some(me) = wcm.main_player.as_ref().map(|p| p.chr_ins.modules.physics.position) else { return };
        let mut cands: Vec<(f32, HavokPosition, u32)> = Vec::new();
        for e in wcm.chr_inses_by_distance.iter() {
            let chr = unsafe { e.chr_ins.as_ref() };
            let team_ok = if boss { matches!(chr.team_type, 6 | 7) } else { chr.team_type == 6 };
            if !team_ok || chr.modules.data.hp <= 0 || chr.modules.data.max_hp < 100 {
                continue;
            }
            let p = chr.modules.physics.position;
            let d = ((p.0 - me.0).powi(2) + (p.2 - me.2).powi(2)).sqrt();
            // bosses: most health first
            cands.push((if boss { -(chr.modules.data.max_hp as f32) } else { d }, p, chr.character_id));
        }
        cands.sort_by(|a, b| a.0.total_cmp(&b.0));
        let Ok(havok) = (unsafe { CSHavokMan::instance() }) else { return };
        let Some(player) = wcm.main_player.as_mut() else { return };
        for (d, t, id) in cands {
            let (dx, dz) = (me.0 - t.0, me.2 - t.2);
            let len = (dx * dx + dz * dz).sqrt().max(0.01);
            let gap = if boss { 12.0 } else { 5.0 };
            let spot = (t.0 + dx / len * gap, t.2 + dz / len * gap);
            // Solid ground within 3 m below the landing spot, and the target itself on the ground.
            let ground = havok.phys_world.cast_ray(0x2000058, &HavokPosition(spot.0, t.1 + 2.0, spot.1, 0.0), PositionDelta(0.0, -5.0, 0.0), player);
            let under_target = havok.phys_world.cast_ray(0x2000058, &HavokPosition(t.0, t.1 + 1.0, t.2, 0.0), PositionDelta(0.0, -2.5, 0.0), player);
            let (Some(g), Some(_)) = (ground, under_target) else { continue };
            let phys = &mut player.chr_ins.modules.physics;
            phys.position = HavokPosition(spot.0, g.1 + 0.1, spot.1, phys.position.3);
            // Face the target: forward = (-sin t, -cos t) for q = (0, sin t/2, 0, cos t/2).
            let th = (-(t.0 - spot.0)).atan2(-(t.2 - spot.1));
            phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
            phys.interpolated_orientation = phys.orientation;
            phys.chr_proxy_pos_update_requested = true;
            log!("lab: moved next to c{id:04} (was {d:.1}m away)");
            return;
        }
        log!("lab: no hostile standing on solid ground nearby");
    }

    /// Log the nearest characters (lab key F2) to find something to test on.
    pub fn dump(&self, wcm: &WorldChrMan) {
        let me = wcm.main_player.as_ref().map(|p| p.chr_ins.modules.physics.position);
        for e in wcm.chr_inses_by_distance.iter().take(12) {
            let chr = unsafe { e.chr_ins.as_ref() };
            let p = chr.modules.physics.position;
            let dist = me.map(|m| ((p.0 - m.0).powi(2) + (p.1 - m.1).powi(2) + (p.2 - m.2).powi(2)).sqrt()).unwrap_or(-1.0);
            log!("near c{:04} npc={} team={} hp {}/{} dist {:.1}m", chr.character_id, chr.npc_param_id, chr.team_type, chr.modules.data.hp, chr.modules.data.max_hp, dist);
        }
    }
}
