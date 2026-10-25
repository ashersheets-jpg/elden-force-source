//! The active fighter, their four abilities and how they play (sheets: fighters, abilities,
//! deliveries, stat_profiles; sequences: scripts.rs). One `tick` per frame from the FrameBegin task.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use eldenring::cs::{CSFlipper, PlayerIns};
use eldenring::rotation::Quaternion;
use fromsoftware_shared::FromStatic;

use crate::ability::{self, V3};
use crate::anim::{self, Cue};
use crate::combos;
use crate::audio;
use crate::log;
use crate::motion::{self, Motion};
use crate::scripts::{self, Aim, At, Dest, Step};
use crate::sheets::{self, Ability, Fighter, Slot};

/// Awakening gauge needed for an ultimate.
pub const AWAKENING_FULL: f32 = 100.0;
/// Game speed during an impact freeze.
const HITSTOP_SPEED: f32 = 0.15;
/// Battle shouts: at most one every SHOUT_GAP_S.
const SHOUT_GAP_S: f32 = 1.4;
/// Slam shockwave damage (x attack rating).
const SLAM_DMG: f32 = 1.2;

/// A script being played.
struct Run {
    a: &'static Ability,
    steps: Vec<Step>,
    i: usize,
    /// when the current step began, and shots it has fired
    since: Instant,
    shots: u32,
    motion: Option<Motion>,
    /// direction the move faces, kept for the whole step
    dir: V3,
    /// in the air between an Up and a Down
    aloft: bool,
    ultimate: bool,
    began: Instant,
    /// what the move aims at when nothing is locked on, in block coordinates (stable when the map
    /// re-bases): the nearest enemy in front (followed), or a point a few metres ahead
    anchor: V3,
    follows_enemy: bool,
    /// damage of one hit (written into the bullets; also dealt directly when a bullet can't spawn)
    per_hit: u16,
    /// spawns that failed (pool full) this run
    failed: u32,
    /// 0.10.0: when the last strike / kick began: a motion right after it keeps that animation (a
    /// flying kick carried through the enemy) instead of cutting it for a roll or a jump
    last_act: Option<Instant>,
    /// 1.2.0: a node of the fighter's combo tree (combos.rs), not an ability
    node: Option<&'static combos::Node>,
    /// 1.2.0: the launcher found nobody to lift: the climb after it is skipped
    launch_missed: bool,
    /// 1.3.0: held on this spot (x, z) until then, whatever the animation does (Step::Plant: the Gatling's feet planted)
    plant: Option<((f32, f32), Instant)>,
}

/// 1.2.0: where the fighter is in their combo tree.
#[derive(Default)]
struct Chain {
    /// the node playing (or last played): its kind, its index in its row, when it started, its length
    last: Option<(combos::Kind, usize, Instant, f32)>,
    /// 1.2.0 round 4: the node playing now (or last), and when it began: a press before its window is
    /// too early (combos.rs: ignored after a light or air hit, the string breaks after anything heavier)
    cur: Option<(&'static combos::Node, Instant)>,
    /// a press waiting for the node's window: heavy?, when
    queued: Option<(bool, Instant)>,
    /// air hits since leaving the ground
    air_i: usize,
    /// FP before the node: Ashes of War spend FP the game's way, the combos never do
    fp_floor: Option<i32>,
    /// 1.2.0 round 4: the same for stamina: a node costs its own `cost` only (the Ashes of War its
    /// bodies borrow charge stamina of their own: a light string took 120 instead of 50)
    st_floor: Option<i32>,
    /// until the blow playing now has played out (every node, finishers too)
    busy_until: Option<Instant>,
    /// hits in this string, for the HUD counter
    hits: u32,
    hits_at: Option<Instant>,
    /// 1.2.0 round 4: the swing chord (L3 + RB) was last held then: an RB press next to it is the
    /// swing's, never an attack (user's video: Air Kicks came out between swings)
    chord_at: Option<Instant>,
    /// 1.2.0 round 4: a string broken by a press too early: no air hits until this (the launcher still
    /// plays out and the fighter still goes up, empty-handed)
    no_air_until: Option<Instant>,
}

/// Team types of characters that fight the player (lab: c#### hostiles were team 6; 1.2.0: the common
/// enemies of the open world, Godrick's soldiers and the like at Stormgate, are team 48, and were never
/// targeted or launched).
pub fn hostile(team: u8) -> bool {
    matches!(team, 6 | 7 | 48)
}

/// Havok <-> block coordinates through the player's own current offset between the two.
fn to_block(player: &PlayerIns, p: V3) -> V3 {
    let (h, b) = (player.chr_ins.modules.physics.position, &player.block_position);
    (p.0 - (h.0 - b.x), p.1 - (h.1 - b.y), p.2 - (h.2 - b.z))
}
fn to_havok(player: &PlayerIns, p: V3) -> V3 {
    let (h, b) = (player.chr_ins.modules.physics.position, &player.block_position);
    (p.0 + (h.0 - b.x), p.1 + (h.1 - b.y), p.2 + (h.2 - b.z))
}

/// The living hostile nearest to `near` within `radius` (havok coordinates); with `cone`, only ones
/// in front of `dir` from `near`.
pub fn enemy_near(player: &PlayerIns, near: V3, radius: f32, cone: Option<V3>) -> Option<V3> {
    let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
    let me = &player.chr_ins as *const _ as usize;
    let mut best: Option<(f32, V3)> = None;
    for e in crate::chars::near(wcm, 64).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { e.chr_ins.as_ref() };
        if !hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let (dx, dz) = (p.0 - near.0, p.2 - near.2);
        let d = (dx * dx + dz * dz).sqrt();
        if d > radius || (p.1 - near.1).abs() > 8.0 {
            continue;
        }
        if let Some(f) = cone {
            if d > 0.5 && (dx * f.0 + dz * f.2) / d < 0.35 {
                continue;
            }
        }
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, (p.0, p.1, p.2)));
        }
    }
    best.map(|(_, p)| p)
}

/// The character standing at `at` (havok), by address, if it is still around.
fn chr_at(chr: usize) -> Option<&'static mut eldenring::cs::ChrIns> {
    let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
    crate::chars::near(wcm, 96).iter().find(|e| e.chr_ins.as_ptr() as usize == chr).map(|e| unsafe { &mut *e.chr_ins.as_ptr() })
}

/// The hostile standing at `at` (havok), by address.
fn chr_near(at: V3) -> Option<usize> {
    let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
    crate::chars::near(wcm, 64).iter().find(|e| {
        let c = unsafe { e.chr_ins.as_ref() };
        let p = c.modules.physics.position;
        hostile(c.team_type) && (p.0 - at.0).powi(2) + (p.2 - at.2).powi(2) < 0.25
    }).map(|e| e.chr_ins.as_ptr() as usize)
}

/// Turn the hostile standing at `at` to face along `d` (away from the fighter); its old facing back.
fn turn_away(at: V3, d: V3) -> Option<(usize, eldenring::rotation::Quaternion)> {
    let wcm = unsafe { eldenring::cs::WorldChrMan::instance() }.ok()?;
    for e in crate::chars::near(wcm, 64).iter() {
        let c = unsafe { &mut *e.chr_ins.as_ptr() };
        if !hostile(c.team_type) {
            continue;
        }
        let p = c.modules.physics.position;
        if (p.0 - at.0).powi(2) + (p.2 - at.2).powi(2) > 0.25 {
            continue;
        }
        let old = c.modules.physics.orientation;
        // characters face -Z of their model: yaw so that -Z points along d
        let th = (-d.0).atan2(-d.2);
        let q = eldenring::rotation::Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
        c.modules.physics.orientation = q;
        c.modules.physics.interpolated_orientation = q;
        return Some((e.chr_ins.as_ptr() as usize, old));
    }
    None
}

/// Lab switches for the launch reaction tests (0 turn away, 1 unblockable, 2 armour-breaking poise); on
/// outside the lab.
#[cfg(feature = "lab")]
pub static LAB_SWITCHES: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0b111);
fn lab_switch(bit: u32) -> bool {
    #[cfg(feature = "lab")]
    {
        LAB_SWITCHES.load(std::sync::atomic::Ordering::Relaxed) & (1 << bit) != 0
    }
    #[cfg(not(feature = "lab"))]
    {
        let _ = bit;
        true
    }
}
/// Lab: the combo blows' damage x this / 1000 (enemies survive the tests)
#[cfg(feature = "lab")]
pub static LAB_DMG_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1000);
fn lab_dmg_scale() -> f32 {
    #[cfg(feature = "lab")]
    {
        LAB_DMG_SCALE.load(std::sync::atomic::Ordering::Relaxed) as f32 / 1000.0
    }
    #[cfg(not(feature = "lab"))]
    {
        1.0
    }
}
pub fn lab_poise_on() -> bool {
    lab_switch(2)
}

/// Within this (metres) a blow reaches an enemy on any side of the fighter.
const BODY_CONTACT: f32 = 2.0;

