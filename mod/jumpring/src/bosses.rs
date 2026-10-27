//! Empowered bosses (0.7.0): Elden Ring's bosses hold their own against JUMP FORCE fighters. Applies
//! to every enemy with a boss health bar on screen (CSFeMan's boss displays), at runtime only: no game
//! files are changed or shipped.
//! - more health (BOSS_HP), kept as a percentage so the bar stays where it was
//! - faster, more aggressive animations (BOSS_SPEED)
//! - "boss arts": every few seconds an extra attack cast by the boss, made of other Elden Ring bosses'
//!   own spells with their own damage (lab audition, see CANDIDATES): Radagon's golden holy shockwave
//!   up close, Placidusax's red lightning where the player stands (it flashes before it strikes),
//!   Rennala's comets from range, and below half health a cataclysm: Radagon's holy floor bursts around
//!   the boss, then Placidusax's giant explosion. Their damage is
//!   rescaled in memory to a share of the player's max HP (ART_SHARE): the spells' own values are tuned
//!   for late-game fights and one-shot an early character. (This also applies when Rennala, Radagon or
//!   Placidusax cast them in their own fights.)

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eldenring::cs::{AtkParam_Npc, Bullet, CSFeManImp, ChrIns, FieldInsHandle, PlayerIns, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::FromStatic;
use rand::RngExt;

use crate::ability::{self, V3};
use crate::log;
use crate::world_fx::{self, Shape, Sigil};

/// 1.3.0 (user: "the movement of each boss should be fluid"): the animations and the ground they cover sped up
/// together (was 1.12 and root motion 1.3: the feet slid), so a faster boss still plants its feet.
const BOSS_SPEED: f32 = 1.15;
const BOSS_MOVE: f32 = 1.15;
/// 1.3.0 (user: "Every attack should be animated in these fights"): the arts (spells from thin air while the
/// boss slowed to 45% to gather itself) are retired; new threats come from animated villains (villains.rs).
/// The lab's forced arts still work.
const BOSS_ARTS: bool = false;

#[derive(Clone, Copy, Debug)]
enum Art {
    Shockwave,
    Lightning,
    Volley,
    Cataclysm,
}

/// One bullet still to fire: when, which, where, which way.
struct Shot {
    at: Instant,
    owner: FieldInsHandle,
    bullet: i32,
    pos: V3,
    dir: V3,
}

struct Boss {
    /// 0.16.0: is the room around it small (walls close on most sides)? No cataclysm there
    small_room: Option<bool>,
    /// 1.1.0: its last cataclysm (one when it falls below half, then at most one every CATACLYSM_GAP)
    last_cataclysm: Option<Instant>,
    next_art: Instant,
    /// 0.10.0: an art being telegraphed: which, when it strikes, where the player was marked
    tell: Option<(Art, Instant, V3)>,
    /// the art's attacks are out until then; the player has not been hit by it so far
    art_until: Option<Instant>,
    clean: bool,
    /// open to a counter until then (a dodged or parried art)
    exposed_until: Option<Instant>,
    /// 1.3.0: the animation it played last frame, and when its next echo may come
    last_anim: Option<i32>,
    next_echo: Instant,
}

/// 0.10.0 tells: how long before each art strikes, its colour, its words and the aura it borrows.
fn tell_of(art: Art) -> (f32, [f32; 4], &'static str) {
    match art {
        Art::Shockwave => (0.8, [1.0, 0.80, 0.30, 1.0], "HOLY SHOCKWAVE: GET CLEAR"),
        Art::Lightning => (0.65, [1.0, 0.20, 0.14, 1.0], "RED LIGHTNING: KEEP MOVING"),
        Art::Volley => (0.7, [0.40, 0.65, 1.0, 1.0], "COMETS: DASH ASIDE"),
        Art::Cataclysm => (1.6, [0.80, 0.50, 1.0, 1.0], "CATACLYSM: RUN"),
    }
}

/// While a boss winds up an art its animation slows to this (it gathers itself), and while it is open
/// to a counter, to this.
const TELL_SPEED: f32 = 0.45;
/// 2.0.6: how long the switch stays off for the player's death to start (see guard_kills)
const PLAYER_DEATH_WAIT: Duration = Duration::from_secs(8);
const EXPOSED_SPEED: f32 = 0.55;
pub const EXPOSED_FOR: f32 = 2.6;

#[derive(Default)]
pub struct Bosses {
    known: HashMap<FieldInsHandle, Boss>,
    shots: Vec<Shot>,
    /// the player's max HP the art damage was last set for
    damage_for_hp: i32,
    orig_atk: HashMap<i32, [u16; 5]>,
    /// lab: forced arts go through every kind in turn
    forced: usize,
    /// the player's HP last frame
    last_hp: i32,
    /// 0.16.1: while an art's attacks are out, the player's health won't drop below this (until then)
    art_guard: Option<(i32, Instant)>,
    /// 0.16.0: each boss's HP last frame (its hits are scaled by the difficulty)
    boss_last_hp: HashMap<FieldInsHandle, i32>,
    /// the last training-bag heal seen (training.rs)
    refill_seen: u64,
    /// 1.1.0: how far each spell hurts (metres, from its own bullet params), and the radii it had
    reach: HashMap<i32, f32>,
    orig_radius: HashMap<i32, (f32, f32)>,
    /// 1.1.0: the game's "enemies can't die" switch is ours (a boss fight): kills are ours to make
    enemy_guard: bool,
    /// enemies that died under the switch: their death goes through with the switch off
    killing: Vec<(usize, Instant)>,
    /// every other enemy's HP last frame (a fall to 1 HP under the switch was a killing blow)
    others_last: HashMap<usize, i32>,
    /// 2.0.3: a boss whose phase change starts at 1 HP (Malenia, Rykard's serpent) with its first phase's health gone,
    /// held at 1 (not killed) for the game's own phase change, since when
    phase_one_held: HashMap<FieldInsHandle, Instant>,
    /// ... and the ones in their second phase, by entity (the next empty bar is the kill)
    refilled: std::collections::HashSet<u32>,
}

// (1.3.0, lab: Starscourge Radahn is never on the game's by-distance list, even 2 m away (a big open-world boss): found
// by its handle, his fight's health scaling, boss attacks and villain guard had never run)
fn find(handle: FieldInsHandle) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance_mut() }.ok()?;
    wcm.chr_ins_by_handle_mut(&handle).map(|c| unsafe { &mut *(c as *mut ChrIns) })
}

/// a character by its address, wherever it is (every character set, not just the nearest ones)
fn by_addr(key: usize) -> Option<&'static ChrIns> {
    let wcm = unsafe { WorldChrMan::instance() }.ok()?;
    wcm.chr_sets.iter().flatten().flat_map(|set| set.characters()).find(|c| *c as *const ChrIns as usize == key).map(|c| unsafe { &*(c as *const ChrIns) })
}

