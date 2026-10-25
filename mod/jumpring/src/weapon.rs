//! Bare hands (0.10.0, sheet: fighters.bare_hands): JUMP FORCE fighters fight with their fists. Every
//! weapon the player has equipped, in either hand and sheathed in the other slots, takes Elden Ring's
//! empty-hand row (EquipParamWeapon 110000) in memory while a fighter is active: no model, the
//! empty-hand moveset. As with the outfits (outfit.rs), inventory, equipment slots and the save are
//! never touched, and every row is put back when the player switches to Your Tarnished.
//!
//! The bare hands' Ash of War (LT / skill) is Endure, the braced guard Goku's fist wraps had in 0.9.0:
//! it is the parry's stance (parry.rs). For a moment during a move, the hands take another Ash of War
//! so the behaviour script can play its animation (anim.rs Cue::Arts).
//!
//! Asta (fighters.move_weapon) always holds his Demon-Slayer Sword (0.11.0): the right hand's rows play
//! a sword's moveset (his attacks are sword swings) with no model of their own, and the blade is part of
//! his model, bound to his right hand (skins/asta_bd_m_1850.bnd, work/rigport/asta_sword2.py). 0.13.0:
//! the Longsword's one-handed straight sword swings instead of the Greatsword's: he swings the huge
//! blade as if it weighed nothing, as in the show.

use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, AtomicU64, Ordering};
use std::time::Instant;

use eldenring::cs::{EquipParamWeapon, PlayerGameData, ReinforceParamWeapon, SoloParamRepository};
use eldenring::param::EQUIP_PARAM_WEAPON_ST;
use fromsoftware_shared::FromStatic;

use crate::log;

/// 1.3.0: the fighter's weapon (fighters.move_weapon) shows its own Elden Ring model: the new sword fighters
/// (Ichigo, Sasuke, Zoro, Yami) until their own models carry the blade, as Asta's does (look.rs sets it).
pub static SHOW_OWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// 1.3.0: the sword model the fighter's weapon shows instead of the row's own (0: the row's): the fighters' real swords,
/// shipped as weapon model files of their own (mod/parts/wp_a_<id>.partsbnd.dcx, tools_scripts/package_weapons.py)
pub static OWN_MODEL: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

/// 1.3.0 (the user: "zoro doesn't use two swords while fighting, he only uses one while one is sheathed"): a fighter
/// whose sword is in both hands (Zoro's Nitoryu / Santoryu: a katana in each hand). With the same weapon in both hands
/// LB plays Elden Ring's paired attacks (both blades), RB / RT stay the katana's moveset.
pub static DUAL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// the left hand's sword when it is in both: Zoro's second katana (from Zoro v2's model mod, Nexus 2107: wp_a_9604)
const LEFT_MODEL: u16 = 9604;

/// The fighters' own swords: Ichigo's Tensa Zangetsu (AceMx242's Bleach mod, Nexus 6662), Zoro's katana (from Zoro v2's
/// model mod), Yami's black katana (made for Elden Force).
pub fn own_model(fighter: &str) -> Option<u16> {
    match fighter {
        "ichigo" => Some(9601),
        "zoro" => Some(9602),
        // (1.3.0, the user: Yami's sword "is too big. In the show he has a black katana with a white rippling stripe down the
        // blade side": made for Elden Force, work/rigport/weapon_frame.py; he had the Nagakiba's long blade)
        "yami" => Some(9605),
        // (1.3.1, the user: his Kusanagi "sits left of his hand", and Chidori should run through the blade: drawn in his skin's arms
        // piece it hung off his hand's bone; as a weapon model of its own (the Uchigatana's dummies kept) it sits in his grip and
        // carries a weapon's effects; work/rigport/weapon_frame.py)
        "sasuke" => Some(9606),
        _ => None,
    }
}

/// 1.3.0: whose weapon shows a model: every sword fighter's but Asta's (his skin holds his blade; 1.3.1: Sasuke's Kusanagi is a
/// weapon model now, own_model). (Their skins
/// carry no sword: with the stand-in hidden under a skin, Ichigo, Sasuke, Zoro and Yami swung nothing.)
pub fn show_for(f: &crate::sheets::Fighter) {
    SHOW_OWN.store(f.move_weapon.is_some() && f.id != "asta", Ordering::Relaxed);
    OWN_MODEL.store(own_model(f.id).unwrap_or(0), Ordering::Relaxed);
    DUAL.store(f.id == "zoro", Ordering::Relaxed);
}