/// 1.3.1: every hostile within `radius` of `at` (any side; ring blows)
fn enemies_within(at: V3, radius: f32, player: &PlayerIns) -> Vec<V3> {
    let Ok(wcm) = (unsafe { eldenring::cs::WorldChrMan::instance() }) else { return Vec::new() };
    let me = &player.chr_ins as *const _ as usize;
    let mut out: Vec<(f32, V3)> = Vec::new();
    for e in crate::chars::near(wcm, 64).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { e.chr_ins.as_ref() };
        if !hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let d = ((p.0 - at.0).powi(2) + (p.2 - at.2).powi(2)).sqrt();
        if d <= radius && (p.1 - at.1).abs() <= 3.0 {
            out.push((d, (p.0, p.1, p.2)));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// 1.3.1 (the lab: Meteor Combination, Chidori, Amaterasu and Tora Gari dealt 0 to a soldier; only bosses' big bodies took them):
/// an ability's blow (STRIKE_HIT, 0.5 m, travelling 0.2 m) at the target was made in the middle of its body moving away and
/// never touched it. Like the combos' blow (Step::Strike): 0.3 m short of the enemy, going into it, from the fighter's side.
fn blow_at(enemy: V3, from: V3, fallback: V3) -> (V3, V3) {
    let (dx, dz) = (enemy.0 - from.0, enemy.2 - from.2);
    let l = (dx * dx + dz * dz).sqrt();
    let d = if l > 0.3 { (dx / l, 0.0, dz / l) } else { fallback };
    ((enemy.0 - d.0 * 0.3, enemy.1 + 1.0, enemy.2 - d.2 * 0.3), d)
}

/// 1.3.1 round 2 (the user: Goku's Raging Strike and Zoro's melee moves "land from super far away (like 30m+)"): their dash is
/// capped at the ability's range, but what they aim at the enemy (the blow, the cuts, the whirlwind) was put on the enemy wherever
/// it stood. A melee ability now lands only on an enemy within its reach of the fighter; past it, the move plays out in front of
/// him. Goku's and Zoro's dash-in moves, and (round 3, the user's list, "the remaining abilities that need their range to be
/// fixed") these, each its reach in metres: Grizzly Magnum's and the Gatling's stretched arms 10 m ("should have a 10 meter range").
/// The other fighters' moves are as they were.
const MELEE_REACH: f32 = 5.5;
const REACHES: [(&str, f32); 14] = [
    ("sasuke_chidori", MELEE_REACH),
    ("sasuke_amaterasu", MELEE_REACH),
    ("luffy_red_hawk", MELEE_REACH),
    ("luffy_grizzly_magnum", 10.0),
    ("luffy_gatling_battle_axe", 10.0),
    ("asta_black_hurricane", BLACK_HURRICANE_R),
    ("yuji_black_flash", MELEE_REACH),
    ("yuji_manji_kick", MELEE_REACH),
    ("todoroki_dancing_flame", MELEE_REACH),
    ("gon_jajanken_rock", MELEE_REACH),
    ("killua_lightning_palm", MELEE_REACH),
    ("jotaro_ora_rush", MELEE_REACH),
    ("dio_muda_rush", MELEE_REACH),
    ("dio_vaporization_freezing", MELEE_REACH),
];
/// (round 3, the user: Black Hurricane "should be 10m AOE": 10 m across, its blows on everyone within 5 m of Asta: scripts.rs)
pub const BLACK_HURRICANE_R: f32 = 5.0;
fn reach_of(run: &Run) -> Option<f32> {
    if run.node.is_some() {
        return None;
    }
    if let Some((_, r)) = REACHES.iter().find(|(id, _)| *id == run.a.id) {
        return Some(*r);
    }
    ((run.a.id.starts_with("goku_") || run.a.id.starts_with("zoro_"))
        && run.steps.iter().any(|s| matches!(s, Step::Move { to: Dest::Target { .. } | Dest::Through { .. }, .. })))
    .then_some(MELEE_REACH)
}

/// 1.2.0 round 4: the hostiles a blow from `from` reaches: within `reach` metres (plus a body's width),
/// inside a 60-degree wedge each side of `facing`, about level with the fighter; nearest first.
pub fn in_reach(player: &PlayerIns, from: V3, facing: V3, reach: f32) -> Vec<V3> {
    let Ok(wcm) = (unsafe { eldenring::cs::WorldChrMan::instance() }) else { return Vec::new() };
    let me = &player.chr_ins as *const _ as usize;
    let mut out: Vec<(f32, V3)> = Vec::new();
    for e in crate::chars::near(wcm, 64).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { e.chr_ins.as_ref() };
        if !hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let (dx, dy, dz) = (p.0 - from.0, p.1 - from.1, p.2 - from.2);
        let d = (dx * dx + dz * dz).sqrt();
        if d > reach + 0.5 || !(-1.6..=2.2).contains(&dy) {
            continue;
        }
        // in body contact (a spinning kick, a flurry that carries the fighter round them) any side counts
        if d > BODY_CONTACT && (dx * facing.0 + dz * facing.2) / d < 0.5 {
            continue;
        }
        out.push((d, (p.0, p.1, p.2)));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    out.into_iter().map(|(_, p)| p).collect()
}

/// 1.3.1: Gojo's Infinity's look: the Opaline Bubbletear's clear bubble round the body, in white (strip_fx.json row_new 9990125)
const INFINITY_LOOK: i32 = 9990125;

pub struct Combat {
    pub fighter: &'static Fighter,
    /// ability ids for "$loadout" slots (Your Tarnished)
    pub loadout: [&'static str; 4],
    cooldown_until: HashMap<&'static str, Instant>,
    run: Option<Run>,
    pub awakening: f32,
    pub last_cast: &'static str,
    /// an ability winding up, and when it releases
    charging: Option<(&'static Ability, Instant)>,
    /// the Awakening aura is on
    awake_aura: bool,
    last_hp: i32,
    ignore_hp_until: Instant,
    ki_carry: f32,
    last_tick: Instant,
    hitstop_until: Option<Instant>,
    /// 1.2.0 round 5: enemies turned away for the frames a launch / air blow lands (on its back no shield
    /// stops it: the reaction is the airborne pose's), turned back after: (character, facing, until)
    turned: Vec<(usize, eldenring::rotation::Quaternion, Instant)>,
    /// 1.2.0 round 5: the launcher's target can't be hurt until just after Giant Hunt's own blow (it did
    /// its full damage on top of the launcher's and killed a level-1 soldier outright; its reaction also
    /// beat ours): (character, until)
    untouchable: Vec<(usize, Instant)>,
    gravity_off: bool,
    invincible: bool,
    /// size multiplier for Grow steps (look applies it with the fighter's height)
    pub scale: f32,
    last_shout: Instant,
    /// 1.1.0: no quips while swinging and a moment after (user: Deku and Luffy's swings need none)
    pub swing_quiet_until: Instant,
    /// Naruto's Substitution Jutsu: ready again at
    substitution_ready: Instant,
    /// JUMP FORCE's parry (parry.rs)
    pub parry: crate::parry::Parry,
    /// the fighter's speed over the ground last frame (m/s), for the wind streaks
    last_pos: Option<(ability::V3, Instant)>,
    /// invincible because of All Might's Symbol of Peace (charging in with a skill)
    passive_guard: bool,
    /// 1.3.0: Gojo's Infinity holds until then (Step::Guard), and whether its aura is on
    infinity_until: Option<Instant>,
    /// 1.3.1 (the user: Gon's adult form "could last longer"): a form body (look.rs GEAR4) and size kept after its move, until then
    form_until: Option<Instant>,
    infinity_aura: bool,
    /// keeps each move's damage to what the fighter select shows
    ledger: crate::ledger::Ledger,
    /// 0.16.0: the fighter's own attacks (RB / LB / RT) deal fixed damage (ledger.rs start_punch)
    punches: crate::ledger::Ledger,
    /// when the last ultimate (Awakening) ended (All Might steams for a while after it)
    ult_ended: Option<Instant>,
    /// 1.2.0: combo trees (combos.rs): the string so far, the finishers' cooldown, the juggled enemies
    chain: Chain,
    finisher_ready: Instant,
    pub juggle: crate::juggle::Juggle,
    /// set by lib.rs from movement every frame: the fighter is in the air / just dodged or sprinting
    pub airborne: bool,
    pub dash_ctx: bool,
    /// 1.2.0: Deku or Luffy is swinging (lib.rs): RB is the swing chord's (L3 + RB), never an attack
    pub swinging: bool,
    /// what the behaviour script was last told about the combo trees (JR_Combos), and when
    combos_told: Option<(bool, Instant)>,
}

/// 0.16.0: a punch's damage x the attack rating (damage::attack_rating): RB / LB and RT. 0.16.1: a
/// third of 0.16.0's (4.1 / 7.15): the user's Apostle fight on Hard was 63% RB mashing. At level 54
/// (rating 259) that is 354 and 617, about 2.7% and 4.7% of Elemer of the Briar (4897 HP) at x2.7.
const PUNCH_LIGHT: f32 = 1.37;
const PUNCH_HEAVY: f32 = 2.38;

impl Combat {
    pub fn new(fighter_id: &str) -> Self {
        let fighter = sheets::fighter(fighter_id).unwrap_or(&sheets::FIGHTERS[0]);
        anim::SWORD.store(fighter.move_weapon.is_some(), std::sync::atomic::Ordering::Relaxed);
        Self {
            fighter,
            loadout: ["goku_kamehameha", "naruto_rasengan", "goku_meteor_combination", "deku_delaware_detroit_smash"],
            cooldown_until: HashMap::new(),
            run: None,
            awakening: 0.0,
            last_cast: "-",
            charging: None,
            awake_aura: false,
            last_hp: -1,
            ignore_hp_until: Instant::now(),
            ki_carry: 0.0,
            last_tick: Instant::now(),
            hitstop_until: None,
            turned: Vec::new(),
            untouchable: Vec::new(),
            gravity_off: false,
            invincible: false,
            scale: 1.0,
            last_shout: Instant::now() - Duration::from_secs(10),
            swing_quiet_until: Instant::now(),
            substitution_ready: Instant::now(),
            parry: Default::default(),
            last_pos: None,
            passive_guard: false,
            infinity_until: None,
            form_until: None,
            infinity_aura: false,
            ledger: Default::default(),
            punches: Default::default(),
            ult_ended: None,
            chain: Chain::default(),
            finisher_ready: Instant::now(),
            juggle: Default::default(),
            airborne: false,
            dash_ctx: false,
            swinging: false,
            combos_told: None,
        }
    }

    /// Forget the last HP reading (max HP changed, e.g. a stat profile switch) so it isn't taken as damage.
    pub fn resync_hp(&mut self) {
        self.last_hp = -1;
        self.ignore_hp_until = Instant::now() + Duration::from_millis(500);
    }

    /// An ability is moving the player (movement leaves them alone).
    pub fn moving(&self) -> bool {
        self.run.as_ref().is_some_and(|r| r.motion.is_some() || r.aloft)
    }

    /// Lab: every ability ready again (comparison recordings cast the same move twice).
    pub fn clear_cooldowns(&mut self) {
        self.cooldown_until.clear();
    }

    pub fn set_fighter(&mut self, id: &str) {
        if let Some(f) = sheets::fighter(id) {
            self.fighter = f;
            anim::SWORD.store(f.move_weapon.is_some(), std::sync::atomic::Ordering::Relaxed);
            self.cooldown_until.clear();
            self.chain = Chain::default();
            self.juggle.release();
            self.combos_told = None;
            // (1.3.1: a kept form (Gon's adult body) is his alone)
            if self.form_until.take().is_some() {
                crate::look::GEAR4.store(false, std::sync::atomic::Ordering::Relaxed);
                self.scale = 1.0;
            }
            log!("fighter: {} ({})", f.name, f.series);
            self.voice(0.9);
        }
    }

    /// Ability in slot 0..4 for the active fighter.
    pub fn ability(&self, slot: usize) -> Option<&'static Ability> {
        let id = self.fighter.abilities[slot];
        let id = if id == "$loadout" { self.loadout[slot] } else { id };
        sheets::ability(id)
    }

    pub fn cooldown_left(&self, a: &Ability) -> f32 {
        self.cooldown_until
            .get(a.id)
            .map(|t| t.saturating_duration_since(Instant::now()).as_secs_f32())
            .unwrap_or(0.0)
    }

    /// The fighter answers when picked: one of their own battle shouts (0.17.0: the same voice for every
    /// player, user: no JUMP FORCE lines mixed in).
    fn voice(&mut self, volume: f32) {
        let id = self.fighter.id;
        self.bark_as(id, "attack", volume);
    }

    /// The ability owner's battle shout, unless one is still playing.
    fn shout(&mut self, a: &Ability, volume: f32) {
        if self.last_shout.elapsed() < Duration::from_secs_f32(SHOUT_GAP_S) {
            return;
        }
        self.bark_as(a.owner, "attack", volume);
    }

    /// 0.17.0: a battle shout in `owner`'s voice from Elden Force's own shout pack (`kind`: "attack" or
    /// "hurt"; audio.rs voices.bin), the same for every player. True if one plays.
    fn bark_as(&mut self, owner: &str, kind: &str, volume: f32) -> bool {
        if Instant::now() < self.swing_quiet_until {
            return false;
        }
        let group = format!("{owner}/{kind}");
        if audio::has_pack(&group) {
            audio::play(&format!("pack:{group}"), audio::Pick::Random, volume);
            self.last_shout = Instant::now();
            return true;
        }
        false
    }

    /// 0.17.0: the active fighter shouts (punches, hits taken), now and then: `chance` of the time,
    /// never over another shout.
    pub fn bark(&mut self, kind: &str, chance: f32, volume: f32) {
        use rand::RngExt;
        if self.last_shout.elapsed() < Duration::from_secs_f32(SHOUT_GAP_S) || rand::rng().random::<f32>() > chance {
            return;
        }
        let id = self.fighter.id;
        self.bark_as(id, kind, volume);
    }

    /// A slam (movement.rs) hit the ground at `pos`: a shockwave that hurts what's around.
    pub fn slam(&mut self, player: &mut PlayerIns, pos: V3) {
        let level = unsafe { player.player_game_data.as_ref().level };
        let dmg = (crate::damage::attack_rating(level) * SLAM_DMG).round().clamp(1.0, 9999.0) as u16;
        crate::damage::set_bullet(scripts::SHATTER_EARTH, "physical", dmg);
        let _ = ability::fire_at(player, scripts::SHATTER_EARTH, (pos.0, pos.1 + 0.4, pos.2), ability::forward(player));
        set_game_speed(HITSTOP_SPEED);
        self.hitstop_until = Some(Instant::now() + Duration::from_secs_f32(0.07));
        log!("slam: {dmg} damage");
    }

    pub fn tick(&mut self, player: &mut PlayerIns) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32().min(0.25);
        self.last_tick = now;
        let profile = sheets::profile(self.fighter.stat_profile);

        // Ki: FP recharges at the profile's rate.
        self.ki_carry += dt * profile.ki_regen_per_s;
        if self.ki_carry >= 1.0 {
            let d = &mut player.chr_ins.modules.data;
            d.fp = (d.fp + self.ki_carry as i32).min(d.max_fp);
            self.ki_carry = self.ki_carry.fract();
        }

        // Asta's sword is in his hand while a move plays
        if self.run.is_some() && self.fighter.move_weapon.is_some() {
            crate::weapon::draw(0.35);
        }

        // hooks.damage_event: HP drops feed the Awakening gauge.
        self.parry.tick(player);
        let hp = player.chr_ins.modules.data.hp;
        // (1.2.0 stress test: a fighter killed mid-ability kept being carried by its moves, the body rising
        // after death) dead: the move stops, gravity, size and invincibility back to normal
        if hp <= 0 && (self.run.is_some() || self.charging.is_some()) {
            log!("ability: stopped, the fighter is dead");
            self.charging = None;
            self.finish_run(player);
        }
        if now < self.ignore_hp_until {
            self.last_hp = -1;
        }
        // 1.3.1: a kept form (Gon's adult body) back to normal when its time is up
        if self.form_until.is_some_and(|t| now >= t) {
            self.form_until = None;
            if self.run.is_none() {
                crate::look::GEAR4.store(false, std::sync::atomic::Ordering::Relaxed);
                self.scale = 1.0;
            }
            log!("ability: {}'s form is over", self.fighter.name);
        }
        // 1.3.0: Gojo's Infinity: invincible until it runs out, his aura on once the move that raised it is over
        if let Some(until) = self.infinity_until {
            if now < until && hp > 0 {
                player.chr_ins.chr_flags1c5.set_is_invincible(true);
                if self.run.is_none() && !self.infinity_aura {
                    let left = until.duration_since(now).as_secs_f32();
                    // (1.3.1, the user: "a white translucent aura" while Infinity is on: the Opaline Bubbletear's pale shell round
                    // him, its look only)
                    if crate::aura::prepare(crate::aura::CHARGE, INFINITY_LOOK, left) {
                        crate::aura::on(player, crate::aura::CHARGE);
                    }
                    self.infinity_aura = true;
                }
            } else {
                self.infinity_until = None;
                if self.infinity_aura {
                    crate::aura::off(player, crate::aura::CHARGE);
                    self.infinity_aura = false;
                }
                if !self.invincible && !self.passive_guard {
                    player.chr_ins.chr_flags1c5.set_is_invincible(false);
                }
                log!("ability: Infinity is over");
            }
        }
        if self.last_hp >= 0 && hp < self.last_hp && hp > 0 && self.parry.on_hit(player, self.last_hp) {
            self.awakening = (self.awakening + 8.0).min(AWAKENING_FULL);
            self.last_hp = player.chr_ins.modules.data.hp;
        }
        let hp = player.chr_ins.modules.data.hp;
        if self.last_hp >= 0 && hp < self.last_hp && hp > 0 && self.fighter.passive == Some("substitution") {
            // Substitution Jutsu: a heavy hit (a fifth of max HP or more) lands on a log instead
            let max = player.chr_ins.modules.data.max_hp;
            if (self.last_hp - hp) * 5 >= max && now >= self.substitution_ready {
                player.chr_ins.modules.data.hp = self.last_hp;
                self.substitution_ready = now + Duration::from_secs(20);
                crate::ui::notice("Substitution Jutsu");
                log!("passive: substitution took a {} damage hit", self.last_hp - hp);
            }
        }
        let hp = player.chr_ins.modules.data.hp;
        if self.last_hp >= 0 && hp < self.last_hp {
            let lost = self.last_hp - hp;
            // (1.2.0 stress test: the fatal hit counted too, a whole bar of HP: every death left the gauge
            // full, the respawn starting with the ultimate ready)
            if hp > 0 {
                self.awakening = (self.awakening + lost as f32 * 0.15).min(AWAKENING_FULL);
            }
            // 0.17.0: a real hit (4% of max HP or more) draws a cry now and then
            if hp > 0 && lost * 25 >= player.chr_ins.modules.data.max_hp && self.fighter.id != "custom" {
                self.bark("hurt", 0.6, 0.85);
            }
        }
        self.last_hp = player.chr_ins.modules.data.hp;

        if let Some((a, at)) = self.charging {
            if now >= at {
                self.charging = None;
                self.deliver(player, a);
            }
        }
        // Awakening aura while the gauge is full.
        let full = self.awakening >= AWAKENING_FULL;
        if full != self.awake_aura {
            if full && crate::aura::prepare(crate::aura::AWAKE, self.fighter.aura_speffect, -1.0) {
                crate::aura::on(player, crate::aura::AWAKE);
                log!("awakening: {} is ready", self.fighter.name);
            } else {
                crate::aura::off(player, crate::aura::AWAKE);
            }
            self.awake_aura = full;
        }
        if let Some(until) = self.hitstop_until {
            if now >= until {
                set_game_speed(1.0);
                self.hitstop_until = None;
            }
        }
        // wind streaks: sprinting (fast over the ground) or in the middle of a move
        let pos = ability::position(player);
        let speed = self.last_pos.map(|(p, t)| {
            let dt = now.duration_since(t).as_secs_f32().max(1e-3);
            ((pos.0 - p.0).powi(2) + (pos.2 - p.2).powi(2)).sqrt() / dt
        }).unwrap_or(0.0);
        self.last_pos = Some((pos, now));
        let striking = self.run.is_some();
        let wind = if striking { 0.8 } else if speed > 40.0 { 0.0 } else { ((speed - 6.0) / 4.0).clamp(0.0, 1.0) };
        crate::ui::set_wind(wind);
        // the launcher's target can be hurt again (our own blow lands right after)
        if !self.untouchable.is_empty() {
            let now = Instant::now();
            self.untouchable.retain(|(chr, until)| {
                if now < *until {
                    return true;
                }
                if let Some(c) = chr_at(*chr) {
                    c.chr_flags1c5.set_is_invincible(false);
                }
                false
            });
        }
        // turned-away enemies face the fighter again
        if !self.turned.is_empty() {
            let now = Instant::now();
            self.turned.retain(|(chr, q, until)| {
                if now < *until {
                    return true;
                }
                if let Some(c) = chr_at(*chr) {
                    c.modules.physics.orientation = *q;
                    c.modules.physics.interpolated_orientation = *q;
                }
                false
            });
        }
        self.run_script(player);
        // 0.16.0: a press of RB / LB / RT (the fighters, not Your Tarnished's own weapons, and not
        // during an ability) starts a punch
        let p = player.chr_ins.modules.action_request.new_action_presses;
        let up_with_it = self.run.as_ref().is_some_and(|r| r.aloft) || self.high_up(player);
        // prototype 4 (user: "deku jittering up and down to reach the enemy"): up with a launched enemy, the
        // fighter holds its height: the air kicks' own motion carried him down every blow, and the next one
        // pulled him back up. Only the hits move anyone now.
        if crate::juggle::Juggle::impact_mode() && self.juggle.holding() && self.run.as_ref().is_some_and(|r| r.aloft) {
            let leaping = self.run.as_ref().is_some_and(|r| matches!(r.steps.get(r.i), Some(Step::Move { to: crate::scripts::Dest::Up { .. }, .. })));
            if let (Some(t), false) = (self.juggle.target(player), leaping) {
                let ph = &mut player.chr_ins.modules.physics;
                let want = t.1 - 0.15;
                let y = ph.position.1 + (want - ph.position.1) * (1.0 - (-14.0 * dt).exp());
                ph.position = eldenring::position::HavokPosition(ph.position.0, y, ph.position.2, ph.position.3);
                ph.chr_proxy_pos_update_requested = true;
                ph.gravity_disabled = true;
                self.gravity_off = true;
            }
        }
        let slow = if self.hitstop_until.is_some() { HITSTOP_SPEED } else { 1.0 };
        self.juggle.tick(player, dt, up_with_it, slow);
        // the behaviour script keeps Elden Ring's own attacks quiet while the fighter has a combo tree
        // (told again every few seconds: a load reloads the script)
        let own = combos::tree(self.fighter.id).is_some();
        if self.combos_told.is_none_or(|(on, t)| on != own || t.elapsed() > Duration::from_secs(3)) {
            // 1.2.0: and whose player script answers: Carian Combo Warriors' (every fighter; Your Tarnished
            // too, the user: "it might make the whole game feel more consistent"), but the game's own for
            // Shadow of the Erdtree's weapon types, which their script (made for 1.10) doesn't know
            let pgd = unsafe { player.player_game_data.as_ref() };
            let ccw = self.fighter.id != "custom" || crate::weapon::right_category(pgd).is_none_or(|c| c <= 52);
            anim::play(player, match (own, ccw) {
                (true, true) => "JR_Combos(true) JR_UseCCW(true)",
                (true, false) => "JR_Combos(true) JR_UseCCW(false)",
                (false, true) => "JR_Combos(false) JR_UseCCW(true)",
                (false, false) => "JR_Combos(false) JR_UseCCW(false)",
            });
            self.combos_told = Some((own, now));
        }
        if let Some(tree) = combos::tree(self.fighter.id) {
            // 1.2.0: the fighters' attacks are their combo trees
            // (LB stays the guard: a press just before a hit is the parry, parry.rs; and RB with L3 held is
            // the swing chord, user's test: Deku and Luffy kept punching while they swung)
            let chord = crate::binds::pad_held(crate::binds::L3) || self.swinging;
            if chord {
                // and a press made a moment before L3 went down (RB first) is dropped with it
                self.chain.chord_at = Some(now);
                self.chain.queued = None;
            }
            let near_chord = self.chain.chord_at.is_some_and(|t| now.duration_since(t) < Duration::from_millis(300));
            if (p.r1() || p.r2()) && !chord && !near_chord {
                self.press(p.r2(), now);
            }
            // a dodge cuts a string short (NieR's dodge cancel), once its blow is out: during the wind-up
            // the fighter is committed (the behaviour script refuses the dodge then, JR_Commit)
            let committed = self.chain.cur.is_some_and(|(n, t)| now.duration_since(t).as_secs_f32() < n.hit_at());
            if (p.sp_move() || p.rolling() || p.backstep()) && !committed && self.run.as_ref().is_some_and(|r| r.node.is_some()) {
                self.finish_run(player);
                self.chain.queued = None;
                self.chain.busy_until = None;
            }
            self.combo_tick(player, tree);
        } else if self.fighter.id != "custom" && self.run.is_none() && (p.r1() || p.l1() || p.r2()) {
            let level = unsafe { player.player_game_data.as_ref().level };
            let rating = crate::damage::attack_rating(level);
            let (label, k) = if p.r2() { ("attack RT", PUNCH_HEAVY) } else if p.l1() { ("attack LB", PUNCH_LIGHT) } else { ("attack RB", PUNCH_LIGHT) };
            self.punches.start_punch(label, (rating * k).round() as i32);
            // 0.17.0: attack shouts: most heavy attacks, some light ones
            self.bark("attack", if p.r2() { 0.7 } else { 0.3 }, 0.9);
        }
        // 1.2.0: a punch counts until its Carian Combo Warriors move is over
        if self.fighter.id != "custom" && (30_000..40_000).contains(&crate::movement::base_anim(crate::movement::current_anim(player))) {
            self.punches.hold(0.4);
        }
        crate::ledger::PLAYER_ATTACKING.store(false, std::sync::atomic::Ordering::Relaxed);
        self.punches.tick(player);
        self.ledger.tick(player);
        // 1.1.0: about how big one blow is now (bosses.rs: a blow stopped at 1 HP was at least this)
        let level = unsafe { player.player_game_data.as_ref().level };
        let (hint, room) = match &self.run {
            Some(r) => (r.per_hit as i32, self.ledger.room()),
            None if self.punches.active() => (self.punches.cap(), self.punches.room()),
            None if self.ledger.active() => (self.ledger.cap() / 4, self.ledger.room()),
            None => ((crate::damage::attack_rating(level) * PUNCH_HEAVY) as i32, i32::MAX / 4),
        };
        crate::bosses::RAW_HINT.store(hint, std::sync::atomic::Ordering::Relaxed);
        crate::bosses::RAW_ROOM.store(room, std::sync::atomic::Ordering::Relaxed);
    }

    /// 1.2.0: seconds until the finishers are ready again.
    pub fn finisher_cooldown(&self) -> f32 {
        self.finisher_ready.saturating_duration_since(Instant::now()).as_secs_f32()
    }

    /// 1.2.0: the combo tree's next node, when a press is waiting and the string allows it now.
    fn combo_tick(&mut self, player: &mut PlayerIns, tree: &'static combos::Tree) {
        let now = Instant::now();
        // the FP an Ash of War spent inside a node comes back (combos cost no Ki)
        let in_node = self.run.as_ref().is_some_and(|r| r.node.is_some());
        if let Some(floor) = self.chain.fp_floor {
            if in_node {
                let d = &mut player.chr_ins.modules.data;
                if d.fp < floor {
                    d.fp = floor.min(d.max_fp);
                }
            } else {
                self.chain.fp_floor = None;
            }
        }
        if let Some(floor) = self.chain.st_floor {
            // (until the node has played out: the game charges an Ash of War's stamina late in it)
            if in_node || self.chain.busy_until.is_some_and(|t| now < t) {
                let d = &mut player.chr_ins.modules.data;
                if d.stamina < floor {
                    d.stamina = floor.min(d.max_stamina);
                }
            } else {
                self.chain.st_floor = None;
            }
        }
        if !self.high_up(player) && !self.run.as_ref().is_some_and(|r| r.aloft) && !(self.airborne && self.juggle.holding()) {
            self.chain.air_i = 0;
        }
        if self.chain.hits_at.is_some_and(|t| t.elapsed() > Duration::from_secs_f32(2.0)) {
            self.chain.hits = 0;
            self.chain.hits_at = None;
        }
        let Some((heavy, at)) = self.chain.queued else { return };
        if at.elapsed() > Duration::from_millis(900) {
            self.chain.queued = None;
            return;
        }
        // Deku's and Luffy's press waits a tenth of a second in case it starts the swing chord (RB, then L3)
        if matches!(self.fighter.id, "deku" | "luffy") && at.elapsed() < Duration::from_millis(100) {
            return;
        }
        // never over an ability or a wind-up; a node's next press waits for its window
        if self.charging.is_some() {
            return;
        }
        if self.run.as_ref().is_some_and(|r| r.node.is_none()) {
            return;
        }
        // the blow before plays out first (its window), even when its script is already done
        if self.chain.busy_until.is_some_and(|t| now < t) {
            return;
        }
        self.chain.queued = None;
        // the string so far (it lapses CHAIN_GRACE_S after a node's window)
        let last = self.chain.last.filter(|(_, _, t, len)| t.elapsed().as_secs_f32() < len + combos::CHAIN_GRACE_S);
        let aloft = self.high_up(player) || self.run.as_ref().is_some_and(|r| r.aloft) || (self.airborne && self.juggle.holding());
        let finisher_ready = now >= self.finisher_ready;
        use combos::Kind;
        let (node, idx) = if aloft {
            if self.chain.no_air_until.is_some_and(|t| now < t) {
                // the string broke on a press too early: nothing in the air until they are down
                return;
            } else if heavy {
                (if finisher_ready { &tree.air_finisher } else { &tree.air_spike }, 0)
            } else if self.chain.air_i >= tree.air.len() {
                // 1.2.0 round 4: three air hits a jump, no loops (the user: no button mashing)
                return;
            } else {
                let i = self.chain.air_i;
                self.chain.air_i += 1;
                (&tree.air[i], i)
            }
        } else if last.is_none() && self.dash_ctx {
            (&tree.dash, 0)
        } else {
            match (last.map(|l| (l.0, l.1)), heavy) {
                (Some((Kind::Light, k)), false) => {
                    let i = (k + 1) % tree.light.len();
                    (&tree.light[i], i)
                }
                (Some((Kind::Light, 0)), true) => (&tree.launcher, 0),
                (Some((Kind::Light, 1)), true) => (&tree.rush, 0),
                (Some((Kind::Light, _)), true) | (Some((Kind::Heavy, 1..)), true) => (if finisher_ready { &tree.finisher } else { &tree.strong }, 0),
                (Some((Kind::Heavy, 0)), true) => (&tree.heavy[1], 1),
                (_, true) => (&tree.heavy[0], 0),
                (_, false) => (&tree.light[0], 0),
            }
        };
        // 1.2.0 round 4: every blow costs stamina, Elden Ring's way: with none left nothing comes out
        let st = player.chr_ins.modules.data.stamina;
        if st <= 0 {
            log!("combo {}: no stamina", node.id);
            if aloft && !heavy {
                self.chain.air_i -= 1;
            }
            return;
        }
        player.chr_ins.modules.data.stamina = (st - node.cost()).max(0);
        self.chain.st_floor = Some((st - node.cost()).max(0));
        if matches!(node.kind, Kind::Finisher | Kind::AirFinisher) {
            self.finisher_ready = now + Duration::from_secs_f32(combos::FINISHER_COOLDOWN_S);
        }
        self.deliver_node(player, node);
        // a finisher (or the strong heavy in its place) ends the string
        let ends = matches!(node.kind, Kind::Finisher | Kind::AirFinisher) || node.id.ends_with("_strong");
        self.chain.last = if ends { None } else { Some((node.kind, idx, now, node.length())) };
        self.chain.cur = Some((node, now));
    }

    /// 1.2.0: really in the air for an air combo: off the ground (movement) with nothing solid within
    /// 1.4 m below (a jump's first moment, a step down or the lab's placing don't count).
    fn high_up(&self, player: &PlayerIns) -> bool {
        if !self.airborne {
            return false;
        }
        let p = player.chr_ins.modules.physics.position;
        motion::ground_below(player, p.0, p.1 + 0.3, p.2, 1.7).is_none()
    }

    /// Play one node of the combo tree: a script like an ability's, its damage the punch ledger's.
    fn deliver_node(&mut self, player: &mut PlayerIns, node: &'static combos::Node) {
        let level = unsafe { player.player_game_data.as_ref().level };
        let dmg = (crate::damage::attack_rating(level) * node.dmg * lab_dmg_scale()).round().max(1.0) as i32;
        let aloft = self.high_up(player) || self.run.as_ref().is_some_and(|r| r.aloft) || (self.airborne && self.juggle.holding());
        let fp = player.chr_ins.modules.data.fp;
        self.finish_run(player);
        self.chain.fp_floor = Some(fp);
        let a = node_ability(node, self.fighter.id);
        self.punches.start_punch(node.id, dmg);
        // the node's effects carry real damage (a muted 1 could round to nothing on a tough enemy);
        // the punch ledger keeps each enemy to exactly `dmg`
        for st in &node.steps {
            if let Step::Fire { b, .. } | Step::Ring { b, .. } = *st {
                crate::damage::set_bullet(b, "physical", dmg.clamp(1, 9999) as u16);
            }
        }
        // 1.2.0 round 4: the blow itself (Step::Strike): its damage, the enemy's own hit reaction, poise
        let (level_hit, poise) = node.reaction();
        let unblockable = matches!(node.kind, combos::Kind::Launcher | combos::Kind::Air) && lab_switch(1);
        crate::damage::set_strike(scripts::STRIKE_HIT, dmg.clamp(1, 9999) as u16, level_hit, poise, unblockable);
        // committed until the blow lands: the behaviour script refuses a dodge until then (60 a second)
        let commit = node.hit_at();
        if commit > 0.03 {
            anim::play(player, &format!("JR_Commit({})", (commit * 60.0).ceil() as u32 + 1));
        }
        let finisher = matches!(node.kind, combos::Kind::Finisher | combos::Kind::AirFinisher);
        if finisher {
            self.awakening = (self.awakening + 6.0).min(AWAKENING_FULL);
        }
        // shouts: most heavies and finishers, some light hits
        let chance = match node.kind {
            combos::Kind::Light | combos::Kind::Air => 0.25,
            _ => 0.6,
        };
        if !node.steps.iter().any(|s| matches!(s, Step::Voice)) {
            self.bark("attack", chance, 0.9);
        }
        let (pos, _, _, _) = self.aim(player, 6.0);
        // 1.2.0 round 4: the blow goes for who is in front (a 60-degree wedge, a little past its reach);
        // only the dash attack closes distance
        let dir = ability::forward(player);
        let reach = if node.kind == combos::Kind::Dash { 9.0 } else { node.reach() + 1.5 };
        let mut found = if aloft { self.juggle.target(player) } else { None }.or_else(|| in_reach(player, pos, dir, reach).first().copied());
        // the launcher's own Ash of War (Giant Hunt) can't hurt who it will launch: only our blow, just after
        if node.kind == combos::Kind::Launcher {
            for e in in_reach(player, pos, dir, node.reach() + 0.8) {
                if let Some(chr) = chr_near(e) {
                    if let Some(c) = chr_at(chr) {
                        c.chr_flags1c5.set_is_invincible(true);
                    }
                    self.untouchable.push((chr, Instant::now() + Duration::from_secs_f32(combos::GH_SAFE_UNTIL)));
                }
            }
        }
        // locked on, the blow turns to the target when it is close, whichever side it is (Elden Ring's way)
        if found.is_none() && player.chr_ins.is_locked_on {
            let t = player.chr_ins.lock_on_target_position;
            if ((t.0 - pos.0).powi(2) + (t.2 - pos.2).powi(2)).sqrt() < 4.5 {
                found = Some((t.0, t.1, t.2));
            }
        }
        // the dash attack goes for the nearest enemy whichever way the dodge left the fighter facing
        if found.is_none() && node.kind == combos::Kind::Dash {
            found = enemy_near(player, pos, reach, None);
        }
        let (anchor, follows_enemy) = match found {
            Some(e) => (e, true),
            None => ((pos.0 + dir.0 * 2.5, pos.1, pos.2 + dir.2 * 2.5), false),
        };
        let anchor = to_block(player, anchor);
        self.run = Some(Run { a, steps: node.steps.clone(), i: 0, since: Instant::now(), shots: 0, motion: None, dir, aloft, ultimate: false, began: Instant::now(), anchor, follows_enemy, per_hit: dmg.clamp(1, u16::MAX as i32) as u16, failed: 0, last_act: None, node: Some(node), launch_missed: false, plant: None });
        self.chain.hits += 1;
        self.chain.hits_at = Some(Instant::now());
        // the node plays out in full before the next one (combos.rs length)
        self.chain.busy_until = Some(Instant::now() + Duration::from_secs_f32(node.length()));
        crate::ui::set_combo(self.chain.hits, node.name, finisher);
        let d = &player.chr_ins.modules.data;
        log!("combo {}: {} ({:?}, {dmg} dmg{}, stamina {}/{})", node.id, node.name, node.kind, if aloft { ", in the air" } else { "" }, d.stamina, d.max_stamina);
    }

    /// Player position, facing toward the lock-on target (or forward), and the target point
    /// (the lock-on target, or `range` metres ahead).
    fn aim(&self, player: &PlayerIns, range: f32) -> (V3, V3, V3, bool) {
        let pos = ability::position(player);
        let mut dir = ability::forward(player);
        let mut target = (pos.0 + dir.0 * range, pos.1, pos.2 + dir.2 * range);
        let locked = player.chr_ins.is_locked_on;
        if locked {
            let t = player.chr_ins.lock_on_target_position;
            let (dx, dz) = (t.0 - pos.0, t.2 - pos.2);
            let len = (dx * dx + dz * dz).sqrt();
            if len > 0.5 && len < 60.0 {
                dir = (dx / len, 0.0, dz / len);
                target = (t.0, t.1, t.2);
            }
        }
        (pos, dir, target, locked)
    }

    /// Try to use the ability in `slot` (0..3 skills, 3 = ultimate).
    pub fn cast(&mut self, player: &mut PlayerIns, slot: usize) {
        let Some(a) = self.ability(slot) else { return };
        self.cast_ability(player, a);
    }

    /// Deku's Black Whip on an enemy: the tendril pulls him in for a Shoot Style flying kick
    /// (abilities: deku_black_whip, not one of the four slots). False when it can't be used now.
    pub fn whip_strike(&mut self, player: &mut PlayerIns) -> bool {
        // 0.14.3: Luffy grabs with his stretched arm the same way (Gum-Gum Rocket)
        let id = if self.fighter.id == "luffy" { "luffy_gum_gum_rocket" } else { "deku_black_whip" };
        let Some(a) = sheets::ability(id) else { return false };
        if self.cooldown_left(a) > 0.0 || player.chr_ins.modules.data.fp < a.fp_cost || self.run.as_ref().is_some_and(|r| r.ultimate) {
            return false;
        }
        self.cast_ability(player, a);
        self.run.as_ref().is_some_and(|r| r.a.id == a.id)
    }

    /// Lab: a combo-tree press (RB light / RT heavy), as if the button went down now.
    #[cfg(feature = "lab")]
    pub fn lab_press(&mut self, heavy: bool) {
        self.press(heavy, Instant::now());
    }

    /// 1.2.0 round 4 (user: "every animation finishes ... if you mistimed it, you get punished"): an attack
    /// press. One before the playing node's blow has landed is too early: ignored after a light or an air
    /// hit, the string broken after anything heavier (combos.rs).
    fn press(&mut self, heavy: bool, now: Instant) {
        let early = self.chain.cur.filter(|(n, t)| now.duration_since(*t).as_secs_f32() < n.window);
        match early {
            Some((n, _)) if n.strict() => {
                if self.chain.last.is_some() || self.chain.no_air_until.is_none_or(|t| now >= t) {
                    log!("combo {}: pressed too early, the string breaks", n.id);
                }
                self.chain.last = None;
                self.chain.queued = None;
                // and no air hits after it (this node, and a moment to come down)
                self.chain.no_air_until = Some(self.chain.cur.map(|(n, t)| t + Duration::from_secs_f32(n.length() + 1.2)).unwrap_or(now));
            }
            Some(_) => {}
            None => self.chain.queued = Some((heavy, now)),
        }
    }

    /// Lab: one of the fighter's own attacks (RB light, RT heavy), its damage counted by the ledger.
    #[cfg(feature = "lab")]
    pub fn measure_attack(&mut self, player: &mut PlayerIns, heavy: bool) {
        let level = unsafe { player.player_game_data.as_ref().level };
        let k = if heavy { PUNCH_HEAVY } else { PUNCH_LIGHT };
        self.punches.start_punch(if heavy { "attack RT" } else { "attack RB" }, (crate::damage::attack_rating(level) * k).round() as i32);
        anim::cue(player, if heavy { anim::Cue::Heavy } else { anim::Cue::Strike });
    }

    /// 0.16.0 (training): the move playing now, by its menu name.
    pub fn current_move(&self) -> Option<&'static str> {
        self.run.as_ref().map(|r| r.node.map(|n| n.name).unwrap_or(r.a.name)).or(self.charging.map(|(a, _)| a.name))
    }

    /// How strongly the fighter's particles burn (0 calm .. 1 in a move).
    pub fn fx_power(&self) -> f32 {
        if self.run.is_some() || self.charging.is_some() {
            1.0
        } else if self.awakening >= AWAKENING_FULL {
            0.55
        } else {
            0.15
        }
    }

    /// 0.12.0: the fighter's particles show now: Deku's always; All Might's steam only for 5 s after
    /// his Awakening (it was distracting in every move).
    pub fn particles_on(&self) -> bool {
        match self.fighter.particles {
            Some("steam") => self.ult_ended.is_some_and(|t| t.elapsed() < Duration::from_secs(5)),
            Some(_) => true,
            None => false,
        }
    }

    /// The enemy a move is playing against (havok), for the whip's tendril.
    pub fn run_target(&self, player: &PlayerIns) -> Option<(&'static str, V3)> {
        let r = self.run.as_ref()?;
        Some((r.a.id, to_havok(player, r.anchor)))
    }

    fn cast_ability(&mut self, player: &mut PlayerIns, a: &'static Ability) {
        // (1.2.0 stress test) no casting from the death screen
        if player.chr_ins.modules.data.hp <= 0 {
            return;
        }
        let now = Instant::now();
        if self.run.as_ref().is_some_and(|r| r.ultimate) {
            return; // nothing interrupts an ultimate
        }
        if self.cooldown_left(a) > 0.0 {
            log!("ability {}: cooling down ({:.1}s)", a.id, self.cooldown_left(a));
            return;
        }
        if a.slot == Slot::Ultimate && self.awakening < AWAKENING_FULL {
            log!("ability {}: Awakening gauge {:.0}/{AWAKENING_FULL}", a.id, self.awakening);
            return;
        }
        let fp = player.chr_ins.modules.data.fp;
        if fp < a.fp_cost {
            log!("ability {}: not enough Ki ({fp}/{})", a.id, a.fp_cost);
            return;
        }
        player.chr_ins.modules.data.fp = fp - a.fp_cost;
        self.last_cast = a.id;
        self.cooldown_until.insert(a.id, now + Duration::from_secs_f32(a.cooldown_s));
        if a.charge_s > 0.0 {
            // Wind-up: the fighter's aura and a JUMP FORCE line, then the release (tick).
            if crate::aura::prepare(crate::aura::CHARGE, self.fighter.aura_speffect, a.charge_s + 0.4) {
                crate::aura::on(player, crate::aura::CHARGE);
            }
            self.voice(1.0);
            self.charging = Some((a, now + Duration::from_secs_f32(a.charge_s)));
            log!("ability {}: charging {:.1}s", a.id, a.charge_s);
        } else {
            self.deliver(player, a);
        }
    }

    /// Start an ability's script (after its wind-up, if any). A new skill cuts the one playing short,
    /// so attacks and movement chain freely.
    fn deliver(&mut self, player: &mut PlayerIns, a: &'static Ability) {
        let Some(script) = scripts::script(a.id) else {
            log!("ability {}: no script", a.id);
            return;
        };
        let level = unsafe { player.player_game_data.as_ref().level };
        let (ids, shots) = script.bullets();
        let hits = if a.hits > 0.0 { a.hits } else { script.expected_hits() };
        let per_hit = crate::damage::apply_bullets(a, &ids, hits, level);
        self.finish_run(player);
        // 0.16.0: the swing grabs always land on the enemy they pull in
        let grab = matches!(a.id, "deku_black_whip" | "luffy_gum_gum_rocket");
        self.ledger.start(a.id, crate::damage::total(a, level).round() as i32, ids.clone(), a.element, a.slot == Slot::Ultimate, grab);
        let ultimate = a.slot == Slot::Ultimate;
        if ultimate {
            self.awakening = 0.0;
            player.chr_ins.chr_flags1c5.set_is_invincible(true);
            self.invincible = true;
        } else {
            self.awakening = (self.awakening + a.fp_cost as f32 * 0.8).min(AWAKENING_FULL);
            // the owner's battle shout on every move (works in Your Tarnished's loadout too)
            // 1.1.0 (user): not on Deku's and Luffy's swing grabs: they come too often
            if !grab {
                self.shout(a, 0.85);
            }
        }
        let (pos, dir, _, _) = self.aim(player, a.range_m);
        // Nothing locked on: the nearest enemy in front, else a point a few metres ahead.
        let reach = if ultimate { 18.0 } else { a.range_m.clamp(6.0, 14.0) };
        let (anchor, follows_enemy) = match enemy_near(player, pos, reach, Some(dir)) {
            Some(e) => (e, true),
            None => {
                let d = if ultimate { 7.0 } else { a.range_m.min(5.0) };
                ((pos.0 + dir.0 * d, pos.1, pos.2 + dir.2 * d), false)
            }
        };
        let anchor = to_block(player, anchor);
        self.run = Some(Run { a, steps: script.steps, i: 0, since: Instant::now(), shots: 0, motion: None, dir, aloft: false, ultimate, began: Instant::now(), anchor, follows_enemy, per_hit, failed: 0, last_act: None, node: None, launch_missed: false, plant: None });
        log!("ability {}: {} steps, {shots} shots, {hits:.1} hits x {per_hit} dmg, awakening {:.0}, ki {}", a.id, self.run.as_ref().unwrap().steps.len(), self.awakening, player.chr_ins.modules.data.fp);
    }

    /// End the playing script: gravity, size, invincibility and game speed back to normal.
    fn finish_run(&mut self, player: &mut PlayerIns) {
        self.ledger.ending(player);
        let keep_form = self.form_until.is_some_and(|t| Instant::now() < t);
        if !keep_form {
            crate::look::GEAR4.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        if self.run.as_ref().is_some_and(|r| r.ultimate) {
            self.ult_ended = Some(Instant::now());
            log!("ability: {}'s Awakening is over", self.fighter.name);
        }
        if self.run.take().is_some() || self.gravity_off {
            if self.gravity_off {
                player.chr_ins.modules.physics.gravity_disabled = false;
                self.gravity_off = false;
            }
        }
        if !keep_form {
            self.scale = 1.0;
        }
        if self.invincible || self.passive_guard {
            player.chr_ins.chr_flags1c5.set_is_invincible(false);
            self.invincible = false;
            self.passive_guard = false;
        }
        crate::aura::off(player, crate::aura::CHARGE);
        crate::aura::off(player, crate::aura::PALM);
        crate::aura::off(player, crate::aura::CHIDORI_BLADE);
        crate::aura::off(player, crate::aura::AMATERASU_BLADE);
        crate::aura::off(player, crate::aura::DARK_BLADE);
        anim::clear(player);
    }

    /// Leaving the world: drop everything in flight.
    pub fn on_unload(&mut self) {
        self.juggle.forget();
        self.combos_told = None;
        self.chain = Chain::default();
        self.parry.reset();
        self.last_pos = None;
        self.run = None;
        self.charging = None;
        self.gravity_off = false;
        self.invincible = false;
        self.passive_guard = false;
        self.infinity_until = None;
        self.infinity_aura = false;
        self.form_until = None;
        self.scale = 1.0;
        if self.hitstop_until.take().is_some() {
            set_game_speed(1.0);
        }
    }

    fn run_script(&mut self, player: &mut PlayerIns) {
        let Some(run) = &mut self.run else { return };
        // a script never runs longer than this, whatever happens
        if run.i >= run.steps.len() || run.began.elapsed() > Duration::from_secs(8) {
            self.finish_run(player);
            return;
        }
        let a = run.a;
        let range = a.range_m.max(6.0);
        if let Some(((x, z), until)) = run.plant {
            if Instant::now() < until {
                let ph = &mut player.chr_ins.modules.physics;
                if (ph.position.0 - x).abs() > 0.01 || (ph.position.2 - z).abs() > 0.01 {
                    ph.position = eldenring::position::HavokPosition(x, ph.position.1, z, ph.position.3);
                    ph.chr_proxy_pos_update_requested = true;
                }
            } else {
                run.plant = None;
            }
        }
        let step = run.steps[run.i];
        let elapsed = run.since.elapsed().as_secs_f32();
        // All Might's Symbol of Peace: nothing hurts him while he charges in with a skill
        let guard = self.fighter.passive == Some("unbreakable") && !run.ultimate && matches!(step, Step::Move { .. });
        if guard != self.passive_guard {
            self.passive_guard = guard;
            if !self.invincible {
                player.chr_ins.chr_flags1c5.set_is_invincible(guard);
            }
        }
        let (pos, aim_dir, target, locked) = {
            let pos = ability::position(player);
            let mut target = to_havok(player, run.anchor);
            if run.follows_enemy {
                // keep following that enemy as it moves
                if let Some(e) = enemy_near(player, target, 4.0, None) {
                    target = e;
                    run.anchor = to_block(player, e);
                }
            }
            let mut locked = run.follows_enemy;
            // (1.2.0 round 4: a combo blow keeps to the enemy it picked; the lock-on only steers the rest)
            if player.chr_ins.is_locked_on && !(run.node.is_some() && run.follows_enemy) {
                let t = player.chr_ins.lock_on_target_position;
                if ((t.0 - pos.0).powi(2) + (t.2 - pos.2).powi(2)).sqrt() < 60.0 {
                    target = (t.0, t.1, t.2);
                    locked = true;
                }
            }
            let (dx, dz) = (target.0 - pos.0, target.2 - pos.2);
            let len = (dx * dx + dz * dz).sqrt();
            let dir = if len > 0.5 { (dx / len, 0.0, dz / len) } else { run.dir };
            (pos, dir, target, locked)
        };
        // (1.3.1 round 2) where a melee ability's hits go: on the enemy in reach, else just ahead (REACHES)
        let hit_target = if reach_of(run).is_some_and(|r| ((target.0 - pos.0).powi(2) + (target.2 - pos.2).powi(2)).sqrt() > r) {
            (pos.0 + aim_dir.0 * 1.5, pos.1, pos.2 + aim_dir.2 * 1.5)
        } else {
            target
        };
        let mut next = false;
        let mut hitstop = None;
        if run.launch_missed && matches!(step, Step::Move { to: Dest::Up { .. }, .. } | Step::Hover(_)) {
            run.i += 1;
            run.since = Instant::now();
            run.shots = 0;
            run.motion = None;
            return;
        }
        match step {
            // (1.3.1) a drop to the ground while already standing on it: nothing to do (it played a fall pose first)
            Step::Move { to: Dest::Drop, .. } if run.motion.is_none() && run.shots == 0 && player.chr_ins.modules.physics.is_touching_ground
                && motion::ground_below(player, pos.0, pos.1 + 0.3, pos.2, 0.6).is_some() => {
                next = true;
            }
            Step::Move { to, dur, arc } => {
                if run.motion.is_none() && run.shots == 0 {
                    run.shots = 1; // started
                    let (tx, tz) = (target.0 - pos.0, target.2 - pos.2);
                    let tlen = (tx * tx + tz * tz).sqrt().max(0.01);
                    let flat = (tx / tlen, 0.0, tz / tlen);
                    let side = (-flat.2, 0.0, flat.0);
                    let total = match to {
                        Dest::Target { gap } => {
                            let d = (tlen - gap).clamp(0.0, range);
                            (flat.0 * d, if run.aloft { target.1 - pos.1 } else { 0.0 }, flat.2 * d)
                        }
                        Dest::Through { past, side: s } => {
                            let d = (tlen + past).min(range + past);
                            (flat.0 * d + side.0 * s, 0.0, flat.2 * d + side.2 * s)
                        }
                        Dest::Forward(d) => (aim_dir.0 * d, 0.0, aim_dir.2 * d),
                        Dest::Back(d) => (-aim_dir.0 * d, 0.0, -aim_dir.2 * d),
                        Dest::Up { height, toward } => {
                            run.aloft = true;
                            let d = (tlen * toward).min(range);
                            (flat.0 * d, height, flat.2 * d)
                        }
                        Dest::Near { gap, max } => {
                            let dy = if run.aloft { target.1 - pos.1 } else { 0.0 };
                            if (tlen * tlen + dy * dy).sqrt() > max {
                                (0.0, 0.0, 0.0)
                            } else {
                                let d = (tlen - gap).max(0.0);
                                (flat.0 * d, dy, flat.2 * d)
                            }
                        }
                        Dest::Drop => {
                            run.aloft = false;
                            let gy = motion::ground_below(player, pos.0, pos.1, pos.2, 40.0).unwrap_or(pos.1 - 2.0);
                            (0.0, gy - pos.1, 0.0)
                        }
                        Dest::Down => {
                            run.aloft = false;
                            let gy = motion::ground_below(player, target.0, pos.1, target.2, 40.0).or_else(|| motion::ground_below(player, pos.0, pos.1, pos.2, 40.0)).unwrap_or(pos.1 - 2.0);
                            let (dx, dz) = if locked { (target.0 - pos.0, target.2 - pos.2) } else { (aim_dir.0 * 3.0, aim_dir.2 * 3.0) };
                            (dx, gy - pos.1, dz)
                        }
                    };
                    let hlen = (total.0 * total.0 + total.2 * total.2).sqrt();
                    if hlen > 0.3 {
                        run.dir = (total.0 / hlen, 0.0, total.2 / hlen);
                        face(player, run.dir);
                    }
                    let air = run.aloft || arc > 0.0 || total.1.abs() > 0.5 || matches!(to, Dest::Down | Dest::Drop);
                    // the body moves too: a leap, a dive, or a lunge / air dash (anim.rs)
                    let striking = run.last_act.is_some_and(|t| t.elapsed() < Duration::from_millis(400)) || (run.node.is_some() && !matches!(to, Dest::Up { .. } | Dest::Drop));
                    if !striking {
                        anim::cue(player, match to {
                            Dest::Up { .. } => Cue::Jump,
                            Dest::Down => Cue::Dive,
                            _ if arc > 1.0 => Cue::Jump,
                            // 0.10.0: lunges lean into an anime dash instead of rolling
                            _ => Cue::AirDash,
                        });
                    }
                    run.motion = Some(if matches!(to, Dest::Down | Dest::Drop) {
                        Motion::straight(total, dur)
                    } else if air {
                        Motion::air(total, arc, dur)
                    } else {
                        Motion::dash(total, dur)
                    });
                }
                let airborne = run.motion.as_ref().is_some_and(|m| m.airborne()) || run.aloft;
                if airborne != self.gravity_off {
                    player.chr_ins.modules.physics.gravity_disabled = airborne;
                    self.gravity_off = airborne;
                }
                if run.motion.as_mut().is_some_and(|m| m.step(player)) {
                    run.motion = None;
                    next = true;
                }
            }
            Step::Hover(secs) => {
                if !self.gravity_off {
                    player.chr_ins.modules.physics.gravity_disabled = true;
                    self.gravity_off = true;
                }
                next = elapsed >= secs;
            }
            Step::Fire { b, at, aim, n, every, spread, side } => {
                let target = hit_target;
                let due = if every <= 0.0 { n } else { ((elapsed / every) as u32 + 1).min(n) };
                if run.shots < due {
                    let freed = ability::make_room(scripts::all_bullets());
                    if freed > 0 {
                        log!("ability {}: bullet pool nearly full ({}), ended {freed} old ones", a.id, ability::pool_count());
                    }
                }
                while run.shots < due {
                    let k = run.shots;
                    let mut d = if spread != 0.0 { rotate(aim_dir, (k as f32 - (n - 1) as f32 / 2.0) * spread) } else { aim_dir };
                    let lateral = if side != 0.0 { side * if k % 2 == 0 { 0.5 } else { -0.5 } * (1 + k / 2 % 2) as f32 } else { 0.0 };
                    let mut p = place(player, at, pos, d, target, k);
                    let perp = (-d.2, 0.0, d.0);
                    p = (p.0 + perp.0 * lateral, p.1 + (k % 3) as f32 * 0.15 * (side != 0.0) as u8 as f32, p.2 + perp.2 * lateral);
                    d = direction(aim, d, p, target);
                    // (1.3.1) the blow at the enemy: put at its edge, going in
                    if b == scripts::STRIKE_HIT && matches!(at, At::Target | At::NearTarget(_)) {
                        if let Some(e) = enemy_near(player, target, 2.5, None) {
                            (p, d) = blow_at(e, pos, aim_dir);
                        }
                    }
                    if let Err(e) = ability::fire_at(player, b, p, d) {
                        // no room for the effect: the hit still lands on the enemy it was aimed at
                        run.failed += 1;
                        let hit = hurt_near(player, if matches!(at, At::Feet) { p } else { target }, 3.5, run.per_hit);
                        if run.failed <= 3 {
                            log!("ability {}: bullet {b} spawn failed ({e}, pool {}), dealt {hit} directly; {}", a.id, ability::pool_count(), ability::pool_stats());
                        }
                    }
                    run.shots += 1;
                }
                next = run.shots >= n && elapsed >= every * n.saturating_sub(1) as f32;
            }
            Step::Ring { b, at, n, r, aim } => {
                let target = hit_target;
                let c = place(player, at, pos, aim_dir, target, 0);
                // (1.3.1) a ring of blows: one on each enemy inside it, at its edge going in (on the ring itself they flew outward,
                // away from everyone)
                let blows = b == scripts::STRIKE_HIT;
                if blows {
                    for e in enemies_within(c, r + 0.8, player).into_iter().take(n.max(1) as usize) {
                        let (p, d) = blow_at(e, pos, aim_dir);
                        if ability::fire_at(player, b, p, d).is_err() {
                            hurt_near(player, e, 1.0, run.per_hit);
                        }
                    }
                }
                for k in 0..if blows { 0 } else { n } {
                    let ang = k as f32 / n as f32 * std::f32::consts::TAU;
                    let out = (ang.cos(), 0.0, ang.sin());
                    let p = (c.0 + out.0 * r, c.1, c.2 + out.2 * r);
                    let d = match aim {
                        Aim::Out => out,
                        other => direction(other, aim_dir, p, target),
                    };
                    if ability::fire_at(player, b, p, d).is_err() {
                        run.failed += 1;
                        if k == 0 {
                            hurt_near(player, c, r + 1.0, run.per_hit);
                        }
                    }
                }
                next = true;
            }
            Step::Wait(secs) => next = elapsed >= secs,
            Step::Blade(row, on) => {
                if on {
                    if crate::aura::prepare(row, row, 10.0) {
                        crate::aura::on(player, row);
                    }
                } else {
                    crate::aura::off(player, row);
                }
                next = true;
            }
            Step::Aura(on) => {
                if on {
                    if crate::aura::prepare(crate::aura::CHARGE, self.fighter.aura_speffect, 6.0) {
                        crate::aura::on(player, crate::aura::CHARGE);
                    }
                } else {
                    crate::aura::off(player, crate::aura::CHARGE);
                }
                next = true;
            }
            Step::Voice => {
                // 1.1.0: the swing grabs are quiet (user)
                if !matches!(a.id, "deku_black_whip" | "luffy_gum_gum_rocket") {
                    self.bark_as(a.owner, "attack", 1.0);
                }
                next = true;
            }
            Step::HitStop(secs) => {
                hitstop = Some(secs);
                next = true;
            }
            Step::TimeStop(secs) => {
                crate::timestop::start(secs);
                // 1.3.0 (the anime's time stop): a sphere bursts out of the fighter (Rejection's ripple for Star Platinum,
                // Wrath of Gold for The World, no damage of their own) and the stopped world takes on a cold, drained tint
                let gold = self.fighter.id == "dio";
                let chest = (pos.0, pos.1 + 1.0, pos.2);
                let _ = ability::fire_at(player, if gold { crate::scripts::TIME_DOME_GOLD } else { crate::scripts::TIME_DOME }, chest, (aim_dir.0, 0.0, aim_dir.2));
                crate::ui::time_stop(secs, if gold { [1.0, 0.82, 0.3, 1.0] } else { [0.68, 0.5, 1.0, 1.0] });
                next = true;
            }
            Step::Grow(m) => {
                self.scale = m;
                next = true;
            }
            Step::Act(c) => {
                anim::cue(player, c);
                run.last_act = Some(Instant::now());
                next = true;
            }
            Step::Weapon { row, lua, secs } | Step::WeaponShown { row, lua, secs, .. } => {
                // the hands become the weapon first (a frame or two), then its attack plays
                match step {
                    Step::WeaponShown { model, .. } => crate::weapon::set_move_model(row, model, secs),
                    _ => crate::weapon::set_move(row, secs),
                }
                // (a fifth of a second at most: the move plays even if the dressing hasn't been seen)
                let ready = crate::weapon::move_applied(row);
                if ready || elapsed > 0.2 {
                    #[cfg(feature = "lab")]
                    log!("lab weapon step: row {row} ready {ready} after {elapsed:.2}s");
                    anim::play(player, lua);
                    run.last_act = Some(Instant::now());
                    next = true;
                }
            }
            Step::Launch(h) => {
                // only who the blow reached goes up (never the lock-on target across the field)
                let facing = ability::forward(player);
                let reach = run.node.map(|n| n.reach()).unwrap_or(2.6);
                let launched = match in_reach(player, pos, facing, reach).first() {
                    Some(&e) => self.juggle.launch(player, e, 0.6, h),
                    None => false,
                };
                if launched {
                    // the fighter goes up after it
                    run.aloft = true;
                } else {
                    // nothing went up (a boss, a big enemy, nobody): no climb after it
                    run.launch_missed = true;
                }
                next = true;
            }
            Step::FaceTarget => {
                // 1.2.0 round 4: Elden Ring's attack tracking: the wind-up turns to the nearest enemy in
                // front (within 60 degrees and 4 m), or the one held in the air
                let facing = ability::forward(player);
                // (the enemy this blow went for, while it is close, wherever the body has carried them)
                let fighting = (run.follows_enemy && ((target.0 - pos.0).powi(2) + (target.2 - pos.2).powi(2)).sqrt() < 4.0).then_some(target);
                let t = if run.aloft { self.juggle.target(player) } else { None }.or(fighting).or_else(|| in_reach(player, pos, facing, 4.0).first().copied());
                if let Some(e) = t {
                    let (dx, dz) = (e.0 - pos.0, e.2 - pos.2);
                    let l = (dx * dx + dz * dz).sqrt();
                    if l > 0.3 {
                        let d = (dx / l, 0.0, dz / l);
                        run.dir = d;
                        face(player, d);
                    }
                }
                next = true;
            }
            Step::Strike => {
                if crate::juggle::Juggle::impact_mode() {
                    self.juggle.open();
                }
                // 1.2.0 round 4 (user: "every attack must land as predicted every time"): the blow lands
                // on everyone inside its reach, ahead of the fighter, and on the enemy held in the air;
                // the hitbox is put on each of them, so it can't pass over or under
                let facing = ability::forward(player);
                let reach = run.node.map(|n| n.reach()).unwrap_or(2.4);
                // (the enemy held in the air only when it really is within reach: no blow lands from below it)
                let mut hits: Vec<V3> = if run.aloft {
                    self.juggle.positions().into_iter().filter(|e| ((e.0 - pos.0).powi(2) + (e.1 + 0.9 - pos.1 - 0.9).powi(2) + (e.2 - pos.2).powi(2)).sqrt() <= reach + 0.8).collect()
                } else {
                    Vec::new()
                };
                let mut near = in_reach(player, pos, facing, reach);
                // the air finisher reaches the enemy it slams down, whatever side and a little below
                if run.node.is_some_and(|n| n.kind == combos::Kind::AirFinisher) {
                    if let Some(e) = enemy_near(player, pos, 3.0, None) {
                        if (-3.0..=2.2).contains(&(e.1 - pos.1)) {
                            near.push(e);
                        }
                    }
                }
                for e in near {
                    if !hits.iter().any(|h| (h.0 - e.0).powi(2) + (h.1 - e.1).powi(2) + (h.2 - e.2).powi(2) < 0.36) {
                        hits.push(e);
                    }
                }
                ability::make_room(scripts::all_bullets());
                if hits.is_empty() {
                    // a whiff (a pot or a crate in front still breaks)
                    let at = (pos.0 + facing.0 * 0.95, pos.1 + 1.0, pos.2 + facing.2 * 0.95);
                    let _ = ability::fire_at(player, scripts::STRIKE_HIT, at, facing);
                } else {
                    for e in hits.iter().take(4) {
                        let (dx, dz) = (e.0 - pos.0, e.2 - pos.2);
                        let l = (dx * dx + dz * dz).sqrt();
                        let d = if l > 0.3 { (dx / l, 0.0, dz / l) } else { facing };
                        // 1.2.0 round 5: the launch and the air blows reach them from behind (a shield in front
                        // blocked the launch: the soldier played its guard instead of the airborne reaction);
                        // everything else from the fighter's side
                        let behind = run.node.is_some_and(|n| matches!(n.kind, combos::Kind::Launcher | combos::Kind::Air));
                        if behind && lab_switch(0) {
                            // turned away from the fighter for the frames the blow lands
                            if let Some((chr, q)) = turn_away(*e, d) {
                                self.turned.push((chr, q, Instant::now() + Duration::from_millis(70)));
                            }
                        }
                        let (at, d) = if behind { ((e.0 + d.0 * 0.35, e.1 + 1.0, e.2 + d.2 * 0.35), (-d.0, 0.0, -d.2)) } else { ((e.0 - d.0 * 0.3, e.1 + 1.0, e.2 - d.2 * 0.3), d) };
                        if ability::fire_at(player, scripts::STRIKE_HIT, at, d).is_err() {
                            hurt_near(player, *e, 1.0, run.per_hit);
                        }
                    }
                }
                if hits.is_empty() {
                    // why not: the nearest hostile, how far and how far round
                    let near = enemy_near(player, pos, 8.0, None).map(|e| {
                        let (dx, dz) = (e.0 - pos.0, e.2 - pos.2);
                        let d = (dx * dx + dz * dz).sqrt().max(1e-3);
                        format!("nearest {d:.1} m, {:.0} deg off, {:+.1} m up", ((dx * facing.0 + dz * facing.2) / d).clamp(-1.0, 1.0).acos().to_degrees(), e.1 - pos.1)
                    });
                    log!("strike {}: nobody in reach ({:.1} m; {})", run.node.map(|n| n.id).unwrap_or(a.id), reach, near.unwrap_or_else(|| "nobody near".into()));
                } else {
                    log!("strike {}: {} in reach ({:.1} m)", run.node.map(|n| n.id).unwrap_or(a.id), hits.len(), reach);
                }
                next = true;
            }
            Step::HeadFx(on) => {
                crate::aura::off(player, crate::aura::PALM);
                if on && crate::aura::palm_prepare(crate::aura::HEAD_CRACKLE, crate::aura::HEAD_DMY) {
                    crate::aura::on(player, crate::aura::PALM);
                }
                next = true;
            }
            Step::PalmFx(on) => {
                crate::aura::off(player, crate::aura::PALM);
                if on && crate::aura::palm_prepare(crate::aura::PALM_CHARGE, crate::aura::PALM_DMY) {
                    crate::aura::on(player, crate::aura::PALM);
                }
                next = true;
            }
            Step::Beam { b, min } => {
                let dist = ((target.0 - pos.0).powi(2) + (target.2 - pos.2).powi(2)).sqrt();
                if dist < min {
                    // skip the beam itself too
                    run.i += 1;
                } else {
                    use eldenring::cs::{Bullet, SoloParamRepository};
                    if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                        if let Some(r) = repo.get_mut::<Bullet>(b as u32) {
                            let v = r.init_vellocity().max(1.0);
                            r.set_life((dist / v).clamp(0.05, 1.5));
                        }
                    }
                }
                next = true;
            }
            Step::Lob { b, at } => {
                let s = place(player, at, pos, aim_dir, target, 0);
                let chest = (target.0, target.1 + 1.0, target.2);
                let (dx, dy, dz) = (chest.0 - s.0, chest.1 - s.1, chest.2 - s.2);
                let dist = (dx * dx + dy * dy + dz * dz).sqrt().max(0.5);
                use eldenring::cs::{Bullet, SoloParamRepository};
                if let Ok(repo) = unsafe { SoloParamRepository::instance_mut() } {
                    if let Some(r) = repo.get_mut::<Bullet>(b as u32) {
                        let v = r.init_vellocity().max(1.0);
                        r.set_life((dist / v).clamp(0.1, 6.0));
                    }
                }
                if ability::fire_at(player, b, s, (dx / dist, dy / dist, dz / dist)).is_err() {
                    run.failed += 1;
                    hurt_near(player, chest, 5.0, run.per_hit);
                }
                next = true;
            }
            Step::KeepForm(secs) => {
                self.form_until = Some(Instant::now() + Duration::from_secs_f32(secs));
                next = true;
            }
            Step::Gear4(on) => {
                crate::look::GEAR4.store(on, std::sync::atomic::Ordering::Relaxed);
                next = true;
            }
            Step::Stand(secs, what) => {
                if crate::stands::READY.load(std::sync::atomic::Ordering::Relaxed) {
                    *crate::stands::REQUEST.lock().unwrap() = Some((secs, what));
                } else {
                    // no Stand at hand yet (just arrived, or its call failed): the fighter throws the blow himself
                    let cue = match what {
                        crate::stands::ORA | crate::stands::MUDA => Some(Cue::Arts(crate::scripts::REPEATING_THRUST)),
                        crate::stands::THRUST => Some(Cue::Strike),
                        crate::stands::HEAVY => Some(Cue::Heavy),
                        crate::stands::ROAR => Some(Cue::PowerUp),
                        _ => None,
                    };
                    if let Some(c) = cue {
                        anim::cue(player, c);
                        run.last_act = Some(Instant::now());
                    }
                }
                next = true;
            }
            Step::Clone => {
                crate::clones::REQUEST.store(true, std::sync::atomic::Ordering::Relaxed);
                next = true;
            }
            Step::Poof(at) => {
                crate::world_fx::poof(place(player, at, pos, aim_dir, target, 0));
                next = true;
            }
            Step::Stun(secs) => {
                // (1.3.1 round 2: Stun Grenade's close blast, 6 m across 2 m ahead of him: 4.5 m, was 8)
                crate::timestop::start_within(secs, 4.5);
                next = true;
            }
            Step::Guard(secs) => {
                self.infinity_until = Some(Instant::now() + Duration::from_secs_f32(secs));
                self.infinity_aura = false;
                log!("ability: Infinity for {secs:.1} s");
                next = true;
            }
            Step::Plant(secs) => {
                run.plant = Some(((pos.0, pos.2), Instant::now() + Duration::from_secs_f32(secs)));
                next = true;
            }
            Step::RubberFist(set, side, off) => {
                let target = hit_target;
                // from the shoulder to the front of their chest (a little short of its middle), moved `off` across it: the
                // reach closest to that
                let s = place(player, At::Shoulder(side), pos, aim_dir, target, 0);
                let right = (aim_dir.2, 0.0, -aim_dir.0);
                let chest = (target.0 + right.0 * off.0, target.1 + 1.25 + off.1, target.2 + right.2 * off.0);
                let (dx, dy, dz) = (chest.0 - s.0, chest.1 - s.1, chest.2 - s.2);
                let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                let want = (dist - 0.25).max(0.8);
                let (_, b) = scripts::FIST_SETS[set as usize].iter().copied().min_by(|a, b| (a.0 - want).abs().total_cmp(&(b.0 - want).abs())).unwrap();
                let d = if dist > 0.01 { (dx / dist, dy / dist, dz / dist) } else { aim_dir };
                if ability::fire_at(player, b, s, d).is_err() {
                    run.failed += 1;
                }
                next = true;
            }
            Step::Flash => {
                let at = (pos.0 + aim_dir.0 * 1.0, pos.1 + 1.2, pos.2 + aim_dir.2 * 1.0);
                crate::world_fx::impact(at, crate::world_fx::color_of(self.fighter.particles), true);
                next = true;
            }
            Step::Juggle(secs) => {
                self.juggle.extend(secs);
                if crate::juggle::Juggle::impact_mode() {
                    // prototype 4: the blow knocks it; it never comes to the fighter
                    self.juggle.hit(pos);
                } else {
                // 1.2.0 (user: the air hits rarely landed, the enemy hung too low): the held enemy is
                // drawn in front of the fighter at their height for the blow
                let front = (pos.0 + aim_dir.0 * 1.05, pos.1, pos.2 + aim_dir.2 * 1.05);
                self.juggle.bring(player, front);
                }
                next = true;
            }
            Step::Spike => {
                self.juggle.spike();
                next = true;
            }
        }
        if let Some(secs) = hitstop {
            set_game_speed(HITSTOP_SPEED);
            self.hitstop_until = Some(Instant::now() + Duration::from_secs_f32(secs));
        }
        if next {
            if let Some(run) = &mut self.run {
                run.i += 1;
                run.since = Instant::now();
                run.shots = 0;
                run.motion = None;
            }
        }
    }
}

/// Hurt the living hostile nearest to `at` (within `radius`) by `amount`, as if a bullet had hit it.
/// Used when the game's bullet pool is full. Returns the damage dealt.
fn hurt_near(player: &PlayerIns, at: V3, radius: f32, amount: u16) -> i32 {
    let Ok(wcm) = (unsafe { eldenring::cs::WorldChrMan::instance() }) else { return 0 };
    let me = &player.chr_ins as *const _ as usize;
    let mut best: Option<(f32, *mut eldenring::cs::ChrIns)> = None;
    for e in crate::chars::near(wcm, 64).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { &mut *e.chr_ins.as_ptr() };
        if !hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let d = ((p.0 - at.0).powi(2) + (p.2 - at.2).powi(2)).sqrt();
        if d <= radius && (p.1 - at.1).abs() < 6.0 && best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, c as *mut _));
        }
    }
    let Some((_, c)) = best else { return 0 };
    let c = unsafe { &mut *c };
    let data = &mut c.modules.data;
    let dealt = (amount as i32).min(data.hp);
    data.hp -= dealt;
    dealt
}

