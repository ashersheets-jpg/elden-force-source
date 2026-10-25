//! 1.3.0: Naruto's Shadow Clone Jutsu, his Awakening (the user, 2026-10-17: "Build shadow clones with a mimic tear. Replace
//! tailed beast bomb with a shadow clone that can use the L3+A/B/X abilities of Naruto at 50% damage and has 1/2 health of
//! the player").
//!
//! The clone is one of the game's own spirit summons, called through its summon manager anywhere (WorldChrMan's
//! SummonBuddyManager: the request a spirit item's use writes, as if inside a Rebirth Monument's range). Lab findings:
//! - The Mimic Tear itself can't be used: called this way its copy of the player is never filled in and stays invisible
//!   (its item use prepares it), so a human spirit (BuddyParam 25900000, trigger 259000) is dressed as the fighter instead:
//!   its equipment (CharaInitParam 23342) set to the player's worn armour rows, which carry the fighter's models while a
//!   fighter is active (outfit.rs), and bare hands.
//! - A summon away from a monument goes after the manager's disappear delay (10 s): held off while the clone lives.
//! - Its spirit look (the blue glow) is a set of effects on it: removed, it is a solid copy of the fighter.
//! It fights with its own fists (the spirit's AI) and every few seconds uses one of Naruto's three skills, the way he
//! does, at half their damage (its own unseen blow, bullet 9990733, with its own attack row); its health is half the
//! player's. It lasts CLONE_LIFE or until it falls, and goes in a puff of smoke.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use eldenring::cs::{ChrIns, ChrInsExt, FieldInsHandle, PlayerIns, SoloParamRepository, WorldChrMan};
use fromsoftware_shared::FromStatic;

use crate::ability::{self, V3};
use crate::{log, scripts, world_fx};

/// The Awakening asks for a clone (scripts Step::Clone).
pub static REQUEST: AtomicBool = AtomicBool::new(false);

/// The spirit the clone is made from: its summon effect, item, NPC row and equipment row.
const TRIGGER: i32 = 259000;
const NPC_ROW: i32 = 100000020;
const INIT_ROW: u32 = 23342;
/// The clone's unseen blow (strip_fx.json 9990733)
const CLONE_HIT: i32 = 9990733;
const CLONE_LIFE: f32 = 40.0;
/// its fists (the villains' fist row: villains.rs VILLAIN_FIST)
const FIST: i32 = 100500;
/// the effects that draw a spirit summon (lab: on the spirit, not on an ordinary enemy)
const SPIRIT_FX: [i32; 8] = [295200, 295201, 296000, 296010, 297000, 20297000, 20297820, 290050];
/// the game's own disappear delay for a summon out of a monument's range, put back when the clone goes
const GAME_DELAY: f32 = 10.0;
/// how near an enemy has to be for the clone to use a skill on it, and how often
/// (1.3.1, the user: the clone rarely used its attacks: it waited 6-9 s and looked only 14 m round itself and Naruto; its
/// dash takes it to whoever it picks)
const SKILL_RANGE: f32 = 25.0;
const SKILL_GAP: (f32, f32) = (3.0, 5.0);

// its body's animations: Naruto's own, from the event slots (sheets/anims.json; asked directly, a spirit plays only these:
// an Ash of War's id just stops it, lab 2026-10-18): the palm thrust (Rasengan), the overhand throw (Rasen Shuriken), the
// flying kick (Uzumaki Barrage)
const A_PALM: i32 = 65012;
const A_KICK: i32 = 60360;
const A_RAISE: i32 = 67040;

#[derive(Clone, Copy)]
enum S {
    Anim(i32),
    Wait(f32),
    /// to `gap` metres from the target
    Dash(f32),
    /// bullet `b` from its hand at the target (a look only: muted)
    Shot(i32),
    /// bullet `b` on the target (a look only: muted)
    AtTarget(i32),
    /// its blow on the target, `n` of them, for `share` of the skill's half damage each
    Hit(u32, f32),
    Poof,
}