/// Lab (script "swordrow <row>"): the row Asta's sword plays instead of his own (0: his own).
pub static LAB_SWORD_ROW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Empty hand: one row shared by every empty slot.
pub const UNARMED: u32 = 110000;
/// The bare hands' own Ash of War: Endure (SwordArtsParam 700).
pub const ENDURE: i32 = 700;
const ROW_SIZE: usize = std::mem::size_of::<EQUIP_PARAM_WEAPON_ST>();
/// The empty hand's model (nothing in the hand).
const NO_MODEL: u16 = 1;
/// Weapon slots in ChrAsm (left 1, right 1, left 2, right 2, left 3, right 3).
const SLOTS: usize = 6;

/// 0.15.0: the fighters' own punches, kicks and (Asta) sword swings hit with an attack rating that
/// follows the player's level only, like their abilities (no attribute scaling: fighters only put points
/// in Vigor, Mind and Endurance). About a third of a skill's damage per swing at any level: weaker than
/// the moves, but real hits. (Empty hands were 20 physical plus Strength/Dexterity scaling.)
pub fn fist_rating(level: u32) -> f32 {
    // 0.16.0: 1.5x 0.15.0's (0.15: 0.5; tripled, then the user's test: too strong, halved)
    0.75 * (100.0 + level as f32 * 5.0)
}

/// The rating written into a row at upgrade `plus` of its reinforce path, so the hit is the same
/// whatever the slot's weapon was upgraded to.
fn level_attack(repo: &SoloParamRepository, r: &mut EQUIP_PARAM_WEAPON_ST, plus: i32, level: u32) {
    let rate = repo.get::<ReinforceParamWeapon>((r.reinforce_type_id() as i32 + plus.clamp(0, 25)) as u32).map(|x| x.physics_atk_rate()).filter(|x| *x > 0.1).unwrap_or(1.0);
    r.set_attack_base_physics((fist_rating(level) / rate).round().clamp(1.0, 3000.0) as u16);
    r.set_attack_base_magic(0);
    r.set_attack_base_fire(0);
    r.set_attack_base_thunder(0);
    r.set_attack_base_dark(0);
    r.set_correct_strength(0.0);
    r.set_correct_agility(0.0);
    r.set_correct_magic(0.0);
    r.set_correct_faith(0.0);
    r.set_correct_luck(0.0);
}

fn bytes(r: &EQUIP_PARAM_WEAPON_ST) -> Box<[u8; ROW_SIZE]> {
    let mut b = Box::new([0u8; ROW_SIZE]);
    unsafe { std::ptr::copy_nonoverlapping(r as *const _ as *const u8, b.as_mut_ptr(), ROW_SIZE) };
    b
}

fn put(r: &mut EQUIP_PARAM_WEAPON_ST, b: &[u8; ROW_SIZE]) {
    unsafe { std::ptr::copy_nonoverlapping(b.as_ptr(), r as *mut _ as *mut u8, ROW_SIZE) };
}

/// 1.2.0: the motion category (EquipParamWeapon wepmotionCategory) of the weapon in the right hand: which
/// moveset it plays (Carian Combo Warriors' player script knows those up to 52; Shadow of the Erdtree's own
/// weapon types are 53 and up).
pub fn right_category(pgd: &PlayerGameData) -> Option<u8> {
    let asm = &pgd.equipment.chr_asm;
    // (the ChrAsmSlot index itself: 1, 3 or 5)
    let slot = asm.equipment.active_right_weapon_slot() as usize;
    let id = *asm.equipment_param_ids.get(slot)?;
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    let row = if id <= 0 { UNARMED } else { row_of(id) };
    repo.get::<EquipParamWeapon>(row).map(|w| w.wepmotion_category())
}

