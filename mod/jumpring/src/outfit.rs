//! hooks.outfit: cosmetic fighter looks (sheet: fighters.outfit). While a fighter is active, the model
//! fields of the armor rows the player is wearing are overwritten in memory with another armor's
//! model, so the Tarnished renders in that armor. Inventory, equipment slots, stats and the save are
//! never touched; the original fields are restored when the fighter changes.

use std::collections::HashMap;

use eldenring::cs::{EquipParamProtector, PlayerGameData, SoloParamRepository};
use eldenring::param::EQUIP_PARAM_PROTECTOR_ST;
use fromsoftware_shared::FromStatic;

use crate::log;

/// ChrAsm slots of head, chest, hands, legs.
const ARMOR_SLOTS: [usize; 4] = [12, 13, 14, 15];

/// 1.3.0: the fighter wears a whole-body skin (look.rs sets it before each apply)
pub static HIDE_BODY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Copy)]
pub(crate) struct Model {
    pub(crate) id: u16,
    category: u8,
    gender: u8,
    flags: [u8; 96],
}

pub(crate) fn read(r: &EQUIP_PARAM_PROTECTOR_ST) -> Model {
    Model {
        id: r.equip_model_id(),
        category: r.equip_model_category(),
        gender: r.equip_model_gender(),
        flags: [
        r.invisible_flag_sex_ver00(),
        r.invisible_flag_sex_ver01(),
        r.invisible_flag_sex_ver02(),
        r.invisible_flag_sex_ver03(),
        r.invisible_flag_sex_ver04(),
        r.invisible_flag_sex_ver05(),
        r.invisible_flag_sex_ver06(),
        r.invisible_flag_sex_ver07(),
        r.invisible_flag_sex_ver08(),
        r.invisible_flag_sex_ver09(),
        r.invisible_flag_sex_ver10(),
        r.invisible_flag_sex_ver11(),
        r.invisible_flag_sex_ver12(),
        r.invisible_flag_sex_ver13(),
        r.invisible_flag_sex_ver14(),
        r.invisible_flag_sex_ver15(),
        r.invisible_flag_sex_ver16(),
        r.invisible_flag_sex_ver17(),
        r.invisible_flag_sex_ver18(),
        r.invisible_flag_sex_ver19(),
        r.invisible_flag_sex_ver20(),
        r.invisible_flag_sex_ver21(),
        r.invisible_flag_sex_ver22(),
        r.invisible_flag_sex_ver23(),
        r.invisible_flag_sex_ver24(),
        r.invisible_flag_sex_ver25(),
        r.invisible_flag_sex_ver26(),
        r.invisible_flag_sex_ver27(),
        r.invisible_flag_sex_ver28(),
        r.invisible_flag_sex_ver29(),
        r.invisible_flag_sex_ver30(),
        r.invisible_flag_sex_ver31(),
        r.invisible_flag_sex_ver32(),
        r.invisible_flag_sex_ver33(),
        r.invisible_flag_sex_ver34(),
        r.invisible_flag_sex_ver35(),
        r.invisible_flag_sex_ver36(),
        r.invisible_flag_sex_ver37(),
        r.invisible_flag_sex_ver38(),
        r.invisible_flag_sex_ver39(),
        r.invisible_flag_sex_ver40(),
        r.invisible_flag_sex_ver41(),
        r.invisible_flag_sex_ver42(),
        r.invisible_flag_sex_ver43(),
        r.invisible_flag_sex_ver44(),
        r.invisible_flag_sex_ver45(),
        r.invisible_flag_sex_ver46(),
        r.invisible_flag_sex_ver47(),
        r.invisible_flag_sex_ver48(),
        r.invisible_flag_sex_ver49(),
        r.invisible_flag_sex_ver50(),
        r.invisible_flag_sex_ver51(),
        r.invisible_flag_sex_ver52(),
        r.invisible_flag_sex_ver53(),
        r.invisible_flag_sex_ver54(),
        r.invisible_flag_sex_ver55(),
        r.invisible_flag_sex_ver56(),
        r.invisible_flag_sex_ver57(),
        r.invisible_flag_sex_ver58(),
        r.invisible_flag_sex_ver59(),
        r.invisible_flag_sex_ver60(),
        r.invisible_flag_sex_ver61(),
        r.invisible_flag_sex_ver62(),
        r.invisible_flag_sex_ver63(),
        r.invisible_flag_sex_ver64(),
        r.invisible_flag_sex_ver65(),
        r.invisible_flag_sex_ver66(),
        r.invisible_flag_sex_ver67(),
        r.invisible_flag_sex_ver68(),
        r.invisible_flag_sex_ver69(),
        r.invisible_flag_sex_ver70(),
        r.invisible_flag_sex_ver71(),
        r.invisible_flag_sex_ver72(),
        r.invisible_flag_sex_ver73(),
        r.invisible_flag_sex_ver74(),
        r.invisible_flag_sex_ver75(),
        r.invisible_flag_sex_ver76(),
        r.invisible_flag_sex_ver77(),
        r.invisible_flag_sex_ver78(),
        r.invisible_flag_sex_ver79(),
        r.invisible_flag_sex_ver80(),
        r.invisible_flag_sex_ver81(),
        r.invisible_flag_sex_ver82(),
        r.invisible_flag_sex_ver83(),
        r.invisible_flag_sex_ver84(),
        r.invisible_flag_sex_ver85(),
        r.invisible_flag_sex_ver86(),
        r.invisible_flag_sex_ver87(),
        r.invisible_flag_sex_ver88(),
        r.invisible_flag_sex_ver89(),
        r.invisible_flag_sex_ver90(),
        r.invisible_flag_sex_ver91(),
        r.invisible_flag_sex_ver92(),
        r.invisible_flag_sex_ver93(),
        r.invisible_flag_sex_ver94(),
        r.invisible_flag_sex_ver95(),
        ],
    }
}

