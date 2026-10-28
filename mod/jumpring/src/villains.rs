//! 1.3.0 villain summons (user, 2026-10-16: "the 2 most notable villains of each of these animes along with high
//! octane move sets that match the villains they are supposed to represent, and they have to match the gameplay
//! we have already created ... For now, just create them with filler character models. I want you to make their
//! movesets as close to their moves in the show as possible").
//! - Made by the game's own character creator (`WorldChrMan::spawn_debug_character`) as human characters (c0000):
//!   the player's body, so they play every animation the fighters do (Elden Ring's Ashes of War, Carian Combo
//!   Warriors' movesets). Their stats and equipment are rows of their own (NpcParam / CharaInitParam 9990200..,
//!   copies of a hostile human enemy's, param pack), dressed here before each summon: filler looks until the
//!   user's models exist.
//! - Their basic attacks are their AI's (a hostile human enemy's); their specials are ours: a body animation, a
//!   tell (the move's name and a seal or lane where it lands, never less than TELL_MIN before it strikes), then
//!   Elden Ring bosses' and NPCs' own spells, owned by the villain, their damage set to a share of the player's
//!   health (one special takes at most MOVE_CAP of the bar).
//! - In a boss fight: below 60% of the boss's health the player's rival arrives (a villain from their fighter's
//!   anime), on Hard the other one too below 30%. Their health is VILLAIN_SHARE of the boss's (scaled) health.
//!   When the boss falls, they go with it.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eldenring::cs::{CSFeManImp, ChrIns, CharaInitParam, FieldInsHandle, NpcParam, PlayerIns, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::FromStatic;
use rand::RngExt;

use crate::ability::{self, V3};
use crate::log;
use crate::world_fx::{self, Shape, Sigil};

/// The AI every villain fights with: a hostile human enemy's (Limgrave, map m60_43_36: no dialogue).
const THINK: i32 = 533181000;
/// Villain health: this share of the boss's health as the player fights it (its own x the difficulty).
const VILLAIN_SHARE: f32 = 0.4;
/// One special's attacks together take at most this share of the player's health.
const MOVE_CAP: f32 = 0.3;
/// The team every villain fights for (the game's ordinary enemies).
const HOSTILE_TEAM: u8 = 6;
/// 1.3.0: a villain's stance (super armor: emptied, it staggers) and poise (toughness: what a hit must beat to flinch it)
const VILLAIN_STANCE: f32 = 300.0;
const VILLAIN_POISE: f32 = 600.0;
/// the special guard holds this long after a special's last step (its spells still flying, lingering)
const SPECIAL_LINGER: f32 = 1.5;
/// Shortest warning before a special strikes.
const TELL_MIN: f32 = 0.6;
/// 1.3.0, the user on review: no runic seals ("avoid needing to use these runes by instead creating animations
/// and new attacks that the bosses cast themselves"): the warning is the villain's own wind-up, held this long
const SEALS: bool = false;
/// Dante's Body Magic (the user: "life steal instead of passive healing"): he heals this share of the damage his
/// hits deal the player
const LIFE_STEAL: f32 = 0.5;
/// Below these shares of the boss's health the first villain (always) and the second (Hard) arrive.
const FIRST_AT: f32 = 0.6;
const SECOND_AT: f32 = 0.3;
/// A villain's specials: the first a few seconds after it arrives, then one every this long (seconds).
const MOVE_GAP: (f32, f32) = (6.0, 10.0);

/// 1.3.0 (the user: a villain in every remembrance boss fight of the base game, one fixed villain per boss): each
/// arena's map file holds its villain as a hidden human character (tools: flverport msbvillain / emevdvillain,
/// tools_scripts/villain_maps.py). The map's own event shows it while the slot's event flag is on (the flag and the
/// character share the slot's number) and hides it when the flag goes off. The game's debug character creator made
/// bodies that were never drawn (12 lab runs); a map character is drawn like any enemy.
pub struct Arena {
    /// the boss's map entity ids (more than one for bosses with two forms)
    pub bosses: &'static [u32],
    /// the villain slot's entity id and event flag
    pub slot: u32,
    /// the villain (VILLAINS id)
    pub villain: &'static str,
    /// the map file (for the tools)
    pub map: &'static str,
}

pub const ARENAS: [Arena; 16] = [
    Arena { bosses: &[10000800], slot: 10009990, villain: "frieza", map: "m10_00_00_00" },
    // 1.3.1 (the user: "create a new villain for the Margit fight ... Muscular", when Margit drops below 60%)
    Arena { bosses: &[10000850], slot: 10009991, villain: "muscular", map: "m10_00_00_00" },
    Arena { bosses: &[14000801, 14000800], slot: 14009990, villain: "kars", map: "m14_00_00_00" },
    Arena { bosses: &[1052380800], slot: 1052389990, villain: "hisoka", map: "m60_52_38_00" },
    Arena { bosses: &[16000801, 16000800], slot: 16009990, villain: "blackbeard", map: "m16_00_00_00" },
    Arena { bosses: &[11000800], slot: 11009990, villain: "madara", map: "m11_00_00_00" },
    Arena { bosses: &[1052520800], slot: 1052529990, villain: "pain", map: "m60_52_52_00" },
    Arena { bosses: &[13000801, 13000800], slot: 13009990, villain: "cell", map: "m13_00_00_00" },
    Arena { bosses: &[12050800], slot: 12059990, villain: "sukuna", map: "m12_05_00_00" },
    Arena { bosses: &[15000800], slot: 15009990, villain: "shigaraki", map: "m15_00_00_00" },
    Arena { bosses: &[12090800], slot: 12099990, villain: "meruem", map: "m12_09_00_00" },
    Arena { bosses: &[12040800], slot: 12049990, villain: "doflamingo", map: "m12_04_00_00" },
    Arena { bosses: &[12030850], slot: 12039990, villain: "dante", map: "m12_03_00_00" },
    Arena { bosses: &[11050800], slot: 11059990, villain: "all_for_one", map: "m11_05_00_00" },
    Arena { bosses: &[13000830], slot: 13009991, villain: "patry", map: "m13_00_00_00" },
    Arena { bosses: &[19000800, 19000810], slot: 19009990, villain: "aizen", map: "m19_00_00_00" },
];

/// 1.3.0 (the user: "for bosses who have second phases i think the villain should spawn in after second phase begins"):
/// when a boss's second phase begins. Bosses fought as two characters (their events show two health bars, emevdboss):
/// the second one's bar. The rest by the wiki's thresholds (Fextralife): Godrick at half health, Radahn below half,
/// Morgott below 60%, the Fire Giant at 47%, Mohg at half, Godfrey turning into Hoarah Loux after 60% of his health is
/// gone; Malenia when her bar fills again (her health back to 80% after the first bar is emptied). Bosses with one
/// phase: FIRST_AT, as before
enum Phase2 {
    One,
    Second(u32),
    Below(f32),
    Refill,
}

/// 2.0.3: a boss whose phase change the game's own event starts at 1 HP (its first phase can't die), so its last health
/// is held at 1, not a death. From the maps' .emevd: Malenia (m15 event 15002811: HP <= 1 and a hit, then her phase-two
/// SpEffects heal her, which they can't from 0) and Rykard's serpent (m16 16002811: HP ratio <= 1% with SpEffect 11955).
/// (The others: the Beast Clergyman turns at 55% (13002811); Rennala's and Radagon's first forms die (14002811 / 19002811
/// wait for the death); Godrick, Radahn, Morgott, the Fire Giant, Mohg and Godfrey change partway down one bar.)
pub fn phase_change_at_one(boss: u32) -> bool {
    matches!(phase2(boss), Phase2::Refill) || boss == 16000801
}

fn phase2(boss: u32) -> Phase2 {
    match boss {
        14000801 | 14000800 => Phase2::Second(14000800), // Rennala, Queen of the Full Moon
        16000801 | 16000800 => Phase2::Second(16000800), // the God-Devouring Serpent, then Rykard
        13000801 | 13000800 => Phase2::Second(13000800), // the Beast Clergyman, then Maliketh
        19000810 | 19000800 => Phase2::Second(19000800), // Radagon, then the Elden Beast
        10000800 => Phase2::Below(0.5),                  // Godrick
        1052380800 => Phase2::Below(0.5),                // Starscourge Radahn
        11000800 => Phase2::Below(0.6),                  // Morgott
        1052520800 => Phase2::Below(0.47),               // the Fire Giant
        12050800 => Phase2::Below(0.5),                  // Mohg
        11050800 => Phase2::Below(0.4),                  // Godfrey / Hoarah Loux
        15000800 => Phase2::Refill,                      // Malenia
        _ => Phase2::One,
    }
}

/// a second phase's villain comes this long after it begins (its transition and cutscene over)
const PHASE2_DELAY: f32 = 4.0;

/// The NpcParam / CharaInitParam row of villain `i` (the map files name these rows).
pub fn villain_row(i: usize) -> i32 {
    // (1.3.1: 9990215..20 are the second forms' rows: villains after the first fifteen from 9990221)
    if i < 15 { 9990200 + i as i32 } else { 9990221 + (i - 15) as i32 }
}

/// Whether an NpcParam row is a villain's or a villain form's (villain_row, FORMS)
pub fn is_villain_npc(id: i32) -> bool {
    (9990200..9990230).contains(&id)
}

/// 1.3.1 (the user: Muscular "stands at almost 7 ft tall in his muscular form"): a villain drawn larger than the player's
/// skeleton its model is fitted to (about 1.62 m): its size, and its second form's
const HEIGHTS: &[(&str, f32, f32)] = &[("muscular", 1.3, 1.36)];

/// 1.3.0: the armour rows the villains' models are worn through. Rows added to EquipParamProtector at runtime never reach
/// the equipment system (lab: the rows were found and dressed but no parts file was ever asked for), so the game's own rows
/// are borrowed: armour that only DLC NPCs wear (no shop, no drop; every one is on some NPC), given the villains' models
/// while the player is in the base game (where every villain arena is) and their own back in the Shadow of the Erdtree maps.
/// Helm, arms and legs: three rows shared by every villain, Goku's empty pieces (his whole model is in his body).
pub(crate) const SHARED_EMPTY: [i32; 3] = [5040000, 5040200, 5040300];
const SHARED_MODELS: [u16; 3] = [9100, 9102, 9103];
/// one body row per villain (roster order); a helm row used as a body takes a body's category
const BODY_ROWS: [i32; 16] = [5040100, 5050100, 5170100, 5172100, 5340100, 5350100, 5360100, 5370100, 5102100, 5032100, 5050000, 5170000, 5172000, 5340000, 5350000, 5102200];

/// 1.3.0 (the user: "doflamingo you could only see his jacket"): a villain whose model has its own legs piece (Doflamingo:
/// the coat is his body piece, the rest of him the legs piece) wears it through a legs row of its own (another DLC NPC's)
const OWN_LEGS: &[(&str, i32)] = &[("doflamingo", 5350300)];

/// 1.3.0 (the user: "the download for Blackbeard's player model came with flintlock models as well"): a villain's own weapon
/// (recolor::VILLAIN_HELD: his flintlock, in the right hand) worn through an arms row of its own (another DLC NPC's), put in his
/// arms slot only while a move that fires it runs (from its aim to its last shot, and a moment after): the renderer draws a
/// slot's piece again when the slot's row changes (outfit.rs). His hand is empty for his punches and the quake.
// (the user: Blackbeard's "gun is not in his hand when he shoots": the slot's new piece was drawn most of a second late)
// his flintlock is now always in hand (OWN_ARMS); none is swapped in for a move
const HELD: &[(&str, i32)] = &[];
/// how long the weapon stays in hand after the last shot (s)
const HELD_LINGER: f32 = 0.5;
/// 1.3.0 (the user: "kill the villain before it registers the boss fight as finished. Both should have to die before getting
/// the rewards of completing the boss fight"): each arena's flag its boss's defeat event waits on (mod/event, flverport
/// emevdgate: just before the defeat's pause, sound, banner, rewards and the fog's end): on while its villain fights
fn alive_flag(home: u32) -> u32 {
    home - 10
}

/// whether a villain still fights (the boss music plays on for it: scene_music.rs)
pub static FIGHTING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// 1.3.0: a villain this close (m) strikes the player itself (its own blows, between specials)
const MELEE_REACH: f32 = 3.2;
/// and from this close (m) it lunges in to strike, over this long (s)
const LUNGE_REACH: f32 = 9.0;
const LUNGE_SECS: f32 = 0.3;
/// a villain this far (m) below the player and its last ground has fallen out of the world
const FALL_RESCUE: f32 = 25.0;

/// 2.0.2 (the long-session exit): what every villain slot is made wearing at its map's load: the bare-body rows, never lent.
/// The lab bisect (2.0.1): hidden slots made in the borrowed rows at each arena's load cost the game's AI script heap
/// something it never gave back (out of memory after about 14 arenas); made bare, 28 arenas were clean, and dressing a
/// character in the borrowed rows afterwards cost nothing (lab redress_mem: 16 looks and 40 swaps, the heap flat). So the
/// summoned villain is dressed while it is still hidden (dress).
pub const BARE: [i32; 4] = [10000, 10100, 10200, 10300];

/// whether villain i wears its own model (its skin files shipped) through the borrowed rows
fn own_look(i: usize) -> bool {
    crate::recolor::villain_skin(VILLAINS[i].id).is_some_and(|p| crate::recolor::skin_exists(&format!("{p}_bd_m_1850")))
}

/// the rows villain i wears in a form (None: its own)
fn look_rows(i: usize, form: Option<usize>) -> [i32; 4] {
    let mut a = villain_armor(i);
    if let Some(fi) = form {
        a[1] = FORMS[fi].body_row;
    }
    a
}

/// A human character's armour slots (helm, body, arms, legs) pointed at these rows: the renderer loads the pieces when the
/// ids change.
pub fn dress(c: &mut ChrIns, rows: [i32; 4]) {
    if c.character_id != 0 {
        return;
    }
    let p = unsafe { &mut *(c as *mut ChrIns as *mut PlayerIns) };
    let pgd = unsafe { p.player_game_data.as_mut() };
    for ids in [&mut p.chr_asm.equipment_param_ids, &mut pgd.equipment.chr_asm.equipment_param_ids] {
        if ids[12..16] != rows {
            ids[12..16].copy_from_slice(&rows);
        }
    }
}

pub fn villain_armor(i: usize) -> [i32; 4] {
    let legs = OWN_LEGS.iter().find(|(v, _)| *v == VILLAINS[i].id).map_or(SHARED_EMPTY[2], |(_, r)| *r);
    // (a weapon always in hand: its arms row, when its piece is shipped; 2.0.3: a model with no hands of its own: Elden Ring's
    // bare arms)
    let bare = if crate::recolor::villain_er_hands(VILLAINS[i].id) { BARE[2] } else { SHARED_EMPTY[1] };
    let arms = OWN_ARMS.iter().find(|(v, _)| *v == VILLAINS[i].id).filter(|_| held_shipped(VILLAINS[i].id)).map_or(bare, |(_, r)| *r);
    [SHARED_EMPTY[0], BODY_ROWS[i], arms, legs]
}

/// 1.3.0 (the user: villains' own weapons, "e.g. Madara's gunbai"): a villain whose weapon is always in its hand, an arms piece
/// (recolor::VILLAIN_HELD) worn through an arms row of its own (another DLC NPC's). Madara's gunbai: he swings it with a
/// greatsword's moveset (LENT_WEAPONS: an unused greatsword row with no model of its own), two-handed.
const OWN_ARMS: &[(&str, i32)] = &[("madara", 5350200), ("blackbeard", 5340200)];

fn held_shipped(id: &str) -> bool {
    crate::recolor::VILLAIN_HELD.iter().any(|(v, s, _)| *v == id && crate::recolor::skin_exists(s))
}

/// 1.3.0 (the user: "Build aizen's sword please"): villains' own weapons, worn through weapon rows only NPCs use (lent the
/// model like the armour, in the base game): Aizen's Kyoka Suigetsu (model 9603, tools_scripts/package_weapons.py) on
/// 9900000, an unnamed NPC copy of the Uchigatana (the same katana moveset)
/// (1.3.0) and Madara's: 99010000, an unused greatsword row (in no shop or drop table), with no model (1: nothing drawn) under
/// his gunbai (OWN_ARMS)
/// (1.3.0, the user: Patry "should also be using a sword made of light because he is a light magic user") and Patry's: 100600,
/// an unused test row with the Longsword's moveset, shown as the Golden Order Greatsword's blade of golden light (model 627)
const LENT_WEAPONS: &[(i32, u16)] = &[(9900000, 9603), (99010000, 1), (100600, 627)];
static WEAPONS_LENT: std::sync::Mutex<Option<Vec<(i32, u16)>>> = std::sync::Mutex::new(None);

fn lend_weapons(on: bool) {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    let mut saved = WEAPONS_LENT.lock().unwrap();
    use eldenring::cs::EquipParamWeapon as W;
    if on && saved.is_none() {
        let mut keep = Vec::new();
        for &(row, model) in LENT_WEAPONS {
            if let Some(r) = repo.get_mut::<W>(row as u32) {
                keep.push((row, r.equip_model_id()));
                r.set_equip_model_id(model);
            }
        }
        log!("villains: {} weapon rows lent", keep.len());
        *saved = Some(keep);
    } else if !on {
        if let Some(keep) = saved.take() {
            for (row, model) in keep {
                if let Some(r) = repo.get_mut::<W>(row as u32) {
                    r.set_equip_model_id(model);
                }
            }
        }
    }
}

/// the borrowed rows' own looks while the villains wear them
static BORROWED: std::sync::Mutex<Option<Vec<(i32, crate::outfit::Model, u8)>>> = std::sync::Mutex::new(None);

