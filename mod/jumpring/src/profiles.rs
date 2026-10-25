//! hooks.speffect: fighter stat profiles (sheet: stat_profiles) as resident SpEffects. Each profile's
//! SpEffectParam row (unused "Area Scaling" rows) is rewritten in memory once params exist, then the
//! active fighter's row is applied to the player and the previous one removed.

use eldenring::cs::{ChrInsExt, PlayerIns, SoloParamRepository, SpEffectParam};
use eldenring::param::SP_EFFECT_PARAM_ST;
use fromsoftware_shared::FromStatic;

use crate::log;
use crate::sheets::{self, StatProfile};

/// SpEffectParam row copied into each profile row first (Crimson Amber Medallion).
const TEMPLATE: u32 = 310000;
/// Fall damage taken, x the game's (0.8.0: the fighters' speed and air moves made falls a big hazard;
/// 0.12.0: a hundredth of 0.8.0's 0.25, fighters kept dying to their own dashes off ledges).
pub const FALL_DAMAGE: f32 = 0.0025;

/// Write every profile row; returns false if params are not loaded yet.
pub fn write_rows() -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    // Start every profile row from the Crimson Amber Medallion's effect (310000): a permanent,
    // icon-less HP-rate buff. Without this the repurposed rows keep their old resistance fields,
    // which make the game show its status buildup meters after every load.
    let Some(template) = repo.get::<SpEffectParam>(TEMPLATE).map(|r| r as *const SP_EFFECT_PARAM_ST) else {
        log!("profiles: template SpEffect {TEMPLATE} missing");
        return false;
    };
    let mut n = 0;
    log!("profiles: template fall damage rate {}", unsafe { &*template }.fall_damage_rate());
    for p in sheets::STAT_PROFILES {
        let Some(id) = p.er_speffect else { continue };
        let Some(row) = repo.get_mut::<SpEffectParam>(id as u32) else {
            log!("profiles: SpEffect {id} missing");
            continue;
        };
        unsafe { std::ptr::copy_nonoverlapping(template, row as *mut SP_EFFECT_PARAM_ST, 1) };
        row.set_effect_endurance(-1.0);
        row.set_icon_id(-1);
        row.set_vfx_id(-1);
        row.set_state_info(0);
        row.set_max_hp_rate(p.hp_mult);
        row.set_max_mp_rate(p.fp_mult);
        row.set_max_stamina_rate(p.stamina_mult);
        let cut = p.damage_taken_mult;
        row.set_slash_damage_cut_rate(cut);
        row.set_blow_damage_cut_rate(cut);
        row.set_thrust_damage_cut_rate(cut);
        row.set_neutral_damage_cut_rate(cut);
        row.set_magic_damage_cut_rate(cut);
        row.set_fire_damage_cut_rate(cut);
        row.set_thunder_damage_cut_rate(cut);
        row.set_dark_damage_cut_rate(cut);
        row.set_fall_damage_rate(FALL_DAMAGE);
        // 0.10.0: JUMP FORCE fighters fight barehanded (weapon.rs): their Ash of War (LT's Endure, the
        // ones their moves play) costs no FP. Your Tarnished (balanced) keeps the game's costs.
        if p.id != "balanced" {
            row.set_arts_consumption_rate(0.0);
        }
        row.set_effect_target_self(true);
        row.set_effect_target_player(true);
        row.set_effect_target_live(true);
        row.set_effect_target_ghost(true);
        row.set_effect_target_enemy(false);
        row.set_effect_target_friend(false);
        row.set_effect_target_ai(false);
        n += 1;
    }
    log!("profiles: wrote {n} stat profile SpEffects");
    n > 0
}

/// Swap the player's profile SpEffect from `old` to `new`.
pub fn switch(player: &mut PlayerIns, old: Option<&StatProfile>, new: &StatProfile) {
    if let Some(id) = old.and_then(|p| p.er_speffect) {
        player.chr_ins.remove_speffect(id);
    }
    // Remove any other profile row (e.g. after a reload) so only one is active.
    for p in sheets::STAT_PROFILES {
        if let Some(id) = p.er_speffect {
            if p.id != new.id {
                player.chr_ins.remove_speffect(id);
            }
        }
    }
    if let Some(id) = new.er_speffect {
        player.apply_speffect(id, true);
        log!("profiles: {} active (SpEffect {id})", new.id);
    }
}