pub(crate) fn write(r: &mut EQUIP_PARAM_PROTECTOR_ST, m: &Model) {
    r.set_equip_model_id(m.id);
    r.set_equip_model_category(m.category);
    r.set_equip_model_gender(m.gender);
    r.set_invisible_flag_sex_ver00(m.flags[0]);
    r.set_invisible_flag_sex_ver01(m.flags[1]);
    r.set_invisible_flag_sex_ver02(m.flags[2]);
    r.set_invisible_flag_sex_ver03(m.flags[3]);
    r.set_invisible_flag_sex_ver04(m.flags[4]);
    r.set_invisible_flag_sex_ver05(m.flags[5]);
    r.set_invisible_flag_sex_ver06(m.flags[6]);
    r.set_invisible_flag_sex_ver07(m.flags[7]);
    r.set_invisible_flag_sex_ver08(m.flags[8]);
    r.set_invisible_flag_sex_ver09(m.flags[9]);
    r.set_invisible_flag_sex_ver10(m.flags[10]);
    r.set_invisible_flag_sex_ver11(m.flags[11]);
    r.set_invisible_flag_sex_ver12(m.flags[12]);
    r.set_invisible_flag_sex_ver13(m.flags[13]);
    r.set_invisible_flag_sex_ver14(m.flags[14]);
    r.set_invisible_flag_sex_ver15(m.flags[15]);
    r.set_invisible_flag_sex_ver16(m.flags[16]);
    r.set_invisible_flag_sex_ver17(m.flags[17]);
    r.set_invisible_flag_sex_ver18(m.flags[18]);
    r.set_invisible_flag_sex_ver19(m.flags[19]);
    r.set_invisible_flag_sex_ver20(m.flags[20]);
    r.set_invisible_flag_sex_ver21(m.flags[21]);
    r.set_invisible_flag_sex_ver22(m.flags[22]);
    r.set_invisible_flag_sex_ver23(m.flags[23]);
    r.set_invisible_flag_sex_ver24(m.flags[24]);
    r.set_invisible_flag_sex_ver25(m.flags[25]);
    r.set_invisible_flag_sex_ver26(m.flags[26]);
    r.set_invisible_flag_sex_ver27(m.flags[27]);
    r.set_invisible_flag_sex_ver28(m.flags[28]);
    r.set_invisible_flag_sex_ver29(m.flags[29]);
    r.set_invisible_flag_sex_ver30(m.flags[30]);
    r.set_invisible_flag_sex_ver31(m.flags[31]);
    r.set_invisible_flag_sex_ver32(m.flags[32]);
    r.set_invisible_flag_sex_ver33(m.flags[33]);
    r.set_invisible_flag_sex_ver34(m.flags[34]);
    r.set_invisible_flag_sex_ver35(m.flags[35]);
    r.set_invisible_flag_sex_ver36(m.flags[36]);
    r.set_invisible_flag_sex_ver37(m.flags[37]);
    r.set_invisible_flag_sex_ver38(m.flags[38]);
    r.set_invisible_flag_sex_ver39(m.flags[39]);
    r.set_invisible_flag_sex_ver40(m.flags[40]);
    r.set_invisible_flag_sex_ver41(m.flags[41]);
    r.set_invisible_flag_sex_ver42(m.flags[42]);
    r.set_invisible_flag_sex_ver43(m.flags[43]);
    r.set_invisible_flag_sex_ver44(m.flags[44]);
    r.set_invisible_flag_sex_ver45(m.flags[45]);
    r.set_invisible_flag_sex_ver46(m.flags[46]);
    r.set_invisible_flag_sex_ver47(m.flags[47]);
    r.set_invisible_flag_sex_ver48(m.flags[48]);
    r.set_invisible_flag_sex_ver49(m.flags[49]);
    r.set_invisible_flag_sex_ver50(m.flags[50]);
    r.set_invisible_flag_sex_ver51(m.flags[51]);
    r.set_invisible_flag_sex_ver52(m.flags[52]);
    r.set_invisible_flag_sex_ver53(m.flags[53]);
    r.set_invisible_flag_sex_ver54(m.flags[54]);
    r.set_invisible_flag_sex_ver55(m.flags[55]);
    r.set_invisible_flag_sex_ver56(m.flags[56]);
    r.set_invisible_flag_sex_ver57(m.flags[57]);
    r.set_invisible_flag_sex_ver58(m.flags[58]);
    r.set_invisible_flag_sex_ver59(m.flags[59]);
    r.set_invisible_flag_sex_ver60(m.flags[60]);
    r.set_invisible_flag_sex_ver61(m.flags[61]);
    r.set_invisible_flag_sex_ver62(m.flags[62]);
    r.set_invisible_flag_sex_ver63(m.flags[63]);
    r.set_invisible_flag_sex_ver64(m.flags[64]);
    r.set_invisible_flag_sex_ver65(m.flags[65]);
    r.set_invisible_flag_sex_ver66(m.flags[66]);
    r.set_invisible_flag_sex_ver67(m.flags[67]);
    r.set_invisible_flag_sex_ver68(m.flags[68]);
    r.set_invisible_flag_sex_ver69(m.flags[69]);
    r.set_invisible_flag_sex_ver70(m.flags[70]);
    r.set_invisible_flag_sex_ver71(m.flags[71]);
    r.set_invisible_flag_sex_ver72(m.flags[72]);
    r.set_invisible_flag_sex_ver73(m.flags[73]);
    r.set_invisible_flag_sex_ver74(m.flags[74]);
    r.set_invisible_flag_sex_ver75(m.flags[75]);
    r.set_invisible_flag_sex_ver76(m.flags[76]);
    r.set_invisible_flag_sex_ver77(m.flags[77]);
    r.set_invisible_flag_sex_ver78(m.flags[78]);
    r.set_invisible_flag_sex_ver79(m.flags[79]);
    r.set_invisible_flag_sex_ver80(m.flags[80]);
    r.set_invisible_flag_sex_ver81(m.flags[81]);
    r.set_invisible_flag_sex_ver82(m.flags[82]);
    r.set_invisible_flag_sex_ver83(m.flags[83]);
    r.set_invisible_flag_sex_ver84(m.flags[84]);
    r.set_invisible_flag_sex_ver85(m.flags[85]);
    r.set_invisible_flag_sex_ver86(m.flags[86]);
    r.set_invisible_flag_sex_ver87(m.flags[87]);
    r.set_invisible_flag_sex_ver88(m.flags[88]);
    r.set_invisible_flag_sex_ver89(m.flags[89]);
    r.set_invisible_flag_sex_ver90(m.flags[90]);
    r.set_invisible_flag_sex_ver91(m.flags[91]);
    r.set_invisible_flag_sex_ver92(m.flags[92]);
    r.set_invisible_flag_sex_ver93(m.flags[93]);
    r.set_invisible_flag_sex_ver94(m.flags[94]);
    r.set_invisible_flag_sex_ver95(m.flags[95]);
}