/// Give the borrowed rows the villains' models (`on`) or their own back. Returns whether anything changed.
pub fn lend_rows(on: bool) -> bool {
    lend_weapons(on);
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return false };
    let mut saved = BORROWED.lock().unwrap();
    if on == saved.is_some() {
        return false;
    }
    use eldenring::cs::EquipParamProtector as P;
    if on {
        // templates: the Bloodhound pieces the skins replace (their model fields, flags and slot categories)
        let tpl: Vec<(crate::outfit::Model, u8)> = (0..4).filter_map(|k| repo.get::<P>(790000 + k * 100).map(|r| (crate::outfit::read(r), r.protector_category()))).collect();
        if tpl.len() < 4 {
            return false;
        }
        let mut keep = Vec::new();
        let mut lend = |row: i32, piece: usize, model: u16| {
            if let Some(r) = repo.get_mut::<P>(row as u32) {
                keep.push((row, crate::outfit::read(r), r.protector_category()));
                let mut m = tpl[piece].0;
                m.id = model;
                crate::outfit::write(r, &m);
                r.set_protector_category(tpl[piece].1);
            }
        };
        for (k, piece) in [0usize, 2, 3].iter().enumerate() {
            lend(SHARED_EMPTY[k], *piece, SHARED_MODELS[k]);
        }
        for (i, vil) in VILLAINS.iter().enumerate() {
            if crate::recolor::villain_skin(vil.id).is_some_and(|p| crate::recolor::skin_exists(&format!("{p}_bd_m_1850"))) {
                lend(BODY_ROWS[i], 1, crate::recolor::villain_model(i, 1));
                if let Some((_, row)) = OWN_LEGS.iter().find(|(v, _)| *v == vil.id) {
                    lend(*row, 3, crate::recolor::villain_model(i, 3));
                }
                for (_, f) in forms_of(vil.id) {
                    lend(f.body_row, 1, crate::recolor::villain_model(i, f.piece));
                }
                for (_, _, piece) in crate::recolor::VILLAIN_HELD.iter().filter(|(v, s, _)| *v == vil.id && crate::recolor::skin_exists(s)) {
                    if let Some((_, row)) = HELD.iter().chain(OWN_ARMS.iter()).find(|(v, _)| *v == vil.id) {
                        lend(*row, 2, crate::recolor::villain_model(i, *piece));
                    }
                }
            }
        }
        // (1.3.0) the Stands' bodies, the same way (stands.rs)
        for (i, (_, skin)) in crate::recolor::STAND_SKINS.iter().enumerate() {
            if crate::recolor::skin_exists(skin) {
                lend(crate::stands::STAND_ROWS[i], 1, crate::recolor::stand_model(i));
            }
        }
        log!("villains: {} armour rows lent to the villains' models", keep.len());
        *saved = Some(keep);
    } else if let Some(keep) = saved.take() {
        for (row, m, cat) in keep {
            if let Some(r) = repo.get_mut::<P>(row as u32) {
                crate::outfit::write(r, &m);
                r.set_protector_category(cat);
            }
        }
        log!("villains: borrowed armour rows given back (Shadow of the Erdtree map)");
    }
    true
}

/// 1.3.0 (the user: "now that we have multiple models for shigaraki we might be able to have him switch forms during his
/// fight"): a villain with a second form. Below FORM_AT of its pool it bursts and comes back as the second form (a second
/// hidden slot in its arena, slot + 1, its own rows FORM_ROW), the pool carried over. Researched (MHA wiki: after the
/// transfer Shigaraki has All For One's stolen Quirks on top of Decay): the second form fights with All For One's moves.
///
/// 1.3.0: Pain's Six Paths (the user: Pain's Six Paths in his fight; researched, Narutopedia: six bodies, each with one of the
/// Rinnegan's powers: Deva gravity, Asura a mechanised body of missiles and lasers, Human tears out souls, Animal summons
/// giant beasts, Preta absorbs chakra, Naraka the King of Hell restores them): the other five bodies are forms too, one
/// slot each, and after a special Pain changes to another path (one villain on the field at a time, the user's rule; the
/// pool shared). The ultimate, Chibaku Tensei, is the Deva Path's: he changes back to it for that.
pub struct Form {
    pub villain: &'static str,
    /// whose specials it uses ("" for its own `specials`)
    pub moves_of: &'static str,
    pub specials: &'static [Move],
    pub name: &'static str,
    /// below this share of the pool it comes (Shigaraki); None: one of the villain's paths, taken in turn (Pain)
    pub at: Option<f32>,
    /// its model piece (recolor::villain_model), NpcParam / CharaInitParam row, borrowed body row and arena slot (the
    /// villain's slot + this)
    pub piece: usize,
    pub row: i32,
    pub body_row: i32,
    pub slot_off: u32,
}
pub const FORMS: &[Form] = &[
    Form { villain: "shigaraki", moves_of: "all_for_one", specials: &[], name: "TOMURA SHIGARAKI: ALL FOR ONE", at: Some(0.5), piece: 7, row: 9990215, body_row: 5122000, slot_off: 1 },
    Form { villain: "pain", moves_of: "", specials: ASURA, name: "ASURA PATH", at: None, piece: 4, row: 9990216, body_row: 5050200, slot_off: 1 },
    Form { villain: "pain", moves_of: "", specials: HUMAN, name: "HUMAN PATH", at: None, piece: 5, row: 9990217, body_row: 5050300, slot_off: 2 },
    Form { villain: "pain", moves_of: "", specials: ANIMAL, name: "ANIMAL PATH", at: None, piece: 6, row: 9990218, body_row: 5170200, slot_off: 3 },
    Form { villain: "pain", moves_of: "", specials: PRETA, name: "PRETA PATH", at: None, piece: 8, row: 9990219, body_row: 5170300, slot_off: 4 },
    Form { villain: "pain", moves_of: "", specials: NARAKA, name: "NARAKA PATH", at: None, piece: 9, row: 9990220, body_row: 5172200, slot_off: 5 },
    Form { villain: "muscular", moves_of: "", specials: MUSCLE_ARMOR, name: "MUSCULAR: MUSCLE FIBER ARMOR", at: None, piece: 7, row: 9990222, body_row: 5172300, slot_off: 1 },
];
/// after one of its specials, a villain with paths changes to another this often
const PATH_SWITCH: f32 = 0.6;
/// 1.3.1: a form held at least this long before it changes again (lab: Muscular was out of his armour 2.5 s after putting it
/// on); Pain's paths change as before
fn form_hold(id: &str) -> f32 {
    if id == "muscular" { 12.0 } else { 0.0 }
}
pub fn forms_of(id: &str) -> impl Iterator<Item = (usize, &'static Form)> + '_ {
    FORMS.iter().enumerate().filter(move |(_, f)| f.villain == id)
}
pub fn form_of(id: &str) -> Option<&'static Form> {
    FORMS.iter().find(|f| f.villain == id)
}