/// 1.1.0: health the ledger took from a boss itself this frame, already scaled by the difficulty
/// (ChrIns address, amount): its listed-damage top-ups, grab and punch damage. Before, those wrote raw
/// damage straight into the boss's health: an Awakening's top-up (about 3000 at level 62) took the
/// Godskin Apostle from half its bar to nothing.
pub static DIRECT: std::sync::Mutex<Vec<(usize, i32)>> = std::sync::Mutex::new(Vec::new());

/// 1.1.0: about how big one blow is right now (the move playing's hit, a punch), raw, and how much the
/// move may still deal (its ledger's room): a blow the "can't die" switch stopped at 1 HP is counted
/// as one such hit, within that room, and the extra goes back to the ledger (FLOOR_EXTRA).
pub static RAW_HINT: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static RAW_ROOM: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
pub static FLOOR_EXTRA: std::sync::Mutex<Vec<(usize, i32)>> = std::sync::Mutex::new(Vec::new());

/// The ledger takes `raw` from boss `c` itself: what the difficulty leaves of it comes off now, and
/// bosses.rs counts it as already scaled. Never below 1 HP here: a kill is bosses.rs's to make.
pub fn hit_boss(c: &mut ChrIns, raw: i32) -> i32 {
    let scaled = ((raw as f32) / boss_hp_mult()).round().max(1.0) as i32;
    let hp = c.modules.data.hp;
    c.modules.data.hp = (hp - scaled).max(1);
    DIRECT.lock().unwrap().push((c as *const ChrIns as usize, scaled));
    hp - c.modules.data.hp
}

/// 0.16.0: the difficulty's boss health multiplier (f32 bits), for the ledger's Awakening share.
pub static BOSS_HP_MULT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3FAC_CCCD);

pub fn boss_hp_mult() -> f32 {
    f32::from_bits(BOSS_HP_MULT.load(std::sync::atomic::Ordering::Relaxed)).max(1.0)
}

/// Lab key ]: the next art now.
pub static FORCE_ART: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Lab: which of the bullets that end cleanly (lab/judge2.txt) have AtkParam_Npc rows (usable by bosses).
#[cfg(feature = "lab")]
pub fn probe_npc_rows() {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return };
    let mut n = 0;
    for line in include_str!("../../../lab/judge2.txt").lines() {
        if !line.trim_end().ends_with("OK") {
            continue;
        }
        let Some(id) = line.trim_start_matches('\u{feff}').split_whitespace().next().and_then(|x| x.parse::<u32>().ok()) else { continue };
        let Some(b) = repo.get::<Bullet>(id) else { continue };
        let atk = b.atk_id_bullet();
        let hit = b.hit_bullet_id();
        let hit_atk = repo.get::<Bullet>(hit.max(0) as u32).map(|h| h.atk_id_bullet()).unwrap_or(-1);
        let npc = |a: i32| a > 0 && repo.get::<AtkParam_Npc>(a as u32).is_some();
        if npc(atk) || npc(hit_atk) {
            n += 1;
            log!("npc-ok {id} atk {atk} hit {hit}/{hit_atk}: {}", line.trim());
        }
    }
    log!("npc-ok: {n} bullets");
}

/// Lab key ]: try the next candidate boss spell (cast by the boss on screen, at the player). Results
/// 2026-10-08: kept 2190521, 4520341, 2031130, 2190296, 4520325; 4510391/4510392 are Fortissax's death
/// lightning (deathblight: instant death); 4730645 lingers and keeps hitting; the meteors, Godskin
/// pillars and Morgott's swords barely show when another boss casts them.
#[cfg(feature = "lab")]
pub static AUDITION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(usize::MAX);
#[cfg(feature = "lab")]
const CANDIDATES: &[(i32, &str, u8)] = &[
    // 0 = on the player, 1 = above the player falling, 2 = from the boss at the player
    (4510951, "Ancient Dragon slam shockwave", 2), (2190510, "Radagon shockwave", 0), (2190521, "Radagon holy shockwave", 0),
    (4730224, "Radahn shockwave AoE", 0), (4520341, "Placidusax lightning tell", 0), (4520340, "Placidusax lightning on target", 0),
    (2031130, "Rennala comet", 2), (3250510, "Draconic Sentinel homing lightning", 2), (4730380, "Radahn meteor 1", 1),
    (4730384, "Radahn meteor", 1), (4620560, "Astel meteors", 1), (4520325, "Placidusax giant explosion", 0),
    (2190296, "Radagon holy floor explode", 0), (3570301, "Godskin fire pillars", 0), (2130064, "Morgott swords fall", 1),
];

/// Other bosses' spells (each cast by Elden Ring's own boss; they end by themselves and hurt the player
/// when another boss casts them: lab audition 2026-10-08).
const HOLY_SHOCKWAVE: i32 = 2190521; // Radagon
const RED_LIGHTNING: i32 = 4520341; // Placidusax: a tell, then the strike on that spot
const COMET: i32 = 2031130; // Rennala
const HOLY_FLOOR: i32 = 2190296; // Radagon, holy floor explode
const GIANT_EXPLOSION: i32 = 4520325; // Placidusax

/// Raw damage of one hit of each spell, as a share of the player's max HP (before their defenses).
/// 0.16.0: the cataclysm (holy floor x5, then the giant explosion) can no longer take a full bar
/// (it one-shot the player in the Grave Warden Duelist's small room): 0.06 x5 + 0.25 at most.
const ART_SHARE: [(i32, f32); 5] = [(HOLY_SHOCKWAVE, 0.14), (RED_LIGHTNING, 0.28), (COMET, 0.2), (HOLY_FLOOR, 0.06), (GIANT_EXPLOSION, 0.25)];

/// 0.16.1: the most of the player's health one art can take (the user's pick).
const ART_CAP: f32 = 0.4;