/// Naruto's three skills, the way the clone uses them (the abilities' ids, for their damage)
const SKILLS: [(&str, &[S]); 3] = [
    ("naruto_rasengan", &[S::Poof, S::Dash(3.0), S::Anim(A_PALM), S::Wait(0.85), S::Shot(scripts::CCW_FULL_MOON), S::Hit(1, 1.0)]),
    ("naruto_tbb_rasen_shuriken", &[S::Poof, S::Dash(9.0), S::Anim(A_RAISE), S::Wait(0.9), S::Shot(scripts::TRIPLE_RINGS), S::Wait(0.6), S::Hit(1, 1.0)]),
    ("naruto_uzumaki_barrage", &[S::Poof, S::Dash(1.2), S::Anim(A_KICK), S::Wait(0.3), S::AtTarget(scripts::STORM_BLADE), S::Hit(1, 0.34), S::Wait(0.4), S::AtTarget(scripts::VACUUM_SLICE), S::Hit(1, 0.33), S::Wait(0.4), S::AtTarget(scripts::STORM_STOMP), S::Hit(1, 0.33)]),
];

struct Run {
    skill: usize,
    i: usize,
    t0: Instant,
    target: V3,
}

struct Clone {
    asked: Instant,
    handle: Option<FieldInsHandle>,
    until: Instant,
    next_skill: Instant,
    cleaned: Instant,
    found: Option<Instant>,
    puffed: Instant,
    half: i32,
    run: Option<Run>,
    /// its own attacks held off until then (a skill's body plays out: its AI cut the palm thrust short)
    busy_until: Option<Instant>,
}

#[derive(Default)]
pub struct Clones {
    c: Option<Clone>,
    /// the summon manager's own disappear delay still to be put back (after a load forgot the clone)
    restore_delay: bool,
    /// 1.3.1 round 2 (the user: "it was supposed to disappear after 40s but it stayed"; the log said it had gone): a clone
    /// sent away, watched until it really has
    leaving: Option<(FieldInsHandle, Instant)>,
}

/// how long a clone sent away may linger before it is ended outright
const LEAVE_WAIT: f32 = 2.0;

fn find(h: FieldInsHandle) -> Option<&'static mut ChrIns> {
    let wcm = unsafe { WorldChrMan::instance_mut() }.ok()?;
    wcm.chr_ins_by_handle_mut(&h).map(|c| unsafe { &mut *(c as *mut ChrIns) })
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

fn flat(from: V3, to: V3) -> (V3, f32) {
    let (dx, dz) = (to.0 - from.0, to.2 - from.2);
    let d = (dx * dx + dz * dz).sqrt();
    if d < 1e-3 { ((0.0, 0.0, 1.0), 0.0) } else { ((dx / d, 0.0, dz / d), d) }
}

impl Clones {
    pub fn alive(&self) -> bool {
        self.c.is_some()
    }

    /// Call a clone: the spirit dressed as the fighter, asked for beside the player.
    fn summon(&mut self, player: &mut PlayerIns) {
        if self.c.is_some() {
            self.dismiss();
        }
        // (the game keeps one spirit: a new one replaces any still leaving)
        self.leaving = None;
        let ids = unsafe { player.player_game_data.as_ref() }.equipment.chr_asm.equipment_param_ids;
        let Ok(repo) = (unsafe { SoloParamRepository::instance_mut() }) else { return };
        if let Some(c) = repo.get_mut::<eldenring::cs::CharaInitParam>(INIT_ROW) {
            c.set_equip_helm(ids[12]);
            c.set_equip_armer(ids[13]);
            c.set_equip_gaunt(ids[14]);
            c.set_equip_leg(ids[15]);
            // (1.3.0: the villains' fist, row 100500: an empty hand on an NPC hits nothing, villains.rs)
            c.set_equip_wep_right(FIST);
            c.set_equip_wep_left(FIST);
        }
        let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
        let pos = player.chr_ins.modules.physics.position;
        let m = &mut wcm.summon_buddy_manager;
        m.is_within_activation_range = true;
        m.spawn_origin = pos;
        m.request_summon_speffect_id = TRIGGER;
        m.requested_summon_goods_id = TRIGGER;
        m.buddy_disappear_delay_sec = 1.0e7;
        self.restore_delay = true;
        let me = ability::position(player);
        let f = ability::forward(player);
        world_fx::poof((me.0 + f.0 * 1.5, me.1 + 0.6, me.2 + f.2 * 1.5));
        let half = player.chr_ins.modules.data.max_hp / 2;
        let now = Instant::now();
        self.c = Some(Clone { asked: now, handle: None, until: now + Duration::from_secs_f32(CLONE_LIFE), next_skill: now + Duration::from_secs_f32(4.0), cleaned: now, found: None, puffed: now, half, run: None, busy_until: None });
        log!("clones: a shadow clone asked for ({half} HP)");
    }