#[derive(Default)]
pub struct Outfits {
    /// original model of each armor row we overwrote
    saved: HashMap<u32, Model>,
    /// worn row ids at the time of the last apply, and the outfit applied
    applied: Option<([i32; 4], [i32; 4], [Option<u16>; 4])>,
    /// redraw in progress: frames left with the slot ids flipped, and the real ids to put back
    flip: Option<(u8, [i32; 4])>,
    /// 1.2.0: the flipped-to rows' own status resistances, put back when the flip ends
    flip_resist: Vec<(u32, [u16; 7])>,
    /// 1.3.0: the flipped-to rows' own models, put back when the flip ends (the user saw Gon in the template's knight armour
    /// for a frame each time his rod body came and went: the flipped-to row showed its own model while the slot pointed at it)
    flip_model: Vec<(u32, u16)>,
}

/// Status resistances (poison, rot, bleed, death, frost, sleep, madness): a change of these makes the game
/// show its build-up meters (user, 1.2.0: they popped up every time Asta's glider came and went)
fn resist(r: &eldenring::param::EQUIP_PARAM_PROTECTOR_ST) -> [u16; 7] {
    [r.resist_poison(), r.resist_disease(), r.resist_blood(), r.resist_curse(), r.resist_freeze(), r.resist_sleep(), r.resist_madness()]
}