/// Where a bullet appears for `at`, shot `k` of its step.
fn place(player: &PlayerIns, at: At, pos: V3, dir: V3, target: V3, k: u32) -> V3 {
    match at {
        At::Hand => (pos.0 + dir.0 * 1.0, pos.1 + 1.4, pos.2 + dir.2 * 1.0),
        At::Feet => (pos.0, pos.1 + 0.4, pos.2),
        At::Target => (target.0, target.1 + 1.0, target.2),
        At::AboveTarget(h) => (target.0, target.1 + h, target.2),
        At::Above(h) => (pos.0, pos.1 + 1.6 + h, pos.2),
        At::Ahead(d) => (pos.0 + dir.0 * d, pos.1 + 1.2, pos.2 + dir.2 * d),
        At::NearTarget(back) => {
            let (dx, dz) = (pos.0 - target.0, pos.2 - target.2);
            let l = (dx * dx + dz * dz).sqrt().max(0.01);
            (target.0 + dx / l * back, target.1 + 1.0, target.2 + dz / l * back)
        }
        At::OverTarget(back, up) => {
            let (dx, dz) = (pos.0 - target.0, pos.2 - target.2);
            let l = (dx * dx + dz * dz).sqrt().max(0.01);
            (target.0 + dx / l * back, target.1 + up, target.2 + dz / l * back)
        }
        At::Shoulder(side) => {
            // the character's right: (dir.z, 0, -dir.x); a little ahead of the chest so the arm starts at the shoulder.
            // 1.3.0 (user: the Gatling's arms "comes from above his head"): at the shoulders of his punching stance, which
            // is low and wide (about 1.1 m, not the 1.33 m of standing)
            let s = if side == 0.0 { if k % 2 == 0 { 0.2 } else { -0.2 } } else { side };
            (pos.0 + dir.2 * s + dir.0 * 0.15, pos.1 + 1.12, pos.2 - dir.0 * s + dir.2 * 0.15)
        }
        At::Line(start, step) => {
            let d = start + step * k as f32;
            let (x, z) = (pos.0 + dir.0 * d, pos.2 + dir.2 * d);
            let y = motion::ground_below(player, x, pos.1 + 1.5, z, 4.0).unwrap_or(pos.1);
            (x, y + 0.3, z)
        }
    }
}

