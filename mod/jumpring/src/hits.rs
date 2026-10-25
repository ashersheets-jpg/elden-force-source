//! Hits landing (0.10.0): every enemy near the player is watched, and when its health drops an
//! impact burst goes off where it stands (world_fx.rs), in the active fighter's colour. Big hits (a
//! tenth of its health or more) get a bigger burst.

use std::collections::HashMap;

use eldenring::cs::{FieldInsHandle, PlayerIns, WorldChrMan};
use fromsoftware_shared::FromStatic;

#[derive(Default)]
pub struct Hits {
    hp: HashMap<FieldInsHandle, i32>,
}

impl Hits {
    pub fn tick(&mut self, player: &PlayerIns, color: [f32; 4]) {
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        let me = &player.chr_ins as *const _ as usize;
        let pos = player.chr_ins.modules.physics.position;
        let mut seen = HashMap::with_capacity(self.hp.len());
        for e in crate::chars::near(wcm, 32).iter() {
            if e.chr_ins.as_ptr() as usize == me {
                continue;
            }
            let c = unsafe { e.chr_ins.as_ref() };
            if c.modules.data.max_hp <= 0 {
                continue;
            }
            let p = c.modules.physics.position;
            let d = ((p.0 - pos.0).powi(2) + (p.1 - pos.1).powi(2) + (p.2 - pos.2).powi(2)).sqrt();
            if d > 40.0 {
                continue;
            }
            let hp = c.modules.data.hp;
            let h = c.field_ins_handle;
            if let Some(&old) = self.hp.get(&h) {
                if hp < old && old > 0 {
                    let max = c.modules.data.max_hp.max(1);
                    let big = (old - hp) * 10 >= max;
                    crate::world_fx::impact((p.0, p.1 + 1.2, p.2), color, big);
                }
            }
            seen.insert(h, hp);
        }
        self.hp = seen;
    }

    pub fn reset(&mut self) {
        self.hp.clear();
    }
}