/// 1.3.0: how a boss's echo goes out
#[derive(Clone, Copy, Debug)]
enum Echo {
    Ring,
    Shot,
    Wave,
    OnPlayer,
}
/// The remembrance bosses' signature powers (their character number = NpcParam row / 10000, the bullet, how it
/// goes): Godrick's wind, Rennala's comets, Radahn's gravity, Rykard's and the Fire Giant's fire, Morgott's and
/// Radagon's holy light, Maliketh's Destined Death, Mohg's bloodflame, Malenia's scarlet rot, the Regal Ancestor's
/// holy ground, Astel's laser, Fortissax's and Placidusax's red lightning, Hoarah Loux's quake, the Elden Beast's
/// golden swords. All are Elden Ring's own bosses' spells.
const ECHOES: [(i32, i32, Echo); 17] = [
    (4750, 4510951, Echo::Wave),      // Godrick: an Ancient Dragon's moving shockwave (his axe's wind)
    (2030, 2031130, Echo::Shot),      // Rennala: her comet
    (4730, 4730224, Echo::Ring),      // Radahn: his gravity shockwave
    (4710, 2500210, Echo::Wave),      // Rykard: a breath of fire
    (2130, 2190521, Echo::Ring),      // Morgott: holy shockwave
    (4760, 2500210, Echo::Wave),      // Fire Giant: fire
    (2110, 2200294, Echo::Shot),      // Maliketh: a golden blade of Destined Death
    (4800, 2500210, Echo::Wave),      // Mohg: bloodflame
    (2120, 3050905, Echo::Ring),      // Malenia: scarlet rot blooms
    (4670, 2190296, Echo::OnPlayer),  // Regal Ancestor Spirit: holy ground under the player
    (4620, 4620220, Echo::Shot),      // Astel: its laser
    (4510, 4520341, Echo::OnPlayer),  // Fortissax: red lightning
    (4720, 4730224, Echo::Ring),      // Hoarah Loux: the quake
    (4721, 4730224, Echo::Ring),      // Hoarah Loux (second form)
    (4520, 4520341, Echo::OnPlayer),  // Placidusax: red lightning
    (2200, 2200294, Echo::Shot),      // Elden Beast: its sword beam
    (2190, 2190521, Echo::Ring),      // Radagon: holy shockwave
];
/// An echo comes with this share of a boss's attacks, at most one every ECHO_GAP seconds, ECHO_AFTER into the swing.
const ECHO_CHANCE: f32 = 0.45;
const ECHO_GAP: (f32, f32) = (7.0, 11.0);
const ECHO_AFTER: f32 = 0.65;
/// One echo takes at most this share of the player's health.
const ECHO_SHARE: f32 = 0.15;
/// 2.0.3 (the user, Malenia at level 160: "malenia insta-killed me"; the lab log: 4200 HP to nothing 0.65 s after her
/// echo, the ring's eight scarlet rot blooms, 4.3 m round each, all landing at her feet) one echo takes at most this
/// share of the player's health, whatever number of its shots land (the arts' guard)
const ECHO_CAP: f32 = 0.3;

/// 0.16.0: a room is small when walls stand within this many metres of the boss in most directions.
const SMALL_ROOM_M: f32 = 16.0;

/// 1.1.0 (user): the cataclysm's giant explosion reaches this far from the boss, no further (Placidusax's
/// own blast went far past the ring shown, so it couldn't be outrun), and the ring shows exactly this.
const CATACLYSM_R: f32 = 16.0;
/// 1.1.0 (user: Rennala cast it five times in a minute): at most one cataclysm this often.
const CATACLYSM_GAP: Duration = Duration::from_secs(45);

/// 1.1.0: how far a spell hurts: the largest hit radius of its bullet and of the bullets it spawns that
/// carry damage.
fn reach_of(bullet: i32) -> f32 {
    let Ok(repo) = (unsafe { SoloParamRepository::instance() }) else { return 0.0 };
    let (mut queue, mut done, mut hurt, mut any) = (vec![bullet], 0, 0.0f32, 0.0f32);
    while let Some(id) = queue.pop() {
        if id <= 0 || done > 6 {
            continue;
        }
        done += 1;
        let Some(b) = repo.get::<Bullet>(id as u32) else { continue };
        queue.push(b.hit_bullet_id());
        queue.push(b.interval_create_bullet_id());
        let r = b.hit_radius().max(b.hit_radius_max());
        any = any.max(r);
        if b.atk_id_bullet() > 0 {
            hurt = hurt.max(r);
        }
    }
    if hurt > 0.0 { hurt } else { any }
}

/// 1.1.0: a bullet (and its children) flies straight: no homing (Rennala's comets curved after the
/// player, so the lanes shown were wrong). Returns the turn rates it had.
pub(crate) fn no_homing(bullet: i32) -> Vec<(i32, i16, i16)> {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return Vec::new() };
    let mut out = Vec::new();
    let mut queue = vec![bullet];
    while let Some(id) = queue.pop() {
        if id <= 0 || out.len() > 6 || out.iter().any(|(i, _, _)| *i == id) {
            continue;
        }
        let Some(b) = repo.get_mut::<Bullet>(id as u32) else { continue };
        queue.push(b.hit_bullet_id());
        queue.push(b.interval_create_bullet_id());
        out.push((id, b.homing_angle(), b.homing_angle_x()));
        log!("bosses: bullet {id}: auto homing {} follow {} homing from {:.1} m, stop {:.1} m, prediction {:.2}", b.is_enable_auto_homing(), b.follow_type(), b.homing_begin_dist(), b.horming_stop_range(), b.prediction_shoot_observe_time());
        b.set_homing_angle(0);
        b.set_homing_angle_x(0);
        // (1.1.0 playtest: still curved after the turn rate was 0: it also aims itself at the player
        // without a lock (auto homing) and leads them (prediction))
        b.set_is_enable_auto_homing(false);
        b.set_follow_type(0);
        b.set_homing_begin_dist(9999.0);
        b.set_prediction_shoot_observe_time(0.0);
        // (1.1.0 playtest 2: still aimed at where the player stood when it left, not down its lane: the
        // caster's own aim overrode the direction we fire it in)
        log!("bosses: bullet {id}: owner overrides its angle {}, shoot angle {} / {}", b.is_owner_override_init_angle(), b.shoot_angle(), b.shoot_angle_xz());
        b.set_is_owner_override_init_angle(false);
    }
    out
}

/// 1.1.0: the comets' two paths for a mark (where they're aimed). The game sends a boss's comet at where
/// the player stands the moment it leaves, whatever direction it is given (user's playtests: no homing,
/// no angle override, still straight at them), so both leave together the moment the lanes lock, from
/// either side of the boss (2 m in front, COMET_SIDE m apart), at the mark, and on past it.
fn comet_paths(boss: V3, mark: V3) -> [(V3, V3); 2] {
    let (dx, dz) = (mark.0 - boss.0, mark.2 - boss.2);
    let dist = (dx * dx + dz * dz).sqrt().max(0.1);
    let to = (dx / dist, dz / dist);
    let path = |side: f32| {
        let from = (boss.0 + to.0 * 2.0 - to.1 * side, boss.1, boss.2 + to.1 * 2.0 + to.0 * side);
        let (ex, ez) = (mark.0 - from.0, mark.2 - from.2);
        let l = (ex * ex + ez * ez).sqrt().max(0.1);
        (from, (from.0 + ex / l * (l + 8.0), mark.1, from.2 + ez / l * (l + 8.0)))
    };
    [path(-COMET_SIDE), path(COMET_SIDE)]
}

/// How far either side of the boss's line the two comets leave from.
const COMET_SIDE: f32 = 1.2;

/// 1.1.0: shrink a spell's hit radii (its bullet's and its children's, start and full size alike) so it
/// hurts within `max` metres at most. From the original values, so it never compounds. Returns its reach.
fn limit_reach(orig: &mut HashMap<i32, (f32, f32)>, bullet: i32, max: f32) -> f32 {
    let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return 0.0 };
    let mut chain = Vec::new();
    let mut queue = vec![bullet];
    while let Some(id) = queue.pop() {
        if id <= 0 || chain.len() > 6 || chain.contains(&id) {
            continue;
        }
        let Some(b) = repo.get::<Bullet>(id as u32) else { continue };
        queue.push(b.hit_bullet_id());
        queue.push(b.interval_create_bullet_id());
        orig.entry(id).or_insert((b.hit_radius(), b.hit_radius_max()));
        chain.push(id);
    }
    let full = chain.iter().map(|id| { let o = orig[id]; o.0.max(o.1) }).fold(0.0f32, f32::max);
    if full <= 0.0 {
        return 0.0;
    }
    let k = (max / full).min(1.0);
    for id in &chain {
        let o = orig[id];
        if let Some(b) = repo.get_mut::<Bullet>(*id as u32) {
            b.set_hit_radius(if o.0 > 0.0 { o.0 * k } else { o.0 });
            b.set_hit_radius_max(if o.1 > 0.0 { o.1 * k } else { o.1 });
        }
    }
    log!("bosses: bullet {bullet}: reach {full:.1} m -> {:.1} m", full * k);
    full * k
}