fn direction(aim: Aim, dir: V3, from: V3, target: V3) -> V3 {
    match aim {
        Aim::Forward | Aim::Out => dir,
        Aim::Back => (-dir.0, 0.0, -dir.2),
        Aim::Down => (dir.0 * 0.25, -1.0, dir.2 * 0.25),
        Aim::AtTarget => {
            let (dx, dy, dz) = (target.0 - from.0, target.1 + 1.0 - from.1, target.2 - from.2);
            // (1.2.x: the lock-on point is already at the chest, so this aimed over the head: at 1 m a
            // Kamehameha shot almost straight up) no steeper than about 17 degrees up or down
            let h = (dx * dx + dz * dz).sqrt();
            let dy = if h > 0.3 { dy.clamp(-0.3 * h, 0.3 * h) } else { dy };
            let len = (dx * dx + dy * dy + dz * dz).sqrt();
            if len < 0.5 { dir } else { (dx / len, dy / len, dz / len) }
        }
    }
}

/// Turn the player to face `dir` (ground plane).
fn face(player: &mut PlayerIns, dir: V3) {
    // forward = (-sin t, -cos t) for q = (0, sin t/2, 0, cos t/2)
    let th = (-dir.0).atan2(-dir.2);
    let q = Quaternion(0.0, (th / 2.0).sin(), 0.0, (th / 2.0).cos());
    let phys = &mut player.chr_ins.modules.physics;
    phys.orientation = q;
    phys.interpolated_orientation = q;
}

