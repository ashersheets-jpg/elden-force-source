//! JUMP FORCE's parry: press LT (skill) or guard just before a hit lands and the hit is turned away.
//! 0.11.0: only the world slows: every enemy around plays at SLOW_SPEED while the fighter keeps full
//! speed, and nothing hurts the fighter until the slow motion ends.
//! 0.10.0: LT plays Endure's braced guard for every fighter (weapon.rs), and the window is about twice
//! as long (0.45 s), so a press a beat early still counts. The game slows
//! to a crawl for about a second and a half, a burst goes off around the fighter, and the attacker's
//! guard breaks (their poise and stance are emptied and the burst staggers them).
//! A hit is "landing" when the player's HP drops (combat.rs, the same check the Awakening gauge uses);
//! the HP comes straight back and the hit reaction is replaced by a deflecting palm (anims.rs).

use std::time::{Duration, Instant};

use eldenring::cs::{PlayerIns, UserInputKey, WorldChrMan};
use eldenring::fd4::FD4PadManager;
use fromsoftware_shared::FromStatic;

use crate::ability;
use crate::log;

/// How long after pressing LT / guard a hit is parried. 1.3.0 (the user: "The parry window on hard mode should be made
/// much tighter and more precise"): on Hard, about Elden Ring's own parry
const WINDOW: Duration = Duration::from_millis(450);
const WINDOW_HARD: Duration = Duration::from_millis(150);
/// A missed parry can't be retried at once (no mashing).
const RETRY: Duration = Duration::from_millis(600);
/// Slow motion: game speed and how long it lasts (real time).
pub const SLOW_SPEED: f32 = 0.3;
const SLOW_FOR: Duration = Duration::from_millis(1500);
/// Nothing hurts the fighter right after a parry (the whole slow motion).
const SAFE_FOR: Duration = SLOW_FOR;
/// Enemies this close are slowed.
const SLOW_RADIUS: f32 = 45.0;

/// The world is slowed by a parry (bosses.rs keeps the boss slowed too).
pub static WORLD_SLOWED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Our deflecting palm (sheet: anims, "palm").
const DEFLECT_ANIM: u32 = 65012;
/// The burst: Rejection's golden shockwave pushes and staggers, Crystal Burst's blue shards flash.
const BURSTS: [i32; 2] = [crate::scripts::REJECTION, crate::scripts::CRYSTAL_BURST];

/// Lab: every hit is parried (to test the effect without timing a guard press).
#[cfg(feature = "lab")]
pub static LAB_ALWAYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub struct Parry {
    guard_was_down: bool,
    /// a parry just happened (bosses.rs opens a counter window)
    pub parried_at: Option<Instant>,
    window_until: Option<Instant>,
    next_try: Option<Instant>,
    pub slow_until: Option<Instant>,
    safe_until: Option<Instant>,
}

impl Parry {
    /// Every frame, before the hit check: a fresh guard press opens the window.
    pub fn tick(&mut self, player: &mut PlayerIns) {
        let now = Instant::now();
        // 1.2.0: LT (skill) only: LB is the left hand's punch again (Carian Combo Warriors' movesets), and
        // a punch that parried on the side made LB mashing safe
        let down = unsafe { FD4PadManager::instance() }.ok().and_then(|m| m.get_in_game_pad()).is_some_and(|p| p.poll_digital_input(UserInputKey::Skill));
        if down && !self.guard_was_down && self.next_try.is_none_or(|t| now >= t) {
            let hard = crate::ui::UI.lock().unwrap().difficulty == crate::ui::Difficulty::Hard;
            self.window_until = Some(now + if hard { WINDOW_HARD } else { WINDOW });
            self.next_try = Some(now + RETRY);
        }
        self.guard_was_down = down;
        if self.slow_until.is_some_and(|t| now >= t) {
            self.slow_until = None;
            slow_world(player, false);
        } else if self.slow_until.is_some() {
            // enemies that come close meanwhile slow down too
            slow_world(player, true);
        }
        if self.safe_until.is_some_and(|t| now >= t) {
            self.safe_until = None;
            player.chr_ins.chr_flags1c5.set_is_invincible(false);
        }
    }