/// Walls around the boss: 8 level rays at chest height; small when 5 or more hit within SMALL_ROOM_M.
fn small_room(player: &PlayerIns, at: V3) -> bool {
    let mut near = 0;
    for k in 0..8 {
        let a = k as f32 / 8.0 * std::f32::consts::TAU;
        let from = eldenring::position::HavokPosition(at.0, at.1 + 1.5, at.2, 0.0);
        let d = eldenring::position::PositionDelta(a.cos() * SMALL_ROOM_M, 0.0, a.sin() * SMALL_ROOM_M);
        if crate::motion::ray(player, &from, d).is_some() {
            near += 1;
        }
    }
    near >= 5
}

/// Rescale the AtkParam_Npc rows of a bullet and its child bullets to `total` damage, keeping the
/// spell's mix of elements. Rows under 1000 (1, 20...) are generic, shared by many enemies: never
/// touched. `orig` keeps each row's own values so rescaling never compounds.
pub(crate) fn scale_npc_damage(orig: &mut HashMap<i32, [u16; 5]>, bullet: i32, total: f32) -> usize {
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
        if atk < 1000 {
            continue;
        }
        let Some(a) = repo.get_mut::<AtkParam_Npc>(atk as u32) else { continue };
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

impl Bosses {
    /// `boss_hp`: the difficulty (ui::Difficulty): bosses' health x Elden Ring's (0.16.0; their damage
    /// stays Elden Ring's own).
    pub fn tick(&mut self, player: &mut PlayerIns, boss_hp: f32, parried: bool, training: bool) {
        let Ok(fe) = (unsafe { CSFeManImp::instance() }) else { return };
        let now = Instant::now();
        let shown: Vec<FieldInsHandle> = fe.boss_health_displays.iter().filter(|b| b.fmg_id > 0 && !b.field_ins_handle.is_empty()).map(|b| b.field_ins_handle).collect();
        self.known.retain(|h, _| shown.contains(h));
        self.boss_last_hp.retain(|h, _| shown.contains(h));
        BOSS_HP_MULT.store(boss_hp.to_bits(), std::sync::atomic::Ordering::Relaxed);
        let hp = player.chr_ins.modules.data.hp;
        let hurt = self.last_hp > 0 && hp < self.last_hp;
        // 0.16.1: one art takes at most ART_CAP of the player's health (the comet volley's comets hit
        // several times each as they pass through: one volley took 70% of the user's bar, twice fatal)
        // (the comets land several hits in one frame, so the game's own "can't die" switch is on while
        // the art is out: a frame that would kill leaves 1 HP, and the floor puts it back)
        if let Some((floor, until)) = self.art_guard {
            if now >= until {
                self.art_guard = None;
                crate::movement::set_no_dead(crate::movement::NO_DEAD_ART, false);
            } else {
                crate::movement::set_no_dead(crate::movement::NO_DEAD_ART, true);
                // 1.1.0: never a death undone (writing HP back after the game began a death left the user
                // standing at 0 HP after their death animation)
                if hp < floor && hp > 0 {
                    log!("bosses: the art took the player to {hp} HP: held at {floor} (40%)");
                    player.chr_ins.modules.data.hp = floor;
                }
            }
        }
        self.last_hp = player.chr_ins.modules.data.hp;
        // 1.1.0: in a boss fight no enemy dies by the game's own hand (its "enemies can't die" switch):
        // a hit that would kill leaves 1 HP, and this decides. Deku's Awakening took the Godskin Apostle
        // from half to nothing: one blow bigger than its real health killed it outright, past the
        // Awakening's share and the difficulty both (they can only take back from the living).
        self.guard_kills(!shown.is_empty() && !training && boss_hp > 1.0, &shown);
        if shown.is_empty() {
            self.shots.clear();
            let mut fx = world_fx::FX.lock().unwrap();
            fx.live = None;
            fx.live_lanes.clear();
            return;
        }
        let force = FORCE_ART.swap(false, std::sync::atomic::Ordering::Relaxed);
        #[cfg(feature = "lab")]
        let audition = AUDITION.swap(usize::MAX, std::sync::atomic::Ordering::Relaxed);
        // art damage follows the player's health
        let max_hp = player.chr_ins.modules.data.max_hp;
        if max_hp != self.damage_for_hp {
            self.damage_for_hp = max_hp;
            let rows: Vec<usize> = ART_SHARE.iter().map(|(b, share)| scale_npc_damage(&mut self.orig_atk, *b, max_hp as f32 * share)).collect();
            for (_, b, _) in ECHOES.iter() {
                scale_npc_damage(&mut self.orig_atk, *b, max_hp as f32 * ECHO_SHARE);
            }
            log!("bosses: art damage set for {max_hp} HP (rows per spell {rows:?})");
        }
        if self.reach.is_empty() {
            limit_reach(&mut self.orig_radius, GIANT_EXPLOSION, CATACLYSM_R);
            log!("bosses: comets fly straight (homing was {:?})", no_homing(COMET));
            for b in [HOLY_SHOCKWAVE, RED_LIGHTNING, COMET, HOLY_FLOOR, GIANT_EXPLOSION] {
                self.reach.insert(b, reach_of(b));
            }
            log!("bosses: spell reach {:?}", self.reach);
        }
        let reach = |b: i32, or: f32| self.reach.get(&b).copied().filter(|r| *r > 0.1).unwrap_or(or);
        let (r_wave, r_bolt, r_comet, r_floor, r_blast) = (reach(HOLY_SHOCKWAVE, 4.0), reach(RED_LIGHTNING, 2.6), reach(COMET, 1.2), reach(HOLY_FLOOR, 2.0), reach(GIANT_EXPLOSION, CATACLYSM_R).min(CATACLYSM_R));
        // a comet hits the player when it passes within its reach of their body (about 0.4 m round)
        let comet_w = 2.0 * (r_comet + 0.4);
        let me = ability::position(player);
        for h in shown {
            let Some(boss) = find(h) else { continue };
            let boss_key = &*boss as *const ChrIns as usize;
            // lab: every animation a boss plays (1.3.0: which of its own attacks to build on)
            #[cfg(feature = "lab")]
            {
                static LAST: std::sync::Mutex<Option<HashMap<usize, i32>>> = std::sync::Mutex::new(None);
                let ta = &boss.modules.time_act;
                let a = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
                let mut g = LAST.lock().unwrap();
                let m = g.get_or_insert_with(HashMap::new);
                if m.insert(boss_key, a) != Some(a) {
                    log!("lab boss anim: npc {} anim {a}", boss.npc_param_id);
                }
            }
            let state = self.known.entry(h).or_insert_with(|| Boss { small_room: None, last_cataclysm: None, next_art: now + Duration::from_secs(6), tell: None, art_until: None, clean: true, exposed_until: None, last_anim: None, next_echo: now + Duration::from_secs(4) });
            // 2.0.3 (the user, Malenia at level 140: "she just died after the cutscene and Shigaraki never spawned in and it
            // said that I killed her"; the lab log: her first phase's bar emptied, we killed her as at any boss's last health,
            // and the game, holding her dead, ended her second phase as it began. Held at 0 next, her phase change still
            // played but her phase-two heal can't lift a body at 0: m15 event 15002811 waits for HP <= 1 and a hit) such a
            // boss is held at 1, the game's "can't die" switch on: its own phase change goes on from there (or, after 3
            // minutes with none while its bar shows, it dies)
            if let Some(at) = self.phase_one_held.get(&h).copied() {
                if boss.modules.data.hp > 1 {
                    self.phase_one_held.remove(&h);
                    self.refilled.insert(boss.event_entity_id);
                    self.boss_last_hp.insert(h, boss.modules.data.hp);
                    log!("bosses: {h:?} refilled ({} HP): its second phase", boss.modules.data.hp);
                } else if now.duration_since(at) > Duration::from_secs(180) {
                    self.phase_one_held.remove(&h);
                    self.refilled.insert(boss.event_entity_id);
                    self.killing.push((boss_key, now));
                    log!("bosses: {h:?} held at 1 for 3 minutes with no second phase: it dies");
                }
            }
            if boss.modules.data.hp <= 0 {
                continue;
            }
            // 0.16.0: more health for the difficulty chosen. The game puts a boss's max HP back every
            // frame (0.15's x1.35 lasted one frame: bosses had their own health), so each hit takes
            // 1/boss_hp of what it would: the same as boss_hp times the health, and the bar shows it.
            // (A blow that kills outright still kills.)
            // the training bag was healed: its new HP is no hit taken back
            {
                let r = crate::training::REFILL.lock().unwrap();
                if r.0 != self.refill_seen && r.2 == Some(h) {
                    self.refill_seen = r.0;
                    self.boss_last_hp.insert(h, r.3);
                }
            }
            let direct: i32 = {
                let mut dq = DIRECT.lock().unwrap();
                let mine: i32 = dq.iter().filter(|(k, _)| *k == boss_key).map(|(_, v)| *v).sum();
                dq.retain(|(k, _)| *k != boss_key);
                mine
            };
            // 1.3.0: a villain's hit on the boss is put back (villains and bosses don't hurt each other)
            if let Some(&last) = self.boss_last_hp.get(&h) {
                let bp = boss.modules.physics.position;
                let far = ((bp.0 - me.0).powi(2) + (bp.2 - me.2).powi(2)).sqrt() > 7.0;
                let hp = boss.modules.data.hp;
                if hp < last && hp > 0 && direct == 0 && crate::villains::hurt_by_villain(boss.last_hit_by, far) {
                    log!("bosses: {h:?} lost {} to a villain: put back", last - hp);
                    boss.modules.data.hp = last;
                }
            }
            let entity = boss.event_entity_id;
            let d = &mut boss.modules.data;
            match self.boss_last_hp.insert(h, d.hp) {
                None => log!("bosses: {h:?} {} HP, takes 1/{boss_hp} of each hit", d.max_hp),
                // (1.2.0 stress test: the game lowered a boss's max HP mid-fight, 2889 -> 2600; each frame it
                // put HP back down under the new max and that read as a hit, scaled, put back, clamped again)
                // above its max is no hit: the game's own correction
                Some(last) if last > d.max_hp && d.hp <= d.max_hp => {
                    log!("bosses: {h:?} max HP now {}: {last} -> {} is no hit", d.max_hp, d.hp);
                }
                Some(last) if d.hp < last && d.hp > 0 && boss_hp > 1.0 => {
                    // the game's own blows are scaled here; what the ledger took itself already is
                    let mut raw = (last - d.hp - direct).max(0);
                    // a blow the "can't die" switch stopped at 1 HP was bigger than what it took
                    if d.hp == 1 && self.enemy_guard && raw > 0 {
                        let per_hit = RAW_HINT.load(std::sync::atomic::Ordering::Relaxed);
                        let room = RAW_ROOM.load(std::sync::atomic::Ordering::Relaxed);
                        let extra = (per_hit - raw).min(room).max(0);
                        if extra > 0 {
                            raw += extra;
                            FLOOR_EXTRA.lock().unwrap().push((boss_key, extra));
                        }
                    }
                    let keep = if raw > 0 { (raw as f32 / boss_hp).round().max(1.0) as i32 } else { 0 } + direct;
                    log!("bosses: hit {} -> {keep} ({}/{} HP left)", last - d.hp, last - keep, d.max_hp);
                    // (2.0.2, the user: Godrick's bar was empty and he stood on for 20 s, lab log of that fight: a hit scaled to leave
                    // him at exactly 1 HP; under the "can't die" switch the game can take no more from 1, so no later hit ever
                    // showed and he stayed) 1 HP left is the kill: the switch's own 1 is the only one left at 1
                    if last - keep <= 1 && crate::villains::phase_change_at_one(entity) && !self.refilled.contains(&entity) {
                        log!("bosses: {h:?} first phase's health gone: held at 1 for the game's own phase change");
                        self.phase_one_held.insert(h, now);
                        d.hp = 1;
                    } else if last - keep <= 1 {
                        // its last health: now it dies (the switch held it at 1 until we said so)
                        log!("bosses: {h:?} defeated");
                        self.killing.push((boss_key, now));
                        d.hp = 0;
                    } else {
                        d.hp = last - keep;
                    }
                    self.boss_last_hp.insert(h, d.hp);
                }
                _ => {}
            }
            let low = d.hp * 2 < d.max_hp;
            boss.modules.physics.motion_multiplier = BOSS_MOVE;
            let bp = boss.modules.physics.position;
            let bpos = (bp.0, bp.1, bp.2);

            // 1.3.0 (the user: no runes, "new attacks that the bosses cast themselves"): a remembrance boss's own
            // attacks sometimes carry its signature power. When it starts one of its attacks (Elden Ring's enemy attack
            // animations, 3000-3999), at the end of its own swing (ECHO_AFTER) its power goes out toward the player: its
            // own wind-up is the warning, no seal, nothing slowed
            {
                let ta = &boss.modules.time_act;
                let anim = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
                let fresh = state.last_anim.replace(anim) != Some(anim);
                if let Some((_, bullet, kind)) = ECHOES.iter().find(|(c, _, _)| *c == boss.npc_param_id / 10_000) {
                    if fresh && (3000..4000).contains(&anim) && now >= state.next_echo && rand::rng().random::<f32>() < ECHO_CHANCE {
                        state.next_echo = now + Duration::from_secs_f32(rand::rng().random_range(ECHO_GAP.0..ECHO_GAP.1));
                        let (dx, dz) = (me.0 - bpos.0, me.2 - bpos.2);
                        let dist = (dx * dx + dz * dz).sqrt().max(0.1);
                        let to_me = (dx / dist, 0.0, dz / dist);
                        let at = |t: f32| now + Duration::from_secs_f32(t);
                        match kind {
                            // out from its feet all round
                            Echo::Ring => {
                                for k in 0..8 {
                                    let a = k as f32 * std::f32::consts::TAU / 8.0;
                                    self.shots.push(Shot { at: at(ECHO_AFTER), owner: h, bullet: *bullet, pos: (bpos.0, bpos.1 + 0.3, bpos.2), dir: (a.cos(), 0.0, a.sin()) });
                                }
                            }
                            // from its chest, at the player
                            Echo::Shot => {
                                let dy = ((me.1 + 1.0) - (bpos.1 + 1.6)) / dist;
                                self.shots.push(Shot { at: at(ECHO_AFTER), owner: h, bullet: *bullet, pos: (bpos.0 + to_me.0 * 1.5, bpos.1 + 1.6, bpos.2 + to_me.2 * 1.5), dir: (to_me.0, dy.clamp(-0.4, 0.4), to_me.2) });
                            }
                            // along the ground at the player
                            Echo::Wave => {
                                self.shots.push(Shot { at: at(ECHO_AFTER), owner: h, bullet: *bullet, pos: (bpos.0 + to_me.0 * 1.5, bpos.1 + 0.2, bpos.2 + to_me.2 * 1.5), dir: to_me });
                            }
                            // down on where the player stands
                            Echo::OnPlayer => {
                                self.shots.push(Shot { at: at(ECHO_AFTER + 0.2), owner: h, bullet: *bullet, pos: (me.0, me.1 + 0.2, me.2), dir: (0.0, -1.0, 0.0) });
                            }
                        }
                        let d = &player.chr_ins.modules.data;
                        let floor = (d.hp as f32 - d.max_hp as f32 * ECHO_CAP).round() as i32;
                        if floor >= 1 {
                            let until = now + Duration::from_secs_f32(ECHO_AFTER + 0.2 + 1.5);
                            self.art_guard = Some(match self.art_guard {
                                Some((f, u)) => (f.max(floor), u.max(until)),
                                None => (floor, until),
                            });
                        }
                        log!("bosses: npc {} echoes its attack {anim} ({:?})", boss.npc_param_id, kind);
                    }
                }
            }
            // 0.10.0: an art's attacks are out: was the player hit? A clean dodge (or a parry) leaves the
            // boss open to a counter for a moment
            if state.art_until.is_some() && hurt && !parried {
                state.clean = false;
            }
            if parried && (state.art_until.is_some() || state.tell.is_some()) {
                state.clean = true;
                state.art_until = Some(now);
            }
            if let Some(until) = state.art_until {
                if now >= until {
                    state.art_until = None;
                    let near = ((me.0 - bpos.0).powi(2) + (me.2 - bpos.2).powi(2)).sqrt() < 30.0;
                    if state.clean && near {
                        state.exposed_until = Some(now + Duration::from_secs_f32(EXPOSED_FOR));
                        state.next_art = state.next_art.max(now + Duration::from_secs_f32(EXPOSED_FOR + 2.0));
                        world_fx::FX.lock().unwrap().exposed.push((bpos, now + Duration::from_secs_f32(EXPOSED_FOR)));
                        world_fx::banner("OPENING: COUNTER!", [1.0, 0.85, 0.35, 1.0], 1.6);
                        log!("bosses: {h:?} open to a counter");
                    }
                }
            }
            let exposed = state.exposed_until.is_some_and(|t| now < t);
            if !exposed {
                state.exposed_until = None;
            }
            boss.modules.behavior.animation_speed = if crate::parry::WORLD_SLOWED.load(std::sync::atomic::Ordering::Relaxed) {
                crate::parry::SLOW_SPEED
            } else if state.tell.is_some() {
                TELL_SPEED
            } else if exposed {
                // every hit staggers it while it is open
                boss.modules.super_armor.sa_durability = 0.0;
                EXPOSED_SPEED
            } else {
                BOSS_SPEED
            };
            if exposed {
                if let Some(e) = world_fx::FX.lock().unwrap().exposed.last_mut() {
                    e.0 = bpos;
                }
            }
            // a tell runs out: the art strikes
            if let Some((art, at, mark)) = state.tell {
                // the lightning's mark and the comets' lanes follow the player until a moment before they
                // strike, then lock: that is where they go (1.1.0: the comets flew at the player as they
                // were when the comets left, not where the lanes had shown)
                // (the comets' lanes follow right up to the launch: the game aims them at the player then)
                let mark = match art {
                    Art::Lightning if now + Duration::from_secs_f32(LOCK_BEFORE) < at => me,
                    Art::Volley => me,
                    _ => mark,
                };
                state.tell = Some((art, at, mark));
                if matches!(art, Art::Lightning) {
                    world_fx::FX.lock().unwrap().live = (now < at).then_some((mark, r_bolt));
                }
                if matches!(art, Art::Volley) {
                    let mut fx = world_fx::FX.lock().unwrap();
                    fx.live_lanes = if now < at { comet_paths(bpos, mark).iter().map(|(a, b)| (*a, *b, comet_w)).collect() } else { Vec::new() };
                    fx.lanes_locked = false;
                    #[cfg(feature = "lab")]
                    if let Some(l) = fx.live_lanes.first() {
                        log!("lab lanes: from ({:.1},{:.1},{:.1}) to ({:.1},{:.1},{:.1}) w {:.2} me ({:.1},{:.1},{:.1}) locked {}", l.0.0, l.0.1, l.0.2, l.1.0, l.1.1, l.1.2, l.2, me.0, me.1, me.2, fx.lanes_locked);
                    }
                }
                if now >= at {
                    state.tell = None;
                    // (the comets go where their lanes locked: at the mark)
                    let aim = if matches!(art, Art::Volley) { mark } else { me };
                    let (dx, dz) = (aim.0 - bpos.0, aim.2 - bpos.2);
                    let dist = (dx * dx + dz * dz).sqrt();
                    let to_me = if dist > 0.1 { (dx / dist, 0.0, dz / dist) } else { (0.0, 0.0, 1.0) };
                    if matches!(art, Art::Volley) {
                        // the light runs down the locked lanes as the comets come
                        for (a, b) in comet_paths(bpos, mark) {
                            world_fx::marker(Shape::Lane { from: a, to: b, w: comet_w }, tell_of(art).1, 1.1);
                        }
                    }
                    let last = plan(&mut self.shots, art, h, bpos, mark, to_me, now, r_bolt);
                    let d = &player.chr_ins.modules.data;
                    let linger = if matches!(art, Art::Volley) { 2.5 } else { 1.5 };
                    // 1.1.0: the guard keeps one art from taking more than ART_CAP of the bar; with less
                    // than that left, the art may end them
                    let floor = (d.hp as f32 - d.max_hp as f32 * ART_CAP).round() as i32;
                    self.art_guard = (floor >= 1).then(|| (floor, now + Duration::from_secs_f32(last + linger)));
                    state.art_until = Some(now + Duration::from_secs_f32(last + 0.9));
                    state.clean = true;
                    log!("bosses: art {art:?} strikes");
                }
            }

            #[cfg(feature = "lab")]
            if let Some(&(b, name, how)) = CANDIDATES.get(audition) {
                let bp = boss.modules.physics.position;
                let repo = unsafe { SoloParamRepository::instance() }.ok();
                let bullet = repo.as_ref().and_then(|r| r.get::<Bullet>(b as u32));
                let atk = bullet.map(|x| x.atk_id_bullet()).unwrap_or(-2);
                let hit = bullet.map(|x| x.hit_bullet_id()).unwrap_or(-2);
                let npc = repo.as_ref().is_some_and(|r| atk > 0 && r.get::<AtkParam_Npc>(atk as u32).is_some());
                let (pos, dir) = match how {
                    0 => ((me.0, me.1 + 0.2, me.2), (0.0, -1.0, 0.0)),
                    1 => ((me.0, me.1 + 10.0, me.2), (0.0, -1.0, 0.0)),
                    _ => {
                        let (dx, dz) = (me.0 - bp.0, me.2 - bp.2);
                        let l = (dx * dx + dz * dz).sqrt().max(0.1);
                        ((bp.0 + dx / l * 2.0, bp.1 + 1.5, bp.2 + dz / l * 2.0), (dx / l, 0.0, dz / l))
                    }
                };
                let r = ability::spawn(h, -1, b, pos, dir);
                log!("audition {audition}: {b} {name}: atk {atk} (npc row {npc}) hit {hit}: {r:?}");
            }
            if ((BOSS_ARTS && now >= state.next_art && std::env::var_os("JR_NO_ARTS").is_none()) || force) && state.tell.is_none() && state.art_until.is_none() && !exposed {
                let pos = bpos;
                let (dx, dz) = (me.0 - pos.0, me.2 - pos.2);
                let dist = (dx * dx + dz * dz).sqrt();
                if dist < 45.0 {
                    let to_me = if dist > 0.1 { (dx / dist, 0.0, dz / dist) } else { (0.0, 0.0, 1.0) };
                    let mut rng = rand::rng();
                    let art = if force {
                        self.forced += 1;
                        [Art::Shockwave, Art::Lightning, Art::Volley, Art::Cataclysm][(self.forced - 1) % 4]
                    } else if dist < 6.0 {
                        Art::Shockwave
                    } else if low && state.last_cataclysm.is_none_or(|t| t.elapsed() >= CATACLYSM_GAP) && !*state.small_room.get_or_insert_with(|| {
                        let small = small_room(player, bpos);
                        log!("bosses: {h:?} room is {}", if small { "small: no cataclysm" } else { "large" });
                        small
                    }) && (state.last_cataclysm.is_none() || rng.random::<f32>() < 0.5) {
                        // 1.1.0: one as it falls below half, then at most one every CATACLYSM_GAP
                        state.last_cataclysm = Some(now);
                        Art::Cataclysm
                    } else if rng.random::<f32>() < 0.5 {
                        Art::Lightning
                    } else {
                        Art::Volley
                    };
                    // 0.10.0: the tell first: the boss gathers itself in the art's colour, the place it
                    // will hit lights up, and its words show above the health bar
                    let (lead, color, words) = tell_of(art);
                    // 1.1.0: every seal is the spell's own reach (reach_of); its runes light round it and it
                    // is fully lit the moment that part strikes
                    let turn = rng.random::<f32>() * std::f32::consts::TAU;
                    match art {
                        Art::Shockwave => {
                            world_fx::marker(Shape::Sigil { c: pos, r: r_wave, kind: Sigil::Holy, turn }, color, lead);
                            for k in 0..3 {
                                let a = k as f32 / 3.0 * std::f32::consts::TAU;
                                world_fx::marker(Shape::Sigil { c: (pos.0 + a.cos() * WAVE_RING, pos.1, pos.2 + a.sin() * WAVE_RING), r: r_wave, kind: Sigil::Holy, turn: turn + k as f32 }, color, lead + 0.7);
                            }
                            world_fx::marker(Shape::Pillar { c: pos, h: 9.0 }, color, lead);
                        }
                        Art::Lightning => {
                            world_fx::marker(Shape::Pillar { c: pos, h: 9.0 }, color, lead);
                        }
                        Art::Volley => {
                            // (the lanes follow the player while the tell runs: tick, above)
                            world_fx::marker(Shape::Pillar { c: pos, h: 9.0 }, color, lead);
                        }
                        Art::Cataclysm => {
                            for k in 0..5 {
                                let a = k as f32 / 5.0 * std::f32::consts::TAU;
                                world_fx::marker(Shape::Sigil { c: (pos.0 + a.cos() * 3.0, pos.1, pos.2 + a.sin() * 3.0), r: r_floor, kind: Sigil::Holy, turn: turn + k as f32 }, color, lead);
                            }
                            world_fx::marker(Shape::Sigil { c: pos, r: r_blast, kind: Sigil::Cataclysm, turn }, color, lead + BLAST_AFTER);
                            world_fx::marker(Shape::Pillar { c: pos, h: 16.0 }, color, lead + BLAST_AFTER);
                        }
                    }
                    world_fx::banner(words, color, lead + 0.5);
                    state.tell = Some((art, now + Duration::from_secs_f32(lead), me));
                    log!("bosses: art {art:?} at {dist:.0} m (tell {lead:.2}s)");
                }
                let (lo, hi) = if low { (6.0, 10.0) } else { (8.0, 14.0) };
                state.next_art = now + Duration::from_secs_f32(rand::rng().random_range(lo..hi));
            }
        }
        // fire what's due
        let mut i = 0;
        while i < self.shots.len() {
            if now >= self.shots[i].at {
                let s = self.shots.swap_remove(i);
                if let Err(e) = ability::spawn(s.owner, -1, s.bullet, s.pos, s.dir) {
                    log!("bosses: bullet {} failed ({e})", s.bullet);
                }
            } else {
                i += 1;
            }
        }
    }

    /// 1.1.0: the game's "enemies can't die" switch through a boss fight (`on`), and the deaths it holds
    /// back: a boss whose scaled health ran out (tick) and any other enemy a blow left at 1 HP. Their
    /// death goes through with the switch off for a moment (on again once they're dead, or the game
    /// would put them back at 1 HP).
    fn guard_kills(&mut self, on: bool, bosses: &[FieldInsHandle]) {
        let now = Instant::now();
        let Ok(wcm) = (unsafe { WorldChrMan::instance() }) else { return };
        let me = wcm.main_player.as_ref().map(|p| &p.chr_ins as *const ChrIns as usize).unwrap_or(0);
        if on {
            for e in wcm.chr_inses_by_distance.iter().take(64) {
                let key = e.chr_ins.as_ptr() as usize;
                let c = unsafe { &mut *e.chr_ins.as_ptr() };
                if c.modules.data.max_hp <= 0 || bosses.contains(&c.field_ins_handle) {
                    continue;
                }
                if key == me {
                    continue;
                }
                let hp = c.modules.data.hp;
                if let Some(last) = self.others_last.insert(key, hp) {
                    if self.enemy_guard && hp == 1 && last > 1 {
                        c.modules.data.hp = 0;
                        self.killing.push((key, now));
                    }
                }
            }
            // the player (not in that list): the switch holds them too, so a blow that left them at 1 HP
            // was their death, unless one of our own guards (a fighter change, a fall, a boss art's cap)
            // means to keep them
            let wcm_mut = unsafe { WorldChrMan::instance_mut() };
            if let Some(p) = wcm_mut.ok().and_then(|w| w.main_player.as_mut()) {
                let c = &mut p.chr_ins;
                let hp = c.modules.data.hp;
                let held = crate::movement::no_dead_mask() != 0 || std::env::var_os("JR_GOD").is_some();
                let dying = self.killing.iter().any(|(key, _)| *key == me);
                if let Some(last) = self.others_last.insert(me, hp) {
                    // (2.0.6: left at 1 HP by the switch and not already dying: that blow was their death too)
                    if self.enemy_guard && !held && hp == 1 && (last > 1 || !dying) {
                        log!("bosses: a blow that would have killed the player: they die");
                        c.modules.data.hp = 0;
                        self.killing.push((me, now));
                    }
                }
            }
        } else {
            self.others_last.clear();
        }
        // a death is under way once the game has marked it (or after a moment, whatever happens)
        // 2.0.6 (a player's freeze report: "they die", then DIO stood at 1 HP for a minute, unkillable): the game can take
        // longer than 1.5 s to start the player's death (mid-move, mid-air); with the switch back on it put them at 1 HP
        // for good, and a blow from 1 HP never counts as lethal. The player's death keeps the switch off until the game
        // marks it (their health held at 0 meanwhile), for up to PLAYER_DEATH_WAIT.
        if self.killing.iter().any(|(key, _)| *key == me) {
            if let Some(p) = unsafe { WorldChrMan::instance_mut() }.ok().and_then(|w| w.main_player.as_mut()) {
                if !p.chr_ins.chr_flags1c5.death_flag() && p.chr_ins.modules.data.hp > 0 && crate::movement::no_dead_mask() == 0 {
                    p.chr_ins.modules.data.hp = 0;
                }
            }
        }
        self.killing.retain(|(key, at)| {
            let (dead, wait) = if *key == me {
                (wcm.main_player.as_ref().is_none_or(|p| p.chr_ins.chr_flags1c5.death_flag()), PLAYER_DEATH_WAIT)
            } else {
                (by_addr(*key).is_none_or(|c| c.chr_flags1c5.death_flag()), Duration::from_millis(1500))
            };
            if !dead && *key == me && now.duration_since(*at) >= wait {
                log!("bosses: the player's death never started ({} s): the switch goes back on", wait.as_secs());
            }
            !dead && now.duration_since(*at) < wait
        });
        let want = on && self.killing.is_empty();
        if want != self.enemy_guard {
            if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
                f.all_no_dead = want;
            }
            self.enemy_guard = want;
        }
    }

    pub fn reset(&mut self) {
        if self.enemy_guard {
            if let Ok(f) = unsafe { eldenring::cs::WorldChrManDbgFlags::instance_mut() } {
                f.all_no_dead = false;
            }
            self.enemy_guard = false;
        }
        self.killing.clear();
        self.others_last.clear();
        self.phase_one_held.clear();
        self.refilled.clear();
        self.known.clear();
        self.shots.clear();
        self.damage_for_hp = 0;
        self.last_hp = 0;
        self.boss_last_hp.clear();
        if self.art_guard.take().is_some() {
            crate::movement::set_no_dead(crate::movement::NO_DEAD_ART, false);
        }
    }
}