fn set_game_speed(s: f32) {
    if let Ok(f) = unsafe { CSFlipper::instance_mut() } {
        f.game_speed = s;
    }
}

fn rotate(d: V3, ang: f32) -> V3 {
    let (s, c) = ang.sin_cos();
    (d.0 * c - d.2 * s, 0.0, d.0 * s + d.2 * c)
}

/// 1.2.0: a combo node as an ability, for the script runner (its reach, its owner's voice, its name).
fn node_ability(node: &'static combos::Node, owner: &'static str) -> &'static Ability {
    use std::sync::Mutex;
    static MADE: Mutex<Vec<(&'static str, &'static Ability)>> = Mutex::new(Vec::new());
    let mut made = MADE.lock().unwrap();
    if let Some((_, a)) = made.iter().find(|(id, _)| *id == node.id) {
        return a;
    }
    let a: &'static Ability = Box::leak(Box::new(Ability {
        id: node.id,
        name: node.name,
        owner,
        slot: Slot::Skill,
        delivery: sheets::Delivery::MultiHit,
        element: "physical",
        dmg_mult: node.dmg,
        fp_cost: 0,
        cooldown_s: 0.0,
        range_m: 6.0,
        charge: false,
        er_anim: None,
        er_bullet: 0,
        creator_pool: false,
        charge_s: 0.0,
        kind: sheets::Kind::Melee,
        hits: 1.0,
        combo: false,
    }));
    made.push((node.id, a));
    a
}