/// Weapon row `row`'s model id.
fn repo_models(row: u32) -> Option<u16> {
    let repo = unsafe { SoloParamRepository::instance() }.ok()?;
    repo.get::<EquipParamWeapon>(row).map(|w| w.equip_model_id())
}

/// The param row of an equipped weapon id (ids carry the upgrade level in their last two digits).
fn row_of(id: i32) -> u32 {
    (id - id.rem_euclid(100)) as u32
}

/// Shadow of the Erdtree is installed (its archives hold the DLC's Ash of War animations: Storm Kick,
/// Dryleaf Whirlwind, Palm Blast, Blinkbolt, Raging Beast). Without it moves use base-game ones.
pub fn dlc() -> bool {
    static DLC: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DLC.get_or_init(|| {
        let d = crate::ermario::archive::game_dir();
        let yes = d.join("DLC.bhd").exists() && d.join("DLC.bdt").exists();
        log!("weapon: Shadow of the Erdtree {}", if yes { "installed" } else { "not installed: base-game Ashes of War" });
        yes
    })
}

/// A move's Ash of War for the bare hands (SwordArtsParam row) and until when (ms since START).
static ARTS: AtomicI32 = AtomicI32::new(ENDURE);
static ARTS_UNTIL: AtomicU64 = AtomicU64::new(0);
/// Asta's sword in hand until (ms since START).
static DRAWN_UNTIL: AtomicU64 = AtomicU64::new(0);
static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn ms() -> u64 {
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The rows the bare hands are on right now (written by apply), for set_arts.
static HAND_ROWS: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());

fn write_arts(arts: i32) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let rows = HAND_ROWS.lock().unwrap().clone();
    for row in rows.iter().copied().chain([UNARMED]) {
        if let Some(r) = repo.get_mut::<EquipParamWeapon>(row) {
            r.set_sword_arts_param_id(arts);
        }
    }
}

/// For `secs`, the bare hands use Ash of War `arts` (its animation is about to be played).
pub fn set_arts(arts: i32, secs: f32) {
    ARTS.store(arts, Ordering::Relaxed);
    ARTS_UNTIL.store(ms() + (secs * 1000.0) as u64, Ordering::Relaxed);
    write_arts(arts);
}

/// 1.2.0: for `secs` the hands are dressed as weapon `row` (no model, weightless), so an ability can play
/// one of that weapon's attacks: Carian Combo Warriors' moves come with the weapon's moveset (its
/// animations, and its attack rows' effects). scripts.rs Step::Weapon.
static MOVE_ROW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static MOVE_UNTIL: AtomicU64 = AtomicU64::new(0);
/// the row the hands are dressed as now (0: their own)
static MOVE_APPLIED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// the model the borrowed weapon shows (another weapon row's; 0: none, the hands stay bare)
static MOVE_MODEL: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_move(row: u32, secs: f32) {
    set_move_model(row, 0, secs);
}

/// As set_move, with weapon `model_row`'s model in the hand (e.g. Asta riding a great black blade).
pub fn set_move_model(row: u32, model_row: u32, secs: f32) {
    MOVE_MODEL.store(model_row, Ordering::Relaxed);
    MOVE_ROW.store(row, Ordering::Relaxed);
    MOVE_UNTIL.store(ms() + (secs * 1000.0) as u64, Ordering::Relaxed);
}

/// 1.2.0: the borrowed weapon draws no blade trail (Asta's glider: flying, the Dismounter's trail streaked
/// a long blue line behind him).
pub static MOVE_TRACELESS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The hands are dressed as `row` now.
pub fn move_applied(row: u32) -> bool {
    MOVE_APPLIED.load(Ordering::Relaxed) == row
}

/// Asta draws his sword for `secs` (an ability plays).
pub fn draw(secs: f32) {
    DRAWN_UNTIL.store(ms() + (secs * 1000.0) as u64, Ordering::Relaxed);
}