/// Queue an art's attacks; returns when the last one fires (seconds from now).
/// The shockwave art's outer ring of waves (metres from the boss), and when the cataclysm's blast goes off.
const WAVE_RING: f32 = 6.5;
/// 1.1.0: the lightning's mark and the comets' lanes stop following the player this long before they
/// strike (that is where they go).
const LOCK_BEFORE: f32 = 0.3;
const BLAST_AFTER: f32 = 1.4;

fn plan(shots: &mut Vec<Shot>, art: Art, owner: FieldInsHandle, boss: V3, me: V3, to_me: V3, now: Instant, r_bolt: f32) -> f32 {
    let at = |s: f32| now + Duration::from_secs_f32(s);
    let mut push = |t: f32, bullet: i32, pos: V3, dir: V3| shots.push(Shot { at: at(t), owner, bullet, pos, dir });
    match art {
        Art::Shockwave => {
            // a golden shockwave on the boss, then a wider ring of them: back off or roll through
            push(0.0, HOLY_SHOCKWAVE, (boss.0, boss.1 + 0.2, boss.2), to_me);
            for k in 0..3 {
                let a = k as f32 / 3.0 * std::f32::consts::TAU;
                push(0.7, HOLY_SHOCKWAVE, (boss.0 + a.cos() * WAVE_RING, boss.1 + 0.2, boss.2 + a.sin() * WAVE_RING), to_me);
            }
        }
        Art::Lightning => {
            // red lightning on the spot the player stands: it flashes first, so moving dodges it
            push(0.0, RED_LIGHTNING, (me.0, me.1 + 0.2, me.2), (0.0, -1.0, 0.0));
            world_fx::marker(Shape::Sigil { c: me, r: r_bolt, kind: Sigil::Lightning, turn: 0.0 }, tell_of(art).1, 0.8);
        }
        Art::Volley => {
            // both at once, the moment the lanes lock (comet_paths)
            let _ = to_me;
            for (from, to) in comet_paths(boss, me) {
                let (dx, dz) = (to.0 - from.0, to.2 - from.2);
                let l = (dx * dx + dz * dz).sqrt().max(0.1);
                push(0.0, COMET, (from.0, boss.1 + 1.5, from.2), (dx / l, 0.0, dz / l));
            }
        }
        Art::Cataclysm => {
            // the cataclysm: holy floor bursts around the boss as a warning, then the giant explosion
            for k in 0..5 {
                let a = k as f32 / 5.0 * std::f32::consts::TAU;
                push(0.0, HOLY_FLOOR, (boss.0 + a.cos() * 3.0, boss.1 + 0.2, boss.2 + a.sin() * 3.0), to_me);
            }
            push(BLAST_AFTER, GIANT_EXPLOSION, (boss.0, boss.1 + 0.5, boss.2), to_me);
        }
    }
    match art {
        Art::Shockwave => 0.7,
        Art::Lightning => 0.8,
        Art::Volley => 0.0,
        Art::Cataclysm => 1.6,
    }
}