    /// The clone goes, in a puff of smoke.
    fn dismiss(&mut self) {
        let Some(c) = self.c.take() else { return };
        if let Some(ch) = c.handle.and_then(find) {
            let p = pos_of(ch);
            world_fx::poof((p.0, p.1 + 0.8, p.2));
        }
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            wcm.summon_buddy_manager.disappear_requested = true;
            wcm.summon_buddy_manager.buddy_disappear_delay_sec = 0.0;
            wcm.summon_buddy_manager.is_within_activation_range = false;
        }
        self.leaving = c.handle.map(|h| (h, Instant::now()));
        self.restore_delay = true;
        log!("clones: the shadow clone is sent away");
    }

    /// (1.3.1 round 2) the clone sent away: the summon manager told again each frame that it is out of range with no delay
    /// (once, with the game's 10 s, it stayed for good); still there after LEAVE_WAIT, it is ended (its smoke already puffed)
    fn see_off(&mut self) {
        let Some((h, since)) = self.leaving else { return };
        let Some(ch) = find(h).filter(|c| c.modules.data.hp > 0) else {
            log!("clones: the shadow clone has left ({:.1} s)", since.elapsed().as_secs_f32());
            self.leaving = None;
            return;
        };
        if since.elapsed().as_secs_f32() > LEAVE_WAIT {
            ch.modules.data.hp = 0;
            log!("clones: the shadow clone lingered {LEAVE_WAIT} s: ended");
            self.leaving = None;
            return;
        }
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            wcm.summon_buddy_manager.disappear_requested = true;
            wcm.summon_buddy_manager.buddy_disappear_delay_sec = 0.0;
            wcm.summon_buddy_manager.is_within_activation_range = false;
        }
    }

    /// Leaving the world: the clone is forgotten (lab: the summon manager is torn down during a load, and writing to it then
    /// crashed the game); the next request puts the manager's delay back
    pub fn reset(&mut self) {
        if self.c.take().is_some() {
            log!("clones: forgotten (leaving the world)");
        }
        self.leaving = None;
    }

    pub fn tick(&mut self, player: &mut PlayerIns, fighter: &str) {
        if REQUEST.swap(false, Ordering::Relaxed) {
            self.summon(player);
        }
        if self.c.is_none() && self.leaving.is_some() {
            self.see_off();
            return;
        }
        let Some(c) = &mut self.c else {
            if self.restore_delay {
                self.restore_delay = false;
                if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
                    wcm.summon_buddy_manager.buddy_disappear_delay_sec = GAME_DELAY;
                }
            }
            return;
        };
        let now = Instant::now();
        if fighter != "naruto" || now >= c.until {
            self.dismiss();
            return;
        }
        // the summon manager keeps it (and keeps thinking the player is in range)
        if let Ok(wcm) = unsafe { WorldChrMan::instance_mut() } {
            wcm.summon_buddy_manager.buddy_disappear_delay_sec = 1.0e7;
            wcm.summon_buddy_manager.is_within_activation_range = true;
        }
        // smoke over its forming (the summon's own rising sparkles can't be turned off: without its forming animation it never
        // appeared): a puff beside him every half second until it is there
        if c.handle.is_none() && c.asked.elapsed() < Duration::from_millis(2600) && c.puffed.elapsed() > Duration::from_millis(450) {
            c.puffed = now;
            let me = ability::position(player);
            let f = ability::forward(player);
            let r = (f.2, 0.0, -f.0);
            world_fx::poof((me.0 + r.0 * 1.3 + f.0 * 0.5, me.1 + 0.9, me.2 + r.2 * 1.3 + f.2 * 0.5));
        }
        if c.handle.is_none() {
            // (lab: looking for it every frame read the character while the game was still building it, and crashed: it is
            // looked for once it has had time to appear, twice a second)
            if c.asked.elapsed() < Duration::from_millis(1200) || c.cleaned.elapsed() < Duration::from_millis(400) {
                return;
            }
            c.cleaned = now;
            let found = unsafe { WorldChrMan::instance() }.ok().and_then(|wcm| wcm.summon_buddy_chr_set.characters().find(|ch| ch.npc_param_id == NPC_ROW && ch.modules.data.hp > 0).map(|ch| ch.field_ins_handle));
            match found {
                Some(h) => {
                    c.handle = Some(h);
                    c.found = Some(now);
                    log!("clones: the shadow clone is here");
                }
                None if c.asked.elapsed() > Duration::from_secs(8) => {
                    log!("clones: no clone came");
                    self.c = None;
                    return;
                }
                None => return,
            }
        }
        let Some(ch) = c.handle.and_then(find) else {
            self.c = None;
            return;
        };
        if ch.modules.data.hp <= 0 {
            self.dismiss();
            return;
        }
        // (nothing touched while it forms)
        if c.found.is_some_and(|t| t.elapsed() < Duration::from_millis(1200)) {
            return;
        }
        // its fists in the hand slots it really uses (the game leaves the main right slot empty: villains.rs)
        {
            let p = unsafe { &mut *(ch as *mut ChrIns as *mut PlayerIns) };
            let pgd = unsafe { p.player_game_data.as_mut() };
            for ids in [&mut p.chr_asm.equipment_param_ids, &mut pgd.equipment.chr_asm.equipment_param_ids] {
                for k in 0..4 {
                    ids[k] = FIST;
                }
            }
        }
        #[cfg(feature = "lab")]
        {
            static SEEN: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(0);
            let ta = &ch.modules.time_act;
            let a = ta.anim_queue[(ta.read_idx as usize) % 10].anim_id;
            if SEEN.swap(a, Ordering::Relaxed) != a {
                log!("clones: playing {a}");
            }
        }
        ch.debug_flags.set_disabled_secondary_actions(c.busy_until.is_some_and(|t| Instant::now() < t));
        // half the player's health: never more
        let d = &mut ch.modules.data;
        if d.max_hp > c.half {
            d.max_hp = c.half;
        }
        if d.hp > c.half {
            d.hp = c.half;
        }
        // its spirit look off, once it has formed (lab: removing its effects the moment it appeared crashed the game), and again
        // while the game puts some of it back
        if c.found.is_some_and(|t| t.elapsed() > Duration::from_millis(1200)) && c.cleaned.elapsed().as_secs_f32() > 0.3 {
            c.cleaned = now;
            let ids: Vec<i32> = ch.special_effect.entries().map(|e| e.param_id).filter(|id| SPIRIT_FX.contains(id)).collect();
            for id in ids {
                ch.remove_speffect(id);
            }
        }
        // a skill
        let me = pos_of(ch);
        if c.run.is_none() && now >= c.next_skill {
            // an enemy near the clone, or near Naruto (it goes where he fights)
            let target = crate::combat::enemy_near(player, me, SKILL_RANGE, None).or_else(|| crate::combat::enemy_near(player, ability::position(player), SKILL_RANGE, None));
            match target {
                Some(t) => {
                    let skill = rand::random_range(0..SKILLS.len());
                    log!("clones: the clone uses {}", SKILLS[skill].0);
                    c.run = Some(Run { skill, i: 0, t0: now, target: t });
                }
                None => {
                    c.next_skill = now + Duration::from_secs_f32(1.0);
                    let p = ability::position(player);
                    log!("clones: no enemy within {SKILL_RANGE} m of the clone at ({:.1}, {:.1}, {:.1}); the player at ({:.1}, {:.1}, {:.1})", me.0, me.1, me.2, p.0, p.1, p.2);
                }
            }
        }
        let Some(r) = &mut c.run else { return };
        let (id, steps) = SKILLS[r.skill];
        let level = unsafe { player.player_game_data.as_ref() }.level;
        let half_total = crate::sheets::ability(id).map(|a| crate::damage::total(a, level) * 0.5).unwrap_or(100.0);
        let mut guard = 0;
        while r.i < steps.len() && guard < 8 {
            guard += 1;
            let (dir, _) = flat(me, r.target);
            face(ch, dir);
            let hand = (me.0 + dir.0 * 1.0, me.1 + 1.4, me.2 + dir.2 * 1.0);
            let chest = (r.target.0, r.target.1 + 1.2, r.target.2);
            let aim = {
                let v = (chest.0 - hand.0, chest.1 - hand.1, chest.2 - hand.2);
                let l = (v.0 * v.0 + v.1 * v.1 + v.2 * v.2).sqrt().max(1e-3);
                (v.0 / l, v.1 / l, v.2 / l)
            };
            let mut next = true;
            match steps[r.i] {
                S::Anim(a) => {
                    ch.modules.event.request_animation_id = a;
                    c.busy_until = Some(Instant::now() + Duration::from_millis(1200));
                    #[cfg(feature = "lab")]
                    log!("clones: body {a}");
                }
                S::Wait(s) => next = r.t0.elapsed().as_secs_f32() >= s,
                S::Dash(gap) => {
                    let (d, dist) = flat(me, r.target);
                    let k = (dist - gap).max(0.0);
                    let (x, z) = (me.0 + d.0 * k, me.2 + d.2 * k);
                    // (1.3.0: onto the floor there, so it doesn't land in the air and fall out of the skill's pose; no floor
                    // within reach (a drop): it stays where it is)
                    if let Some(g) = crate::motion::ground_below(player, x, me.1 + 3.0, z, 8.0) {
                        let ph = &mut ch.modules.physics;
                        ph.position = eldenring::position::HavokPosition(x, g + 0.05, z, ph.position.3);
                        ph.chr_proxy_pos_update_requested = true;
                    }
                }
                S::Shot(b) => {
                    crate::damage::set_bullet(b, "physical", 1);
                    let _ = ability::fire_at(player, b, hand, aim);
                }
                S::AtTarget(b) => {
                    crate::damage::set_bullet(b, "physical", 1);
                    let _ = ability::fire_at(player, b, chest, (dir.0, 0.0, dir.2));
                }
                S::Hit(n, share) => {
                    let each = ((half_total * share) / n as f32).round().clamp(1.0, 9999.0) as u16;
                    crate::damage::set_bullet(CLONE_HIT, "physical", each);
                    // (1.3.1: at the enemy's edge, going in: made at its chest it started inside the body and never touched it)
                    let e = crate::combat::enemy_near(player, r.target, 2.5, None).unwrap_or(r.target);
                    let (d2, _) = flat(me, e);
                    let at = (e.0 - d2.0 * 0.3, e.1 + 1.0, e.2 - d2.2 * 0.3);
                    for _ in 0..n {
                        let _ = ability::fire_at(player, CLONE_HIT, at, d2);
                    }
                }
                S::Poof => world_fx::poof((me.0, me.1 + 0.8, me.2)),
            }
            if !next {
                break;
            }
            r.i += 1;
            r.t0 = Instant::now();
        }
        if r.i >= steps.len() {
            c.run = None;
            let gap = rand::random_range(SKILL_GAP.0..SKILL_GAP.1);
            c.next_skill = now + Duration::from_secs_f32(gap);
        }
    }
}