#[derive(Default)]
pub struct Weapons {
    /// original bytes of every row we dressed
    saved: HashMap<u32, Box<[u8; ROW_SIZE]>>,
    /// what is applied: (bare, the slots' ids, the sword, the level) when it was last written
    applied: Option<(bool, [i32; SLOTS], Option<u32>, u32)>,
    /// and whether the sword was in both hands then
    applied_dual: bool,
    /// the hand's row dressed as the drawn sword
    drawn: Option<u32>,
    /// redraw: frames left with the slot showing another id, the slot and its real id
    flip: Option<(u8, usize, i32)>,
    /// a borrowed weapon's model is in the hand (Step::WeaponShown): drawn again when it goes
    model_shown: bool,
    arts_now: i32,
    /// 1.3.1: the right-hand slot the sword fighter's sword holds (slot, the slot's own id)
    occupied: Option<(usize, i32)>,
    /// the fighter's own sword last frame (its row is what an occupied slot holds)
    last_sword: Option<u32>,
}

impl Weapons {
    pub fn restore(&mut self) {
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        for (id, b) in self.saved.drain() {
            if let Some(r) = repo.get_mut::<EquipParamWeapon>(id) {
                put(r, &b);
            }
        }
        HAND_ROWS.lock().unwrap().clear();
        self.applied = None;
        self.drawn = None;
        self.arts_now = 0;
    }

