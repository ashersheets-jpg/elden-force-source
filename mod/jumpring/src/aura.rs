//! Fighter auras (sheet: fighters.aura_speffect): while charging an ability and while the Awakening
//! gauge is full, the fighter glows with the visual effect of an Elden Ring buff (Golden Vow,
//! Lightning Fortification...). Only the visuals are borrowed: two spare SpEffect rows start from the
//! neutral talisman template and get the source effect's vfx ids, no stats.

use eldenring::cs::{ChrInsExt, PlayerIns, SoloParamRepository, SpEffectParam};
use eldenring::param::SP_EFFECT_PARAM_ST;
use fromsoftware_shared::FromStatic;

use crate::log;

/// Spare "Area Scaling - (Unused)" rows: charge-up aura and Awakening aura.
pub const CHARGE: i32 = 7260;
pub const AWAKE: i32 = 7270;
/// 1.3.1: weapon coatings, our own SpEffect rows (tools_scripts/ccw/strip_fx.json row_new): Sasuke's Chidori running through
/// the Kusanagi (the game's lightning coating in his blue-white) and Amaterasu's black flames round it (Black Flame Blade's)
pub const CHIDORI_BLADE: i32 = 9990122;
pub const AMATERASU_BLADE: i32 = 9990123;
/// Yami's Dark Cloak round his katana (Black Flame Blade's coating in pure black)
pub const DARK_BLADE: i32 = 9990124;

const TEMPLATE: u32 = 310000;

/// Make `row` show `source`'s visuals for `seconds` (-1 = until removed).
pub fn prepare(row: i32, source: i32, seconds: f32) -> bool {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let Some(src) = repo.get::<SpEffectParam>(source as u32) else {
        log!("aura: SpEffect {source} not found");
        return false;
    };
    let vfx = [src.vfx_id(), src.vfx_id1(), src.vfx_id2(), src.vfx_id3(), src.vfx_id4(), src.vfx_id5(), src.vfx_id6(), src.vfx_id7()];
    let Some(template) = repo.get::<SpEffectParam>(TEMPLATE).map(|r| r as *const SP_EFFECT_PARAM_ST) else { return false };
    let Some(r) = repo.get_mut::<SpEffectParam>(row as u32) else { return false };
    unsafe { std::ptr::copy_nonoverlapping(template, r as *mut SP_EFFECT_PARAM_ST, 1) };
    r.set_max_hp_rate(1.0);
    r.set_effect_endurance(seconds);
    r.set_icon_id(-1);
    r.set_vfx_id(vfx[0]);
    r.set_vfx_id1(vfx[1]);
    r.set_vfx_id2(vfx[2]);
    r.set_vfx_id3(vfx[3]);
    r.set_vfx_id4(vfx[4]);
    r.set_vfx_id5(vfx[5]);
    r.set_vfx_id6(vfx[6]);
    r.set_vfx_id7(vfx[7]);
    true
}

pub fn on(player: &mut PlayerIns, row: i32) {
    player.apply_speffect(row, true);
}

pub fn off(player: &mut PlayerIns, row: i32) {
    player.chr_ins.remove_speffect(row);
}

/// 1.2.x (user: "a blue ball surrounded by spinning air inside their right palm" while Rasengan and Kamehameha
/// charge): our own SpEffect row (added by the param pack, tools_scripts/ccw/param_closure.py) with two
/// visuals on the right hand's dummy poly.
pub const PALM: i32 = 9990100;
const PALM_VFX: [i32; 2] = [9990100, 9990101];
/// The charge: the bubble spell's ball (a vortex of air inside it), recoloured a light chakra blue; Storm Blade's
/// wind (900440082) swirled around his whole body, too big
/// (tools_scripts/ccw/make_red_fx.sh), on the right hand's dummy poly 26 (the palm)
pub const PALM_CHARGE: [i32; 2] = [900523232, -1];
pub const PALM_DMY: i16 = 26;
/// 1.3.1 (the user: Killua's Godspeed needs "glowing white hair"): his blue-white crackle (Lightning Fortification's, recoloured:
/// make_red_fx.sh 900004730) on the head's dummy poly 220 (the game's head effects': Frenzyflame's)
pub const HEAD_CRACKLE: [i32; 2] = [900004730, -1];
pub const HEAD_DMY: i16 = 220;

/// Point the palm effect at `sfx` (two effects, -1 for none) on dummy poly `dmy` of the player.
pub fn palm_prepare(sfx: [i32; 2], dmy: i16) -> bool {
    use eldenring::cs::SpEffectVfxParam;
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    for (k, v) in PALM_VFX.iter().enumerate() {
        let Some(r) = repo.get_mut::<SpEffectVfxParam>(*v as u32) else {
            log!("aura: palm vfx row {v} missing");
            return false;
        };
        r.set_midst_sfx_id(sfx[k]);
        r.set_midst_dmy_id(dmy);
        r.set_init_sfx_id(-1);
        r.set_finish_sfx_id(-1);
    }
    let Some(template) = repo.get::<SpEffectParam>(TEMPLATE).map(|r| r as *const SP_EFFECT_PARAM_ST) else { return false };
    let Some(r) = repo.get_mut::<SpEffectParam>(PALM as u32) else {
        log!("aura: palm row {PALM} missing");
        return false;
    };
    unsafe { std::ptr::copy_nonoverlapping(template, r as *mut SP_EFFECT_PARAM_ST, 1) };
    r.set_max_hp_rate(1.0);
    r.set_effect_endurance(-1.0);
    r.set_icon_id(-1);
    r.set_vfx_id(PALM_VFX[0]);
    r.set_vfx_id1(if sfx[1] > 0 { PALM_VFX[1] } else { -1 });
    for (i, f) in [SP_EFFECT_PARAM_ST::set_vfx_id2, SP_EFFECT_PARAM_ST::set_vfx_id3, SP_EFFECT_PARAM_ST::set_vfx_id4, SP_EFFECT_PARAM_ST::set_vfx_id5,
                   SP_EFFECT_PARAM_ST::set_vfx_id6, SP_EFFECT_PARAM_ST::set_vfx_id7].iter().enumerate() {
        let _ = i;
        f(r, -1);
    }
    true
}