fn set_resist(r: &mut eldenring::param::EQUIP_PARAM_PROTECTOR_ST, v: [u16; 7]) {
    r.set_resist_poison(v[0]);
    r.set_resist_disease(v[1]);
    r.set_resist_blood(v[2]);
    r.set_resist_curse(v[3]);
    r.set_resist_freeze(v[4]);
    r.set_resist_sleep(v[5]);
    r.set_resist_madness(v[6]);
}

impl Outfits {
    /// The armor ids the player is wearing (head, chest, hands, legs).
    pub fn worn(pgd: &PlayerGameData) -> [i32; 4] {
        ARMOR_SLOTS.map(|s| pgd.equipment.chr_asm.equipment_param_ids[s])
    }

    /// Put every overwritten row back.
    pub fn restore(&mut self) {
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        for (id, m) in self.saved.drain() {
            if let Some(r) = repo.get_mut::<EquipParamProtector>(id) {
                write(r, &m);
            }
        }
        self.applied = None;
    }

    /// Dress the worn rows in `outfit` (armor ids; <= 0 keeps that piece; None = own look).
    /// `live`: also make the renderer reload the parts now (flip the slot ids for a few frames);
    /// otherwise the look shows from the next model rebuild (load, warp). Call every frame:
    /// it re-applies when the worn armor changes. Returns true when rows were (re)written.
    pub fn apply(&mut self, pgd: &mut PlayerGameData, outfit: Option<[i32; 4]>, models: [Option<u16>; 4], live: bool) -> bool {
        if let Some((frames, real)) = self.flip {
            if frames == 0 {
                for (i, s) in ARMOR_SLOTS.iter().enumerate() {
                    pgd.equipment.chr_asm.equipment_param_ids[*s] = real[i];
                }
                self.flip = None;
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    for (id, v) in self.flip_resist.drain(..) {
                        if let Some(r) = repo.get_mut::<EquipParamProtector>(id) {
                            set_resist(r, v);
                        }
                    }
                    for (id, m) in self.flip_model.drain(..) {
                        if let Some(r) = repo.get_mut::<EquipParamProtector>(id) {
                            r.set_equip_model_id(m);
                        }
                    }
                }
            } else {
                self.flip = Some((frames - 1, real));
            }
            return false;
        }
        let worn = Self::worn(pgd);
        let Some(outfit) = outfit else {
            if self.applied.is_some() {
                self.restore();
                return true;
            }
            return false;
        };
        if self.applied == Some((worn, outfit, models)) {
            return false;
        }
        // The player changed armor themselves: their new pieces must be redrawn live.
        let live = live || matches!(self.applied, Some((w, o, _)) if o == outfit && w != worn);
        self.restore();
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
        for i in 0..4 {
            let (w, t) = (worn[i], outfit[i]);
            if w <= 0 || t <= 0 {
                continue;
            }
            let Some(mut target) = repo.get::<EquipParamProtector>(t as u32).map(read) else {
                log!("outfit: armor {t} not found");
                continue;
            };
            let Some(row) = repo.get_mut::<EquipParamProtector>(w as u32) else { continue };
            self.saved.entry(w as u32).or_insert_with(|| read(row));
            // hooks.recolor: the fighter's recoloured copy of this piece
            if let Some(m) = models[i] {
                target.id = m;

            }
            write(row, &target);
        }
        self.applied = Some((worn, outfit, models));
        log!("outfit: worn {worn:?} now look like {outfit:?} (live {live})");
        if live {
            // The renderer reloads armor parts when a slot's param id changes: flip to the target ids
            // for a few frames, then back to the real ids (which now carry the target's model).
            // (the flipped-to rows wear the worn rows' resistances meanwhile: no build-up meters)
            for (i, s) in ARMOR_SLOTS.iter().enumerate() {
                if worn[i] > 0 && outfit[i] > 0 && outfit[i] != worn[i] {
                    let theirs = repo.get::<EquipParamProtector>(worn[i] as u32).map(resist);
                    if let (Some(v), Some(r)) = (theirs, repo.get_mut::<EquipParamProtector>(outfit[i] as u32)) {
                        if !self.flip_resist.iter().any(|(id, _)| *id == outfit[i] as u32) {
                            self.flip_resist.push((outfit[i] as u32, resist(r)));
                        }
                        set_resist(r, v);
                    }
                    // and the fighter's own model meanwhile, never the template's
                    if let (Some(m), Some(r)) = (models[i], repo.get_mut::<EquipParamProtector>(outfit[i] as u32)) {
                        if !self.flip_model.iter().any(|(id, _)| *id == outfit[i] as u32) {
                            self.flip_model.push((outfit[i] as u32, r.equip_model_id()));
                        }
                        r.set_equip_model_id(m);
                    }
                    pgd.equipment.chr_asm.equipment_param_ids[*s] = outfit[i];
                }
            }
            self.flip = Some((3, worn));
        }
        true
    }

    /// After a load the player is rebuilt: keep the rows, re-check on the next apply.
    pub fn forget(&mut self) {
        self.flip = None;
        if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
            for (id, v) in self.flip_resist.drain(..) {
                if let Some(r) = repo.get_mut::<EquipParamProtector>(id) {
                    set_resist(r, v);
                }
            }
            for (id, m) in self.flip_model.drain(..) {
                if let Some(r) = repo.get_mut::<EquipParamProtector>(id) {
                    r.set_equip_model_id(m);
                }
            }
        }
    }
}