    /// Call every frame (and when the look is written before a load). `bare`: the fighter fights
    /// barehanded; `sword`: a weapon row drawn for abilities.
    pub fn apply(&mut self, pgd: &mut PlayerGameData, bare: bool, sword: Option<u32>, _live: bool) {
        // lab: another weapon row in the sword's place (script "swordrow", to audition its moveset)
        let lab_row = LAB_SWORD_ROW.load(Ordering::Relaxed);
        let moving = ms() < MOVE_UNTIL.load(Ordering::Relaxed);
        // (1.3.1 round 3, the user: Todoroki's Dancing Flame was "the kicking flurry with electricity on the ground instead of the
        // fiery spin": his held right-hand slot held a weapon of the user's (the lab's was empty), and a borrowed weapon only
        // rewrote that weapon's row, whose moveset the game had already taken; the fists' move played) a move's borrowed weapon
        // takes the held slot itself for the move, as a sword fighter's sword does, and gives it back after
        let base_sword = if moving && bare { Some(MOVE_ROW.load(Ordering::Relaxed)) } else { sword };
        let base_sword_last = self.last_sword;
        let sword = if lab_row > 0 && sword.is_some() { Some(lab_row) } else { sword };
        let sword = if moving && bare { Some(MOVE_ROW.load(Ordering::Relaxed)) } else { sword };
        let asm = &mut pgd.equipment.chr_asm;
        // (the ChrAsmSlot index itself: 1, 3 or 5)
        let right = asm.equipment.active_right_weapon_slot() as usize;
        let now = ms();

        // a weapon is redrawn when its slot's id changes: show another id for a few frames, then the
        // real one again (whose row now carries the new look)
        if let Some((frames, slot, real)) = self.flip {
            if frames == 0 {
                asm.equipment_param_ids[slot] = real;
                self.flip = None;
            } else {
                self.flip = Some((frames - 1, slot, real));
            }
            return;
        }
        // (1.3.1, the user's playtest: Ichigo, Yami, Asta and Sasuke held no sword and fought with fists. Their character held
        // its third right-hand slot, empty, while the sword went on the first: the game's active slot is the ChrAsmSlot index
        // itself (1, 3, 5), read here as 0..2 (2 * slot + 1).) A sword fighter's sword takes the active right-hand slot itself:
        // its id becomes the sword's row (the inventory's own record, the gaitem handle, is untouched; a menu change rewrites
        // the id from it), the slot's own id back when the sword goes or another slot is held. Zoro's two blades keep their way.
        let r_idx = right;
        match base_sword.filter(|_| bare && (moving || !DUAL.load(Ordering::Relaxed))) {
            Some(row) if (1..SLOTS).step_by(2).any(|x| x == r_idx) => {
                let row = row as i32;
                if let Some((s0, orig)) = self.occupied {
                    if s0 != r_idx {
                        if asm.equipment_param_ids[s0] == row {
                            asm.equipment_param_ids[s0] = orig;
                        }
                        self.occupied = None;
                    }
                }
                if asm.equipment_param_ids[r_idx] != row {
                    let orig = asm.equipment_param_ids[r_idx];
                    if self.occupied.is_none() {
                        self.occupied = Some((r_idx, orig));
                    }
                    asm.equipment_param_ids[r_idx] = row;
                    log!("weapon: the sword ({row}) in the held right-hand slot {r_idx} (its own id {} kept)", self.occupied.map_or(orig, |o| o.1));
                }
            }
            _ => {
                if let Some((s0, orig)) = self.occupied.take() {
                    if base_sword_last.is_some_and(|r| asm.equipment_param_ids[s0] == r as i32) || asm.equipment_param_ids[s0] != orig {
                        asm.equipment_param_ids[s0] = orig;
                    }
                    log!("weapon: right-hand slot {s0} has its own weapon back ({orig})");
                }
            }
        }
        self.last_sword = base_sword.filter(|_| bare);
        let mut ids = [0i32; SLOTS];
        ids.copy_from_slice(&asm.equipment_param_ids[..SLOTS]);
        let _ = now;

        if !bare {
            if self.applied.is_some() {
                self.restore();
            }
            return;
        }
        let arts = if now < ARTS_UNTIL.load(Ordering::Relaxed) { ARTS.load(Ordering::Relaxed) } else { ENDURE };
        let level = pgd.level;
        // (a move's borrowed weapon stays in the right hand only)
        let dual = DUAL.load(Ordering::Relaxed) && !moving && sword.is_some();
        if self.applied == Some((bare, ids, sword, level)) && self.applied_dual == dual {
            // (already dressed so: e.g. Asta's own Longsword borrowed for one of its moves)
            MOVE_APPLIED.store(if moving { MOVE_ROW.load(Ordering::Relaxed) } else { 0 }, Ordering::Relaxed);
            if arts != self.arts_now {
                write_arts(arts);
                self.arts_now = arts;
            }
            return;
        }
        self.restore();
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        let Some(unarmed) = repo.get::<EquipParamWeapon>(UNARMED).map(bytes) else { return };
        // 0.11.0: Asta's sword (fighters.move_weapon) is always in his right hand: the right slots'
        // rows play its moveset with no model of their own (his model holds the blade)
        let held = sword.and_then(|row| repo.get::<EquipParamWeapon>(row)).map(|r| {
            let mut b = bytes(r);
            let r2 = unsafe { &mut *(b.as_mut_ptr() as *mut EQUIP_PARAM_WEAPON_ST) };
            let model = if moving { MOVE_MODEL.load(Ordering::Relaxed) } else { 0 };
            let shown = (model > 0).then(|| repo_models(model)).flatten();
            if shown.is_some() || !SHOW_OWN.load(Ordering::Relaxed) {
                r2.set_equip_model_id(shown.unwrap_or(NO_MODEL));
            } else if OWN_MODEL.load(Ordering::Relaxed) > 0 {
                r2.set_equip_model_id(OWN_MODEL.load(Ordering::Relaxed));
            }
            if moving && MOVE_TRACELESS.load(Ordering::Relaxed) {
                r2.set_trace_sfx_id0(-1);
                r2.set_trace_sfx_id1(-1);
                r2.set_trace_sfx_id2(-1);
                r2.set_trace_sfx_id3(-1);
                r2.set_trace_sfx_id4(-1);
                r2.set_trace_sfx_id5(-1);
                r2.set_trace_sfx_id6(-1);
                r2.set_trace_sfx_id7(-1);
            }
            // 0.12.0: weightless: the sword's weight on every right slot overloaded Asta (no
            // running, rolling or jumping)
            r2.set_weight(0.0);
            b
        });
        // (the sword in both hands: the left one shows the second blade)
        let held_left = held.as_ref().filter(|_| dual).map(|h| {
            let mut b = h.clone();
            let r2 = unsafe { &mut *(b.as_mut_ptr() as *mut EQUIP_PARAM_WEAPON_ST) };
            if SHOW_OWN.load(Ordering::Relaxed) {
                r2.set_equip_model_id(LEFT_MODEL);
            }
            b
        });
        // (1.3.1: the hand held, not every right slot: with the held slot empty and weapons in the others, a borrowed weapon
        // (Dancing Flame's twinblade) never reached the hand)
        let right_empty = ids[right] <= 0 || row_of(ids[right]) == UNARMED;
        let mut rows = Vec::new();
        let mut dress = |repo: &mut SoloParamRepository, row: u32, plus: i32, src: &[u8; ROW_SIZE], saved: &mut HashMap<u32, Box<[u8; ROW_SIZE]>>| {
            let repo_ro: &SoloParamRepository = unsafe { &*(repo as *const SoloParamRepository) };
            let Some(r) = repo.get_mut::<EquipParamWeapon>(row) else { return false };
            saved.entry(row).or_insert_with(|| bytes(r));
            let keep_reinforce = r.reinforce_type_id();
            let keep_icon = r.icon_id();
            put(r, src);
            // the HUD keeps showing the weapon's own icon (the empty hand has none)
            r.set_icon_id(keep_icon);
            // the weapon's own upgrade path, so its +N still exists; no requirements
            r.set_reinforce_type_id(keep_reinforce);
            r.set_proper_strength(0);
            r.set_proper_agility(0);
            r.set_proper_magic(0);
            r.set_proper_faith(0);
            r.set_proper_luck(0);
            level_attack(repo_ro, r, plus, level);
            true
        };
        for s in 0..SLOTS {
            if ids[s] <= 0 {
                continue;
            }
            let row = row_of(ids[s]);
            if rows.contains(&row) || row == UNARMED {
                continue;
            }
            let src = match &held {
                Some(h) if s % 2 == 1 => h.as_ref(),
                Some(_) if dual => held_left.as_ref().map(|b| b.as_ref()).unwrap_or(unarmed.as_ref()),
                _ => unarmed.as_ref(),
            };
            if dress(repo, row, ids[s].rem_euclid(100), src, &mut self.saved) {
                rows.push(row);
            }
        }
        // the empty hand itself (its original is kept for restore); with nothing in the right hand,
        // Asta's sword goes on the empty hand
        match (&held, right_empty || dual) {
            (Some(h), true) => {
                // (the sword in both hands with the right one held: the empty hand is the left one, its second blade)
                let src = match &held_left {
                    Some(l) if !right_empty => l.as_ref(),
                    _ => h.as_ref(),
                };
                if dress(repo, UNARMED, 0, src, &mut self.saved) {
                    rows.push(UNARMED);
                }
            }
            _ => {
                // the empty hand itself punches at the fighter's level too
                if dress(repo, UNARMED, 0, unarmed.as_ref(), &mut self.saved) {
                    rows.push(UNARMED);
                }
            }
        }
        *HAND_ROWS.lock().unwrap() = rows.clone();
        write_arts(arts);
        self.arts_now = arts;
        self.applied = Some((bare, ids, sword, level));
        self.applied_dual = dual;
        MOVE_APPLIED.store(if moving { MOVE_ROW.load(Ordering::Relaxed) } else { 0 }, Ordering::Relaxed);
        // the hand's model is only loaded when the weapon is drawn: a borrowed model (or its going) needs the
        // right slot drawn again: another id for a few frames, then its own (the flip, above)
        let shows = moving && MOVE_MODEL.load(Ordering::Relaxed) > 0;
        if shows || self.model_shown {
            let slot = right;
            let real = pgd.equipment.chr_asm.equipment_param_ids[slot];
            let other = if row_of(real) == UNARMED { 2_000_000 } else { UNARMED as i32 };
            pgd.equipment.chr_asm.equipment_param_ids[slot] = other;
            self.flip = Some((3, slot, real));
        }
        self.model_shown = shows;
        log!("weapon: bare hands on rows {rows:?}, {:.0} attack at level {level}", fist_rating(level));
    }

    /// Leaving the world: a slot being redrawn gets its real id back.
    pub fn forget(&mut self) {
        if let Some((_, slot, real)) = self.flip.take() {
            if let Ok(gdm) = unsafe { eldenring::cs::GameDataMan::instance_mut() } {
                gdm.main_player_game_data.equipment.chr_asm.equipment_param_ids[slot] = real;
            }
        }
        self.drawn = None;
        DRAWN_UNTIL.store(0, Ordering::Relaxed);
    }
}