// the paths' specials (Narutopedia)
/// Asura Path: missiles from his mechanised arms, and the laser from his head
const ASURA: &[Move] = &[
    // (lab: Placidusax's blast came out as red lightning: now the missiles burst in fire, the Howitzer blast's orange explosion)
    Move { name: "ASURA ATTACK", range: (4.0, 25.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Mark, V::Warn(Tell::AtTarget { r: 4.5 }, 0.9), V::Fire { b: B_MISSILE, from: From::Target(0.2), aim: Aim::Down, n: 3, every: 0.25, spread: 0.0 }, V::Wait(0.8)] },
    Move { name: "ASURA PATH: LASER", range: (5.0, 30.0), share: 0.16, steps: &[V::Face, V::Anim(A_RAISE), V::Warn(Tell::Lane { len: 30.0, w: 1.4 }, 0.9), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.2, spread: 0.0 }, V::Wait(0.6)] },
];
/// Human Path: his hand on your head pulls out your soul
const HUMAN: &[Move] = &[
    Move { name: "SOUL REMOVAL", range: (3.0, 16.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 16.0, w: 1.6 }, 0.7), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 16.0, secs: 0.6 }, V::Anim(A_HEAVY), V::Fire { b: B_ROT, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Heal(0.04), V::Wait(0.8)] },
];
/// Animal Path: a giant centipede bursts up out of the ground along a line toward you
const ANIMAL: &[Move] = &[
    Move { name: "SUMMONING: GIANT CENTIPEDE", range: (3.0, 20.0), share: 0.18, steps: &[V::Face, V::Anim(A_STOMP), V::Mark, V::Warn(Tell::Lane { len: 20.0, w: 2.5 }, 0.9), V::Fire { b: B_SPIKES, from: From::Feet, aim: Aim::Line { step: 2.2 }, n: 9, every: 0.06, spread: 0.0 }, V::Wait(0.6)] },
];
/// Preta Path: Blocking Technique Absorption Seal: he grabs you and drinks your chakra
/// 1.3.1: Muscular's Muscle Fiber Armor (the user: "He can switch forms when he uses some of his abilities"): the fibres over
/// his whole body, his heaviest blows
const MUSCLE_ARMOR: &[Move] = &[
    // he charges in shoulder first and crushes you with a hammer blow (no magic pull: his own weight carries him)
    Move { name: "MUSCLE RUSH", range: (3.0, 10.0), share: 0.16, steps: &[V::Face, V::Warn(Tell::Lane { len: 9.0, w: 2.5 }, 0.8), V::Anim(A_DASH), V::Dash { dist: 7.0, secs: 0.4 }, V::Face, V::Anim(A_HEAVY), V::Fire { b: B_DUST_STOMP, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
    // the armoured fists hammered down twice
    Move { name: "FIBER HAMMER", range: (0.0, 9.0), share: 0.18, steps: &[V::Face, V::Warn(Tell::Lane { len: 9.0, w: 4.0 }, 0.8), V::Anim(A_SMASH), V::Fire { b: B_DUST_STOMP, from: From::Feet, aim: Aim::Line { step: 2.4 }, n: 3, every: 0.08, spread: 0.0 }, V::Wait(0.4), V::Anim(A_SMASH), V::Fire { b: B_DUST_BLAST, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
];
const PRETA: &[Move] = &[
    Move { name: "ABSORPTION SEAL", range: (2.0, 14.0), share: 0.12, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 14.0, w: 1.6 }, 0.7), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 14.0, secs: 0.6 }, V::Anim(A_HEAVY), V::Heal(0.08), V::Wait(0.8)] },
];
/// Naraka Path: the King of Hell rises behind him and restores the paths
const NARAKA: &[Move] = &[
    Move { name: "KING OF HELL", range: (0.0, 40.0), share: 0.0, steps: &[V::Anim(A_RAISE), V::Warn(Tell::AtSelf { r: 3.0 }, 0.8), V::Heal(0.12), V::Wait(1.0)] },
    // (and the King of Hell judges: its tongue drags out the soul of whoever it holds)
    Move { name: "KING OF HELL: JUDGEMENT", range: (2.0, 14.0), share: 0.14, steps: &[V::Face, V::Anim(A_RAISE), V::Warn(Tell::Lane { len: 14.0, w: 1.6 }, 0.8), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 14.0, secs: 0.6 }, V::Fire { b: B_ROT, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
];

/// The character with this map entity id, wherever it is loaded.
fn by_entity(id: u32) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()).find(|c| c.event_entity_id == id).map(|c| unsafe { &mut *(c as *mut ChrIns) })
}

/// 1.3.0 (lab: after Shigaraki changed form his first body stayed in the fight beside All For One; the map event's hide
/// didn't take on a character already fighting): the character in slot `id` drawn, hit and thinking, or none of those
fn set_present(id: u32, on: bool) {
    let Some(c) = by_entity(id) else { return };
    c.chr_flags1c5.set_enable_render(on);
    c.chr_ctrl.flags.set_disable_hit(!on);
    c.chr_ctrl.flags.set_disable_character_capsule_collision(!on);
    c.debug_flags.set_disabled_movement(!on);
    c.debug_flags.set_disabled_secondary_actions(!on);
    c.debug_flags.set_disabled_hit(!on);
}

/// every slot flag of an arena: its villain's own and each of its forms' (home + slot_off). (2.0.2, the user: Muscular "respawns
/// every time your character loads into the world": his Muscle Fiber Armor body's slot, home + 1, was never cleared, and a
/// slot left on shows its character at every load)
fn slots_of(a: &'static Arena) -> impl Iterator<Item = u32> {
    std::iter::once(a.slot).chain(FORMS.iter().filter(move |f| f.villain == a.villain).map(move |f| a.slot + f.slot_off))
}

/// every arena's slots off (no fight: none of them should show its character)
fn all_slots_off(why: &str) {
    // (each slot logged once until the next load: after a boss's defeat the game turns a dead villain's slot back on every
    // frame, and this clears it every 2 s for as long as the player stays)
    static SAID: std::sync::Mutex<Vec<u32>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap();
    if why == "loading" {
        said.clear();
    }
    for a in ARENAS.iter() {
        for s in slots_of(a) {
            if flag(s) {
                set_flag(s, false);
                if !said.contains(&s) {
                    said.push(s);
                    log!("villains: slot {s} ({}) was left on: off ({why}; reads back {})", a.map, flag(s));
                }
            }
        }
    }
}

/// lab (2.0.2, the long-session memory): the character in slot `id` re-dressed: "vil <i>" in villain i's armour rows (the lent
/// looks), "naked" (10000.. rows), or "rows <a> <b> <c> <d>"; the renderer loads the pieces when the ids change
#[cfg(feature = "lab")]
pub fn lab_redress(id: u32, how: &str) {
    let Some(c) = by_entity(id) else { log!("lab script: redress: no character {id}"); return };
    if c.character_id != 0 {
        return;
    }
    let mut it = how.split_whitespace();
    let rows: [i32; 4] = match it.next() {
        Some("vil") => villain_armor(it.next().and_then(|t| t.parse().ok()).unwrap_or(0).min(VILLAINS.len() - 1)),
        Some("rows") => {
            let v: Vec<i32> = it.filter_map(|t| t.parse().ok()).collect();
            if v.len() < 4 { return; }
            [v[0], v[1], v[2], v[3]]
        }
        _ => BARE,
    };
    if let Ok(repo) = unsafe { SoloParamRepository::instance() } {
        if let Some(r) = rows.iter().find(|r| repo.get::<eldenring::cs::EquipParamProtector>(**r as u32).is_none()) {
            log!("lab script: redress: no armour row {r}");
            return;
        }
    }
    dress(c, rows);
    log!("lab script: slot {id} dressed in {rows:?}");
}

/// lab: each slot of the arenas with a character loaded: its flag, and its character (health, drawn, how far)
#[cfg(feature = "lab")]
pub fn slot_report(me: V3) {
    for a in ARENAS.iter() {
        for s in slots_of(a) {
            let c = by_entity(s);
            if c.is_none() && !flag(s) {
                continue;
            }
            match c {
                Some(c) => {
                    let p = pos_of(c);
                    log!("lab script: slot {s} ({}) flag {} chr hp {} render {} active {} dead {} at {:.1} m", a.villain, flag(s), c.modules.data.hp, c.chr_flags1c5.enable_render(),
                        c.chr_flags1c8.is_active(), c.chr_flags1c5.death_flag(),
                        ((p.0 - me.0).powi(2) + (p.1 - me.1).powi(2) + (p.2 - me.2).powi(2)).sqrt());
                }
                None => log!("lab script: slot {s} ({}) flag {} no chr", a.villain, flag(s)),
            }
        }
    }
}

fn flag(id: u32) -> bool {
    unsafe { eldenring::cs::CSEventFlagMan::instance() }.is_ok_and(|f| f.virtual_memory_flag.get_flag(id))
}

fn set_flag(id: u32, on: bool) {
    if let Ok(f) = unsafe { eldenring::cs::CSEventFlagMan::instance_mut() } {
        f.virtual_memory_flag.set_flag(id, on);
    }
}

/// 1.3.0 (the user: Frieza "has a straight sword moveset ... He should have a fist moveset"; lab meter: his punches did
/// nothing): an enemy given the empty hand (110000) is treated as unarmed: its hits deal no damage and its AI falls back
/// to straight sword attacks. Row 100500 is the same fist under another id (an unused, unnamed test row, given the bare
/// hands' data by strip_fx.json row_new), which the game treats as a real weapon: the fist villains hold it. (They held the
/// Uchigatana's row 9000000 before: it only acted as a fist while the fighters' hands happened to be dressed over it, and
/// the punch damage written to it changed Zoro's, Sasuke's and the player's own Uchigatana.)
const VILLAIN_FIST: i32 = 100500;
/// the fist's attack at FIST_ATTACK_HP of player health (about 2.4% of the bar a punch), scaled with it
const FIST_ATTACK: f32 = 109.0;
const FIST_ATTACK_HP: f32 = 910.0;
fn fist_row(w: i32) -> i32 {
    if w == crate::weapon::UNARMED as i32 { VILLAIN_FIST } else { w }
}

/// Every villain's rows dressed (looks, weapons, a hostile enemy's team, no runes or drops), as soon as the
/// rows exist: the map files' villains take these when their area loads.
pub fn dress_rows() {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
    for (i, vil) in VILLAINS.iter().enumerate() {
        let row = villain_row(i) as u32;
        // 1.3.0: their own models (the user's downloads), worn through borrowed armour rows (MODEL_ROWS) when the skin
        // files are shipped; the filler armour otherwise
        // (2.0.2) made bare (BARE), dressed in them when summoned
        let armor = if own_look(i) {
            BARE
        } else {
            vil.armor
        };
        if let Some(c) = repo.get_mut::<CharaInitParam>(row) {
            c.set_equip_helm(armor[0]);
            c.set_equip_armer(armor[1]);
            c.set_equip_gaunt(armor[2]);
            c.set_equip_leg(armor[3]);
            let w = [fist_row(vil.weapons[0]), fist_row(vil.weapons[1])];
            c.set_equip_wep_right(w[0]);
            c.set_equip_wep_left(w[1]);
            c.set_equip_subwep_right(w[0]);
            c.set_equip_subwep_left(w[1]);
        }
        if let Some(n) = repo.get_mut::<NpcParam>(row) {
            n.set_team_type(HOSTILE_TEAM);
            n.set_get_soul(0);
            n.set_item_lot_id_enemy(-1);
            n.set_item_lot_id_map(-1);
            n.set_super_armor_durability(VILLAIN_STANCE);
            n.set_toughness(VILLAIN_POISE as u32);
            // (lab: the copied enemy's name, "Yura, Hunter of Bloody Fingers", floated over every villain) no name tag: the
            // villain's banner names it
            n.set_name_id(-1);
        }
    }
    for f in FORMS {
        let Some(i) = VILLAINS.iter().position(|v| v.id == f.villain) else { continue };
        let armor = if own_look(i) { BARE } else { let mut a = villain_armor(i); a[1] = f.body_row; a };
        if let Some(c) = repo.get_mut::<CharaInitParam>(f.row as u32) {
            c.set_equip_helm(armor[0]);
            c.set_equip_armer(armor[1]);
            c.set_equip_gaunt(armor[2]);
            c.set_equip_leg(armor[3]);
            let w = [fist_row(VILLAINS[i].weapons[0]), fist_row(VILLAINS[i].weapons[1])];
            c.set_equip_wep_right(w[0]);
            c.set_equip_wep_left(w[1]);
            c.set_equip_subwep_right(w[0]);
            c.set_equip_subwep_left(w[1]);
        }
        if let Some(n) = repo.get_mut::<NpcParam>(f.row as u32) {
            n.set_team_type(HOSTILE_TEAM);
            n.set_get_soul(0);
            n.set_item_lot_id_enemy(-1);
            n.set_item_lot_id_map(-1);
            n.set_super_armor_durability(VILLAIN_STANCE);
            n.set_toughness(VILLAIN_POISE as u32);
            // (lab: the copied enemy's name, "Yura, Hunter of Bloody Fingers", floated over every villain) no name tag: the
            // villain's banner names it
            n.set_name_id(-1);
        }
    }
    lend_rows(true);
    log!("villains: {} villain rows dressed", VILLAINS.len());
}

/// Where a special's attacks come from.
#[derive(Clone, Copy, Debug)]
pub enum From {
    /// the villain's hands, a little in front of its chest
    Hand,
    /// this high above the villain
    Above(f32),
    /// this high above where the target was marked
    Target(f32),
    /// the villain's feet
    Feet,
}

/// Which way they fly.
#[derive(Clone, Copy, Debug)]
pub enum Aim {
    /// at the marked target (chest height)
    AtTarget,
    /// straight down
    Down,
    /// along the ground toward the mark: one shot at each of `n` points `step` metres apart (from `From`)
    Line { step: f32 },
    /// a ring of shots around the mark, `r` metres out, flying in toward it
    Inward { r: f32 },
    /// a ring around the source, flying out
    Outward,
}

/// The warning before a special strikes.
#[derive(Clone, Copy, Debug)]
pub enum Tell {
    /// a seal `r` metres round where the target is (it follows them until a moment before it strikes)
    AtTarget { r: f32 },
    /// a seal round the villain
    AtSelf { r: f32 },
    /// a lane from the villain toward the target, `len` long and `w` wide
    Lane { len: f32, w: f32 },
}

#[derive(Clone, Copy, Debug)]
pub enum V {
    /// the villain's body plays this animation (Elden Ring's Ashes of War, Carian Combo Warriors)
    Anim(i32),
    /// it turns to face the player
    Face,
    /// the warning, this long before the next attack step
    Warn(Tell, f32),
    /// the mark is set where the player is now
    Mark,
    /// `n` shots of bullet `b` (its damage shared out by the move), `every` seconds apart, `spread` radians apart
    Fire { b: i32, from: From, aim: Aim, n: u32, every: f32, spread: f32 },
    /// the player is pulled toward the villain at `speed` m/s for `secs`
    Pull { speed: f32, secs: f32 },
    /// the villain vanishes and comes out this far behind the player (Warp Gate, Light Speed)
    Blink(f32),
    /// the villain dashes this far toward (and through) the player over `secs`
    Dash { dist: f32, secs: f32 },
    /// it heals this share of its health
    Heal(f32),
    Wait(f32),
}

pub struct Move {
    pub name: &'static str,
    /// it uses it from this far to this far from the player
    pub range: (f32, f32),
    /// share of the player's health the whole move can take (at most MOVE_CAP)
    pub share: f32,
    pub steps: &'static [V],
}

pub struct Villain {
    pub id: &'static str,
    pub name: &'static str,
    pub series: &'static str,
    /// the fighters whose boss fights call them
    pub rivals: &'static [&'static str],
    /// filler look: helm, body, arms, legs (EquipParamProtector), right and left weapons
    pub armor: [i32; 4],
    pub weapons: [i32; 2],
    /// its colour (banners, seals)
    pub color: [f32; 4],
    pub specials: &'static [Move],
    pub ultimate: Move,
}

// --- animations (Elden Ring's own motion capture, as the fighters' abilities play them; lab-checked on a villain)
const A_PALM: i32 = 861040000; // Palm Blast: the gather and the palm thrust
const A_SMASH: i32 = 858040000;
const A_LEAP: i32 = 741040000;
const A_KICK: i32 = 880040000;
const A_DASH: i32 = 624040000;
const A_STOMP: i32 = 710040000;
const A_RAISE: i32 = 825040000;
const A_HEAVY: i32 = 42030600; // Carian Combo Warriors' fist heavy
/// 1.3.0: an arm thrust straight out at the target (the game's Point Forwards gesture, an event animation a human character
/// does play when asked): aiming a flintlock (lab: the arm is level, the pistol on the target, 0.9-1.4 s in)
// Point Forwards: the arm is out level from 1.35 s to 2.5 s in (work/portraits/game_pose.py on a000_080200); it swings up
// past level (20 degrees) before that (the user: "his arm bends weird": his shots fired then, at 0.9 s)
const A_AIM: i32 = 80200;
/// the wait after A_AIM before the glint, so the shots land while the arm is level
const AIM_LEVEL: f32 = 1.0;

// --- spells (Elden Ring bosses' and NPCs' own: their damage rows are the enemy table's)
/// 1.3.0 (the user: Blackbeard has guns, he should use them): Carian Combo Warriors' curved-greatsword pistol shot (its
/// 30 m/s; its attack row copied to the enemy table: strip_fx.json villain_atk 800004). The user: "a black projectile with a
/// thin yellow fiery aura so that you can see them and they look like bullets": its copy 9990742, a dark stone ball leading a
/// thin fiery-yellow streak (its own effect was a whole curved-greatsword slash: fx_copies.json 900700058)
const B_PISTOL: i32 = 9990742;
/// (the user: his bullets "didn't have any indication that they are coming") a gold glint held at the muzzle for 0.45 s before
/// the shot (strip_fx.json 9990786, look only)
const B_GLINT: i32 = 9990786;
/// 2.0.3 (the user on Shigaraki: "you just take damage with no way to tell that an attack is coming. I want to see the ground
/// rumble or have an earth shatter effect coming at you to indicate that decay is coming"): look only, no damage
/// (strip_fx.json): the ground cracking open in a run toward you before his decay (the Erdtree Avatar's stomp, its dust and
/// rock), and the ground crumbling where the decay lands (the Draconic Tree Sentinel's slam, a wide burst of dust)
const B_CRACK: i32 = 9990796;
const B_CRUMBLE: i32 = 9990797;
/// shots that only show something: no damage of their own, no part of a move's damage share
fn look_only(b: i32) -> bool {
    b == B_GLINT || b == B_CRACK || b == B_CRUMBLE
}
const B_LASER: i32 = 4620220; // Astel's laser: Death Beam, Special Beam Cannon
const B_PUSH: i32 = 3600403; // the Alabaster Lords' push: Shinra Tensei
const B_PULL: i32 = 3600204; // the Alabaster Lords' pull: Bansho Ten'in, Kurouzu
const B_ORB_PULL: i32 = 3600220; // the Alabaster Lords' gravity orb: Chibaku Tensei, Gravity Singularity
const B_ORB_PUSH: i32 = 3600440; // gravity orb push: Air Cannon
const B_METEORS: i32 = 3600373; // the Alabaster Lords' meteors
const B_METEOR: i32 = 4730342; // Radahn's meteor: Tengai Shinsei, Death Ball
const B_QUAKE: i32 = 4730224; // Radahn's shockwave: quake punch
const B_SLAM_WAVE: i32 = 4510951; // an Ancient Dragon's moving shockwave
const B_FIRE: i32 = 2500210; // a Crucible Knight's fire breath: Great Fire Annihilation
const B_SWORD_BEAM: i32 = 2200294; // the Elden Beast's sword beam: Susanoo, Light Swords
const B_SWORD_DOWN: i32 = 2200330; // the Elden Beast's vertical sword beam
const B_SPIKES: i32 = 3100410; // Elemer of the Briar's briars: Wood Release, flesh spears, Rivet Stab
const B_THREAD: i32 = 3810190; // the Kindred of Rot's thread: Doflamingo's strings
const B_ROT: i32 = 3050905; // a Commander's rot: Shigaraki's decay, Blackbeard's darkness
const B_ARROWS: i32 = 4350680; // Castle Knights' rain of arrows: Light Arrows
const B_HOLY: i32 = 2190521; // Radagon's holy shockwave: Perfect Barrier
const B_HOLY_FLOOR: i32 = 2190296; // Radagon's holy floor: Light Creation
/// 1.3.1: Muscular's blows (the user: "he doesnt ... control any magic"): plain dust and rock thrown up off the ground, no glow
/// (lab rockaud: the Erdtree Avatar's stomp, a ring of dust; the Draconic Tree Sentinel's slam, a wide burst of dust)
const B_DUST_STOMP: i32 = 4810250;
const B_DUST_BLAST: i32 = 3250141;
const B_SPEAR: i32 = 4350670; // a Castle Knight's thrown spear: chakra rods
const B_EXPLOSION: i32 = 4520325; // Placidusax's giant explosion
const B_MISSILE: i32 = 9990652; // the Howitzer blast (Giantsflame's explosion, orange): Asura Path missiles

pub const VILLAINS: [Villain; 16] = [
    Villain {
        id: "doflamingo", name: "Doflamingo", series: "One Piece", rivals: &["luffy"],
        armor: [-1, 930100, 930200, 930300], weapons: [110000, 110000], color: [1.0, 0.45, 0.7, 1.0],
        specials: &[
            Move { name: "OVERHEAT", range: (3.0, 18.0), share: 0.18, steps: &[V::Face, V::Anim(A_KICK), V::Warn(Tell::Lane { len: 18.0, w: 2.0 }, 0.7), V::Fire { b: B_THREAD, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.05, spread: 0.0 }, V::Wait(0.6)] },
            Move { name: "TAMAITO", range: (4.0, 25.0), share: 0.14, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 25.0, w: 3.0 }, 0.6), V::Fire { b: B_THREAD, from: From::Hand, aim: Aim::AtTarget, n: 5, every: 0.08, spread: 0.1 }, V::Wait(0.6)] },
            Move { name: "GOSHIKITO", range: (3.0, 18.0), share: 0.2, steps: &[V::Face, V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 4.0 }, 0.9), V::Fire { b: B_THREAD, from: From::Target(8.0), aim: Aim::Down, n: 5, every: 0.06, spread: 0.0 }, V::Wait(0.6)] },
        ],
        ultimate: Move { name: "BIRDCAGE", range: (0.0, 30.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 12.0 }, 1.8), V::Fire { b: B_THREAD, from: From::Target(1.2), aim: Aim::Inward { r: 12.0 }, n: 14, every: 0.0, spread: 0.0 }, V::Wait(1.2)] },
    },
    Villain {
        id: "frieza", name: "Frieza", series: "Dragon Ball", rivals: &["goku"],
        armor: [600000, 600100, 600200, 600300], weapons: [110000, 110000], color: [0.85, 0.45, 1.0, 1.0],
        specials: &[
            Move { name: "DEATH BEAM", range: (4.0, 30.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 30.0, w: 1.0 }, 0.7), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.35, spread: 0.0 }, V::Wait(0.6)] },
            Move { name: "NOVA STRIKE", range: (5.0, 18.0), share: 0.16, steps: &[V::Face, V::Warn(Tell::Lane { len: 14.0, w: 2.0 }, 0.6), V::Anim(A_DASH), V::Dash { dist: 14.0, secs: 0.35 }, V::Fire { b: B_PUSH, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.6)] },
            Move { name: "DEATH SAUCER", range: (3.0, 25.0), share: 0.14, steps: &[V::Face, V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 2.5 }, 0.8), V::Fire { b: B_SWORD_BEAM, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.3, spread: 0.3 }, V::Wait(0.6)] },
        ],
        ultimate: Move { name: "DEATH BALL", range: (0.0, 35.0), share: 0.3, steps: &[V::Face, V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 9.0 }, 1.8), V::Fire { b: B_METEOR, from: From::Target(18.0), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.9), V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "patry", name: "Patry", series: "Black Clover", rivals: &["asta"],
        armor: [330000, 330100, 330200, 330300], weapons: [100600, 110000], color: [1.0, 0.9, 0.55, 1.0],
        specials: &[
            Move { name: "LIGHT SPEED", range: (8.0, 40.0), share: 0.14, steps: &[V::Blink(2.0), V::Face, V::Warn(Tell::AtSelf { r: 3.0 }, 0.6), V::Anim(A_HEAVY), V::Fire { b: B_HOLY, from: From::Feet, aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            // (researched, BC wiki) Bright Judgment Whip: a long whip of light lashing out from his hand
            Move { name: "BRIGHT JUDGMENT WHIP", range: (3.0, 16.0), share: 0.16, steps: &[V::Face, V::Anim(A_HEAVY), V::Warn(Tell::Lane { len: 16.0, w: 3.0 }, 0.7), V::Fire { b: B_SWORD_BEAM, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.1, spread: 0.35 }, V::Wait(0.6)] },
            // Light Sword of Judgment: shards of light thrown at you
            Move { name: "LIGHT SWORD OF JUDGMENT", range: (5.0, 28.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 28.0, w: 3.0 }, 0.8), V::Fire { b: B_ARROWS, from: From::Hand, aim: Aim::AtTarget, n: 5, every: 0.08, spread: 0.12 }, V::Wait(0.6)] },
        ],
        // Heaven-Splitting Flash: orbs of light all around you, firing beams in (BC wiki)
        ultimate: Move { name: "HEAVEN-SPLITTING FLASH", range: (0.0, 30.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 6.0 }, 1.4), V::Fire { b: B_HOLY_FLOOR, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.5), V::Fire { b: B_SWORD_DOWN, from: From::Target(10.0), aim: Aim::Inward { r: 5.0 }, n: 8, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "cell", name: "Cell", series: "Dragon Ball", rivals: &["goku"],
        armor: [5010000, 5010100, 5010200, 5010300], weapons: [110000, 110000], color: [0.45, 0.95, 0.5, 1.0],
        specials: &[
            Move { name: "KAMEHAMEHA", range: (4.0, 30.0), share: 0.2, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 30.0, w: 2.0 }, 0.85), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.12, spread: 0.0 }, V::Wait(0.7)] },
            Move { name: "PERFECT BARRIER", range: (0.0, 6.0), share: 0.16, steps: &[V::Anim(A_STOMP), V::Warn(Tell::AtSelf { r: 6.0 }, 0.7), V::Fire { b: B_HOLY, from: From::Feet, aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
            Move { name: "SPECIAL BEAM CANNON", range: (6.0, 35.0), share: 0.2, steps: &[V::Face, V::Anim(A_RAISE), V::Warn(Tell::Lane { len: 35.0, w: 1.2 }, 1.5), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.6)] },
        ],
        // (researched, DB wiki: he stores immense energy for a Super Kamehameha and fires it forward with both hands)
        ultimate: Move { name: "SOLAR KAMEHAMEHA", range: (4.0, 35.0), share: 0.3, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 35.0, w: 4.0 }, 1.6), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 6, every: 0.08, spread: 0.06 }, V::Wait(0.3), V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "blackbeard", name: "Blackbeard", series: "One Piece", rivals: &["luffy"],
        armor: [380000, 380100, 930200, 930300], weapons: [110000, 110000], color: [0.55, 0.35, 0.85, 1.0],
        specials: &[
            // (researched, One Piece wiki: he carries flintlock pistols and shoots with them; the user: "despite having guns,
            // Blackbeard doesn't use them") Kurouzu: the darkness drags you in, and a pistol shot point-blank as you arrive
            Move { name: "KUROUZU", range: (5.0, 18.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 18.0, w: 2.0 }, 0.6), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 16.0, secs: 0.7 }, V::Face, V::Anim(A_AIM), V::Wait(AIM_LEVEL), V::Fire { b: B_GLINT, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.35), V::Fire { b: B_PISTOL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            // his flintlocks: he levels a pistol and fires three shots
            Move { name: "FLINTLOCK", range: (5.0, 25.0), share: 0.15, steps: &[V::Face, V::Anim(A_AIM), V::Wait(AIM_LEVEL), V::Fire { b: B_GLINT, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Warn(Tell::Lane { len: 25.0, w: 1.2 }, 0.35), V::Fire { b: B_PISTOL, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.3, spread: 0.05 }, V::Wait(0.6)] },
            Move { name: "QUAKE PUNCH", range: (2.0, 14.0), share: 0.2, steps: &[V::Face, V::Anim(A_SMASH), V::Warn(Tell::Lane { len: 14.0, w: 5.0 }, 0.8), V::Fire { b: B_SLAM_WAVE, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_QUAKE, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
            Move { name: "BLACK HOLE", range: (0.0, 9.0), share: 0.16, steps: &[V::Anim(A_STOMP), V::Warn(Tell::AtSelf { r: 8.0 }, 0.8), V::Fire { b: B_ROT, from: From::Feet, aim: Aim::Outward, n: 6, every: 0.05, spread: 1.05 }, V::Wait(0.8)] },
        ],
        ultimate: Move { name: "GURA GURA + YAMI YAMI", range: (0.0, 20.0), share: 0.3, steps: &[V::Face, V::Anim(A_PALM), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 16.0, secs: 0.6 }, V::Face, V::Anim(A_AIM), V::Wait(AIM_LEVEL), V::Fire { b: B_GLINT, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.35), V::Fire { b: B_PISTOL, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.25, spread: 0.0 }, V::Warn(Tell::AtSelf { r: 9.0 }, 0.6), V::Anim(A_SMASH), V::Fire { b: B_QUAKE, from: From::Feet, aim: Aim::Outward, n: 2, every: 0.5, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "kars", name: "Kars", series: "JoJo's Bizarre Adventure", rivals: &[],
        armor: [970000, 970100, 970200, 970300], weapons: [110000, 110000], color: [0.95, 0.85, 0.45, 1.0],
        specials: &[
            // Light Mode: Light Blades (researched: bladed appendages from his arms, chainsaw-edged, reflecting blinding light): a cutting charge
            Move { name: "LIGHT BLADES", range: (4.0, 15.0), share: 0.18, steps: &[V::Face, V::Warn(Tell::Lane { len: 13.0, w: 2.0 }, 0.6), V::Anim(A_DASH), V::Dash { dist: 12.0, secs: 0.3 }, V::Fire { b: B_SWORD_BEAM, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.1, spread: 0.4 }, V::Wait(0.6)] },
            // a Pillar Man's body: bones turned into spikes from the ground
            Move { name: "BONE SPIKES", range: (3.0, 18.0), share: 0.16, steps: &[V::Face, V::Anim(A_STOMP), V::Warn(Tell::Lane { len: 18.0, w: 2.5 }, 0.8), V::Fire { b: B_SPIKES, from: From::Feet, aim: Aim::Line { step: 2.2 }, n: 8, every: 0.06, spread: 0.0 }, V::Wait(0.6)] },
            // the Ultimate Life Form takes to the air and comes down
            Move { name: "ULTIMATE LIFE FORM", range: (5.0, 20.0), share: 0.18, steps: &[V::Face, V::Anim(A_LEAP), V::Mark, V::Warn(Tell::AtTarget { r: 4.0 }, 0.9), V::Fire { b: B_QUAKE, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
        ],
        // (researched, JoJo wiki: the Ultimate Kars' Ripple is "comparable to the intensity of the Sun itself") blinding light on
        // you, then his light blades closing in from every side
        ultimate: Move { name: "ULTIMATE RIPPLE", range: (0.0, 30.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 7.0 }, 1.6), V::Fire { b: B_HOLY_FLOOR, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.5), V::Fire { b: B_SWORD_BEAM, from: From::Target(1.2), aim: Aim::Inward { r: 6.0 }, n: 8, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "all_for_one", name: "All For One", series: "My Hero Academia", rivals: &["deku", "all_might", "bakugo", "todoroki"],
        armor: [230000, 230100, 230200, 230300], weapons: [110000, 110000], color: [0.6, 0.2, 0.3, 1.0],
        specials: &[
            Move { name: "AIR CANNON", range: (3.0, 25.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 25.0, w: 3.0 }, 0.7), V::Fire { b: B_ORB_PUSH, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            Move { name: "RIVET STAB", range: (3.0, 16.0), share: 0.16, steps: &[V::Face, V::Anim(A_KICK), V::Mark, V::Warn(Tell::Lane { len: 16.0, w: 3.0 }, 0.7), V::Fire { b: B_SPIKES, from: From::Feet, aim: Aim::Line { step: 2.0 }, n: 7, every: 0.05, spread: 0.0 }, V::Wait(0.6)] },
            Move { name: "WARP GATE", range: (8.0, 40.0), share: 0.14, steps: &[V::Blink(2.5), V::Face, V::Warn(Tell::AtSelf { r: 3.5 }, 0.6), V::Anim(A_HEAVY), V::Fire { b: B_PUSH, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
        ],
        ultimate: Move { name: "KINETIC + STRENGTH + SPRING", range: (0.0, 25.0), share: 0.3, steps: &[V::Face, V::Mark, V::Warn(Tell::AtTarget { r: 7.0 }, 1.2), V::Anim(A_LEAP), V::Wait(0.6), V::Fire { b: B_QUAKE, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "madara", name: "Madara Uchiha", series: "Naruto", rivals: &["naruto"],
        armor: [870000, 870100, 870200, 870300], weapons: [99010000, 110000], color: [1.0, 0.3, 0.25, 1.0],
        specials: &[
            Move { name: "GREAT FIRE ANNIHILATION", range: (2.0, 14.0), share: 0.2, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 14.0, w: 7.0 }, 0.9), V::Fire { b: B_FIRE, from: From::Hand, aim: Aim::AtTarget, n: 6, every: 0.1, spread: 0.18 }, V::Wait(0.8)] },
            Move { name: "SUSANOO", range: (3.0, 25.0), share: 0.18, steps: &[V::Face, V::Anim(A_SMASH), V::Warn(Tell::Lane { len: 25.0, w: 3.0 }, 0.8), V::Fire { b: B_SWORD_BEAM, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            Move { name: "WOOD RELEASE", range: (4.0, 18.0), share: 0.18, steps: &[V::Face, V::Anim(A_STOMP), V::Mark, V::Warn(Tell::Lane { len: 18.0, w: 2.5 }, 0.8), V::Fire { b: B_SPIKES, from: From::Feet, aim: Aim::Line { step: 2.2 }, n: 8, every: 0.06, spread: 0.0 }, V::Wait(0.6)] },
        ],
        ultimate: Move { name: "TENGAI SHINSEI", range: (0.0, 40.0), share: 0.3, steps: &[V::Face, V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 8.0 }, 2.0), V::Fire { b: B_METEOR, from: From::Target(25.0), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0), V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "hisoka", name: "Hisoka Morow", series: "Hunter x Hunter", rivals: &[],
        armor: [600000, 600100, 600200, 600300], weapons: [110000, 110000], color: [0.95, 0.5, 0.75, 1.0],
        specials: &[
            // Bungee Gum: it has the properties of both rubber and gum; you snap to him
            Move { name: "BUNGEE GUM", range: (4.0, 18.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 18.0, w: 1.4 }, 0.6), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 16.0, secs: 0.5 }, V::Anim(A_HEAVY), V::Wait(0.7)] },
            // playing cards thrown, reinforced with Nen
            Move { name: "CARD THROW", range: (5.0, 25.0), share: 0.14, steps: &[V::Face, V::Anim(A_KICK), V::Warn(Tell::Lane { len: 25.0, w: 3.0 }, 0.6), V::Fire { b: B_SPEAR, from: From::Hand, aim: Aim::AtTarget, n: 5, every: 0.05, spread: 0.12 }, V::Wait(0.6)] },
            // Texture Surprise: what you saw was a fake; he steps out beside you
            Move { name: "TEXTURE SURPRISE", range: (6.0, 30.0), share: 0.14, steps: &[V::Blink(2.5), V::Face, V::Warn(Tell::AtSelf { r: 2.5 }, 0.6), V::Anim(A_HEAVY), V::Fire { b: B_PUSH, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
        ],
        // bungee gum on everything: you are dragged in to a barrage of cards
        ultimate: Move { name: "BUNGEE GUM: CONTRACT", range: (0.0, 22.0), share: 0.3, steps: &[V::Face, V::Anim(A_RAISE), V::Warn(Tell::Lane { len: 22.0, w: 2.0 }, 1.0), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 18.0, secs: 0.6 }, V::Fire { b: B_SPEAR, from: From::Hand, aim: Aim::AtTarget, n: 8, every: 0.06, spread: 0.2 }, V::Wait(1.0)] },
    },
    Villain {
        id: "meruem", name: "Meruem", series: "Hunter x Hunter", rivals: &[],
        armor: [5010000, 5010100, 5010200, 5010300], weapons: [110000, 110000], color: [0.35, 0.85, 0.6, 1.0],
        specials: &[
            // the tail: a lunge that crosses the room
            Move { name: "TAIL STRIKE", range: (4.0, 14.0), share: 0.18, steps: &[V::Face, V::Warn(Tell::Lane { len: 12.0, w: 2.0 }, 0.6), V::Anim(A_DASH), V::Dash { dist: 11.0, secs: 0.25 }, V::Anim(A_HEAVY), V::Wait(0.6)] },
            // Rage Blast (researched, Hunterpedia: explosive blasts of aura fired from his arm, enough to wreck a rock formation)
            Move { name: "RAGE BLAST", range: (4.0, 22.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Mark, V::Warn(Tell::AtTarget { r: 3.5 }, 0.7), V::Fire { b: B_ORB_PUSH, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.25, spread: 0.1 }, V::Wait(0.7)] },
            // a beam of Nen compressed in the palm
            Move { name: "NEN BEAM", range: (6.0, 30.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 30.0, w: 1.6 }, 0.9), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.1, spread: 0.0 }, V::Wait(0.6)] },
        ],
        // (researched: the Miniature Rose was the bomb used on him; his strongest is Photon, Youpi's and Pouf's powers combined:
        // a sustained torrent of aura) a long beam that sweeps toward you, its end exploding
        ultimate: Move { name: "PHOTON", range: (4.0, 35.0), share: 0.3, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 35.0, w: 3.0 }, 1.4), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 6, every: 0.12, spread: 0.08 }, V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "aizen", name: "Sosuke Aizen", series: "Bleach", rivals: &[],
        armor: [330000, 330100, 330200, 330300], weapons: [9900000, 110000], color: [0.55, 0.35, 0.85, 1.0],
        specials: &[
            // Kyoka Suigetsu: the you that you fought was an illusion; he is behind you
            Move { name: "KYOKA SUIGETSU", range: (4.0, 30.0), share: 0.16, steps: &[V::Blink(2.0), V::Face, V::Warn(Tell::AtSelf { r: 2.5 }, 0.6), V::Anim(A_HEAVY), V::Fire { b: B_PUSH, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            // Hado #90 Kurohitsugi: the black coffin closes around you
            Move { name: "HADO #90: KUROHITSUGI", range: (4.0, 25.0), share: 0.2, steps: &[V::Face, V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 3.5 }, 1.0), V::Fire { b: B_ORB_PULL, from: From::Target(1.5), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.9)] },
            // Hado #63 Raikoho: thunder from the palm
            Move { name: "HADO #63: RAIKOHO", range: (5.0, 30.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 30.0, w: 1.6 }, 0.8), V::Fire { b: B_LASER, from: From::Hand, aim: Aim::AtTarget, n: 2, every: 0.15, spread: 0.0 }, V::Wait(0.6)] },
        ],
        // (researched, Bleach wiki) Fragor, after fusing with the Hogyoku: a small energy ball that erupts into a towering
        // columnar explosion where it lands
        ultimate: Move { name: "FRAGOR", range: (4.0, 35.0), share: 0.3, steps: &[V::Face, V::Anim(A_PALM), V::Mark, V::Warn(Tell::AtTarget { r: 8.0 }, 1.4), V::Fire { b: B_ORB_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.6), V::Fire { b: B_EXPLOSION, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "pain", name: "Pain", series: "Naruto", rivals: &["naruto"],
        armor: [180000, 180100, 180200, 180300], weapons: [110000, 110000], color: [1.0, 0.55, 0.2, 1.0],
        specials: &[
            Move { name: "SHINRA TENSEI", range: (0.0, 7.0), share: 0.18, steps: &[V::Anim(A_STOMP), V::Warn(Tell::AtSelf { r: 7.0 }, 0.7), V::Fire { b: B_PUSH, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
            Move { name: "BANSHO TEN'IN", range: (6.0, 22.0), share: 0.16, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 22.0, w: 1.5 }, 0.6), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 18.0, secs: 0.6 }, V::Anim(A_HEAVY), V::Wait(0.7)] },
            Move { name: "CHAKRA RODS", range: (4.0, 25.0), share: 0.14, steps: &[V::Face, V::Anim(A_KICK), V::Warn(Tell::Lane { len: 25.0, w: 2.5 }, 0.6), V::Fire { b: B_SPEAR, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.12, spread: 0.12 }, V::Wait(0.6)] },
        ],
        ultimate: Move { name: "CHIBAKU TENSEI", range: (0.0, 30.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 8.0 }, 1.5), V::Fire { b: B_ORB_PULL, from: From::Target(6.0), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0), V::Fire { b: B_METEORS, from: From::Target(14.0), aim: Aim::Down, n: 4, every: 0.25, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "dante", name: "Dante Zogratis", series: "Black Clover", rivals: &["asta"],
        armor: [970000, 970100, 970200, 970300], weapons: [110000, 110000], color: [0.75, 0.2, 0.35, 1.0],
        specials: &[
            // Body Magic is his passive life steal (LIFE_STEAL); this is its reach: flesh tendrils drag you in
            Move { name: "FLESH TENDRILS", range: (3.0, 14.0), share: 0.14, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 14.0, w: 2.0 }, 0.6), V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 12.0, secs: 0.5 }, V::Anim(A_HEAVY), V::Wait(0.8)] },
            Move { name: "FLESH SPEARS", range: (3.0, 18.0), share: 0.18, steps: &[V::Face, V::Anim(A_STOMP), V::Mark, V::Warn(Tell::Lane { len: 18.0, w: 2.5 }, 0.8), V::Fire { b: B_SPIKES, from: From::Feet, aim: Aim::Line { step: 2.2 }, n: 8, every: 0.06, spread: 0.0 }, V::Wait(0.6)] },
            Move { name: "GRAVITY SINGULARITY", range: (4.0, 20.0), share: 0.18, steps: &[V::Face, V::Anim(A_PALM), V::Mark, V::Warn(Tell::AtTarget { r: 5.0 }, 0.9), V::Fire { b: B_ORB_PULL, from: From::Target(1.5), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.9)] },
        ],
        ultimate: Move { name: "BLACK HOLE SLAM", range: (0.0, 20.0), share: 0.3, steps: &[V::Face, V::Fire { b: B_PULL, from: From::Hand, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Pull { speed: 14.0, secs: 0.7 }, V::Warn(Tell::AtSelf { r: 8.0 }, 0.6), V::Anim(A_SMASH), V::Fire { b: B_QUAKE, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
    Villain {
        id: "shigaraki", name: "Tomura Shigaraki", series: "My Hero Academia", rivals: &["deku", "all_might", "bakugo", "todoroki"],
        armor: [880000, 880100, 880200, 880300], weapons: [110000, 110000], color: [0.6, 0.75, 0.95, 1.0],
        specials: &[
            // (2.0.3, the user: decay must show itself coming) his hand to the ground, the ground cracking open in a run to where
            // you stand, a breath, then the decay along it, the ground crumbling where it lands
            Move { name: "DECAY", range: (3.0, 20.0), share: 0.2, steps: &[V::Face, V::Anim(A_STOMP), V::Mark, V::Fire { b: B_CRACK, from: From::Feet, aim: Aim::Line { step: 2.4 }, n: 8, every: 0.07, spread: 0.0 }, V::Warn(Tell::Lane { len: 20.0, w: 3.0 }, 0.6), V::Fire { b: B_CRUMBLE, from: From::Feet, aim: Aim::Line { step: 2.4 }, n: 8, every: 0.0, spread: 0.0 }, V::Fire { b: B_ROT, from: From::Feet, aim: Aim::Line { step: 2.4 }, n: 8, every: 0.08, spread: 0.0 }, V::Wait(0.6)] },
            // (the dash's lane cracks first, the ground crumbles under his touch)
            Move { name: "FIVE-FINGER TOUCH", range: (3.0, 12.0), share: 0.16, steps: &[V::Face, V::Mark, V::Fire { b: B_CRACK, from: From::Feet, aim: Aim::Line { step: 2.2 }, n: 4, every: 0.07, spread: 0.0 }, V::Warn(Tell::Lane { len: 10.0, w: 2.0 }, 0.6), V::Anim(A_DASH), V::Dash { dist: 9.0, secs: 0.3 }, V::Anim(A_HEAVY), V::Fire { b: B_CRUMBLE, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_ROT, from: From::Hand, aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.6)] },
            // (the ground round him splits first)
            Move { name: "CATASTROPHE", range: (0.0, 8.0), share: 0.18, steps: &[V::Anim(A_STOMP), V::Fire { b: B_CRACK, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Warn(Tell::AtSelf { r: 8.0 }, 0.8), V::Fire { b: B_CRUMBLE, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_ROT, from: From::Feet, aim: Aim::Outward, n: 8, every: 0.03, spread: 0.785 }, V::Wait(0.8)] },
        ],
        // (the ground cracks open toward you twice over, then crumbles round him as the decay spreads)
        ultimate: Move { name: "COLLAPSE", range: (0.0, 25.0), share: 0.3, steps: &[V::Anim(A_STOMP), V::Mark, V::Fire { b: B_CRACK, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_CRACK, from: From::Feet, aim: Aim::Line { step: 2.6 }, n: 7, every: 0.07, spread: 0.0 }, V::Warn(Tell::AtSelf { r: 16.0 }, 1.0), V::Fire { b: B_CRUMBLE, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_ROT, from: From::Feet, aim: Aim::Outward, n: 8, every: 0.0, spread: 0.785 }, V::Wait(0.4), V::Fire { b: B_ROT, from: From::Feet, aim: Aim::Outward, n: 12, every: 0.0, spread: 0.52 }, V::Wait(1.0)] },
    },
    Villain {
        id: "sukuna", name: "Ryomen Sukuna", series: "Jujutsu Kaisen", rivals: &[],
        armor: [880000, 880100, 880200, 880300], weapons: [110000, 110000], color: [0.95, 0.25, 0.3, 1.0],
        specials: &[
            // Dismantle: invisible slashes that cut everything in front of him
            Move { name: "DISMANTLE", range: (3.0, 22.0), share: 0.18, steps: &[V::Face, V::Anim(A_KICK), V::Warn(Tell::Lane { len: 22.0, w: 4.0 }, 0.7), V::Fire { b: B_SWORD_BEAM, from: From::Hand, aim: Aim::AtTarget, n: 3, every: 0.08, spread: 0.25 }, V::Wait(0.6)] },
            // Cleave: one cut that adapts to its target, up close
            Move { name: "CLEAVE", range: (0.0, 6.0), share: 0.18, steps: &[V::Face, V::Anim(A_HEAVY), V::Warn(Tell::AtSelf { r: 4.0 }, 0.6), V::Fire { b: B_SLAM_WAVE, from: From::Feet, aim: Aim::AtTarget, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.7)] },
            // Fuga (Open): the flame arrow
            Move { name: "FUGA", range: (6.0, 30.0), share: 0.2, steps: &[V::Face, V::Anim(A_PALM), V::Warn(Tell::Lane { len: 30.0, w: 2.5 }, 1.0), V::Fire { b: B_FIRE, from: From::Hand, aim: Aim::AtTarget, n: 4, every: 0.06, spread: 0.0 }, V::Wait(0.7)] },
        ],
        // Domain Expansion: Malevolent Shrine, slashes everywhere around you
        ultimate: Move { name: "MALEVOLENT SHRINE", range: (0.0, 30.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Mark, V::Warn(Tell::AtTarget { r: 8.0 }, 1.5), V::Fire { b: B_SWORD_DOWN, from: From::Target(8.0), aim: Aim::Inward { r: 4.0 }, n: 10, every: 0.12, spread: 0.0 }, V::Wait(1.0)] },
    },
    // 1.3.1 (the user's picture and wish: Muscular from My Hero Academia in his Muscle Augmentation form, the fist moveset,
    // "he doesnt move fast or control any magic, he just punches really hard"; his model and his Muscle Fiber Armor form from the
    // user's download). Researched (MHA wiki, the Forest Training Camp fight): Muscle Augmentation pushes muscle fibres out
    // through his skin for overwhelming strength and an armour of muscle; he fights with crushing punches that crater the ground,
    // grabs and squeezes his opponent (Deku), and at full Bulk Up the fibres cover his whole body
    Villain {
        id: "muscular", name: "Muscular", series: "My Hero Academia", rivals: &["deku", "all_might", "bakugo"],
        armor: [230000, 230100, 230200, 230300], weapons: [110000, 110000], color: [0.86, 0.32, 0.3, 1.0],
        specials: &[
            // his punch: the blow's force tears the ground ahead of him (the craters of the camp fight)
            Move { name: "MUSCLE PUNCH", range: (2.0, 10.0), share: 0.16, steps: &[V::Face, V::Warn(Tell::Lane { len: 10.0, w: 3.0 }, 0.8), V::Anim(A_SMASH), V::Fire { b: B_DUST_STOMP, from: From::Feet, aim: Aim::Line { step: 2.4 }, n: 4, every: 0.08, spread: 0.0 }, V::Wait(0.7)] },
            // both fists down into the ground: the shock rings out around him
            Move { name: "CRATER SLAM", range: (0.0, 7.0), share: 0.16, steps: &[V::Anim(A_STOMP), V::Warn(Tell::AtSelf { r: 7.0 }, 0.9), V::Fire { b: B_DUST_BLAST, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.35), V::Fire { b: B_DUST_STOMP, from: From::Feet, aim: Aim::Outward, n: 1, every: 0.0, spread: 0.0 }, V::Wait(0.8)] },
        ],
        // Bulk Up at full power: the roar, then he leaps and crashes down on you with everything
        ultimate: Move { name: "BULK UP: FULL POWER", range: (0.0, 20.0), share: 0.3, steps: &[V::Anim(A_RAISE), V::Face, V::Mark, V::Warn(Tell::AtTarget { r: 6.0 }, 1.4), V::Anim(A_LEAP), V::Wait(0.6), V::Fire { b: B_DUST_BLAST, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Fire { b: B_DUST_STOMP, from: From::Target(0.2), aim: Aim::Down, n: 1, every: 0.0, spread: 0.0 }, V::Wait(1.0)] },
    },
];

/// Every bullet a villain fires (their damage is set from the player's health).
/// lab: every villain bullet's attack rows, in the enemy table and the player table (which one a human-bodied villain's hits read)
#[cfg(feature = "lab")]
pub fn probe_atk() {
    use eldenring::cs::{AtkParam_Npc, AtkParam_Pc, Bullet};
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return };
    for (b, share) in all_bullets() {
        let mut queue = vec![b];
        let mut seen = 0;
        while let Some(id) = queue.pop() {
            if id <= 0 || seen > 6 {
                continue;
            }
            seen += 1;
            let Some(row) = repo.get::<Bullet>(id as u32) else { log!("atkprobe: bullet {id} missing"); continue };
            let atk = row.atk_id_bullet();
            queue.push(row.hit_bullet_id());
            queue.push(row.interval_create_bullet_id());
            let npc = repo.get::<AtkParam_Npc>(atk as u32).map(|a| (a.atk_phys(), a.atk_mag(), a.atk_fire(), a.atk_thun(), a.atk_dark()));
            let pc = repo.get::<AtkParam_Pc>(atk as u32).map(|a| (a.atk_phys(), a.atk_mag(), a.atk_fire(), a.atk_thun(), a.atk_dark()));
            log!("atkprobe: villain bullet {b} (share {share:.2}) -> bullet {id} atk {atk}: npc {npc:?} pc {pc:?}");
        }
    }
}

/// 1.3.0 (lab meter, 2026-10-17: no villain hit ever took any of the player's health): a villain has the player's human
/// body (c0000), and a c0000 character's hits read the player table (AtkParam_Pc), where the boss spells villains cast had
/// no rows. The param pack copies their enemy rows there under the same ids (tools_scripts/ccw/strip_fx.json
/// "villain_pc_atk"; only ids no player attack uses), and these are scaled like the enemy rows.
const VILLAIN_PC_ATK: &[i32] = &[3810190, 4620220, 3600400, 2200294, 4730342, 4730348, 4730346, 4520325, 2190520, 4350680, 4350681,
    4350682, 4350683, 4350684, 2190292, 3600200, 4510910, 4730224, 3050905, 3600440, 2500210, 4350670, 3600220, 3600372,
    // (1.3.1: Muscular's dust: the Erdtree Avatar's stomp, the Draconic Tree Sentinel's slam)
    4810250, 3250141];

/// `bullet`'s (and its children's) player-table copies set to deal `total` together, like bosses::scale_npc_damage
fn scale_pc_damage(orig: &mut HashMap<i32, [u16; 5]>, bullet: i32, total: f32) -> usize {
    use eldenring::cs::{AtkParam_Pc, Bullet};
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return 0 };
    let mut queue = vec![bullet];
    let (mut done, mut written) = (0, 0);
    while let Some(id) = queue.pop() {
        if id <= 0 || done > 6 {
            continue;
        }
        done += 1;
        let Some(b) = repo.get::<Bullet>(id as u32) else { continue };
        let (atk, hit, interval) = (b.atk_id_bullet(), b.hit_bullet_id(), b.interval_create_bullet_id());
        queue.push(hit);
        queue.push(interval);
        if !VILLAIN_PC_ATK.contains(&atk) {
            continue;
        }
        let Some(a) = repo.get_mut::<AtkParam_Pc>(atk as u32) else { continue };
        let o = *orig.entry(atk).or_insert([a.atk_phys(), a.atk_mag(), a.atk_fire(), a.atk_thun(), a.atk_dark()]);
        let sum: f32 = o.iter().map(|v| *v as f32).sum();
        if sum <= 0.0 {
            continue;
        }
        let k = total / sum;
        let v = |x: u16| ((x as f32 * k).round() as u32).min(9999) as u16;
        a.set_atk_phys(v(o[0]));
        a.set_atk_mag(v(o[1]));
        a.set_atk_fire(v(o[2]));
        a.set_atk_thun(v(o[3]));
        a.set_atk_dark(v(o[4]));
        written += 1;
    }
    written
}

fn all_bullets() -> Vec<(i32, f32)> {
    let mut v: Vec<(i32, f32)> = Vec::new();
    // (1.3.0: and every form's own specials: Pain's paths)
    let moves = VILLAINS.iter().flat_map(|vil| vil.specials.iter().chain(std::iter::once(&vil.ultimate))).chain(FORMS.iter().flat_map(|f| f.specials.iter()));
    {
        for m in moves {
            let shots: u32 = m.steps.iter().map(|s| if let V::Fire { b, n, .. } = s { if look_only(*b) { 0 } else { (*n).max(1) } } else { 0 }).sum();
            for s in m.steps {
                if let V::Fire { b, .. } = s {
                    if look_only(*b) {
                        continue;
                    }
                    // a move's share split over its shots (at most MOVE_CAP); the same spell in two moves keeps the larger
                    let each = m.share.min(MOVE_CAP) / shots.max(1) as f32;
                    match v.iter_mut().find(|(x, _)| x == b) {
                        Some(e) => e.1 = e.1.max(each),
                        None => v.push((*b, each)),
                    }
                }
            }
        }
    }
    v
}

/// One special being performed.
struct Run {
    villain: usize,
    mv: &'static Move,
    i: usize,
    t0: Instant,
    shots: u32,
    mark: V3,
    /// a warning that runs (until), its seal following the player until LOCK_BEFORE
    warn_until: Option<Instant>,
    dash_from: Option<V3>,
}

struct Live {
    villain: usize,
    /// the arena slot (entity id and flag) it comes from
    slot: u32,
    handle: Option<FieldInsHandle>,
    asked: Instant,
    next_move: Instant,
    run: Option<Run>,
    ult_used: bool,
    max_hp: i32,
    announced_down: bool,
    /// its health in this fight (max_hp at the start): a human character's own maximum comes from its stats and the
    /// game keeps resetting it, so the character's real bar stands for this pool (damage scaled to match)
    pool: f32,
    last_hp: i32,
    /// 1.3.0: the form it is in now (FORMS index; None: its own), its own arena slot, and whether it changes path now
    form: Option<usize>,
    home: u32,
    switch_due: bool,
    /// 1.3.1: when it last changed form (FORM_HOLD)
    form_at: Instant,
    /// its stance and poise logged
    stance_said: bool,
    /// its stance and poise last frame (their losses are cut)
    stance_prev: Option<f32>,
    poise_prev: Option<f32>,
    /// the pool a second form takes over (adopt keeps it instead of a full bar)
    keep_pool: Option<f32>,
    /// 1.3.0: its special's body, asked of its behaviour script a frame after what it was doing is broken off
    /// (stands::npc_cue; asked directly an attack's animation only interrupts a human character), and its marker state
    cue_pending: Option<(i32, Instant)>,
    cue_flip: bool,
    /// 1.3.0: its own weapon in hand until then (HELD)
    held_until: Option<Instant>,
    /// 1.3.0: where it last stood on the ground near the player's height (FALL_RESCUE)
    ground: Option<V3>,
    /// 1.3.0: where it came down (its arena spot): where it goes back to when it is lost
    home_at: Option<V3>,
    /// 1.3.0: its own close-range blows (melee): the next chance, and the chain under way (blows given, the last one's time)
    next_melee: Instant,
    chain: Option<(u8, Instant)>,
    /// a lunge in to strike: from, direction, distance, when it began
    lunge: Option<(V3, V3, f32, Instant)>,
    /// when it last played an attack of any kind (its own fighting's or ours)
    swung: Option<Instant>,
    /// 1.3.0: it last fired one of its own shots then (a hit with no attacker just after is its own shot's)
    own_shot: Option<Instant>,
    /// 1.3.0: its entrance while it lasts (arrive), and its health bar's pale lag
    arrival: Option<Arrival>,
    bar_lag: f32,
    /// 2.0.2: a changed form's body kept out of sight until then, while its new pieces load (it was made bare)
    reveal_at: Option<Instant>,
}

#[derive(Default)]
pub struct Villains {
    live: Vec<Live>,
    /// boss -> how many villains it has called
    called: HashMap<FieldInsHandle, u8>,
    /// 1.3.0: the fight's second phase began then; the boss's lowest health share so far (Malenia's refill)
    phase2_at: Option<Instant>,
    lowest: Option<f32>,
    no_boss_since: Option<Instant>,
    /// when the slots were last checked for a flag left on
    idle_check: Option<Instant>,
    orig_atk: HashMap<i32, [u16; 5]>,
    /// the player-table copies' first values (VILLAIN_PC_ATK)
    orig_pc_atk: HashMap<i32, [u16; 5]>,
    damage_for_hp: i32,
    /// lab: summon this villain now
    pub force: Option<usize>,
    pull_until: Option<(Instant, f32, FieldInsHandle)>,
    /// the player's health last frame (Dante's life steal)
    player_hp: i32,
    /// 1.3.0 (lab meter: Patry's Bright Judgment Whip took 99.9% of the bar, others 40-54%: a lingering spell hits again
    /// and again): while a special runs and a moment after, the player's health is held at or above this floor (the bar
    /// when it began, less the move's share), like the bosses' arts (bosses.rs ART_CAP)
    special_guard: Option<(i32, Instant)>,
    /// the fist row's attack as last written (0: not yet)
    fist_attack: u16,
    /// 1.3.0: a boss bar was up last frame (its boss falling first: STILL STANDS), and whether that was said this fight
    boss_was: bool,
    still_said: bool,
}

/// 1.3.0: when a villain last fired or swung (its shots are the boss's and its fists' bursts leave no attacker on what they hit)
static SHOT_AT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

/// 1.3.0 (the user: "the villains still do damage to the bosses"): whether a boss's loss just now was a villain's doing: a
/// villain's blow, or no attacker just after a villain's shot while none of the player's moves is being counted and the
/// player isn't close enough to have struck it
pub fn hurt_by_villain(by: FieldInsHandle, player_far: bool) -> bool {
    if find(by).is_some_and(|c| is_villain_npc(c.npc_param_id)) {
        return true;
    }
    let shot = SHOT_AT.lock().unwrap().is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
    shot && player_far && find(by).is_none() && !crate::ledger::PLAYER_ATTACKING.load(std::sync::atomic::Ordering::Relaxed)
}

fn find(h: FieldInsHandle) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance_mut() }.ok()?;
    wcm.chr_ins_by_handle_mut(&h).map(|c| unsafe { &mut *(c as *mut ChrIns) })
}

/// The creator's init data: rotation (identity) and scale (1) set before a spawn. The bindings' helper leaves them
/// as they are, and at zero scale the character exists, walks and fights but has no visible body. Layout:
/// position at +0x00, rotation +0x10, unknown +0x20, scale +0x30 (four f32 each).
pub fn prepare_creator(wcm: &mut WorldChrMan) {
    let base = &mut wcm.debug_chr_creator.init_data as *mut _ as *mut f32;
    unsafe {
        for (i, v) in [0.0f32, 0.0, 0.0, 1.0].iter().enumerate() {
            *base.add(4 + i) = *v;
        }
        for (i, v) in [1.0f32, 1.0, 1.0, 1.0].iter().enumerate() {
            *base.add(12 + i) = *v;
        }
    }
}

/// A character the debug creator made isn't in any map render group, so the game doesn't draw it: in, render
/// on, fully opaque (every frame: the game may clear them).
pub fn make_visible(c: &mut ChrIns) {
    // (a map's characters carry draw groups from the map file; one the creator made has none, so the game's
    // per-frame draw-group check never passes: every group)
    // (all three of its masks: the map's draw, display and collision groups; the second and third follow the
    // first in ChrIns, not exposed by the bindings)
    unsafe { std::ptr::write_bytes(c.render_group_mask.as_mut_ptr(), 0xFF, 0x60) };
    c.chr_flags1c4.set_force_update(true);
    c.chr_flags1c4.set_is_render_group_enabled(true);
    c.chr_flags1c5.set_enable_render(true);
    c.base_transparency = 1.0;
    c.camouflage_transparency = 1.0;
}

fn pos_of(c: &ChrIns) -> V3 {
    let p = c.modules.physics.position;
    (p.0, p.1, p.2)
}

fn face(c: &mut ChrIns, dir: V3) {
    let l = (dir.0 * dir.0 + dir.2 * dir.2).sqrt();
    if l < 1e-3 {
        return;
    }
    let th = (-dir.0 / l).atan2(-dir.2 / l);
    let phys = &mut c.modules.physics;
    phys.orientation = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
    phys.interpolated_orientation = phys.orientation;
}


// ---- 1.3.0 (the user: "a really epic animation that plays when the villains spawn in during the boss fights ... an 'Oh shit'
// moment ... a big indicator of where the villain spawned and that there is a new threat on the battlefield"; then: "enter the
// game through starscourge radahn's meteor impact where he comes in from the sky. This entrance should do no damage to anyone"):
// each villain's entrance. Hidden and still where it will stand, a column of light over the spot with its name
// (world_fx::threat), Radahn's meteor streaks down out of the sky there, his gravity burst and shockwave, and it stands in the
// crater in a gesture of its own; it fights a moment later.

/// Radahn's own meteor crash (the user's reference: his phase-two entrance, "this entrance should do no damage to anyone";
/// then: "I want to be able to see the meteor crash onto the ground and see the villain emerge from the rubble in the
/// crater"): his effects copied into the common binder (radahn_fx.py; a character's effects otherwise exist only in its own
/// fight), carried by look-only bullets (strip_fx.json, no attack rows): his meteor at his size (9990795, a 6x copy of his
/// effect, 40 m/s, no homing) falling in at an angle from high up beyond the spot, its purple streak as it arrives, then all at once the gravity dome, the shockwave and
/// flying rock, the fire blast and the dust (9990787..92, held still); the villain is crouched in the crater as the dust
/// clears, and rises.
const V_METEOR: i32 = 9990795; // (6x: Radahn's size; lab dropt)
const V_STREAK: i32 = 9990787;
const V_STREAK2: i32 = 9990788;
const V_DOME: i32 = 9990789;
const V_WAVE: i32 = 9990790;
const V_FIRE: i32 = 9990791;
const V_DUST: i32 = 9990792;
/// the meteor's drop: from this high above the spot and this far beyond it (seen from the player: it comes in at an angle,
/// toward them, as Radahn's does), at its speed (its row's)
const METEOR_HIGH: f32 = 55.0;
const METEOR_BACK: f32 = 25.0;
const METEOR_SPEED: f32 = 40.0;

#[derive(Clone, Copy)]
enum Ex {
    /// falling from METEOR_HIGH above and METEOR_BACK beyond the spot (seen from the player) onto it
    Drop(i32),
    /// at the spot, this high
    At(i32, f32),
}

/// the meteor lands then (s)
const METEOR_LANDS: f32 = 1.51; // sqrt(55^2 + 25^2) / 40
/// it is in the crater then (inside the dust), crouched
const REVEAL: f32 = METEOR_LANDS + 0.15;
/// it rises then, and fights ENTRANCE_HOLD after
const RISE: f32 = REVEAL + 1.0;
const ENTRANCE_HOLD: f32 = 1.6;
/// the crouch it waits in (Elden Ring's squatting gesture), asked while it is still hidden so it is down when it shows
const A_CROUCH: i32 = 80940;
/// every villain's arrival
const ARRIVAL: &[(f32, Ex)] = &[
    (0.0, Ex::Drop(V_METEOR)),
    (METEOR_LANDS - 0.3, Ex::At(V_STREAK, 0.0)),
    (METEOR_LANDS - 0.15, Ex::At(V_STREAK2, 0.0)),
    (METEOR_LANDS, Ex::At(V_DOME, 0.0)),
    (METEOR_LANDS, Ex::At(V_WAVE, 0.0)),
    (METEOR_LANDS, Ex::At(V_FIRE, 0.0)),
    (METEOR_LANDS, Ex::At(V_DUST, 0.0)),
];

/// its gesture once it stands there (an event animation a human character plays when asked)
fn gesture_of(id: &str) -> i32 {
    match id {
        "frieza" | "sukuna" | "dante" | "all_for_one" => 80230, // Beckon
        "hisoka" => 80010,                                        // a polite bow, the magician's
        "madara" | "shigaraki" => 80200,                          // Point Forwards
        "pain" => 80220,                                          // Point Downwards: looking down on you
        "patry" => 80210,                                         // Point Upwards
        "doflamingo" => 80070,                                    // Wave
        "meruem" | "aizen" => 80300,                              // Nod In Thought
        _ => 80090,                                               // Strength!
    }
}

/// an entrance under way: when it began, the spot, the next step, and whether it has appeared
struct Arrival {
    t0: Instant,
    at: V3,
    next: usize,
    revealed: bool,
    crouched: bool,
    risen: bool,
}

/// One frame of an entrance. False while it lasts (the villain hidden, then still); true once it may fight.
fn arrive(c: &mut ChrIns, vil: &Villain, a: &mut Arrival, owner: FieldInsHandle, slot: u32, me: V3, now: Instant) -> bool {
    let el = now.duration_since(a.t0).as_secs_f32();
    while a.next < ARRIVAL.len() && el >= ARRIVAL[a.next].0 {
        let (p, d, b) = match ARRIVAL[a.next].1 {
            Ex::Drop(b) => {
                let (away, _) = flat_dir(me, a.at);
                let from = (a.at.0 + away.0 * METEOR_BACK, a.at.1 + METEOR_HIGH, a.at.2 + away.2 * METEOR_BACK);
                let v = (a.at.0 - from.0, a.at.1 - from.1, a.at.2 - from.2);
                let l = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt().max(0.01);
                (from, (v.0 / l, v.1 / l, v.2 / l), b)
            }
            Ex::At(b, hgt) => ((a.at.0, a.at.1 + hgt, a.at.2), (0.0, 1.0, 0.0), b),
        };
        if let Err(err) = ability::spawn(owner, -1, b, p, d) {
            log!("villains: {} entrance bullet {b} failed ({err})", vil.name);
        }
        a.next += 1;
    }
    // (lab: hidden, with no collision, it sank into the hill; it rose out of sight) held on its spot until it stands up
    if !a.risen {
        let p = &mut c.modules.physics;
        if (p.position.1 - a.at.1).abs() > 0.05 || (p.position.0 - a.at.0).abs() > 0.05 || (p.position.2 - a.at.2).abs() > 0.05 {
            p.position.0 = a.at.0;
            p.position.1 = a.at.1;
            p.position.2 = a.at.2;
            p.chr_proxy_pos_update_requested = true;
        }
    }
    // crouched while still hidden, so it is down in the crater when the dust clears
    if !a.crouched && el > 0.2 {
        a.crouched = true;
        c.modules.event.request_animation_id = A_CROUCH;
    }
    if !a.revealed {
        if el < REVEAL {
            return false;
        }
        a.revealed = true;
        set_present(slot, true);
        make_visible(c);
        c.debug_flags.set_disabled_movement(true);
        c.debug_flags.set_disabled_secondary_actions(true);
        log!("villains: {} lands (y {:.2}, its spot {:.2}, on the ground {})", vil.name, c.modules.physics.position.1, a.at.1, c.modules.physics.is_touching_ground);
        return false;
    }
    if !a.risen {
        if el < RISE {
            return false;
        }
        a.risen = true;
        c.modules.event.request_animation_id = gesture_of(vil.id);
        log!("villains: {} rises (y {:.2}, its spot {:.2}, on the ground {})", vil.name, c.modules.physics.position.1, a.at.1, c.modules.physics.is_touching_ground);
        world_fx::banner(format!("{} JOINS THE FIGHT", vil.name.to_uppercase()), vil.color, 2.6);
        return false;
    }
    if el < RISE + ENTRANCE_HOLD {
        return false;
    }
    c.debug_flags.set_disabled_movement(false);
    c.debug_flags.set_disabled_secondary_actions(false);
    true
}

/// 1.3.0 (the user: "I also want them to have their own boss health bars"): each villain in the fight, for the overlay's
/// boss bars (overlay.rs): its name, its health (share of the fight's pool), the pale lag of what it just lost, colour
pub static BARS: std::sync::Mutex<Vec<(String, f32, f32, [f32; 4])>> = std::sync::Mutex::new(Vec::new());

fn flat_dir(from: V3, to: V3) -> (V3, f32) {
    let (dx, dz) = (to.0 - from.0, to.2 - from.2);
    let d = (dx * dx + dz * dz).sqrt();
    if d < 1e-3 { ((0.0, 0.0, 1.0), 0.0) } else { ((dx / d, 0.0, dz / d), d) }
}

/// The villains a fighter's boss fights call (Your Tarnished: any two).
fn rivals_of(fighter: &str) -> Vec<usize> {
    let mut v: Vec<usize> = (0..VILLAINS.len()).filter(|&i| VILLAINS[i].rivals.contains(&fighter)).collect();
    if v.is_empty() {
        let mut rng = rand::rng();
        let a = rng.random_range(0..VILLAINS.len());
        let b = (a + 1 + rng.random_range(0..VILLAINS.len() - 1)) % VILLAINS.len();
        v = vec![a, b];
    }
    v
}

impl Villains {
    /// The arena's villain comes: its slot's flag on (the map's event shows the character); the character is taken
    /// over (health, team, placement) once it is there.
    fn summon(&mut self, arena: &Arena, hp: i32) {
        let Some(i) = VILLAINS.iter().position(|v| v.id == arena.villain) else {
            log!("villains: {} has no moveset yet (arena {})", arena.villain, arena.map);
            return;
        };
        set_flag(arena.slot, true);
        let now = Instant::now();
        self.live.push(Live { villain: i, slot: arena.slot, handle: None, asked: now, next_move: now + Duration::from_secs_f32(3.5), run: None, ult_used: false, max_hp: hp, announced_down: false, pool: hp as f32, last_hp: 0, form: None, home: arena.slot, switch_due: false, form_at: now, keep_pool: None, stance_said: false, stance_prev: None, poise_prev: None, cue_pending: None, cue_flip: false, held_until: None, ground: None, home_at: None, next_melee: now, chain: None, lunge: None, swung: None, own_shot: None, arrival: None, bar_lag: 1.0, reveal_at: None });
        log!("villains: {} summoned ({} HP, slot {} in {})", VILLAINS[i].name, hp, arena.slot, arena.map);
    }

    /// The slot's character, once the map's event has shown it: hostile, with this fight's health, brought in
    /// beside the player (to the side, away from the boss).
    fn adopt(player: &PlayerIns, l: &mut Live, boss: Option<V3>) {
        let Some(c) = by_entity(l.slot) else { return };
        let vil = &VILLAINS[l.villain];
        // (2.0.2) made bare: dressed in its own look now, while it is still out of sight
        if own_look(l.villain) {
            dress(c, look_rows(l.villain, l.form));
        }
        // (its first arrival in the fight, not a change of form or path)
        let first = l.form.is_none() && l.keep_pool.is_none() && !l.switch_due;
        c.team_type = HOSTILE_TEAM;
        l.pool = l.keep_pool.take().unwrap_or(l.max_hp.max(300) as f32);
        // (2.0.2) its health straight at the pool's share: a new form's body set full and then lowered to the pool read, to a
        // move's damage count (ledger.rs), as a blow far past the move's damage, and the "overkill" given back healed the pool
        let full = l.max_hp.max(300) as f32;
        let d = &mut c.modules.data;
        d.hp = ((l.pool / full * d.max_hp as f32).ceil() as i32).clamp(2, d.max_hp.max(2));
        l.last_hp = d.hp;
        // it enters from its own spot beside the boss's start (the map file's: valid ground in the arena). (lab: moving
        // it beside the player killed open-world villains at once: they belong to the large map tile, whose frame
        // isn't the player's, and the game took them for out of bounds)
        let me = ability::position(player);
        let at = pos_of(c);
        let _ = boss;
        face(c, flat_dir(at, me).0);
        l.handle = Some(c.field_ins_handle);
        if first {
            // its entrance (arrive): hidden until Radahn's meteor has come down on the spot
            set_present(l.slot, true);
            set_present(l.slot, false);
            l.home_at = Some(at);
            l.arrival = Some(Arrival { t0: Instant::now(), at, next: 0, revealed: false, crouched: false, risen: false });
            static STRAIGHT: std::sync::Once = std::sync::Once::new();
            STRAIGHT.call_once(|| { crate::bosses::no_homing(V_METEOR); });
            l.bar_lag = 1.0;
            world_fx::banner("A NEW THREAT DESCENDS", vil.color, 1.8);
            log!("villains: {} is here ({:?}, {} HP) at ({:.1}, {:.1}, {:.1}): its entrance begins", vil.name, c.field_ins_handle, c.modules.data.hp, at.0, at.1, at.2);
            return;
        }
        world_fx::impact((at.0, at.1 + 1.0, at.2), vil.color, true);
        match l.form {
            Some(fi) => world_fx::banner(FORMS[fi].name.to_string(), vil.color, 2.6),
            None if (l.keep_pool.is_some() || l.switch_due) && vil.id == "pain" => world_fx::banner(format!("{}: DEVA PATH", vil.name.to_uppercase()), vil.color, 2.6),
            // (1.3.1: Muscular back out of his armour)
            None if l.keep_pool.is_some() || l.switch_due => world_fx::banner(vil.name.to_uppercase(), vil.color, 2.6),
            None => world_fx::banner(format!("{} JOINS THE FIGHT", vil.name.to_uppercase()), vil.color, 2.6),
        }
        l.switch_due = false;
        log!("villains: {} is here ({:?}, {} HP) at ({:.1}, {:.1}, {:.1})", vil.name, c.field_ins_handle, c.modules.data.hp, at.0, at.1, at.2);
    }

    /// lab: each change of a slot's flag, the frame it happens (who turns them back on)
    #[cfg(feature = "lab")]
    fn watch_flags(&mut self, at: &str) {
        static SEEN: std::sync::Mutex<Vec<(u32, bool)>> = std::sync::Mutex::new(Vec::new());
        let mut g = SEEN.lock().unwrap();
        for a in ARENAS.iter() {
            for s in slots_of(a) {
                let now = flag(s);
                match g.iter_mut().find(|e| e.0 == s) {
                    Some(e) if e.1 == now => {}
                    Some(e) => {
                        e.1 = now;
                        log!("villains: watch: slot {s} flag -> {now} ({at})");
                    }
                    None => g.push((s, now)),
                }
            }
        }
    }

    /// Every frame in the world.
    pub fn tick(&mut self, player: &mut PlayerIns, fighter: &str, difficulty: crate::ui::Difficulty, training: bool) {
        #[cfg(feature = "lab")]
        self.watch_flags("frame start");
        // Shadow of the Erdtree's maps (m20..m28, m61): the borrowed armour rows go back to their own NPCs
        let area = player.current_block_id.area();
        if area != 0 && area != 0xFF {
            lend_rows(!(20..=28).contains(&area) && area != 61);
        }
        let now = Instant::now();
        // damage follows the player's health
        let max_hp = player.chr_ins.modules.data.max_hp;
        if max_hp > 0 && max_hp != self.damage_for_hp {
            self.damage_for_hp = max_hp;
            let rows: usize = all_bullets().iter().map(|(b, share)| crate::bosses::scale_npc_damage(&mut self.orig_atk, *b, max_hp as f32 * share)).sum();
            let pc: usize = all_bullets().iter().map(|(b, share)| scale_pc_damage(&mut self.orig_pc_atk, *b, max_hp as f32 * share)).sum();
            log!("villains: special damage set for {max_hp} HP ({rows} rows, {pc} in the player table)");
        }
        // the boss fight: below its marks, the rivals come
        let mut boss: Option<(FieldInsHandle, f32, V3, i32, u32)> = None;
        if let Ok(fe) = unsafe { CSFeManImp::instance() } {
            for b in fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()) {
                if let Some(c) = find(b.field_ins_handle) {
                    let d = &c.modules.data;
                    if d.max_hp > 0 && d.hp > 0 {
                        boss = Some((b.field_ins_handle, d.hp as f32 / d.max_hp as f32, pos_of(c), d.max_hp, c.event_entity_id));
                        break;
                    }
                }
            }
        }
        let arena = boss.and_then(|b| ARENAS.iter().find(|a| a.bosses.contains(&b.4)));
        let boss_owner = boss.map(|b| b.0);
        // (1.3.0) the villains' punches follow the player's health (lab meter: 109 attack took 2.4% of 910 HP a punch; a fixed
        // attack would barely scratch a high-level fighter); the row's own attack back when no villain fights (other
        // enemies may hold it)
        {
            let want = if self.live.is_empty() { FIST_ATTACK as u16 } else { (FIST_ATTACK * max_hp as f32 / FIST_ATTACK_HP).round().clamp(20.0, 3000.0) as u16 };
            if want != self.fist_attack {
                if let Some(w) = unsafe { SoloParamRepository::instance_mut() }.ok().and_then(|r| r.get_mut::<eldenring::cs::EquipParamWeapon>(VILLAIN_FIST as u32)) {
                    w.set_attack_base_physics(want);
                    self.fist_attack = want;
                    log!("villains: fist attack {want} (player {max_hp} HP)");
                }
            }
        }
        // (1.3.0, the user: "we have to make it so villains and bosses cannot do damage to each other"; lab: some bosses are
        // on a team of their own, Maliketh's 33, hostile to the villains' 6: Cell fought his boss) a villain fights on its
        // boss's team
        let boss_team = boss.and_then(|b| find(b.0)).map(|c| c.team_type);
        let forced = self.force.take().is_some();
        if let (Some((h, frac, _, bmax, _)), Some(a)) = (boss, arena) {
            let called = *self.called.get(&h).unwrap_or(&0);
            let low = self.lowest.map_or(frac, |l| l.min(frac));
            self.lowest = Some(low);
            let began = match phase2(boss.map_or(0, |b| b.4)) {
                Phase2::One => frac < FIRST_AT,
                Phase2::Second(id) => boss.is_some_and(|b| b.4 == id),
                Phase2::Below(t) => frac < t,
                Phase2::Refill => frac > low + 0.3,
            };
            if began && self.phase2_at.is_none() {
                self.phase2_at = Some(now);
                log!("villains: the boss's phase for its villain began ({:.0}% health)", frac * 100.0);
            }
            let one_phase = matches!(phase2(boss.map_or(0, |b| b.4)), Phase2::One);
            let ready = self.phase2_at.is_some_and(|t| one_phase || now.duration_since(t).as_secs_f32() >= PHASE2_DELAY);
            // (the called count is the whole fight's: a second character's bar is a new boss handle)
            let called = called.max(if self.live.is_empty() { 0 } else { 1 });
            if called == 0 && ((ready && !training) || forced) {
                let hp = (bmax as f32 * difficulty.boss_hp() * VILLAIN_SHARE).round() as i32;
                self.summon(a, hp);
                self.called.insert(h, 1);
            }
        } else if forced {
            // (lab: no boss bar yet, e.g. warped past the fog: the nearest remembrance boss's arena)
            let me = ability::position(player);
            let near = ARENAS.iter().find(|a| a.bosses.iter().any(|b| by_entity(*b).is_some_and(|c| flat_dir(pos_of(c), me).1 < 80.0)));
            match near {
                Some(a) => self.summon(a, 3000),
                None => log!("villains: no remembrance boss nearby (the villains come only to those)"),
            }
        }
        let _ = fighter;
        // (1.3.0, the user: both must die before the fight is won) its boss fallen first, the villain fights on: the fight's
        // end (the game's banner and rewards) waits for it (alive_flag)
        let fighting = self.live.iter().any(|l| !l.announced_down);
        if !fighting {
            self.still_said = false;
        }
        if boss.is_none() && fighting && self.boss_was && !self.still_said {
            self.still_said = true;
            if let Some(l) = self.live.iter().find(|l| !l.announced_down) {
                let vil = &VILLAINS[l.villain];
                world_fx::banner(format!("{} STILL STANDS", vil.name.to_uppercase()), vil.color, 3.0);
                log!("villains: the boss fell first: {} fights on, the fight's end waits for it", vil.name);
            }
        }
        self.boss_was = boss.is_some();
        if boss.is_none() && !self.called.is_empty() && !fighting {
            // the fight is over: the villains go with it
            for l in &mut self.live {
                // (a burst of its colour where it stood, as when it changes form: it doesn't just blink out)
                if let Some(c) = by_entity(l.slot).filter(|c| c.modules.data.hp > 0) {
                    let at = pos_of(c);
                    world_fx::impact((at.0, at.1 + 1.0, at.2), VILLAINS[l.villain].color, true);
                }
                set_flag(l.slot, false);
                log!("villains: {} goes with the boss", VILLAINS[l.villain].name);
            }
            self.live.clear();
            self.called.clear();
        }
        // (kept through a phase change's cutscene, when the bar can go: Malenia's refill is seen across it)
        if boss.is_some() {
            self.no_boss_since = None;
        } else if self.no_boss_since.get_or_insert(now).elapsed() > Duration::from_secs(30) {
            self.phase2_at = None;
            self.lowest = None;
        }
        // each arena's flag for its boss's defeat event: on exactly while its villain fights (also clears one a save kept on)
        for a in ARENAS.iter() {
            let want = self.live.iter().any(|l| l.home == a.slot && !l.announced_down);
            if flag(alive_flag(a.slot)) != want {
                set_flag(alive_flag(a.slot), want);
                log!("villains: {} defeat event {}", a.map, if want { "waits for the villain" } else { "free to end" });
            }
        }
        FIGHTING.store(self.live.iter().any(|l| !l.announced_down && l.handle.is_some()), std::sync::atomic::Ordering::Relaxed);
        // no fight: every slot off (a save made mid-fight keeps its flag on, and the map would show the villain)
        if boss.is_none() && self.live.is_empty() && self.idle_check.is_none_or(|t| t.elapsed() > Duration::from_secs(2)) {
            self.idle_check = Some(now);
            all_slots_off("no fight");
        }
        // the slot's character, once the map shows it
        let bpos = boss.map(|b| b.2);
        for l in self.live.iter_mut().filter(|l| l.handle.is_none() && !l.announced_down) {
            Self::adopt(player, l, bpos);
            if l.handle.is_none() && l.asked.elapsed() > Duration::from_secs(5) {
                log!("villains: {} never appeared (slot {})", VILLAINS[l.villain].name, l.slot);
                l.announced_down = true;
            }
        }
        // Dante's Body Magic: what his hits take from the player, he takes back
        let php = player.chr_ins.modules.data.hp;
        if php < self.player_hp {
            let by = player.chr_ins.last_hit_by;
            for l in &mut self.live {
                if VILLAINS[l.villain].id == "dante" && l.handle == Some(by) && l.pool > 0.0 {
                    let gain = (self.player_hp - php) as f32 * LIFE_STEAL;
                    l.pool = (l.pool + gain).min(l.max_hp.max(300) as f32);
                    log!("villains: Dante's Body Magic takes back {gain:.0} HP");
                }
            }
        }
        // the special guard: held while any special runs, and SPECIAL_LINGER after the last one ends
        if let Some((floor, until)) = self.special_guard {
            let running = self.live.iter().any(|l| l.run.is_some());
            let until = if running { now + Duration::from_secs_f32(SPECIAL_LINGER) } else { until.min(now + Duration::from_secs_f32(SPECIAL_LINGER)) };
            if now >= until {
                self.special_guard = None;
                crate::movement::set_no_dead(crate::movement::NO_DEAD_SPECIAL, false);
            } else {
                self.special_guard = Some((floor, until));
                crate::movement::set_no_dead(crate::movement::NO_DEAD_SPECIAL, true);
                let hp = player.chr_ins.modules.data.hp;
                if hp < floor && hp > 0 && !crate::movement::dying() {
                    log!("villains: a special took the player to {hp} HP: held at {floor}");
                    player.chr_ins.modules.data.hp = floor;
                }
            }
        }
        let php = player.chr_ins.modules.data.hp;
        self.player_hp = php;
        let me = ability::position(player);
        let mut rng = rand::rng();
        // the villains' health bars (overlay.rs): those that have appeared and still stand
        {
            let mut bars = Vec::new();
            for l in &mut self.live {
                if l.handle.is_none() || l.announced_down || l.pool <= 0.0 || l.arrival.as_ref().is_some_and(|a| !a.revealed) {
                    continue;
                }
                let frac = (l.pool / l.max_hp.max(300) as f32).clamp(0.0, 1.0);
                l.bar_lag = if frac >= l.bar_lag { frac } else { l.bar_lag + (frac - l.bar_lag) * 0.035 };
                let name = match l.form { Some(fi) => FORMS[fi].name.to_string(), None => VILLAINS[l.villain].name.to_string() };
                bars.push((name, frac, l.bar_lag, VILLAINS[l.villain].color));
            }
            *BARS.lock().unwrap() = bars;
        }
        for l in &mut self.live {
            let Some(h) = l.handle else { continue };
            let vil = &VILLAINS[l.villain];
            let Some(c) = find(h) else {
                l.run = None;
                continue;
            };
            // (2.0.2) a new form's body shown once its pieces have loaded
            if let Some(t) = l.reveal_at {
                if now < t {
                    set_present(l.slot, false);
                    continue;
                }
                l.reveal_at = None;
                set_present(l.slot, true);
            }
            // (1.3.0, lab: its own fist attacks (Carian Combo Warriors' 42033xxx) hit the boss too, with no attacker on the
            // hit: while one plays, a loss with no attacker is the villain's (hurt_by_villain), as with its shots)
            {
                let ta = &c.modules.time_act;
                let an = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
                if an >= 30000 && !(60000..100000).contains(&an) {
                    *SHOT_AT.lock().unwrap() = Some(now);
                    if l.chain.is_none() {
                        l.swung = Some(now);
                    }
                }
            }
            // lab: each animation a villain plays (stagger and knockdown checks)
            #[cfg(feature = "lab")]
            {
                static LAST: std::sync::Mutex<Vec<(u32, i32)>> = std::sync::Mutex::new(Vec::new());
                let ta = &c.modules.time_act;
                let an = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
                let mut g = LAST.lock().unwrap();
                match g.iter_mut().find(|e| e.0 == l.slot) {
                    Some(e) if e.1 == an => {}
                    Some(e) => {
                        e.1 = an;
                        log!("villains: {} anim {an}", vil.name);
                    }
                    None => g.push((l.slot, an)),
                }
            }
            // (1.3.0: it has its own boss bar, overlay.rs) no floating health bar over its head, as a boss has none
            c.modules.action_flag.action_modifiers_flags.set_disable_floating_gauge_display(true);
            // 1.3.1: its height (HEIGHTS)
            if let Some((_, own, form)) = HEIGHTS.iter().find(|(id, _, _)| *id == vil.id) {
                let k = if l.form.is_some() { *form } else { *own };
                c.chr_ctrl.scale_size_x = k;
                c.chr_ctrl.scale_size_y = k;
                c.chr_ctrl.scale_size_z = k;
            }
            // its entrance first (nothing else while it lasts; on its boss's side already)
            if let Some(a) = l.arrival.as_mut() {
                if let Some(t) = boss_team {
                    c.team_type = t;
                }
                if arrive(c, vil, a, boss_owner.unwrap_or(h), l.slot, me, now) {
                    l.arrival = None;
                    l.next_move = now + Duration::from_secs_f32(1.5);
                } else {
                    continue;
                }
            }
            // (1.3.0, the user: Frieza "is using the sword moveset without being equipped with a sword": his punches never
            // landed. The game leaves a human enemy's main right-hand slot empty (-1) whatever its row says (only the spare
            // took the weapon), and an empty right hand plays straight sword attacks that hit nothing.) Its weapons, put in
            // the slots it really uses: [left 1, right 1, left 2, right 2, ...]
            #[cfg(feature = "lab")]
            let boss_team = crate::lab::VILTEAM.lock().unwrap().or(boss_team);
            if let Some(t) = boss_team {
                if c.team_type != t {
                    log!("villains: {} joins its boss's team ({} -> {t})", vil.name, c.team_type);
                    c.team_type = t;
                }
            }
            if c.character_id == 0 {
                let want = match l.form { Some(fi) => { let i = VILLAINS.iter().position(|v| v.id == FORMS[fi].villain).unwrap_or(l.villain); [fist_row(VILLAINS[i].weapons[0]), fist_row(VILLAINS[i].weapons[1])] } None => [fist_row(vil.weapons[0]), fist_row(vil.weapons[1])] };
                let p = unsafe { &mut *(c as *mut ChrIns as *mut PlayerIns) };
                let pgd = unsafe { p.player_game_data.as_mut() };
                for ids in [&mut p.chr_asm.equipment_param_ids, &mut pgd.equipment.chr_asm.equipment_param_ids] {
                    if ids[1] != want[0] || ids[0] != want[1] {
                        ids[1] = want[0];
                        ids[3] = want[0];
                        ids[0] = want[1];
                        ids[2] = want[1];
                    }
                }
                // (1.3.0, the user: "every character with a sword is using sword moves"; lab: Aizen's left-hand attacks were fist
                // punches, 17 of them, and Patry's 3) a villain with a weapon holds it in both hands: every attack is the weapon's
                if want[0] != VILLAIN_FIST {
                    use eldenring::cs::ChrAsmArmStyle as A;
                    for e in [&mut p.chr_asm.equipment, &mut pgd.equipment.chr_asm.equipment] {
                        if e.arm_style != A::RightBothHands {
                            e.arm_style = A::RightBothHands;
                        }
                    }
                }
            }
            // (1.3.0, the user: villains "should have super high poise and be really difficult to stance break (except for
            // perfect parries)": their NpcParam rows carry VILLAIN_STANCE / VILLAIN_POISE, dress_rows) once, what it really has
            // (a human enemy's stance and poise come from its body and armour, recomputed every frame, not its NpcParam row:
            // lab, stance 100 / poise 0-4) so its losses are cut here: a third of each stance loss is kept, its poise held
            // full, except while a parry's slow motion runs (a perfect parry empties both: parry.rs)
            if !crate::parry::WORLD_SLOWED.load(std::sync::atomic::Ordering::Relaxed) {
                let sa = c.modules.super_armor.sa_durability;
                if let Some(prev) = l.stance_prev {
                    if sa < prev {
                        // (1.3.0, the user: "impossible to knock down unless you get a parry on them") held full
                        c.modules.super_armor.sa_durability = prev;
                        #[cfg(feature = "lab")]
                        log!("villains: {} stance hit {:.1}: {prev:.1} -> {:.1}", vil.name, prev - sa, c.modules.super_armor.sa_durability);
                    } else if sa <= 0.0 && prev > 0.0 {
                        log!("villains: {} stance broken (from {prev:.1})", vil.name);
                    }
                }
                // (lab: a human enemy has no stance break; it staggers when its poise runs out) poise VILLAIN_POISE, held full:
                // no hit staggers it, a perfect parry still does (parry.rs empties it)
                let t = &mut c.modules.toughness;
                let first = t.toughness_max < VILLAIN_POISE;
                t.toughness_max = t.toughness_max.max(VILLAIN_POISE);
                if first && l.poise_prev.is_none() {
                    t.toughness = VILLAIN_POISE;
                } else if let Some(prev) = l.poise_prev {
                    if t.toughness < prev {
                        let kept = prev;
                        #[cfg(feature = "lab")]
                        log!("villains: {} poise hit {:.1}: {prev:.1} -> {kept:.1}", vil.name, prev - t.toughness);
                        t.toughness = kept;
                    }
                }
            }
            l.stance_prev = Some(c.modules.super_armor.sa_durability);
            l.poise_prev = Some(c.modules.toughness.toughness);
            if !l.stance_said && now.duration_since(l.asked).as_secs_f32() > 6.0 {
                l.stance_said = true;
                log!("villains: {} stance {:.0}/{:.0}, poise {:.0}/{:.0}", vil.name, c.modules.super_armor.sa_durability, c.modules.super_armor.sa_durability_max,
                    c.modules.toughness.toughness, c.modules.toughness.toughness_max);
            }
            if c.modules.data.hp <= 0 {
                if !l.announced_down {
                    l.announced_down = true;
                    // (its 8 s from here: the fall plays out before its slot goes off)
                    l.asked = now;
                    world_fx::banner(format!("{} DEFEATED", vil.name.to_uppercase()), vil.color, 2.2);
                    log!("villains: {} defeated", vil.name);
                }
                l.run = None;
                continue;
            }
            #[cfg(feature = "lab")]
            if let Some(k) = *crate::lab::VILPOOL.lock().unwrap() {
                l.pool = l.max_hp.max(300) as f32 * k;
            }
            // the pool: what the bar lost, in the fight's terms (its real bar is its stat maximum). (1.3.0, lab: Hisoka took
            // 254 every time his own Bungee Gum pull flew: the fight's boss owns the villains' shots, and on the boss's team
            // (Radahn's 7) its shots and its blows hit their own side. A loss to the boss or to itself doesn't count, nor one
            // with no attacker just after its own shot (a shot the mod spawns leaves none); the bar is put back)
            {
                let by = c.last_hit_by;
                // (1.3.0, the user: Madara "keeps taking damage from his own fire attack": the villains' spells are the boss's and
                // leave no attacker, nor do the player's blows) only the player's side hurts a villain: the player, their clones
                // and Stands (the player's team), or a hit while one of the player's moves is being counted
                let theirs = crate::ledger::PLAYER_ATTACKING.load(std::sync::atomic::Ordering::Relaxed);
                let player_side = by == player.chr_ins.field_ins_handle || find(by).is_some_and(|a| a.team_type == player.chr_ins.team_type);
                // (and while its own special plays, or just after, the player's move counts only up close: an ability's
                // count runs for seconds, the villain's own fire and blasts land meanwhile)
                let own_busy = l.run.is_some() || l.own_shot.is_some_and(|t| t.elapsed() < Duration::from_millis(1500));
                let near = { let p = pos_of(c); ((p.0 - me.0).powi(2) + (p.2 - me.2).powi(2)).sqrt() < 5.0 };
                let friendly = !(player_side || (theirs && (!own_busy || near)));
                let d = &mut c.modules.data;
                let full = l.max_hp.max(300) as f32;
                let scale = full / d.max_hp.max(1) as f32;
                if d.hp < l.last_hp && friendly {
                    #[cfg(feature = "lab")]
                    log!("villains: {} lost {} to its own side: put back", vil.name, l.last_hp - d.hp);
                } else if d.hp < l.last_hp {
                    l.pool -= (l.last_hp - d.hp) as f32 * scale;
                    log!("villains: {} took {} ({:.0} of {}), {:.0} left", vil.name, l.last_hp - d.hp, (l.last_hp - d.hp) as f32 * scale, l.max_hp, l.pool.max(0.0));
                    #[cfg(feature = "lab")]
                    log!("villains: (hit by {by:?}; the boss {boss_owner:?}; the player {:?})", player.chr_ins.field_ins_handle);
                }
                // (2.0.2, the user: Muscular "full healed every time he switched forms and also he seemed to have unlimited healing
                // potions"; their log: a Detroit Smash's ledger gave 7014 HP back (an overkill trimmed to the move's listed damage)
                // and the pool took the rise for a heal, 1277 -> 5738; and the invader AI's own flasks) a rise in its health is
                // never a heal: its own heals (V::Heal, Dante's Body Magic) go to the pool itself
                // (2.0.2: as a boss's, its last real HP is the kill: at 1 under the boss fight's "can't die" switch no further hit
                // would show, and it would stand on with an empty bar)
                if l.pool <= scale {
                    l.pool = 0.0;
                }
                d.hp = if l.pool <= 0.0 { 0 } else { ((l.pool / scale).ceil() as i32).clamp(2, d.max_hp.max(2)) };
                l.last_hp = d.hp;
            }
            if c.modules.data.hp <= 0 {
                continue;
            }
            // (1.3.0, lab: Hisoka put up a slope in Radahn's fight fell through it and fought on from 1 km under the map) a
            // villain this far below the player and where it last stood is out of the world: back on that ground (or by the
            // player)
            {
                let vp = pos_of(c);
                // (1.3.0, the user: Madara "died from fall damage ... because I was swinging as deku") no fall hurts a villain:
                // its fall's clock held at nothing while it is off the ground
                if !c.modules.physics.is_touching_ground {
                    c.modules.fall.fall_timer = 0.0;
                }
                if c.modules.physics.is_touching_ground && (vp.1 - me.1).abs() < 15.0 {
                    l.ground = Some(vp);
                }
                if vp.1 < me.1 - FALL_RESCUE && l.ground.is_none_or(|g| vp.1 < g.1 - FALL_RESCUE) {
                    let d = flat_dir(me, vp).0;
                    let (dx, dz) = (d.0 * 5.0, d.2 * 5.0);
                    // (2.0.3, the user's tour, Shigaraki in Malenia's arena: he had never stood anywhere yet and his summon spot
                    // was 470 m off and 50 m up, over nothing: he fell again every 1.5 s for good) a remembered spot only when
                    // it is near the player, else beside them
                    let near = |g: &(f32, f32, f32)| ((g.0 - me.0).powi(2) + (g.2 - me.2).powi(2)).sqrt() < 40.0 && (g.1 - me.1).abs() < 15.0;
                    let mut at = l.ground.or(l.home_at).filter(near).unwrap_or((me.0 + dx, me.1, me.2 + dz));
                    if let Some(g) = crate::motion::ground_below(player, at.0, at.1 + 4.0, at.2, 12.0) {
                        at.1 = g;
                    }
                    let p = &mut c.modules.physics;
                    p.position.0 = at.0;
                    p.position.1 = at.1 + 0.2;
                    p.position.2 = at.2;
                    p.chr_proxy_pos_update_requested = true;
                    log!("villains: {} fell out of the world (y {:.0}, the player at {:.0}): back on the ground at ({:.1}, {:.1}, {:.1})", vil.name, vp.1, me.1, at.0, at.1, at.2);
                    continue;
                }
            }
            // its second form (a burst where it stood, then the other slot's character takes over the fight's pool), or
            // another of its paths after a special; the ultimate is its own form's
            let full = l.max_hp.max(300) as f32;
            let mut want: Option<Option<usize>> = None;
            if l.form.is_none() {
                if let Some((fi, _)) = forms_of(vil.id).find(|(_, f)| f.at.is_some_and(|a| l.pool < a * full)) {
                    want = Some(Some(fi));
                }
            }
            let paths: Vec<usize> = forms_of(vil.id).filter(|(_, f)| f.at.is_none()).map(|(fi, _)| fi).collect();
            if !paths.is_empty() && l.run.is_none() {
                if l.form.is_some() && !l.ult_used && l.pool < 0.3 * full {
                    want = Some(None);
                } else if l.switch_due && now.duration_since(l.form_at).as_secs_f32() >= form_hold(vil.id) {
                    let opts: Vec<Option<usize>> = std::iter::once(None).chain(paths.iter().map(|&p| Some(p))).filter(|o| *o != l.form).collect();
                    want = Some(opts[rng.random_range(0..opts.len())]);
                }
                l.switch_due = false;
            }
            if let Some(to) = want {
                let at = pos_of(c);
                world_fx::impact((at.0, at.1 + 1.0, at.2), vil.color, true);
                set_flag(l.slot, false);
                set_present(l.slot, false);
                l.slot = l.home + to.map_or(0, |fi| FORMS[fi].slot_off);
                set_flag(l.slot, true);
                // (2.0.2) out of sight a moment: it was made bare, its pieces load (adopt dresses it); then shown
                set_present(l.slot, false);
                l.reveal_at = Some(now + Duration::from_millis(350));
                // (2.0.2, the user: Muscular "kept disappearing and reappearing": the other body came in where it was last hidden)
                // it takes over where this one stood (both bodies are the same map's: the same frame)
                if let Some(n) = by_entity(l.slot) {
                    let p = &mut n.modules.physics;
                    p.position.0 = at.0;
                    p.position.1 = at.1 + 0.1;
                    p.position.2 = at.2;
                    p.chr_proxy_pos_update_requested = true;
                }
                l.form = to;
                l.stance_said = false;
                l.stance_prev = None;
                l.poise_prev = None;
                l.switch_due = to.is_none() && vil.id == "pain";
                l.form_at = now;
                l.keep_pool = Some(l.pool);
                l.handle = None;
                l.run = None;
                l.asked = now;
                l.next_move = now + Duration::from_secs_f32(2.5);
                log!("villains: {} changes form ({:.0} left, slot {})", vil.name, l.pool, l.slot);
                continue;
            }
            // its special's body (1.3.0): its fists' (or weapon's) own attacks played by its behaviour script (asked directly,
            // an attack's animation only interrupts a human character): a kick for the moves that name one, otherwise the
            // heavy blow (Carian Combo Warriors' fist heavy on the bare-handed). (An Ash of War set on its fist row didn't take:
            // it played Lion's Claw whatever the row named, lab 2026-10-18.)
            if let Some((a, at)) = l.cue_pending {
                if at.elapsed() > Duration::from_millis(30) {
                    l.cue_pending = None;
                    let lua = if a == A_KICK { "JR_Kick()" } else { "JR_Strike(3)" };
                    crate::stands::npc_cue(c, lua, &mut l.cue_flip);
                    #[cfg(feature = "lab")]
                    log!("villains: {} body {lua} for {a}", vil.name);
                }
            }
            // its own weapon in hand (HELD) from the move's aim to its last shot, its arms slot empty otherwise
            if let Some((_, row)) = HELD.iter().find(|(v, _)| *v == vil.id && l.form.is_none()) {
                let firing = l.run.as_ref().is_some_and(|r| {
                    let aim = r.mv.steps.iter().position(|s| matches!(s, V::Anim(A_AIM)));
                    let last = r.mv.steps.iter().rposition(|s| matches!(s, V::Fire { b: B_PISTOL, .. }));
                    matches!((aim, last), (Some(a), Some(f)) if r.i >= a && r.i <= f)
                });
                if firing {
                    l.held_until = Some(now + Duration::from_secs_f32(HELD_LINGER));
                }
                // (only while its models are worn: otherwise its arms are what dress_rows gave it)
                let (lent, held_lent) = BORROWED.lock().unwrap().as_ref().map_or((false, false), |v| (true, v.iter().any(|(r, ..)| r == row)));
                let want = if held_lent && l.held_until.is_some_and(|u| now < u) { *row } else { villain_armor(l.villain)[2] };
                if lent && c.character_id == 0 {
                    let p = unsafe { &mut *(c as *mut ChrIns as *mut PlayerIns) };
                    let pgd = unsafe { p.player_game_data.as_mut() };
                    let mut changed = false;
                    for ids in [&mut p.chr_asm.equipment_param_ids, &mut pgd.equipment.chr_asm.equipment_param_ids] {
                        if ids[14] != want {
                            ids[14] = want;
                            changed = true;
                        }
                    }
                    #[cfg(feature = "lab")]
                    if changed {
                        log!("villains: {} arms slot {want}", vil.name);
                    }
                    let _ = changed;
                }
            }
            // (its Ashes of War cost focus it would otherwise run out of)
            c.modules.data.fp = c.modules.data.max_fp;
            let vpos = pos_of(c);
            let (to_me, dist) = flat_dir(vpos, me);
            // (1.3.0, the user: All For One "just spams his ground dodge roll instead of fighting": the game's own fighting
            // goes on dodging where it can't find its way to the player) its own blows up close, between specials: turned
            // to the player, a light blow, a second, sometimes a heavy finisher, then a pause (one at a time, never a flurry)
            if l.run.is_none() && !crate::parry::WORLD_SLOWED.load(std::sync::atomic::Ordering::Relaxed) {
                // (a lunge: in from mid range, its first blow thrown on the way)
                if let Some((from, d, len, t0)) = l.lunge {
                    let t = (t0.elapsed().as_secs_f32() / LUNGE_SECS).min(1.0);
                    let p = &mut c.modules.physics;
                    p.position.0 = from.0 + d.0 * len * t;
                    p.position.2 = from.2 + d.2 * len * t;
                    p.chr_proxy_pos_update_requested = true;
                    if t >= 1.0 {
                        l.lunge = None;
                    }
                }
                match l.chain {
                    Some((n, at)) if at.elapsed() > Duration::from_millis(if n == 2 { 750 } else { 600 }) => {
                        let finish = n >= 3 || dist > MELEE_REACH + 1.0 || rng.random_range(0..3) == 0;
                        if finish {
                            l.chain = None;
                            l.next_melee = now + Duration::from_secs_f32(rng.random_range(1.6..2.8));
                        } else {
                            face(c, to_me);
                            let lua = if n == 1 { "JR_Strike(2)" } else { "JR_Strike(3)" };
                            crate::stands::npc_cue(c, lua, &mut l.cue_flip);
                            l.chain = Some((n + 1, now));
                            #[cfg(feature = "lab")]
                            log!("villains: {} melee {lua}", vil.name);
                        }
                    }
                    Some(_) => {}
                    // (only when its own fighting has gone quiet: most villains' game AI attacks well on its own, and the two
                    // together took 90% of a standing player's health in 20 s, lab)
                    None if dist < LUNGE_REACH && now >= l.next_melee && l.next_move.saturating_duration_since(now) > Duration::from_millis(900)
                        && l.swung.is_none_or(|t| now.duration_since(t) > Duration::from_secs(2)) => {
                        face(c, to_me);
                        if dist > MELEE_REACH {
                            // (only onto ground: a lunge goes straight, it must not carry the villain off a ledge)
                            let len = dist - 1.4;
                            let end = (vpos.0 + to_me.0 * len, vpos.1, vpos.2 + to_me.2 * len);
                            if crate::motion::ground_below(player, end.0, end.1 + 1.5, end.2, 4.5).is_some() {
                                l.lunge = Some((vpos, to_me, len, now));
                            }
                        }
                        crate::stands::npc_cue(c, "JR_Strike(1)", &mut l.cue_flip);
                        l.chain = Some((1, now));
                        #[cfg(feature = "lab")]
                        log!("villains: {} melee JR_Strike(1) at {dist:.1} m", vil.name);
                    }
                    None => {}
                }
            } else {
                l.chain = None;
                l.lunge = None;
            }
            // a special: start one
            if l.run.is_none() && l.chain.is_none() && now >= l.next_move {
                let low = l.pool < 0.3 * l.max_hp.max(300) as f32;
                let mv: Option<&'static Move> = if low && !l.ult_used && dist <= vil.ultimate.range.1 {
                    l.ult_used = true;
                    Some(&vil.ultimate)
                } else {
                    // the second form fights with another villain's specials (Shigaraki with All For One's Quirks)
                    let specials: &'static [Move] = match l.form.map(|fi| &FORMS[fi]) {
                        Some(f) if !f.moves_of.is_empty() => VILLAINS.iter().find(|v| v.id == f.moves_of).map(|v| v.specials).unwrap_or(vil.specials),
                        Some(f) => f.specials,
                        None => vil.specials,
                    };
                    let fits: Vec<&'static Move> = specials.iter().filter(|m| dist >= m.range.0 && dist <= m.range.1).filter(|m| m.share > 0.0 || c.modules.data.hp < c.modules.data.max_hp * 3 / 4).collect();
                    #[cfg(feature = "lab")]
                    let fits: Vec<&'static Move> = { let only = crate::lab::VILMOVES.lock().unwrap(); fits.into_iter().filter(|m| only.is_empty() || only.iter().any(|n| n == m.name)).collect() };
                    (!fits.is_empty()).then(|| fits[rng.random_range(0..fits.len())])
                };
                match mv {
                    Some(m) => {
                        world_fx::banner(m.name, vil.color, 1.4);
                        log!("villains: {} uses {} at {:.0} m", vil.name, m.name, dist);
                        let d = &player.chr_ins.modules.data;
                        let floor = (d.hp as f32 - d.max_hp as f32 * m.share.min(MOVE_CAP)).round() as i32;
                        // (a guard already running keeps the lower floor: two specials together still take both shares)
                        let floor = self.special_guard.map_or(floor, |(f, _)| f.min(floor));
                        self.special_guard = (floor >= 1).then(|| (floor, now + Duration::from_secs(30)));
                        l.run = Some(Run { villain: l.villain, mv: m, i: 0, t0: now, shots: 0, mark: me, warn_until: None, dash_from: None });
                    }
                    None => {
                        l.next_move = now + Duration::from_secs_f32(1.0);
                        // (lab: Pain stood 26 s as the Naraka Path at full health, its only technique a heal) a path with
                        // nothing to use from here changes path
                        if l.form.is_some_and(|fi| FORMS[fi].at.is_none()) {
                            l.switch_due = true;
                        }
                    }
                }
            }
            let Some(r) = &mut l.run else { continue };
            let _ = r.villain;
            // run its steps (several instant ones in a frame)
            let mut guard = 0;
            while r.i < r.mv.steps.len() && guard < 8 {
                guard += 1;
                let el = now.duration_since(r.t0).as_secs_f32();
                let mut next = true;
                match r.mv.steps[r.i] {
                    // (an event animation, a gesture, plays when asked)
                    V::Anim(a) if a < 1_000_000 => c.modules.event.request_animation_id = a,
                    V::Anim(a) => {
                        // (an attack's own id breaks off what it is doing; its body follows next frame: below)
                        c.modules.event.request_animation_id = a;
                        l.cue_pending = Some((a, now));
                    }
                    V::Face => face(c, to_me),
                    V::Mark => r.mark = me,
                    V::Warn(t, secs) => {
                        let secs = secs.max(TELL_MIN);
                        let fresh = r.warn_until.is_none();
                        if fresh {
                            r.warn_until = Some(now + Duration::from_secs_f32(secs));
                        }
                        if fresh && SEALS {
                            let turn = rng.random::<f32>() * std::f32::consts::TAU;
                            match t {
                                Tell::AtTarget { r: rad } => world_fx::marker(Shape::Sigil { c: r.mark, r: rad, kind: Sigil::Lightning, turn }, vil.color, secs),
                                Tell::AtSelf { r: rad } => world_fx::marker(Shape::Sigil { c: vpos, r: rad, kind: Sigil::Holy, turn }, vil.color, secs),
                                Tell::Lane { len, w } => {
                                    let (d, _) = flat_dir(vpos, me);
                                    world_fx::marker(Shape::Lane { from: vpos, to: (vpos.0 + d.0 * len, vpos.1, vpos.2 + d.2 * len), w }, vil.color, secs);
                                }
                            }
                        }
                        next = r.warn_until.is_some_and(|u| now >= u);
                        if next {
                            r.warn_until = None;
                        }
                    }
                    V::Fire { b, from, aim, n, every, spread } => {
                        let due = if every <= 0.0 { n } else { ((el / every) as u32 + 1).min(n) };
                        while r.shots < due {
                            let k = r.shots;
                            let fwd = to_me;
                            let src = match from {
                                // (the user: Madara "keeps taking damage from his own fire": a breath of fire starts clear of him)
                                From::Hand if b == B_FIRE => (vpos.0 + fwd.0 * 2.0, vpos.1 + 1.3, vpos.2 + fwd.2 * 2.0),
                                From::Hand => (vpos.0 + fwd.0 * 0.8, vpos.1 + 1.3, vpos.2 + fwd.2 * 0.8),
                                From::Above(hh) => (vpos.0, vpos.1 + hh, vpos.2),
                                From::Target(hh) => (r.mark.0, r.mark.1 + hh, r.mark.2),
                                From::Feet => (vpos.0, vpos.1 + 0.2, vpos.2),
                            };
                            let (pos, dir) = match aim {
                                Aim::AtTarget => {
                                    let a = (k as f32 - (n as f32 - 1.0) / 2.0) * spread;
                                    let (c0, s0) = (a.cos(), a.sin());
                                    let (dx, dz) = (fwd.0 * c0 - fwd.2 * s0, fwd.0 * s0 + fwd.2 * c0);
                                    let dy = ((me.1 + 1.0) - src.1) / dist.max(1.0);
                                    let l2 = (dx * dx + dy * dy + dz * dz).sqrt().max(1e-3);
                                    (src, (dx / l2, dy / l2, dz / l2))
                                }
                                Aim::Down => {
                                    let off = if n > 1 { let a = k as f32 / n as f32 * std::f32::consts::TAU; (a.cos() * 1.6, a.sin() * 1.6) } else { (0.0, 0.0) };
                                    ((src.0 + off.0, src.1, src.2 + off.1), (0.0, -1.0, 0.0))
                                }
                                Aim::Line { step } => {
                                    let (d, _) = flat_dir(vpos, r.mark);
                                    let s = (k + 1) as f32 * step;
                                    let mut p = (vpos.0 + d.0 * s, vpos.1 + 0.2, vpos.2 + d.2 * s);
                                    if let Some(g) = crate::motion::ground_below(player, p.0, p.1 + 3.0, p.2, 8.0) {
                                        p.1 = g + 0.1;
                                    }
                                    (p, (d.0, 0.0, d.2))
                                }
                                Aim::Inward { r: rad } => {
                                    let a = k as f32 / n.max(1) as f32 * std::f32::consts::TAU;
                                    let p = (r.mark.0 + a.cos() * rad, src.1, r.mark.2 + a.sin() * rad);
                                    (p, (-a.cos(), 0.0, -a.sin()))
                                }
                                Aim::Outward => {
                                    let a = k as f32 * spread;
                                    (src, (a.cos(), 0.0, a.sin()))
                                }
                            };
                            // (1.3.0, lab meter: a villain has the player's human body, and the game scales a human's
                            // spells by its catalyst and stats: the same spell did 17% of the bar fired by the boss and
                            // nothing fired by the villain. The fight's boss owns the shot (its damage is the enemy row
                            // villains.rs sets); the villain still casts, aims and plays the move)
                            let owner = boss_owner.unwrap_or(h);
                            #[cfg(feature = "lab")]
                            if k == 0 {
                                let o = find(owner);
                                log!("villains: {} shot {b} owner npc {:?} team {:?} hp {:?} no_attack {:?} no_move {:?} from ({:.1},{:.1},{:.1}) dir ({:.2},{:.2},{:.2}) player ({:.1},{:.1},{:.1})", vil.name,
                                    o.as_ref().map(|c| c.npc_param_id), o.as_ref().map(|c| c.team_type), o.as_ref().map(|c| c.modules.data.hp),
                                    o.as_ref().map(|c| c.debug_flags.disabled_secondary_actions()), o.as_ref().map(|c| c.debug_flags.disabled_movement()),
                                    pos.0, pos.1, pos.2, dir.0, dir.1, dir.2, me.0, me.1, me.2);
                            }
                            if let Err(e) = ability::spawn(owner, -1, b, pos, dir) {
                                log!("villains: {} bullet {b} failed ({e})", vil.name);
                            }
                            l.own_shot = Some(now);
                            *SHOT_AT.lock().unwrap() = Some(now);
                            r.shots += 1;
                        }
                        next = r.shots >= n;
                        if next {
                            r.shots = 0;
                        }
                    }
                    V::Pull { speed, secs } => {
                        let d = (vpos.0 - me.0, vpos.2 - me.2);
                        let len = (d.0 * d.0 + d.1 * d.1).sqrt();
                        if el < secs && len > 2.0 {
                            let dt = 1.0 / 60.0;
                            let step = (speed * dt).min(len - 2.0);
                            let p = &mut player.chr_ins.modules.physics;
                            p.position.0 += d.0 / len * step;
                            p.position.2 += d.1 / len * step;
                            p.chr_proxy_pos_update_requested = true;
                            next = false;
                        }
                    }
                    V::Blink(behind) => {
                        let back = flat_dir(vpos, me).0;
                        let mut at = (me.0 + back.0 * behind, me.1, me.2 + back.2 * behind);
                        if let Some(g) = crate::motion::ground_below(player, at.0, me.1 + 4.0, at.2, 12.0) {
                            at.1 = g;
                        }
                        world_fx::impact((vpos.0, vpos.1 + 1.0, vpos.2), vil.color, false);
                        let p = &mut c.modules.physics;
                        p.position.0 = at.0;
                        p.position.1 = at.1 + 0.1;
                        p.position.2 = at.2;
                        p.chr_proxy_pos_update_requested = true;
                        world_fx::impact((at.0, at.1 + 1.0, at.2), vil.color, false);
                        face(c, (-back.0, 0.0, -back.2));
                    }
                    V::Dash { dist: dd, secs } => {
                        let from = *r.dash_from.get_or_insert(vpos);
                        let (d, _) = flat_dir(from, r.mark);
                        let t = (el / secs).min(1.0);
                        let p = &mut c.modules.physics;
                        p.position.0 = from.0 + d.0 * dd * t;
                        p.position.2 = from.2 + d.2 * dd * t;
                        p.chr_proxy_pos_update_requested = true;
                        next = t >= 1.0;
                        if next {
                            r.dash_from = None;
                        }
                    }
                    V::Heal(share) => {
                        // (2.0.2) its own pool healed (a rise in the character's health no longer counts: the pool)
                        let full = l.max_hp.max(300) as f32;
                        l.pool = (l.pool + full * share).min(full);
                        world_fx::impact((vpos.0, vpos.1 + 1.0, vpos.2), vil.color, false);
                    }
                    V::Wait(s) => next = el >= s,
                }
                if !next {
                    break;
                }
                r.i += 1;
                r.t0 = now;
            }
            if r.i >= r.mv.steps.len() {
                l.run = None;
                l.next_move = now + Duration::from_secs_f32(rng.random_range(MOVE_GAP.0..MOVE_GAP.1));
                l.switch_due = rng.random::<f32>() < PATH_SWITCH;
                #[cfg(feature = "lab")]
                if crate::lab::VILPATH.load(std::sync::atomic::Ordering::Relaxed) {
                    l.switch_due = true;
                    l.next_move = now + Duration::from_secs_f32(2.0);
                }
            }
        }
        // (2.0.2) a defeated villain gone from the fight: its slot off, or the map shows it again at every load (the user:
        // Muscular killed after Margit came back at every rest, fighter change and warp)
        self.live.retain(|l| {
            let keep = !(l.announced_down && l.asked.elapsed() > Duration::from_secs(8)) || l.run.is_some();
            if !keep {
                set_flag(l.slot, false);
                log!("villains: {} gone (slot {} off)", VILLAINS[l.villain].name, l.slot);
            }
            keep
        });
        let _ = &self.pull_until;
        #[cfg(feature = "lab")]
        self.watch_flags("our tick");
    }

    /// While the game loads: every slot off. (2.0.2, the user: after a fall took them back to the grace Muscular "was still there
    /// instead of resetting the fight"; lab log 10-21: a slot cleared as the player left came back on after the load, the
    /// flags restored from the save. Cleared until the player is in, and again once in: the no-fight check.)
    pub fn loading(&mut self) {
        all_slots_off("loading");
    }

    /// Leaving the world: forget everything (the characters go with the map).
    pub fn reset(&mut self) {
        all_slots_off("leaving the world");
        self.live.clear();
        self.called.clear();
        self.phase2_at = None;
        self.lowest = None;
        if self.special_guard.take().is_some() {
            crate::movement::set_no_dead(crate::movement::NO_DEAD_SPECIAL, false);
        }
    }
}