    /// A hit just took `lost` HP: parry it if the window is open. True when parried (HP restored).
    pub fn on_hit(&mut self, player: &mut PlayerIns, before_hp: i32) -> bool {
        let now = Instant::now();
        #[cfg(feature = "lab")]
        if LAB_ALWAYS.load(std::sync::atomic::Ordering::Relaxed) {
            self.window_until = Some(now);
        }
        if !self.window_until.is_some_and(|t| now <= t) {
            return false;
        }
        self.window_until = None;
        self.parried_at = Some(now);
        player.chr_ins.modules.data.hp = before_hp;
        player.chr_ins.chr_flags1c5.set_is_invincible(true);
        self.safe_until = Some(now + SAFE_FOR);
        crate::anims::play(player, DEFLECT_ANIM);
        let pos = ability::position(player);
        let fwd = ability::forward(player);
        for b in BURSTS {
            let _ = ability::fire_at(player, b, (pos.0, pos.1 + 1.0, pos.2), fwd);
        }
        let broke = guard_break(player, pos, 6.0);
        slow_world(player, true);
        self.slow_until = Some(now + SLOW_FOR);
        crate::ui::parry_flash(SLOW_FOR);
        log!("parry: hit turned away, {} guard broken", if broke { "attacker's" } else { "no" });
        true
    }

    /// Leaving the world: back to normal speed.
    pub fn reset(&mut self) {
        self.slow_until = None;
        WORLD_SLOWED.store(false, std::sync::atomic::Ordering::Relaxed);
        self.window_until = None;
        self.safe_until = None;
    }

    pub fn slowed(&self) -> bool {
        self.slow_until.is_some()
    }
}

/// Every hostile near the player plays at SLOW_SPEED (`on`), or back at normal speed. Bosses get their
/// own speed back from bosses.rs.
fn slow_world(player: &PlayerIns, on: bool) {
    WORLD_SLOWED.store(on, std::sync::atomic::Ordering::Relaxed);
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return };
    let me = &player.chr_ins as *const _ as usize;
    let pos = ability::position(player);
    for e in crate::chars::near(wcm, 64).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { &mut *e.chr_ins.as_ptr() };
        if !crate::combat::hostile(c.team_type) {
            continue;
        }
        let p = c.modules.physics.position;
        if ((p.0 - pos.0).powi(2) + (p.2 - pos.2).powi(2)).sqrt() > SLOW_RADIUS {
            continue;
        }
        c.modules.behavior.animation_speed = if on { SLOW_SPEED } else { 1.0 };
    }
}

/// The nearest hostile within `radius`: poise emptied (the burst staggers them) and stance nearly
/// broken (bosses drop to a knee on the next solid hit).
fn guard_break(player: &PlayerIns, pos: ability::V3, radius: f32) -> bool {
    let Ok(wcm) = (unsafe { WorldChrMan::instance_mut() }) else { return false };
    let me = &player.chr_ins as *const _ as usize;
    for e in crate::chars::near(wcm, 32).iter() {
        if e.chr_ins.as_ptr() as usize == me {
            continue;
        }
        let c = unsafe { &mut *e.chr_ins.as_ptr() };
        if !crate::combat::hostile(c.team_type) || c.modules.data.hp <= 0 {
            continue;
        }
        let p = c.modules.physics.position;
        let (dx, dz) = (p.0 - pos.0, p.2 - pos.2);
        if (dx * dx + dz * dz).sqrt() > radius || (p.1 - pos.1).abs() > 4.0 {
            continue;
        }
        log!("parry: guard break on a chr: poise {:.0}/{:.0} stance {:.0}/{:.0}", c.modules.super_armor.sa_durability, c.modules.super_armor.sa_durability_max, c.modules.toughness.toughness, c.modules.toughness.toughness_max);
        c.modules.super_armor.sa_durability = 0.0;
        c.modules.action_flag.action_modifiers_flags.set_poise_forced_break(true);
        let t = &mut c.modules.toughness;
        t.toughness = t.toughness.min(t.toughness_max * 0.05);
        return true;
    }
    false
}
